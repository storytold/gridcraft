//! Reading worksheet parts.

use std::collections::HashMap;
use std::sync::Arc;

use craft_xlsx::SheetPart as CraftSheetPart;
use gridcraft_core::date::{serial_from_ymd, time_fraction};
use gridcraft_core::{CellError, CellRef, DateSystem, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_model::{
    AutoFilter, Cell, CfOperator, CfRule, CfValueKind, Comment, CondFormat, EditRange, ErrorStyle, FilterCriterion, Formula, Hyperlink, LineInfo,
    NoteBox, NoteCorner, Orientation, PageOrder, PasswordHash, Sheet, SheetProtection, Sparkline, SparklineKind, Style, Table, TableColumn, TotalsFn,
    Validation, ValidationKind, XmlColumnPr,
};

use crate::IoError;
use crate::fmla::{from_file, unwrap_google_sheets};
use crate::package::{Rel, craft_error};
use crate::read::Ctx;
use crate::styles::read_color;
use crate::tables::{col_width_to_px, paper_name, pt_to_px};
use crate::xml::{self, El};

mod parts;
mod rules;
mod stream;

pub use parts::read_autofilter;
use parts::read_threaded;
use rules::{read_cf, read_validation, read_x14_cf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamedFormulaKind {
    Normal,
    Shared,
    Array,
    DataTable,
}

pub(super) struct StreamedFormula<'a> {
    pub kind: StreamedFormulaKind,
    pub text: &'a str,
    pub shared_index: Option<u32>,
    pub range: Option<RangeRef>,
}

struct Shared {
    anchor: CellRef,
    expr: Option<Arc<Expr>>,
    text: String,
    unwrapped: bool,
    /// Approximate bytes one dependent cell costs (its printed text plus its expression tree);
    /// what exceeds [`SHARED_FORMULA_FREE_COST`] is charged against [`MAX_SHARED_FORMULA_BYTES`].
    cost: u64,
}

struct ColTemplate {
    base_row: u32,
    base: Arc<Expr>,
    /// True when `base` has no `Name`, `Struct`, or `SheetSel::Named`/`Span`, so ASCII case
    /// differences outside quotes are guaranteed to parse to the exact same AST.
    case_safe: bool,
}

/// What one sheet's hyperlinks have copied so far, across all its `<hyperlinks>` blocks.
#[derive(Default)]
struct LinkBudget {
    cells: u64,
    bytes: u64,
}

#[derive(Default)]
pub(super) struct RowState {
    shared: HashMap<u32, Shared>,
    /// Per-column last parsed formula template, used to deduplicate non-shared repeated column
    /// formulas (common in Google Sheets exports and Excel files) into shared `Arc<Expr>`s.
    col_templates: HashMap<u32, ColTemplate>,
    /// Reusable text buffer for shifted-formula fast-match comparisons.
    scratch: String,
    /// Dynamic-array spill ranges and legacy array ranges: cached values inside them (other than
    /// the anchor) are not stored, so the formula can fill them again.
    dynamic: Vec<RangeRef>,
    /// Cells inside the ranges above, for a cap before a pathological sheet proves it itself.
    dynamic_cells: u64,
    /// Row-membership checks served so far, for the work cap above.
    dynamic_checks: u64,
    /// The cell being read lost its shared formula to a copy limit: its cached value is kept, even
    /// inside an array range (where a formula's cached values otherwise give way to the spill).
    shared_copy_refused: bool,
}
/// The known dynamic range `pos` lies in, when it isn't the range's anchor: the cell's cached
/// value gives way to the formula that will fill it (or, for a Python cell, becomes its
/// result). Charges every scanned range against one work budget, then gives up consulting the
/// ranges at all: the worst kind of file may then import a few cached spill values, but it
/// never stalls the whole import.
fn dynamic_range(st: &mut RowState, pos: CellRef) -> Option<RangeRef> {
    if st.dynamic_checks > MAX_DYNAMIC_CHECKS || st.dynamic.is_empty() {
        return None;
    }
    let ranges = st.dynamic.len();
    let hit = st.dynamic.iter().find(|r| r.contains(pos) && r.start != pos).copied();
    st.dynamic_checks = st.dynamic_checks.saturating_add(ranges as u64);
    hit
}

/// Records a dynamic-array range, so its cells outside the anchor import as empty and the
/// formula fills them again. Repeated identical anchors (a damaged or hostile sheet may repeat
/// the same record) are recorded once, and count and area are capped: every later cell inside
/// known range territory pays a scan proportional to what is recorded here.
fn push_dynamic(st: &mut RowState, range: RangeRef) {
    if st.dynamic.contains(&range) {
        return;
    }
    let cells = range.count();
    if st.dynamic.len() >= MAX_DYNAMIC_RANGES || st.dynamic_cells.saturating_add(cells) > MAX_DYNAMIC_CELLS {
        return;
    }
    st.dynamic_cells = st.dynamic_cells.saturating_add(cells);
    st.dynamic.push(range);
}

/// Forgets the range anchored at `pos` (a Python cell whose code the file doesn't hold, which
/// keeps only its cached value): the cached values over the rest of its spill are kept as plain
/// values too, as the file shows them, rather than left for a result that won't come. Charged
/// like [`dynamic_range`]'s scans.
fn forget_dynamic(st: &mut RowState, pos: CellRef) {
    if st.dynamic_checks > MAX_DYNAMIC_CHECKS {
        return;
    }
    st.dynamic_checks = st.dynamic_checks.saturating_add(st.dynamic.len() as u64);
    if let Some(i) = st.dynamic.iter().rposition(|r| r.start == pos) {
        st.dynamic.remove(i);
    }
}

/// Records the spill range of the Python cell read at `pos`, its anchor, when the file declares
/// one that a result can fill: the spill then exists even when the file holds no records for its
/// blank cells. Charged like [`dynamic_range`]'s scans.
fn record_python_spill(st: &mut RowState, sheet: &mut Sheet, pos: CellRef) {
    if st.dynamic_checks > MAX_DYNAMIC_CHECKS {
        return;
    }
    st.dynamic_checks = st.dynamic_checks.saturating_add(st.dynamic.len() as u64);
    let Some(r) = st.dynamic.iter().rev().find(|r| r.start == pos).copied() else { return };
    if r.count() > 1 && r.count() <= gridcraft_model::python::MAX_RESULT_CELLS as u64 {
        sheet.spill_ranges.insert(pos, r);
    }
}

/// Past the scan budget, the cells of a Python spill read afterwards were kept as plain cells, not
/// as its spilled values, and a result rebuilt over them would block itself. Every Python spill of
/// such a sheet keeps its values as plain values instead, with no live spill.
fn settle_python_spills_past_budget(st: &RowState, sheet: &mut Sheet) {
    if st.dynamic_checks <= MAX_DYNAMIC_CHECKS {
        return;
    }
    let anchors: Vec<CellRef> = sheet.spill_ranges.keys().copied().filter(|a| sheet.cells.get(*a).is_some_and(|c| c.py.is_some())).collect();
    for anchor in anchors {
        sheet.settle_spill(anchor);
    }
}

/// The most cell elements read from one workbook, across its sheets: the reader's own cap.
/// (`calc::eval::MAX_CELLS` has the same value but limits one array result, not a workbook.)
pub(crate) const MAX_CELLS: u64 = 16_000_000;
/// Target and tooltip bytes kept for hyperlinks of one sheet, over all cells: the same URL is
/// copied for every cell of its range, so length × cells must stay bounded whatever the file.
const MAX_HYPERLINK_BYTES: u64 = 8 * 1024 * 1024;
/// Most cells one sheet's hyperlinks may cover (each cell gets its own copy of the link).
const MAX_HYPERLINK_CELLS: u64 = 10_000;
/// Longest hyperlink target kept, in bytes (Excel's own limit is about 2,079 characters).
const MAX_HYPERLINK_TARGET: usize = 8_192;
/// Longest hyperlink ScreenTip kept, in characters (Excel's limit).
const MAX_TOOLTIP_CHARS: usize = 255;
/// What a dependent of a shared formula may cost for free: an ordinary formula cell's worth (by
/// [`shared_cost`], a formula of about 40 operators and operands), so a sheet of many ordinary
/// shared formulas is never cut short however many cells share them.
const SHARED_FORMULA_FREE_COST: u64 = 4096;
/// Budget, per workbook, for what shared formulas cost beyond [`SHARED_FORMULA_FREE_COST`] per
/// dependent cell: a long master copied into many cells is where the expansion multiplies the
/// file's bytes, and this bounds it to well within a web build's memory.
pub const MAX_SHARED_FORMULA_BYTES: u64 = 256 << 20;
const MAX_COLUMN_ASSIGNMENTS: u64 = 1_000_000;
/// Most dynamic-array and legacy-array ranges of one sheet.
const MAX_DYNAMIC_RANGES: usize = 65_536;
/// Worst-case cells inside the dynamic ranges the sheet declares. Beyond it further ranges are
/// dropped: they would cost their storage in every membership lookup that follows.
const MAX_DYNAMIC_CELLS: u64 = 16_000_000;
/// Most row-membership checks the sheet's ranges may serve. Beyond it the remaining cells are
/// imported with their cached values instead of being blanked for a spill that may still come,
/// so adversarial range counts can't make each cell scan a long list (a real workbook has a
/// handful of ranges).
const MAX_DYNAMIC_CHECKS: u64 = 16_000_000;
/// Most conditional-format range cells one sheet keeps, over all rules of an element: every
/// rule copies its element's whole range list, so rules × ranges must stay bounded.
const MAX_COND_CELLS: u64 = 4_000_000;

fn from_craft_el(e: &craft_xml::El) -> El {
    let name = e.name.split_once(':').map_or(e.name.as_str(), |(_, local)| local).to_string();
    let attrs = e.attrs.iter().map(|(k, v)| (k.split_once(':').map_or(k.as_str(), |(_, local)| local).to_string(), v.clone())).collect();
    let mut children = Vec::new();
    let mut text = String::new();
    for node in &e.children {
        match node {
            craft_xml::Node::El(c) => children.push(from_craft_el(c)),
            craft_xml::Node::Text(t) | craft_xml::Node::CData(t) => text.push_str(t),
            craft_xml::Node::Pi { .. } | craft_xml::Node::Comment(_) => {}
        }
    }
    El { name, attrs, children, text }
}

fn to_range_ref(r: craft_xlsx::Range) -> RangeRef {
    RangeRef::new(CellRef::new(r.first.row, r.first.col), CellRef::new(r.last.row, r.last.col))
}

/// Reads the sheet part `part`, with the VML shape numbers of the form controls read into the
/// model (pass-through keeps the other controls).
pub fn read_sheet(cx: &mut Ctx<'_>, part: &str, name: &str) -> Result<(Sheet, crate::controls::ReadControls), IoError> {
    let Some(bytes) = cx.pkg.read(part)? else {
        return Err(IoError::Format(format!("missing part {part}")));
    };
    let rels = cx.pkg.rels(part)?;
    let mut sheet = Sheet::new(name);
    let mut st = RowState::default();
    let streamed = stream::stream_worksheet(&bytes, cx, &mut sheet, &mut st)?;
    drop(bytes);
    if let Some(msg) = streamed.damaged {
        cx.warn(format!("sheet \"{name}\" is damaged ({msg}); kept {} rows", streamed.rows_read));
    }
    if streamed.dropped_cells > 0 && streamed.dropped_cells.saturating_add(streamed.total_cells) > crate::xml::MAX_XML_NODES as u64 {
        cx.warn(format!("sheet \"{name}\" is damaged: more than {} cells", crate::xml::MAX_XML_NODES));
        return Ok((Sheet::new(name), Default::default()));
    }
    charge_cells(cx, streamed.dropped_cells)?;
    if streamed.dropped > 0 {
        cx.warn(format!("sheet \"{name}\": {} cells or rows outside the 1,048,576 × 16,384 grid were dropped", streamed.dropped));
    }
    settle_python_spills_past_budget(&st, &mut sheet);
    sheet.cells.shrink_to_fit();

    let root = streamed.root;
    apply_sheet_root_xml(cx, &root, &rels, name, &mut sheet)?;
    let read_controls = read_form_controls(cx, &root, &rels, &mut sheet);

    // Build a minimal CraftSheetPart shell (table_parts + legacy_drawing + rels) for tables/comments/note shapes.
    let mut sp = CraftSheetPart { part: part.to_string(), ..Default::default() };
    sp.rels = cx.pkg.craft_mut().rels(part).map_err(craft_error)?;
    if let Some(tp) = root.child("tableParts") {
        for t in tp.kids("tablePart") {
            if let Some(id) = t.attr("id") {
                sp.features.table_parts.push(id.to_string());
            }
        }
    }
    sp.features.legacy_drawing = root.child("legacyDrawing").and_then(|e| e.attr("id")).map(str::to_string);
    read_sheet_notes_and_tables(cx, &sp, &rels, &mut sheet)?;
    Ok((sheet, read_controls))
}

fn apply_sheet_root_xml(cx: &mut Ctx<'_>, root: &El, rels: &[Rel], name: &str, sheet: &mut Sheet) -> Result<(), IoError> {
    let mut rel_by_id: HashMap<&str, &Rel> = HashMap::with_capacity(rels.len());
    for r in rels {
        rel_by_id.entry(r.id.as_str()).or_insert(r);
    }
    if let Some(fmt) = root.child("sheetFormatPr") {
        if let Some(h) = fmt.attr_f64("defaultRowHeight") {
            let px = pt_to_px(h);
            if px > 0.0 {
                sheet.default_row_height = px;
            }
        }
        if let Some(w) = fmt.attr_f64("defaultColWidth") {
            let px = col_width_to_px(w);
            if px > 0.0 {
                sheet.default_col_width = px;
            }
        } else if let Some(b) = fmt.attr_u32("baseColWidth") {
            let px = ((f64::from(b.min(255)) * 7.0 + 5.0) / 8.0).ceil() * 8.0;
            sheet.default_col_width = px as f32;
        }
    }
    let mut remaining_cols = MAX_COLUMN_ASSIGNMENTS;
    for cols in root.kids("cols") {
        for c in cols.kids("col") {
            let raw = RawCol {
                min: c.attr_u32("min").unwrap_or(0),
                max: c.attr_u32("max").unwrap_or(0),
                width: c.attr_f64("width"),
                custom_width: c.flag("customWidth", false),
                best_fit: c.flag("bestFit", false),
                hidden: c.flag("hidden", false),
                outline_level: c.attr_u32("outlineLevel").unwrap_or(0).min(7) as u8,
                collapsed: c.flag("collapsed", false),
                style: c.attr_u32("style"),
            };
            apply_col_entry(cx, raw, &mut remaining_cols, sheet)?;
        }
    }
    if let Some(mc) = root.child("mergeCells") {
        let count = mc.kids("mergeCell").count();
        if count > crate::xml::MAX_XML_NODES {
            cx.warn(format!("sheet \"{name}\" is damaged: more than {} merge cells", crate::xml::MAX_XML_NODES));
        } else {
            for m in mc.kids("mergeCell") {
                if let Some(r) = m.attr("ref").and_then(RangeRef::parse)
                    && !r.is_single()
                {
                    sheet.merges.push(r);
                }
            }
        }
    }
    if let Some(sv) = root.child("sheetViews").and_then(|v| v.child("sheetView")) {
        apply_xml_view(sv, sheet);
    }
    let mut fit_to_page = false;
    let mut cf_kept_cells = 0u64;
    cx.cf_ext_ids.clear();
    for cf in root.kids("conditionalFormatting") {
        read_cf(cx, cf, sheet, &mut cf_kept_cells);
    }
    if let Some(vals) = root.child("dataValidations") {
        for v in vals.kids("dataValidation") {
            if let Some(val) = read_validation(v) {
                sheet.validations.push(val);
            }
        }
    }
    for child in &root.children {
        match child.name.as_str() {
            "sheetPr" => {
                if let Some(t) = child.child("tabColor") {
                    sheet.tab_color = Some(read_color(t, &cx.palette));
                }
                if let Some(o) = child.child("outlinePr") {
                    sheet.summary_below = o.flag("summaryBelow", true);
                    sheet.summary_right = o.flag("summaryRight", true);
                }
                fit_to_page = child.child("pageSetUpPr").is_some_and(|p| p.flag("fitToPage", false));
            }
            "sheetProtection" => {
                if child.flag("sheet", false) {
                    sheet.protection = Some(SheetProtection {
                        password: read_password(child, ""),
                        legacy_password_hash: None,
                        select_locked: !child.flag("selectLockedCells", false),
                        select_unlocked: !child.flag("selectUnlockedCells", false),
                        format_cells: !child.flag("formatCells", true),
                        format_columns: !child.flag("formatColumns", true),
                        format_rows: !child.flag("formatRows", true),
                        insert_columns: !child.flag("insertColumns", true),
                        insert_rows: !child.flag("insertRows", true),
                        delete_columns: !child.flag("deleteColumns", true),
                        delete_rows: !child.flag("deleteRows", true),
                        sort: !child.flag("sort", true),
                        autofilter: !child.flag("autoFilter", true),
                    });
                }
            }
            "protectedRanges" => read_protected_ranges(child, sheet),
            "autoFilter" => sheet.autofilter = read_autofilter(cx, child),
            "extLst" => read_ext(cx, child, sheet, &mut cf_kept_cells),
            _ => {}
        }
    }
    let mut link_budget = LinkBudget::default();
    'hl: for hl in root.kids("hyperlinks") {
        for h in hl.kids("hyperlink") {
            let Some(range) = h.attr("ref").and_then(RangeRef::parse) else { continue };
            let raw = RawHyperlink { range, rel_id: h.attr("id"), location: h.attr("location"), tooltip: h.attr("tooltip") };
            if !push_hyperlink(cx, name, raw, &rel_by_id, sheet, &mut link_budget) {
                break 'hl;
            }
        }
    }
    apply_xml_print(root, fit_to_page, sheet);
    if let Some(id) = root.child("drawing").and_then(|d| d.attr("id"))
        && let Some(rel) = rel_by_id.get(id).copied()
    {
        let target = rel.target.clone();
        crate::drawing::read_drawing(cx, &target, sheet)?;
    }
    Ok(())
}

