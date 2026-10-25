//! Spike S3 – inpainting a region of the panorama (typically the nadir) in a flat gnomonic view.
//!
//! 1. Render a gnomonic (rectilinear) view around the mask with full level-0 detail.
//! 2. Build the hole mask (spherical cap and/or alpha hole) and a feathered blend weight.
//! 3. Fill the hole (AI model, PatchMatch, or both) – see `ai.rs` and `patchmatch`.
//! 4. Reproject into level 0: only where the blend weight > 0; everything else stays bit-exact.

use crate::edit::{angle, dir, V3};
use crate::pyramid::{Level, Pixels, Sample};
use half::f16;
use rayon::prelude::*;

/// RGBA f32 image, row-major.
#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
}

impl Img {
    pub fn new(w: usize, h: usize) -> Img {
        Img { w, h, px: vec![[0.0; 4]; w * h] }
    }
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> [f32; 4] {
        self.px[y * self.w + x]
    }
    /// Bilinear sample at pixel coordinates (clamped).
    pub fn bilinear(&self, x: f32, y: f32) -> [f32; 4] {
        let fx = (x - 0.5).clamp(0.0, self.w as f32 - 1.0);
        let fy = (y - 0.5).clamp(0.0, self.h as f32 - 1.0);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let (a, b, c, d) = (self.at(x0, y0), self.at(x1, y0), self.at(x0, y1), self.at(x1, y1));
        std::array::from_fn(|k| (a[k] * (1.0 - ax) + b[k] * ax) * (1.0 - ay) + (c[k] * (1.0 - ax) + d[k] * ax) * ay)
    }
    /// Area-average resize (downscale) or bilinear (upscale).
    pub fn resize(&self, nw: usize, nh: usize) -> Img {
        let mut out = Img::new(nw, nh);
        let (sx, sy) = (self.w as f32 / nw as f32, self.h as f32 / nh as f32);
        out.px.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
            for (x, o) in row.iter_mut().enumerate() {
                if sx <= 1.0 && sy <= 1.0 {
                    *o = self.bilinear((x as f32 + 0.5) * sx, (y as f32 + 0.5) * sy);
                    continue;
                }
                let (x0, x1) = ((x as f32 * sx) as usize, (((x + 1) as f32 * sx).ceil() as usize).min(self.w));
                let (y0, y1) = ((y as f32 * sy) as usize, (((y + 1) as f32 * sy).ceil() as usize).min(self.h));
                let mut acc = [0f32; 4];
                for yy in y0..y1.max(y0 + 1) {
                    for xx in x0..x1.max(x0 + 1) {
                        let p = self.at(xx.min(self.w - 1), yy.min(self.h - 1));
                        for k in 0..4 {
                            acc[k] += p[k];
                        }
                    }
                }
                let n = ((y1.max(y0 + 1) - y0) * (x1.max(x0 + 1) - x0)) as f32;
                *o = acc.map(|v| v / n);
            }
        });
        out
    }
}

/// A gnomonic view: centre direction, orthonormal right/up, half field of view, size.
#[derive(Clone, Copy, Debug)]
pub struct Gnomonic {
    pub f: V3,
    pub r: V3,
    pub u: V3,
    pub tan_half: f64,
    pub n: usize,
}

impl Gnomonic {
    pub fn new(center: V3, half_fov_deg: f64, n: usize) -> Gnomonic {
        let f = normalize(center);
        let up = if f[1].abs() > 0.9 { [0.0, 0.0, -1.0] } else { [0.0, 1.0, 0.0] };
        let r = normalize(cross(f, up));
        let u = cross(r, f);
        Gnomonic { f, r, u, tan_half: half_fov_deg.to_radians().tan(), n }
    }

    /// Direction through the centre of pixel (i, j).
    pub fn dir(&self, i: f64, j: f64) -> V3 {
        let n = self.n as f64;
        let x = ((i + 0.5) / n * 2.0 - 1.0) * self.tan_half;
        let y = (1.0 - (j + 0.5) / n * 2.0) * self.tan_half;
        normalize([0, 1, 2].map(|k| self.f[k] + self.r[k] * x + self.u[k] * y))
    }

