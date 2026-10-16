//! Counts drawing occurrences per worksheet before import and after export. It intentionally
//! does not compare geometry, styles, relationship IDs or shared media part names.

use super::{Category, Diffs, MAX_ENTRIES, MAX_XML_PART, Zip, part_key, resolve_target};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::name::{QName, ResolveResult};
use quick_xml::{NsReader, XmlVersion};

const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const MAX_NODES: usize = 200_000;
const MAX_DEPTH: usize = 128;
const MAX_TOTAL: u64 = 256 * 1024 * 1024;

fn namespace(ns: &str, family: &str) -> bool {
    match family {
        "sheet" => matches!(ns, "http://schemas.openxmlformats.org/spreadsheetml/2006/main" | "http://purl.oclc.org/ooxml/spreadsheetml/main"),
        "drawing" => matches!(
            ns,
            "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" | "http://purl.oclc.org/ooxml/drawingml/spreadsheetDrawing"
        ),
        "chart" => matches!(
            ns,
            "http://schemas.openxmlformats.org/drawingml/2006/chart"
                | "http://purl.oclc.org/ooxml/drawingml/chart"
                | "http://schemas.microsoft.com/office/drawing/2014/chartex"
        ),
        "main" => matches!(ns, "http://schemas.openxmlformats.org/drawingml/2006/main" | "http://purl.oclc.org/ooxml/drawingml/main"),
        "rel" => matches!(
            ns,
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships" | "http://purl.oclc.org/ooxml/officeDocument/relationships"
        ),
        _ => false,
    }
}

#[derive(Default)]
struct Node {
    ns: String,
    name: String,
    attrs: Vec<(String, String, String)>,
    children: Vec<Node>,
    chart_choice: bool,
}

impl Node {
    fn is(&self, family: &str, name: &str) -> bool {
        namespace(&self.ns, family) && self.name == name
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(ns, n, _)| ns.is_empty() && n == name).map(|(_, _, v)| v.as_str())
    }
    fn rid(&self) -> Option<&str> {
        self.attrs.iter().find(|(ns, n, _)| namespace(ns, "rel") && n == "id").map(|(_, _, v)| v.as_str())
    }
}

fn resolved(ns: ResolveResult<'_>) -> Result<String, String> {
    match ns {
        ResolveResult::Bound(n) => Ok(String::from_utf8_lossy(n.as_ref()).into_owned()),
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Unknown(_) => Err("unbound XML namespace prefix".into()),
    }
}

