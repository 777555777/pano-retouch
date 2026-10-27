//! TIFF / BigTIFF: pixels via the `tiff` crate, metadata via our raw directory model.

use std::io::{Cursor, Seek, Write};

use tiff::Directory;
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::encoder::{
    Compression, DirectoryEncoder, Predictor, TiffEncoder, TiffKind, colortype,
    compression::DeflateLevel,
};
use tiff::tags::{Tag, Type};

use crate::ifd::{self, Field, Ifd};
use crate::meta::*;
use crate::{Image, Px};

#[derive(Clone, Debug)]
pub struct Layout {
    pub compression: u16,
    pub predictor: u16,
    pub rows_per_strip: Option<u32>,
    pub big: bool,
}

pub fn read(b: &[u8]) -> Result<(Image, Meta, Layout), String> {
    let blk = ifd::parse(b, 1).ok_or("not a TIFF")?;
    let i0 = blk.chain.into_iter().next().ok_or("no IFD0")?;
    let uint = |t: u16| {
        i0.get(t).and_then(|f| match f.ty {
            3 => Some(u32::from(u16::from_le_bytes([f.data[0], f.data[1]]))),
            4 => Some(u32::from_le_bytes(f.data[..4].try_into().ok()?)),
            _ => None,
        })
    };
    if !blk.le {
        return Err("big-endian TIFF not handled by the spike".into());
    }
    let layout = Layout {
        compression: uint(259).unwrap_or(1) as u16,
        predictor: uint(317).unwrap_or(1) as u16,
        rows_per_strip: uint(278),
        big: blk.big,
    };
    let mut m = Meta {
        le: blk.le,
        subs: i0.subs.clone(),
        ..Default::default()
    };
    for f in i0.fields {
        match f.tag {
            TAG_XMP => {
                m.xmp = Some(
                    String::from_utf8_lossy(&f.data)
                        .trim_end_matches('\0')
                        .to_owned(),
                )
            }
            TAG_ICC => m.icc = Some(f.data),
            TAG_PHOTOSHOP => m.photoshop = Some(f.data),
            TAG_IPTC => m.iptc = Some((f.ty, f.data)),
            t if EDITOR_DOCUMENT.contains(&t) => m.notes.push(format!(
                "dropped editor document tag {t} ({} bytes)",
                f.data.len()
            )),
            t if TIFF_STRUCTURAL.contains(&t) => {}
            _ => m.ifd0.push(f),
        }
    }

    let mut dec = Decoder::new(Cursor::new(b))
        .map_err(|e| e.to_string())?
        .with_limits(Limits::unlimited());
    let (w, h) = dec.dimensions().map_err(|e| e.to_string())?;
    let ch = match dec.colortype().map_err(|e| e.to_string())? {
        tiff::ColorType::RGB(_) => 3,
        tiff::ColorType::RGBA(_) => 4,
        c => return Err(format!("colour type {c:?} not handled by the spike")),
    };
    let px = match dec.read_image().map_err(|e| e.to_string())? {
        DecodingResult::U8(v) => Px::U8(v),
        DecodingResult::U16(v) => Px::U16(v),
        _ => return Err("sample type not handled by the spike".into()),
    };
    Ok((Image { w, h, ch, px }, m, layout))
}

pub fn write(img: &Image, m: &Meta, layout: &Layout, big: bool) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    let compression = match layout.compression {
        1 => Compression::Uncompressed,
        5 => Compression::Lzw,
        8 | 32946 => Compression::Deflate(DeflateLevel::Balanced),
        32773 => Compression::Packbits,
        c => return Err(format!("compression {c} not supported for writing")),
    };
    let predictor = if layout.predictor == 2 {
        Predictor::Horizontal
    } else {
        Predictor::None
    };
    let r = if big {
        let e = TiffEncoder::new_big(&mut out).map_err(|e| e.to_string())?;
        encode(
            e.with_compression(compression).with_predictor(predictor),
            img,
            m,
            layout,
        )
    } else {
        let e = TiffEncoder::new(&mut out).map_err(|e| e.to_string())?;
        encode(
            e.with_compression(compression).with_predictor(predictor),
            img,
            m,
            layout,
        )
    };
    r.map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn ty(t: u16) -> tiff::TiffResult<Type> {
    Type::from_u16(t).ok_or(tiff::TiffError::UsageError(
        tiff::UsageError::InvalidChunkIndex(t as u32),
    ))
}

