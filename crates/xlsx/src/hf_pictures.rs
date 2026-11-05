//! Header and footer pictures (`&G`, #1257) in XLSX.
//!
//! Excel draws them from a VML drawing that the sheet names with `<legacyDrawingHF>`: one
//! `<v:shape>` per picture, whose `id` says where it goes (`LH`, `CH`, `RH` for the left, centre
//! and right header, `LF`, `CF`, `RF` for the footer, with a `FIRST` or `EVEN` suffix for the first
//! page and the even pages), whose `style` gives its size, and whose `<v:imagedata o:relid>` names
//! the image through the drawing's relationships.
//!
//! On read, the pass-through keeps that drawing and its images; [`take_from_kept`] turns them into
//! the sheet's [`HfPicture`]s and drops them from what is kept, so that the pictures are workbook
//! data GridCraft writes ([`write`]). A drawing with a shape that can't be modelled is left kept
//! as it was (written back byte for byte) and nothing is modelled from it.

use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_model::{HfPage, HfPicture, MAX_HF_PICTURES, Preserved, PreservedPart, Sheet};

use crate::passthrough::shape_spans;
use crate::write::{Out, Rels};
use crate::xml::{esc_attr, num};

/// Largest picture size kept, in points (a 22 in page side is far beyond any paper size).
pub(crate) const MAX_SIDE_PT: f32 = 1584.0;
/// Excel's picture settings on `<v:imagedata>` that are kept as read: crop and colour.
const ADJUST: &[&str] = &["cropleft", "croptop", "cropright", "cropbottom", "gain", "blacklevel", "gamma", "grayscale", "bilevel"];
const CT_VML: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";

// ---------------------------------------------------------------- reading

/// Moves the header and footer pictures out of a sheet's kept content into the model: returns
/// them, and removes the `<legacyDrawingHF>` element, its relationship, the drawing and the images
/// only it uses from `kept`. `None` (and `kept` unchanged) when the sheet keeps no such drawing,
/// or when one of its shapes can't be modelled.
pub(crate) fn take_from_kept(kept: &mut Preserved) -> Option<Vec<HfPicture>> {
    let el = kept.elements.iter().position(|e| e.slot == "legacyDrawingHF")?;
    let rid = attr(&kept.elements.get(el)?.xml, "r:id")?.to_string();
    let rel = kept.rels.iter().find(|r| r.id == rid && !r.external)?.clone();
    let vml = part(kept, &rel.target)?.clone();
    let pictures = parse(kept, &vml)?;
    // Drop the drawing and the images nothing else that is kept refers to.
    let images: Vec<String> = vml.rels.iter().filter(|r| !r.external).map(|r| r.target.to_ascii_lowercase()).collect();
    kept.elements.remove(el);
    kept.rels.retain(|r| r.id != rid);
    kept.parts.retain(|q| !q.name.eq_ignore_ascii_case(&vml.name));
    let used: Vec<String> = kept
        .parts
        .iter()
        .flat_map(|q| &q.rels)
        .chain(&kept.rels)
        .chain(kept.vml.iter().flat_map(|v| &v.rels))
        .map(|r| r.target.to_ascii_lowercase())
        .collect();
    kept.parts.retain(|q| {
        let name = q.name.to_ascii_lowercase();
        !images.contains(&name) || used.contains(&name)
    });
    Some(pictures)
}

/// Moves a sheet's kept header and footer pictures into its print settings
/// ([`take_from_kept`]). `false` when it keeps a drawing that can't be modelled (it stays kept).
pub(crate) fn take_into(sheet: &mut Sheet) -> bool {
    let Some(arc) = sheet.preserved.take() else { return true };
    let mut p = Arc::unwrap_or_clone(arc);
    let ok = match take_from_kept(&mut p) {
        Some(pictures) => {
            sheet.print.hf_pictures = pictures;
            true
        }
        None => !p.elements.iter().any(|e| e.slot == "legacyDrawingHF"),
    };
    sheet.preserved = (!p.is_empty()).then(|| Arc::new(p));
    ok
}

/// The pictures of a header/footer drawing, at most one per section (the first shape for a
/// section wins). `None` when it can't be read as text, names the VML namespace with another
/// prefix than `v` (its shapes wouldn't all be found), or a shape can't be modelled.
fn parse(kept: &Preserved, vml: &PreservedPart) -> Option<Vec<HfPicture>> {
    // A drawing may be UTF-16: read from a UTF-8 copy.
    let utf8 = crate::xml::to_utf8(&vml.data);
    let text = std::str::from_utf8(&utf8).ok()?;
    if other_vml_prefix(text) {
        return None;
    }
    let mut out: Vec<HfPicture> = vec![];
    for (a, b) in shape_spans(text) {
        let pic = picture(kept, vml, text.get(a..b)?)?;
        if out.iter().any(|p| p.is_at(pic.page, pic.footer, usize::from(pic.section))) {
            continue;
        }
        if out.len() >= MAX_HF_PICTURES {
            return None;
        }
        out.push(pic);
    }
    Some(out)
}

