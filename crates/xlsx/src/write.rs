//! Writing a workbook as an XLSX package.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use gridcraft_core::RangeRef;
use gridcraft_formula::quote_sheet;
use gridcraft_model::{CalcMode, Style, Visibility, Workbook};

use crate::IoError;
use crate::package::ZipOut;
use crate::xml::{esc, esc_attr};

pub const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

pub const CT_WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
pub const CT_DRAWING: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";
pub const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
pub const CT_TABLE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";
pub const CT_COMMENTS: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.comments+xml";

/// A part's relationships.
#[derive(Default)]
pub struct Rels {
    items: Vec<(String, String, String, bool)>,
}

impl Rels {
    /// Adds a relationship of type `kind` (last segment, e.g. `worksheet`); returns its id.
    pub fn add(&mut self, kind: &str, target: &str) -> String {
        let id = format!("rId{}", self.items.len() + 1);
        let ty = match kind {
            "core-properties" => "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties".to_string(),
            k => format!("{NS_REL}/{k}"),
        };
        self.items.push((id.clone(), ty, target.to_string(), false));
        id
    }
    pub fn add_external(&mut self, kind: &str, target: &str) -> String {
        let id = format!("rId{}", self.items.len() + 1);
        self.items.push((id.clone(), format!("{NS_REL}/{kind}"), target.to_string(), true));
        id
    }
    /// Adds a Microsoft extension relationship with its complete namespace URI.
    pub fn add_uri(&mut self, ty: &str, target: &str) -> String {
        let id = format!("rId{}", self.items.len() + 1);
        self.items.push((id.clone(), ty.to_string(), target.to_string(), false));
        id
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn xml(&self) -> String {
        let mut s = format!("{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
        for (id, ty, target, ext) in &self.items {
            let _ = write!(
                s,
                "<Relationship Id=\"{id}\" Type=\"{ty}\" Target=\"{}\"{}/>",
                esc_attr(target),
                if *ext { " TargetMode=\"External\"" } else { "" }
            );
        }
        s.push_str("</Relationships>");
        s
    }
}

/// Shared string table being built.
#[derive(Default)]
pub struct Sst {
    list: Vec<String>,
    map: HashMap<String, u32>,
    pub count: u64,
}

impl Sst {
    pub fn index(&mut self, s: &str) -> u32 {
        self.count += 1;
        if let Some(&i) = self.map.get(s) {
            return i;
        }
        let i = self.list.len() as u32;
        self.list.push(s.to_string());
        self.map.insert(s.to_string(), i);
        i
    }
}

/// Package being assembled.
pub struct Out {
    parts: Vec<(String, Vec<u8>)>,
    overrides: Vec<(String, String)>,
    defaults: BTreeMap<String, String>,
    pub dxfs: Vec<Style>,
    pub sst: Sst,
    pub images: u32,
    pub charts: u32,
    /// References of chartex parts, defined as hidden `_xlchart.v1.N` names.
    pub chart_names: Vec<String>,
    pub drawings: u32,
    pub tables: u32,
    pub comments: u32,
    pub table_id: u32,
    pub dynamic_arrays: bool,
    pub needs_calc: bool,
    pub cell_pictures: crate::richdata::PicturesOut,
    /// Shared formulas converted to file form once (see `fmla::to_file_shared`).
    pub file_exprs: crate::fmla::FileExprs,
}

impl Out {
    pub fn part(&mut self, name: &str, content_type: Option<&str>, data: Vec<u8>) {
        if let Some(ct) = content_type {
            self.overrides.push((format!("/{name}"), ct.to_string()));
        }
        self.parts.push((name.to_string(), data));
    }
    pub fn default_type(&mut self, ext: &str, ct: &str) {
        self.defaults.entry(ext.to_string()).or_insert_with(|| ct.to_string());
    }
}

/// `$A$1:$D$20` (or `$1:$3` / `$A:$C`).
pub fn abs_range(r: &RangeRef) -> String {
    let c = |col: u32| gridcraft_core::col_to_letters(col);
    if r.is_full_rows() && !r.is_full_cols() {
        return format!("${}:${}", r.start.row + 1, r.end.row + 1);
    }
    if r.is_full_cols() && !r.is_full_rows() {
        return format!("${}:${}", c(r.start.col), c(r.end.col));
    }
    if r.is_single() {
        return format!("${}${}", c(r.start.col), r.start.row + 1);
    }
    format!("${}${}:${}${}", c(r.start.col), r.start.row + 1, c(r.end.col), r.end.row + 1)
}

pub fn write_xlsx(wb: &Workbook) -> Result<Vec<u8>, IoError> {
    if wb.sheets.is_empty() {
        return Err(IoError::Format("a workbook needs at least one sheet".into()));
    }
    if wb.sheets.iter().any(|sheet| sheet.cell_pictures.keys().any(|c| sheet.cell(*c).is_some_and(|cell| cell.formula.is_some()))) {
        return Err(IoError::Format("a static cell picture cannot also contain a formula".into()));
    }
    let mut out = Out {
        parts: vec![],
        overrides: vec![],
        defaults: BTreeMap::new(),
        dxfs: vec![],
        sst: Sst::default(),
        images: 0,
        charts: 0,
        chart_names: vec![],
        drawings: 0,
        tables: 0,
        comments: 0,
        table_id: 0,
        dynamic_arrays: false,
        needs_calc: false,
        cell_pictures: Default::default(),
        file_exprs: Default::default(),
    };
    out.default_type("rels", "application/vnd.openxmlformats-package.relationships+xml");
    out.default_type("xml", "application/xml");

    // Which sheet is active (must be visible), and at least one visible sheet.
    let any_visible = wb.sheets.iter().any(|s| s.visibility == Visibility::Visible);
    let active = if wb.sheets.get(wb.active_sheet).is_some_and(|s| s.visibility == Visibility::Visible) {
        wb.active_sheet
    } else {
        wb.sheets.iter().position(|s| s.visibility == Visibility::Visible).unwrap_or(0)
    };

    let mut wb_rels = Rels::default();
    let mut pivots = crate::pivot::PivotWriter::default();
    let mut sheets_xml = String::new();
    for (i, sheet) in wb.sheets.iter().enumerate() {
        let (xml, mut rels) = crate::sheet_write::write_sheet(wb, i, i == active, &mut out);
        pivots.add_sheet(wb, i, &mut out, &mut rels);
        let name = format!("xl/worksheets/sheet{}.xml", i + 1);
        out.part(&name, Some(CT_WORKSHEET), xml.into_bytes());
        if !rels.is_empty() {
            out.part(&format!("xl/worksheets/_rels/sheet{}.xml.rels", i + 1), None, rels.xml().into_bytes());
        }
        let rid = wb_rels.add("worksheet", &format!("worksheets/sheet{}.xml", i + 1));
        let state = match sheet.visibility {
            Visibility::Visible => "",
            _ if !any_visible && i == active => "",
            Visibility::Hidden => " state=\"hidden\"",
            Visibility::VeryHidden => " state=\"veryHidden\"",
        };
        let name: String = sheet.name.chars().take(31).collect();
        let _ = write!(sheets_xml, "<sheet name=\"{}\" sheetId=\"{}\"{state} r:id=\"{rid}\"/>", esc_attr(&name), i + 1);
    }

    // Defined names, then print areas and titles.
    let mut names = String::new();
    let mut seen = std::collections::HashSet::new();
    for n in &wb.names {
        if n.name.is_empty() || !seen.insert((n.name.to_ascii_lowercase(), n.scope)) {
            continue;
        }
        if let Some(sc) = n.scope
            && sc >= wb.sheets.len()
        {
            continue;
        }
        let _ = write!(names, "<definedName name=\"{}\"", esc_attr(&n.name));
        if let Some(sc) = n.scope {
            let _ = write!(names, " localSheetId=\"{sc}\"");
        }
        if n.hidden {
            names.push_str(" hidden=\"1\"");
        }
        if !n.comment.is_empty() {
            let _ = write!(names, " comment=\"{}\"", esc_attr(&n.comment));
        }
        let _ = write!(names, ">{}</definedName>", esc(&crate::fmla::text_to_file(&n.formula)));
    }
    for (i, f) in out.chart_names.iter().enumerate() {
        let _ = write!(
            names,
            "<definedName name=\"{}{i}\" hidden=\"1\">{}</definedName>",
            crate::chartex::NAME_PREFIX,
            esc(&crate::fmla::text_to_file(f))
        );
    }
    for (i, sheet) in wb.sheets.iter().enumerate() {
        let q = quote_sheet(&sheet.name);
        if let Some(pa) = &sheet.print.print_area {
            let _ = write!(names, "<definedName name=\"_xlnm.Print_Area\" localSheetId=\"{i}\">{}!{}</definedName>", esc(&q), abs_range(pa));
        }
        let mut titles = vec![];
        if let Some((c0, c1)) = sheet.print.title_cols {
            titles.push(format!("{q}!{}", abs_range(&RangeRef::cols(c0.min(c1), c0.max(c1)))));
        }
        if let Some((r0, r1)) = sheet.print.title_rows {
            titles.push(format!("{q}!{}", abs_range(&RangeRef::rows(r0.min(r1), r0.max(r1)))));
        }
        if !titles.is_empty() {
            let _ = write!(names, "<definedName name=\"_xlnm.Print_Titles\" localSheetId=\"{i}\">{}</definedName>", esc(&titles.join(",")));
        }
    }

    let mut w = format!("{XML_DECL}<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\">");
    w.push_str(if wb.date_system == gridcraft_core::DateSystem::D1904 { "<workbookPr date1904=\"1\"/>" } else { "<workbookPr/>" });
    if wb.protected_structure {
        w.push_str("<workbookProtection lockStructure=\"1\"/>");
    }
    let _ = write!(w, "<bookViews><workbookView activeTab=\"{active}\"/></bookViews><sheets>{sheets_xml}</sheets>");
    if !names.is_empty() {
        let _ = write!(w, "<definedNames>{names}</definedNames>");
    }
    w.push_str("<calcPr calcId=\"191029\"");
    match wb.calc.mode {
        CalcMode::Automatic => {}
        CalcMode::Manual => w.push_str(" calcMode=\"manual\""),
        CalcMode::AutomaticExceptTables => w.push_str(" calcMode=\"autoNoTable\""),
    }
    if wb.calc.iterative {
        let _ = write!(w, " iterate=\"1\" iterateCount=\"{}\" iterateDelta=\"{}\"", wb.calc.max_iterations, crate::xml::num(wb.calc.max_change));
    }
    if wb.calc.precision_as_displayed {
        w.push_str(" fullPrecision=\"0\"");
    }
    if !wb.calc.multi_threaded {
        w.push_str(" concurrentCalc=\"0\"");
    }
    if wb.calc.threads > 0 {
        let _ = write!(w, " concurrentManualCount=\"{}\"", wb.calc.threads);
    }
    if out.needs_calc {
        w.push_str(" fullCalcOnLoad=\"1\"");
    }
    w.push_str("/>");
    w.push_str(&pivots.workbook_xml(&mut wb_rels));
    w.push_str("</workbook>");

    // Workbook-level parts.
    let (styles, fmt_ids) = crate::styles::write_styles(wb, &out.dxfs, &pivots.number_formats(wb));
    pivots.finish(wb, &mut out, &|code: &str| crate::tables::builtin_id(code).or_else(|| fmt_ids.get(code).copied()));
    out.part("xl/styles.xml", Some("application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"), styles.into_bytes());
    wb_rels.add("styles", "styles.xml");
    out.part(
        "xl/theme/theme1.xml",
        Some("application/vnd.openxmlformats-officedocument.theme+xml"),
        crate::theme::write_theme(&wb.theme).into_bytes(),
    );
    wb_rels.add("theme", "theme/theme1.xml");
    let mut sst = format!("{XML_DECL}<sst xmlns=\"{NS_MAIN}\" count=\"{}\" uniqueCount=\"{}\">", out.sst.count, out.sst.list.len());
    for t in &out.sst.list {
        let _ = write!(sst, "<si>{}</si>", crate::xml::t_el(t));
    }
    sst.push_str("</sst>");
    out.part("xl/sharedStrings.xml", Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"), sst.into_bytes());
    wb_rels.add("sharedStrings", "sharedStrings.xml");
    if !out.cell_pictures.is_empty() {
        crate::richdata::write(&mut out, &mut wb_rels)?;
    } else if out.dynamic_arrays {
        out.part(
            "xl/metadata.xml",
            Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml"),
            METADATA.as_bytes().to_vec(),
        );
        wb_rels.add("sheetMetadata", "metadata.xml");
    }
    out.part("xl/workbook.xml", Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"), w.into_bytes());
    out.part("xl/_rels/workbook.xml.rels", None, wb_rels.xml().into_bytes());

    // Document properties.
    let p = &wb.props;
    let mut core = format!(
        "{XML_DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">"
    );
    for (tag, v) in [
        ("dc:title", &p.title),
        ("dc:subject", &p.subject),
        ("dc:creator", &p.author),
        ("cp:keywords", &p.keywords),
        ("dc:description", &p.description),
    ] {
        if !v.is_empty() {
            let _ = write!(core, "<{tag}>{}</{tag}>", esc(v));
        }
    }
    core.push_str("</cp:coreProperties>");
    out.part("docProps/core.xml", Some("application/vnd.openxmlformats-package.core-properties+xml"), core.into_bytes());
    let mut app = String::from(XML_DECL);
    app.push_str("<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>GridCraft</Application>");
    if !p.company.is_empty() {
        let _ = write!(app, "<Company>{}</Company>", esc(&p.company));
    }
    app.push_str("</Properties>");
    out.part("docProps/app.xml", Some("application/vnd.openxmlformats-officedocument.extended-properties+xml"), app.into_bytes());
    let mut root = Rels::default();
    root.add("officeDocument", "xl/workbook.xml");
    root.add("core-properties", "docProps/core.xml");
    root.add("extended-properties", "docProps/app.xml");
    out.part("_rels/.rels", None, root.xml().into_bytes());

    // Content types first, then the parts in creation order.
    let mut ct = format!("{XML_DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    for (ext, t) in &out.defaults {
        let _ = write!(ct, "<Default Extension=\"{ext}\" ContentType=\"{t}\"/>");
    }
    for (part, t) in &out.overrides {
        let _ = write!(ct, "<Override PartName=\"{}\" ContentType=\"{t}\"/>", esc_attr(part));
    }
    ct.push_str("</Types>");
    let mut zip = ZipOut::new();
    zip.add("[Content_Types].xml", ct.as_bytes())?;
    // Conventional order: rels, docProps, workbook, the rest.
    let rank = |n: &str| match n {
        "_rels/.rels" => 0,
        n if n.starts_with("docProps/") => 1,
        "xl/workbook.xml" => 2,
        "xl/_rels/workbook.xml.rels" => 3,
        _ => 4,
    };
    let mut parts = std::mem::take(&mut out.parts);
    parts.sort_by_key(|(n, _)| rank(n));
    for (name, data) in &parts {
        zip.add(name, data)?;
    }
    zip.finish()
}

/// Cell metadata declaring dynamic-array formulas (MS-XLSX `XLDAPR`).
const METADATA: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<metadata xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:xda=\"http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray\"><metadataTypes count=\"1\"><metadataType name=\"XLDAPR\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" coerce=\"1\" cellMeta=\"1\"/></metadataTypes><futureMetadata name=\"XLDAPR\" count=\"1\"><bk><extLst><ext uri=\"{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}\"><xda:dynamicArrayProperties fDynamic=\"1\" fCollapsed=\"0\"/></ext></extLst></bk></futureMetadata><cellMetadata count=\"1\"><bk><rc t=\"1\" v=\"0\"/></bk></cellMetadata></metadata>";