fn apply_xml_view(v: &El, sheet: &mut Sheet) {
    sheet.show_gridlines = v.flag("showGridLines", true);
    sheet.show_headings = v.flag("showRowColHeaders", true);
    sheet.show_formulas = v.flag("showFormulas", false);
    sheet.show_zeros = v.flag("showZeros", true);
    sheet.show_outline_symbols = v.flag("showOutlineSymbols", true);
    sheet.right_to_left = v.flag("rightToLeft", false);
    sheet.view = v.attr("view").and_then(gridcraft_model::SheetViewMode::parse).unwrap_or_default();
    if let Some(z) = v.attr_u32("zoomScale") {
        sheet.zoom = z.clamp(10, 400) as u16;
    }
    if let Some(c) = v.attr("topLeftCell").and_then(CellRef::parse) {
        sheet.view_top_left = c;
    }
    let mut active_pane = None;
    if let Some(p) = v.child("pane") {
        if matches!(p.attr("state"), Some("frozen" | "frozenSplit")) {
            let cols = p.attr_f64("xSplit").unwrap_or(0.0).clamp(0.0, (MAX_COLS - 1) as f64) as u32;
            let rows = p.attr_f64("ySplit").unwrap_or(0.0).clamp(0.0, (MAX_ROWS - 1) as f64) as u32;
            if rows > 0 || cols > 0 {
                sheet.freeze = Some((rows, cols));
                if let Some(c) = p.attr("topLeftCell").and_then(CellRef::parse) {
                    sheet.view_top_left = c;
                }
            }
        }
        active_pane = p.attr("activePane");
    }
    let sel = v.kids("selection").find(|s| s.attr("pane") == active_pane).or_else(|| v.kids("selection").last());
    if let Some(c) = sel.and_then(|s| s.attr("activeCell")).and_then(CellRef::parse) {
        sheet.view_active = c;
    }
}

