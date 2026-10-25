//! Spike S3 – inpaint the nadir (or any cap) of a panorama with LaMa, MI-GAN, PatchMatch or LaMa +
//! PatchMatch and write review images (local only – never commit results of private images).
//!
//! Usage: s3-inpaint <image> [--cap 20] [--center yaw,pitch] [--alpha] [--methods lama,migan,pm,lama+pm]
//!                   [--provider cpu|coreml|directml] [--pm-res 1536] [--max-n 4096]
//!                   [--models testdata-local/_models] [--out testdata-local/_s3/<name>]

use s1_tile_viewer_lib::ai::{self, ModelKind, Provider};
use s1_tile_viewer_lib::edit::dir;
use s1_tile_viewer_lib::inpaint::{self, patchmatch, Gnomonic, Img, ToneMap};
use s1_tile_viewer_lib::pyramid::{Level, Pixels, Storage};
use s1_tile_viewer_lib::{decode, session};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(|s| s.as_str())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with("--") && !args.iter().any(|b| b.starts_with("--") && args.iter().position(|x| x == b).map(|i| args.get(i + 1)) == Some(Some(a)))) else {
        eprintln!("usage: s3-inpaint <image> [--cap deg] [--alpha] [--methods lama,migan,pm,lama+pm] [--provider cpu|coreml|directml]");
        std::process::exit(2);
    };
    let path = PathBuf::from(path);
    let root = s1_tile_viewer_lib::testdata_root();
    let cap: f64 = arg(&args, "--cap").and_then(|v| v.parse().ok()).unwrap_or(20.0);
    let alpha = args.iter().any(|a| a == "--alpha");
    let methods: Vec<String> = arg(&args, "--methods").unwrap_or("lama,migan,pm,lama+pm").split(',').map(String::from).collect();
    let provider = arg(&args, "--provider").and_then(Provider::parse).unwrap_or(if cfg!(target_os = "macos") { Provider::CoreMl } else if cfg!(windows) { Provider::DirectMl } else { Provider::Cpu });
    let pm_res: usize = arg(&args, "--pm-res").and_then(|v| v.parse().ok()).unwrap_or(1536);
    let max_n: usize = arg(&args, "--max-n").and_then(|v| v.parse().ok()).unwrap_or(4096);
    let models = arg(&args, "--models").map(PathBuf::from).unwrap_or_else(|| root.join("_models"));
    let center = arg(&args, "--center")
        .and_then(|v| v.split_once(',').map(|(a, b)| (a.parse().unwrap_or(0.0), b.parse().unwrap_or(-90.0))))
        .unwrap_or((0.0, -90.0));
    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string()).replace([':', ' '], "_");
    let out = arg(&args, "--out").map(PathBuf::from).unwrap_or_else(|| root.join("_s3").join(&name));
    std::fs::create_dir_all(&out).unwrap();

    println!("## {}  cap {cap}° at ({}, {}), alpha hole: {alpha}, provider {provider:?}", path.display(), center.0, center.1);
    let t = Instant::now();
    let p = decode::probe(&path).unwrap();
    let l0 = decode::decode_full(&path, p.kind, decode::JpegDecoder::Turbo).unwrap();
    println!("decode            {:8.0} ms  {}×{} {:?}", ms(t), l0.width, l0.height, p.storage);
    let hdr = p.storage == Storage::F16;

    // gnomonic view around the cap with level-0 density at its centre
    let c = dir(center.0, center.1);
    let half_fov = (cap * 1.4 + 3.0).min(75.0);
    let n_full = (2.0 * half_fov.to_radians().tan() * l0.width as f64 / std::f64::consts::TAU).round() as usize;
    let n = n_full.min(max_n);
    let g = Gnomonic::new(c, half_fov, n);
    let t = Instant::now();
    let view = inpaint::render(&l0, &g);
    println!("render view       {:8.0} ms  {n}×{n} px (level-0 density would be {n_full}), half FOV {half_fov:.1}°", ms(t));

    let t = Instant::now();
    let hole = inpaint::hole_mask(&view, &g, c, Some(cap), alpha, 0);
    let feather = (n as f32 * 0.015).max(4.0);
    let dist = inpaint::distance_outside(&hole, n, n);
    let fill: Vec<bool> = dist.iter().map(|&d| d <= feather).collect();
    let weight = inpaint::feather_weight(&hole, n, n, feather);
    let tm = ToneMap::for_view(&view, &fill, hdr);
    let disp = inpaint::to_display(&view, tm);
    let hole_px = fill.iter().filter(|&&b| b).count();
    println!("mask + tone map   {:8.0} ms  fill region {:.1} MP ({:.0} % of the view), feather {feather:.0} px, exposure {:.3}",
        ms(t), hole_px as f64 / 1e6, hole_px as f64 / (n * n) as f64 * 100.0, tm.exposure);
    save(&out.join("view-before.jpg"), &disp.resize(1600, 1600));
    save(&out.join("mask.jpg"), &overlay(&disp, &fill, &hole).resize(1600, 1600));

    let mut report = vec![];
    for m in &methods {
        let t_all = Instant::now();
        let refine_tex = m.ends_with("+tex");
        let m = &m.trim_end_matches("+tex").to_string();
        let label = if refine_tex { format!("{m}+tex") } else { m.clone() };
        let mut info = serde_json::json!({ "method": label, "n": n });
        let filled_small: Result<Img, String> = (|| {
            Ok(match m.as_str() {
                "lama" | "lama+pm" => {
                    let mut model = ai::load(&models.join("lama_fp32.onnx"), ModelKind::Lama, provider)?;
                    info["load_ms"] = model.load_ms.into();
                    info["ep"] = model.used.clone().into();
                    let small = disp.resize(512, 512);
                    let ms512 = inpaint::resize_mask(&fill, n, n, 512, 512);
                    let (r, t1) = model.run(&small, &ms512)?;
                    // a second run shows the warm (cached) inference time
                    let (r, t2) = model.run(&small, &ms512).map(|(_, t2)| (r, t2))?;
                    info["infer_ms_first"] = t1.into();
                    info["infer_ms"] = t2.into();
                    if m == "lama" {
                        r
                    } else {
                        let t = Instant::now();
                        let sm = disp.resize(pm_res, pm_res);
                        let mk = inpaint::resize_mask(&fill, n, n, pm_res, pm_res);
                        let guide = r.resize(pm_res, pm_res);
                        let out = patchmatch::complete(&sm, &mk, Some(&guide), 4, 2);
                        info["pm_ms"] = ms(t).into();
                        info["pm_res"] = pm_res.into();
                        out
                    }
                }
                "migan" => {
                    let mut model = ai::load(&models.join("migan_pipeline_v2.onnx"), ModelKind::Migan, provider)?;
                    info["load_ms"] = model.load_ms.into();
                    info["ep"] = model.used.clone().into();
                    let nm = n.min(2048);
                    let small = disp.resize(nm, nm);
                    let mk = inpaint::resize_mask(&fill, n, n, nm, nm);
                    let (r, t1) = model.run(&small, &mk)?;
                    let (_, t2) = model.run(&small, &mk)?;
                    info["infer_ms_first"] = t1.into();
                    info["infer_ms"] = t2.into();
                    info["model_input"] = nm.into();
                    r
                }
                "pm" => {
                    let t = Instant::now();
                    let sm = disp.resize(pm_res, pm_res);
                    let mk = inpaint::resize_mask(&fill, n, n, pm_res, pm_res);
                    let out = patchmatch::complete(&sm, &mk, None, 6, 0);
                    info["pm_ms"] = ms(t).into();
                    info["pm_res"] = pm_res.into();
                    out
                }
                other => return Err(format!("unknown method {other}")),
            })
        })();
        let small = match filled_small {
            Ok(s) => s,
            Err(e) => {
                println!("{label:10} ERROR {e}");
                info["error"] = e.into();
                report.push(info);
                continue;
            }
        };
        let t = Instant::now();
        let mut up = small.resize(n, n);
        // "+tex": add real full-resolution texture from the surroundings (see patchmatch::texture_refine)
        if refine_tex {
            let t = Instant::now();
            // the anchor was upscaled from `small.w` px → its blur is about n / small.w pixels
            let sigma = (n as f32 / small.w as f32).max(1.5);
            up = patchmatch::texture_refine(&disp, &fill, &up, sigma, 3);
            info["tex_sigma"] = sigma.into();
            info["tex_ms"] = ms(t).into();
        }
        let filled = inpaint::composite(&disp, &up, &weight);
        info["compose_ms"] = ms(t).into();
        save(&out.join(format!("{label}-view.jpg")), &filled.resize(1600, 1600));
        save(&out.join(format!("{label}-full.jpg")), &filled);

        // colour/brightness of the fill vs. a ring of known context (HDR: in linear light)
        let lin = |p: [f32; 4]| [tm.inv(p[0]), tm.inv(p[1]), tm.inv(p[2])];
        let (mut a, mut na, mut b, mut nb) = ([0f64; 3], 0f64, [0f64; 3], 0f64);
        for (i, &d) in dist.iter().enumerate() {
            if hole[i] {
                let v = lin(filled.px[i]);
                for k in 0..3 { a[k] += v[k] as f64; }
                na += 1.0;
            } else if d > feather && d < feather * 4.0 {
                let v = lin(filled.px[i]);
                for k in 0..3 { b[k] += v[k] as f64; }
                nb += 1.0;
            }
        }
        let ratio: Vec<f64> = (0..3).map(|k| ((a[k] / na.max(1.0)) / (b[k] / nb.max(1.0)).max(1e-9) * 1000.0).round() / 1000.0).collect();
        info["fill_vs_context_rgb"] = ratio.clone().into();

        // reproject into a copy of level 0 and check that nothing outside the blend region changed
        let t = Instant::now();
        let mut l1 = clone_level(&l0);
        let changed = inpaint::reproject(&mut l1, &g, &filled, &weight, tm);
        info["reproject_ms"] = ms(t).into();
        let differing = count_diff(&l0, &l1);
        info["changed_px"] = changed.into();
        info["differing_px"] = differing.into();
        let check = Gnomonic::new(c, half_fov, 1600);
        save(&out.join(format!("{label}-reprojected.jpg")), &inpaint::to_display(&inpaint::render(&l1, &check), tm));
        info["total_ms"] = ms(t_all).into();
        println!("{label:10} {}", info);
        report.push(info);
    }
    let rss = session::peak_rss_mb();
    let summary = serde_json::json!({ "image": name, "cap_deg": cap, "alpha": alpha, "provider": format!("{provider:?}"), "view_px": n, "peak_rss_mb": rss, "methods": report });
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&summary).unwrap()).unwrap();
    println!("peak RSS {rss:.0} MB – review images in {}", out.display());
}