    /// Pixel coordinates (continuous, pixel centres at +0.5) of direction `d`, if in front.
    pub fn project(&self, d: V3) -> Option<(f64, f64)> {
        let z = dot(d, self.f);
        if z <= 1e-6 {
            return None;
        }
        let (x, y) = (dot(d, self.r) / z, dot(d, self.u) / z);
        let n = self.n as f64;
        Some(((x / self.tan_half + 1.0) / 2.0 * n, (1.0 - y / self.tan_half) / 2.0 * n))
    }
}

#[inline]
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
fn normalize(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Equirect pixel coordinates of a direction (same convention as `pyramid::dir_to_uv`).
#[inline]
fn dir_to_px(d: V3, w: usize, h: usize) -> (f64, f64) {
    let u = 0.5 + d[0].atan2(-d[2]) / std::f64::consts::TAU;
    let v = 0.5 - d[1].clamp(-1.0, 1.0).asin() / std::f64::consts::PI;
    (u * w as f64, v * h as f64)
}

/// Normalised sample value: u8/u16 → 0..1, f16 as is (linear HDR).
fn level_norm(l: &Level) -> f32 {
    match l.px {
        Pixels::U8(_) => 1.0 / 255.0,
        Pixels::U16(_) => 1.0 / 65535.0,
        Pixels::F16(_) => 1.0,
    }
}

/// Renders the gnomonic view from level 0 (Catmull-Rom, x wraps), values normalised (see `level_norm`).
pub fn render(l0: &Level, g: &Gnomonic) -> Img {
    let (w, h) = (l0.width as usize, l0.height as usize);
    let k = level_norm(l0);
    fn go<T: Sample>(s: &[T], w: usize, h: usize, g: &Gnomonic, k: f32) -> Img {
        let mut out = Img::new(g.n, g.n);
        out.px.par_chunks_mut(g.n).enumerate().for_each(|(j, row)| {
            for (i, o) in row.iter_mut().enumerate() {
                let (x, y) = dir_to_px(g.dir(i as f64, j as f64), w, h);
                let c = crate::edit::catmull_rom(s, w, h, x, y);
                *o = c.map(|v| (v as f32 * k).max(0.0));
            }
        });
        out
    }
    crate::with_pixels!(&l0.px, v => go(v, w, h, g, k))
}

/// Hole mask in the gnomonic view: inside the cap (centre `c`, radius `cap_deg`) and/or where the
/// source alpha < 0.5 (`alpha_hole`), grown by `grow` pixels.
pub fn hole_mask(view: &Img, g: &Gnomonic, c: V3, cap_deg: Option<f64>, alpha_hole: bool, grow: usize) -> Vec<bool> {
    let n = g.n;
    let cap = cap_deg.map(f64::to_radians);
    let mut m: Vec<bool> = (0..n * n)
        .into_par_iter()
        .map(|p| {
            let (i, j) = (p % n, p / n);
            let in_cap = cap.is_some_and(|r| angle(g.dir(i as f64, j as f64), c) < r);
            in_cap || (alpha_hole && view.px[p][3] < 0.5)
        })
        .collect();
    if grow > 0 {
        let d = distance_outside(&m, n, n);
        m = d.iter().map(|&v| v <= grow as f32).collect();
    }
    m
}

/// Chamfer distance (pixels) from every pixel to the nearest hole pixel (0 inside the hole).
pub fn distance_outside(hole: &[bool], w: usize, h: usize) -> Vec<f32> {
    let inf = 1e9f32;
    let mut d: Vec<f32> = hole.iter().map(|&b| if b { 0.0 } else { inf }).collect();
    let (a, b) = (1.0f32, std::f32::consts::SQRT_2);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 { v = v.min(d[i - 1] + a); }
            if y > 0 {
                v = v.min(d[i - w] + a);
                if x > 0 { v = v.min(d[i - w - 1] + b); }
                if x + 1 < w { v = v.min(d[i - w + 1] + b); }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w { v = v.min(d[i + 1] + a); }
            if y + 1 < h {
                v = v.min(d[i + w] + a);
                if x + 1 < w { v = v.min(d[i + w + 1] + b); }
                if x > 0 { v = v.min(d[i + w - 1] + b); }
            }
            d[i] = v;
        }
    }
    d
}

