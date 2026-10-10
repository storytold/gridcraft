//! Reading the workbook part and everything hanging off it.

use std::sync::Arc;

use gridcraft_core::{DateSystem, RangeRef};
use gridcraft_model::{CalcMode, CellPicture, DefinedName, Sheet, Style, StyleId, StyleTable, Visibility, Workbook};

use crate::package::{Package, Rel};
use crate::styles::{StylesIn, read_styles};
use crate::xml::{self, El};
use crate::{IoError, ReadReport};

/// State shared while reading one package.
pub struct Ctx<'a> {
    pub pkg: Package<'a>,
    pub report: ReadReport,
    pub sst: Vec<Arc<str>>,
    /// Cell format index → model style.
    pub xf_map: Vec<StyleId>,
    pub dxfs: Vec<Style>,
    pub palette: Vec<u32>,
    pub date_system: DateSystem,
    pub styles: StyleTable,
    /// Threaded-comment person id → display name.
    pub persons: std::collections::HashMap<String, String>,
    pub next_id: u32,
    /// Custom number formats from the styles part, by id.
    pub num_fmts: std::collections::HashMap<u32, String>,
    /// Value metadata blocks, indexed by the worksheet's one-based `vm` attribute.
    pub cell_pictures: Vec<Option<Arc<CellPicture>>>,
}

impl Ctx<'_> {
    pub fn warn(&mut self, m: impl Into<String>) {
        self.report.warn(m);
    }
    /// A fresh id for a chart, picture, shape or table.
    pub fn next_object_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }
    pub fn style(&self, xf: Option<u32>) -> StyleId {
        xf.and_then(|i| self.xf_map.get(i as usize).copied()).unwrap_or_default()
    }
    /// Reads and parses an optional part: problems other than size limits become warnings.
    pub fn optional_xml(&mut self, part: &str) -> Result<Option<El>, IoError> {
        match self.pkg.read_xml(part) {
            Ok(Some(e)) => Ok(Some(e)),
            Ok(None) => {
                self.warn(format!("missing part {part}"));
                Ok(None)
            }
            Err(IoError::TooLarge(m)) => Err(IoError::TooLarge(m)),
            Err(e) => {
                self.warn(format!("skipped {part}: {e}"));
                Ok(None)
            }
        }
    }
}

pub fn find_rel<'r>(rels: &'r [Rel], kind: &str) -> Option<&'r Rel> {
    rels.iter().find(|r| r.kind == kind)
}

