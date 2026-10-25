//! Headless benchmark of the Rust side of spike S1 (decode, pyramid, tiles, disk cache).
//!
//! Usage: `cargo run --release --bin s1-bench -- [--zune] [--no-cache] <file>…`
//! Without files, all images under PANO_TESTDATA are measured.

use s1_tile_viewer_lib::{cache, decode, edit, pyramid, session, synthetic};
use std::path::PathBuf;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zune = args.iter().any(|a| a == "--zune");
    let no_cache = args.iter().any(|a| a == "--no-cache");
    if let Some(i) = args.iter().position(|a| a == "--golden") {
        golden(&PathBuf::from(args.get(i + 1).expect("--golden <dir>")));
        return;
    }
    if args.iter().any(|a| a == "--strokes") {
        for f in args.iter().filter(|a| !a.starts_with("--")) {
            strokes(&PathBuf::from(f));
        }
        println!("peak RSS: {:.0} MB", session::peak_rss_mb());
        return;
    }
    let mut files: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();
    if files.is_empty() {
        let root = s1_tile_viewer_lib::testdata_root();
        files = s1_tile_viewer_lib::list_images(&root);
    }
    println!("threads: {}", rayon::current_num_threads());
    for f in files {
        if let Err(e) = bench(&f, zune, no_cache) {
            println!("{}: ERROR {e}", f.display());
        }
    }
    println!("peak RSS: {:.0} MB", session::peak_rss_mb());
}

