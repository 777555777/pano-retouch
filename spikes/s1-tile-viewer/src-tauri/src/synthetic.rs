//! Synthetic equirect test panorama (safe to publish): hue by longitude, 10° checker, 1° grid,
//! fine noise (so clone strokes are visible), a dark "tripod" at the nadir and rings at ±80°.

use crate::pyramid::{Level, Pixels};
use rayon::prelude::*;

pub const PREFIX: &str = "synthetic:";

pub fn width_of(path: &str) -> Option<u32> {
    path.strip_prefix(PREFIX)?.parse().ok()
}

fn hsv(h: f64, s: f64, v: f64) -> [f64; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as i64 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn hash(x: u32, y: u32) -> f64 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f64 / 65535.0
}

pub fn generate(width: u32) -> Level {
    let (w, h) = (width, width / 2);
    let ppd = w as f64 / 360.0; // pixels per degree
    let mut px = vec![0u8; (w * h * 4) as usize];
    px.par_chunks_mut((w * 4) as usize)
        .enumerate()
        .for_each(|(y, row)| {
            let lat = 90.0 - (y as f64 + 0.5) / h as f64 * 180.0;
            for x in 0..w as usize {
                let lon = (x as f64 + 0.5) / w as f64 * 360.0 - 180.0;
                let mut c = hsv((lon + 180.0) / 360.0, 0.35, 0.85);
                if ((lon / 10.0).floor() as i64 + (lat / 10.0).floor() as i64).rem_euclid(2) == 1 {
                    c = c.map(|v| v * 0.85);
                }
                // grid: 1° thin dark lines, 10° bright lines (widths in pixels, lon lines widen with 1/cos(lat))
                let dist_line = |v: f64, step: f64| ((v / step).round() * step - v).abs();
                let lon_px = dist_line(lon, 1.0) * ppd * lat.to_radians().cos();
                let lat_px = dist_line(lat, 1.0) * ppd;
                if lon_px < 0.6 || lat_px < 0.6 {
                    c = c.map(|v| v * 0.7);
                }
                let lon10 = dist_line(lon, 10.0) * ppd * lat.to_radians().cos();
                let lat10 = dist_line(lat, 10.0) * ppd;
                if lon10 < 1.2 || lat10 < 1.2 {
                    c = [0.97, 0.97, 0.97];
                }
                if (lat.abs() - 80.0).abs() * ppd < 2.0 {
                    c = [0.85, 0.15, 0.2];
                }
                // tripod: disk around the nadir + three legs
                let polar = 90.0 + lat; // degrees from the nadir
                let leg = [0.0, 120.0, -120.0].iter().any(|&a: &f64| {
                    let dl = ((lon - a + 540.0) % 360.0 - 180.0).abs();
                    dl * polar.to_radians().sin() < 1.2 && polar < 25.0
                });
                if polar < 6.0 || leg {
                    c = [0.12, 0.12, 0.13];
                }
                let n = (hash(x as u32, y as u32) - 0.5) * 0.08;
                let o = x * 4;
                for k in 0..3 {
                    row[o + k] = ((c[k] + n).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
                row[o + 3] = 255;
            }
        });
    Level {
        width: w,
        height: h,
        px: Pixels::U8(px),
    }
}
