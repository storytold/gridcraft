//! Pass-through of package content GridCraft does not model, so that open → edit → save keeps it.
//!
//! On read, the raw bytes of unmodelled parts are kept with their content type and their
//! relationship graph, per owner: the package root (custom document properties, the sensitivity
//! label, ribbon customisation, add-in task panes), the workbook (custom XML, links to external
//! workbooks with their `<externalReferences>`, data connections, XML maps) and each sheet (query
//! tables, cells mapped to XML elements, form controls, ActiveX controls, OLE objects, the sheet
//! background, printer settings and custom properties, with the sheet's `<customProperties>`,
//! `<picture>`, `<oleObjects>` and `<controls>` XML, the `r:id` of `<pageSetup>` and the legacy
//! VML shapes those controls need). The workbook also keeps its `<fileVersion>` and the
//! calcFeatures extension, which Excel needs to trust the saved values.
//!
//! On write, kept parts get fresh part names where needed and new relationship ids; the `r:id`s in
//! kept XML are rewritten to match. Kept content is conservative:
//! - Anything whose relationship graph reaches a part GridCraft reads and rewrites (worksheets,
//!   drawings, charts, tables, PivotTables, comments, slicers…) is not kept: it would point at
//!   content that no longer exists in the same form.
//! - External relationship targets (the path of a linked workbook) are kept as text and never
//!   opened, followed or refreshed. A relative path to a linked workbook is rewritten when the
//!   workbook is saved into another folder ([`rebase_links`]), so it still names the same file.
//! - Sheet content lives on the sheet: deleting the sheet drops it, Copy Sheet doesn't copy it.
//!   Cell references inside kept parts (a control's linked cell, a query table's range, a cell
//!   mapped to an XML element) are kept as they were, not adjusted to inserted or deleted rows.
//! - Strict Open XML files are not passed through (their namespaces differ from what we write).
//! - Sizes are bounded: per part and per workbook.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Component, Path};
use std::sync::Arc;

use gridcraft_model::{Preserved, PreservedPart, PreservedRel, PreservedSheet, PreservedXml, Sheet, Visibility, Workbook};
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::{Reader, Writer};

use crate::IoError;
use crate::package::{self, normalize};
use crate::read::Ctx;
use crate::write::{NS_REL, Out, Rels, XML_DECL};
use crate::xml::{self, esc_attr};

/// Most bytes kept for one workbook.
pub const MAX_KEPT_TOTAL: usize = 256 * 1024 * 1024;
/// Largest single kept part.
pub const MAX_KEPT_PART: u64 = 64 * 1024 * 1024;
/// Most parts kept for one workbook.
const MAX_KEPT_PARTS: usize = 4096;
/// Most relationships read from one rels part.
const MAX_RELS: usize = 10_000;
/// Largest relationships, content-types or VML part read here.
const MAX_META: u64 = 16 * 1024 * 1024;
/// Longest chain of relationships followed from an owner.
const MAX_CHAIN: usize = 16;
/// Largest kept XML fragment of a sheet.
const MAX_FRAGMENT: usize = 4 * 1024 * 1024;
/// Deepest element nesting accepted in a kept fragment.
const MAX_FRAGMENT_DEPTH: usize = 256;

#[cfg(test)]
thread_local! {
    /// A smaller kept-bytes allowance for tests on this thread.
    pub(crate) static KEPT_TOTAL_FOR_TEST: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_KEPT_TOTAL) };
}

fn kept_total() -> usize {
    #[cfg(test)]
    {
        KEPT_TOTAL_FOR_TEST.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        MAX_KEPT_TOTAL
    }
}

const NS_REL_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
const CT_VML: &str = "application/vnd.openxmlformats-officedocument.vmlDrawing";

/// Relationship kinds (last segment of the type) kept from the package root.
const PACKAGE_ROOTS: &[&str] = &["custom-properties", "classificationlabels", "extensibility", "webextensiontaskpanes"];
/// Relationship kinds kept from the workbook without a workbook element.
const WORKBOOK_ROOTS: &[&str] = &["customXml", "connections", "xmlMaps"];
/// Relationship kinds kept from a sheet without a sheet element.
const SHEET_ROOTS: &[&str] = &["queryTable", "tableSingleCells"];
/// Sheet relationship kinds that mean the sheet XML may hold elements worth keeping.
const SHEET_ELEMENT_KINDS: &[&str] = &["oleObject", "ctrlProp", "control", "image", "package", "printerSettings", "customProperty"];
/// Attributes of `<fileVersion>` (CT_FileVersion).
const FILE_VERSION_ATTRS: &[&str] = &["appName", "lastEdited", "lowestEdited", "rupBuild", "codeName"];
/// The workbook extension listing the calculation features the saved values were computed with.
const CALC_FEATURES_URI: &str = "{B58B0392-4F1F-4190-BB64-5DF3571DCE5F}";
const NS_CALC_FEATURES: &str = "http://schemas.microsoft.com/office/spreadsheetml/2018/calcfeatures";
/// Kept sheet elements written after `<legacyDrawing>` (before `<tableParts>`), in schema order.
const SHEET_SLOTS: &[&str] = &["picture", "oleObjects", "controls"];
/// Kept sheet elements: [`SHEET_SLOTS`] and `<customProperties>` (written after `<colBreaks>`).
const KEPT_SLOTS: &[&str] = &["customProperties", "picture", "oleObjects", "controls"];

/// Relationship kinds GridCraft reads and rewrites, or that depend on content it rewrites:
/// kept content whose graph reaches one of them is dropped.
const REWRITTEN: &[&str] = &[
    "officedocument",
    "core-properties",
    "extended-properties",
    "worksheet",
    "chartsheet",
    "dialogsheet",
    "xlmacrosheet",
    "drawing",
    "chart",
    "chartex",
    "chartstyle",
    "chartcolorstyle",
    "chartusershapes",
    "table",
    "pivottable",
    "pivotcachedefinition",
    "pivotcacherecords",
    "comments",
    "vmldrawing",
    "threadedcomment",
    "person",
    "sharedstrings",
    "styles",
    "theme",
    "sheetmetadata",
    "calcchain",
    "vbaproject",
    "rdrichvalue",
    "rdrichvaluestructure",
    "rdrichvaluetypes",
    "rdrichvaluewebimage",
    "rdarray",
    "rdsupportingpropertybag",
    "rdsupportingpropertybagstructure",
    "richvaluerel",
    "slicer",
    "slicercache",
    "timeline",
    "timelinecache",
    "powerpivotdata",
];

/// Relationship kinds a chart sheet reaches that are otherwise rewritten: its drawing, chart and
/// their style, colour and shape parts are kept byte for byte (the chart sheet is not modelled).
const CHART_SHEET_GRAPH: &[&str] = &["chartsheet", "drawing", "chart", "chartex", "chartstyle", "chartcolorstyle", "chartusershapes"];