/// Whether `text` binds the VML namespace to a prefix other than `v` (or makes it the default
/// namespace): shapes are found as `<v:shape`, so such a drawing's would be missed.
fn other_vml_prefix(text: &str) -> bool {
    text.match_indices("urn:schemas-microsoft-com:vml").any(|(i, ns)| {
        let quoted = text.get(i + ns.len()..).is_some_and(|after| after.starts_with(['"', '\'']));
        let Some(name) = text[..i].strip_suffix(['"', '\'']).map(str::trim_end).and_then(|s| s.strip_suffix('=')).map(str::trim_end) else {
            return false;
        };
        quoted && name.rsplit_once(char::is_whitespace).map_or(name, |(_, n)| n).strip_prefix("xmlns").is_some_and(|prefix| prefix != ":v")
    })
}

/// The kept part named `name` (part names compare without case, as in OPC).
fn part<'a>(kept: &'a Preserved, name: &str) -> Option<&'a PreservedPart> {
    kept.parts.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// One `<v:shape>…</v:shape>` as a picture, or `None` when it isn't a readable one.
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
    let mime = if image.content_type.starts_with("image/") { image.content_type.clone() } else { mime_of(&image.name).to_string() };
    let title = attr(data_tag, "o:title").map(unescape).unwrap_or_default().chars().take(255).collect();
    let adjust = ADJUST.iter().filter_map(|k| Some((k.to_string(), attr(data_tag, k)?.to_string()))).filter(|(_, v)| is_plain_value(v)).collect();
    Some(HfPicture { page, footer, section, width, height, data: Arc::new(image.data.to_vec()), mime, title, adjust })
}

/// A media type from a part name's extension.
fn mime_of(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    crate::drawing::image_content_type(match ext.as_str() {
        "jpg" => "jpeg",
        e => e,
    })
}

/// A setting value safe to write back as it is: short, made of digits, letters, `.`, `-`, `#`.
fn is_plain_value(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '#'))
}