fn apply_xml_print(root: &El, fit_to_page: bool, sheet: &mut Sheet) {
    if let Some(o) = root.child("printOptions") {
        sheet.print.gridlines = o.flag("gridLines", false);
        sheet.print.headings = o.flag("headings", false);
        sheet.print.center_h = o.flag("horizontalCentered", false);
        sheet.print.center_v = o.flag("verticalCentered", false);
    }
    if let Some(pm) = root.child("pageMargins") {
        for (i, attr) in ["left", "right", "top", "bottom", "header", "footer"].into_iter().enumerate() {
            if let (Some(v), Some(slot)) = (pm.attr_f64(attr), sheet.print.margins.get_mut(i)) {
                *slot = v.clamp(0.0, 50.0) as f32;
            }
        }
    }
    if let Some(ps) = root.child("pageSetup") {
        if ps.attr("orientation") == Some("landscape") {
            sheet.print.orientation = Orientation::Landscape;
        }
        sheet.print.page_order = ps.attr("pageOrder").and_then(PageOrder::parse).unwrap_or_default();
        sheet.print.first_page_number = if ps.flag("useFirstPageNumber", false) { ps.attr_u32("firstPageNumber") } else { None };
        if let Some(p) = ps.attr_u32("paperSize").and_then(paper_name) {
            sheet.print.paper = p.to_string();
        }
        if let Some(s) = ps.attr_u32("scale") {
            sheet.print.scale = s.clamp(10, 400) as u16;
        }
        if fit_to_page {
            let w = ps.attr_u32("fitToWidth").unwrap_or(1).min(32767) as u16;
            let h = ps.attr_u32("fitToHeight").unwrap_or(1).min(32767) as u16;
            sheet.print.fit_to = Some((w, h));
        }
    }
    if let Some(hf) = root.child("headerFooter") {
        let pr = &mut sheet.print;
        for (tag, slot) in [
            ("oddHeader", &mut pr.header),
            ("oddFooter", &mut pr.footer),
            ("evenHeader", &mut pr.even_header),
            ("evenFooter", &mut pr.even_footer),
            ("firstHeader", &mut pr.first_header),
            ("firstFooter", &mut pr.first_footer),
        ] {
            *slot = hf.child(tag).map(|e| xml::decode_xstring(&e.text)).unwrap_or_default().chars().take(255).collect();
        }
        pr.different_first = hf.flag("differentFirst", false);
        pr.different_odd_even = hf.flag("differentOddEven", false);
        pr.hf_scale_with_doc = hf.flag("scaleWithDoc", true);
        pr.hf_align_with_margins = hf.flag("alignWithMargins", true);
    }
    if let Some(rb) = root.child("rowBreaks") {
        sheet.print.row_breaks = rb.kids("brk").filter_map(|b| b.attr_u32("id")).filter(|&r| r > 0 && r < MAX_ROWS).collect();
    }
    if let Some(cb) = root.child("colBreaks") {
        sheet.print.col_breaks = cb.kids("brk").filter_map(|b| b.attr_u32("id")).filter(|&c| c > 0 && c < MAX_COLS).collect();
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_streamed_cell(
    cx: &mut Ctx<'_>,
    sheet: &mut Sheet,
    st: &mut RowState,
    pos: CellRef,
    raw_style: u32,
    value: Value,
    raw_formula: Option<StreamedFormula<'_>>,
    value_metadata: Option<u32>,
    cell_metadata: Option<u32>,
) {
    let style = cx.style((raw_style != 0).then_some(raw_style));
    let picture = value_metadata.and_then(|vm| {
        if vm == 0 {
            return None;
        }
        let picture = (vm as usize).checked_sub(1).and_then(|n| cx.cell_pictures.get(n)).cloned().flatten();
        if picture.is_none() {
            cx.warn("some cells have unsupported or missing value metadata; kept their scalar fallback (cell pictures may be missing)");
        }
        picture
    });
    if let Some(picture) = picture {
        if raw_formula.is_some() {
            cx.warn("formula-generated cell pictures were imported as static pictures; their formulas were not retained");
        }
        sheet.set_picture(pos, picture);
        sheet.set_style(pos, style);
        return;
    }
    let dynamic = cell_metadata.is_some();
    st.shared_copy_refused = false;
    let formula = raw_formula.as_ref().and_then(|f| read_streamed_formula(cx, st, f, pos, dynamic));
    let py = match (&formula, &raw_formula) {
        (Some(fm), Some(f)) => crate::python_part::cell_code(&mut cx.python, fm, f.text),
        _ => crate::python_part::PyFormula::Not,
    };
    let (formula, py) = match py {
        crate::python_part::PyFormula::Not => (formula, None),
        crate::python_part::PyFormula::Code(py) => (None, Some(Arc::new(py))),
        crate::python_part::PyFormula::Missing => {
            forget_dynamic(st, pos);
            (None, None)
        }
    };
    if py.is_some() {
        record_python_spill(st, sheet, pos);
    }
    if formula.as_ref().is_some_and(|f| !f.dynamic && f.array.is_none() && f.has_cached_expr()) {
        cx.legacy_formulas.push(pos);
    }
    let spilled_from = if formula.is_none() && py.is_none() && !st.shared_copy_refused { dynamic_range(st, pos) } else { None };
    let python_anchor = spilled_from.is_some_and(|r| sheet.cells.get(r.start).is_some_and(|a| a.py.is_some()));
    let value = match spilled_from {
        Some(r) if python_anchor && r.count() <= gridcraft_model::python::MAX_RESULT_CELLS as u64 => {
            if !value.is_empty() {
                sheet.spill.insert(pos, value);
            }
            sheet.spill_ranges.insert(r.start, r);
            Value::Empty
        }
        Some(_) if python_anchor => value,
        Some(_) => Value::Empty,
        None => value,
    };
    if dynamic
        && formula.as_ref().is_some_and(|f| f.array.is_none())
        && let Some(r) = st.dynamic.last().filter(|r| r.start == pos)
    {
        sheet.spill_ranges.insert(pos, *r);
    }
    sheet.cells.set(pos, Cell { value, formula: formula.map(Arc::new), style, py });
}

/// Whether `e` contains no `Name` or `Struct` whose identifier case is preserved by the parser,
/// making outside-quotes ASCII case folding (with exact matching inside `"..."`, `'...'`, and
/// unquoted sheet selectors before `!`) 100% AST-exact.
fn is_case_safe(e: &Expr) -> bool {
    let mut safe = true;
    e.walk(&mut |x| {
        if safe && matches!(x, Expr::Name(_) | Expr::Struct(_)) {
            safe = false;
        }
    });
    safe
}

/// Compares two formula strings for byte-for-byte equality inside `"..."`, `'...'`, and unquoted
/// sheet selectors before `!`, and ASCII case-insensitive equality elsewhere.
fn eq_formula_outside_quotes(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let mut quote = 0u8;
    for i in 0..ab.len() {
        let (x, y) = (ab[i], bb[i]);
        if quote != 0 {
            if x != y {
                return false;
            }
            if x == quote {
                quote = 0;
            }
        } else if x == b'"' || x == b'\'' {
            if x != y {
                return false;
            }
            quote = x;
        } else if x != y {
            if !x.eq_ignore_ascii_case(&y) {
                return false;
            }
            // Preserve exact case for unquoted sheet names (`Sheet1!A1`, `Sheet1:Sheet3!A1`).
            let mut j = i + 1;
            while j < ab.len() && (ab[j].is_ascii_alphanumeric() || matches!(ab[j], b'_' | b'.' | b':')) {
                j += 1;
            }
            if ab.get(j) == Some(&b'!') {
                return false;
            }
        }
    }
    true
}

/// Returns `true` when `cur` is structurally equal to `shift_relative(base, dr, dc)` without cloning `base`.
fn expr_eq_shifted(base: &Expr, dr: i64, dc: i64, cur: &Expr) -> bool {
    let mut stack = Vec::with_capacity(8);
    stack.push((base, cur));
    while let Some((a, b)) = stack.pop() {
        match (a, b) {
            (Expr::Number(x), Expr::Number(y)) if x.to_bits() == y.to_bits() => {}
            (Expr::Text(x), Expr::Text(y)) if x == y => {}
            (Expr::Bool(x), Expr::Bool(y)) if x == y => {}
            (Expr::Error(x), Expr::Error(y)) if x == y => {}
            (Expr::Missing, Expr::Missing) => {}
            (Expr::Name(x), Expr::Name(y)) if x == y => {}
            (Expr::Struct(x), Expr::Struct(y)) if x == y => {}
            (Expr::Ref(ra), Expr::Ref(rb)) => {
                if ra.sheet != rb.sheet {
                    return false;
                }
                match gridcraft_formula::adjust::shift_ref_kind(&ra.kind, dr, dc) {
                    Some(k) if k == rb.kind => {}
                    _ => return false,
                }
            }
            (Expr::Ref(ra), Expr::Error(CellError::Ref)) => {
                if gridcraft_formula::adjust::shift_ref_kind(&ra.kind, dr, dc).is_some() {
                    return false;
                }
            }
            (Expr::Unary(oa, xa), Expr::Unary(ob, xb)) if oa == ob => stack.push((xa, xb)),
            (Expr::Paren(xa), Expr::Paren(xb)) => stack.push((xa, xb)),
            (Expr::Binary(oa, la, ra), Expr::Binary(ob, lb, rb)) if oa == ob => {
                stack.push((ra, rb));
                stack.push((la, lb));
            }
            (Expr::Call(na, aa), Expr::Call(nb, ab)) if na == nb && aa.len() == ab.len() => {
                for (x, y) in aa.iter().zip(ab.iter()).rev() {
                    stack.push((x, y));
                }
            }
            (Expr::Invoke(ca, aa), Expr::Invoke(cb, ab)) if aa.len() == ab.len() => {
                for (x, y) in aa.iter().zip(ab.iter()).rev() {
                    stack.push((x, y));
                }
                stack.push((ca, cb));
            }
            (Expr::Array(ra), Expr::Array(rb)) if ra.len() == rb.len() => {
                for (row_a, row_b) in ra.iter().zip(rb.iter()) {
                    if row_a.len() != row_b.len() {
                        return false;
                    }
                    for (x, y) in row_a.iter().zip(row_b.iter()) {
                        stack.push((x, y));
                    }
                }
            }
            _ => return false,
        }
    }
    true
}

/// Parses or deduplicates a non-shared formula text at `pos` against the column's previous template.
fn parse_or_dedup_formula(st: &mut RowState, pos: CellRef, text: &str) -> Formula {
    if let Some(tmpl) = st.col_templates.get(&pos.col) {
        let dr = pos.row as i64 - tmpl.base_row as i64;
        if dr != 0
            && let Ok(dr32) = i32::try_from(dr)
        {
            st.scratch.clear();
            gridcraft_formula::printer::write_shifted_into(&mut st.scratch, &tmpl.base, dr, 0);
            if text == st.scratch || (tmpl.case_safe && eq_formula_outside_quotes(text, &st.scratch)) {
                return Formula::from_shared(Arc::clone(&tmpl.base), dr32, 0);
            }
        }
    }
    match gridcraft_formula::parse(text) {
        Ok(parsed) => {
            if let Some(tmpl) = st.col_templates.get(&pos.col) {
                let dr = pos.row as i64 - tmpl.base_row as i64;
                if let Ok(dr32) = i32::try_from(dr)
                    && expr_eq_shifted(&tmpl.base, dr, 0, &parsed)
                {
                    return Formula::from_shared(Arc::clone(&tmpl.base), dr32, 0);
                }
            }
            let arc = Arc::new(parsed);
            let case_safe = is_case_safe(&arc);
            st.col_templates.insert(pos.col, ColTemplate { base_row: pos.row, base: Arc::clone(&arc), case_safe });
            Formula::from_shared(arc, 0, 0)
        }
        Err(_) => Formula::new(text),
    }
}

fn read_streamed_formula(cx: &mut Ctx<'_>, st: &mut RowState, f: &StreamedFormula<'_>, pos: CellRef, dynamic: bool) -> Option<Formula> {
    let mut unwrapped = false;
    let fm = read_streamed_formula_text(cx, st, f, pos, dynamic, &mut unwrapped)?;
    let dynamic = dynamic || unwrapped;
    Some(fm.with_dynamic(dynamic))
}

fn read_streamed_formula_text(
    cx: &mut Ctx<'_>,
    st: &mut RowState,
    f: &StreamedFormula<'_>,
    pos: CellRef,
    dynamic: bool,
    unwrapped: &mut bool,
) -> Option<Formula> {
    let mut model = from_file(f.text);
    if let Some(known) = cx.known_function
        && let Some(u) = unwrap_google_sheets(&model, known)
    {
        model = u;
        *unwrapped = true;
    }
    let text = model.as_str();
    match f.kind {
        StreamedFormulaKind::Shared => {
            let si = f.shared_index?;
            if !text.is_empty() {
                let expr = gridcraft_formula::parse(text).ok().map(Arc::new);
                let cost = shared_cost(text, expr.as_deref());
                let out = match &expr {
                    Some(e) => Formula::from_shared(Arc::clone(e), 0, 0),
                    None => Formula::new(text),
                };
                st.shared.insert(si, Shared { anchor: pos, expr, text: text.to_string(), unwrapped: *unwrapped, cost });
                Some(out)
            } else {
                let Some(sh) = st.shared.get(&si) else {
                    cx.warn("a shared formula refers to a missing master cell; kept the cached value");
                    return None;
                };
                if sh.expr.is_none()
                    && sh.text.len() > gridcraft_formula::MAX_FORMULA_LEN
                    && sh.text.chars().count() > gridcraft_formula::MAX_FORMULA_LEN
                {
                    cx.warn(format!(
                        "a shared formula is longer than {} characters; kept the cached values of the cells that share it",
                        gridcraft_formula::MAX_FORMULA_LEN
                    ));
                    st.shared_copy_refused = true;
                    return None;
                }
                if !charge(&mut cx.shared_formula_bytes, extra_shared_cost(sh.cost), MAX_SHARED_FORMULA_BYTES) {
                    cx.warn("shared formulas expand to too much data; kept the cached values of the remaining cells that share them");
                    st.shared_copy_refused = true;
                    return None;
                }
                *unwrapped |= sh.unwrapped;
                Some(match &sh.expr {
                    Some(e) => {
                        let dr = pos.row as i64 - sh.anchor.row as i64;
                        let dc = pos.col as i64 - sh.anchor.col as i64;
                        if let (Ok(dr32), Ok(dc32)) = (i32::try_from(dr), i32::try_from(dc)) {
                            Formula::from_shared(Arc::clone(e), dr32, dc32)
                        } else {
                            Formula::from_expr(gridcraft_formula::adjust::shift_relative(e.as_ref().clone(), dr, dc))
                        }
                    }
                    None => Formula::new(&sh.text),
                })
            }
        }
        StreamedFormulaKind::Array => {
            if text.is_empty() {
                return None;
            }
            let mut fm = parse_or_dedup_formula(st, pos, text);
            let range = f.range.unwrap_or(RangeRef::cell(pos));
            if !range.is_single() {
                push_dynamic(st, range);
            }
            if !dynamic {
                fm.array = Some(range);
            }
            Some(fm)
        }
        StreamedFormulaKind::DataTable => {
            cx.warn("what-if data tables are not supported; kept their values");
            None
        }
        StreamedFormulaKind::Normal => {
            if text.is_empty() {
                None
            } else {
                Some(parse_or_dedup_formula(st, pos, text))
            }
        }
    }
}

fn read_sheet_notes_and_tables(cx: &mut Ctx<'_>, sp: &CraftSheetPart, rels: &[Rel], sheet: &mut Sheet) -> Result<(), IoError> {
    let tables = cx.pkg.craft_mut().tables(sp).map_err(craft_error)?;
    for (_, t) in tables {
        let mut model = parts::from_craft_table(cx, &t);
        model.id = cx.next_object_id();
        sheet.tables.push(model);
    }
    if let Some(comments) = cx.pkg.craft_mut().comments(sp).map_err(craft_error)? {
        parts::apply_craft_comments(&comments, sheet);
    }
    for rel in rels.iter().filter(|r| r.kind == "threadedComment") {
        if let Some(x) = cx.optional_xml(&rel.target)? {
            read_threaded(cx, &x, sheet);
        }
    }
    if !sheet.comments.is_empty() {
        match cx.pkg.craft_mut().note_shapes(sp) {
            Ok(shapes) => parts::apply_craft_note_shapes(&shapes, sheet),
            Err(e) => cx.warn(format!("sheet \"{}\": note drawing could not be read ({e})", sheet.name)),
        }
    }
    Ok(())
}

struct RawCol {
    min: u32,
    max: u32,
    width: Option<f64>,
    custom_width: bool,
    best_fit: bool,
    hidden: bool,
    outline_level: u8,
    collapsed: bool,
    style: Option<u32>,
}

fn apply_col_entry(cx: &Ctx<'_>, c: RawCol, remaining: &mut u64, sheet: &mut Sheet) -> Result<(), IoError> {
    let min = c.min;
    let max = c.max.min(MAX_COLS);
    if min == 0 || min > MAX_COLS || max < min {
        return Ok(());
    }
    let mut info = LineInfo::default();
    if let Some(w) = c.width {
        let px = col_width_to_px(w);
        // Excel writes customWidth for every width it stores, and bestFit as well for one that
        // AutoFit or a number format sized: only customWidth without bestFit was set by hand.
        info.custom = c.custom_width && !c.best_fit;
        if c.custom_width || px != sheet.default_col_width {
            info.size = Some(px);
        }
    }
    info.hidden = c.hidden;
    info.outline = c.outline_level.min(7);
    info.collapsed = c.collapsed;
    if let Some(s) = c.style {
        let id = cx.style(Some(s));
        if id != gridcraft_model::StyleId::DEFAULT {
            info.style = Some(id);
        }
    }
    if info == LineInfo::default() {
        return Ok(());
    }
    *remaining = remaining
        .checked_sub(u64::from(max - min + 1))
        .ok_or_else(|| IoError::TooLarge(format!("sheet \"{}\": more than {MAX_COLUMN_ASSIGNMENTS} expanded column assignments", sheet.name)))?;
    for col in (min - 1)..max {
        sheet.cols.insert(col, info);
    }
    Ok(())
}

struct RawHyperlink<'a> {
    range: RangeRef,
    rel_id: Option<&'a str>,
    location: Option<&'a str>,
    tooltip: Option<&'a str>,
}

fn push_hyperlink(cx: &mut Ctx<'_>, name: &str, h: RawHyperlink<'_>, rels: &HashMap<&str, &Rel>, sheet: &mut Sheet, budget: &mut LinkBudget) -> bool {
    let rel_target = h.rel_id.and_then(|id| rels.get(id)).map(|r| r.target.as_str()).unwrap_or("");
    let location = h.location.filter(|l| !l.is_empty());
    if rel_target.len().saturating_add(location.map_or(0, str::len)) > MAX_HYPERLINK_TARGET {
        cx.warn(format!("sheet \"{name}\": hyperlinks longer than {MAX_HYPERLINK_TARGET} characters were dropped"));
        return true;
    }
    let mut target = rel_target.to_string();
    let external = !target.is_empty();
    if let Some(loc) = location {
        if target.is_empty() {
            target = loc.trim_start_matches('#').to_string();
        } else {
            target = format!("{target}#{loc}");
        }
    }
    if target.is_empty() {
        return true;
    }
    let tooltip = h.tooltip.map(|t| truncate_chars(t, MAX_TOOLTIP_CHARS));
    let link_bytes = (target.len() + tooltip.as_ref().map_or(0, String::len)) as u64;
    let link = Hyperlink { target, tooltip, external };
    for c in h.range.iter() {
        if budget.cells >= MAX_HYPERLINK_CELLS {
            cx.warn(format!("sheet \"{name}\": hyperlinks past the first {MAX_HYPERLINK_CELLS} cells were dropped"));
            return false;
        }
        if budget.bytes.saturating_add(link_bytes) > MAX_HYPERLINK_BYTES {
            cx.warn(format!("sheet \"{name}\": hyperlink targets past {MAX_HYPERLINK_BYTES} bytes were dropped"));
            return false;
        }
        budget.cells += 1;
        budget.bytes += link_bytes;
        sheet.hyperlinks.insert(c, link.clone());
    }
    true
}

/// Buttons, scroll bars and spin buttons: the `<controls>` list (also inside
/// `mc:AlternateContent`) with its properties parts, else the shapes of the VML drawing that
/// `<legacyDrawing r:id>` names (not the header and footer drawing of `<legacyDrawingHF>`).
/// Returns the VML shape numbers read, so pass-through keeps only the others.
fn read_form_controls(cx: &mut Ctx<'_>, root: &El, rels: &[Rel], sheet: &mut Sheet) -> crate::controls::ReadControls {
    let controls = root.children.iter().filter_map(crate::controls::controls_of).next_back();
    let vml = root.child("legacyDrawing").and_then(|e| e.attr("id"));
    if controls.is_some() || vml.is_some() { crate::controls::read(cx, controls, vml, rels, sheet) } else { Default::default() }
}

/// A protection password hash: the iterated attributes (`algorithmName`, `hashValue`,
/// `saltValue`, `spinCount`) or the legacy 16-bit `password`, each name preceded by `prefix`
/// (`workbook` gives `workbookHashValue`…). One that can't be read keeps a hash that never
/// matches, so what it protects stays locked rather than open.
pub(crate) fn read_password(e: &El, prefix: &str) -> Option<PasswordHash> {
    let attr = |base: &str| e.attr(&password_attr(prefix, base));
    if attr("algorithmName").is_some() || attr("hashValue").is_some() {
        return Some(PasswordHash::Iterated {
            algorithm: attr("algorithmName").unwrap_or("").to_string(),
            hash: attr("hashValue").unwrap_or("").to_string(),
            salt: attr("saltValue").unwrap_or("").to_string(),
            spin_count: e.attr_u32(&password_attr(prefix, "spinCount")).unwrap_or(0),
        });
    }
    attr("password").map(|h| match u16::from_str_radix(h.trim(), 16) {
        Ok(h) => PasswordHash::Legacy(h),
        Err(_) => PasswordHash::Iterated { algorithm: String::new(), hash: String::new(), salt: String::new(), spin_count: 0 },
    })
}

/// `base` preceded by `prefix` in camel case: ("workbook", "hashValue") → `workbookHashValue`.
pub(crate) fn password_attr(prefix: &str, base: &str) -> String {
    let mut chars = base.chars();
    match chars.next() {
        Some(c) if !prefix.is_empty() => format!("{prefix}{}{}", c.to_ascii_uppercase(), chars.as_str()),
        _ => base.to_string(),
    }
}

/// Allow Edit Ranges. A range whose password can't be read keeps a hash that never matches
/// (or an unsupported algorithm), so it stays locked rather than open.
fn read_protected_ranges(e: &El, sheet: &mut Sheet) {
    for p in e.kids("protectedRange") {
        let ranges: Vec<RangeRef> = p.attr("sqref").unwrap_or("").split_whitespace().filter_map(RangeRef::parse).collect();
        if ranges.is_empty() {
            continue;
        }
        let password = read_password(p, "");
        let title = p.attr("name").unwrap_or("").to_string();
        sheet.edit_ranges.push(EditRange { title, ranges, password, security_descriptor: p.attr("securityDescriptor").map(str::to_string) });
    }
}

/// Charges `n` cell elements against the workbook's [`MAX_CELLS`].
fn charge_cells(cx: &mut Ctx<'_>, n: u64) -> Result<(), IoError> {
    cx.cells = cx.cells.saturating_add(n);
    if cx.cells > MAX_CELLS {
        return Err(IoError::TooLarge(format!("the workbook has more than {MAX_CELLS} cells")));
    }
    Ok(())
}

/// `2024-03-15`, `2024-03-15T10:30:00Z`, `10:30:00` → serial.
pub fn parse_iso_datetime(s: &str, sys: DateSystem) -> Option<f64> {
    parse_iso_datetime_with(s, |y, m, d| serial_from_ymd(sys, y, m, d))
}

/// [`parse_iso_datetime`] with `ymd` turning the date part (year, month 1–12, day 1–31) into a serial.
pub(crate) fn parse_iso_datetime_with(s: &str, ymd: impl Fn(i64, i64, i64) -> Option<f64>) -> Option<f64> {
    let s = s.trim_end_matches('Z');
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None if s.contains(':') => ("", Some(s)),
        None => (s, None),
    };
    let mut serial = 0.0;
    if !date.is_empty() {
        let mut it = date.split('-');
        let y: i64 = it.next()?.parse().ok()?;
        let m: i64 = it.next()?.parse().ok()?;
        let d: i64 = it.next()?.parse().ok()?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        serial = ymd(y, m, d)?;
    }
    if let Some(t) = time {
        // Drop a time-zone offset.
        let t = t.split(['+']).next().unwrap_or(t);
        let mut it = t.split(':');
        let h: f64 = it.next()?.parse().ok()?;
        let mi: f64 = it.next().unwrap_or("0").parse().ok()?;
        let se: f64 = it.next().unwrap_or("0").parse().ok()?;
        if !(0.0..24.0).contains(&h) || !(0.0..60.0).contains(&mi) || !(0.0..61.0).contains(&se) {
            return None;
        }
        serial += time_fraction(h, mi, se);
    }
    Some(serial)
}

