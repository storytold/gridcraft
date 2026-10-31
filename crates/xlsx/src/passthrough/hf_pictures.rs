//! Header and footer pictures (`&G`, #1257), read from the content a sheet keeps for saving.
//!
//! Excel draws them from a VML drawing that the sheet names with `<legacyDrawingHF>`: one
//! `<v:shape>` per picture, whose `id` says where it goes (`LH`, `CH`, `RH` for the left, centre
//! and right header, `LF`, `CF`, `RF` for the footer, with a `FIRST` or `EVEN` suffix for the first
//! page and the even pages), whose `style` gives its size, and whose `<v:imagedata o:relid>` names
//! the image through the drawing's relationships.
//!
//! The drawing and its images stay pass-through content, written back byte for byte; this module
//! only reads them, so that printing can draw the pictures. Damaged or unexpected shapes are
//! skipped, never an error.

use std::sync::Arc;

use gridcraft_model::{HfPage, Preserved, PreservedPart};

use super::read::shape_spans;

/// At most one picture per section: 3 sections × header/footer × odd/first/even pages.
const MAX_PICTURES: usize = 18;
/// Largest picture size kept, in points (a 22 in page side is far beyond any paper size).
const MAX_SIDE_PT: f32 = 1584.0;

/// A picture in a header or footer section.
#[derive(Clone, Debug, PartialEq)]
pub struct HfPicture {
    /// The pages it prints on (odd/every page, first page, even pages).
    pub page: HfPage,
    /// In the footer (else the header).
    pub footer: bool,
    /// The section: 0 left, 1 centre, 2 right.
    pub section: usize,
    /// Printed size in points.
    pub width: f32,
    pub height: f32,
    /// The encoded image (PNG, JPEG…), as the file holds it.
    pub data: Arc<[u8]>,
}

/// The header and footer pictures a sheet keeps, at most one per section (the first shape for a
/// section wins). Empty when the sheet has none or they can't be read.
pub fn header_footer_pictures(kept: &Preserved) -> Vec<HfPicture> {
    let Some(vml) = hf_drawing(kept) else { return vec![] };
    let Ok(text) = std::str::from_utf8(&vml.data) else { return vec![] };
    let mut out: Vec<HfPicture> = vec![];
    for (a, b) in shape_spans(text) {
        if out.len() >= MAX_PICTURES {
            break;
        }
        let Some(shape) = text.get(a..b) else { continue };
        let Some(pic) = picture(kept, vml, shape) else { continue };
        if !out.iter().any(|p| p.page == pic.page && p.footer == pic.footer && p.section == pic.section) {
            out.push(pic);
        }
    }
    out
}

/// The VML drawing `<legacyDrawingHF r:id>` names.
fn hf_drawing(kept: &Preserved) -> Option<&PreservedPart> {
    let el = kept.elements.iter().find(|e| e.slot == "legacyDrawingHF")?;
    let id = attr(&el.xml, "r:id")?;
    let rel = kept.rels.iter().find(|r| r.id == id && !r.external)?;
    part(kept, &rel.target)
}

fn part<'a>(kept: &'a Preserved, name: &str) -> Option<&'a PreservedPart> {
    kept.parts.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// One `<v:shape>…</v:shape>` as a picture, or `None` when it isn't a header/footer picture.
fn picture(kept: &Preserved, vml: &PreservedPart, shape: &str) -> Option<HfPicture> {
    let tag = shape.get(..shape.find('>')?)?;
    let (page, footer, section) = position(attr(tag, "id")?)?;
    let style = attr(tag, "style").unwrap_or("");
    let width = style_length(style, "width")?;
    let height = style_length(style, "height")?;
    let i = shape.find("<v:imagedata")?;
    let data_tag = shape.get(i..i + shape.get(i..)?.find('>')?)?;
    let rid = attr(data_tag, "o:relid").or_else(|| attr(data_tag, "r:id"))?;
    let rel = vml.rels.iter().find(|r| r.id == rid && !r.external)?;
    let image = part(kept, &rel.target)?;
    Some(HfPicture { page, footer, section, width, height, data: image.data.clone() })
}

/// Where a shape id (`LH`, `CFFIRST`, `RHEVEN`…) puts its picture.
fn position(id: &str) -> Option<(HfPage, bool, usize)> {
    let id = id.to_ascii_uppercase();
    let (base, page) = if let Some(b) = id.strip_suffix("FIRST") {
        (b, HfPage::First)
    } else if let Some(b) = id.strip_suffix("EVEN") {
        (b, HfPage::Even)
    } else {
        (id.as_str(), HfPage::Odd)
    };
    let mut c = base.chars();
    let section = match c.next()? {
        'L' => 0,
        'C' => 1,
        'R' => 2,
        _ => return None,
    };
    let footer = match c.next()? {
        'H' => false,
        'F' => true,
        _ => return None,
    };
    c.next().is_none().then_some((page, footer, section))
}

/// A length property of a CSS-like VML `style` (`width:120pt`), in points. Positive and bounded.
fn style_length(style: &str, key: &str) -> Option<f32> {
    let value = style.split(';').find_map(|decl| {
        let (k, v) = decl.split_once(':')?;
        k.trim().eq_ignore_ascii_case(key).then(|| v.trim())
    })?;
    let split = value.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(value.len());
    let (num, unit) = value.split_at(split);
    let n: f32 = num.parse().ok()?;
    let k = match unit.trim().to_ascii_lowercase().as_str() {
        "pt" => 1.0,
        "in" => 72.0,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "pc" => 12.0,
        // CSS pixels, and VML's default unit.
        "px" | "" => 0.75,
        _ => return None,
    };
    let pt = n * k;
    (pt.is_finite() && pt > 0.0).then_some(pt.min(MAX_SIDE_PT))
}