/// Blend weight: 1 in the hole, smooth falloff to 0 over `feather` pixels outside it.
pub fn feather_weight(hole: &[bool], w: usize, h: usize, feather: f32) -> Vec<f32> {
    distance_outside(hole, w, h)
        .iter()
        .map(|&d| {
            if d <= 0.0 {
                1.0
            } else if d >= feather {
                0.0
            } else {
                let t = d / feather;
                1.0 - t * t * (3.0 - 2.0 * t)
            }
        })
        .collect()
}

/// Tone mapping for linear HDR data so that 8-bit models can work on it, and its exact inverse.
/// Exposure is chosen from the known context (median luminance → 0.18).
#[derive(Clone, Copy, Debug)]
pub struct ToneMap {
    pub hdr: bool,
    pub exposure: f32,
}

impl ToneMap {
    pub fn for_view(view: &Img, hole: &[bool], hdr: bool) -> ToneMap {
        if !hdr {
            return ToneMap { hdr, exposure: 1.0 };
        }
        let mut lum: Vec<f32> = view.px.iter().zip(hole).filter(|(_, h)| !**h).map(|(p, _)| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).collect();
        lum.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = lum.get(lum.len() / 2).copied().unwrap_or(0.18).max(1e-4);
        ToneMap { hdr, exposure: 0.18 / med }
    }
    /// linear → display 0..1 (Reinhard + sRGB-like gamma); identity for SDR data.
    #[inline]
    pub fn fwd(&self, v: f32) -> f32 {
        if !self.hdr {
            return v.clamp(0.0, 1.0);
        }
        let x = (v * self.exposure).max(0.0);
        (x / (1.0 + x)).powf(1.0 / 2.2)
    }
    #[inline]
    pub fn inv(&self, y: f32) -> f32 {
        if !self.hdr {
            return y;
        }
        let t = y.clamp(0.0, 0.999).powf(2.2);
        t / (1.0 - t) / self.exposure
    }
}

/// Reprojects the filled gnomonic image into level 0, blending with `weight`; pixels with weight 0
/// are not touched (bit-exact). Values in `filled` are in the tone-mapped domain of `tm`.
/// Returns the number of changed pixels.
pub fn reproject(l0: &mut Level, g: &Gnomonic, filled: &Img, weight: &[f32], tm: ToneMap) -> usize {
    let (w, h) = (l0.width as usize, l0.height as usize);
    let k = level_norm(l0);
    let n = g.n;
    let wimg = Img { w: n, h: n, px: weight.iter().map(|&v| [v, 0.0, 0.0, 0.0]).collect() };
    // rows that can be affected: those whose directions lie within the view cone
    let half = g.tan_half.atan() * std::f64::consts::SQRT_2;
    let (_, vc) = dir_to_px(g.f, w, h);
    let rows_per_rad = h as f64 / std::f64::consts::PI;
    let y0 = ((vc - (half + 0.01) * rows_per_rad).floor().max(0.0)) as usize;
    let y1 = ((vc + (half + 0.01) * rows_per_rad).ceil() as usize).min(h);
    fn go<T: Sample>(s: &mut [T], w: usize, h: usize, y0: usize, y1: usize, g: &Gnomonic, filled: &Img, wimg: &Img, tm: ToneMap, k: f32) -> usize {
        s[y0 * w * 4..y1 * w * 4]
            .par_chunks_mut(w * 4)
            .enumerate()
            .map(|(jj, row)| {
                let y = y0 + jj;
                let mut changed = 0;
                let pitch = (0.5 - (y as f64 + 0.5) / h as f64) * 180.0;
                for x in 0..w {
                    let yaw = ((x as f64 + 0.5) / w as f64 - 0.5) * 360.0;
                    let d = dir(yaw, pitch);
                    let Some((gx, gy)) = g.project(d) else { continue };
                    if gx < 0.0 || gy < 0.0 || gx >= g.n as f64 || gy >= g.n as f64 {
                        continue;
                    }
                    let a = wimg.bilinear(gx as f32, gy as f32)[0];
                    if a <= 0.0 {
                        continue;
                    }
                    let c = filled.bilinear(gx as f32, gy as f32);
                    let o = x * 4;
                    for ch in 0..3 {
                        let old = row[o + ch].to_f() * k;
                        let new = tm.inv(c[ch]);
                        row[o + ch] = T::from_f((old + (new - old) * a) / k);
                    }
                    let oa = row[o + 3].to_f() * k;
                    row[o + 3] = T::from_f((oa + (1.0 - oa) * a) / k);
                    changed += 1;
                }
                changed
            })
            .sum()
    }
    match &mut l0.px {
        Pixels::U8(v) => go(v, w, h, y0, y1, g, filled, &wimg, tm, k),
        Pixels::U16(v) => go(v, w, h, y0, y1, g, filled, &wimg, tm, k),
        Pixels::F16(v) => go::<f16>(v, w, h, y0, y1, g, filled, &wimg, tm, k),
    }
}

