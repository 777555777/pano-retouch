//! Minimal TIFF/EXIF directory model: every field is kept as raw bytes (in the source byte order),
//! so tags we don't know survive. Pointer tags (Exif, GPS, Interop) become child directories; the
//! IFD1 thumbnail (513/514) is carried as a blob. Serialising relocates all values.

pub const TAG_EXIF: u16 = 34665;
pub const TAG_GPS: u16 = 34853;
pub const TAG_INTEROP: u16 = 40965;
pub const TAG_MAKERNOTE: u16 = 37500;
const TAG_THUMB_OFFSET: u16 = 513;
const TAG_THUMB_LEN: u16 = 514;
const POINTERS: [u16; 3] = [TAG_EXIF, TAG_GPS, TAG_INTEROP];

#[derive(Clone, Debug)]
pub struct Field {
    pub tag: u16,
    pub ty: u16,
    pub count: u64,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Ifd {
    pub fields: Vec<Field>,
    pub subs: Vec<(u16, Ifd)>,
    pub thumb: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub le: bool,
    pub big: bool,
    /// IFD0, IFD1, …
    pub chain: Vec<Ifd>,
}

/// Bytes per value of a TIFF field type.
pub fn elem(ty: u16) -> usize {
    match ty {
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 | 16 | 17 | 18 => 8,
        _ => 1,
    }
}

/// Size of the units whose byte order flips (rationals flip per 32-bit half).
fn swap_unit(ty: u16) -> usize {
    match ty {
        5 | 10 => 4,
        t => elem(t),
    }
}

/// Convert a value between byte orders (rationals swap per 32-bit half).
pub fn reorder(ty: u16, v: &[u8], from_le: bool, to_le: bool) -> Vec<u8> {
    let n = swap_unit(ty);
    if from_le == to_le || n == 1 {
        return v.to_vec();
    }
    v.chunks(n).flat_map(|c| c.iter().rev().copied()).collect()
}

struct R<'a> {
    b: &'a [u8],
    le: bool,
    big: bool,
}

impl R<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let s: [u8; 2] = self.b.get(at..at + 2)?.try_into().ok()?;
        Some(if self.le {
            u16::from_le_bytes(s)
        } else {
            u16::from_be_bytes(s)
        })
    }
    fn u32(&self, at: usize) -> Option<u32> {
        let s: [u8; 4] = self.b.get(at..at + 4)?.try_into().ok()?;
        Some(if self.le {
            u32::from_le_bytes(s)
        } else {
            u32::from_be_bytes(s)
        })
    }
    fn u64(&self, at: usize) -> Option<u64> {
        let s: [u8; 8] = self.b.get(at..at + 8)?.try_into().ok()?;
        Some(if self.le {
            u64::from_le_bytes(s)
        } else {
            u64::from_be_bytes(s)
        })
    }
    fn off(&self, at: usize) -> Option<u64> {
        if self.big {
            self.u64(at)
        } else {
            self.u32(at).map(u64::from)
        }
    }
    fn uint(&self, f: &Field) -> Option<u64> {
        match f.ty {
            3 => Some(u64::from(
                R {
                    b: &f.data,
                    ..*self
                }
                .u16(0)?,
            )),
            4 | 13 => Some(u64::from(
                R {
                    b: &f.data,
                    ..*self
                }
                .u32(0)?,
            )),
            16 | 18 => R {
                b: &f.data,
                ..*self
            }
            .u64(0),
            _ => None,
        }
    }

    fn ifd(&self, at: u64, depth: u32) -> Option<(Ifd, u64)> {
        let at = usize::try_from(at).ok()?;
        let (n, esz, head) = if self.big {
            (self.u64(at)? as usize, 20, 8)
        } else {
            (self.u16(at)? as usize, 12, 2)
        };
        if n > 4096 {
            return None;
        }
        let inline = if self.big { 8 } else { 4 };
        let mut ifd = Ifd::default();
        for i in 0..n {
            let e = at + head + i * esz;
            let tag = self.u16(e)?;
            let ty = self.u16(e + 2)?;
            let count = if self.big {
                self.u64(e + 4)?
            } else {
                u64::from(self.u32(e + 4)?)
            };
            let len = usize::try_from(count).ok()?.checked_mul(elem(ty))?;
            let vat = if self.big { e + 12 } else { e + 8 };
            let data = if len <= inline {
                self.b.get(vat..vat + len)?.to_vec()
            } else {
                let o = usize::try_from(self.off(vat)?).ok()?;
                match self.b.get(o..o + len) {
                    Some(d) => d.to_vec(),
                    None => continue,
                }
            };
            ifd.fields.push(Field {
                tag,
                ty,
                count,
                data,
            });
        }
        let next = self.off(at + head + n * esz)?;
        if depth < 3 {
            for p in POINTERS {
                if let Some(f) = ifd.fields.iter().find(|f| f.tag == p) {
                    if let Some(o) = self.uint(f) {
                        if let Some((sub, _)) = self.ifd(o, depth + 1) {
                            ifd.subs.push((p, sub));
                        }
                    }
                }
            }
            ifd.fields
                .retain(|f| !POINTERS.contains(&f.tag) || !ifd.subs.iter().any(|s| s.0 == f.tag));
        }
        let off = ifd
            .fields
            .iter()
            .find(|f| f.tag == TAG_THUMB_OFFSET)
            .and_then(|f| self.uint(f));
        let len = ifd
            .fields
            .iter()
            .find(|f| f.tag == TAG_THUMB_LEN)
            .and_then(|f| self.uint(f));
        if let (Some(o), Some(l)) = (off, len) {
            ifd.thumb = self.b.get(o as usize..(o + l) as usize).map(<[u8]>::to_vec);
        }
        Some((ifd, next))
    }
}

