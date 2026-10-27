//! Format-neutral metadata as raw payloads, plus reading/writing the JPEG APPn segments.

use crate::ifd::{self, Block, Field, Ifd};

pub const EXIF_HDR: &[u8] = b"Exif\0\0";
pub const XMP_HDR: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub const XMP_EXT_HDR: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
pub const ICC_HDR: &[u8] = b"ICC_PROFILE\0";
pub const PS_HDR: &[u8] = b"Photoshop 3.0\0";

pub const TAG_XMP: u16 = 700;
pub const TAG_ICC: u16 = 34675;
pub const TAG_PHOTOSHOP: u16 = 34377;
pub const TAG_IPTC: u16 = 33723;
pub const TAG_SOFTWARE: u16 = 305;
pub const TAG_PIXEL_X: u16 = 40962;
pub const TAG_PIXEL_Y: u16 = 40963;

/// Tags that describe the pixel layout of a TIFF; the encoder writes them from the image.
pub const TIFF_STRUCTURAL: [u16; 17] = [
    256, 257, 258, 259, 262, 273, 277, 278, 279, 317, 322, 323, 324, 325, 339, 513, 514,
];
/// TIFF layout tags that are valid in a TIFF but not in a JPEG EXIF IFD0.
pub const TIFF_ONLY: [u16; 4] = [266, 284, 338, 254];
/// Editor documents stored next to the pixels: stale after any edit, and huge (Affinity: 535 MB in
/// the test set). Dropped on export. 37724 = Photoshop ImageSourceData (layers),
/// 51168/51169 = Affinity Photo document.
pub const EDITOR_DOCUMENT: [u16; 3] = [37724, 51168, 51169];

#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub le: bool,
    /// Descriptive IFD0 fields (TIFF: everything except structural tags and the payloads below).
    pub ifd0: Vec<Field>,
    /// Exif / GPS directories hanging off IFD0 (Exif may carry an Interop child).
    pub subs: Vec<(u16, Ifd)>,
    /// JPEG EXIF IFD1 (thumbnail).
    pub ifd1: Option<Ifd>,
    pub xmp: Option<String>,
    pub icc: Option<Vec<u8>>,
    /// Photoshop image resources (JPEG APP13 payload without header / TIFF 34377).
    pub photoshop: Option<Vec<u8>>,
    /// TIFF 33723 (IPTC-NAA), raw.
    pub iptc: Option<(u16, Vec<u8>)>,
    /// Other JPEG segments kept verbatim (JFIF APP0, Adobe APP14, COM, unknown APPn).
    pub jpeg_other: Vec<(u8, Vec<u8>)>,
    pub notes: Vec<String>,
}

impl Meta {
    pub fn exif_ifd(&mut self) -> Option<&mut Ifd> {
        self.subs
            .iter_mut()
            .find(|s| s.0 == ifd::TAG_EXIF)
            .map(|s| &mut s.1)
    }

    pub fn set_software(&mut self, s: &str) {
        let f = ifd::ascii(TAG_SOFTWARE, s);
        match self.ifd0.iter_mut().find(|g| g.tag == TAG_SOFTWARE) {
            Some(g) => *g = f,
            None => self.ifd0.push(f),
        }
    }

    pub fn set_pixel_dims(&mut self, w: u32, h: u32) {
        let le = self.le;
        if let Some(e) = self.exif_ifd() {
            e.patch_uint(TAG_PIXEL_X, w, le);
            e.patch_uint(TAG_PIXEL_Y, h, le);
        }
    }

    pub fn has_makernote(&self) -> bool {
        self.subs.iter().any(|s| s.1.has_makernote())
    }

    /// The EXIF block for a JPEG APP1 segment.
    pub fn exif_block(&self) -> Option<Vec<u8>> {
        if self.ifd0.is_empty() && self.subs.is_empty() {
            return None;
        }
        let ifd0 = Ifd {
            fields: self
                .ifd0
                .iter()
                .filter(|f| !TIFF_ONLY.contains(&f.tag))
                .cloned()
                .collect(),
            subs: self.subs.clone(),
            thumb: None,
        };
        let mut chain = vec![ifd0];
        chain.extend(self.ifd1.clone());
        Some(
            Block {
                le: self.le,
                big: false,
                chain,
            }
            .to_classic(),
        )
    }
}