/// Tone-maps a view (in place copy) for the 8-bit fill methods.
pub fn to_display(view: &Img, tm: ToneMap) -> Img {
    Img { w: view.w, h: view.h, px: view.px.iter().map(|p| [tm.fwd(p[0]), tm.fwd(p[1]), tm.fwd(p[2]), p[3]]).collect() }
}

/// Composites `fill` over `base` with `weight` (all in the same domain).
pub fn composite(base: &Img, fill: &Img, weight: &[f32]) -> Img {
    let mut out = base.clone();
    out.px.par_iter_mut().zip(fill.px.par_iter()).zip(weight.par_iter()).for_each(|((o, f), &a)| {
        for k in 0..3 {
            o[k] += (f[k] - o[k]) * a;
        }
        o[3] += (1.0 - o[3]) * a;
    });
    out
}

/// Downscales a mask (any hole pixel in the footprint → hole).
pub fn resize_mask(m: &[bool], w: usize, h: usize, nw: usize, nh: usize) -> Vec<bool> {
    let (sx, sy) = (w as f32 / nw as f32, h as f32 / nh as f32);
    (0..nw * nh)
        .into_par_iter()
        .map(|p| {
            let (x, y) = (p % nw, p / nw);
            let (x0, x1) = ((x as f32 * sx) as usize, (((x + 1) as f32 * sx).ceil() as usize).clamp(1, w));
            let (y0, y1) = ((y as f32 * sy) as usize, (((y + 1) as f32 * sy).ceil() as usize).clamp(1, h));
            (y0..y1).any(|yy| (x0..x1).any(|xx| m[yy * w + xx]))
        })
        .collect()
}

/// Approximate Gaussian blur (three box passes per axis).
pub fn blur(img: &Img, sigma: f32) -> Img {
    let r = ((sigma * 0.9).round() as usize).max(1);
    let mut cur = img.clone();
    for _ in 0..3 {
        cur = box_pass(&cur, r, true);
        cur = box_pass(&cur, r, false);
    }
    cur
}

fn box_pass(img: &Img, r: usize, horizontal: bool) -> Img {
    let (w, h) = (img.w, img.h);
    let mut out = Img::new(w, h);
    let n = (2 * r + 1) as f32;
    if horizontal {
        out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            let src = &img.px[y * w..(y + 1) * w];
            let mut acc = [0f32; 4];
            for i in 0..=2 * r {
                let p = src[(i as isize - r as isize).clamp(0, w as isize - 1) as usize];
                for k in 0..4 { acc[k] += p[k]; }
            }
            for x in 0..w {
                row[x] = acc.map(|v| v / n);
                let add = src[(x + r + 1).min(w - 1)];
                let sub = src[(x as isize - r as isize).clamp(0, w as isize - 1) as usize];
                for k in 0..4 { acc[k] += add[k] - sub[k]; }
            }
        });
    } else {
        let cols: Vec<Vec<[f32; 4]>> = (0..w)
            .into_par_iter()
            .map(|x| {
                let mut col = vec![[0f32; 4]; h];
                let mut acc = [0f32; 4];
                for i in 0..=2 * r {
                    let p = img.px[(i as isize - r as isize).clamp(0, h as isize - 1) as usize * w + x];
                    for k in 0..4 { acc[k] += p[k]; }
                }
                for y in 0..h {
                    col[y] = acc.map(|v| v / n);
                    let add = img.px[(y + r + 1).min(h - 1) * w + x];
                    let sub = img.px[(y as isize - r as isize).clamp(0, h as isize - 1) as usize * w + x];
                    for k in 0..4 { acc[k] += add[k] - sub[k]; }
                }
                col
            })
            .collect();
        for (x, col) in cols.into_iter().enumerate() {
            for (y, p) in col.into_iter().enumerate() {
                out.px[y * w + x] = p;
            }
        }
    }
    out
}

