//! Headless benchmark of the Rust side of spike S1 (decode, pyramid, tiles, disk cache).
//!
//! Usage: `cargo run --release --bin s1-bench -- [--zune] [--no-cache] <file>…`
//! Without files, all images under PANO_TESTDATA are measured.

use s1_tile_viewer_lib::{cache, decode, pyramid, session};
use std::path::PathBuf;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zune = args.iter().any(|a| a == "--zune");
    let no_cache = args.iter().any(|a| a == "--no-cache");
    let mut files: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
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
    println!("probe            {:>8.1} ms   {}×{} {:?} {:?} ({:.0} MP)", ms(t), p.width, p.height, p.kind, p.storage,
        p.width as f64 * p.height as f64 / 1e6);

    if p.kind == decode::Kind::Jpeg {
        let t = Instant::now();
        let q = decode::jpeg_quick(path)?;
        let tq = ms(t);
        let t = Instant::now();
        let _b = pyramid::Base::build(&q);
        println!("jpeg 1/8 decode  {:>8.1} ms   ({}×{}), base resample {:.1} ms", tq, q.width, q.height, ms(t));
    }

    let dec = if zune { decode::JpegDecoder::Zune } else { decode::JpegDecoder::Turbo };
    let t = Instant::now();
    let l0 = decode::decode_full(path, p.kind, dec)?;
    println!("full decode      {:>8.1} ms   ({dec:?})", ms(t));
    let t = Instant::now();
    let pyr = pyramid::Pyramid::build(l0);
    println!("pyramid          {:>8.1} ms   levels {:?}", ms(t), pyr.levels.iter().map(|l| l.width).collect::<Vec<_>>());
    let t = Instant::now();
    let base = pyramid::Base::build(pyr.levels.last().unwrap());
    println!("base texture     {:>8.1} ms", ms(t));

    let infos = pyramid::level_infos(p.width, p.height);
    let l0i = &infos[0];
    let n = (l0i.tiles_x * l0i.tiles_y) as usize;
    for wire in [pyramid::Wire::U8, pyramid::Wire::F16] {
        let t = Instant::now();
        let mut bytes = 0usize;
        for ty in 0..l0i.tiles_y {
            for tx in 0..l0i.tiles_x {
                let raw = pyr.equirect_tile_raw(0, tx, ty).unwrap();
                bytes += pyramid::to_wire(p.storage, raw, wire).len();
            }
        }
        let el = ms(t);
        println!("tile L0 {:?}      {:>8.3} ms/tile ({n} tiles, {:.2} MB/tile, single thread)", wire, el / n as f64,
            bytes as f64 / n as f64 / 1e6);
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
    println!("cube tile L0     {:>8.3} ms/tile (on-demand resample, single thread, face {}²)", ms(t) / k as f64, cl[0].n);

    if !no_cache {
        let dir = cache::root().join("bench");
        let t = Instant::now();
        let bytes = cache::write(&dir, &pyr, &base, &infos)?;
        println!("cache write      {:>8.1} ms   {:.0} MB on disk", ms(t), bytes as f64 / 1e6);
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
        println!("cache tile L0 u8 {:>8.3} ms/tile (read + lz4 + convert, single thread)", ms(t) / n as f64);
        let _ = std::fs::remove_dir_all(&dir);
    }
    println!("peak RSS so far  {:>8.0} MB", session::peak_rss_mb());
    Ok(())
}
