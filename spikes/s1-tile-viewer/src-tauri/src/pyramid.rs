//! In-memory image levels, pyramid construction, tile extraction and wire-format conversion.
//!
//! Spike code: pixels are RGBA interleaved in the storage type of the source (u8, u16 or f16).
//! Tiles are served as `SLOT×SLOT` blocks (tile + 1 px gutter on each side) so the GPU can filter
//! bilinearly across tile borders. Equirect tiles wrap horizontally and clamp vertically.

use half::f16;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const TILE: u32 = 512;
pub const GUTTER: u32 = 1;
pub const SLOT: u32 = TILE + 2 * GUTTER;
/// Size of the always-resident overview texture (with full mip chain).
pub const BASE_W: u32 = 2048;
pub const BASE_H: u32 = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    U8,
    U16,
    F16,
}

impl Storage {
    pub fn bytes_per_sample(self) -> usize {
        match self {
            Storage::U8 => 1,
            _ => 2,
        }
    }
}

/// Pixel format sent to the frontend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wire {
    U8,
    F16,
}

impl Wire {
    pub fn parse(s: &str) -> Option<Wire> {
        match s {
            "u8" => Some(Wire::U8),
            "f16" => Some(Wire::F16),
            _ => None,
        }
    }
}

pub trait Sample: Copy + Default + Send + Sync + bytemuck::Pod + 'static {
    fn to_f(self) -> f32;
    fn from_f(v: f32) -> Self;
    /// Converts a run of samples into wire bytes (`dst.len() == src.len() * wire bytes per sample`).
    fn write_wire(src: &[Self], dst: &mut [u8], wire: Wire);
}

impl Wire {
    pub fn bytes_per_sample(self) -> usize {
        match self {
            Wire::U8 => 1,
            Wire::F16 => 2,
        }
    }
}

#[inline]
fn write_f16(dst: &mut [u8], vals: impl Iterator<Item = f16>) {
    for (d, v) in dst.chunks_exact_mut(2).zip(vals) {
        d.copy_from_slice(&v.to_le_bytes());
    }
}

impl Sample for u8 {
    #[inline(always)]
    fn to_f(self) -> f32 {
        self as f32
    }
    #[inline(always)]
    fn from_f(v: f32) -> Self {
        (v + 0.5).clamp(0.0, 255.0) as u8
    }
    fn write_wire(src: &[Self], dst: &mut [u8], wire: Wire) {
        match wire {
            Wire::U8 => dst.copy_from_slice(src),
            Wire::F16 => {
                let lut = u8_to_f16_lut();
                write_f16(dst, src.iter().map(|&v| lut[v as usize]));
            }
        }
    }
}

impl Sample for u16 {
    #[inline(always)]
    fn to_f(self) -> f32 {
        self as f32
    }
    #[inline(always)]
    fn from_f(v: f32) -> Self {
        (v + 0.5).clamp(0.0, 65535.0) as u16
    }
    fn write_wire(src: &[Self], dst: &mut [u8], wire: Wire) {
        match wire {
            Wire::U8 => {
                for (d, &v) in dst.iter_mut().zip(src) {
                    *d = ((v as u32 * 255 + 32767) / 65535) as u8;
                }
            }
            Wire::F16 => {
                let lut = u16_to_f16_lut();
                write_f16(dst, src.iter().map(|&v| lut[v as usize]));
            }
        }
    }
}