pub mod patchmatch {
    //! Classic exemplar-based completion (Wexler et al. / PatchMatch): multi-scale, EM iterations of
    //! nearest-neighbour search (propagation + random search) and voting. No model needed.

    use super::Img;
    use rayon::prelude::*;

    const R: i32 = 3; // patch radius → 7×7

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    fn down2(img: &Img, hole: &[bool]) -> (Img, Vec<bool>) {
        let (w, h) = (img.w.div_ceil(2), img.h.div_ceil(2));
        let small = img.resize(w, h);
        let m = super::resize_mask(hole, img.w, img.h, w, h);
        (small, m)
    }

    /// Fills `hole` in `img` (RGB in 0..1). `init`: optional guide (e.g. an upscaled AI result) used
    /// instead of the coarse-to-fine start; then only `levels_with_guide` finest levels are run.
    pub fn complete(img: &Img, hole: &[bool], init: Option<&Img>, iters: usize, levels_with_guide: usize) -> Img {
        // pyramid until the hole is small
        let mut imgs = vec![img.clone()];
        let mut holes = vec![hole.to_vec()];
        while imgs.last().unwrap().w > 64 && holes.last().unwrap().iter().filter(|&&b| b).count() > 400 {
            let (a, b) = down2(imgs.last().unwrap(), holes.last().unwrap());
            imgs.push(a);
            holes.push(b);
        }
        let top = imgs.len() - 1;
        let (start, mut cur) = match init {
            Some(g) => {
                let lv = levels_with_guide.min(top + 1) - 1;
                let s = &imgs[lv];
                (lv, guided(s, &holes[lv], &g.resize(s.w, s.h)))
            }
            None => (top, diffuse(&imgs[top], &holes[top])),
        };
        let mut nnf: Option<Vec<(i32, i32)>> = None;
        for lv in (0..=start).rev() {
            let (src, m) = (&imgs[lv], &holes[lv]);
            if lv != start {
                // upsample previous estimate into the hole of this level
                let up = cur.resize(src.w, src.h);
                cur = guided(src, m, &up);
                nnf = nnf.map(|f| upsample_nnf(&f, imgs[lv + 1].w, imgs[lv + 1].h, src.w, src.h));
            }
            let valid = valid_sources(m, src.w, src.h);
            let mut field = nnf.take().unwrap_or_else(|| random_nnf(src.w, src.h, &valid));
            let n_it = if lv == 0 { iters.div_ceil(2) } else { iters };
            // with a guide: match source patches against the (fixed) guide so the result keeps the
            // guide's structure and only gains real texture from the surroundings
            let anchor = init.map(|g| guided(src, m, &g.resize(src.w, src.h)));
            for it in 0..n_it {
                search(anchor.as_ref().unwrap_or(&cur), m, &valid, &mut field, it as u64 + lv as u64 * 97);
                cur = vote(&cur, src, m, &field);
            }
            nnf = Some(field);
        }
        cur
    }

