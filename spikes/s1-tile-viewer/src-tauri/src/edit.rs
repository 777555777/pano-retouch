//! Spike S2 – brush strokes on the sphere and their full-resolution rasterisation.
//!
//! A stroke is a centre line of directions (stored as yaw/pitch in degrees, ADR-0002) plus an angular
//! radius. Every pixel computes its direction and the angular distance to the line (great-circle
//! segments) – so a brush is a true circle on the sphere: no special cases at the seam or the poles.
//! The shader preview (viewer.wgsl, `strokeAlpha`) uses exactly the same formula.

use crate::pyramid::{GUTTER, Pixels, Pyramid, Sample, TILE, dir_to_uv};
use half::f16;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub type V3 = [f64; 3];

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StrokeKind {
    Mask,
    Clone,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Stroke {
    pub kind: StrokeKind,
    /// Centre line as [yaw, pitch] in degrees.
    pub points: Vec<[f64; 2]>,
    pub radius_deg: f64,
    /// 0 = soft (falloff over the whole radius) … 1 = hard edge.
    pub hardness: f64,
    pub opacity: f64,
    /// Clone source as a rotation quaternion [w, x, y, z]: source direction = R · target direction.
    pub rotation: Option<[f64; 4]>,
}

/// Mask overlay colour (domain colour "mask" #ff5c8a) and strength – must match the shader.
pub const MASK_RGB: [f64; 3] = [1.0, 92.0 / 255.0, 138.0 / 255.0];
pub const MASK_STRENGTH: f64 = 0.5;

pub fn dir(yaw_deg: f64, pitch_deg: f64) -> V3 {
    let (y, p) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    [y.sin() * p.cos(), p.sin(), -y.cos() * p.cos()]
}

#[inline]
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline]
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
fn normalize(a: V3) -> V3 {
    let l = norm(a);
    [a[0] / l, a[1] / l, a[2] / l]
}
/// Angle between unit vectors; atan2 form stays accurate for tiny angles (sub-pixel at 300 MP).
#[inline]
pub fn angle(a: V3, b: V3) -> f64 {
    norm(cross(a, b)).atan2(dot(a, b))
}

pub fn rotate(q: [f64; 4], v: V3) -> V3 {
    // v' = v + 2w(q×v) + 2 q×(q×v), q = (w, x, y, z)
    let (w, u) = (q[0], [q[1], q[2], q[3]]);
    let t = cross(u, v);
    let t = [2.0 * t[0], 2.0 * t[1], 2.0 * t[2]];
    let c = cross(u, t);
    [
        v[0] + w * t[0] + c[0],
        v[1] + w * t[1] + c[1],
        v[2] + w * t[2] + c[2],
    ]
}

#[derive(Clone, Copy)]
struct Segment {
    a: V3,
    b: V3,
    /// unit normal of the great circle through a and b; None for (near-)degenerate segments
    n: Option<V3>,
}

/// Distance from `d` to a great-circle arc a→b.
#[inline]
fn seg_dist(d: V3, s: &Segment) -> f64 {
    if let Some(n) = s.n {
        let dn = dot(d, n);
        let p = [d[0] - n[0] * dn, d[1] - n[1] * dn, d[2] - n[2] * dn];
        if dot(cross(s.a, p), n) >= 0.0 && dot(cross(p, s.b), n) >= 0.0 {
            return dn.abs().min(1.0).asin();
        }
    }
    angle(d, s.a).min(angle(d, s.b))
}

struct Chunk {
    c: V3,
    /// cap radius incl. brush radius
    r: f64,
    /// cos(r): a pixel is outside the cap iff dot(d, c) < cos_r (no trigonometry per pixel)
    cos_r: f64,
    segs: std::ops::Range<usize>,
}

pub struct Geometry {
    segs: Vec<Segment>,
    chunks: Vec<Chunk>,
    pub radius: f64,
    inner: f64,
    opacity: f64,
    /// bounding cap of the whole stroke
    pub cap: (V3, f64),
}

const SEGS_PER_CHUNK: usize = 8;