fn clone_level(l: &Level) -> Level {
    let px = match &l.px {
        Pixels::U8(v) => Pixels::U8(v.clone()),
        Pixels::U16(v) => Pixels::U16(v.clone()),
        Pixels::F16(v) => Pixels::F16(v.clone()),
    };
    Level { width: l.width, height: l.height, px }
}

/// Number of pixels whose stored value differs (bit-exact comparison).
fn count_diff(a: &Level, b: &Level) -> usize {
    let (x, y) = (a.px.as_bytes(), b.px.as_bytes());
    let bpp = x.len() / (a.width as usize * a.height as usize);
    x.chunks(bpp).zip(y.chunks(bpp)).filter(|(p, q)| p != q).count()
}

fn overlay(img: &Img, fill: &[bool], hole: &[bool]) -> Img {
    let mut o = img.clone();
    for (i, p) in o.px.iter_mut().enumerate() {
        if hole[i] {
            *p = [p[0] * 0.5 + 0.5, p[1] * 0.5, p[2] * 0.5, 1.0];
        } else if fill[i] {
            *p = [p[0] * 0.6 + 0.4, p[1] * 0.6 + 0.4, p[2] * 0.6, 1.0];
        }
    }
    o
}

fn save(path: &Path, img: &Img) {
    let rgb: Vec<u8> = img.px.iter().flat_map(|p| {
        // composite over a mid grey where alpha < 1 (PTGui holes)
        let a = p[3].clamp(0.0, 1.0);
        [0, 1, 2].map(|k| ((p[k] + (1.0 - a) * 0.4).clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
    }).collect();
    let mut c = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    c.set_size(img.w, img.h);
    c.set_quality(90.0);
    let mut c = c.start_compress(Vec::new()).unwrap();
    c.write_scanlines(&rgb).unwrap();
    std::fs::write(path, c.finish().unwrap()).unwrap();
}