/// `s` with XML's five entities replaced by their characters.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Where a shape id (`LH`, `CFFIRST`, `RHEVEN`…) puts its picture.
fn position(id: &str) -> Option<(HfPage, bool, u8)> {
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

/// The shape id of a picture's place (the inverse of [`position`]).
fn shape_id(p: &HfPicture) -> String {
    let side = ["L", "C", "R"].get(usize::from(p.section)).copied().unwrap_or("C");
    let part = if p.footer { "F" } else { "H" };
    let suffix = match p.page {
        HfPage::Odd => "",
        HfPage::First => "FIRST",
        HfPage::Even => "EVEN",
    };
    format!("{side}{part}{suffix}")
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
/// they are.
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

// ---------------------------------------------------------------- writing

/// Writes sheet `sheet`'s header and footer pictures as Excel does: a VML drawing with one
/// shape per picture, its relationships to the images in `xl/media`, and the sheet's
/// relationship to it. Returns the `<legacyDrawingHF>` element for the sheet, `None` when the
/// sheet has no pictures, or keeps a drawing from the file (written back as it was, so the sheet
/// still has one). Its shape ids use the next free VML shape block, so they are unique in
/// the package.
pub(crate) fn write(sheet: &Sheet, n: usize, rels: &mut Rels, out: &mut Out) -> Option<String> {
    let pictures: Vec<&HfPicture> = sheet.print.hf_pictures.iter().filter(|p| !p.data.is_empty()).take(MAX_HF_PICTURES).collect();
    // A sheet has one header/footer drawing: one kept from the file (it couldn't be modelled)
    // is written back as it was, and wins.
    let kept = sheet.preserved.as_deref().is_some_and(|k| k.elements.iter().any(|e| e.slot == "legacyDrawingHF"));
    if pictures.is_empty() || kept {
        return None;
    }
    // The next free shape block: after those kept drawings use and every drawing written so far.
    let block = out.kept.notes_block(out.vml_block.saturating_add(1));
    out.vml_block = out.vml_block.saturating_add(1);
    let mut drels = Rels::default();
    let mut vml = format!(
        "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\"><o:shapelayout v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"{block}\"/></o:shapelayout>{SHAPETYPE}"
    );
    for (i, p) in pictures.iter().enumerate() {
        let ext = crate::drawing::image_ext(&p.mime, &p.data);
        out.default_type(ext, crate::drawing::image_content_type(ext));
        let file = out.image_part(ext, &p.data);
        let rid = drels.add("image", &format!("../media/{file}"));
        let spid = u64::from(block).saturating_mul(1024).saturating_add(i as u64 + 1);
        let size = |v: f32| num(f64::from(if v.is_finite() { v.clamp(0.75, MAX_SIDE_PT) } else { 72.0 }));
        let mut adjust = String::new();
        for (k, v) in p.adjust.iter().filter(|(k, v)| ADJUST.contains(&k.as_str()) && is_plain_value(v)) {
            let _ = write!(adjust, " {k}=\"{v}\"");
        }
        let _ = write!(
            vml,
            "<v:shape id=\"{}\" o:spid=\"_x0000_s{spid}\" type=\"#_x0000_t75\" style=\"position:absolute;margin-left:0;margin-top:0;width:{}pt;height:{}pt;z-index:{}\"><v:imagedata o:relid=\"{rid}\" o:title=\"{}\"{adjust}/><o:lock v:ext=\"edit\" rotation=\"t\"/></v:shape>",
            shape_id(p),
            size(p.width),
            size(p.height),
            i + 1,
            esc_attr(&p.title),
        );
    }
    vml.push_str("</xml>");
    out.default_type("vml", CT_VML);
    let name = format!("xl/drawings/vmlDrawingHF{n}.vml");
    out.part(&name, None, vml.into_bytes());
    out.part(&format!("xl/drawings/_rels/vmlDrawingHF{n}.vml.rels"), None, drels.xml().into_bytes());
    let rid = rels.add("vmlDrawing", &format!("../drawings/vmlDrawingHF{n}.vml"));
    Some(format!("<legacyDrawingHF r:id=\"{rid}\"/>"))
}

/// The VML picture frame shape type (ECMA-376 Part 4, §14.1: `o:spt="75"`, a frame its picture
/// fills), which every picture shape refers to.
const SHAPETYPE: &str = "<v:shapetype id=\"_x0000_t75\" coordsize=\"21600,21600\" o:spt=\"75\" o:preferrelative=\"t\" path=\"m@4@5l@4@11@9@11@9@5xe\" filled=\"f\" stroked=\"f\"><v:stroke joinstyle=\"miter\"/><v:formulas><v:f eqn=\"if lineDrawn pixelLineWidth 0\"/><v:f eqn=\"sum @0 1 0\"/><v:f eqn=\"sum 0 0 @1\"/><v:f eqn=\"prod @2 1 2\"/><v:f eqn=\"prod @3 21600 pixelWidth\"/><v:f eqn=\"prod @3 21600 pixelHeight\"/><v:f eqn=\"sum @0 0 1\"/><v:f eqn=\"prod @6 1 2\"/><v:f eqn=\"prod @7 21600 pixelWidth\"/><v:f eqn=\"sum @8 21600 0\"/><v:f eqn=\"prod @7 21600 pixelHeight\"/><v:f eqn=\"sum @10 21600 0\"/></v:formulas><v:path o:extrusionok=\"f\" gradientshapeok=\"t\" o:connecttype=\"rect\"/><o:lock v:ext=\"edit\" aspectratio=\"t\"/></v:shapetype>";

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_model::{PreservedRel, PreservedXml};

    /// A relationship from a kept part to `target`.
    fn rel(id: &str, target: &str) -> PreservedRel {
        PreservedRel { id: id.into(), rel_type: "x/image".into(), target: target.into(), external: false }
    }

    /// Kept content with a header/footer drawing `vml` and two images.
    fn kept(vml: &str) -> Preserved {
        let part = |name: &str, data: &[u8], rels: Vec<PreservedRel>| PreservedPart {
            name: name.into(),
            content_type: if name.ends_with(".png") { "image/png".into() } else { String::new() },
            data: Arc::from(data),
            rels,
        };
        Preserved {
            rels: vec![rel("rId3", "xl/drawings/vmlDrawing2.vml")],
            elements: vec![PreservedXml { slot: "legacyDrawingHF".into(), xml: "<legacyDrawingHF xmlns:r=\"x\" r:id=\"rId3\"/>".into() }],
            parts: vec![
                part("xl/drawings/vmlDrawing2.vml", vml.as_bytes(), vec![rel("rId1", "xl/media/image1.png"), rel("rId2", "xl/media/image2.jpeg")]),
                part("xl/media/image1.png", b"one", vec![]),
                part("xl/media/image2.jpeg", b"two", vec![]),
            ],
            ..Preserved::default()
        }
    }

    /// A picture shape `id` with `style`, showing the image `rid`.
    fn shape(id: &str, style: &str, rid: &str) -> String {
        format!(
            r##"<v:shape id="{id}" o:spid="_x0000_s1025" type="#_x0000_t75" style="{style}"><v:imagedata o:relid="{rid}" o:title="x"/></v:shape>"##
        )
    }

    /// A drawing's pictures move into the model, each with its place, size, image, title and
    /// crop/colour settings, and the drawing and its images leave what is kept.
    #[test]
    fn pictures_move_from_kept_content_into_the_model() {
        let vml = [
            shape("LH", "position:absolute;margin-left:0;width:120pt;height:40pt", "rId1"),
            r##"<v:shape id="CFFIRST" style="width:1in;height:0.5in"><v:imagedata o:relid="rId2" o:title="Logo &amp; co" cropleft="0.1" gain="74473f" bilevel="t&quot;x"/></v:shape>"##.into(),
            shape("RHEVEN", "width:96px;height:2.54cm", "rId1"),
        ]
        .concat();
        let mut k = kept(&format!("<xml>{vml}</xml>"));
        let pics = take_from_kept(&mut k).unwrap();
        let got: Vec<_> = pics.iter().map(|p| (p.page, p.footer, p.section, p.width, p.height.round(), p.data.as_slice(), p.mime.as_str())).collect();
        assert_eq!(
            got,
            [
                (HfPage::Odd, false, 0, 120.0, 40.0, b"one".as_slice(), "image/png"),
                (HfPage::First, true, 1, 72.0, 36.0, b"two".as_slice(), "image/jpeg"),
                (HfPage::Even, false, 2, 72.0, 72.0, b"one".as_slice(), "image/png"),
            ]
        );
        assert_eq!(pics[1].title, "Logo & co");
        // Crop and colour settings are kept; a value that isn't plain is not.
        assert_eq!(pics[1].adjust, [("cropleft".to_string(), "0.1".to_string()), ("gain".into(), "74473f".into())]);
        // Nothing of the drawing is left to pass through.
        assert!(k.is_empty(), "{k:?}");
    }

    /// A drawing with a shape that can't be modelled stays kept as it was, and an image another
    /// kept part uses stays when its drawing moves.
    #[test]
    fn a_drawing_with_a_shape_that_cant_be_modelled_stays_kept() {
        for bad in [
            shape("XH", "width:10pt;height:10pt", "rId1"),       // unknown position
            shape("LH", "width:abc;height:10pt", "rId1"),        // bad size
            shape("CH", "width:10pt;height:10pt", "rId9"),       // missing relationship
            shape("LF", "width:10furlongs;height:10pt", "rId1"), // unknown unit
        ] {
            let vml = [shape("RF", "width:5pt;height:5pt", "rId2"), bad].concat();
            let mut k = kept(&vml);
            let before = k.clone();
            assert!(take_from_kept(&mut k).is_none());
            assert_eq!(k, before);
        }
        // An image another kept part also uses stays.
        let mut k = kept(&shape("LH", "width:5pt;height:5pt", "rId1"));
        k.parts.push(PreservedPart { name: "xl/other.xml".into(), rels: vec![rel("rIdX", "xl/media/image1.png")], ..PreservedPart::default() });
        assert_eq!(take_from_kept(&mut k).map(|p| p.len()), Some(1));
        assert!(k.parts.iter().any(|p| p.name == "xl/media/image1.png"));
        assert!(!k.parts.iter().any(|p| p.name.ends_with(".vml") || p.name.ends_with(".jpeg")));
    }

    /// No drawing, no drawing part or a drawing that isn't text gives no pictures.
    #[test]
    fn nothing_without_the_drawing() {
        assert!(take_from_kept(&mut Preserved::default()).is_none());
        let mut k = kept(&shape("LH", "width:1pt;height:1pt", "rId1"));
        k.parts.retain(|p| !p.name.ends_with(".vml"));
        assert!(take_from_kept(&mut k).is_none());
    }

    /// Each shape id gives back the place it was read from.
    #[test]
    fn shape_ids_round_trip() {
        for id in ["LH", "CH", "RH", "LF", "CF", "RF", "LHFIRST", "CFEVEN", "RFFIRST"] {
            let (page, footer, section) = position(id).unwrap();
            let p = HfPicture {
                page,
                footer,
                section,
                width: 1.0,
                height: 1.0,
                data: Arc::default(),
                mime: String::new(),
                title: String::new(),
                adjust: vec![],
            };
            assert_eq!(shape_id(&p), id);
        }
    }

    /// Attributes are matched by their whole name, in either quote style.
    #[test]
    fn attributes_need_their_own_name() {
        let tag = r#"<v:shape o:spid="_x0000_s1" id='CH' xid="no""#;
        assert_eq!(attr(tag, "id"), Some("CH"));
        assert_eq!(attr(r#"<v:imagedata o:relid = "rId4""#, "o:relid"), Some("rId4"));
        assert_eq!(attr("<a b=c>", "b"), None);
    }
}