impl Geometry {
    pub fn new(s: &Stroke) -> Geometry {
        let pts: Vec<V3> = s.points.iter().map(|p| dir(p[0], p[1])).collect();
        let mut segs = vec![];
        if pts.len() == 1 {
            segs.push(Segment {
                a: pts[0],
                b: pts[0],
                n: None,
            });
        }
        for w in pts.windows(2) {
            let c = cross(w[0], w[1]);
            let n = if norm(c) > 1e-12 {
                Some(normalize(c))
            } else {
                None
            };
            segs.push(Segment {
                a: w[0],
                b: w[1],
                n,
            });
        }
        let radius = s.radius_deg.to_radians();
        let mut chunks = vec![];
        for (ci, group) in segs.chunks(SEGS_PER_CHUNK).enumerate() {
            let mut sum = [0.0; 3];
            for sg in group {
                for p in [sg.a, sg.b] {
                    sum = [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]];
                }
            }
            let c = if norm(sum) > 1e-9 {
                normalize(sum)
            } else {
                group[0].a
            };
            let r = group
                .iter()
                .map(|sg| angle(c, sg.a).max(angle(c, sg.b)))
                .fold(0.0, f64::max)
                + radius;
            let start = ci * SEGS_PER_CHUNK;
            chunks.push(Chunk {
                c,
                r,
                cos_r: if r >= std::f64::consts::PI {
                    -2.0
                } else {
                    r.cos()
                },
                segs: start..start + group.len(),
            });
        }
        let mut sum = [0.0; 3];
        for p in &pts {
            sum = [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]];
        }
        let cc = if norm(sum) > 1e-9 {
            normalize(sum)
        } else {
            pts[0]
        };
        let cr = chunks
            .iter()
            .map(|ch| angle(cc, ch.c) + ch.r)
            .fold(0.0, f64::max);
        Geometry {
            segs,
            chunks,
            radius,
            inner: radius * s.hardness.clamp(0.0, 1.0),
            opacity: s.opacity.clamp(0.0, 1.0),
            cap: (cc, cr),
        }
    }

    /// Minimum angular distance from `d` to the centre line (∞ if outside every chunk cap).
    pub fn dist(&self, d: V3) -> f64 {
        self.dist_in(d, &(0..self.chunks.len()).collect::<Vec<_>>())
    }

    /// Like `dist`, restricted to the given chunks (pre-filtered per tile).
    fn dist_in(&self, d: V3, chunks: &[usize]) -> f64 {
        let mut best = f64::INFINITY;
        for &ci in chunks {
            let ch = &self.chunks[ci];
            if dot(d, ch.c) < ch.cos_r {
                continue;
            }
            for s in &self.segs[ch.segs.clone()] {
                best = best.min(seg_dist(d, s));
            }
            if best <= self.inner {
                break; // fully opaque already
            }
        }
        best
    }

    /// Chunks whose cap overlaps the cap (c, r).
    fn chunks_near(&self, c: V3, r: f64) -> Vec<usize> {
        (0..self.chunks.len())
            .filter(|&i| angle(c, self.chunks[i].c) <= r + self.chunks[i].r)
            .collect()
    }

    /// Brush profile: 1 inside the hard core, smoothstep falloff to 0 at the radius.
    pub fn alpha(&self, d: V3) -> f64 {
        self.alpha_from(self.dist(d))
    }

    fn alpha_from(&self, dist: f64) -> f64 {
        if dist >= self.radius {
            return 0.0;
        }
        let a = if dist <= self.inner {
            1.0
        } else {
            let t = ((dist - self.inner) / (self.radius - self.inner)).clamp(0.0, 1.0);
            1.0 - t * t * (3.0 - 2.0 * t)
        };
        a * self.opacity
    }

    fn touches_cap(&self, c: V3, r: f64) -> bool {
        angle(c, self.cap.0) <= r + self.cap.1
            && self.chunks.iter().any(|ch| angle(c, ch.c) <= r + ch.r)
    }
}

/// Pixel rectangle [x0, x1) × [y0, y1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

/// Bounding cap of an equirect tile (centre + max angle to corners and edge midpoints).
fn tile_cap(w: u32, h: u32, tx: u32, ty: u32) -> (V3, f64) {
    rect_cap(
        w,
        h,
        tx * TILE,
        ((tx + 1) * TILE).min(w),
        ty * TILE,
        ((ty + 1) * TILE).min(h),
    )
}

