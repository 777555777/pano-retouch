//! Spike S4: lossless metadata round trip.
//!
//!   s4-meta rt  <in.jpg|tif> <out.jpg|tif> [--half] [--heading DEG] [--srgb lcms|mox]
//!               [--xmp toolkit|text] [--exif own|little] [--bigtiff|--classic] [--software S]
//!   s4-meta exr <in.exr> <out.exr>
//!   s4-meta icc <in.tif|jpg>            compare lcms2 and moxcms (sRGB conversion)
//!   s4-meta pixeq <a.tif> <b.tif>       pixel equality of two TIFFs
//!   s4-meta synth <dir>                 synthetic 4096x2048 JPEG + 16-bit RGBA TIFF without metadata

mod icc;
mod ifd;
mod meta;
mod tif;
mod xmp;

use std::time::Instant;

use meta::Meta;

pub enum Px {
    U8(Vec<u8>),
    U16(Vec<u16>),
}

pub struct Image {
    pub w: u32,
    pub h: u32,
    pub ch: usize,
    pub px: Px,
}

impl Image {
    fn to_u16(&mut self) {
        if let Px::U8(v) = &self.px {
            self.px = Px::U16(v.iter().map(|&x| u16::from(x) * 257).collect());
        }
    }
    fn to_u8(&mut self) {
        if let Px::U16(v) = &self.px {
            self.px = Px::U8(
                v.iter()
                    .map(|&x| ((u32::from(x) + 128) / 257) as u8)
                    .collect(),
            );
        }
    }
    fn drop_alpha(&mut self) {
        if self.ch != 4 {
            return;
        }
        fn rgb<T: Copy>(v: &[T]) -> Vec<T> {
            v.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
        }
        self.px = match &self.px {
            Px::U8(v) => Px::U8(rgb(v)),
            Px::U16(v) => Px::U16(rgb(v)),
        };
        self.ch = 3;
    }
    /// 2×2 box filter (stand-in for an export preset that changes the size).
    fn half(&mut self) {
        let (w, h, ch) = (self.w as usize / 2, self.h as usize / 2, self.ch);
        let sw = self.w as usize;
        fn go<T: Copy + Into<u32>>(
            v: &[T],
            sw: usize,
            w: usize,
            h: usize,
            ch: usize,
            from: impl Fn(u32) -> T,
        ) -> Vec<T> {
            let mut out = Vec::with_capacity(w * h * ch);
            for y in 0..h {
                for x in 0..w {
                    for c in 0..ch {
                        let at = |dx: usize, dy: usize| {
                            v[((2 * y + dy) * sw + 2 * x + dx) * ch + c].into()
                        };
                        out.push(from((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4));
                    }
                }
            }
            out
        }
        self.px = match &self.px {
            Px::U8(v) => Px::U8(go(v, sw, w, h, ch, |x| x as u8)),
            Px::U16(v) => Px::U16(go(v, sw, w, h, ch, |x| x as u16)),
        };
        self.w = w as u32;
        self.h = h as u32;
    }
}

struct Opts {
    half: bool,
    heading: Option<f64>,
    srgb: Option<icc::Cms>,
    xmp: xmp::Engine,
    little_exif: bool,
    bigtiff: bool,
    classic: bool,
    software: String,
}

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let r = match args.get(1).map(String::as_str) {
        Some("rt") => {
            let o = Opts {
                half: args.iter().any(|a| a == "--half"),
                heading: arg(&args, "--heading").map(|s| s.parse().expect("heading")),
                srgb: arg(&args, "--srgb").map(|s| {
                    if s == "mox" {
                        icc::Cms::Mox
                    } else {
                        icc::Cms::Lcms
                    }
                }),
                xmp: if arg(&args, "--xmp") == Some("text") {
                    xmp::Engine::Text
                } else {
                    xmp::Engine::Toolkit
                },
                little_exif: arg(&args, "--exif") == Some("little"),
                bigtiff: args.iter().any(|a| a == "--bigtiff"),
                classic: args.iter().any(|a| a == "--classic"),
                software: arg(&args, "--software")
                    .unwrap_or("pano-retouch 0.0.0 (spike S4)")
                    .to_owned(),
            };
            round_trip(&args[2], &args[3], &o)
        }
        Some("exr") => exr_round_trip(&args[2], &args[3]),
        Some("icc") => icc_compare(&args[2]),
        Some("pixeq") => pixeq(&args[2], &args[3]),
        Some("synth") => synth(&args[2]),
        _ => Err("usage: s4-meta rt|exr|icc …".into()),
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn is_jpeg(p: &str) -> bool {
    let p = p.to_ascii_lowercase();
    p.ends_with(".jpg") || p.ends_with(".jpeg")
}

fn decode_jpeg(b: &[u8]) -> Result<Image, String> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let opts = DecoderOptions::new_fast()
        .jpeg_set_out_colorspace(ColorSpace::RGB)
        .set_max_width(1 << 16)
        .set_max_height(1 << 16);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(
        zune_jpeg::zune_core::bytestream::ZCursor::new(b),
        opts,
    );
    let px = d.decode().map_err(|e| format!("{e:?}"))?;
    let info = d.info().ok_or("no info")?;
    Ok(Image {
        w: info.width as u32,
        h: info.height as u32,
        ch: 3,
        px: Px::U8(px),
    })
}

fn encode_jpeg(img: &Image) -> Result<Vec<u8>, String> {
    let Px::U8(v) = &img.px else { unreachable!() };
    let mut out = Vec::new();
    let mut e = jpeg_encoder::Encoder::new(&mut out, 95);
    e.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
    e.encode(v, img.w as u16, img.h as u16, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn round_trip(input: &str, output: &str, o: &Opts) -> Result<(), String> {
    let t0 = Instant::now();
    let bytes = std::fs::read(input).map_err(|e| e.to_string())?;
    let (mut img, mut m, layout) = if is_jpeg(input) {
        (decode_jpeg(&bytes)?, meta::read_jpeg(&bytes), None)
    } else {
        let (i, m, l) = tif::read(&bytes)?;
        (i, m, Some(l))
    };
    let t_read = t0.elapsed();
    let src = (img.w, img.h);
    for n in &m.notes {
        println!("note: {n}");
    }
    if m.has_makernote() {
        println!("note: MakerNote present – relocating it may break maker-specific offsets");
    }

    // Pixels.
    let t1 = Instant::now();
    if o.half {
        img.half();
    }
    if let Some(cms) = o.srgb {
        let icc = m.icc.clone().ok_or("no ICC profile to convert from")?;
        img.to_u16();
        let Px::U16(v) = &mut img.px else {
            unreachable!()
        };
        icc::to_srgb(v, img.ch, &icc, cms)?;
        m.icc = Some(icc::srgb_profile());
    }
    let t_px = t1.elapsed();

    // Metadata edits.
    let t2 = Instant::now();
    m.set_software(&o.software);
    m.set_pixel_dims(img.w, img.h);
    if let Some(x) = m.xmp.take() {
        let sets = xmp::export_sets(&x, src, (img.w, img.h), o.heading);
        m.xmp = Some(xmp::apply(&x, &sets, o.xmp)?);
    } else if o.heading.is_some() {
        println!("note: no XMP packet – heading not written");
    }
    let t_meta = t2.elapsed();

    let t3 = Instant::now();
    let out = if is_jpeg(output) {
        img.drop_alpha();
        img.to_u8();
        let enc = encode_jpeg(&img)?;
        let exif = if o.little_exif { None } else { m.exif_block() };
        let segs = meta::jpeg_segments(&m, exif)?;
        let mut out = meta::splice_jpeg(&enc, &segs);
        if o.little_exif {
            out = little_exif_write(&bytes, input, out, &m, &o.software, (img.w, img.h))?;
        }
        out
    } else {
        let layout = layout.ok_or("JPEG → TIFF not handled by the spike")?;
        tif::write(&img, &m, &layout, o.bigtiff || (layout.big && !o.classic))?
    };
    let t_write = t3.elapsed();
    std::fs::write(output, &out).map_err(|e| e.to_string())?;
    println!(
        "{}x{} -> {}x{}  read+decode {:.2}s  pixels {:.2}s  metadata edits {:.1}ms  encode+metadata {:.2}s  total {:.2}s  ({} MB)",
        src.0,
        src.1,
        img.w,
        img.h,
        t_read.as_secs_f64(),
        t_px.as_secs_f64(),
        t_meta.as_secs_f64() * 1e3,
        t_write.as_secs_f64(),
        t0.elapsed().as_secs_f64(),
        out.len() >> 20
    );
    Ok(())
}

/// Alternative EXIF engine: little_exif reads the source's EXIF and writes it into the new file.
fn little_exif_write(
    src: &[u8],
    input: &str,
    mut out: Vec<u8>,
    _m: &Meta,
    software: &str,
    dims: (u32, u32),
) -> Result<Vec<u8>, String> {
    use little_exif::exif_tag::ExifTag;
    use little_exif::filetype::FileExtension;
    use little_exif::metadata::Metadata;
    let ext = if is_jpeg(input) {
        FileExtension::JPEG
    } else {
        FileExtension::TIFF
    };
    let mut md =
        Metadata::new_from_vec(&src.to_vec(), ext).map_err(|e| format!("little_exif read: {e}"))?;
    md.set_tag(ExifTag::Software(software.to_owned()));
    md.set_tag(ExifTag::ExifImageWidth(vec![dims.0]));
    md.set_tag(ExifTag::ExifImageHeight(vec![dims.1]));
    md.write_to_vec(&mut out, FileExtension::JPEG)
        .map_err(|e| format!("little_exif write: {e}"))?;
    Ok(out)
}

fn exr_round_trip(input: &str, output: &str) -> Result<(), String> {
    use exr::prelude::*;
    let t0 = Instant::now();
    let mut img = read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .all_layers()
        .all_attributes()
        .from_file(input)
        .map_err(|e| e.to_string())?;
    let t_read = t0.elapsed();
    println!("image attributes: {:?}", img.attributes);
    for l in &img.layer_data {
        println!(
            "layer {:?} {:?} encoding {:?}",
            l.attributes.layer_name, l.size, l.encoding
        );
        println!("layer attributes: {:?}", l.attributes);
        let names: Vec<_> = l
            .channel_data
            .list
            .iter()
            .map(|c| c.name.to_string())
            .collect();
        println!("channels: {names:?}");
    }
    for l in &mut img.layer_data {
        l.attributes.software_name = Some(Text::from("pano-retouch 0.0.0 (spike S4)"));
    }
    let t1 = Instant::now();
    img.write().to_file(output).map_err(|e| e.to_string())?;
    println!(
        "read {:.2}s  write {:.2}s",
        t_read.as_secs_f64(),
        t1.elapsed().as_secs_f64()
    );
    Ok(())
}

fn icc_compare(input: &str) -> Result<(), String> {
    let bytes = std::fs::read(input).map_err(|e| e.to_string())?;
    let (mut img, m) = if is_jpeg(input) {
        (decode_jpeg(&bytes)?, meta::read_jpeg(&bytes))
    } else {
        let (i, m, _) = tif::read(&bytes)?;
        (i, m)
    };
    let icc = m.icc.ok_or("no ICC profile")?;
    println!(
        "profile: {} ({} bytes), {}x{}x{}",
        icc::description(&icc),
        icc.len(),
        img.w,
        img.h,
        img.ch
    );
    img.to_u16();
    let Px::U16(base) = &img.px else {
        unreachable!()
    };
    let mut res = Vec::new();
    for cms in [icc::Cms::Lcms, icc::Cms::Mox] {
        let mut v = base.clone();
        let t = Instant::now();
        icc::to_srgb(&mut v, img.ch, &icc, cms)?;
        let dt = t.elapsed().as_secs_f64();
        println!(
            "{cms:?}: {:.2}s = {:.0} MP/s",
            dt,
            f64::from(img.w) * f64::from(img.h) / dt / 1e6
        );
        res.push(v);
    }
    // Sampled pixels against a double-precision reference (colour channels only).
    let step = ((img.w as usize * img.h as usize) / 1_000_000).max(1);
    let pick = |v: &[u16]| -> Vec<u16> {
        v.chunks_exact(img.ch)
            .step_by(step)
            .flatten()
            .copied()
            .collect()
    };
    let base_s = pick(base);
    let refv = icc::reference(&base_s, img.ch, &icc)?;
    for (name, r) in ["lcms2 16-bit", "moxcms 16-bit"].iter().zip(&res) {
        let s = pick(r);
        let (mut max, mut sum, mut max8, mut n8, mut n) = (0f64, 0f64, 0u32, 0u64, 0u64);
        for (k, px) in s.chunks_exact(img.ch).enumerate() {
            for c in 0..3 {
                let want = refv[k * 3 + c];
                let d = (f64::from(px[c]) - want).abs();
                max = max.max(d);
                sum += d;
                let d8 = ((u32::from(px[c]) + 128) / 257).abs_diff((want / 257.0).round() as u32);
                max8 = max8.max(d8);
                n8 += u64::from(d8 > 0);
                n += 1;
            }
        }
        println!(
            "{name} vs double reference: 16-bit max {max:.0} mean {:.2}; 8-bit max {max8}, {:.3}% of samples off ({} px sampled)",
            sum / n as f64,
            100.0 * n8 as f64 / n as f64,
            n / 3
        );
    }
    Ok(())
}

fn pixeq(a: &str, b: &str) -> Result<(), String> {
    let load = |p: &str| -> Result<Image, String> {
        Ok(tif::read(&std::fs::read(p).map_err(|e| e.to_string())?)?.0)
    };
    let (x, y) = (load(a)?, load(b)?);
    let same = (x.w, x.h, x.ch) == (y.w, y.h, y.ch)
        && match (&x.px, &y.px) {
            (Px::U8(p), Px::U8(q)) => p == q,
            (Px::U16(p), Px::U16(q)) => p == q,
            _ => false,
        };
    println!(
        "{}",
        if same {
            "pixels identical"
        } else {
            "PIXELS DIFFER"
        }
    );
    Ok(())
}

fn synth(dir: &str) -> Result<(), String> {
    let (w, h) = (4096u32, 2048u32);
    let mut v16 = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let check = ((x / 256 + y / 256) % 2) as u16;
            v16.extend_from_slice(&[
                (x * 16) as u16,
                (y * 32) as u16,
                20000 + check * 20000,
                if y > h * 15 / 16 { 0 } else { 65535 },
            ]);
        }
    }
    let mut img = Image {
        w,
        h,
        ch: 4,
        px: Px::U16(v16),
    };
    let layout = tif::Layout {
        compression: 5,
        predictor: 2,
        rows_per_strip: Some(64),
        big: false,
    };
    std::fs::write(
        format!("{dir}/synthetic.tif"),
        tif::write(&img, &Meta::default(), &layout, false)?,
    )
    .map_err(|e| e.to_string())?;
    img.drop_alpha();
    img.to_u8();
    std::fs::write(format!("{dir}/synthetic.jpg"), encode_jpeg(&img)?)
        .map_err(|e| e.to_string())?;
    Ok(())
}