fn parse(bytes: &[u8], remaining: &mut usize, worksheet: bool) -> Result<Node, String> {
    let mut reader = NsReader::from_reader(bytes);
    let mut stack: Vec<Node> = Vec::new();
    let mut root = None;
    let mut depth = 0;
    let mut skipped = None;
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if depth >= MAX_DEPTH {
                    return Err("drawing inspection XML depth limit exceeded".into());
                }
                let start = matches!(event, Event::Start(_));
                if skipped.is_some() {
                    depth += usize::from(start);
                    continue;
                }
                let (ns, name) = reader.resolver_mut().resolve_element(e.name());
                let mut node = Node { ns: resolved(ns)?, name: String::from_utf8_lossy(name.as_ref()).into_owned(), ..Default::default() };
                // A worksheet can contain millions of cells. Stream past cell data and other
                // unrelated direct children without retaining their attributes or descendants.
                if worksheet && depth == 1 && !node.is("sheet", "drawing") && !(node.ns == MC && node.name == "AlternateContent") {
                    if start {
                        depth += 1;
                        skipped = Some(depth);
                    }
                    continue;
                }
                if *remaining == 0 {
                    return Err("drawing inspection XML node limit exceeded".into());
                }
                *remaining -= 1;
                for a in e.attributes() {
                    let a = a.map_err(|e| e.to_string())?;
                    if a.key.as_ref() == b"xmlns" || a.key.as_ref().starts_with(b"xmlns:") {
                        continue;
                    }
                    let (ns, name) = reader.resolver_mut().resolve_attribute(a.key);
                    node.attrs.push((
                        resolved(ns)?,
                        String::from_utf8_lossy(name.as_ref()).into_owned(),
                        a.normalized_value(XmlVersion::Explicit1_0).map_err(|e| e.to_string())?.into_owned(),
                    ));
                }
                // Chartex has an explicit standard chart choice and a shape fallback. Count
                // the chart once, as the importer does, rather than adding both representations.
                if node.ns == MC
                    && node.name == "Choice"
                    && let Some(requires) = node.attr("Requires")
                {
                    node.chart_choice = !requires.trim().is_empty()
                        && requires.split_whitespace().all(|prefix| {
                            let qname = format!("{prefix}:chart");
                            let (ns, _) = reader.resolver_mut().resolve_element(QName(qname.as_bytes()));
                            matches!(ns, ResolveResult::Bound(n) if matches!(n.as_ref(),
                                b"http://schemas.microsoft.com/office/drawing/2014/chartex"
                                | b"http://schemas.microsoft.com/office/drawing/2015/9/8/chartex"
                                | b"http://schemas.microsoft.com/office/drawing/2015/10/21/chartex"))
                        });
                }
                if start {
                    depth += 1;
                    stack.push(node);
                } else {
                    append(&mut stack, &mut root, node)?;
                }
            }
            Event::End(_) => {
                if skipped.is_some() {
                    if skipped == Some(depth) {
                        skipped = None;
                    }
                    depth = depth.checked_sub(1).ok_or("unbalanced XML")?;
                    continue;
                }
                depth = depth.checked_sub(1).ok_or("unbalanced XML")?;
                let node = stack.pop().ok_or("unbalanced XML")?;
                append(&mut stack, &mut root, node)?;
            }
            Event::DocType(_) => return Err("DTD is unsupported in drawing inspection".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || !stack.is_empty() {
        return Err("incomplete XML".into());
    }
    root.ok_or_else(|| "empty XML".into())
}

fn append(stack: &mut [Node], root: &mut Option<Node>, node: Node) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err("multiple XML roots".into());
    }
    Ok(())
}

struct Package<'a> {
    zip: Zip<'a>,
    names: BTreeMap<String, String>,
    bytes_left: u64,
    nodes_left: usize,
}

