//! XMP edits with two engines: the Adobe XMP Toolkit (`xmp_toolkit`) and a minimal text patch that
//! leaves the packet byte-identical outside the touched values.

use xmp_toolkit::{ToStringOptions, XmpMeta, XmpValue};

pub const NS_GPANO: &str = "http://ns.google.com/photos/1.0/panorama/";
pub const NS_TIFF: &str = "http://ns.adobe.com/tiff/1.0/";
pub const NS_EXIF: &str = "http://ns.adobe.com/exif/1.0/";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    Toolkit,
    Text,
}

/// One requested change: set `ns:name` to `value`. `only_if_present` skips properties the source
/// doesn't have (dimensions); otherwise the property is added (heading).
#[derive(Clone, Debug)]
pub struct Set {
    pub ns: &'static str,
    pub prefix: &'static str,
    pub name: &'static str,
    pub value: String,
    pub only_if_present: bool,
}

pub fn get(xmp: &str, ns: &str, name: &str) -> Option<String> {
    let m: XmpMeta = xmp.parse().ok()?;
    m.property(ns, name).map(|v| v.value)
}

pub fn apply(xmp: &str, sets: &[Set], engine: Engine) -> Result<String, String> {
    match engine {
        Engine::Toolkit => toolkit(xmp, sets),
        Engine::Text => sets.iter().try_fold(xmp.to_owned(), |x, s| text(&x, s)),
    }
}

fn toolkit(xmp: &str, sets: &[Set]) -> Result<String, String> {
    let mut m: XmpMeta = xmp.parse().map_err(|e| format!("xmp parse: {e:?}"))?;
    XmpMeta::register_namespace(NS_GPANO, "GPano").map_err(|e| format!("{e:?}"))?;
    for s in sets {
        if s.only_if_present && !m.contains_property(s.ns, s.name) {
            continue;
        }
        m.set_property(s.ns, s.name, &XmpValue::new(s.value.clone()))
            .map_err(|e| format!("set {}: {e:?}", s.name))?;
    }
    m.to_string_with_options(ToStringOptions::default().omit_packet_wrapper().use_compact_format())
        .map(|s| format!("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n{s}<?xpacket end=\"w\"?>"))
        .map_err(|e| format!("xmp serialise: {e:?}"))
}

/// Text patch: attribute form `prefix:name="…"` or element form `<prefix:name>…</prefix:name>`.
/// The prefix used in the packet is looked up from its `xmlns:` declaration.
fn text(xmp: &str, s: &Set) -> Result<String, String> {
    let prefix = declared_prefix(xmp, s.ns);
    if let Some(p) = &prefix {
        for q in ['"', '\''] {
            let attr = format!("{p}:{}={q}", s.name);
            if let Some(i) = find_attr(xmp, &attr) {
                let v0 = i + attr.len();
                let v1 = v0 + xmp[v0..].find(q).ok_or("unterminated attribute")?;
                return Ok(format!("{}{}{}", &xmp[..v0], escape(&s.value), &xmp[v1..]));
            }
        }
        let open = format!("<{p}:{}>", s.name);
        if let Some(i) = xmp.find(&open) {
            let v0 = i + open.len();
            let close = format!("</{p}:{}>", s.name);
            let v1 = v0 + xmp[v0..].find(&close).ok_or("unterminated element")?;
            return Ok(format!("{}{}{}", &xmp[..v0], escape(&s.value), &xmp[v1..]));
        }
    }
    if s.only_if_present {
        return Ok(xmp.to_owned());
    }
    // Add: as an attribute on the first rdf:Description, declaring the namespace if needed.
    let d = xmp.find("<rdf:Description").ok_or("no rdf:Description")?;
    let at = d + "<rdf:Description".len();
    let (p, decl) = match prefix {
        Some(p) => (p, String::new()),
        None => (
            s.prefix.to_owned(),
            format!(" xmlns:{}=\"{}\"", s.prefix, s.ns),
        ),
    };
    Ok(format!(
        "{}{decl} {p}:{}=\"{}\"{}",
        &xmp[..at],
        s.name,
        escape(&s.value),
        &xmp[at..]
    ))
}

/// An attribute occurrence, not a longer name ending in the same text (`GPano:X` vs `oGPano:X`).
fn find_attr(xmp: &str, attr: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(j) = xmp[from..].find(attr) {
        let i = from + j;
        if i > 0 && xmp.as_bytes()[i - 1].is_ascii_whitespace() {
            return Some(i);
        }
        from = i + attr.len();
    }
    None
}

/// The prefix bound to `ns` (either quote style, as Adobe and ExifTool write them).
fn declared_prefix(xmp: &str, ns: &str) -> Option<String> {
    let i = [format!("=\"{ns}\""), format!("='{ns}'")]
        .iter()
        .find_map(|n| xmp.find(n.as_str()))?;
    let head = &xmp[..i];
    let x = head.rfind("xmlns:")?;
    Some(head[x + 6..].to_owned())
}