/// Fields are stored in the source byte order; the encoder writes native (little-endian).
fn put_fields<W: Write + Seek, K: TiffKind>(
    d: &mut DirectoryEncoder<'_, W, K>,
    fields: &[Field],
    le: bool,
) -> tiff::TiffResult<()> {
    let mut dir = Directory::empty();
    for f in fields {
        let data = ifd::reorder(f.ty, &f.data, le, true);
        let entry = d.write_entry_bytes(ty(f.ty)?, &data)?;
        dir.extend([(Tag::from_u16_exhaustive(f.tag), entry)]);
    }
    d.extend_from(&dir);
    Ok(())
}

/// Write a sub-directory tree (Exif → Interop) before the image; returns its offset.
fn sub_dir<W: Write + Seek, K: TiffKind>(
    enc: &mut TiffEncoder<W, K>,
    ifd: &Ifd,
    le: bool,
) -> tiff::TiffResult<u64> {
    let mut children = Vec::new();
    for (tag, s) in &ifd.subs {
        children.push((*tag, sub_dir(enc, s, le)?));
    }
    let mut d = enc.extra_directory()?;
    put_fields(&mut d, &ifd.fields, le)?;
    for (tag, at) in children {
        pointer(&mut d, tag, at)?;
    }
    Ok(d.finish_with_offsets()?.offset.into())
}

/// Sub-IFD pointer: LONG in classic TIFF (as PTGui/Photoshop write it), IFD8 in BigTIFF.
fn pointer<W: Write + Seek, K: TiffKind>(
    d: &mut DirectoryEncoder<'_, W, K>,
    tag: u16,
    at: u64,
) -> tiff::TiffResult<()> {
    if std::mem::size_of::<K::OffsetType>() == 8 {
        d.write_tag(Tag::from_u16_exhaustive(tag), tiff::encoder::Ifd8(at))
    } else {
        d.write_tag(Tag::from_u16_exhaustive(tag), u32::try_from(at)?)
    }
}

fn encode<W: Write + Seek, K: TiffKind>(
    mut enc: TiffEncoder<W, K>,
    img: &Image,
    m: &Meta,
    layout: &Layout,
) -> tiff::TiffResult<()> {
    let mut subs = Vec::new();
    for (tag, s) in &m.subs {
        subs.push((*tag, sub_dir(&mut enc, s, m.le)?));
    }
    let mut extra: Vec<Field> = m.ifd0.clone();
    if img.ch == 4 && !extra.iter().any(|f| f.tag == 338) {
        // The `tiff` encoder doesn't declare the alpha of RGBA; associated, as PTGui writes it.
        extra.push(Field {
            tag: 338,
            ty: 3,
            count: 1,
            data: if m.le { vec![1, 0] } else { vec![0, 1] },
        });
    }
    if let Some(x) = &m.xmp {
        extra.push(Field {
            tag: TAG_XMP,
            ty: 1,
            count: x.len() as u64,
            data: x.as_bytes().to_vec(),
        });
    }
    if let Some(i) = &m.icc {
        extra.push(Field {
            tag: TAG_ICC,
            ty: 7,
            count: i.len() as u64,
            data: i.clone(),
        });
    }
    if let Some(p) = &m.photoshop {
        extra.push(Field {
            tag: TAG_PHOTOSHOP,
            ty: 7,
            count: p.len() as u64,
            data: p.clone(),
        });
    }
    if let Some((t, d)) = &m.iptc {
        extra.push(Field {
            tag: TAG_IPTC,
            ty: *t,
            count: (d.len() / ifd::elem(*t)) as u64,
            data: d.clone(),
        });
    }
    macro_rules! go {
        ($c:ty, $v:expr) => {{
            let mut im = enc.new_image::<$c>(img.w, img.h)?;
            if let Some(r) = layout.rows_per_strip {
                im.rows_per_strip(r.min(img.h))?;
            }
            put_fields(im.encoder(), &extra, m.le)?;
            for (tag, at) in &subs {
                pointer(im.encoder(), *tag, *at)?;
            }
            im.write_data($v)
        }};
    }
    match (&img.px, img.ch) {
        (Px::U8(v), 3) => go!(colortype::RGB8, v),
        (Px::U8(v), 4) => go!(colortype::RGBA8, v),
        (Px::U16(v), 3) => go!(colortype::RGB16, v),
        (Px::U16(v), 4) => go!(colortype::RGBA16, v),
        _ => unreachable!(),
    }
}