/// Part-name prefixes (lower case) GridCraft's writer never produces: kept parts there keep their
/// original names. Anything else is renamed so it can't collide with written parts.
const OWN_DIRS: &[&str] = &[
    "customxml/",
    "docprops/custom.xml",
    "docmetadata/",
    "customui/",
    "xl/externallinks/",
    "xl/connections.xml",
    "xl/xmlmaps.xml",
    "xl/querytables/",
    "xl/ctrlprops/",
    "xl/embeddings/",
    "xl/activex/",
    "xl/webextensions/",
    "xl/printersettings/",
    "xl/customproperty",
];

fn kind(rel_type: &str) -> &str {
    rel_type.rsplit('/').next().unwrap_or("")
}

fn is_kind(r: &PreservedRel, kinds: &[&str]) -> bool {
    let k = kind(&r.rel_type);
    kinds.iter().any(|x| x.eq_ignore_ascii_case(k))
}

fn rewritten(r: &PreservedRel) -> bool {
    !r.external && REWRITTEN.contains(&kind(&r.rel_type).to_ascii_lowercase().as_str())
}

fn rels_part_name(part: &str) -> String {
    let (dir, file) = package::split_dir(part);
    if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") }
}

/// A part's relationships with full type URIs. `Ok(vec![])` when it has none; an error when the
/// rels part is damaged or too large (its part then can't be kept faithfully).
fn raw_rels(cx: &mut Ctx<'_>, part: &str) -> Result<Vec<PreservedRel>, String> {
    let name = rels_part_name(part);
    let bytes = match cx.pkg.read_limited(&name, MAX_META) {
        Ok(Some(b)) => b,
        Ok(None) => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let root = xml::parse(&bytes).map_err(|e| format!("{name}: {e}"))?;
    let (dir, _) = package::split_dir(part);
    let mut out = vec![];
    for r in root.kids("Relationship") {
        if out.len() >= MAX_RELS {
            return Err(format!("{name}: too many relationships"));
        }
        let (Some(id), Some(target)) = (r.attr("Id"), r.attr("Target")) else { continue };
        let external = r.attr("TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
        out.push(PreservedRel {
            id: id.to_string(),
            rel_type: r.attr("Type").unwrap_or("").to_string(),
            target: if external { target.to_string() } else { package::resolve(dir, target) },
            external,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- reading

/// The `<sheet>` entries of the workbook that are chart sheets, each with the number of worksheets
/// (model sheets) before it.
fn chart_sheet_entries<'a>(wb_xml: &'a xml::El, wb_rels: &[PreservedRel], file_to_model: &[Option<usize>]) -> Vec<(usize, &'a xml::El)> {
    let mut out = vec![];
    let mut worksheets = 0usize;
    for (i, se) in wb_xml.child("sheets").into_iter().flat_map(|s| s.kids("sheet")).enumerate() {
        if file_to_model.get(i).copied().flatten().is_some() {
            worksheets += 1;
        } else if se.attr("id").and_then(|id| wb_rels.iter().find(|r| r.id == id)).is_some_and(|r| is_kind(r, &["chartsheet"])) {
            out.push((worksheets, se));
        }
    }
    out
}

/// Collects kept content while a package is read.
pub struct KeptReader {
    on: bool,
    strict: bool,
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
    budget: usize,
    parts_left: usize,
}

impl KeptReader {
    pub fn new(cx: &mut Ctx<'_>) -> KeptReader {
        let mut k = KeptReader {
            on: true,
            strict: false,
            defaults: HashMap::new(),
            overrides: HashMap::new(),
            budget: kept_total(),
            parts_left: MAX_KEPT_PARTS,
        };
        if let Ok(root) = raw_rels(cx, "")
            && root.iter().any(|r| r.rel_type.starts_with("http://purl.oclc.org/"))
        {
            k.on = false;
            k.strict = true;
        }
        match cx.pkg.read_limited("[Content_Types].xml", MAX_META).ok().flatten().and_then(|b| xml::parse(&b).ok()) {
            Some(types) => {
                for d in types.kids("Default") {
                    if let (Some(e), Some(t)) = (d.attr("Extension"), d.attr("ContentType")) {
                        k.defaults.insert(e.to_ascii_lowercase(), t.to_string());
                    }
                }
                for o in types.kids("Override") {
                    if let (Some(p), Some(t)) = (o.attr("PartName"), o.attr("ContentType")) {
                        k.overrides.insert(normalize(p), t.to_string());
                    }
                }
            }
            None => k.on = false,
        }
        k
    }

    fn content_type(&self, part: &str) -> Option<String> {
        let key = normalize(part);
        if let Some(t) = self.overrides.get(&key) {
            return Some(t.clone());
        }
        let (_, file) = package::split_dir(&key);
        let ext = file.rsplit_once('.').map(|(_, e)| e)?;
        self.defaults.get(ext).cloned()
    }

    /// The parts reachable from `root` that aren't in `seen` yet. Fails (and keeps nothing) when the
    /// graph reaches rewritten content, a missing or damaged part, or a size limit.
    fn closure(&mut self, cx: &mut Ctx<'_>, root: &PreservedRel, seen: &HashSet<String>, through: &[&str]) -> Result<Vec<PreservedPart>, String> {
        if root.external {
            return Ok(vec![]);
        }
        let blocked = |r: &PreservedRel| rewritten(r) && !through.iter().any(|k| k.eq_ignore_ascii_case(kind(&r.rel_type)));
        if blocked(root) {
            return Err(format!("it refers to {}", kind(&root.rel_type)));
        }
        let mut parts = vec![];
        let mut bytes = 0usize;
        let mut local: HashSet<String> = HashSet::new();
        let mut queue = VecDeque::from([(root.target.clone(), 0usize)]);
        while let Some((name, depth)) = queue.pop_front() {
            let key = normalize(&name);
            if seen.contains(&key) || !local.insert(key) {
                continue;
            }
            if depth > MAX_CHAIN {
                return Err("its relationships are nested too deeply".into());
            }
            if parts.len() >= self.parts_left {
                return Err("it has too many parts".into());
            }
            let content_type = self.content_type(&name).ok_or_else(|| format!("{name} has no content type"))?;
            let data = match cx.pkg.read_limited(&name, MAX_KEPT_PART) {
                Ok(Some(d)) => d,
                Ok(None) => return Err(format!("{name} is missing")),
                Err(e) => return Err(e.to_string()),
            };
            bytes = bytes.saturating_add(data.len());
            if bytes > self.budget {
                return Err("it is too large".into());
            }
            let rels = raw_rels(cx, &name)?;
            for r in rels.iter().filter(|r| !r.external) {
                if blocked(r) {
                    return Err(format!("it refers to {}", kind(&r.rel_type)));
                }
                queue.push_back((r.target.clone(), depth + 1));
            }
            parts.push(PreservedPart { name, content_type, data: Arc::from(data), rels });
        }
        self.budget = self.budget.saturating_sub(bytes);
        self.parts_left = self.parts_left.saturating_sub(parts.len());
        Ok(parts)
    }

    /// Keeps a set of relationships (all or nothing) and the parts behind them.
    fn keep(&mut self, cx: &mut Ctx<'_>, p: &mut Preserved, roots: &[PreservedRel], seen: &mut HashSet<String>) -> Result<(), String> {
        self.keep_through(cx, p, roots, seen, &[])
    }

    /// [`Self::keep`] for roots whose graph may reach the rewritten kinds in `through`.
    fn keep_through(
        &mut self,
        cx: &mut Ctx<'_>,
        p: &mut Preserved,
        roots: &[PreservedRel],
        seen: &mut HashSet<String>,
        through: &[&str],
    ) -> Result<(), String> {
        let mut new_parts: Vec<PreservedPart> = vec![];
        let mut scratch = seen.clone();
        for r in roots {
            let parts = self.closure(cx, r, &scratch, through)?;
            scratch.extend(parts.iter().map(|q| normalize(&q.name)));
            new_parts.extend(parts);
        }
        *seen = scratch;
        p.parts.extend(new_parts);
        for r in roots {
            if !p.rels.iter().any(|x| x.id == r.id) {
                p.rels.push(r.clone());
            }
        }
        Ok(())
    }

    /// Package-root and workbook-level content.
    pub fn workbook(&mut self, cx: &mut Ctx<'_>, wb_part: &str, wb_xml: &xml::El, file_to_model: &[Option<usize>]) -> Option<Arc<Preserved>> {
        if !self.on {
            let wb_rels = raw_rels(cx, wb_part).unwrap_or_default();
            for (_, se) in chart_sheet_entries(wb_xml, &wb_rels, file_to_model) {
                cx.warn(format!("chart sheet \"{}\" was skipped and is not kept on save", se.attr("name").unwrap_or("")));
            }
            if self.strict {
                cx.warn("unmodelled content of a Strict Open XML workbook (custom XML, links, controls…) is not kept on save");
            }
            return None;
        }
        let mut p = Preserved::default();
        let mut seen = HashSet::new();
        // Package root: custom document properties, ribbon customisation, task panes.
        if let Ok(root) = raw_rels(cx, "") {
            for r in root.iter().filter(|r| is_kind(r, PACKAGE_ROOTS)) {
                let mut q = Preserved::default();
                match self.keep(cx, &mut q, std::slice::from_ref(r), &mut seen) {
                    Ok(()) => {
                        p.parts.extend(q.parts);
                        p.package_rels.extend(q.rels);
                    }
                    Err(why) => cx.warn(format!("{} was dropped: {why}", kind(&r.rel_type))),
                }
            }
        }
        let wb_rels = raw_rels(cx, wb_part).unwrap_or_default();
        for r in wb_rels.iter().filter(|r| is_kind(r, WORKBOOK_ROOTS)) {
            if let Err(why) = self.keep(cx, &mut p, std::slice::from_ref(r), &mut seen) {
                cx.warn(format!("{} part {} was dropped: {why}", kind(&r.rel_type), r.target));
            }
        }
        // Chart sheets: the sheet entry (name, state, position) plus the chart sheet part and the
        // drawing, chart, style and colour parts behind it.
        for (worksheets, se) in chart_sheet_entries(wb_xml, &wb_rels, file_to_model) {
            let name = se.attr("name").unwrap_or("").to_string();
            let Some(r) = se.attr("id").and_then(|id| wb_rels.iter().find(|r| r.id == id)) else { continue };
            match self.keep_through(cx, &mut p, std::slice::from_ref(r), &mut seen, CHART_SHEET_GRAPH) {
                Ok(()) => p.sheets.push(PreservedSheet {
                    name,
                    visibility: match se.attr("state") {
                        Some("hidden") => Visibility::Hidden,
                        Some("veryHidden") => Visibility::VeryHidden,
                        _ => Visibility::Visible,
                    },
                    rel_id: r.id.clone(),
                    before: worksheets,
                }),
                Err(why) => cx.warn(format!("chart sheet \"{name}\" was dropped: {why}")),
            }
        }
        // Links to external workbooks: `[1]` in formulas is the position in <externalReferences>,
        // so they are kept all together or not at all.
        let ids: Vec<&str> =
            wb_xml.child("externalReferences").map(|e| e.kids("externalReference").filter_map(|x| x.attr("id")).collect()).unwrap_or_default();
        if !ids.is_empty() {
            let roots: Option<Vec<PreservedRel>> =
                ids.iter().map(|id| wb_rels.iter().find(|r| r.id == *id && is_kind(r, &["externalLink"])).cloned()).collect();
            let kept = match roots {
                Some(roots) => self.keep(cx, &mut p, &roots, &mut seen),
                None => Err("a link has no part".into()),
            };
            match kept {
                Ok(()) => {
                    let mut x = format!("<externalReferences xmlns:r=\"{NS_REL}\">");
                    for id in &ids {
                        let _ = write!(x, "<externalReference r:id=\"{}\"/>", esc_attr(id));
                    }
                    x.push_str("</externalReferences>");
                    p.elements.push(PreservedXml { slot: "externalReferences".into(), xml: x });
                    cx.warn("links to external workbooks were kept; they are not followed or refreshed");
                }
                Err(why) => cx.warn(format!("links to external workbooks were dropped: {why}")),
            }
        }
        // Excel trusts the saved cell values only when both are present; without them it
        // recalculates everything on open, and links into closed workbooks come back as errors.
        if let Some(v) = wb_xml.child("fileVersion") {
            let mut x = String::from("<fileVersion");
            for k in FILE_VERSION_ATTRS {
                if let Some(a) = v.attr(k) {
                    let _ = write!(x, " {k}=\"{}\"", esc_attr(a));
                }
            }
            x.push_str("/>");
            p.elements.push(PreservedXml { slot: "fileVersion".into(), xml: x });
        }
        let calc_features = wb_xml
            .child("extLst")
            .and_then(|l| l.kids("ext").find(|e| e.attr("uri").is_some_and(|u| u.eq_ignore_ascii_case(CALC_FEATURES_URI))))
            .and_then(|e| e.child("calcFeatures"));
        if let Some(c) = calc_features {
            let mut x = format!("<ext uri=\"{CALC_FEATURES_URI}\" xmlns:xcalcf=\"{NS_CALC_FEATURES}\"><xcalcf:calcFeatures>");
            for name in c.kids("feature").filter_map(|f| f.attr("name")) {
                let _ = write!(x, "<xcalcf:feature name=\"{}\"/>", esc_attr(name));
            }
            x.push_str("</xcalcf:calcFeatures></ext>");
            p.elements.push(PreservedXml { slot: "ext".into(), xml: x });
        }
        (!p.is_empty()).then(|| Arc::new(p))
    }

    /// Sheet-level content of the sheet part `part`.
    pub fn sheet(&mut self, cx: &mut Ctx<'_>, part: &str, name: &str) -> Option<Arc<Preserved>> {
        if !self.on {
            return None;
        }
        let rels = match raw_rels(cx, part) {
            Ok(r) => r,
            Err(e) => {
                cx.warn(format!("sheet \"{name}\": relationships could not be read ({e}); unmodelled content was dropped"));
                return None;
            }
        };
        let mut p = Preserved::default();
        let mut seen = HashSet::new();
        for r in rels.iter().filter(|r| is_kind(r, SHEET_ROOTS)) {
            if let Err(why) = self.keep(cx, &mut p, std::slice::from_ref(r), &mut seen) {
                cx.warn(format!("sheet \"{name}\": {} was dropped: {why}", kind(&r.rel_type)));
            }
        }
        if rels.iter().any(|r| is_kind(r, SHEET_ELEMENT_KINDS)) {
            self.sheet_elements(cx, part, name, &rels, &mut p, &mut seen);
        }
        (!p.is_empty()).then(|| Arc::new(p))
    }

    fn sheet_elements(&mut self, cx: &mut Ctx<'_>, part: &str, name: &str, rels: &[PreservedRel], p: &mut Preserved, seen: &mut HashSet<String>) {
        let scan = match cx.pkg.read(part) {
            Ok(Some(bytes)) => scan_sheet(&bytes),
            _ => None,
        };
        let Some(scan) = scan else {
            cx.warn(format!("sheet \"{name}\": controls and embedded objects could not be read and were dropped"));
            return;
        };
        // Printer settings (a binary part per sheet) hang off <pageSetup r:id>.
        if let Some(id) = scan.page_setup.as_deref()
            && let Some(r) = rels.iter().find(|r| r.id == id && is_kind(r, &["printerSettings"]))
        {
            match self.keep(cx, p, std::slice::from_ref(r), seen) {
                Ok(()) => p.elements.push(PreservedXml { slot: "pageSetup".into(), xml: r.id.clone() }),
                Err(why) => cx.warn(format!("sheet \"{name}\": printer settings were dropped: {why}")),
            }
        }
        let mut frags = scan.fragments;
        // Controls and OLE objects are drawn from shapes in the legacy VML drawing.
        let drawn = |slot: &str| slot == "oleObjects" || slot == "controls";
        if frags.iter().any(|(slot, _)| drawn(slot)) {
            match self.vml(cx, rels, scan.legacy_drawing.as_deref(), seen) {
                Ok((vml, parts)) => {
                    p.parts.extend(parts);
                    p.vml = Some(vml);
                }
                Err(why) => {
                    cx.warn(format!("sheet \"{name}\": controls and embedded objects were dropped: {why}"));
                    frags.retain(|(slot, _)| !drawn(slot));
                }
            }
        }
        for (slot, x) in frags {
            let mut ids = vec![];
            let ok = map_rel_ids(&x, &mut |id| {
                ids.push(id.to_string());
                Some(id.to_string())
            })
            .is_some();
            let roots: Option<Vec<PreservedRel>> = ids.iter().map(|id| rels.iter().find(|r| r.id == *id).cloned()).collect();
            let kept = match (ok, roots) {
                (true, Some(roots)) => self.keep(cx, p, &roots, seen),
                (false, _) => Err("its XML is damaged".into()),
                (_, None) => Err("it refers to a missing relationship".into()),
            };
            match kept {
                Ok(()) => p.elements.push(PreservedXml { slot, xml: x }),
                Err(why) => cx.warn(format!("sheet \"{name}\": {slot} were dropped: {why}")),
            }
        }
        if p.elements.iter().any(|e| e.slot == "oleObjects" || e.slot == "controls") {
            cx.warn(format!("sheet \"{name}\": embedded objects and form controls are not shown; they are kept for saving"));
        } else {
            p.vml = None;
        }
    }

    /// The sheet's VML drawing without its notes (GridCraft writes those), and the parts its
    /// shapes refer to (preview images).
    fn vml(
        &mut self,
        cx: &mut Ctx<'_>,
        rels: &[PreservedRel],
        rid: Option<&str>,
        seen: &mut HashSet<String>,
    ) -> Result<(PreservedPart, Vec<PreservedPart>), String> {
        let rel = rid.and_then(|id| rels.iter().find(|r| r.id == id && !r.external)).ok_or("the sheet has no VML drawing")?;
        // Charged to the kept total like any other part: many sheets can point at one highly
        // compressed VML drawing, and each keeps its own copy.
        if self.budget == 0 {
            return Err("it is too large".into());
        }
        let limit = MAX_META.min(u64::try_from(self.budget).unwrap_or(u64::MAX));
        let data = match cx.pkg.read_limited(&rel.target, limit) {
            Ok(Some(d)) => d,
            Ok(None) => return Err(format!("{} is missing", rel.target)),
            Err(IoError::TooLarge(_)) => return Err("it is too large".into()),
            Err(e) => return Err(e.to_string()),
        };
        self.budget = self.budget.saturating_sub(data.len());
        let text = String::from_utf8(data).map_err(|_| "the VML drawing is not UTF-8".to_string())?;
        let text = strip_notes(&text);
        let vrels = raw_rels(cx, &rel.target)?;
        let mut q = Preserved::default();
        let roots: Vec<PreservedRel> = vrels.iter().filter(|r| !r.external).cloned().collect();
        self.keep(cx, &mut q, &roots, seen)?;
        let vml = PreservedPart { name: rel.target.clone(), content_type: CT_VML.into(), data: Arc::from(text.into_bytes()), rels: vrels };
        Ok((vml, q.parts))
    }
}

/// What a sheet part holds for pass-through.
struct SheetScan {
    /// (slot, XML with the namespace declarations it needs).
    fragments: Vec<(String, String)>,
    legacy_drawing: Option<String>,
    /// The relationship id on `<pageSetup>` (printer settings).
    page_setup: Option<String>,
}

/// Finds the sheet's `<customProperties>`, `<picture>`, `<oleObjects>`, `<controls>` (also inside
/// `<mc:AlternateContent>`) and the `<legacyDrawing>` id. `None` when the XML is damaged.
fn scan_sheet(bytes: &[u8]) -> Option<SheetScan> {
    let data = xml::to_utf8(bytes);
    let text = std::str::from_utf8(&data).ok()?;
    let mut reader = Reader::from_str(text);
    let mut decls: Vec<(String, String)> = vec![];
    let mut out = SheetScan { fragments: vec![], legacy_drawing: None, page_setup: None };
    let mut in_root = false;
    loop {
        let start = usize::try_from(reader.buffer_position()).ok()?;
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) if !in_root => {
                in_root = true;
                for a in e.attributes().with_checks(false).flatten() {
                    if let Some(p) = a.key.as_ref().strip_prefix(b"xmlns:")
                        && let (Ok(p), Ok(v)) = (std::str::from_utf8(p), std::str::from_utf8(&a.value))
                        && !v.contains('"')
                    {
                        decls.push((p.to_string(), v.to_string()));
                    }
                }
            }
            Event::Start(e) => {
                let local = e.local_name().as_ref().to_vec();
                match local.as_slice() {
                    b"pageSetup" => out.page_setup = rel_id_attr(&e),
                    b"legacyDrawing" => out.legacy_drawing = rel_id_attr(&e),
                    _ => {}
                }
                reader.read_to_end(e.name()).ok()?;
                let end = usize::try_from(reader.buffer_position()).ok()?;
                let wanted = matches!(local.as_slice(), b"customProperties" | b"picture" | b"oleObjects" | b"controls" | b"AlternateContent");
                if wanted && end.saturating_sub(start) <= MAX_FRAGMENT {
                    let frag = text.get(start..end)?;
                    push_fragment(&mut out, frag, &local, &decls);
                }
            }
            Event::Empty(e) if in_root => {
                let local = e.local_name().as_ref().to_vec();
                if local == b"legacyDrawing" || local == b"pageSetup" {
                    let id = rel_id_attr(&e);
                    if local == b"pageSetup" {
                        out.page_setup = id;
                    } else {
                        out.legacy_drawing = id;
                    }
                } else if local == b"picture"
                    && let Some(frag) = text.get(start..usize::try_from(reader.buffer_position()).ok()?)
                {
                    push_fragment(&mut out, frag, &local, &decls);
                }
            }
            Event::End(_) => break,
            _ => {}
        }
    }
    Some(out)
}

/// The `r:id` of an element (an `id` attribute with a namespace prefix).
fn rel_id_attr(e: &BytesStart<'_>) -> Option<String> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| a.key.local_name().as_ref() == b"id" && a.key.prefix().is_some())
        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
}

fn push_fragment(out: &mut SheetScan, frag: &str, local: &[u8], decls: &[(String, String)]) {
    let frag = with_decls(frag, decls);
    let slot = if local == b"AlternateContent" {
        // The kept element is the one the first choice holds.
        let Ok(el) = xml::parse(frag.as_bytes()) else { return };
        match el.child("Choice").and_then(|c| c.children.first()) {
            Some(c) => c.name.clone(),
            None => return,
        }
    } else {
        String::from_utf8_lossy(local).into_owned()
    };
    if KEPT_SLOTS.contains(&slot.as_str()) && !out.fragments.iter().any(|(s, _)| *s == slot) {
        out.fragments.push((slot, frag));
    }
}

/// Adds the root's namespace declarations a fragment doesn't declare itself.
fn with_decls(frag: &str, decls: &[(String, String)]) -> String {
    let Some(name_end) = frag.char_indices().skip(1).find(|(_, c)| c.is_whitespace() || *c == '/' || *c == '>').map(|(i, _)| i) else {
        return frag.to_string();
    };
    let head = frag.get(..frag.find('>').unwrap_or(frag.len())).unwrap_or("");
    let mut add = String::new();
    for (p, v) in decls {
        if !head.contains(&format!("xmlns:{p}=")) {
            let _ = write!(add, " xmlns:{p}=\"{v}\"");
        }
    }
    format!("{}{add}{}", frag.get(..name_end).unwrap_or(""), frag.get(name_end..).unwrap_or(""))
}

/// Byte ranges of the `<v:shape …>…</v:shape>` (or self-closed) elements in VML text.
fn shape_spans(vml: &str) -> Vec<(usize, usize)> {
    let mut spans = vec![];
    let mut from = 0;
    while let Some(i) = vml.get(from..).and_then(|s| s.find("<v:shape")).map(|i| i + from) {
        let after = i + "<v:shape".len();
        let is_shape = vml.get(after..).and_then(|s| s.chars().next()).is_some_and(|c| c.is_whitespace() || c == '>' || c == '/');
        if !is_shape {
            from = after;
            continue;
        }
        let Some(tag_end) = vml.get(i..).and_then(|s| s.find('>')).map(|j| i + j + 1) else { break };
        let end = if vml.get(..tag_end).is_some_and(|t| t.ends_with("/>")) {
            tag_end
        } else {
            match vml.get(tag_end..).and_then(|s| s.find("</v:shape>")) {
                Some(j) => tag_end + j + "</v:shape>".len(),
                None => break,
            }
        };
        spans.push((i, end));
        from = end;
    }
    spans
}

/// VML without its note shapes (`ObjectType="Note"`).
fn strip_notes(vml: &str) -> String {
    let mut out = String::with_capacity(vml.len());
    let mut last = 0;
    for (a, b) in shape_spans(vml) {
        let shape = vml.get(a..b).unwrap_or("");
        if shape.contains("ObjectType=\"Note\"") {
            out.push_str(vml.get(last..a).unwrap_or(""));
            last = b;
        }
    }
    out.push_str(vml.get(last..).unwrap_or(""));
    out
}

/// Shape numbers (`_x0000_s1025` → 1025) used in VML text.
fn shape_numbers(vml: &str) -> Vec<u64> {
    vml.match_indices("_x0000_s")
        .filter_map(|(i, m)| {
            let digits: String = vml.get(i + m.len()..)?.chars().take_while(char::is_ascii_digit).take(18).collect();
            digits.parse().ok()
        })
        .collect()
}

/// Kept VML (controls, OLE objects) plus GridCraft's note shapes: the notes get ids in a fresh
/// shape block so they can't collide with the kept shapes the sheet XML refers to.
fn merge_vml(kept: &str, ours: &str) -> String {
    let Some(close) = kept.rfind("</xml>") else { return ours.to_string() };
    let block = shape_numbers(kept).into_iter().max().unwrap_or(0) / 1024 + 1;
    let mut add = String::new();
    if !kept.contains("id=\"_x0000_t202\"")
        && let Some(i) = ours.find("<v:shapetype")
        && let Some(j) = ours.get(i..).and_then(|s| s.find("</v:shapetype>"))
    {
        add.push_str(ours.get(i..i + j + "</v:shapetype>".len()).unwrap_or(""));
    }
    for (k, (a, b)) in shape_spans(ours).into_iter().enumerate() {
        let shape = ours.get(a..b).unwrap_or("");
        let id = block.saturating_mul(1024).saturating_add(1 + k as u64);
        match (shape.find("id=\"_x0000_s"), shape.find("id=\"_x0000_s").and_then(|i| shape.get(i + 4..)?.find('"').map(|j| i + 4 + j))) {
            (Some(i), Some(j)) => {
                let _ = write!(add, "{}id=\"_x0000_s{id}\"{}", shape.get(..i).unwrap_or(""), shape.get(j + 1..).unwrap_or(""));
            }
            _ => add.push_str(shape),
        }
    }
    let mut head = kept.get(..close).unwrap_or("").to_string();
    // Claim the block in the drawing's id map.
    if let Some(i) = head.find("<o:idmap")
        && let Some(d) = head.get(i..).and_then(|s| s.find("data=\"")).map(|d| i + d + "data=\"".len())
        && let Some(e) = head.get(d..).and_then(|s| s.find('"')).map(|e| d + e)
    {
        head.insert_str(e, &format!(",{block}"));
    }
    format!("{head}{add}{}", kept.get(close..).unwrap_or(""))
}

/// Rewrites relationship-id attributes (`r:id`, `r:embed`…: any attribute in the relationships
/// namespace) of an XML fragment through `map`. `None` when the fragment is damaged or `map` has
/// no id for one of them.
pub fn map_rel_ids(x: &str, map: &mut dyn FnMut(&str) -> Option<String>) -> Option<String> {
    let mut reader = Reader::from_str(x);
    let mut writer = Writer::new(Vec::with_capacity(x.len()));
    let mut prefixes: HashSet<Vec<u8>> = HashSet::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event().ok()? {
            Event::Eof => break,
            Event::Start(e) => {
                depth += 1;
                if depth > MAX_FRAGMENT_DEPTH {
                    return None;
                }
                let e = map_start(&e, &mut prefixes, map)?;
                writer.write_event(Event::Start(e)).ok()?;
            }
            Event::Empty(e) => {
                let e = map_start(&e, &mut prefixes, map)?;
                writer.write_event(Event::Empty(e)).ok()?;
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                writer.write_event(Event::End(e)).ok()?;
            }
            ev => writer.write_event(ev).ok()?,
        }
    }
    String::from_utf8(writer.into_inner()).ok()
}

fn map_start(e: &BytesStart<'_>, prefixes: &mut HashSet<Vec<u8>>, map: &mut dyn FnMut(&str) -> Option<String>) -> Option<BytesStart<'static>> {
    for a in e.attributes().with_checks(false) {
        let a = a.ok()?;
        if let Some(p) = a.key.as_ref().strip_prefix(b"xmlns:")
            && (a.value.as_ref() == NS_REL.as_bytes() || a.value.as_ref() == NS_REL_STRICT.as_bytes())
        {
            prefixes.insert(p.to_vec());
        }
    }
    let name = String::from_utf8(e.name().as_ref().to_vec()).ok()?;
    let mut out = BytesStart::new(name);
    for a in e.attributes().with_checks(false) {
        let a = a.ok()?;
        let key = a.key.as_ref();
        let is_rel = !key.starts_with(b"xmlns") && a.key.prefix().is_some_and(|p| prefixes.contains(p.as_ref()));
        if is_rel {
            let old = std::str::from_utf8(&a.value).ok()?;
            let new = map(old)?;
            out.push_attribute(Attribute { key: QName(key), value: Cow::Owned(esc_attr(&new).into_bytes()) });
        } else {
            out.push_attribute(Attribute { key: QName(key), value: a.value.clone() });
        }
    }
    Some(out)
}

// ---------------------------------------------------------------- writing

/// Names given to kept parts while a package is written.
#[derive(Default)]
pub struct KeptOut {
    used: HashSet<String>,
    n: u32,
    /// Package-root relationships (type, target, external), added when `_rels/.rels` is written.
    root: Vec<(String, String, bool)>,
}

impl KeptOut {
    /// A part name for a kept part: its original name when that can't collide with anything
    /// written, otherwise a fresh one in the same folder.
    fn claim(&mut self, original: &str) -> String {
        let lower = original.to_ascii_lowercase();
        let own = OWN_DIRS.iter().any(|d| lower.starts_with(d));
        if own && self.used.insert(lower) {
            return original.to_string();
        }
        let (dir, file) = package::split_dir(original);
        let (stem, ext) = match file.rsplit_once('.') {
            Some((s, e)) => (s, format!(".{e}")),
            None => (file, String::new()),
        };
        let stem = if own { format!("{stem}_") } else { "kept".to_string() };
        loop {
            self.n = self.n.saturating_add(1);
            let file = format!("{stem}{}{ext}", self.n);
            let cand = if dir.is_empty() { file } else { format!("{dir}/{file}") };
            if self.used.insert(cand.to_ascii_lowercase()) || self.n == u32::MAX {
                return cand;
            }
        }
    }
}

/// Original part name (normalized) → written part name.
type NameMap = HashMap<String, String>;

fn rel_target(r: &PreservedRel, names: &NameMap) -> Option<String> {
    if r.external { Some(r.target.clone()) } else { names.get(&normalize(&r.target)).map(|n| format!("/{n}")) }
}

/// A kept part's relationships, ids unchanged, targets renamed.
fn part_rels_xml(rels: &[PreservedRel], names: &NameMap) -> String {
    let mut s = format!("{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for r in rels {
        let Some(target) = rel_target(r, names) else { continue };
        let _ = write!(
            s,
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
            esc_attr(&r.id),
            esc_attr(&r.rel_type),
            esc_attr(&target),
            if r.external { " TargetMode=\"External\"" } else { "" }
        );
    }
    s.push_str("</Relationships>");
    s
}

fn write_parts(out: &mut Out, parts: &[PreservedPart]) -> NameMap {
    let mut names = NameMap::new();
    for p in parts {
        names.entry(normalize(&p.name)).or_insert_with(|| out.kept.claim(&p.name));
    }
    let mut written = HashSet::new();
    for p in parts {
        let Some(new) = names.get(&normalize(&p.name)).cloned() else { continue };
        if !written.insert(new.clone()) {
            continue;
        }
        out.part(&new, Some(&p.content_type), p.data.to_vec());
        if !p.rels.is_empty() {
            out.part(&rels_part_name(&new), None, part_rels_xml(&p.rels, &names).into_bytes());
        }
    }
    names
}

/// Adds the owner's relationships; returns old id → new id.
fn attach(rels: &mut Rels, list: &[PreservedRel], names: &NameMap) -> HashMap<String, String> {
    let mut ids = HashMap::new();
    for r in list {
        if let Some(target) = rel_target(r, names) {
            let id = rels.add_raw(&r.rel_type, &target, r.external);
            ids.insert(r.id.clone(), id);
        }
    }
    ids
}

fn remap(x: &str, ids: &HashMap<String, String>) -> Option<String> {
    map_rel_ids(x, &mut |id| ids.get(id).cloned())
}

/// A kept chart sheet being written: its `<sheet>` entry's fields and its new relationship id.
pub struct KeptSheet {
    before: usize,
    name: String,
    visibility: Visibility,
    rid: String,
}

/// The workbook's kept content as written.
#[derive(Default)]
pub struct WorkbookKept {
    /// Workbook XML elements that go right after `<sheets>` (`<externalReferences>`).
    pub elements: String,
    sheets: Vec<KeptSheet>,
}

impl WorkbookKept {
    /// Where the model's sheet `i` is in the file's sheet list: chart sheets come between worksheets.
    pub fn file_index(&self, i: usize) -> usize {
        i + self.sheets.iter().filter(|s| s.before <= i).count()
    }

    /// The `<sheet>` entries of the chart sheets that go right before the model's sheet `i`
    /// (`i` = the number of sheets for those that come last); `id` numbers them from `first_id`.
    pub fn sheet_entries(&self, i: usize, last: bool, mut id: usize) -> (String, usize) {
        let mut x = String::new();
        for s in self.sheets.iter().filter(|s| if last { s.before >= i } else { s.before == i }) {
            let state = match s.visibility {
                Visibility::Visible => "",
                Visibility::Hidden => " state=\"hidden\"",
                Visibility::VeryHidden => " state=\"veryHidden\"",
            };
            let name: String = s.name.chars().take(31).collect();
            let _ = write!(x, "<sheet name=\"{}\" sheetId=\"{id}\"{state} r:id=\"{}\"/>", esc_attr(&name), esc_attr(&s.rid));
            id += 1;
        }
        (x, id)
    }
}

/// The part names reachable from `root` through kept parts' relationships (normalized).
fn reachable(p: &Preserved, root: &PreservedRel) -> HashSet<String> {
    let by_name: HashMap<String, &PreservedPart> = p.parts.iter().map(|q| (normalize(&q.name), q)).collect();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([normalize(&root.target)]);
    while let Some(n) = queue.pop_front() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(q) = by_name.get(&n) {
            queue.extend(q.rels.iter().filter(|r| !r.external).map(|r| normalize(&r.target)));
        }
    }
    seen
}

/// Every formula (`<c:f>`) of a chart part, depth first.
fn chart_formulas(el: &xml::El, out: &mut Vec<String>) {
    if el.name == "f" && !el.text.trim().is_empty() {
        out.push(el.text.trim().to_string());
    }
    for c in &el.children {
        chart_formulas(c, out);
    }
}

/// The sheet names a formula such as `'My Sheet'!$A$1:$A$9,Data!$B$1` refers to. References to
/// other workbooks (`[1]Sheet!A1`) are not sheets of this one and are left out.
pub(crate) fn sheet_names_in(formula: &str) -> Vec<String> {
    let mut names = vec![];
    let chars: Vec<char> = formula.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut i = 0;
    while let Some(c) = at(i) {
        let mut name = String::new();
        let start = i;
        if c == '\'' {
            i += 1;
            while let Some(c) = at(i) {
                if c == '\'' {
                    if at(i + 1) == Some('\'') {
                        name.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                name.push(c);
                i += 1;
            }
            i += 1;
        } else {
            while let Some(c) = at(i).filter(|c| !matches!(c, '!' | ',' | '(' | ')' | '\'' | ' ')) {
                name.push(c);
                i += 1;
            }
        }
        if at(i) == Some('!') {
            if !name.is_empty() && !name.starts_with('[') {
                names.push(name);
            }
            i += 1;
        } else if i == start {
            i += 1;
        }
    }
    names
}

/// Whether a kept chart sheet can be written back: its chart's series must still refer to sheets
/// that exist under the same names (the chart is kept as it was, so a series on a deleted or
/// renamed sheet would be left pointing at nothing), and no worksheet may have taken its name.
fn chart_sheet_usable(wb: &Workbook, p: &Preserved, s: &PreservedSheet) -> bool {
    let same = |a: &str, b: &str| a.chars().take(31).collect::<String>().eq_ignore_ascii_case(&b.chars().take(31).collect::<String>());
    if wb.sheets.iter().any(|w| same(&w.name, &s.name)) {
        return false;
    }
    let Some(root) = p.rels.iter().find(|r| r.id == s.rel_id) else { return false };
    let parts = reachable(p, root);
    let mut formulas = vec![];
    for q in p.parts.iter().filter(|q| parts.contains(&normalize(&q.name)) && q.content_type.contains("chart")) {
        if let Ok(x) = xml::parse(&q.data) {
            chart_formulas(&x, &mut formulas);
        }
    }
    formulas.iter().flat_map(|f| sheet_names_in(f)).all(|n| wb.sheets.iter().any(|w| same(&w.name, &n)))
}

/// Writes the workbook's kept parts and relationships. Chart sheets that can't be written back
/// (see [`chart_sheet_usable`]) are left out with the parts only they reach.
pub fn write_workbook(wb: &Workbook, wb_rels: &mut Rels, out: &mut Out) -> WorkbookKept {
    let Some(p) = wb.preserved.as_deref() else { return WorkbookKept::default() };
    let (live, dropped): (Vec<&PreservedSheet>, Vec<&PreservedSheet>) = p.sheets.iter().partition(|s| chart_sheet_usable(wb, p, s));
    let root_of = |s: &PreservedSheet| p.rels.iter().find(|r| r.id == s.rel_id);
    let dropped_ids: HashSet<&str> = dropped.iter().map(|s| s.rel_id.as_str()).collect();
    // Parts reached only by dropped chart sheets go with them.
    let mut gone: HashSet<String> = dropped.iter().filter_map(|s| root_of(s)).flat_map(|r| reachable(p, r)).collect();
    if !gone.is_empty() {
        let others = p.rels.iter().filter(|r| !dropped_ids.contains(r.id.as_str())).flat_map(|r| reachable(p, r));
        for n in others {
            gone.remove(&n);
        }
    }
    let parts: Vec<PreservedPart> = p.parts.iter().filter(|q| !gone.contains(&normalize(&q.name))).cloned().collect();
    let rels: Vec<PreservedRel> = p.rels.iter().filter(|r| !dropped_ids.contains(r.id.as_str())).cloned().collect();
    let names = write_parts(out, &parts);
    let ids = attach(wb_rels, &rels, &names);
    for r in &p.package_rels {
        if let Some(target) = rel_target(r, &names) {
            out.kept.root.push((r.rel_type.clone(), target, r.external));
        }
    }
    WorkbookKept {
        elements: p.elements.iter().filter(|e| e.slot == "externalReferences").filter_map(|e| remap(&e.xml, &ids)).collect(),
        sheets: live
            .iter()
            .filter_map(|s| Some(KeptSheet { before: s.before, name: s.name.clone(), visibility: s.visibility, rid: ids.get(&s.rel_id)?.clone() }))
            .collect(),
    }
}

/// The workbook's kept elements of `slot` that refer to no part (`fileVersion`, `ext`).
pub fn workbook_elements(wb: &Workbook, slot: &str) -> String {
    let Some(p) = wb.preserved.as_deref() else { return String::new() };
    p.elements.iter().filter(|e| e.slot == slot).map(|e| e.xml.as_str()).collect()
}

/// Adds the kept package-root relationships (custom properties…) to `_rels/.rels`.
pub fn write_package_rels(root: &mut Rels, out: &mut Out) {
    for (ty, target, external) in std::mem::take(&mut out.kept.root) {
        root.add_raw(&ty, &target, external);
    }
}

/// A sheet's kept content being written.
pub struct SheetKept<'a> {
    p: Option<&'a Preserved>,
    names: NameMap,
    ids: HashMap<String, String>,
}

impl<'a> SheetKept<'a> {
    /// Writes the sheet's kept parts and adds their relationships to the sheet's.
    pub fn write(sheet: &'a Sheet, rels: &mut Rels, out: &mut Out) -> SheetKept<'a> {
        let Some(p) = sheet.preserved.as_deref() else { return SheetKept { p: None, names: NameMap::new(), ids: HashMap::new() } };
        let names = write_parts(out, &p.parts);
        let ids = attach(rels, &p.rels, &names);
        SheetKept { p: Some(p), names, ids }
    }

    fn vml(&self) -> Option<&'a PreservedPart> {
        self.p?.vml.as_ref()
    }

    fn write_vml_rels(&self, vml: &PreservedPart, name: &str, out: &mut Out) {
        if !vml.rels.is_empty() {
            out.part(&rels_part_name(name), None, part_rels_xml(&vml.rels, &self.names).into_bytes());
        }
    }

    /// GridCraft's notes VML for part `name`, merged with the kept control and object shapes.
    pub fn notes_vml(&self, ours: String, name: &str, out: &mut Out) -> String {
        let Some(vml) = self.vml() else { return ours };
        self.write_vml_rels(vml, name, out);
        merge_vml(&String::from_utf8_lossy(&vml.data), &ours)
    }

    /// The kept VML drawing of a sheet without notes; its new relationship id.
    pub fn lone_vml(&self, rels: &mut Rels, out: &mut Out) -> Option<String> {
        let vml = self.vml()?;
        let name = out.kept.claim(&vml.name);
        out.part(&name, Some(CT_VML), vml.data.to_vec());
        self.write_vml_rels(vml, &name, out);
        Some(rels.add("vmlDrawing", &format!("/{name}")))
    }

    /// The new relationship id for `<pageSetup r:id>` (printer settings).
    pub fn page_setup_id(&self) -> Option<&str> {
        let old = &self.p?.elements.iter().find(|e| e.slot == "pageSetup")?.xml;
        self.ids.get(old).map(String::as_str)
    }

    /// The kept sheet elements that go after `<legacyDrawing>`, in schema order.
    pub fn elements(&self) -> String {
        SHEET_SLOTS.iter().map(|slot| self.slot(slot)).collect()
    }

    /// The kept `<customProperties>` (they go after `<colBreaks>`).
    pub fn custom_properties(&self) -> String {
        self.slot("customProperties")
    }

    fn slot(&self, slot: &str) -> String {
        let Some(p) = self.p else { return String::new() };
        p.elements.iter().filter(|e| e.slot == slot).filter_map(|e| remap(&e.xml, &self.ids)).collect()
    }
}

// ---------------------------------------------------------------- linked workbooks

/// `wb` as it is saved into the folder `to` (absolute): the relative paths of its linked
/// workbooks, relative to the folder it was read from, are rewritten relative to `to`, as Excel
/// does on Save As, or made absolute when no relative path exists (another drive). `None` when
/// nothing changes.
pub fn rebase_links(wb: &Workbook, to: &Path) -> Option<Workbook> {
    let p = wb.preserved.as_deref()?;
    let from = Folder::of(wb.source_dir.as_deref()?)?;
    let to = Folder::of(to)?;
    if from == to {
        return None;
    }
    let mut parts = p.parts.clone();
    let mut changed = false;
    for r in parts.iter_mut().flat_map(|part| part.rels.iter_mut()) {
        if r.external
            && is_kind(r, &["externalLinkPath"])
            && let Some(target) = rebase(&r.target, &from, &to)
        {
            r.target = target;
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    let mut out = wb.clone();
    out.preserved = Some(Arc::new(Preserved { parts, ..p.clone() }));
    Some(out)
}

/// An absolute folder, lexically normalized.
#[derive(PartialEq)]
struct Folder {
    /// Drive or share and root separator (`/`, `C:\`, `\\server\share\`).
    root: String,
    names: Vec<String>,
}

impl Folder {
    fn of(dir: &Path) -> Option<Folder> {
        if !dir.is_absolute() {
            return None;
        }
        let mut f = Folder { root: String::new(), names: vec![] };
        for c in dir.components() {
            match c {
                Component::Prefix(_) | Component::RootDir => f.root.push_str(&c.as_os_str().to_string_lossy()),
                Component::CurDir => {}
                Component::ParentDir => {
                    f.names.pop();
                }
                Component::Normal(n) => f.names.push(n.to_string_lossy().into_owned()),
            }
        }
        Some(f)
    }
}

/// A relative link target read from `from`, rewritten for `to`. `None` keeps it as it is: an
/// absolute path or URL, or a path that climbs above the root.
fn rebase(target: &str, from: &Folder, to: &Folder) -> Option<String> {
    if !is_relative_path(target) {
        return None;
    }
    // Each name as the file system knows it, and as it is written in a target. Names taken from
    // the original target keep their spelling.
    let mut path: Vec<(String, String)> = from.names.iter().map(|n| (n.clone(), encode_name(n))).collect();
    for seg in target.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                path.pop()?;
            }
            _ => path.push((decode_name(seg), seg.to_string())),
        }
    }
    let folders = path.len().checked_sub(1)?;
    if !from.root.eq_ignore_ascii_case(&to.root) {
        let sep = if from.root.contains('\\') { "\\" } else { "/" };
        let names: Vec<&str> = path.iter().map(|(name, _)| name.as_str()).collect();
        return Some(format!("file:///{}{}", from.root, names.join(sep)));
    }
    let common = to.names.iter().zip(&path).take(folders).take_while(|(a, (b, _))| *a == b).count();
    let up = "../".repeat(to.names.len().saturating_sub(common));
    let down: Vec<&str> = path.iter().skip(common).map(|(_, written)| written.as_str()).collect();
    Some(format!("{up}{}", down.join("/")))
}

/// Not empty, no root, no scheme or drive (`file:`, `http:`, `C:`).
fn is_relative_path(target: &str) -> bool {
    let scheme = target.split_once(':').is_some_and(|(s, _)| {
        s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    !target.is_empty() && !target.starts_with(['/', '\\']) && !scheme
}

/// A folder name as a segment of a relationship target (a URI): ASCII characters a URI path
/// can't hold are percent-encoded, other characters are written as they are.
fn encode_name(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        if !c.is_ascii() || c.is_ascii_alphanumeric() || "-._~!$&'()+,;=@".contains(c) {
            s.push(c);
        } else {
            let _ = write!(s, "%{:02X}", c as u32);
        }
    }
    s
}

/// A target segment with its percent-escapes decoded; as written when they don't decode to UTF-8.
fn decode_name(seg: &str) -> String {
    let b = seg.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) if c == b'%' => {
                out.push(byte);
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| seg.to_string())
}