    /// Full-resolution texture refinement: the search compares only the *low frequencies* (blurred
    /// surroundings vs. the smooth `anchor`, e.g. an upscaled AI result), but the hole receives the
    /// original full-detail pixel at the matched position – real grain from the surroundings,
    /// arranged along the anchor's structure. `sigma`: blur of the anchor in pixels.
    pub fn texture_refine(img: &Img, hole: &[bool], anchor: &Img, sigma: f32, iters: usize) -> Img {
        let lp = super::blur(img, sigma);
        let target = guided(&lp, hole, anchor);
        let valid = valid_sources(hole, img.w, img.h);
        let mut field = random_nnf(img.w, img.h, &valid);
        for it in 0..iters {
            search(&target, hole, &valid, &mut field, 7919 + it as u64);
        }
        let mut out = img.clone();
        for (p, &h) in hole.iter().enumerate() {
            if h {
                let (x, y) = field[p];
                out.px[p] = img.px[y as usize * img.w + x as usize];
            }
        }
        out
    }

    /// Image with the hole replaced by `guide`.
    fn guided(src: &Img, m: &[bool], guide: &Img) -> Img {
        let mut o = src.clone();
        for (p, &h) in m.iter().enumerate() {
            if h {
                o.px[p] = guide.px[p];
            }
        }
        o
    }