fn bench(path: &std::path::Path, zune: bool, no_cache: bool) -> Result<(), String> {
    let name = path.file_name().unwrap().to_string_lossy();
    let size_mb = std::fs::metadata(path).map_err(|e| e.to_string())?.len() as f64 / 1e6;
    println!("\n## {name}  ({size_mb:.0} MB)");
    let t = Instant::now();
    let p = decode::probe(path)?;
    println!(
        "probe            {:>8.1} ms   {}×{} {:?} {:?} ({:.0} MP)",
        ms(t),
        p.width,
        p.height,
        p.kind,
        p.storage,
        p.width as f64 * p.height as f64 / 1e6
    );

    if p.kind == decode::Kind::Jpeg {
        let t = Instant::now();
        let q = decode::jpeg_quick(path)?;
        let tq = ms(t);
        let t = Instant::now();
        let _b = pyramid::Base::build(&q);
        println!(
            "jpeg 1/8 decode  {:>8.1} ms   ({}×{}), base resample {:.1} ms",
            tq,
            q.width,
            q.height,
            ms(t)
        );
    }

    let dec = if zune {
        decode::JpegDecoder::Zune
    } else {
        decode::JpegDecoder::Turbo
    };
    let t = Instant::now();
    let l0 = decode::decode_full(path, p.kind, dec)?;
    println!("full decode      {:>8.1} ms   ({dec:?})", ms(t));
    let t = Instant::now();
    let pyr = pyramid::Pyramid::build(l0);
    println!(
        "pyramid          {:>8.1} ms   levels {:?}",
        ms(t),
        pyr.levels
            .iter()
            .map(|l| l.read().width)
            .collect::<Vec<_>>()
    );
    let t = Instant::now();
    let base = pyramid::Base::build(&pyr.levels.last().unwrap().read());
    println!("base texture     {:>8.1} ms", ms(t));

    let infos = pyramid::level_infos(p.width, p.height);
    let l0i = &infos[0];
    let n = (l0i.tiles_x * l0i.tiles_y) as usize;
    for wire in [pyramid::Wire::U8, pyramid::Wire::F16] {
        // S1: extract raw tile, then convert (several allocations and copies per tile)
        let t = Instant::now();
        for ty in 0..l0i.tiles_y {
            for tx in 0..l0i.tiles_x {
                let raw = pyr.equirect_tile_raw(0, tx, ty).unwrap();
                std::hint::black_box(pyramid::to_wire(p.storage, raw, wire));
            }
        }
        let old = ms(t) / n as f64;
        // S1b: one pass straight into the wire format
        let t = Instant::now();
        let mut bytes = 0usize;
        for ty in 0..l0i.tiles_y {
            for tx in 0..l0i.tiles_x {
                bytes +=
                    std::hint::black_box(pyr.equirect_tile_wire(0, tx, ty, wire).unwrap()).len();
            }
        }
        println!(
            "tile L0 {:?}      {:>8.3} ms/tile single pass, {:.3} ms raw+convert ({n} tiles, {:.2} MB/tile, 1 thread)",
            wire,
            ms(t) / n as f64,
            old,
            bytes as f64 / n as f64 / 1e6
        );
    }
    let cl = pyramid::cube_level_infos(&infos);
    let t = Instant::now();
    let mut k = 0;
    for face in [5u32, 3] {
        for (tx, ty) in [(0, 0), (3, 3), (7, 7), (10, 4)] {
            let _ = pyr.cube_tile_raw(face, 0, cl[0].n, tx, ty);
            k += 1;
        }
    }
    println!(
        "cube tile L0     {:>8.3} ms/tile (on-demand resample, single thread, face {}²)",
        ms(t) / k as f64,
        cl[0].n
    );

    if !no_cache {
        let dir = cache::root().join("bench");
        let t = Instant::now();
        let bytes = cache::write(&dir, &pyr, &base, &infos)?;
        println!(
            "cache write      {:>8.1} ms   {:.0} MB on disk",
            ms(t),
            bytes as f64 / 1e6
        );
        drop(pyr);
        let t = Instant::now();
        let (r, _b) = cache::Reader::open(&dir)?.ok_or("cache missing")?;
        println!("cache open+base  {:>8.1} ms", ms(t));
        let t = Instant::now();
        for ty in 0..l0i.tiles_y {
            for tx in 0..l0i.tiles_x {
                let raw = r.tile_raw(0, tx, ty)?;
                let _ = pyramid::to_wire(p.storage, raw, pyramid::Wire::U8);
            }
        }
        println!(
            "cache tile L0 u8 {:>8.3} ms/tile (read + lz4 + convert, single thread)",
            ms(t) / n as f64
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
    println!("peak RSS so far  {:>8.0} MB", session::peak_rss_mb());
    Ok(())
}

// ---------------------------------------------------------------- S2

/// Typical strokes used by the in-app benchmark too (yaw/pitch in degrees).
pub fn scenario_strokes() -> Vec<(&'static str, edit::Stroke)> {
    use edit::{Stroke, StrokeKind::*};
    let line = |a: [f64; 2], b: [f64; 2], n: usize| -> Vec<[f64; 2]> {
        (0..=n)
            .map(|i| {
                let t = i as f64 / n as f64;
                [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t + (t * 12.0).sin() * 1.5,
                ]
            })
            .collect()
    };
    let spiral: Vec<[f64; 2]> = (0..80)
        .map(|i| {
            [
                i as f64 * 27.0 % 360.0 - 180.0,
                -90.0 + 2.0 + i as f64 * 0.2,
            ]
        })
        .collect();
    let circle: Vec<[f64; 2]> = (0..=72).map(|i| [i as f64 * 5.0 - 180.0, -72.0]).collect();
    // clone source: 40° rotation about +x (target at the nadir → source 40° towards yaw 0)
    let h = 20f64.to_radians();
    let rot = Some([h.cos(), h.sin(), 0.0, 0.0]);
    vec![
        (
            "horizon mask r1.5° 30° long",
            Stroke {
                kind: Mask,
                points: line([-15.0, 0.0], [15.0, 0.0], 60),
                radius_deg: 1.5,
                hardness: 0.5,
                opacity: 1.0,
                rotation: None,
            },
        ),
        (
            "seam mask r2° across 180°",
            Stroke {
                kind: Mask,
                points: line([165.0, 5.0], [195.0, 5.0], 60),
                radius_deg: 2.0,
                hardness: 0.5,
                opacity: 1.0,
                rotation: None,
            },
        ),
        (
            "nadir ring mask r2° at -72°",
            Stroke {
                kind: Mask,
                points: circle,
                radius_deg: 2.0,
                hardness: 0.7,
                opacity: 1.0,
                rotation: None,
            },
        ),
        (
            "nadir clone r4° spiral (tripod)",
            Stroke {
                kind: Clone,
                points: spiral,
                radius_deg: 4.0,
                hardness: 0.6,
                opacity: 1.0,
                rotation: rot,
            },
        ),
        (
            "1:1 clone r0.15°",
            Stroke {
                kind: Clone,
                points: line([30.0, 2.0], [31.0, 2.0], 30),
                radius_deg: 0.15,
                hardness: 0.5,
                opacity: 1.0,
                rotation: Some([
                    (0.25f64.to_radians()).cos(),
                    0.0,
                    (0.25f64.to_radians()).sin(),
                    0.0,
                ]),
            },
        ),
        (
            "big mask r10° 90° long",
            Stroke {
                kind: Mask,
                points: line([-45.0, -20.0], [45.0, -20.0], 90),
                radius_deg: 10.0,
                hardness: 0.3,
                opacity: 1.0,
                rotation: None,
            },
        ),
    ]
}

fn strokes(path: &std::path::Path) {
    println!(
        "\n## strokes on {}",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string())
    );
    let p = decode::probe(path).unwrap();
    let pyr = pyramid::Pyramid::build(
        decode::decode_full(path, p.kind, decode::JpegDecoder::Turbo).unwrap(),
    );
    let infos = pyramid::level_infos(p.width, p.height);
    for (name, s) in scenario_strokes() {
        let t = Instant::now();
        let ap = edit::apply(&pyr, &s);
        let tr = ms(t);
        let px = ap.pixels;
        let t = Instant::now();
        let rects = edit::propagate(&pyr, ap.rects);
        let tp = ms(t);
        let t = Instant::now();
        let _b = pyramid::Base::build(&pyr.levels.last().unwrap().read());
        let tb = ms(t);
        let dirty: usize = rects
            .iter()
            .enumerate()
            .map(|(l, r)| edit::dirty_tiles(infos[l].w, infos[l].h, r).len())
            .sum();
        println!(
            "{name:34} raster {tr:6.1} ms  pyramid {tp:5.1} ms  base {tb:4.1} ms  = {:6.1} ms   {:.1} MP touched, {dirty} dirty tiles",
            tr + tp + tb,
            px as f64 / 1e6
        );
    }
}

/// Golden images on the synthetic panorama (public): equirect + views of the nadir and the seam,
/// before and after the scenario strokes.
fn golden(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let pyr = pyramid::Pyramid::build(synthetic::generate(4096));
    let snap = |tag: &str| {
        let l = pyr.levels[0].read();
        let pyramid::Pixels::U8(px) = &l.px else {
            unreachable!()
        };
        save_jpeg(
            &dir.join(format!("{tag}-equirect.jpg")),
            &downscale(px, l.width as usize, l.height as usize, 2),
            2048,
            1024,
        );
        for (name, yaw, pitch, fov) in [("nadir", 0.0, -90.0, 110.0), ("seam", 180.0, 5.0, 70.0)] {
            let img = render_view(
                px,
                l.width as usize,
                l.height as usize,
                yaw,
                pitch,
                fov,
                1200,
                800,
            );
            save_jpeg(&dir.join(format!("{tag}-{name}.jpg")), &img, 1200, 800);
        }
    };
    snap("before");
    for (name, s) in scenario_strokes() {
        let ap = edit::apply(&pyr, &s);
        edit::propagate(&pyr, ap.rects);
        println!("applied {name}: {} px", ap.pixels);
    }
    snap("after");
    println!("golden images written to {}", dir.display());
}

fn downscale(px: &[u8], w: usize, h: usize, f: usize) -> Vec<u8> {
    let (nw, nh) = (w / f, h / f);
    let mut out = vec![0u8; nw * nh * 3];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..3 {
                let mut acc = 0u32;
                for dy in 0..f {
                    for dx in 0..f {
                        acc += px[((y * f + dy) * w + x * f + dx) * 4 + c] as u32;
                    }
                }
                out[(y * nw + x) * 3 + c] = (acc / (f * f) as u32) as u8;
            }
        }
    }
    out
}