pub fn read_xlsx(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let pkg = Package::open(bytes)?;
    let mut cx = Ctx {
        pkg,
        report: ReadReport::default(),
        sst: vec![],
        xf_map: vec![],
        dxfs: vec![],
        palette: crate::tables::INDEXED.to_vec(),
        date_system: DateSystem::D1900,
        styles: StyleTable::default(),
        persons: Default::default(),
        next_id: 1,
        num_fmts: Default::default(),
        cell_pictures: vec![],
    };
    let root_rels = cx.pkg.rels("")?;
    let wb_part = find_rel(&root_rels, "officeDocument").map(|r| r.target.clone()).unwrap_or_else(|| "xl/workbook.xml".into());
    let wb_xml = match cx.pkg.read_xml(&wb_part)? {
        Some(x) => x,
        None => {
            if cx.pkg.has("xl/workbook.xml") && wb_part != "xl/workbook.xml" {
                cx.pkg.read_xml("xl/workbook.xml")?.ok_or_else(|| IoError::Format("no workbook part".into()))?
            } else if cx.pkg.has("content.xml") {
                return Err(IoError::Format("this is an OpenDocument file, not XLSX".into()));
            } else {
                return Err(IoError::Format("no workbook part".into()));
            }
        }
    };
    if wb_xml.name != "workbook" {
        return Err(IoError::Format(format!("unexpected root element <{}> in workbook part", wb_xml.name)));
    }
    let wb_rels = cx.pkg.rels(&wb_part)?;
    cx.cell_pictures = match crate::richdata::read(&mut cx, &wb_rels) {
        Ok(pictures) => pictures,
        Err(error) => {
            cx.warn(format!("cell-picture data could not be read ({error}); scalar fallbacks were retained"));
            vec![]
        }
    };
    let mut wb = Workbook::new();
    wb.sheets.clear();

    // Styles and theme.
    if let Some(r) = find_rel(&wb_rels, "styles").cloned() {
        if let Some(x) = cx.optional_xml(&r.target)? {
            let StylesIn { xfs, dxfs, palette, named, num_fmts } = read_styles(&x);
            cx.num_fmts = num_fmts;
            cx.xf_map = xfs.into_iter().enumerate().map(|(i, s)| if i == 0 { StyleId::DEFAULT } else { cx.styles.intern(s) }).collect();
            cx.dxfs = dxfs;
            cx.palette = palette;
            wb.cell_styles = named;
        }
    } else {
        cx.warn("no styles part");
    }
    if let Some(r) = find_rel(&wb_rels, "theme").cloned()
        && let Some(x) = cx.optional_xml(&r.target)?
    {
        wb.theme = crate::theme::read_theme(&x);
    }
    // Shared strings, streamed item by item.
    if let Some(r) = find_rel(&wb_rels, "sharedStrings").cloned() {
        match cx.pkg.read(&r.target)? {
            Some(b) => {
                let mut sst = Vec::new();
                let res = xml::parse_streaming(&b, &["si"], &mut |si| {
                    sst.push(Arc::from(xml::rich_text(&si)));
                    Ok(())
                });
                if let Err(e) = res {
                    cx.warn(format!("shared strings are damaged ({e}); kept {} strings", sst.len()));
                }
                cx.sst = sst;
            }
            None => cx.warn(format!("missing part {}", r.target)),
        }
    }

    if let Some(r) = find_rel(&wb_rels, "person").cloned()
        && let Ok(Some(x)) = cx.pkg.read_xml(&r.target)
    {
        for p in x.kids("person") {
            if let (Some(id), Some(n)) = (p.attr("id"), p.attr("displayName")) {
                cx.persons.insert(id.to_string(), n.to_string());
            }
        }
    }

    // Workbook properties.
    if let Some(p) = wb_xml.child("workbookPr")
        && p.flag("date1904", false)
    {
        cx.date_system = DateSystem::D1904;
    }
    wb.date_system = cx.date_system;
    if let Some(c) = wb_xml.child("calcPr") {
        wb.calc.mode = match c.attr("calcMode") {
            Some("manual") => CalcMode::Manual,
            Some("autoNoTable") => CalcMode::AutomaticExceptTables,
            _ => CalcMode::Automatic,
        };
        wb.calc.iterative = c.flag("iterate", false);
        if let Some(n) = c.attr_u32("iterateCount") {
            wb.calc.max_iterations = n.clamp(1, 32767);
        }
        if let Some(d) = c.attr_f64("iterateDelta") {
            wb.calc.max_change = d.abs();
        }
        wb.calc.precision_as_displayed = !c.flag("fullPrecision", true);
    }
    if let Some(p) = wb_xml.child("workbookProtection") {
        wb.protected_structure = p.flag("lockStructure", false);
    }
    let active_tab = wb_xml.path(&["bookViews", "workbookView"]).and_then(|v| v.attr_u32("activeTab")).unwrap_or(0) as usize;

    // Sheets. `file_to_model[i]` maps the i-th <sheet> to a model sheet index.
    let mut file_to_model: Vec<Option<usize>> = vec![];
    let mut pivot_caches = crate::pivot::CachesIn::new(&wb_xml, &wb_rels);
    let sheet_els: Vec<El> = wb_xml.child("sheets").map(|s| s.kids("sheet").cloned().collect()).unwrap_or_default();
    for (i, se) in sheet_els.iter().enumerate() {
        let name = se.attr("name").unwrap_or("").to_string();
        let rel = se.attr("id").and_then(|id| wb_rels.iter().find(|r| r.id == id)).cloned();
        let Some(rel) = rel else {
            cx.warn(format!("sheet \"{name}\" has no part; skipped"));
            file_to_model.push(None);
            continue;
        };
        if rel.kind != "worksheet" {
            cx.warn(format!("sheet \"{name}\" is a {} and was skipped", rel.kind));
            file_to_model.push(None);
            continue;
        }
        let name = unique_name(&wb, &name, i);
        let mut sheet = match crate::sheet_read::read_sheet(&mut cx, &rel.target, &name) {
            Ok(s) => s,
            Err(IoError::TooLarge(m)) => return Err(IoError::TooLarge(m)),
            Err(e) => {
                cx.warn(format!("sheet \"{name}\" could not be read: {e}"));
                Sheet::new(name.clone())
            }
        };
        match crate::pivot::read_pivots(&mut cx, &mut pivot_caches, &rel.target, &name) {
            Ok(p) => sheet.pivots = p,
            Err(IoError::TooLarge(m)) => return Err(IoError::TooLarge(m)),
            Err(e) => cx.warn(format!("PivotTables on sheet \"{name}\" could not be read: {e}")),
        }
        sheet.visibility = match se.attr("state") {
            Some("hidden") => Visibility::Hidden,
            Some("veryHidden") => Visibility::VeryHidden,
            _ => Visibility::Visible,
        };
        file_to_model.push(Some(wb.sheets.len()));
        wb.sheets.push(Arc::new(sheet));
    }
    if wb.sheets.is_empty() {
        cx.warn("the workbook has no worksheets; added an empty sheet");
        wb.sheets.push(Arc::new(Sheet::new("Sheet1")));
    }
    wb.active_sheet = file_to_model.get(active_tab).copied().flatten().unwrap_or(0);

    // Defined names.
    if let Some(dn) = wb_xml.child("definedNames") {
        for d in dn.kids("definedName") {
            let Some(name) = d.attr("name") else { continue };
            let scope = match d.attr_u32("localSheetId") {
                Some(i) => match file_to_model.get(i as usize).copied().flatten() {
                    Some(m) => Some(m),
                    None => continue,
                },
                None => None,
            };
            let text = d.text.trim();
            let lname = name.to_ascii_lowercase();
            if lname == "_xlnm._filterdatabase" {
                continue;
            }
            if lname == "_xlnm.print_area" || lname == "_xlnm.print_titles" {
                if let Some(si) = scope
                    && let Some(sheet) = wb.sheet_mut(si)
                {
                    if lname == "_xlnm.print_area" {
                        sheet.print.print_area = parse_print_area(text);
                    } else {
                        let (rows, cols) = parse_print_titles(text);
                        sheet.print.title_rows = rows;
                        sheet.print.title_cols = cols;
                    }
                }
                continue;
            }
            wb.names.push(DefinedName {
                name: name.to_string(),
                scope,
                formula: crate::fmla::from_file(text),
                comment: d.attr("comment").unwrap_or("").to_string(),
                hidden: d.flag("hidden", false),
            });
        }
    }

    // Document properties.
    if let Some(r) = find_rel(&root_rels, "core-properties").cloned()
        && let Ok(Some(x)) = cx.pkg.read_xml(&r.target)
    {
        let get = |n: &str| x.child(n).map(|e| e.text.trim().to_string()).unwrap_or_default();
        wb.props.title = get("title");
        wb.props.subject = get("subject");
        wb.props.author = get("creator");
        wb.props.keywords = get("keywords");
        wb.props.description = get("description");
    }
    if let Some(r) = find_rel(&root_rels, "extended-properties").cloned()
        && let Ok(Some(x)) = cx.pkg.read_xml(&r.target)
    {
        wb.props.company = x.child("Company").map(|e| e.text.trim().to_string()).unwrap_or_default();
    }

    if wb_rels.iter().any(|r| r.kind == "vbaProject") {
        cx.warn("macros (VBA project) are not supported and were dropped");
    }
    if wb_rels.iter().any(|r| r.kind == "externalLink") {
        cx.warn("links to external workbooks are not supported");
    }
    // Calls of LAMBDA names are spelled like the names (the parser upper-cases them).
    wb.apply_name_call_case();
    wb.styles = cx.styles;
    Ok((wb, cx.report))
}