/// Parse a TIFF stream (classic or BigTIFF). `max_chain` limits how many top-level IFDs are read
/// (1 for an image file's IFD0, 2 for an EXIF block with IFD1).
pub fn parse(b: &[u8], max_chain: usize) -> Option<Block> {
    let le = match b.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let r0 = R { b, le, big: false };
    let big = match r0.u16(2)? {
        42 => false,
        43 => true,
        _ => return None,
    };
    let r = R { b, le, big };
    let mut at = if big { r.u64(8)? } else { u64::from(r.u32(4)?) };
    let mut chain = Vec::new();
    while at != 0 && chain.len() < max_chain {
        let (ifd, next) = r.ifd(at, 0)?;
        chain.push(ifd);
        at = next;
    }
    Some(Block { le, big, chain })
}

impl Ifd {
    pub fn get(&self, tag: u16) -> Option<&Field> {
        self.fields.iter().find(|f| f.tag == tag)
    }
    /// Replace an integer field keeping its type (SHORT or LONG); no-op when absent.
    pub fn patch_uint(&mut self, tag: u16, v: u32, le: bool) -> bool {
        let Some(f) = self.fields.iter_mut().find(|f| f.tag == tag) else {
            return false;
        };
        f.data = match f.ty {
            3 => {
                let v = v as u16;
                (if le { v.to_le_bytes() } else { v.to_be_bytes() }).to_vec()
            }
            _ => {
                f.ty = 4;
                (if le { v.to_le_bytes() } else { v.to_be_bytes() }).to_vec()
            }
        };
        f.count = 1;
        true
    }
    pub fn has_makernote(&self) -> bool {
        self.get(TAG_MAKERNOTE).is_some() || self.subs.iter().any(|s| s.1.has_makernote())
    }
}

pub fn ascii(tag: u16, s: &str) -> Field {
    let mut data = s.as_bytes().to_vec();
    data.push(0);
    Field {
        tag,
        ty: 2,
        count: data.len() as u64,
        data,
    }
}

impl Block {
    /// Serialise as a classic TIFF stream (for a JPEG APP1 EXIF block). All values are relocated.
    pub fn to_classic(&self) -> Vec<u8> {
        let le = self.le;
        let mut out = Vec::new();
        out.extend_from_slice(if le { b"II" } else { b"MM" });
        put16(&mut out, 42, le);
        put32(&mut out, 8, le);
        let mut link = 4usize; // where the offset of the next IFD goes
        for ifd in &self.chain {
            let at = write_ifd(&mut out, ifd, le);
            out[link..link + 4].copy_from_slice(&u32b(at as u32, le));
            link = at + 2 + 12 * (ifd.fields.len() + ifd.subs.len());
        }
        out
    }
}

fn u32b(v: u32, le: bool) -> [u8; 4] {
    if le { v.to_le_bytes() } else { v.to_be_bytes() }
}
fn put16(o: &mut Vec<u8>, v: u16, le: bool) {
    o.extend_from_slice(&if le { v.to_le_bytes() } else { v.to_be_bytes() });
}
fn put32(o: &mut Vec<u8>, v: u32, le: bool) {
    o.extend_from_slice(&u32b(v, le));
}

/// Writes children first, then the directory and its out-of-line values; returns the IFD offset.
fn write_ifd(out: &mut Vec<u8>, ifd: &Ifd, le: bool) -> usize {
    let mut fields = ifd.fields.clone();
    for (tag, sub) in &ifd.subs {
        let at = write_ifd(out, sub, le);
        fields.push(Field {
            tag: *tag,
            ty: 4,
            count: 1,
            data: u32b(at as u32, le).to_vec(),
        });
    }
    let mut thumb_at = None;
    if let Some(t) = &ifd.thumb {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        thumb_at = Some(out.len());
        out.extend_from_slice(t);
    }
    if let Some(at) = thumb_at {
        if let Some(f) = fields.iter_mut().find(|f| f.tag == TAG_THUMB_OFFSET) {
            f.ty = 4;
            f.count = 1;
            f.data = u32b(at as u32, le).to_vec();
        }
    }
    fields.sort_by_key(|f| f.tag);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let start = out.len();
    let mut extra = start + 2 + 12 * fields.len() + 4;
    let mut values = Vec::new();
    put16(out, fields.len() as u16, le);
    for f in &fields {
        put16(out, f.tag, le);
        put16(out, f.ty, le);
        put32(out, f.count as u32, le);
        if f.data.len() <= 4 {
            let mut v = f.data.clone();
            v.resize(4, 0);
            out.extend_from_slice(&v);
        } else {
            put32(out, extra as u32, le);
            values.extend_from_slice(&f.data);
            if f.data.len() % 2 == 1 {
                values.push(0);
            }
            extra += f.data.len().next_multiple_of(2);
        }
    }
    put32(out, 0, le); // next IFD, patched by the caller for chains
    out.extend_from_slice(&values);
    start
}