/// The value of attribute `name` in a start tag (`name="…"` or `name='…'`), entities left as
/// they are (ids, styles and relationship ids don't use them).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = tag.get(from..)?.find(name).map(|i| i + from) {
        let before = tag.get(..i)?.chars().next_back();
        let rest = tag.get(i + name.len()..)?.trim_start();
        from = i + name.len();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let body = rest.get(1..)?;
        return body.get(..body.find(quote)?);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_model::{PreservedRel, PreservedXml};

    fn rel(id: &str, target: &str) -> PreservedRel {
        PreservedRel { id: id.into(), rel_type: "x/image".into(), target: target.into(), external: false }
    }

    fn kept(vml: &str) -> Preserved {
        let part = |name: &str, data: &[u8], rels: Vec<PreservedRel>| PreservedPart {
            name: name.into(),
            content_type: String::new(),
            data: Arc::from(data),
            rels,
        };
        Preserved {
            rels: vec![rel("rId3", "xl/drawings/vmlDrawing2.vml")],
            elements: vec![PreservedXml { slot: "legacyDrawingHF".into(), xml: "<legacyDrawingHF xmlns:r=\"x\" r:id=\"rId3\"/>".into() }],
            parts: vec![
                part("xl/drawings/vmlDrawing2.vml", vml.as_bytes(), vec![rel("rId1", "xl/media/image1.png"), rel("rId2", "xl/media/image2.png")]),
                part("xl/media/image1.png", b"one", vec![]),
                part("xl/media/image2.png", b"two", vec![]),
            ],
            ..Preserved::default()
        }
    }

    fn shape(id: &str, style: &str, rid: &str) -> String {
        format!(
            r##"<v:shape id="{id}" o:spid="_x0000_s1025" type="#_x0000_t75" style="{style}"><v:imagedata o:relid="{rid}" o:title="x"/></v:shape>"##
        )
    }

    #[test]
    fn positions_sizes_and_images() {
        let vml = [
            shape("LH", "position:absolute;margin-left:0;width:120pt;height:40pt", "rId1"),
            shape("CFFIRST", "width:1in;height:0.5in", "rId2"),
            shape("RHEVEN", "width:96px;height:2.54cm", "rId1"),
        ]
        .concat();
        let pics = header_footer_pictures(&kept(&format!("<xml>{vml}</xml>")));
        let got: Vec<_> = pics.iter().map(|p| (p.page, p.footer, p.section, p.width, p.height.round(), &*p.data)).collect();
        assert_eq!(
            got,
            [
                (HfPage::Odd, false, 0, 120.0, 40.0, b"one".as_slice()),
                (HfPage::First, true, 1, 72.0, 36.0, b"two".as_slice()),
                (HfPage::Even, false, 2, 72.0, 72.0, b"one".as_slice()),
            ]
        );
    }

    #[test]
    fn damaged_or_unknown_shapes_are_skipped() {
        let vml = [
            shape("XH", "width:10pt;height:10pt", "rId1"),        // unknown position
            shape("LH", "width:abc;height:10pt", "rId1"),         // bad size
            shape("CH", "width:10pt;height:10pt", "rId9"),        // missing relationship
            shape("RH", "width:-5pt;height:10pt", "rId1"),        // negative size
            shape("LF", "width:10furlongs;height:10pt", "rId1"),  // unknown unit
            shape("CF", "width:1e9pt;height:10pt", "rId1"),       // exponent: not a CSS length
            shape("RF", "width:99999pt;height:10pt", "rId2"),     // capped, still read
            shape("RF", "width:5pt;height:5pt", "rId1"),          // second one for RF: ignored
            "<v:shape id=\"CF\" style=\"width:1pt\"".to_string(), // truncated at the end
        ]
        .concat();
        let pics = header_footer_pictures(&kept(&vml));
        assert_eq!(pics.len(), 1, "{pics:?}");
        assert_eq!((pics[0].section, pics[0].footer, pics[0].width, &*pics[0].data), (2, true, MAX_SIDE_PT, b"two".as_slice()));
    }

    #[test]
    fn nothing_without_the_drawing() {
        assert!(header_footer_pictures(&Preserved::default()).is_empty());
        let mut k = kept(&shape("LH", "width:1pt;height:1pt", "rId1"));
        k.parts.retain(|p| !p.name.ends_with(".vml"));
        assert!(header_footer_pictures(&k).is_empty());
        let mut k = kept("\u{0}\u{ff}not vml");
        k.parts[0].data = Arc::from(&[0xff, 0xfe, 0x00][..]);
        assert!(header_footer_pictures(&k).is_empty());
    }

    #[test]
    fn attributes_need_their_own_name() {
        let tag = r#"<v:shape o:spid="_x0000_s1" id='CH' xid="no""#;
        assert_eq!(attr(tag, "id"), Some("CH"));
        assert_eq!(attr(r#"<v:imagedata o:relid = "rId4""#, "o:relid"), Some("rId4"));
        assert_eq!(attr("<a b=c>", "b"), None);
    }
}