    /// Push-pull fill of the hole from its surroundings (coarsest level start).
    fn diffuse(src: &Img, m: &[bool]) -> Img {
        let mut o = src.clone();
        let (w, h) = (src.w, src.h);
        let mut known: Vec<bool> = m.iter().map(|&b| !b).collect();
        for _ in 0..(w + h) {
            let mut changed = false;
            let snapshot = o.px.clone();
            let kn = known.clone();
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    if kn[i] {
                        continue;
                    }
                    let mut acc = [0f32; 4];
                    let mut c = 0.0;
                    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let j = ny as usize * w + nx as usize;
                        if kn[j] {
                            for k in 0..4 {
                                acc[k] += snapshot[j][k];
                            }
                            c += 1.0;
                        }
                    }
                    if c > 0.0 {
                        o.px[i] = acc.map(|v| v / c);
                        known[i] = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        o
    }

    /// Patch centres usable as sources: the whole patch lies in the known region.
    fn valid_sources(m: &[bool], w: usize, h: usize) -> Vec<(i32, i32)> {
        let d = super::distance_outside(m, w, h);
        let mut v = vec![];
        for y in R..h as i32 - R {
            for x in R..w as i32 - R {
                if d[y as usize * w + x as usize] > (R as f32 + 1.5) {
                    v.push((x, y));
                }
            }
        }
        v
    }

    fn random_nnf(w: usize, h: usize, valid: &[(i32, i32)]) -> Vec<(i32, i32)> {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        (0..w * h).map(|_| valid[rng.below(valid.len())]).collect()
    }

    fn upsample_nnf(f: &[(i32, i32)], w0: usize, h0: usize, w: usize, h: usize) -> Vec<(i32, i32)> {
        (0..w * h)
            .map(|p| {
                let (x, y) = (p % w, p / w);
                let (sx, sy) = ((x * w0 / w).min(w0 - 1), (y * h0 / h).min(h0 - 1));
                let (tx, ty) = f[sy * w0 + sx];
                ((tx * 2 + (x % 2) as i32).clamp(R, w as i32 - R - 1), (ty * 2 + (y % 2) as i32).clamp(R, h as i32 - R - 1))
            })
            .collect()
    }

    #[inline]
    fn dist(img: &Img, ax: i32, ay: i32, bx: i32, by: i32, best: f32) -> f32 {
        let mut s = 0.0;
        for dy in -R..=R {
            let (y1, y2) = (ay + dy, by + dy);
            if y1 < 0 || y1 >= img.h as i32 {
                continue;
            }
            for dx in -R..=R {
                let x1 = ax + dx;
                if x1 < 0 || x1 >= img.w as i32 {
                    continue;
                }
                let p = img.px[y1 as usize * img.w + x1 as usize];
                let q = img.px[y2 as usize * img.w + (bx + dx) as usize];
                s += (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
            }
            if s >= best {
                return s;
            }
        }
        s
    }

    /// PatchMatch for every patch centre that overlaps the hole (rows in parallel bands; alternating
    /// scan direction per iteration).
    fn search(img: &Img, m: &[bool], valid: &[(i32, i32)], f: &mut [(i32, i32)], seed: u64) {
        let (w, h) = (img.w as i32, img.h as i32);
        let near = super::distance_outside(m, img.w, img.h);
        let rev = seed % 2 == 1;
        let band = 32usize;
        let fs: Vec<(i32, i32)> = f.to_vec();
        let rows: Vec<Vec<(i32, i32)>> = (0..img.h)
            .collect::<Vec<_>>()
            .par_chunks(band)
            .flat_map(|ys| {
                let mut rng = Rng(seed.wrapping_mul(0x2545F4914F6CDD1D) ^ (ys[0] as u64 + 1) * 0x9E37);
                let mut local: Vec<Vec<(i32, i32)>> = ys.iter().map(|&y| fs[y * img.w..(y + 1) * img.w].to_vec()).collect();
                let order: Vec<usize> = if rev { (0..ys.len()).rev().collect() } else { (0..ys.len()).collect() };
                for &li in &order {
                    let y = ys[li] as i32;
                    let xs: Vec<i32> = if rev { (0..w).rev().collect() } else { (0..w).collect() };
                    for x in xs {
                        if near[(y * w + x) as usize] > R as f32 * 1.5 {
                            continue; // patch does not touch the hole
                        }
                        let mut best = local[li][x as usize];
                        let mut bd = dist(img, x, y, best.0, best.1, f32::MAX);
                        let step = if rev { 1 } else { -1 };
                        // propagation from the previous pixel in scan order (x) and row (y)
                        let mut cands = vec![];
                        let px = x + step;
                        if px >= 0 && px < w {
                            let c = local[li][px as usize];
                            cands.push((c.0 - step, c.1));
                        }
                        let pl = li as i32 + step;
                        if pl >= 0 && (pl as usize) < ys.len() {
                            let c = local[pl as usize][x as usize];
                            cands.push((c.0, c.1 - step));
                        }
                        // random search around the current best
                        let mut rad = w.max(h);
                        while rad >= 1 {
                            let c = (best.0 + (rng.below(2 * rad as usize + 1) as i32 - rad), best.1 + (rng.below(2 * rad as usize + 1) as i32 - rad));
                            cands.push(c);
                            rad /= 2;
                        }
                        cands.push(valid[rng.below(valid.len())]);
                        for c in cands {
                            if c.0 < R || c.1 < R || c.0 >= w - R || c.1 >= h - R {
                                continue;
                            }
                            if near[(c.1 * w + c.0) as usize] <= R as f32 + 1.5 {
                                continue; // source patch must be known
                            }
                            let d = dist(img, x, y, c.0, c.1, bd);
                            if d < bd {
                                bd = d;
                                best = c;
                            }
                        }
                        local[li][x as usize] = best;
                    }
                }
                local
            })
            .collect();
        for (y, r) in rows.into_iter().enumerate() {
            f[y * img.w..(y + 1) * img.w].copy_from_slice(&r);
        }
    }

    /// Every hole pixel = average of the pixels that the patches covering it point to.
    fn vote(cur: &Img, src: &Img, m: &[bool], f: &[(i32, i32)]) -> Img {
        let (w, h) = (cur.w as i32, cur.h as i32);
        let mut out = src.clone();
        out.px.par_chunks_mut(cur.w).enumerate().for_each(|(y, row)| {
            let y = y as i32;
            for x in 0..w {
                if !m[(y * w + x) as usize] {
                    continue;
                }
                let mut acc = [0f32; 4];
                let mut n = 0.0;
                for dy in -R..=R {
                    for dx in -R..=R {
                        let (cx, cy) = (x + dx, y + dy);
                        if cx < 0 || cy < 0 || cx >= w || cy >= h {
                            continue;
                        }
                        let (tx, ty) = f[(cy * w + cx) as usize];
                        let (sx, sy) = (tx - dx, ty - dy);
                        if sx < 0 || sy < 0 || sx >= w || sy >= h {
                            continue;
                        }
                        let p = cur.px[(sy * w + sx) as usize];
                        for k in 0..4 {
                            acc[k] += p[k];
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    row[x as usize] = acc.map(|v| v / n);
                }
            }
        });
        out
    }
}