impl Sample for f16 {
    #[inline(always)]
    fn to_f(self) -> f32 {
        self.to_f32()
    }
    #[inline(always)]
    fn from_f(v: f32) -> Self {
        f16::from_f32(v)
    }
    fn write_wire(src: &[Self], dst: &mut [u8], wire: Wire) {
        match wire {
            Wire::U8 => {
                for (d, v) in dst.iter_mut().zip(src) {
                    *d = (v.to_f32().clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
            Wire::F16 => dst.copy_from_slice(bytemuck::cast_slice(src)),
        }
    }
}

pub enum Pixels {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F16(Vec<f16>),
}

/// Runs `$body` with `$v` bound to the typed sample slice of a `Pixels`.
#[macro_export]
macro_rules! with_pixels {
    ($px:expr, $v:ident => $body:expr) => {
        match $px {
            $crate::pyramid::Pixels::U8($v) => $body,
            $crate::pyramid::Pixels::U16($v) => $body,
            $crate::pyramid::Pixels::F16($v) => $body,
        }
    };
}

impl Pixels {
    pub fn storage(&self) -> Storage {
        match self {
            Pixels::U8(_) => Storage::U8,
            Pixels::U16(_) => Storage::U16,
            Pixels::F16(_) => Storage::F16,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        with_pixels!(self, v => bytemuck::cast_slice(v))
    }
    pub fn from_bytes(storage: Storage, bytes: &[u8]) -> Pixels {
        match storage {
            Storage::U8 => Pixels::U8(bytes.to_vec()),
            Storage::U16 => Pixels::U16(bytemuck::pod_collect_to_vec(bytes)),
            Storage::F16 => Pixels::F16(bytemuck::pod_collect_to_vec(bytes)),
        }
    }
}

pub struct Level {
    pub width: u32,
    pub height: u32,
    pub px: Pixels,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LevelInfo {
    pub w: u32,
    pub h: u32,
    pub tiles_x: u32,
    pub tiles_y: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CubeLevelInfo {
    /// Face edge length in pixels.
    pub n: u32,
    pub tiles: u32,
}

pub fn level_infos(width: u32, height: u32) -> Vec<LevelInfo> {
    // Build levels until the width fits into 2×BASE_W; coarser views use the base texture.
    let mut out = vec![];
    let (mut w, mut h) = (width, height);
    loop {
        out.push(LevelInfo {
            w,
            h,
            tiles_x: w.div_ceil(TILE),
            tiles_y: h.div_ceil(TILE),
        });
        if w <= 2 * BASE_W {
            break;
        }
        w = w.div_ceil(2);
        h = h.div_ceil(2);
    }
    out
}

pub fn cube_level_infos(levels: &[LevelInfo]) -> Vec<CubeLevelInfo> {
    // Match the equator resolution of each equirect level: W/(2π) px per radian = N/2.
    levels
        .iter()
        .map(|l| {
            let n = (l.w as f64 / std::f64::consts::PI).ceil() as u32;
            CubeLevelInfo {
                n,
                tiles: n.div_ceil(TILE),
            }
        })
        .collect()
}

/// Area weights of the source samples covered by output sample `i` when `n_src` samples are mapped
/// onto `n_dst` (≤ n_src): output i covers [i·r, (i+1)·r) with r = n_src / n_dst. Keeps every level
/// geometrically aligned with level 0 even for odd sizes (pixel centres at (i + 0.5) / n everywhere).
fn weights_at(n_src: usize, n_dst: usize, i: usize, out: &mut Vec<(usize, f32)>) {
    out.clear();
    let r = n_src as f64 / n_dst as f64;
    let (s0, s1) = (i as f64 * r, ((i + 1) as f64 * r).min(n_src as f64));
    let mut k = s0.floor() as usize;
    while (k as f64) < s1 && k < n_src {
        let ov = (s1.min(k as f64 + 1.0) - s0.max(k as f64)) / (s1 - s0);
        if ov > 1e-9 {
            out.push((k, ov as f32));
        }
        k += 1;
    }
}

/// Area-weighted resampling of `src` into the region [x0, x1) × [y0, y1) of `dst`.
pub fn resample_into(src: &Level, dst: &mut Level, x0: u32, x1: u32, y0: u32, y1: u32) {
    let (sw, sh, dw, dh) = (src.width as usize, src.height as usize, dst.width as usize, dst.height as usize);
    let (x0, x1, y0, y1) = (x0 as usize, (x1 as usize).min(dw), y0 as usize, (y1 as usize).min(dh));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let mut tmp = vec![];
    let wx: Vec<Vec<(usize, f32)>> = (x0..x1)
        .map(|x| {
            weights_at(sw, dw, x, &mut tmp);
            tmp.clone()
        })
        .collect();
    fn go<T: Sample>(s: &[T], d: &mut [T], sw: usize, sh: usize, dw: usize, dh: usize, x0: usize, y0: usize, y1: usize, wx: &[Vec<(usize, f32)>]) {
        d[y0 * dw * 4..y1 * dw * 4].par_chunks_mut(dw * 4).enumerate().for_each(|(j, row)| {
            let mut wy = vec![];
            weights_at(sh, dh, y0 + j, &mut wy);
            for (i, wxi) in wx.iter().enumerate() {
                let mut acc = [0f32; 4];
                for &(yy, fy) in &wy {
                    let srow = &s[yy * sw * 4..][..sw * 4];
                    for &(xx, fx) in wxi {
                        let f = fx * fy;
                        for c in 0..4 {
                            acc[c] += srow[xx * 4 + c].to_f() * f;
                        }
                    }
                }
                let o = (x0 + i) * 4;
                for c in 0..4 {
                    row[o + c] = T::from_f(acc[c]);
                }
            }
        });
    }
    match (&src.px, &mut dst.px) {
        (Pixels::U8(s), Pixels::U8(d)) => go(s, d, sw, sh, dw, dh, x0, y0, y1, &wx),
        (Pixels::U16(s), Pixels::U16(d)) => go(s, d, sw, sh, dw, dh, x0, y0, y1, &wx),
        (Pixels::F16(s), Pixels::F16(d)) => go(s, d, sw, sh, dw, dh, x0, y0, y1, &wx),
        _ => unreachable!("storage types differ"),
    }
}

fn empty_like(src: &Level, nw: u32, nh: u32) -> Level {
    let n = (nw * nh * 4) as usize;
    let px = match &src.px {
        Pixels::U8(_) => Pixels::U8(vec![0; n]),
        Pixels::U16(_) => Pixels::U16(vec![0; n]),
        Pixels::F16(_) => Pixels::F16(vec![f16::ZERO; n]),
    };
    Level { width: nw, height: nh, px }
}

/// Next pyramid level (size ⌈w/2⌉ × ⌈h/2⌉). Even sizes: plain 2×2 average (fast path); odd sizes:
/// area-weighted, so the level stays aligned with level 0.
pub fn downsample(src: &Level) -> Level {
    let (w, h) = (src.width as usize, src.height as usize);
    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
    if w % 2 == 1 || h % 2 == 1 {
        let mut d = empty_like(src, nw as u32, nh as u32);
        resample_into(src, &mut d, 0, nw as u32, 0, nh as u32);
        return d;
    }
    fn go<T: Sample>(s: &[T], w: usize, nw: usize, nh: usize) -> Vec<T> {
        let mut d = vec![T::default(); nw * nh * 4];
        d.par_chunks_mut(nw * 4).enumerate().for_each(|(y, row)| {
            let r0 = &s[(2 * y) * w * 4..][..w * 4];
            let r1 = &s[(2 * y + 1) * w * 4..][..w * 4];
            for x in 0..nw {
                let (a, b) = (2 * x * 4, (2 * x + 1) * 4);
                for c in 0..4 {
                    let v = r0[a + c].to_f() + r0[b + c].to_f() + r1[a + c].to_f() + r1[b + c].to_f();
                    row[x * 4 + c] = T::from_f(v * 0.25);
                }
            }
        });
        d
    }
    let px = match &src.px {
        Pixels::U8(v) => Pixels::U8(go(v, w, nw, nh)),
        Pixels::U16(v) => Pixels::U16(go(v, w, nw, nh)),
        Pixels::F16(v) => Pixels::F16(go(v, w, nw, nh)),
    };
    Level { width: nw as u32, height: nh as u32, px }
}

/// Area-average resample to an arbitrary (smaller) size (base texture).
pub fn resample(src: &Level, nw: u32, nh: u32) -> Level {
    let mut d = empty_like(src, nw, nh);
    resample_into(src, &mut d, 0, nw, 0, nh);
    d
}

/// Base texture with its full mip chain (BASE_W×BASE_H … 1×1), raw storage bytes concatenated.
pub struct Base {
    pub storage: Storage,
    pub mips: Vec<(u32, u32)>,
    pub raw: Vec<u8>,
}

impl Base {
    pub fn build(from: &Level) -> Base {
        let mut cur = resample(from, BASE_W, BASE_H);
        let storage = cur.px.storage();
        let mut mips = vec![];
        let mut raw = vec![];
        loop {
            mips.push((cur.width, cur.height));
            raw.extend_from_slice(cur.px.as_bytes());
            if cur.width == 1 && cur.height == 1 {
                break;
            }
            let next = downsample(&cur);
            cur = next;
        }
        Base { storage, mips, raw }
    }
}

pub struct Pyramid {
    /// S2: levels are mutable (strokes are rasterised into them); readers take short read locks.
    pub levels: Vec<parking_lot::RwLock<Level>>,
}

impl Pyramid {
    pub fn build(level0: Level) -> Pyramid {
        let infos = level_infos(level0.width, level0.height);
        let mut levels = vec![level0];
        while levels.len() < infos.len() {
            let next = downsample(levels.last().unwrap());
            levels.push(next);
        }
        Pyramid {
            levels: levels.into_iter().map(parking_lot::RwLock::new).collect(),
        }
    }

    pub fn storage(&self) -> Storage {
        self.levels[0].read().px.storage()
    }

    /// Equirect tile incl. gutter, raw storage bytes (SLOT×SLOT×4 samples).
    pub fn equirect_tile_raw(&self, level: usize, tx: u32, ty: u32) -> Option<Vec<u8>> {
        let l = self.levels.get(level)?.read();
        let (w, h) = (l.width as i64, l.height as i64);
        let x0 = tx as i64 * TILE as i64 - GUTTER as i64;
        let y0 = ty as i64 * TILE as i64 - GUTTER as i64;
        let xs: Vec<usize> = (0..SLOT as i64)
            .map(|i| (x0 + i).rem_euclid(w) as usize)
            .collect();
        fn go<T: Sample>(s: &[T], w: usize, h: i64, y0: i64, xs: &[usize]) -> Vec<u8> {
            let n = SLOT as usize;
            let mut out = vec![T::default(); n * n * 4];
            for j in 0..n {
                let sy = (y0 + j as i64).clamp(0, h - 1) as usize;
                let srow = &s[sy * w * 4..][..w * 4];
                let drow = &mut out[j * n * 4..][..n * 4];
                // Copy contiguous runs (usually the whole row except where it wraps).
                let mut i = 0;
                while i < n {
                    let start = xs[i];
                    let mut len = 1;
                    while i + len < n && xs[i + len] == start + len {
                        len += 1;
                    }
                    drow[i * 4..(i + len) * 4].copy_from_slice(&srow[start * 4..(start + len) * 4]);
                    i += len;
                }
            }
            bytemuck::cast_slice(&out).to_vec()
        }
        Some(with_pixels!(&l.px, v => go(v, w as usize, h, y0, &xs)))
    }

    /// Equirect tile incl. gutter, written in one pass directly in the wire format (one allocation,
    /// no intermediate copies).
    pub fn equirect_tile_wire(
        &self,
        level: usize,
        tx: u32,
        ty: u32,
        wire: Wire,
    ) -> Option<Vec<u8>> {
        let l = self.levels.get(level)?.read();
        let (w, h) = (l.width as i64, l.height as i64);
        let x0 = tx as i64 * TILE as i64 - GUTTER as i64;
        let y0 = ty as i64 * TILE as i64 - GUTTER as i64;
        // contiguous source runs of one row: (dst start px, src start px, len)
        let n = SLOT as usize;
        let mut runs = vec![];
        let mut i = 0usize;
        while i < n {
            let start = (x0 + i as i64).rem_euclid(w) as usize;
            let len = (n - i).min(w as usize - start);
            runs.push((i, start, len));
            i += len;
        }
        let bps = wire.bytes_per_sample();
        let mut out = vec![0u8; n * n * 4 * bps];
        fn go<T: Sample>(
            s: &[T],
            w: usize,
            h: i64,
            y0: i64,
            runs: &[(usize, usize, usize)],
            out: &mut [u8],
            wire: Wire,
        ) {
            let n = SLOT as usize;
            let bps = wire.bytes_per_sample();
            for (j, drow) in out.chunks_exact_mut(n * 4 * bps).enumerate() {
                let sy = (y0 + j as i64).clamp(0, h - 1) as usize;
                let srow = &s[sy * w * 4..][..w * 4];
                for &(d, src, len) in runs {
                    T::write_wire(
                        &srow[src * 4..(src + len) * 4],
                        &mut drow[d * 4 * bps..(d + len) * 4 * bps],
                        wire,
                    );
                }
            }
        }
        with_pixels!(&l.px, v => go(v, w as usize, h, y0, &runs, &mut out, wire));
        Some(out)
    }

    /// Cube-face tile incl. gutter, generated on demand from equirect level `level` (bilinear).
    pub fn cube_tile_raw(
        &self,
        face: u32,
        level: usize,
        n: u32,
        tx: u32,
        ty: u32,
    ) -> Option<Vec<u8>> {
        let l = self.levels.get(level)?.read();
        let (w, h) = (l.width as usize, l.height as usize);
        fn go<T: Sample>(
            s: &[T],
            w: usize,
            h: usize,
            face: u32,
            n: u32,
            tx: u32,
            ty: u32,
        ) -> Vec<u8> {
            let ns = SLOT as usize;
            let mut out = vec![T::default(); ns * ns * 4];
            let inv_n = 2.0 / n as f32;
            for j in 0..ns {
                let py = (ty * TILE) as f32 + j as f32 - GUTTER as f32 + 0.5;
                let t = py * inv_n - 1.0;
                for i in 0..ns {
                    let px = (tx * TILE) as f32 + i as f32 - GUTTER as f32 + 0.5;
                    let sc = px * inv_n - 1.0;
                    let d = cube_dir(face, sc, t);
                    let (u, v) = dir_to_uv(d);
                    let c = bilinear(s, w, h, u * w as f32, v * h as f32);
                    let o = (j * ns + i) * 4;
                    for k in 0..4 {
                        out[o + k] = T::from_f(c[k]);
                    }
                }
            }
            bytemuck::cast_slice(&out).to_vec()
        }
        Some(with_pixels!(&l.px, v => go(v, w, h, face, n, tx, ty)))
    }
}

#[inline]
fn bilinear<T: Sample>(s: &[T], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    let fx = x - 0.5;
    let fy = (y - 0.5).clamp(0.0, h as f32 - 1.0);
    let x0f = fx.floor();
    let y0f = fy.floor();
    let ax = fx - x0f;
    let ay = fy - y0f;
    let x0 = (x0f as i64).rem_euclid(w as i64) as usize;
    let x1 = (x0 + 1) % w;
    let y0 = y0f as usize;
    let y1 = (y0 + 1).min(h - 1);
    let mut c = [0f32; 4];
    let p00 = (y0 * w + x0) * 4;
    let p10 = (y0 * w + x1) * 4;
    let p01 = (y1 * w + x0) * 4;
    let p11 = (y1 * w + x1) * 4;
    for k in 0..4 {
        let top = s[p00 + k].to_f() * (1.0 - ax) + s[p10 + k].to_f() * ax;
        let bot = s[p01 + k].to_f() * (1.0 - ax) + s[p11 + k].to_f() * ax;
        c[k] = top * (1.0 - ay) + bot * ay;
    }
    c
}

/// Same convention as the mockups and the shader: forward = -z, up = +y.
#[inline]
pub fn dir_to_uv(d: [f32; 3]) -> (f32, f32) {
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let u = 0.5 + d[0].atan2(-d[2]) / std::f32::consts::TAU;
    let v = 0.5 - (d[1] / len).clamp(-1.0, 1.0).asin() / std::f32::consts::PI;
    (u, v)
}

/// Cube face basis (normal, right, down). Must match `cubeBasis` in the shader and tiles.ts.
pub fn cube_basis(face: u32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    match face {
        0 => ([1., 0., 0.], [0., 0., 1.], [0., -1., 0.]), // +x
        1 => ([-1., 0., 0.], [0., 0., -1.], [0., -1., 0.]), // -x
        2 => ([0., 1., 0.], [1., 0., 0.], [0., 0., 1.]),  // +y (zenith)
        3 => ([0., -1., 0.], [1., 0., 0.], [0., 0., -1.]), // -y (nadir)
        4 => ([0., 0., 1.], [-1., 0., 0.], [0., -1., 0.]), // +z
        _ => ([0., 0., -1.], [1., 0., 0.], [0., -1., 0.]), // -z (forward)
    }
}

#[inline]
fn cube_dir(face: u32, s: f32, t: f32) -> [f32; 3] {
    let (n, r, d) = cube_basis(face);
    [
        n[0] + s * r[0] + t * d[0],
        n[1] + s * r[1] + t * d[1],
        n[2] + s * r[2] + t * d[2],
    ]
}

fn u8_to_f16_lut() -> &'static [f16; 256] {
    static L: OnceLock<[f16; 256]> = OnceLock::new();
    L.get_or_init(|| std::array::from_fn(|i| f16::from_f32(i as f32 / 255.0)))
}

fn u16_to_f16_lut() -> &'static [f16] {
    static L: OnceLock<Vec<f16>> = OnceLock::new();
    L.get_or_init(|| {
        (0..65536u32)
            .map(|i| f16::from_f32(i as f32 / 65535.0))
            .collect()
    })
}

/// Converts raw storage samples to the wire format requested by the frontend.
pub fn to_wire(storage: Storage, raw: Vec<u8>, wire: Wire) -> Vec<u8> {
    if matches!(
        (storage, wire),
        (Storage::U8, Wire::U8) | (Storage::F16, Wire::F16)
    ) {
        return raw;
    }
    fn go<T: Sample>(raw: &[u8], wire: Wire) -> Vec<u8> {
        // Avoid a copy when the buffer is suitably aligned (it practically always is).
        let owned;
        let src: &[T] = match bytemuck::try_cast_slice(raw) {
            Ok(s) => s,
            Err(_) => {
                owned = bytemuck::pod_collect_to_vec::<u8, T>(raw);
                &owned
            }
        };
        let mut out = vec![0u8; src.len() * wire.bytes_per_sample()];
        T::write_wire(src, &mut out, wire);
        out
    }
    match storage {
        Storage::U8 => go::<u8>(&raw, wire),
        Storage::U16 => go::<u16>(&raw, wire),
        Storage::F16 => go::<f16>(&raw, wire),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Level {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let p = ((y * w + x) * 4) as usize;
                v[p] = (x % 256) as u8;
                v[p + 1] = (y % 256) as u8;
                v[p + 3] = 255;
            }
        }
        Level {
            width: w,
            height: h,
            px: Pixels::U8(v),
        }
    }

    #[test]
    fn levels_stop_at_base_size() {
        let l = level_infos(23800, 11900);
        assert_eq!(
            l.iter().map(|l| l.w).collect::<Vec<_>>(),
            vec![23800, 11900, 5950, 2975]
        );
        assert_eq!((l[0].tiles_x, l[0].tiles_y), (47, 24));
    }

    #[test]
    fn equirect_tile_wraps_x_and_clamps_y() {
        let p = Pyramid::build(gradient(1000, 500));
        let raw = p.equirect_tile_raw(0, 0, 0).unwrap();
        let n = SLOT as usize;
        // left gutter wraps to the last column, top gutter clamps to row 0
        assert_eq!(raw[0], (999 % 256) as u8);
        assert_eq!(raw[1], 0);
        // first interior pixel is (0,0)
        let p11 = (n + 1) * 4;
        assert_eq!((raw[p11], raw[p11 + 1]), (0, 0));
    }

    #[test]
    fn cube_forward_face_centre_hits_equirect_centre() {
        let (u, v) = dir_to_uv(cube_dir(5, 0.0, 0.0));
        assert!((u - 0.5).abs() < 1e-6 && (v - 0.5).abs() < 1e-6);
        let (_, v) = dir_to_uv(cube_dir(3, 0.0, 0.0));
        assert!((v - 1.0).abs() < 1e-6);
    }

    #[test]
    fn single_pass_wire_tile_matches_raw_plus_conversion() {
        let mut v = vec![0u16; 1000 * 500 * 4];
        for (i, x) in v.iter_mut().enumerate() {
            *x = (i * 37 % 65536) as u16;
        }
        let p = Pyramid::build(Level {
            width: 1000,
            height: 500,
            px: Pixels::U16(v),
        });
        for (tx, ty) in [(0, 0), (1, 0), (1, 1)] {
            for wire in [Wire::U8, Wire::F16] {
                let a = p.equirect_tile_wire(0, tx, ty, wire).unwrap();
                let b = to_wire(Storage::U16, p.equirect_tile_raw(0, tx, ty).unwrap(), wire);
                assert_eq!(a, b, "tile {tx},{ty} {wire:?}");
            }
        }
    }

    #[test]
    fn odd_sized_levels_stay_aligned_with_level_0() {
        // a vertical gradient: value = v coordinate. Every level must reproduce v at its pixel centres.
        let (w, h) = (64u32, 47u32);
        let mut v = vec![0u16; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                v[((y * w + x) * 4) as usize] = (((y as f64 + 0.5) / h as f64) * 60000.0) as u16;
            }
        }
        let p = Pyramid::build(Level { width: w, height: h, px: Pixels::U16(v) });
        let l1 = downsample(&p.levels[0].read());
        let l2 = downsample(&l1);
        for l in [&l1, &l2] {
            let Pixels::U16(px) = &l.px else { panic!() };
            for y in 1..l.height - 1 {
                let expect = (y as f64 + 0.5) / l.height as f64 * 60000.0;
                let got = px[((y * l.width) * 4) as usize] as f64;
                assert!((got - expect).abs() < 60000.0 / h as f64 * 0.05, "h={} y={y}: {got} vs {expect}", l.height);
            }
        }
    }

    #[test]
    fn wire_conversion_u16_to_u8() {
        let raw = bytemuck::cast_slice(&[0u16, 65535, 32896]).to_vec();
        assert_eq!(to_wire(Storage::U16, raw, Wire::U8), vec![0, 255, 128]);
    }
}