impl<'a> Package<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        if zip.len() > MAX_ENTRIES {
            return Err("drawing inspection ZIP entry limit exceeded".into());
        }
        let mut names = BTreeMap::new();
        for i in 0..zip.len() {
            let f = zip.by_index_raw(i).map_err(|e| e.to_string())?;
            if !f.is_dir() && names.insert(part_key(f.name()), f.name().to_string()).is_some() {
                return Err("duplicate package part name".into());
            }
        }
        Ok(Self { zip, names, bytes_left: MAX_TOTAL, nodes_left: MAX_NODES })
    }
    fn xml(&mut self, path: &str) -> Result<Node, String> {
        self.xml_projected(path, false)
    }
    fn xml_projected(&mut self, path: &str, worksheet: bool) -> Result<Node, String> {
        let name = self.names.get(&part_key(path)).ok_or_else(|| format!("missing part {path}"))?;
        let file = self.zip.by_name(name).map_err(|e| e.to_string())?;
        let limit = MAX_XML_PART.min(self.bytes_left);
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > limit {
            return Err("drawing inspection XML byte limit exceeded".into());
        }
        self.bytes_left -= bytes.len() as u64;
        parse(&bytes, &mut self.nodes_left, worksheet).map_err(|e| format!("{path}: {e}"))
    }
    fn relationships(&mut self, part: &str) -> Result<BTreeMap<String, (String, String)>, String> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let path = if part.is_empty() {
            "_rels/.rels".into()
        } else if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        if !self.names.contains_key(&part_key(&path)) {
            return Ok(BTreeMap::new());
        }
        let root = self.xml(&path)?;
        if root.ns != RELS || root.name != "Relationships" {
            return Err(format!("unsupported relationships XML in {path}"));
        }
        let mut out = BTreeMap::new();
        for child in root.children {
            if child.ns != RELS || child.name != "Relationship" {
                return Err(format!("unsupported relationship element in {path}"));
            }
            if child.attr("TargetMode").is_some_and(|v| v.eq_ignore_ascii_case("External")) {
                continue;
            }
            let id = child.attr("Id").ok_or("relationship missing Id")?;
            let kind = child.attr("Type").ok_or("relationship missing Type")?;
            let target = child.attr("Target").ok_or("relationship missing Target")?;
            if out.insert(id.to_string(), (kind.to_string(), resolve_target(dir, target))).is_some() {
                return Err("duplicate relationship Id".into());
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    pictures: usize,
    charts: usize,
    shapes: usize,
}

impl Counts {
    fn add(&mut self, other: Self) {
        self.pictures += other.pictures;
        self.charts += other.charts;
        self.shapes += other.shapes;
    }
}

fn objects(node: &Node) -> Result<Counts, String> {
    if node.ns == MC && node.name == "AlternateContent" {
        if let Some(choice) = node.children.iter().find(|c| c.ns == MC && c.name == "Choice" && c.chart_choice) {
            return children(choice);
        }
        let mut choices = Vec::new();
        for branch in &node.children {
            if branch.ns != MC || !matches!(branch.name.as_str(), "Choice" | "Fallback") {
                return Err("unsupported AlternateContent branch".into());
            }
            choices.push(children(branch)?);
        }
        let first = choices.first().copied().ok_or("empty AlternateContent")?;
        if choices.iter().any(|c| *c != first) {
            return Err("ambiguous AlternateContent drawing counts".into());
        }
        return Ok(first);
    }
    if !namespace(&node.ns, "drawing") {
        return Err(format!("unsupported drawing element {{{}}}{}", node.ns, node.name));
    }
    match node.name.as_str() {
        "pic" => Ok(Counts { pictures: 1, ..Default::default() }),
        "sp" | "cxnSp" => Ok(Counts { shapes: 1, ..Default::default() }),
        "graphicFrame" => {
            let graphics: Vec<_> = node.children.iter().filter(|c| c.is("main", "graphic")).collect();
            let data: Vec<_> = graphics.iter().flat_map(|g| &g.children).filter(|c| c.is("main", "graphicData")).collect();
            if graphics.len() == 1
                && data.len() == 1
                && data.first().is_some_and(|d| d.children.len() == 1 && d.children.first().is_some_and(|c| c.is("chart", "chart")))
            {
                Ok(Counts { charts: 1, ..Default::default() })
            } else {
                Err("unsupported or ambiguous drawing graphicFrame".into())
            }
        }
        "wsDr" | "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" | "grpSp" => children(node),
        "from" | "to" | "pos" | "ext" | "clientData" | "nvGrpSpPr" | "grpSpPr" => Ok(Counts::default()),
        _ => Err(format!("unsupported drawing object {}", node.name)),
    }
}

fn children(node: &Node) -> Result<Counts, String> {
    let mut counts = Counts::default();
    for child in &node.children {
        counts.add(objects(child)?);
    }
    Ok(counts)
}

fn is_kind(uri: &str, kind: &str) -> bool {
    uri.rsplit_once('/').is_some_and(|(ns, name)| namespace(ns, "rel") && name == kind)
}

/// None means another input format (CSV, ODS, XLSB), not an empty XLSX inventory.
fn inventory(bytes: &[u8]) -> Result<Option<BTreeMap<String, Counts>>, String> {
    if !bytes.starts_with(b"PK") || zip::ZipArchive::new(Cursor::new(bytes)).is_err() {
        return Ok(None);
    }
    let mut package = Package::open(bytes)?;
    let rels = package.relationships("")?;
    let books: Vec<_> = rels.values().filter(|(kind, _)| is_kind(kind, "officeDocument")).collect();
    if books.is_empty() {
        return Ok(None);
    }
    if books.len() != 1 {
        return Err("ambiguous workbook relationship".into());
    }
    let book_path = &books.first().ok_or("missing workbook")?.1;
    if book_path.to_ascii_lowercase().ends_with(".bin") {
        return Ok(None);
    }
    let book = package.xml(book_path)?;
    if !book.is("sheet", "workbook") {
        return Ok(None);
    }
    let rels = package.relationships(book_path)?;
    let mut sheets = BTreeMap::new();
    for sheet in book.children.iter().filter(|c| c.is("sheet", "sheets")).flat_map(|c| &c.children) {
        if !sheet.is("sheet", "sheet") {
            return Err("unsupported workbook sheet entry".into());
        }
        let name = sheet.attr("name").ok_or("sheet missing name")?;
        let rid = sheet.rid().ok_or("sheet missing relationship")?;
        let (kind, path) = rels.get(rid).ok_or("unresolved worksheet relationship")?;
        // Chartsheets and legacy dialog/macro sheets are preserved separately. This category
        // deliberately inventories worksheet drawings, not those other sheet formats.
        if kind.rsplit('/').next().is_some_and(|k| matches!(k, "chartsheet" | "dialogsheet" | "xlMacrosheet" | "xlIntlMacrosheet")) {
            continue;
        }
        if !is_kind(kind, "worksheet") {
            return Err(format!("{name}: unsupported sheet kind {kind}"));
        }
        let worksheet = package.xml_projected(path, true)?;
        if !worksheet.is("sheet", "worksheet") {
            return Err(format!("{name}: unsupported worksheet XML"));
        }
        let drawing_refs: Vec<_> = worksheet.children.iter().filter(|c| c.is("sheet", "drawing")).collect();
        fn has_drawing(n: &Node) -> bool {
            n.is("sheet", "drawing") || n.children.iter().any(has_drawing)
        }
        if worksheet.children.iter().any(|c| c.ns == MC && c.name == "AlternateContent" && has_drawing(c)) {
            return Err(format!("{name}: AlternateContent worksheet needs manual drawing inspection"));
        }
        let mut count = Counts::default();
        if !drawing_refs.is_empty() {
            let drawing_rels = package.relationships(path)?;
            for drawing in drawing_refs {
                let rid = drawing.rid().ok_or("drawing missing relationship")?;
                let (kind, part) = drawing_rels.get(rid).ok_or("unresolved drawing relationship")?;
                if !is_kind(kind, "drawing") {
                    return Err(format!("{name}: unexpected drawing relationship type"));
                }
                let root = package.xml(part)?;
                if !root.is("drawing", "wsDr") {
                    return Err(format!("{name}: unsupported drawing root"));
                }
                count.add(objects(&root).map_err(|e| format!("{name}: {e}"))?);
            }
        }
        if sheets.insert(name.to_string(), count).is_some() {
            return Err("duplicate worksheet name".into());
        }
    }
    Ok(Some(sheets))
}

pub(super) fn compare(source: &[u8], written: &[u8], max: usize) -> Category {
    let mut diffs = Diffs::new("drawings", max);
    let before = match inventory(source) {
        Ok(Some(v)) => v,
        Ok(None) => return diffs.done(),
        Err(e) => {
            diffs.push(|| format!("source drawing inspection incomplete: {e}"));
            return diffs.done();
        }
    };
    let after = match inventory(written) {
        Ok(Some(v)) => v,
        Ok(None) => {
            diffs.push(|| "written drawing inspection unavailable".into());
            return diffs.done();
        }
        Err(e) => {
            diffs.push(|| format!("written drawing inspection incomplete: {e}"));
            return diffs.done();
        }
    };
    for (sheet, a) in before {
        let b = after.get(&sheet).copied().unwrap_or_default();
        for (kind, before, after) in [("pictures", a.pictures, b.pictures), ("charts", a.charts, b.charts), ("shapes", a.shapes, b.shapes)] {
            if before > after {
                diffs.push(|| format!("{sheet}: {kind} {before} -> {after} ({} lost)", before - after));
            }
        }
    }
    diffs.done()
}
