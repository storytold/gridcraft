//! A small XML tree built on `quick-xml`, with streaming of repeated elements (worksheet rows,
//! shared strings) so large parts never need a full tree, plus escaping helpers for writing.

use std::fmt::Write as _;

use gridcraft_model::{PhoneticProps, PhoneticRun, PhoneticString};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::IoError;

/// Maximum element nesting accepted in any part.
pub const MAX_DEPTH: usize = 256;

/// Most elements kept in one part's parsed XML tree: an emptied `El` costs well over a hundred
/// bytes, so a highly-compressible part of endless empty elements could fit the package byte
/// limit and still take gigabytes of tree before any semantic check ran. Parts whose big runs
/// matter (worksheet rows, shared strings) are streamed instead and never hold this tree.
pub const MAX_XML_NODES: usize = 1_000_000;

/// An element: local name (namespace prefix dropped), attributes by local name, children and
/// its direct text content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<El>,
    pub text: String,
}

impl El {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn attr_u32(&self, k: &str) -> Option<u32> {
        self.attr(k).and_then(|v| v.trim().parse::<u32>().ok())
    }
    pub fn attr_i64(&self, k: &str) -> Option<i64> {
        self.attr(k).and_then(|v| v.trim().parse::<i64>().ok())
    }
    pub fn attr_f64(&self, k: &str) -> Option<f64> {
        self.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }
    /// xsd:boolean (`1`/`true`/`0`/`false`).
    pub fn attr_bool(&self, k: &str) -> Option<bool> {
        match self.attr(k)?.trim() {
            "1" | "true" | "on" | "t" => Some(true),
            "0" | "false" | "off" | "f" => Some(false),
            _ => None,
        }
    }
    pub fn flag(&self, k: &str, default: bool) -> bool {
        self.attr_bool(k).unwrap_or(default)
    }
    pub fn child(&self, name: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Follows a path of child names.
    pub fn path(&self, names: &[&str]) -> Option<&El> {
        let mut e = self;
        for n in names {
            e = e.child(n)?;
        }
        Some(e)
    }
    /// `val` attribute of a child (the common `<c:x val=".."/>` pattern).
    pub fn child_val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.attr("val"))
    }
}

/// Text of a SpreadsheetML string item (`<si>`, `<is>`, comment `<text>`): `<t>` and rich text
/// runs `<r><t>`, phonetic runs ignored (read them with [`phonetic_data`] instead — they are hints,
/// not part of the text). Decodes `_xHHHH_` escapes.
pub fn rich_text(e: &El) -> String {
    let mut s = String::new();
    for c in &e.children {
        match c.name.as_str() {
            "t" => s.push_str(&c.text),
            "r" => {
                for t in c.kids("t") {
                    s.push_str(&t.text);
                }
            }
            _ => {}
        }
    }
    decode_xstring(&s)
}

/// The phonetic (furigana) data of a string item: its `<rPh>` runs in document order, and its
/// `<phoneticPr>` when present.
///
/// `base` is the item's base text ([`rich_text`]); the offsets are validated against it and never
/// used to index anything. Behaviour reference: ISO/IEC 29500-1 §18.4.6 `rPh` (`sb` and `eb` are
/// zero-based offsets into the base text with `sb < eb`, both within its length; child `<t>` holds
/// the reading) and §18.4.7 `phoneticPr` — the `PhoneticRun` and `PhoneticProperties` classes on
/// Microsoft Learn:
/// <https://learn.microsoft.com/en-us/previous-versions/office/developer/office-2010/cc820083(v=office.14)>,
/// <https://learn.microsoft.com/en-us/previous-versions/office/developer/office-2010/cc797016(v=office.14)>.
/// Damaged runs are dropped rather than repaired: one without both offsets, with `sb >= eb`, wholly
/// past the end of the base text, or without a reading; the caller reports how many.
pub fn phonetic_data(e: &El, base: &str) -> (Vec<PhoneticRun>, Option<PhoneticProps>) {
    // The base text's length in UTF-16 code units: the widest reading of "the total length of the
    // base text" (§18.4.6), so a run is only dropped when it cannot refer to the text at all.
    let len = u32::try_from(base.encode_utf16().count()).unwrap_or(u32::MAX);
    let mut runs = Vec::new();
    for r in e.kids("rPh") {
        let (Some(sb), Some(eb)) = (r.attr_u32("sb"), r.attr_u32("eb")) else { continue };
        let start = sb.min(len);
        let end = eb.min(len);
        if start >= end {
            continue;
        }
        let reading = r.child("t").map(|t| decode_xstring(&t.text)).unwrap_or_default();
        if reading.is_empty() {
            continue;
        }
        runs.push(PhoneticRun { start, end, text: reading });
    }
    (runs, e.child("phoneticPr").map(phonetic_props))
}