fn unique_name(wb: &Workbook, name: &str, i: usize) -> String {
    let mut base: String = name.chars().filter(|c| !matches!(c, ':' | '\\' | '/' | '?' | '*' | '[' | ']')).take(31).collect();
    if base.trim().is_empty() {
        base = format!("Sheet{}", i + 1);
    }
    if wb.sheet_index(&base).is_none() {
        return base;
    }
    (2..).map(|n| format!("{} ({n})", base.chars().take(25).collect::<String>())).find(|n| wb.sheet_index(n).is_none()).unwrap_or(base)
}

/// Strips a sheet qualifier (`'My Sheet'!$A$1` → `$A$1`).
fn strip_sheet(s: &str) -> &str {
    match s.rfind('!') {
        Some(i) => s.get(i + 1..).unwrap_or(""),
        None => s,
    }
}

pub fn parse_print_area(text: &str) -> Option<RangeRef> {
    let first = text.split(',').next()?;
    let r = strip_sheet(first.trim()).replace('$', "");
    RangeRef::parse(&r)
}

pub fn parse_print_titles(text: &str) -> (Option<(u32, u32)>, Option<(u32, u32)>) {
    let mut rows = None;
    let mut cols = None;
    for part in text.split(',') {
        let p = strip_sheet(part.trim()).replace('$', "");
        let Some((a, b)) = p.split_once(':') else { continue };
        if let (Ok(r0), Ok(r1)) = (a.parse::<u32>(), b.parse::<u32>()) {
            if r0 >= 1 && r1 >= r0 && r1 <= gridcraft_core::MAX_ROWS {
                rows = Some((r0 - 1, r1 - 1));
            }
        } else if let (Some(c0), Some(c1)) = (gridcraft_core::letters_to_col(a), gridcraft_core::letters_to_col(b))
            && c1 >= c0
        {
            cols = Some((c0, c1));
        }
    }
    (rows, cols)
}
