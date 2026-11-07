//! The custom document properties part (`docProps/custom.xml`) and the two notices a workbook with
//! Python cells carries in it, for whoever opens the file with openpyxl, which can read neither the
//! code nor a picture inside a cell, and removes both when it saves.
//!
//! - **At load:** openpyxl warns about a property of a type it doesn't model, naming the property. The
//!   name of the `vt:bstr` property is therefore a sentence, printed as
//!   `UserWarning: Unknown type for <the sentence>`. It also reaches `pd.read_excel`.
//! - **On request:** the `vt:lpwstr` property [`NOTICE_NAME`], in `wb.custom_doc_props`, says where
//!   things are. openpyxl keeps it through `wb.save()`, and a later reader learns why the code is gone.
//!
//! They aren't a person's own properties. Reading takes them out of what is kept, and a save writes
//! them from the workbook it has, so a workbook with no Python cells loses them. The other properties
//! stay as they came: a part with neither is kept byte for byte, and one that has to be rewritten keeps
//! the XML of each. A part that isn't well formed stays as it came too, without the notices: nothing in
//! it can be told from anything else, and openpyxl can't read it anyway. Each text is at most 255
//! characters, the limit Excel's interface has for a property, and every one is escaped, as an unescaped
//! `<` in this part stops `load_workbook`.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::ops::Range;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::write::{Out, XML_DECL};
use crate::xml::{self, esc, esc_attr};

const NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
const CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
const PART: &str = "docProps/custom.xml";
/// The format identifier the format requires of a custom property.
const FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";

/// The property that says where things are, by this name in `wb.custom_doc_props`.
pub const NOTICE_NAME: &str = "GridCraft.Python.Notice";
pub const NOTICE: &str = "Pictures: ws._images (needs Pillow; _data() closes a picture's stream). descr gridcraft:python:<n>:<ref>:<alt> is a linked picture, gridcraft:pythonCell:<cell>:<density>:<alt> a Python cell's. Code: xl/python.xml, by zipfile. wb.save() removes the code.";
/// The property whose name openpyxl prints at load.
pub const WARNING_NAME: &str = "GridCraft: this workbook has Python cells. openpyxl cannot read their code or in-cell pictures, and wb.save() removes them. See wb.custom_doc_props.";
pub const WARNING_VALUE: &str = "See GridCraft.Python.Notice";
/// How the warning's name begins: an older or newer wording is the same property.
const WARNING_START: &str = "GridCraft: this workbook has Python cells.";

fn is_notice(name: &str) -> bool {
    name == NOTICE_NAME || name.starts_with(WARNING_START)
}

/// A custom properties part, read as far as its properties.
struct Scan {
    text: String,
    /// The root's start tag, open for content.
    open: String,
    /// The root's end tag.
    close: String,
    /// Where the root's content is in `text`.
    content: Range<usize>,
    properties: Vec<Property>,
}

struct Property {
    span: Range<usize>,
    notice: bool,
    pid: Option<i64>,
}

/// `None` for a part that isn't well formed, or isn't a list of properties.
fn scan(bytes: &[u8]) -> Option<Scan> {
    let text = String::from_utf8(xml::to_utf8(bytes).into_owned()).ok()?;
    let mut reader = Reader::from_str(&text);
    let mut root: Option<(String, usize)> = None;
    let mut properties = vec![];
    loop {
        let start = usize::try_from(reader.buffer_position()).ok()?;
        let (e, has_end) = match reader.read_event().ok()? {
            Event::Eof => return None,
            Event::End(_) => {
                let (open, content_start) = root?;
                let end = usize::try_from(reader.buffer_position()).ok()?;
                let close = text.get(start..end)?.to_string();
                return Some(Scan { open, close, content: content_start..start, properties, text });
            }
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            _ => continue,
        };
        if root.is_none() {
            if e.local_name().as_ref() != b"Properties" {
                return None;
            }
            let end = usize::try_from(reader.buffer_position()).ok()?;
            if has_end {
                root = Some((text.get(start..end)?.to_string(), end));
                continue;
            }
            let open = format!("{}>", text.get(start..end)?.strip_suffix("/>")?);
            let close = format!("</{}>", std::str::from_utf8(e.name().as_ref()).ok()?);
            return Some(Scan { open, close, content: end..end, properties, text });
        }
        let property = xml::start_el(&e);
        if has_end {
            reader.read_to_end(e.name()).ok()?;
        }
        if property.name == "property" {
            let end = usize::try_from(reader.buffer_position()).ok()?;
            properties.push(Property { span: start..end, notice: property.attr("name").is_some_and(is_notice), pid: property.attr_i64("pid") });
        }
    }
}