/// `<phoneticPr>` attributes (ISO/IEC 29500-1 §18.4.7 `CT_PhoneticPr`: `fontId` is required,
/// `type`, `alignment`). `type` and `alignment` are kept only when they are values §18.18.57
/// `ST_PhoneticType` / §18.18.56 `ST_PhoneticAlignment` define, so a damaged file cannot make the
/// saved file invalid; a missing or unusable `fontId` becomes 0, the Normal style's font, which an
/// out-of-range index falls back to anyway.
/// `ST_PhoneticType` (ISO/IEC 29500-1 §18.18.57), as `phoneticPr/@type` may write them.
const PHONETIC_KINDS: &[&str] = &["halfwidthKatakana", "fullwidthKatakana", "Hiragana", "noConversion"];
/// `ST_PhoneticAlignment` (ISO/IEC 29500-1 §18.18.56), as `phoneticPr/@alignment` may write them.
const PHONETIC_ALIGNMENTS: &[&str] = &["noControl", "left", "center", "distributed"];

fn phonetic_props(e: &El) -> PhoneticProps {
    const KINDS: &[&str] = PHONETIC_KINDS;
    const ALIGNMENTS: &[&str] = PHONETIC_ALIGNMENTS;
    PhoneticProps {
        font_id: e.attr_u32("fontId").unwrap_or(0),
        kind: e.attr("type").map(str::trim).filter(|v| KINDS.contains(v)).map(str::to_string),
        alignment: e.attr("alignment").map(str::trim).filter(|v| ALIGNMENTS.contains(v)).map(str::to_string),
    }
}

/// Converts input bytes to UTF-8 XML text (handles BOMs and UTF-16).
pub(crate) fn to_utf8(xml: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if let Some(rest) = xml.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::borrow::Cow::Borrowed(rest);
    }
    let utf16 = |le: bool, b: &[u8]| -> Vec<u8> {
        let units = b.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        let s: String = char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect();
        // The declaration may claim UTF-16; quick-xml without the encoding feature ignores it.
        s.into_bytes()
    };
    if let Some(rest) = xml.strip_prefix(&[0xFF, 0xFE]) {
        return std::borrow::Cow::Owned(utf16(true, rest));
    }
    if let Some(rest) = xml.strip_prefix(&[0xFE, 0xFF]) {
        return std::borrow::Cow::Owned(utf16(false, rest));
    }
    // UTF-16 without BOM: `<\0?\0`.
    if xml.len() >= 4 && xml[1] == 0 && xml[3] == 0 && xml[0] != 0 {
        return std::borrow::Cow::Owned(utf16(true, xml));
    }
    if xml.len() >= 4 && xml[0] == 0 && xml[2] == 0 && xml[1] != 0 {
        return std::borrow::Cow::Owned(utf16(false, xml));
    }
    std::borrow::Cow::Borrowed(xml)
}

fn local(name: &[u8]) -> String {
    let n = match name.iter().rposition(|&b| b == b':') {
        Some(i) => name.get(i + 1..).unwrap_or(name),
        None => name,
    };
    String::from_utf8_lossy(n).into_owned()
}