fn escape(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// The edits for an export: dimensions (only where the source records them; GPano pixel values
/// scaled with the image) and optionally the heading.
pub fn export_sets(xmp: &str, src: (u32, u32), dst: (u32, u32), heading: Option<f64>) -> Vec<Set> {
    let (w, h) = dst;
    let mut v = vec![
        set(NS_TIFF, "tiff", "ImageWidth", w.to_string(), true),
        set(NS_TIFF, "tiff", "ImageLength", h.to_string(), true),
        set(NS_EXIF, "exif", "PixelXDimension", w.to_string(), true),
        set(NS_EXIF, "exif", "PixelYDimension", h.to_string(), true),
    ];
    if dst != src {
        for (name, horizontal) in [
            ("FullPanoWidthPixels", true),
            ("FullPanoHeightPixels", false),
            ("CroppedAreaImageWidthPixels", true),
            ("CroppedAreaImageHeightPixels", false),
            ("CroppedAreaLeftPixels", true),
            ("CroppedAreaTopPixels", false),
        ] {
            let Some(old) = get(xmp, NS_GPANO, name).and_then(|s| s.trim().parse::<f64>().ok())
            else {
                continue;
            };
            let f = if horizontal {
                f64::from(dst.0) / f64::from(src.0)
            } else {
                f64::from(dst.1) / f64::from(src.1)
            };
            v.push(set(
                NS_GPANO,
                "GPano",
                name,
                ((old * f).round() as i64).to_string(),
                true,
            ));
        }
    }
    if let Some(hd) = heading {
        v.push(set(
            NS_GPANO,
            "GPano",
            "PoseHeadingDegrees",
            format!("{hd:.1}"),
            false,
        ));
    }
    v
}

fn set(
    ns: &'static str,
    prefix: &'static str,
    name: &'static str,
    value: String,
    only_if_present: bool,
) -> Set {
    Set {
        ns,
        prefix,
        name,
        value,
        only_if_present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTR: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:GPano="http://ns.google.com/photos/1.0/panorama/" GPano:FullPanoWidthPixels="8000" GPano:ProjectionType="equirectangular"/></rdf:RDF></x:xmpmeta>"#;
    const ELEM: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:GPano="http://ns.google.com/photos/1.0/panorama/"><GPano:FullPanoWidthPixels>8000</GPano:FullPanoWidthPixels></rdf:Description></rdf:RDF></x:xmpmeta>"#;

    #[test]
    fn both_engines_set_and_add() {
        for src in [ATTR, ELEM] {
            for e in [Engine::Toolkit, Engine::Text] {
                let sets = [
                    set(
                        NS_GPANO,
                        "GPano",
                        "FullPanoWidthPixels",
                        "4000".into(),
                        true,
                    ),
                    set(
                        NS_GPANO,
                        "GPano",
                        "PoseHeadingDegrees",
                        "90.0".into(),
                        false,
                    ),
                    set(NS_TIFF, "tiff", "ImageWidth", "4000".into(), true),
                ];
                let out = apply(src, &sets, e).unwrap();
                assert_eq!(
                    get(&out, NS_GPANO, "FullPanoWidthPixels").as_deref(),
                    Some("4000"),
                    "{e:?}"
                );
                assert_eq!(
                    get(&out, NS_GPANO, "PoseHeadingDegrees").as_deref(),
                    Some("90.0"),
                    "{e:?}"
                );
                assert_eq!(get(&out, NS_TIFF, "ImageWidth"), None, "{e:?}");
            }
        }
    }

    #[test]
    fn exiftool_style_single_quotes_and_elements() {
        let src = "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:GPano='http://ns.google.com/photos/1.0/panorama/'><GPano:PoseHeadingDegrees>42.0</GPano:PoseHeadingDegrees></rdf:Description></rdf:RDF></x:xmpmeta>";
        let out = apply(
            src,
            &[set(
                NS_GPANO,
                "GPano",
                "PoseHeadingDegrees",
                "90.0".into(),
                false,
            )],
            Engine::Text,
        )
        .unwrap();
        assert_eq!(out.matches("PoseHeadingDegrees>").count(), 2, "{out}");
        assert_eq!(
            get(&out, NS_GPANO, "PoseHeadingDegrees").as_deref(),
            Some("90.0")
        );
    }

    #[test]
    fn text_engine_adds_namespace() {
        let src = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about=""/></rdf:RDF></x:xmpmeta>"#;
        let out = apply(
            src,
            &[set(
                NS_GPANO,
                "GPano",
                "PoseHeadingDegrees",
                "12.0".into(),
                false,
            )],
            Engine::Text,
        )
        .unwrap();
        assert_eq!(
            get(&out, NS_GPANO, "PoseHeadingDegrees").as_deref(),
            Some("12.0")
        );
    }
}