/// What one dependent of a shared formula costs: its printed text plus its expression tree.
fn shared_cost(text: &str, expr: Option<&Expr>) -> u64 {
    let mut nodes = 0u64;
    if let Some(e) = expr {
        e.walk(&mut |_| nodes = nodes.saturating_add(1));
    }
    let node = std::mem::size_of::<Expr>() as u64;
    (text.len() as u64).saturating_mul(2).saturating_add(nodes.saturating_mul(node)).saturating_add(64)
}

/// What one dependent of a shared formula costing `cost` is charged: its cost beyond an ordinary
/// formula cell's.
fn extra_shared_cost(cost: u64) -> u64 {
    cost.saturating_sub(SHARED_FORMULA_FREE_COST)
}

/// The reader's charge for each dependent of this emitted master, or `None` when the
/// reader cannot reconstruct it by shifting the same formula. The worksheet writer
/// excludes Google Sheets wrappers, whose unwrapping depends on the reader's callback.
pub(crate) fn shared_master_extra_cost(text: &str, base: &Expr, dr: i32, dc: i32) -> Option<u64> {
    // `push_esc` drops characters XML cannot carry; all other formula text decodes losslessly.
    if !text.chars().all(craft_xml::is_xml_char) {
        return None;
    }
    let model = from_file(text.trim());
    let expr = gridcraft_formula::parse(&model).ok()?;
    // File normalization can change what parses as a relative reference, e.g. `_xlpm.A1`.
    expr_eq_shifted(base, i64::from(dr), i64::from(dc), &expr).then(|| extra_shared_cost(shared_cost(&model, Some(&expr))))
}