fn start_el(e: &BytesStart<'_>) -> El {
    let mut el = El { name: local(e.name().as_ref()), ..Default::default() };
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key.as_ref();
        // Namespace declarations carry no data for us.
        if key == b"xmlns" || key.starts_with(b"xmlns:") {
            continue;
        }
        let v = match a.normalized_value(XmlVersion::Explicit1_0) {
            Ok(v) => v.into_owned(),
            Err(_) => String::from_utf8_lossy(&a.value).into_owned(),
        };
        el.attrs.push((local(key), v));
    }
    el
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

/// Parses a whole part into a tree.
pub fn parse(xml: &[u8]) -> Result<El, IoError> {
    parse_streaming(xml, &[], &mut |_| Ok(()))
}

/// Parses a part; complete elements named in `split` (below the root) are handed to `cb` and not
/// kept in the returned tree.
pub fn parse_streaming(xml: &[u8], split: &[&str], cb: &mut dyn FnMut(El) -> Result<(), IoError>) -> Result<El, IoError> {
    let data = to_utf8(xml);
    let mut reader = Reader::from_reader(data.as_ref());
    {
        let cfg = reader.config_mut();
        cfg.check_end_names = false;
        cfg.allow_unmatched_ends = true;
        cfg.allow_dangling_amp = true;
        cfg.trim_text_start = false;
        cfg.trim_text_end = false;
    }
    let mut stack: Vec<El> = Vec::new();
    let mut root: Option<El> = None;
    let mut buf = Vec::new();
    // Elements kept in the tree so far, so a pathological part that passes the byte limits can
    // still be refused before the tree itself outgrows memory.
    let mut kept = 0usize;
    // Attach a finished element to its parent (or emit it / make it the root).
    fn finish(
        stack: &mut [El],
        root: &mut Option<El>,
        el: El,
        split: &[&str],
        cb: &mut dyn FnMut(El) -> Result<(), IoError>,
        kept: &mut usize,
    ) -> Result<(), IoError> {
        match stack.last_mut() {
            Some(parent) => {
                if split.contains(&el.name.as_str()) {
                    cb(el)?;
                } else {
                    *kept = kept.saturating_add(1);
                    if *kept > MAX_XML_NODES {
                        return Err(IoError::Xml(format!("the part holds more than {MAX_XML_NODES} elements")));
                    }
                    parent.children.push(el);
                }
            }
            None => {
                if root.is_none() {
                    *root = Some(el);
                }
            }
        }
        Ok(())
    }
    loop {
        let ev = reader.read_event_into(&mut buf).map_err(|e| IoError::Xml(format!("{e} at byte {}", reader.buffer_position())))?;
        match ev {
            Event::Start(e) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(IoError::Xml("XML nested too deeply".into()));
                }
                stack.push(start_el(&e));
            }
            Event::Empty(e) => {
                let el = start_el(&e);
                finish(&mut stack, &mut root, el, split, cb, &mut kept)?;
            }
            Event::End(_) => {
                if let Some(el) = stack.pop() {
                    finish(&mut stack, &mut root, el, split, cb, &mut kept)?;
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    let s = t.xml_content(XmlVersion::Explicit1_0).map_err(|e| IoError::Xml(e.to_string()))?;
                    top.text.push_str(&s);
                }
            }
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::GeneralRef(r) => {
                if let Some(top) = stack.last_mut() {
                    match r.resolve_char_ref() {
                        Ok(Some(c)) => top.text.push(c),
                        _ => {
                            let name = String::from_utf8_lossy(&r).into_owned();
                            match named_entity(&name) {
                                Some(c) => top.text.push(c),
                                None => {
                                    top.text.push('&');
                                    top.text.push_str(&name);
                                    top.text.push(';');
                                }
                            }
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    // Unclosed elements (truncated part): close them so we keep what we have.
    while let Some(el) = stack.pop() {
        finish(&mut stack, &mut root, el, split, cb, &mut kept)?;
    }
    root.ok_or_else(|| IoError::Xml("no root element".into()))
}

// ---------------------------------------------------------------- writing helpers

/// Escapes text content.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '\r' => o.push_str("&#13;"),
            c if is_xml_char(c) => o.push(c),
            _ => {}
        }
    }
    o
}

/// Escapes an attribute value (double-quoted).
pub fn esc_attr(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\n' => o.push_str("&#10;"),
            '\r' => o.push_str("&#13;"),
            '\t' => o.push_str("&#9;"),
            c if is_xml_char(c) => o.push(c),
            _ => {}
        }
    }
    o
}

fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// ST_Xstring encoding for cell text: characters XML can't carry and carriage returns become
/// `_xHHHH_`, and literal `_xHHHH_` sequences are protected with `_x005F_`. The result still needs
/// [`esc`].
pub fn encode_xstring(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '_' && looks_like_escape(&chars, i) {
            o.push_str("_x005F_");
        } else if !is_xml_char(c) || (c.is_control() && !matches!(c, '\t' | '\n')) {
            o.push_str(&format!("_x{:04X}_", c as u32 & 0xFFFF));
        } else {
            o.push(c);
        }
    }
    o
}

fn looks_like_escape(chars: &[char], i: usize) -> bool {
    chars.get(i + 1) == Some(&'x') && chars.get(i + 6) == Some(&'_') && (i + 2..i + 6).all(|k| chars.get(k).is_some_and(|c| c.is_ascii_hexdigit()))
}

fn escape_unit(chars: &[char], i: usize) -> Option<u32> {
    let hex: String = chars.get(i + 2..i + 6)?.iter().collect();
    u32::from_str_radix(&hex, 16).ok()
}

/// The character for a high surrogate escape at `i` (value `high`) followed directly by a low surrogate escape.
fn surrogate_pair(chars: &[char], i: usize, high: u32) -> Option<char> {
    if !(0xD800..=0xDBFF).contains(&high) || chars.get(i + 7) != Some(&'_') || !looks_like_escape(chars, i + 7) {
        return None;
    }
    let low = escape_unit(chars, i + 7).filter(|l| (0xDC00..=0xDFFF).contains(l))?;
    char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00))
}

/// Inverse of [`encode_xstring`].
pub fn decode_xstring(s: &str) -> String {
    if !s.contains("_x") {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut o = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '_' && looks_like_escape(&chars, i) {
            let unit = escape_unit(&chars, i);
            if let Some(ch) = unit.and_then(char::from_u32) {
                o.push(ch);
                i += 7;
                continue;
            }
            // A high surrogate escape followed by a low surrogate escape is one UTF-16 pair.
            if let Some(ch) = unit.and_then(|high| surrogate_pair(&chars, i, high)) {
                o.push(ch);
                i += 14;
                continue;
            }
        }
        o.push(chars[i]);
        i += 1;
    }
    o
}

/// `<t>` element with `xml:space="preserve"` when needed.
pub fn t_el(s: &str) -> String {
    let enc = esc(&encode_xstring(s));
    if s.starts_with([' ', '\t', '\n', '\r']) || s.ends_with([' ', '\t', '\n', '\r']) || s.contains('\n') {
        format!("<t xml:space=\"preserve\">{enc}</t>")
    } else {
        format!("<t>{enc}</t>")
    }
}

/// The contents of a shared string item (`<si>`): the base text, then — when the string has
/// phonetic (furigana) data — its `<rPh sb="…" eb="…"><t>…</t></rPh>` runs in document order and
/// its `<phoneticPr …/>`. Behaviour reference: ISO/IEC 29500-1 §18.4.8 `si` with §18.4.6 `rPh` and
/// §18.4.7 `phoneticPr`. A string without phonetic data gives exactly [`t_el`]'s output.
///
/// The runs and the properties are independent: `<phoneticPr>` is a valid child of `<si>` on its
/// own (§18.4.8's `CT_SharedStringItem` allows it without any `rPh`), so a string whose runs were
/// all dropped still keeps its settings.
pub fn si_body(text: &str, phonetic: Option<&PhoneticString>) -> String {
    let mut s = t_el(text);
    let Some(p) = phonetic else { return s };
    // The model is public and restored from GridCraft JSON too, so the reader's checks are not the
    // only way data gets here: validate again and write only what the schema allows (§18.4.6: an
    // `sb` before `eb`, both inside the base text, a non-empty reading; §18.18.56/57: a known enum
    // value). Damaged data is dropped, never written back invalid.
    let len = u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
    for r in &p.runs {
        let start = r.start.min(len);
        let end = r.end.min(len);
        if start >= end || r.text.is_empty() {
            continue;
        }
        let _ = write!(s, "<rPh sb=\"{start}\" eb=\"{end}\">{}</rPh>", t_el(&r.text));
    }
    if let Some(pr) = &p.props {
        let _ = write!(s, "<phoneticPr fontId=\"{}\"", pr.font_id);
        if let Some(kind) = pr.kind.as_deref().filter(|kind| PHONETIC_KINDS.contains(kind)) {
            let _ = write!(s, " type=\"{}\"", esc_attr(kind));
        }
        if let Some(alignment) = pr.alignment.as_deref().filter(|a| PHONETIC_ALIGNMENTS.contains(a)) {
            let _ = write!(s, " alignment=\"{}\"", esc_attr(alignment));
        }
        s.push_str("/>");
    }
    s
}

