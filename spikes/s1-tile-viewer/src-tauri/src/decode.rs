//! Decoders for the spike: JPEG (libjpeg-turbo via mozjpeg, or zune-jpeg), striped TIFF (parallel
//! per strip) and EXR (exr crate, parallel block decompression). All produce RGBA level 0.

use crate::pyramid::{Level, Pixels, Storage};
use half::f16;
use rayon::prelude::*;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Jpeg,
    Tiff,
    Exr,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some(Kind::Jpeg),
        "tif" | "tiff" => Some(Kind::Tiff),
        "exr" => Some(Kind::Exr),
        _ => None,
    }
}

pub struct Probe {
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    pub storage: Storage,
}

pub fn probe(path: &Path) -> Result<Probe> {
    let kind = kind_of(path).ok_or("unsupported file type")?;
    match kind {
        Kind::Jpeg => {
            let d = mozjpeg::Decompress::new_path(path).map_err(|e| e.to_string())?;
            Ok(Probe { kind, width: d.width() as u32, height: d.height() as u32, storage: Storage::U8 })
        }
        Kind::Tiff => {
            let mut d = tiff_decoder(path)?;
            let (width, height) = d.dimensions().map_err(|e| e.to_string())?;
            let storage = match d.colortype().map_err(|e| e.to_string())? {
                tiff::ColorType::RGB(8) | tiff::ColorType::RGBA(8) => Storage::U8,
                tiff::ColorType::RGB(16) | tiff::ColorType::RGBA(16) => Storage::U16,
                c => return Err(format!("unsupported TIFF colour type {c:?}")),
            };
            Ok(Probe { kind, width, height, storage })
        }
        Kind::Exr => {
            let meta = exr::meta::MetaData::read_from_file(path, false).map_err(|e| e.to_string())?;
            let size = meta.headers.first().ok_or("no EXR layer")?.layer_size;
            Ok(Probe { kind, width: size.width() as u32, height: size.height() as u32, storage: Storage::F16 })
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JpegDecoder {
    Turbo,
    Zune,
}

pub fn decode_full(path: &Path, kind: Kind, jpeg: JpegDecoder) -> Result<Level> {
    match (kind, jpeg) {
        (Kind::Jpeg, JpegDecoder::Turbo) => jpeg_turbo(path, 8),
        (Kind::Jpeg, JpegDecoder::Zune) => jpeg_zune(path),
        (Kind::Tiff, _) => tiff_parallel(path),
        (Kind::Exr, _) => exr_rgba(path),
    }
}

/// Fast preview: libjpeg-turbo DCT scaling (`scale_num/8`), only entropy decoding is full cost.
pub fn jpeg_quick(path: &Path) -> Result<Level> {
    jpeg_turbo(path, 1)
}

fn jpeg_turbo(path: &Path, scale_num: u8) -> Result<Level> {
    let mut d = mozjpeg::Decompress::new_path(path).map_err(|e| e.to_string())?;
    d.scale(scale_num);
    if scale_num < 8 {
        d.do_fancy_upsampling(false);
    }
    let mut s = d.rgba().map_err(|e| e.to_string())?;
    let (w, h) = (s.width(), s.height());
    let mut buf = vec![0u8; w * h * 4];
    s.read_scanlines_into(&mut buf).map_err(|e| e.to_string())?;
    s.finish().map_err(|e| e.to_string())?;
    Ok(Level { width: w as u32, height: h as u32, px: Pixels::U8(buf) })
}

fn jpeg_zune(path: &Path) -> Result<Level> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let opts = DecoderOptions::new_fast()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(1 << 16)
        .set_max_height(1 << 16);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(zune_jpeg::zune_core::bytestream::ZCursor::new(&data), opts);
    let px = d.decode().map_err(|e| format!("{e:?}"))?;
    let info = d.info().ok_or("no info")?;
    Ok(Level { width: info.width as u32, height: info.height as u32, px: Pixels::U8(px) })
}

fn tiff_decoder(path: &Path) -> Result<tiff::decoder::Decoder<BufReader<File>>> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    Ok(tiff::decoder::Decoder::new(BufReader::new(f))
        .map_err(|e| e.to_string())?
        .with_limits(tiff::decoder::Limits::unlimited()))
}

/// Strips are independent: each rayon worker opens its own decoder and decodes whole strips
/// straight into the matching row range of the output.
fn tiff_parallel(path: &Path) -> Result<Level> {
    let mut d = tiff_decoder(path)?;
    let (w, h) = d.dimensions().map_err(|e| e.to_string())?;
    let ct = d.colortype().map_err(|e| e.to_string())?;
    if d.get_chunk_type() != tiff::decoder::ChunkType::Strip {
        return Err("spike only supports striped TIFF".into());
    }
    let (spp, bits) = match ct {
        tiff::ColorType::RGB(b) => (3usize, b),
        tiff::ColorType::RGBA(b) => (4usize, b),
        c => return Err(format!("unsupported TIFF colour type {c:?}")),
    };
    let rows_per_strip = d.chunk_dimensions().1 as usize;
    let (w, h) = (w as usize, h as usize);
    drop(d);

    fn run<T: crate::pyramid::Sample>(
        path: &Path,
        w: usize,
        h: usize,
        spp: usize,
        rows: usize,
        opaque: T,
    ) -> Result<Vec<T>> {
        let mut out = vec![T::default(); w * h * 4];
        out.par_chunks_mut(rows * w * 4)
            .enumerate()
            .try_for_each_init(
                || (tiff_decoder(path), Vec::<T>::new()),
                |(dec, buf), (i, dst)| -> Result<()> {
                    let dec = dec.as_mut().map_err(|e| e.clone())?;
                    let n = dst.len() / 4;
                    buf.resize(n * spp, T::default());
                    dec.read_chunk_bytes(i as u32, bytemuck::cast_slice_mut(&mut buf[..]))
                        .map_err(|e| e.to_string())?;
                    if spp == 4 {
                        dst.copy_from_slice(&buf[..n * 4]);
                    } else {
                        for p in 0..n {
                            dst[p * 4..p * 4 + 3].copy_from_slice(&buf[p * 3..p * 3 + 3]);
                            dst[p * 4 + 3] = opaque;
                        }
                    }
                    Ok(())
                },
            )?;
        Ok(out)
    }

    let px = match bits {
        8 => Pixels::U8(run::<u8>(path, w, h, spp, rows_per_strip, 255)?),
        16 => Pixels::U16(run::<u16>(path, w, h, spp, rows_per_strip, 65535)?),
        b => return Err(format!("unsupported bit depth {b}")),
    };
    Ok(Level { width: w as u32, height: h as u32, px })
}

fn exr_rgba(path: &Path) -> Result<Level> {
    let img = exr::prelude::read_first_rgba_layer_from_file(
        path,
        |res, _| (res.width(), vec![f16::ZERO; res.width() * res.height() * 4]),
        |(w, v): &mut (usize, Vec<f16>), pos, (r, g, b, a): (f16, f16, f16, f16)| {
            let i = (pos.y() * *w + pos.x()) * 4;
            v[i] = r;
            v[i + 1] = g;
            v[i + 2] = b;
            v[i + 3] = a;
        },
    )
    .map_err(|e| e.to_string())?;
    let size = img.layer_data.size;
    let (_, px) = img.layer_data.channel_data.pixels;
    Ok(Level { width: size.width() as u32, height: size.height() as u32, px: Pixels::F16(px) })
}