/// Bounding cap of the pixel rectangle [x0, x1) × [y0, y1).
fn rect_cap(w: u32, h: u32, x0: u32, x1: u32, y0: u32, y1: u32) -> (V3, f64) {
    let (x0, x1, y0, y1) = (x0 as f64, x1 as f64, y0 as f64, y1 as f64);
    let px = |x: f64, y: f64| -> V3 {
        let yaw = (x / w as f64 - 0.5) * 360.0;
        let pitch = (0.5 - y / h as f64) * 180.0;
        dir(yaw, pitch)
    };
    let c = px((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut r: f64 = 0.0;
    for (x, y) in [
        (x0, y0),
        (x1, y0),
        (x0, y1),
        (x1, y1),
        ((x0 + x1) / 2.0, y0),
        ((x0 + x1) / 2.0, y1),
        (x0, (y0 + y1) / 2.0),
        (x1, (y0 + y1) / 2.0),
    ] {
        r = r.max(angle(c, px(x, y)));
    }
    // a pixel of margin
    (c, r + std::f64::consts::PI / h as f64)
}

/// Block size for culling inside a tile.
const BLOCK: u32 = 32;

pub struct Applied {
    /// touched pixel rectangles at level 0 (one per affected tile)
    pub rects: Vec<Rect>,
    pub pixels: usize,
    pub tiles_tested: usize,
}

/// Rasterises `stroke` into level 0 of `pyr` (reads a snapshot first, then writes – clone strokes may
/// read from the area they write to).
pub fn apply(pyr: &Pyramid, stroke: &Stroke) -> Applied {
    let g = Geometry::new(stroke);
    let rot = stroke.rotation.unwrap_or([1.0, 0.0, 0.0, 0.0]);
    let (w, h) = {
        let l = pyr.levels[0].read();
        (l.width, l.height)
    };
    let (tiles_x, tiles_y) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let tiles: Vec<(u32, u32)> = (0..tiles_y)
        .flat_map(|ty| (0..tiles_x).map(move |tx| (tx, ty)))
        .filter(|&(tx, ty)| {
            let (c, r) = tile_cap(w, h, tx, ty);
            g.touches_cap(c, r)
        })
        .collect();
    let tiles_tested = tiles.len();

    // per-column yaw and per-row pitch trigonometry
    let lon: Vec<(f64, f64)> = (0..w)
        .map(|x| ((x as f64 + 0.5) / w as f64 - 0.5) * std::f64::consts::TAU)
        .map(|a| a.sin_cos())
        .collect();
    let lat: Vec<(f64, f64)> = (0..h)
        .map(|y| (0.5 - (y as f64 + 0.5) / h as f64) * std::f64::consts::PI)
        .map(|a| a.sin_cos())
        .collect();

    type Change<T> = (usize, [T; 4]);
    fn compute<T: Sample>(
        px: &[T],
        w: u32,
        h: u32,
        tiles: &[(u32, u32)],
        g: &Geometry,
        kind: StrokeKind,
        rot: [f64; 4],
        lon: &[(f64, f64)],
        lat: &[(f64, f64)],
        max: f64,
    ) -> Vec<(Rect, Vec<Change<T>>)> {
        let mask = [MASK_RGB[0] * max, MASK_RGB[1] * max, MASK_RGB[2] * max];
        tiles
            .par_iter()
            .map(|&(tx, ty)| {
                let mut ch = vec![];
                let (mut r, mut any) = (
                    Rect {
                        x0: u32::MAX,
                        y0: u32::MAX,
                        x1: 0,
                        y1: 0,
                    },
                    false,
                );
                let (tx1, ty1) = (((tx + 1) * TILE).min(w), ((ty + 1) * TILE).min(h));
                for by in (ty * TILE..ty1).step_by(BLOCK as usize) {
                    for bx in (tx * TILE..tx1).step_by(BLOCK as usize) {
                        let (bx1, by1) = ((bx + BLOCK).min(tx1), (by + BLOCK).min(ty1));
                        let (bc, br) = rect_cap(w, h, bx, bx1, by, by1);
                        let near = g.chunks_near(bc, br);
                        if near.is_empty() {
                            continue;
                        }
                        for y in by..by1 {
                            let (sp, cp) = lat[y as usize];
                            for x in bx..bx1 {
                                let (sl, cl) = lon[x as usize];
                                let d = [sl * cp, sp, -cl * cp];
                                let a = g.alpha_from(g.dist_in(d, &near));
                                if a <= 0.0 {
                                    continue;
                                }
                                let i = (y as usize * w as usize + x as usize) * 4;
                                let old = [
                                    px[i].to_f() as f64,
                                    px[i + 1].to_f() as f64,
                                    px[i + 2].to_f() as f64,
                                    px[i + 3].to_f() as f64,
                                ];
                                let new = match kind {
                                    StrokeKind::Mask => {
                                        let k = a * MASK_STRENGTH;
                                        [
                                            old[0] + (mask[0] - old[0]) * k,
                                            old[1] + (mask[1] - old[1]) * k,
                                            old[2] + (mask[2] - old[2]) * k,
                                            old[3],
                                        ]
                                    }
                                    StrokeKind::Clone => {
                                        let s = rotate(rot, d);
                                        let (u, v) =
                                            dir_to_uv([s[0] as f32, s[1] as f32, s[2] as f32]);
                                        let src = sample_clone(
                                            px,
                                            w as usize,
                                            h as usize,
                                            u as f64 * w as f64,
                                            v as f64 * h as f64,
                                        );
                                        [0, 1, 2, 3].map(|k| old[k] + (src[k] - old[k]) * a)
                                    }
                                };
                                ch.push((i, new.map(|v| T::from_f(v as f32))));
                                any = true;
                                r = Rect {
                                    x0: r.x0.min(x),
                                    y0: r.y0.min(y),
                                    x1: r.x1.max(x + 1),
                                    y1: r.y1.max(y + 1),
                                };
                            }
                        }
                    }
                }
                (
                    if any {
                        r
                    } else {
                        Rect {
                            x0: 0,
                            y0: 0,
                            x1: 0,
                            y1: 0,
                        }
                    },
                    ch,
                )
            })
            .filter(|(r, _)| r.x1 > r.x0)
            .collect()
    }

    fn write<T: Sample>(px: &mut [T], changes: Vec<(Rect, Vec<Change<T>>)>) -> (Vec<Rect>, usize) {
        let mut rects = vec![];
        let mut n = 0;
        for (r, ch) in changes {
            n += ch.len();
            for (i, v) in ch {
                px[i..i + 4].copy_from_slice(&v);
            }
            rects.push(r);
        }
        (rects, n)
    }

    let kind = stroke.kind;
    let (rects, pixels) = {
        let l0 = pyr.levels[0].read();
        match &l0.px {
            Pixels::U8(v) => {
                let c = compute(v, w, h, &tiles, &g, kind, rot, &lon, &lat, 255.0);
                drop(l0);
                let mut l0 = pyr.levels[0].write();
                let Pixels::U8(v) = &mut l0.px else {
                    unreachable!()
                };
                write(v, c)
            }
            Pixels::U16(v) => {
                let c = compute(v, w, h, &tiles, &g, kind, rot, &lon, &lat, 65535.0);
                drop(l0);
                let mut l0 = pyr.levels[0].write();
                let Pixels::U16(v) = &mut l0.px else {
                    unreachable!()
                };
                write(v, c)
            }
            Pixels::F16(v) => {
                // linear HDR data: the mask colour in linear light
                let c = compute::<f16>(v, w, h, &tiles, &g, kind, rot, &lon, &lat, 1.0);
                drop(l0);
                let mut l0 = pyr.levels[0].write();
                let Pixels::F16(v) = &mut l0.px else {
                    unreachable!()
                };
                write(v, c)
            }
        }
    };
    Applied {
        rects,
        pixels,
        tiles_tested,
    }
}

/// Clone-source sampling. `S2_CLONE_FILTER=bilinear` switches back for comparison.
fn sample_clone<T: Sample>(s: &[T], w: usize, h: usize, x: f64, y: f64) -> [f64; 4] {
    static BILINEAR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *BILINEAR.get_or_init(|| std::env::var("S2_CLONE_FILTER").is_ok_and(|v| v == "bilinear")) {
        return bilinear(s, w, h, x, y);
    }
    catmull_rom(s, w, h, x, y)
}

/// Bicubic (Catmull-Rom) sampling, x wraps, y clamps. Slightly sharper than bilinear, which offsets
/// the second (bilinear) filtering when the result is displayed.
pub(crate) fn catmull_rom<T: Sample>(s: &[T], w: usize, h: usize, x: f64, y: f64) -> [f64; 4] {
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let wts = |t: f64| -> [f64; 4] {
        let (t2, t3) = (t * t, t * t * t);
        [
            -0.5 * t3 + t2 - 0.5 * t,
            1.5 * t3 - 2.5 * t2 + 1.0,
            -1.5 * t3 + 2.0 * t2 + 0.5 * t,
            0.5 * t3 - 0.5 * t2,
        ]
    };
    let (wx, wy) = (wts(tx), wts(ty));
    let mut acc = [0.0; 4];
    for (j, wyj) in wy.iter().enumerate() {
        let yy = (y0 as i64 - 1 + j as i64).clamp(0, h as i64 - 1) as usize;
        for (i, wxi) in wx.iter().enumerate() {
            let xx = (x0 as i64 - 1 + i as i64).rem_euclid(w as i64) as usize;
            let p = (yy * w + xx) * 4;
            let k = wxi * wyj;
            for c in 0..4 {
                acc[c] += s[p + c].to_f() as f64 * k;
            }
        }
    }
    acc
}

fn bilinear<T: Sample>(s: &[T], w: usize, h: usize, x: f64, y: f64) -> [f64; 4] {
    let fx = x - 0.5;
    let fy = (y - 0.5).clamp(0.0, h as f64 - 1.0);
    let (x0f, y0f) = (fx.floor(), fy.floor());
    let (ax, ay) = (fx - x0f, fy - y0f);
    let x0 = (x0f as i64).rem_euclid(w as i64) as usize;
    let x1 = (x0 + 1) % w;
    let y0 = y0f as usize;
    let y1 = (y0 + 1).min(h - 1);
    let p = |x: usize, y: usize, k: usize| s[(y * w + x) * 4 + k].to_f() as f64;
    [0, 1, 2, 3].map(|k| {
        let top = p(x0, y0, k) * (1.0 - ax) + p(x1, y0, k) * ax;
        let bot = p(x0, y1, k) * (1.0 - ax) + p(x1, y1, k) * ax;
        top * (1.0 - ay) + bot * ay
    })
}

/// Propagates changed level-0 rectangles to the coarser levels; returns the changed rects per level.
pub fn propagate(pyr: &Pyramid, rects0: Vec<Rect>) -> Vec<Vec<Rect>> {
    let mut all = vec![rects0];
    for li in 1..pyr.levels.len() {
        let src = pyr.levels[li - 1].read();
        let mut dst = pyr.levels[li].write();
        let (dw, dh) = (dst.width, dst.height);
        // output pixel i covers source [i·r, (i+1)·r) with r = src / dst size (≈ 2)
        let (rx, ry) = (src.width as f64 / dw as f64, src.height as f64 / dh as f64);
        let rects: Vec<Rect> = all[li - 1]
            .iter()
            .map(|r| Rect {
                x0: (r.x0 as f64 / rx).floor() as u32,
                y0: (r.y0 as f64 / ry).floor() as u32,
                x1: ((r.x1 as f64 / rx).ceil() as u32).min(dw),
                y1: ((r.y1 as f64 / ry).ceil() as u32).min(dh),
            })
            .collect();
        for &r in &rects {
            crate::pyramid::resample_into(&src, &mut dst, r.x0, r.x1, r.y0, r.y1);
        }
        all.push(rects);
    }
    all
}

/// Tiles whose slot (tile + gutter) overlaps one of `rects`. Slots wrap in x exactly like
/// `equirect_tile_*` (x mod W) – including the partial last column, whose right part shows x ≥ 0 again.
pub fn dirty_tiles(w: u32, h: u32, rects: &[Rect]) -> Vec<(u32, u32)> {
    let (tx_n, ty_n) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let (w, t, g) = (w as i64, TILE as i64, GUTTER as i64);
    let mut set = std::collections::BTreeSet::new();
    for r in rects {
        let (rx0, rx1) = (r.x0 as i64, r.x1 as i64);
        for ty in 0..ty_n {
            let (sy0, sy1) = (ty as i64 * t - g, (ty as i64 + 1) * t + g);
            if sy1 <= r.y0 as i64 || sy0 >= r.y1 as i64 {
                continue;
            }
            for tx in 0..tx_n {
                let (sx0, sx1) = (tx as i64 * t - g, (tx as i64 + 1) * t + g);
                if [-w, 0, w].iter().any(|&o| sx0 + o < rx1 && sx1 + o > rx0) {
                    set.insert((tx, ty));
                }
            }
        }
    }
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pyramid::{Level, Pyramid};

    fn flat(w: u32, h: u32, v: u8) -> Pyramid {
        Pyramid::build(Level {
            width: w,
            height: h,
            px: Pixels::U8(vec![v; (w * h * 4) as usize]),
        })
    }

    fn mask_stroke(points: Vec<[f64; 2]>, r: f64) -> Stroke {
        Stroke {
            kind: StrokeKind::Mask,
            points,
            radius_deg: r,
            hardness: 1.0,
            opacity: 1.0,
            rotation: None,
        }
    }

    /// Brute-force reference: densely sampled centre line, plain angular distance.
    fn reference_alpha(s: &Stroke, d: V3) -> f64 {
        let mut best = f64::INFINITY;
        for w in s.points.windows(2) {
            let (a, b) = (dir(w[0][0], w[0][1]), dir(w[1][0], w[1][1]));
            let om = angle(a, b);
            for i in 0..=2000 {
                let t = i as f64 / 2000.0;
                // slerp
                let p = if om < 1e-12 {
                    a
                } else {
                    let (sa, sb) = (((1.0 - t) * om).sin() / om.sin(), (t * om).sin() / om.sin());
                    [
                        a[0] * sa + b[0] * sb,
                        a[1] * sa + b[1] * sb,
                        a[2] * sa + b[2] * sb,
                    ]
                };
                best = best.min(angle(d, p));
            }
        }
        if best < s.radius_deg.to_radians() {
            1.0
        } else {
            0.0
        }
    }

    fn pixel_dir(w: u32, h: u32, x: u32, y: u32) -> V3 {
        dir(
            ((x as f64 + 0.5) / w as f64 - 0.5) * 360.0,
            (0.5 - (y as f64 + 0.5) / h as f64) * 180.0,
        )
    }

    #[test]
    fn segment_distance_matches_brute_force() {
        let s = mask_stroke(vec![[-30.0, 10.0], [40.0, -20.0], [100.0, 60.0]], 5.0);
        let g = Geometry::new(&s);
        let (w, h) = (360, 180);
        let mut wrong = 0;
        for y in 0..h {
            for x in 0..w {
                let d = pixel_dir(w, h, x, y);
                let a = if g.dist(d) < g.radius { 1.0 } else { 0.0 };
                // brute force has ~0.02° sampling error – ignore pixels right at the edge
                let edge = (g.dist(d) - g.radius).abs() < 0.05f64.to_radians();
                if !edge && a != reference_alpha(&s, d) {
                    wrong += 1;
                }
            }
        }
        assert_eq!(wrong, 0);
    }

    #[test]
    fn stroke_across_the_seam_is_continuous() {
        let pyr = flat(2048, 1024, 0);
        let s = mask_stroke(vec![[170.0, 0.0], [-170.0, 0.0]], 2.0); // the short way: through 180°
        let ap = apply(&pyr, &s);
        let l0 = pyr.levels[0].read();
        let Pixels::U8(px) = &l0.px else { panic!() };
        let y = 512usize;
        let at = |x: usize| px[(y * 2048 + x) * 4];
        assert!(
            at(0) > 0 && at(2047) > 0,
            "both sides of the seam are painted"
        );
        assert_eq!(at(0), at(2047));
        assert_eq!(at(1024), 0, "the far side (yaw 0) is untouched");
        // dirty tiles include the first and the last column
        let tiles = dirty_tiles(2048, 1024, &ap.rects);
        assert!(tiles.iter().any(|t| t.0 == 0) && tiles.iter().any(|t| t.0 == 3));
    }

    #[test]
    fn dirty_tiles_follow_the_wrapping_gutter() {
        // W = 1100 → tiles 0,1 full, tile 2 covers 1024..1536 and wraps: its pixels ≥ 1100 show x = 0..436
        let one = |x: u32| Rect {
            x0: x,
            y0: 10,
            x1: x + 1,
            y1: 11,
        };
        assert_eq!(dirty_tiles(1100, 512, &[one(1099)]), vec![(0, 0), (2, 0)]); // tile 0's left gutter
        assert!(dirty_tiles(1100, 512, &[one(0)]).contains(&(2, 0))); // wrapped part of the last tile
        assert_eq!(dirty_tiles(1100, 512, &[one(600)]), vec![(1, 0)]);
        assert_eq!(dirty_tiles(1100, 512, &[one(512)]), vec![(0, 0), (1, 0)]); // tile 0's right gutter
    }

    #[test]
    fn stroke_over_the_nadir_covers_the_whole_bottom_row() {
        let pyr = flat(2048, 1024, 0);
        // great circle through the nadir: from yaw 0 to yaw 180 at pitch -80
        let s = mask_stroke(vec![[0.0, -80.0], [0.0, -90.0], [180.0, -80.0]], 1.0);
        apply(&pyr, &s);
        let l0 = pyr.levels[0].read();
        let Pixels::U8(px) = &l0.px else { panic!() };
        let y = 1023usize; // last row, ~0.09° from the nadir
        assert!(
            (0..2048).all(|x| px[(y * 2048 + x) * 4] > 0),
            "a cap around the nadir is a full row band"
        );
    }

    #[test]
    fn brush_is_a_circle_on_the_sphere_at_the_pole() {
        // a dab at pitch -89 with radius 3° must reach the nadir and beyond (pitch -88 on the other side)
        let s = mask_stroke(vec![[0.0, -89.0]], 3.0);
        let g = Geometry::new(&s);
        assert!(g.alpha(dir(180.0, -88.5)) > 0.0);
        assert!(g.alpha(dir(180.0, -87.0)) == 0.0);
        assert!(g.alpha(dir(90.0, -88.0)) > 0.0); // ≈ √(1² + 2²) = 2.2° away
        assert!(g.alpha(dir(90.0, -86.5)) == 0.0); // ≈ √(1² + 3.5²) = 3.6° away
    }

    #[test]
    fn propagation_updates_all_levels_and_dirty_tiles() {
        let pyr = flat(4096, 2048, 10);
        let s = mask_stroke(vec![[0.0, 0.0], [10.0, 0.0]], 1.0);
        let ap = apply(&pyr, &s);
        let rects = propagate(&pyr, ap.rects);
        assert_eq!(rects.len(), pyr.levels.len());
        for (li, lv) in pyr.levels.iter().enumerate() {
            let l = lv.read();
            let Pixels::U8(px) = &l.px else { panic!() };
            let (x, y) = (
                (l.width / 2 + l.width / 72) as usize,
                (l.height / 2) as usize,
            ); // yaw 5°, pitch 0
            assert!(
                px[(y * l.width as usize + x) * 4] > 10,
                "level {li} shows the stroke"
            );
        }
    }

    #[test]
    fn clone_reads_from_the_rotated_source() {
        // left half dark, right half bright; clone with a 180° rotation around y copies right → left
        let (w, h) = (512u32, 256u32);
        let mut v = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in w / 2..w {
                v[((y * w + x) * 4) as usize] = 200;
            }
        }
        let pyr = Pyramid::build(Level {
            width: w,
            height: h,
            px: Pixels::U8(v),
        });
        let q = [0.0, 0.0, 1.0, 0.0]; // 180° about +y
        let s = Stroke {
            kind: StrokeKind::Clone,
            points: vec![[-90.0, 0.0]],
            radius_deg: 10.0,
            hardness: 1.0,
            opacity: 1.0,
            rotation: Some(q),
        };
        apply(&pyr, &s);
        let l0 = pyr.levels[0].read();
        let Pixels::U8(px) = &l0.px else { panic!() };
        let x = w / 4; // yaw -90°
        assert!(px[((h / 2 * w + x) * 4) as usize] > 190);
    }
}