/// Formats a number for an XML attribute/value (shortest round-trip form).
pub fn num(n: f64) -> String {
    if !n.is_finite() {
        return "0".into();
    }
    if n == n.trunc() && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    format!("{n:?}").replace('e', "E")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn si_without_phonetics_is_written_exactly_as_before() {
        for s in ["plain", " leading ", "a & <b>", "line\nbreak", "日本語", "x_x0041_"] {
            assert_eq!(si_body(s, None), t_el(s));
        }
    }

    #[test]
    fn a_part_of_endless_empty_elements_is_refused() {
        assert!(parse(b"<r><x/><y t=\"1\"/><x/></r>").is_ok());
        let xml = format!("<r>{}<y/></r>", "<x/>".repeat(MAX_XML_NODES + 1));
        let err = parse(xml.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("elements"), "{err}");
    }

    #[test]
    fn phonetic_data_validates_offsets() {
        let x = r#"<si><t>課きく</t><rPh sb="0" eb="1"><t>カ</t></rPh><rPh sb="2" eb="9"><t>ク</t></rPh><rPh sb="2" eb="2"><t>x</t></rPh><rPh sb="1"/></si>"#;
        let si = parse(x.as_bytes()).unwrap();
        let (runs, props) = phonetic_data(&si, &rich_text(&si));
        assert_eq!(runs, vec![PhoneticRun { start: 0, end: 1, text: "カ".into() }, PhoneticRun { start: 2, end: 3, text: "ク".into() }]);
        assert!(props.is_none());
    }

    /// The write path validates too: a `PhoneticString` that came from JSON or an API caller (not
    /// from this reader) is written only where the schema allows it — an `sb` before `eb`, both
    /// inside the base text, a reading that isn't empty, and known `type`/`alignment` values.
    #[test]
    fn si_body_drops_damaged_phonetic_data() {
        // 課きく is three UTF-16 units.
        let text = "課きく";
        let damaged = PhoneticString {
            text: text.into(),
            runs: vec![
                PhoneticRun { start: 8, end: 9, text: "x".into() },    // past the text
                PhoneticRun { start: 2, end: 2, text: "x".into() },    // empty range
                PhoneticRun { start: 0, end: 1, text: String::new() }, // no reading
                PhoneticRun { start: 1, end: 3, text: "く".into() },   // valid, clamped end
            ],
            props: Some(PhoneticProps { font_id: 7, kind: Some("bogus".into()), alignment: Some("center".into()) }),
        };
        let s = si_body(text, Some(&damaged));
        assert_eq!(s, "<t>課きく</t><rPh sb=\"1\" eb=\"3\"><t>く</t></rPh><phoneticPr fontId=\"7\" alignment=\"center\"/>");
        // With no run left, valid properties are still written: `<phoneticPr>` is a legal child of
        // `<si>` on its own, and Excel keeps the settings when the last reading is removed.
        let empty = PhoneticString { text: text.into(), runs: vec![], props: damaged.props.clone() };
        assert_eq!(si_body(text, Some(&empty)), "<t>課きく</t><phoneticPr fontId=\"7\" alignment=\"center\"/>");
        // With neither runs nor properties there is nothing to write.
        let none = PhoneticString { text: text.into(), runs: vec![], props: None };
        assert_eq!(si_body(text, Some(&none)), t_el(text));
        // A known kind is written; the schema default ("none") is not invented for a missing one.
        let known = PhoneticString {
            text: text.into(),
            runs: vec![PhoneticRun { start: 0, end: 1, text: "カ".into() }],
            props: Some(PhoneticProps { font_id: 1, kind: Some("Hiragana".into()), alignment: None }),
        };
        assert_eq!(si_body(text, Some(&known)), "<t>課きく</t><rPh sb=\"0\" eb=\"1\"><t>カ</t></rPh><phoneticPr fontId=\"1\" type=\"Hiragana\"/>");
    }

    #[test]
    fn parse_basic() {
        let x = br#"<?xml version="1.0"?><a:root xmlns:a="x" k="v &amp; w"><b>t &lt; u&#65;</b><c/><![CDATA[<raw>]]></a:root>"#;
        let e = parse(x).unwrap();
        assert_eq!(e.name, "root");
        assert_eq!(e.attr("k"), Some("v & w"));
        assert_eq!(e.child("b").unwrap().text, "t < uA");
        assert!(e.child("c").is_some());
        assert_eq!(e.text, "<raw>");
    }

    #[test]
    fn streaming_split() {
        let x = b"<r><row n='1'/><row n='2'><c/></row><other/></r>";
        let mut n = 0;
        let root = parse_streaming(x, &["row"], &mut |e| {
            assert_eq!(e.name, "row");
            n += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 2);
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn deep_nesting_is_an_error() {
        let mut s = String::new();
        for _ in 0..10_000 {
            s.push_str("<a>");
        }
        assert!(parse(s.as_bytes()).is_err());
    }

    #[test]
    fn garbage() {
        assert!(parse(b"").is_err());
        assert!(parse(b"\x00\x01\x02 not xml").is_err() || parse(b"\x00\x01\x02 not xml").is_ok());
        let _ = parse(b"<a><b></a>");
        let _ = parse(b"<a attr='&bogus;'>&unknown;</a>");
    }

    #[test]
    fn xstring() {
        assert_eq!(decode_xstring("a_x000D_b"), "a\rb");
        assert_eq!(encode_xstring("a\r\nb\rc"), "a_x000D_\nb_x000D_c");
        assert_eq!(decode_xstring(&encode_xstring("a\r\nb\rc")), "a\r\nb\rc");
        assert_eq!(encode_xstring("a\u{1}b"), "a_x0001_b");
        assert_eq!(decode_xstring(&encode_xstring("_x0041_")), "_x0041_");
        assert_eq!(decode_xstring("_x00"), "_x00");
    }

    #[test]
    fn xstring_surrogate_pairs() {
        assert_eq!(decode_xstring("a_xD83D__xDE00_b"), "a\u{1F600}b");
        assert_eq!(decode_xstring("_xd83d__xde00_"), "\u{1F600}");
        // Lone surrogates stay as literal escape text.
        assert_eq!(decode_xstring("_xD83D_"), "_xD83D_");
        assert_eq!(decode_xstring("_xDE00_"), "_xDE00_");
        assert_eq!(decode_xstring("_xD83D_x"), "_xD83D_x");
        assert_eq!(decode_xstring("_xD83D__x0041_"), "_xD83D_A");
        // A protected literal escape is not a low surrogate.
        assert_eq!(decode_xstring("_xD83D__x005F_xDE00_"), "_xD83D__xDE00_");
    }

    #[test]
    fn text_escaping_preserves_carriage_returns() {
        assert_eq!(esc("a\rb"), "a&#13;b");
        assert_eq!(esc_attr("a\rb"), "a&#13;b");
    }

    #[test]
    fn numbers() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.1), "0.1");
        assert_eq!(num(1e-7), "1E-7");
        assert_eq!(num(-3.0), "-3");
        assert_eq!(num(1.5e20), "1.5E20");
        assert_eq!("1.5E20".parse::<f64>().unwrap(), 1.5e20);
    }
}