/// Read the metadata segments of a JPEG (everything before SOS).
pub fn read_jpeg(b: &[u8]) -> Meta {
    let mut m = Meta {
        le: true,
        ..Default::default()
    };
    let mut icc: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut ps = Vec::new();
    let mut i = 2;
    while i + 4 <= b.len() && b[i] == 0xFF {
        let marker = b[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let seg = &b[i + 4..i + 2 + len];
        match marker {
            0xE1 if seg.starts_with(EXIF_HDR) => {
                if let Some(blk) = ifd::parse(&seg[EXIF_HDR.len()..], 2) {
                    m.le = blk.le;
                    let mut it = blk.chain.into_iter();
                    if let Some(i0) = it.next() {
                        m.ifd0 = i0.fields;
                        m.subs = i0.subs;
                    }
                    m.ifd1 = it.next();
                }
            }
            0xE1 if seg.starts_with(XMP_HDR) => {
                m.xmp = Some(
                    String::from_utf8_lossy(&seg[XMP_HDR.len()..])
                        .trim_end_matches('\0')
                        .to_owned(),
                )
            }
            0xE1 if seg.starts_with(XMP_EXT_HDR) => m
                .notes
                .push("ExtendedXMP present (not handled by the spike)".into()),
            0xE2 if seg.starts_with(ICC_HDR) => icc.push((seg[12], seg[14..].to_vec())),
            0xED if seg.starts_with(PS_HDR) => ps.extend_from_slice(&seg[PS_HDR.len()..]),
            0xE0..=0xEF | 0xFE => m.jpeg_other.push((marker, seg.to_vec())),
            _ => {} // tables, SOF, DRI: belong to the encoder
        }
        i += 2 + len;
    }
    if !icc.is_empty() {
        icc.sort_by_key(|c| c.0);
        m.icc = Some(icc.into_iter().flat_map(|c| c.1).collect());
    }
    if !ps.is_empty() {
        m.photoshop = Some(ps);
    }
    m
}

/// Build the APPn segments for a JPEG (marker, payload), in a conventional order.
pub fn jpeg_segments(m: &Meta, exif: Option<Vec<u8>>) -> Result<Vec<(u8, Vec<u8>)>, String> {
    let mut out = Vec::new();
    for (mk, p) in &m.jpeg_other {
        if *mk == 0xE0 {
            out.push((*mk, p.clone()));
        }
    }
    if let Some(e) = exif {
        let seg = [EXIF_HDR, &e].concat();
        if seg.len() > 65533 {
            return Err(format!(
                "EXIF block too large for one segment ({} bytes)",
                seg.len()
            ));
        }
        out.push((0xE1, seg));
    }
    if let Some(x) = &m.xmp {
        let seg = [XMP_HDR, x.as_bytes()].concat();
        if seg.len() > 65533 {
            return Err(format!(
                "XMP packet too large for one segment ({} bytes) – needs ExtendedXMP",
                seg.len()
            ));
        }
        out.push((0xE1, seg));
    }
    if let Some(icc) = &m.icc {
        let chunks: Vec<&[u8]> = icc.chunks(65519).collect();
        for (k, c) in chunks.iter().enumerate() {
            out.push((
                0xE2,
                [ICC_HDR, &[(k + 1) as u8, chunks.len() as u8], c].concat(),
            ));
        }
    }
    let ps = photoshop_with_iptc(m);
    if let Some(ps) = &ps {
        for c in ps.chunks(65533 - PS_HDR.len()) {
            out.push((0xED, [PS_HDR, c].concat()));
        }
    }
    for (mk, p) in &m.jpeg_other {
        if *mk != 0xE0 {
            out.push((*mk, p.clone()));
        }
    }
    Ok(out)
}

/// Replace the metadata segments of an encoded JPEG with `segs` (inserted right after SOI).
pub fn splice_jpeg(encoded: &[u8], segs: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(encoded.len() + 1 << 16);
    out.extend_from_slice(&encoded[..2]);
    for (mk, p) in segs {
        out.extend_from_slice(&[0xFF, *mk]);
        out.extend_from_slice(&((p.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(p);
    }
    let mut i = 2;
    while i + 4 <= encoded.len() && encoded[i] == 0xFF {
        let marker = encoded[i + 1];
        if marker == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([encoded[i + 2], encoded[i + 3]]) as usize;
        if !matches!(marker, 0xE0..=0xEF | 0xFE) {
            out.extend_from_slice(&encoded[i..i + 2 + len]);
        }
        i += 2 + len;
    }
    out.extend_from_slice(&encoded[i..]);
    out
}

/// Photoshop image resources for a JPEG APP13. A TIFF keeps IPTC in its own tag (33723); in a JPEG
/// it lives in the 8BIM resource 0x0404, which is added when the resources don't have one.
fn photoshop_with_iptc(m: &Meta) -> Option<Vec<u8>> {
    let mut ps = m.photoshop.clone().unwrap_or_default();
    if let Some((_, iptc)) = &m.iptc {
        if !resource_ids(&ps).contains(&0x0404) {
            ps.extend_from_slice(b"8BIM");
            ps.extend_from_slice(&0x0404u16.to_be_bytes());
            ps.extend_from_slice(&[0, 0]); // empty Pascal name, padded to even
            ps.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
            ps.extend_from_slice(iptc);
            if iptc.len() % 2 == 1 {
                ps.push(0);
            }
        }
    }
    (!ps.is_empty()).then_some(ps)
}

fn resource_ids(ps: &[u8]) -> Vec<u16> {
    let mut ids = Vec::new();
    let mut i = 0;
    while i + 12 <= ps.len() && &ps[i..i + 4] == b"8BIM" {
        ids.push(u16::from_be_bytes([ps[i + 4], ps[i + 5]]));
        let name = (1 + ps[i + 6] as usize).next_multiple_of(2);
        let at = i + 6 + name;
        let Some(len) = ps
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()) as usize)
        else {
            break;
        };
        i = at + 4 + len.next_multiple_of(2);
    }
    ids
}