/// Adds `cost` to `used` when that stays within `cap`; `false` (and `used` unchanged) otherwise.
fn charge(used: &mut u64, cost: u64, cap: u64) -> bool {
    match used.checked_add(cost) {
        Some(total) if total <= cap => {
            *used = total;
            true
        }
        _ => false,
    }
}

/// The first `max` characters of `s`.
fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => s.get(..at).unwrap_or(s).to_string(),
        None => s.to_string(),
    }
}

pub fn parse_sqref(s: Option<&str>) -> Vec<RangeRef> {
    s.unwrap_or("").split_whitespace().filter_map(RangeRef::parse).collect()
}

/// Worksheet extensions: x14 data validations and sparklines.
fn read_ext(cx: &mut Ctx<'_>, e: &El, sheet: &mut Sheet, cf_kept_cells: &mut u64) {
    for ext in e.kids("ext") {
        crate::extlist::read_sheet_ext(cx, ext);
        for c in &ext.children {
            match c.name.as_str() {
                "dataValidations" => {
                    for v in c.kids("dataValidation") {
                        if let Some(v) = read_validation(v) {
                            sheet.validations.push(v);
                        }
                    }
                }
                "sparklineGroups" => {
                    for g in c.kids("sparklineGroup") {
                        let kind = match g.attr("type") {
                            Some("column") => SparklineKind::Column,
                            Some("stacked") => SparklineKind::WinLoss,
                            _ => SparklineKind::Line,
                        };
                        let color = g.child("colorSeries").map(|c| read_color(c, &cx.palette)).unwrap_or_default();
                        let markers = g.flag("markers", false);
                        for s in g.child("sparklines").map(|s| s.kids("sparkline").collect::<Vec<_>>()).unwrap_or_default() {
                            let source = s.child("f").map(|f| f.text.trim().to_string()).unwrap_or_default();
                            if let Some(cell) = s.child("sqref").and_then(|q| CellRef::parse(q.text.trim())) {
                                sheet.sparklines.push(Sparkline { cell, source, kind, color, markers });
                            }
                        }
                    }
                }
                "conditionalFormattings" => read_x14_cf(cx, c, sheet, cf_kept_cells),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_formula_budget_stops_expansion() {
        let expr = gridcraft_formula::parse("SUM(1,2,3)").ok();
        let cost = shared_cost("SUM(1,2,3)", expr.as_ref());
        assert!(cost > 64 + 4 * std::mem::size_of::<Expr>() as u64);
        let cap = cost * 3;
        let mut used = 0;
        assert!((0..3).all(|_| charge(&mut used, cost, cap)));
        assert!(!charge(&mut used, cost, cap));
        assert_eq!(used, cap);
        assert!(!charge(&mut used, u64::MAX, u64::MAX));
    }

    #[test]
    fn ordinary_shared_formulas_are_free_and_long_ones_pay_their_excess() {
        let text = r#"IF(AND(A1>0,B1<>""),VLOOKUP(C1,Sheet2!$A$1:$D$500,4,FALSE)*SUM(D1:D10)/COUNT(D1:D10),IFERROR(E1/F1,0))"#;
        let expr = gridcraft_formula::parse(text).ok();
        assert!(expr.is_some());
        assert_eq!(extra_shared_cost(shared_cost(text, expr.as_ref())), 0, "an ordinary formula costs nothing extra");
        let long = vec![format!("SUM({}1)", "1,".repeat(199)); 10].join("+");
        let expr = gridcraft_formula::parse(&long).ok();
        assert!(expr.is_some());
        let cost = shared_cost(&long, expr.as_ref());
        assert_eq!(extra_shared_cost(cost), cost - SHARED_FORMULA_FREE_COST);
        // A few thousand copies of a long master no longer fit the budget.
        assert!(extra_shared_cost(cost).saturating_mul(10_000) > MAX_SHARED_FORMULA_BYTES);
    }

    #[test]
    fn truncate_keeps_whole_characters() {
        assert_eq!(truncate_chars("ab\u{e9}d", 3), "ab\u{e9}");
        assert_eq!(truncate_chars("ab", 3), "ab");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(parse_iso_datetime("1900-01-01", DateSystem::D1900), Some(1.0));
        assert_eq!(parse_iso_datetime("2024-01-01T12:00:00Z", DateSystem::D1900), Some(45292.5));
        assert_eq!(parse_iso_datetime("1904-01-02", DateSystem::D1904), Some(1.0));
        assert_eq!(parse_iso_datetime("06:00:00", DateSystem::D1900), Some(0.25));
        assert_eq!(parse_iso_datetime("garbage", DateSystem::D1900), None);
        assert_eq!(parse_iso_datetime("2024-13-01", DateSystem::D1900), None);
    }

    fn ctx(pkg: crate::package::Package<'_>) -> Ctx<'_> {
        Ctx {
            pkg,
            report: Default::default(),
            sst: vec![],
            phonetics: vec![],
            xf_map: vec![],
            dxfs: vec![],
            palette: vec![],
            date_system: DateSystem::D1900,
            styles: Default::default(),
            persons: Default::default(),
            next_id: 1,
            num_fmts: Default::default(),
            cell_pictures: vec![],
            cf_ext_ids: vec![],
            sheet_exts: vec![],
            sheet_exts_dropped: vec![],
            known_function: None,
            python: Default::default(),
            chart_parts: vec![],
            cells: 0,
            shared_formula_bytes: 0,
            legacy_formulas: vec![],
            pivot_charts: vec![],
            chart_budget: crate::chart::ChartReadBudget::new(),
        }
    }

    /// #2159: the workbook-wide cell cap refuses the open once passed, across sheets.
    #[test]
    fn cells_past_the_workbook_cap_are_refused() {
        let bytes = crate::tests::minimal(r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row></sheetData>"#, &[], "", "");
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        cx.cells = MAX_CELLS - 2;
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert_eq!(sheet.cells.len(), 2);
        assert!(matches!(read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data"), Err(IoError::TooLarge(_))));
    }

    /// #2159: the cells of a row dropped for its row number still count against the cap, so a
    /// part read for many sheets can't parse cells uncounted.
    #[test]
    fn cells_of_dropped_rows_count_against_the_cap() {
        let rows = r#"<sheetData><row r="0"><c><v>1</v></c><c><v>2</v></c></row><row r="2000000"><c><v>3</v></c></row></sheetData>"#;
        let bytes = crate::tests::minimal(rows, &[], "", "");
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert!(sheet.cells.is_empty(), "both rows are dropped");
        assert_eq!(cx.cells, 3);
        cx.cells = MAX_CELLS - 2;
        assert!(matches!(read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data"), Err(IoError::TooLarge(_))));
    }

    #[test]
    fn sqref_keeps_more_than_ten_thousand_ranges() {
        let sq = (1..=12_000).map(|r| format!("A{r}")).collect::<Vec<_>>().join(" ");
        let ranges = parse_sqref(Some(&sq));
        assert_eq!(ranges.len(), 12_000);
        assert_eq!(ranges.last().copied(), RangeRef::parse("A12000"));
    }

    #[test]
    fn stream_normalizes_xml_newlines_and_preserves_char_refs() {
        let xml = concat!(
            "<sheetData><row r=\"1\">",
            "<c r=\"A1\" t=\"inlineStr\"><is><t>a\r\nb\rc&#xD;d</t></is></c>",
            "<c r=\"B1\" t=\"str\"><v>x\r\ny\rz</v></c>",
            "<c r=\"C1\" t=\"inlineStr\"><is><t><![CDATA[p\r\nq\rr]]></t></is></c>",
            "</row></sheetData>",
        );
        let bytes = crate::tests::minimal(xml, &[], "", "");
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert_eq!(sheet.value(CellRef::parse("A1").unwrap()), Value::text("a\nb\nc\rd"));
        assert_eq!(sheet.value(CellRef::parse("B1").unwrap()), Value::text("x\ny\nz"));
        assert_eq!(sheet.value(CellRef::parse("C1").unwrap()), Value::text("p\nq\nr"));
    }

    #[test]
    fn stream_preserves_invalid_utf8_bytes_lossily() {
        let mut sheet_xml = Vec::new();
        sheet_xml.extend_from_slice(
            br#"<?xml version="1.0" encoding="ISO-8859-1"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="str"><v>caf"#,
        );
        sheet_xml.push(0xE9);
        sheet_xml.extend_from_slice(br#"</v></c><c r="B1" t="inlineStr"><is><t>na"#);
        sheet_xml.push(0xEF);
        sheet_xml.extend_from_slice(b"ve\r\nok</t></is></c></row></sheetData></worksheet>");
        let wb = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let wb_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let bytes = crate::tests::make_zip(&[
            ("[Content_Types].xml", crate::tests::CT.as_bytes()),
            ("_rels/.rels", crate::tests::ROOT_RELS.as_bytes()),
            ("xl/workbook.xml", wb),
            ("xl/_rels/workbook.xml.rels", wb_rels),
            ("xl/worksheets/sheet1.xml", &sheet_xml),
        ]);
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert_eq!(sheet.value(CellRef::parse("A1").unwrap()), Value::text("caf\u{FFFD}"));
        assert_eq!(sheet.value(CellRef::parse("B1").unwrap()), Value::text("na\u{FFFD}ve\nok"));
    }

    #[test]
    fn stream_ignores_stray_unmatched_end_tags_and_preserves_subsequent_rows() {
        let xml = concat!(
            r#"<sheetData>"#,
            r#"<row r="1"><c r="A1"><v>1</v></c></foo><c r="B1"><v>2</v></c></c><c r="C1"><v>3</v></c></row>"#,
            r#"</row>"#,
            r#"<row r="2"><c r="A2"><v>4</v></c></row>"#,
            r#"</sheetData>"#,
        );
        let bytes = crate::tests::minimal(xml, &[], "", "");
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert_eq!(sheet.value(CellRef::parse("A1").unwrap()), Value::Number(1.0));
        assert_eq!(sheet.value(CellRef::parse("B1").unwrap()), Value::Number(2.0));
        assert_eq!(sheet.value(CellRef::parse("C1").unwrap()), Value::Number(3.0));
        assert_eq!(sheet.value(CellRef::parse("A2").unwrap()), Value::Number(4.0));
    }

    #[test]
    fn stream_warns_when_out_of_bounds_rows_or_cells_are_dropped() {
        let xml = r#"<sheetData><row r="0"><c><v>1</v></c></row><row r="2000000"><c><v>2</v></c></row><row r="1"><c r="XFE1"><v>3</v></c><c r="A1"><v>4</v></c></row></sheetData>"#;
        let bytes = crate::tests::minimal(xml, &[], "", "");
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert_eq!(sheet.value(CellRef::parse("A1").unwrap()), Value::Number(4.0));
        assert!(
            cx.report.warnings.iter().any(|w| w.contains("3 cells or rows outside the 1,048,576 × 16,384 grid were dropped")),
            "{:?}",
            cx.report.warnings
        );
    }

    #[test]
    fn stream_truncated_sheet_data_flushes_open_cell_and_warns_damaged() {
        let sheet_xml = br#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v>"#;
        let wb = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let wb_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let bytes = crate::tests::make_zip(&[
            ("[Content_Types].xml", crate::tests::CT.as_bytes()),
            ("_rels/.rels", crate::tests::ROOT_RELS.as_bytes()),
            ("xl/workbook.xml", wb),
            ("xl/_rels/workbook.xml.rels", wb_rels),
            ("xl/worksheets/sheet1.xml", sheet_xml),
        ]);
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert_eq!(sheet.value(CellRef::parse("A1").unwrap()), Value::Number(1.0));
        assert_eq!(sheet.value(CellRef::parse("A2").unwrap()), Value::Number(2.0));
        assert!(cx.report.warnings.iter().any(|w| w.contains("is damaged (unexpected end of file)")), "{:?}", cx.report.warnings);
    }

    #[test]
    fn stream_truncated_mid_tag_flushes_open_cell_and_warns_damaged() {
        let sheet_xml = br#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c"#;
        let wb = br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
        let wb_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let bytes = crate::tests::make_zip(&[
            ("[Content_Types].xml", crate::tests::CT.as_bytes()),
            ("_rels/.rels", crate::tests::ROOT_RELS.as_bytes()),
            ("xl/workbook.xml", wb),
            ("xl/_rels/workbook.xml.rels", wb_rels),
            ("xl/worksheets/sheet1.xml", sheet_xml),
        ]);
        let pkg = crate::package::Package::open(&bytes).unwrap();
        let mut cx = ctx(pkg);
        let (sheet, _) = read_sheet(&mut cx, "xl/worksheets/sheet1.xml", "Data").unwrap();
        assert_eq!(sheet.value(CellRef::parse("A1").unwrap()), Value::Number(1.0));
        assert_eq!(sheet.value(CellRef::parse("A2").unwrap()), Value::Number(2.0));
        assert!(cx.report.warnings.iter().any(|w| w.contains("is damaged (")), "{:?}", cx.report.warnings);
    }
}