impl Scan {
    /// The root's content without the notices.
    fn content_without_notices(&self) -> Option<String> {
        let mut out = String::with_capacity(self.content.len());
        let mut from = self.content.start;
        for p in self.properties.iter().filter(|p| p.notice) {
            out.push_str(self.text.get(from..p.span.start)?);
            from = p.span.end;
        }
        out.push_str(self.text.get(from..self.content.end)?);
        Some(out)
    }

    fn part(&self, added: &str) -> Option<Vec<u8>> {
        let content = self.content_without_notices()?;
        Some(format!("{XML_DECL}{}{content}{added}{}", self.open, self.close).into_bytes())
    }
}

/// What a kept custom properties part is without the notices.
pub enum Stripped {
    /// It had none, and stays as it came.
    Unchanged,
    /// It holds something else.
    Part(Vec<u8>),
    /// They were all it had.
    Nothing,
}

pub fn without_notices(bytes: &[u8]) -> Stripped {
    let Some(scan) = scan(bytes) else { return Stripped::Unchanged };
    if !scan.properties.iter().any(|p| p.notice) {
        return Stripped::Unchanged;
    }
    // Any other content keeps the part: an element beside the properties isn't one of them.
    if scan.properties.iter().all(|p| p.notice) && scan.content_without_notices().is_some_and(|rest| rest.trim().is_empty()) {
        return Stripped::Nothing;
    }
    scan.part("").map_or(Stripped::Unchanged, Stripped::Part)
}

/// The two notices, with `pid`s the part doesn't use. In a part whose root doesn't declare what they
/// need, each declares it itself.
fn notices(used: &HashSet<i64>, declared: bool) -> String {
    let mut free = (2i64..).filter(|pid| !used.contains(pid));
    let (property_ns, value_ns) =
        if declared { (String::new(), String::new()) } else { (format!(" xmlns=\"{NS}\""), format!(" xmlns:vt=\"{NS_VT}\"")) };
    let mut out = String::new();
    for (name, tag, value) in [(NOTICE_NAME, "lpwstr", NOTICE), (WARNING_NAME, "bstr", WARNING_VALUE)] {
        let pid = free.next().unwrap_or(2);
        let _ = write!(
            out,
            "<property{property_ns} fmtid=\"{FMTID}\" pid=\"{pid}\" name=\"{}\"><vt:{tag}{value_ns}>{}</vt:{tag}></property>",
            esc_attr(name),
            esc(value)
        );
    }
    out
}

/// The custom properties part of a workbook with Python cells: `existing` with the notices, or a new
/// part with only them. `None` when `existing` isn't well formed, and has to stay as it is.
fn with_notices(existing: Option<&[u8]>) -> Option<Vec<u8>> {
    let Some(existing) = existing else {
        let mut part = format!("{XML_DECL}<Properties xmlns=\"{NS}\" xmlns:vt=\"{NS_VT}\">");
        part.push_str(&notices(&HashSet::new(), true));
        part.push_str("</Properties>");
        return Some(part.into_bytes());
    };
    let scan = scan(existing)?;
    let used: HashSet<i64> = scan.properties.iter().filter(|p| !p.notice).filter_map(|p| p.pid).collect();
    let declared = scan.open.contains(&format!("xmlns=\"{NS}\"")) && scan.open.contains(&format!("xmlns:vt=\"{NS_VT}\""));
    scan.part(&notices(&used, declared))
}

/// Gives a package with Python cells its notices, in the custom properties part it kept, or in a new
/// one with its content type and relationship.
pub fn write_notices(out: &mut Out) {
    if out.python_scripts.is_empty() {
        return;
    }
    match out.kept.custom_properties_part() {
        Some(name) => {
            if let Some(merged) = out.part_data(&name).and_then(|data| with_notices(Some(data))) {
                out.replace_part(&name, merged);
            }
        }
        None => {
            let Some(part) = with_notices(None) else { return };
            let name = out.kept.claim(PART);
            out.part(&name, Some(CONTENT_TYPE), part);
            out.kept.add_root(REL, format!("/{name}"));
        }
    }
}