/// Rectilinear view, same conventions as the viewer (bilinear, x wraps).
fn render_view(
    px: &[u8],
    w: usize,
    h: usize,
    yaw: f64,
    pitch: f64,
    fov: f64,
    ow: usize,
    oh: usize,
) -> Vec<u8> {
    let (cy, sy, cp, sp) = (
        yaw.to_radians().cos(),
        yaw.to_radians().sin(),
        pitch.to_radians().cos(),
        pitch.to_radians().sin(),
    );
    let f = [sy * cp, sp, -cy * cp];
    let r = [cy, 0.0, sy];
    let u = [-sy * sp, cp, cy * sp];
    let tan = (fov.to_radians() / 2.0).tan();
    let asp = ow as f64 / oh as f64;
    let mut out = vec![0u8; ow * oh * 3];
    for j in 0..oh {
        for i in 0..ow {
            let sx = (i as f64 + 0.5) / ow as f64 * 2.0 - 1.0;
            let syy = 1.0 - (j as f64 + 0.5) / oh as f64 * 2.0;
            let d = [0, 1, 2].map(|k| f[k] + r[k] * sx * asp * tan + u[k] * syy * tan);
            let (uu, vv) = pyramid::dir_to_uv([d[0] as f32, d[1] as f32, d[2] as f32]);
            let (x, y) = (
                uu as f64 * w as f64 - 0.5,
                (vv as f64 * h as f64 - 0.5).clamp(0.0, h as f64 - 1.0),
            );
            let (x0, y0) = (x.floor(), y.floor());
            let (ax, ay) = (x - x0, y - y0);
            let xi = (x0 as i64).rem_euclid(w as i64) as usize;
            let x1 = (xi + 1) % w;
            let yi = y0 as usize;
            let y1 = (yi + 1).min(h - 1);
            for c in 0..3 {
                let p = |xx: usize, yy: usize| px[(yy * w + xx) * 4 + c] as f64;
                let v = (p(xi, yi) * (1.0 - ax) + p(x1, yi) * ax) * (1.0 - ay)
                    + (p(xi, y1) * (1.0 - ax) + p(x1, y1) * ax) * ay;
                out[(j * ow + i) * 3 + c] = v.round() as u8;
            }
        }
    }
    out
}

fn save_jpeg(path: &std::path::Path, rgb: &[u8], w: usize, h: usize) {
    let mut c = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    c.set_size(w, h);
    c.set_quality(85.0);
    let mut c = c.start_compress(Vec::new()).unwrap();
    c.write_scanlines(rgb).unwrap();
    std::fs::write(path, c.finish().unwrap()).unwrap();
}
