//! Reading worksheet parts.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::date::{serial_from_ymd, time_fraction};
use gridcraft_core::{CellError, CellRef, DateSystem, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_model::{
    AutoFilter, Cell, CfOperator, CfRule, CfValueKind, Comment, CondFormat, EditRange, ErrorStyle, FilterCriterion, Formula, Hyperlink, LineInfo,
    NoteBox, NoteCorner, Orientation, PasswordHash, Sheet, SheetProtection, Sparkline, SparklineKind, Table, TableColumn, TotalsFn, Validation,
    ValidationKind, XmlColumnPr,
};

use crate::IoError;
use crate::fmla::from_file;
use crate::package::Rel;
use crate::read::Ctx;
use crate::styles::read_color;
use crate::tables::{col_width_to_px, paper_name, pt_to_px};
use crate::xml::{self, El};

mod parts;
mod rules;

pub use parts::{read_autofilter, read_table};
use parts::{read_comments, read_hyperlinks, read_note_shapes, read_threaded};
use rules::{read_cf, read_validation};

struct Shared {
    anchor: CellRef,
    expr: Option<Expr>,
    text: String,
}

#[derive(Default)]
struct RowState {
    next_row: u32,
    shared: HashMap<u32, Shared>,
    /// Dynamic-array spill ranges and legacy array ranges: cached values inside them (other than
    /// the anchor) are not stored, so the formula can fill them again.
    dynamic: Vec<RangeRef>,
    /// Cells inside the ranges above, for a cap before a pathological sheet proves it itself.
    dynamic_cells: u64,
    /// Row-membership checks served so far, for the work cap above.
    dynamic_checks: u64,
    dropped: u64,
    rows: u64,
}

/// Whether `pos` lies in a known dynamic range that isn't its anchor: the cell's cached value
/// gives way to the formula that will fill it. Charges every scanned range against one work
/// budget, then gives up consulting the ranges at all: the worst kind of file may then import
/// a few cached spill values, but it never stalls the whole import.
fn in_dynamic(st: &mut RowState, pos: CellRef) -> bool {
    if st.dynamic_checks > MAX_DYNAMIC_CHECKS || st.dynamic.is_empty() {
        return false;
    }
    let ranges = st.dynamic.len();
    let hit = st.dynamic.iter().any(|r| r.contains(pos) && r.start != pos);
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

const MAX_HYPERLINK_CELLS: u64 = 10_000;
/// Target and tooltip bytes kept for hyperlinks of one sheet, over all cells: the same URL is
/// copied for every cell of its range, so length × cells must stay bounded whatever the file.
const MAX_HYPERLINK_BYTES: u64 = 8 * 1024 * 1024;
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

fn read_sheet_format(e: &El, sheet: &mut Sheet) {
    if let Some(h) = e.attr_f64("defaultRowHeight") {
        let px = pt_to_px(h);
        if px > 0.0 {
            sheet.default_row_height = px;
        }
    }
    if let Some(w) = e.attr_f64("defaultColWidth") {
        let px = col_width_to_px(w);
        if px > 0.0 {
            sheet.default_col_width = px;
        }
    } else if let Some(b) = e.attr_f64("baseColWidth") {
        let px = ((b.clamp(0.0, 255.0) * 7.0 + 5.0) / 8.0).ceil() * 8.0;
        sheet.default_col_width = px as f32;
    }
}

pub fn read_sheet(cx: &mut Ctx<'_>, part: &str, name: &str) -> Result<Sheet, IoError> {
    let bytes = cx.pkg.read(part)?.ok_or_else(|| IoError::Format(format!("missing part {part}")))?;
    let rels = cx.pkg.rels(part)?;
    let mut sheet = Sheet::new(name);
    let mut st = RowState::default();
    let res = xml::parse_streaming(&bytes, &["row"], &mut |row| {
        read_row(cx, &mut sheet, &mut st, &row);
        Ok(())
    });
    let root = match res {
        Ok(r) => r,
        Err(e) => {
            cx.warn(format!("sheet \"{name}\" is damaged ({e}); kept {} rows", st.rows));
            El::default()
        }
    };
    if st.dropped > 0 {
        cx.warn(format!("sheet \"{name}\": {} cells or rows outside the 1,048,576 × 16,384 grid were dropped", st.dropped));
    }
    drop(bytes);

    let mut fit_to_page = false;
    let mut column_assignments_left = MAX_COLUMN_ASSIGNMENTS;
    cx.cf_ext_ids.clear();
    for e in &root.children {
        match e.name.as_str() {
            "sheetPr" => {
                if let Some(t) = e.child("tabColor") {
                    sheet.tab_color = Some(read_color(t, &cx.palette));
                }
                if let Some(o) = e.child("outlinePr") {
                    sheet.summary_below = o.flag("summaryBelow", true);
                    sheet.summary_right = o.flag("summaryRight", true);
                }
                fit_to_page = e.child("pageSetUpPr").is_some_and(|p| p.flag("fitToPage", false));
            }
            "sheetViews" => read_views(e, &mut sheet),
            "sheetFormatPr" => read_sheet_format(e, &mut sheet),
            "cols" => read_cols(cx, e, &mut sheet, &mut column_assignments_left)?,
            "mergeCells" => {
                for m in e.kids("mergeCell") {
                    if let Some(r) = m.attr("ref").and_then(RangeRef::parse)
                        && !r.is_single()
                    {
                        sheet.merges.push(r);
                    }
                }
            }
            "conditionalFormatting" => read_cf(cx, e, &mut sheet),
            "dataValidations" => {
                for v in e.kids("dataValidation") {
                    if let Some(v) = read_validation(v) {
                        sheet.validations.push(v);
                    }
                }
            }
            "hyperlinks" => read_hyperlinks(cx, name, e, &rels, &mut sheet),
            "autoFilter" => sheet.autofilter = read_autofilter(cx, e),
            "printOptions" => {
                sheet.print.gridlines = e.flag("gridLines", false);
                sheet.print.headings = e.flag("headings", false);
                sheet.print.center_h = e.flag("horizontalCentered", false);
                sheet.print.center_v = e.flag("verticalCentered", false);
            }
            "pageMargins" => {
                let m = &mut sheet.print.margins;
                for (i, k) in ["left", "right", "top", "bottom", "header", "footer"].iter().enumerate() {
                    if let (Some(v), Some(slot)) = (e.attr_f64(k), m.get_mut(i)) {
                        *slot = v.clamp(0.0, 50.0) as f32;
                    }
                }
            }
            "pageSetup" => {
                if e.attr("orientation") == Some("landscape") {
                    sheet.print.orientation = Orientation::Landscape;
                }
                if let Some(p) = e.attr_u32("paperSize").and_then(paper_name) {
                    sheet.print.paper = p.to_string();
                }
                if let Some(s) = e.attr_u32("scale") {
                    sheet.print.scale = s.clamp(10, 400) as u16;
                }
                if fit_to_page || root.child("sheetPr").and_then(|p| p.child("pageSetUpPr")).is_some_and(|p| p.flag("fitToPage", false)) {
                    let w = e.attr_u32("fitToWidth").unwrap_or(1).min(32767) as u16;
                    let h = e.attr_u32("fitToHeight").unwrap_or(1).min(32767) as u16;
                    sheet.print.fit_to = Some((w, h));
                }
            }
            "headerFooter" => {
                let pr = &mut sheet.print;
                for (tag, slot) in [
                    ("oddHeader", &mut pr.header),
                    ("oddFooter", &mut pr.footer),
                    ("evenHeader", &mut pr.even_header),
                    ("evenFooter", &mut pr.even_footer),
                    ("firstHeader", &mut pr.first_header),
                    ("firstFooter", &mut pr.first_footer),
                ] {
                    // Excel caps each at 255 characters; a hostile file can't make them huge.
                    *slot = e.child(tag).map(|h| h.text.chars().take(255).collect()).unwrap_or_default();
                }
                pr.different_first = e.flag("differentFirst", false);
                pr.different_odd_even = e.flag("differentOddEven", false);
                pr.hf_scale_with_doc = e.flag("scaleWithDoc", true);
                pr.hf_align_with_margins = e.flag("alignWithMargins", true);
            }
            "rowBreaks" => sheet.print.row_breaks = e.kids("brk").filter_map(|b| b.attr_u32("id")).filter(|&r| r > 0 && r < MAX_ROWS).collect(),
            "colBreaks" => sheet.print.col_breaks = e.kids("brk").filter_map(|b| b.attr_u32("id")).filter(|&c| c > 0 && c < MAX_COLS).collect(),
            "sheetProtection" if e.flag("sheet", false) => {
                sheet.protection = Some(SheetProtection {
                    password: read_password(e, ""),
                    legacy_password_hash: None,
                    select_locked: !e.flag("selectLockedCells", false),
                    select_unlocked: !e.flag("selectUnlockedCells", false),
                    format_cells: !e.flag("formatCells", true),
                    format_columns: !e.flag("formatColumns", true),
                    format_rows: !e.flag("formatRows", true),
                    insert_columns: !e.flag("insertColumns", true),
                    insert_rows: !e.flag("insertRows", true),
                    delete_columns: !e.flag("deleteColumns", true),
                    delete_rows: !e.flag("deleteRows", true),
                    sort: !e.flag("sort", true),
                    autofilter: !e.flag("autoFilter", true),
                });
            }
            "drawing" => {
                if let Some(rel) = e.attr("id").and_then(|id| rels.iter().find(|r| r.id == id)) {
                    let target = rel.target.clone();
                    crate::drawing::read_drawing(cx, &target, &mut sheet)?;
                }
            }
            "tableParts" => {
                for tp in e.kids("tablePart") {
                    if let Some(rel) = tp.attr("id").and_then(|id| rels.iter().find(|r| r.id == id)) {
                        let target = rel.target.clone();
                        if let Some(x) = cx.optional_xml(&target)?
                            && let Some(mut t) = read_table(cx, &x)
                        {
                            t.id = cx.next_object_id();
                            sheet.tables.push(t);
                        }
                    }
                }
            }
            "extLst" => read_ext(cx, e, &mut sheet),
            "protectedRanges" => read_protected_ranges(e, &mut sheet),
            _ => {}
        }
    }
    // Comments are related to the sheet without an element in it.
    for rel in rels.iter().filter(|r| r.kind == "comments") {
        if let Some(x) = cx.optional_xml(&rel.target)? {
            read_comments(&x, &mut sheet);
        }
    }
    for rel in rels.iter().filter(|r| r.kind == "threadedComment") {
        if let Some(x) = cx.optional_xml(&rel.target)? {
            read_threaded(cx, &x, &mut sheet);
        }
    }
    // Which notes are displayed (`<x:Visible/>`) and where their boxes sit live in the VML shapes.
    // Best effort: the VML is only read for those, so a part that doesn't parse is skipped without
    // a warning.
    if !sheet.comments.is_empty() {
        for rel in rels.iter().filter(|r| r.kind == "vmlDrawing") {
            if let Ok(Some(x)) = cx.pkg.read_xml(&rel.target) {
                read_note_shapes(&x, &mut sheet, 0);
            }
        }
    }
    Ok(sheet)
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

fn read_views(e: &El, sheet: &mut Sheet) {
    let Some(v) = e.child("sheetView") else { return };
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
        if matches!(p.attr("state"), Some("frozen") | Some("frozenSplit")) {
            let cols = p.attr_f64("xSplit").unwrap_or(0.0).clamp(0.0, (MAX_COLS - 1) as f64) as u32;
            let rows = p.attr_f64("ySplit").unwrap_or(0.0).clamp(0.0, (MAX_ROWS - 1) as f64) as u32;
            if rows > 0 || cols > 0 {
                sheet.freeze = Some((rows, cols));
                // With frozen panes the scroll position is the scrolling pane's first cell.
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

fn read_cols(cx: &Ctx<'_>, e: &El, sheet: &mut Sheet, remaining: &mut u64) -> Result<(), IoError> {
    for c in e.kids("col") {
        let (Some(min), Some(max)) = (c.attr_u32("min"), c.attr_u32("max")) else { continue };
        if min == 0 || min > MAX_COLS || max < min {
            continue;
        }
        let max = max.min(MAX_COLS);
        let mut info = LineInfo::default();
        // A width without `customWidth` that equals the default is just the default.
        if let Some(w) = c.attr_f64("width") {
            let px = col_width_to_px(w);
            if c.flag("customWidth", false) || px != sheet.default_col_width {
                info.size = Some(px);
            }
        }
        info.hidden = c.flag("hidden", false);
        info.outline = c.attr_u32("outlineLevel").unwrap_or(0).min(7) as u8;
        info.collapsed = c.flag("collapsed", false);
        if let Some(s) = c.attr_u32("style") {
            let id = cx.style(Some(s));
            if id != gridcraft_model::StyleId::DEFAULT {
                info.style = Some(id);
            }
        }
        // A custom width equal to the default still pins the column (Standard Width leaves it alone).
        if info == LineInfo::default() {
            continue;
        }
        // Overlapping declarations repeat work even when the final map stays small.
        // Charge the clipped span before expansion, sharing the budget across <cols> blocks.
        *remaining = remaining
            .checked_sub(u64::from(max - min + 1))
            .ok_or_else(|| IoError::TooLarge(format!("sheet \"{}\": more than {MAX_COLUMN_ASSIGNMENTS} expanded column assignments", sheet.name)))?;
        for col in (min - 1)..max {
            sheet.cols.insert(col, info);
        }
    }
    Ok(())
}

fn read_row(cx: &mut Ctx<'_>, sheet: &mut Sheet, st: &mut RowState, row: &El) {
    st.rows += 1;
    let r = match row.attr_u32("r") {
        Some(r) if r >= 1 => r - 1,
        Some(_) => {
            st.dropped += 1;
            return;
        }
        None => st.next_row,
    };
    if r >= MAX_ROWS {
        st.dropped += 1;
        return;
    }
    st.next_row = r + 1;
    let mut info = LineInfo::default();
    if let Some(ht) = row.attr_f64("ht") {
        info.custom = row.flag("customHeight", false);
        // Automatic heights equal to the default are noise.
        if info.custom || (pt_to_px(ht) - sheet.default_row_height).abs() > 0.5 {
            info.size = Some(pt_to_px(ht));
        }
    }
    info.hidden = row.flag("hidden", false);
    info.outline = row.attr_u32("outlineLevel").unwrap_or(0).min(7) as u8;
    info.collapsed = row.flag("collapsed", false);
    if row.flag("customFormat", false)
        && let Some(s) = row.attr_u32("s")
    {
        info.style = Some(cx.style(Some(s)));
    }
    if info != LineInfo::default() {
        sheet.rows.insert(r, info);
    }
    let mut next_col = 0u32;
    for c in row.kids("c") {
        let pos = match c.attr("r") {
            Some(a) => match CellRef::parse(a.trim()) {
                Some(p) => p,
                None => {
                    st.dropped += 1;
                    continue;
                }
            },
            None => CellRef::new(r, next_col),
        };
        if pos.col >= MAX_COLS || pos.row >= MAX_ROWS {
            st.dropped += 1;
            continue;
        }
        next_col = pos.col + 1;
        read_cell(cx, sheet, st, c, pos);
    }
}

fn read_cell(cx: &mut Ctx<'_>, sheet: &mut Sheet, st: &mut RowState, c: &El, pos: CellRef) {
    let style = cx.style(c.attr_u32("s"));
    let v = c.child("v").map(|v| v.text.as_str());
    let value = match c.attr("t").unwrap_or("n") {
        "s" => match v.and_then(|v| v.trim().parse::<usize>().ok()) {
            Some(i) => match cx.sst.get(i) {
                Some(s) => Value::Text(s.clone()),
                None => {
                    cx.warn("some cells refer to missing shared strings");
                    Value::Empty
                }
            },
            None => Value::Empty,
        },
        "str" => v.map(|v| Value::text(xml::decode_xstring(v))).unwrap_or(Value::Empty),
        "inlineStr" => {
            c.child("is").map(|is| Value::text(xml::rich_text(is))).or_else(|| v.map(|v| Value::text(xml::decode_xstring(v)))).unwrap_or(Value::Empty)
        }
        "b" => v.map(|v| Value::Bool(matches!(v.trim(), "1" | "true" | "TRUE"))).unwrap_or(Value::Empty),
        "e" => v.map(|v| Value::Error(CellError::parse(v.trim()).unwrap_or(CellError::Value))).unwrap_or(Value::Empty),
        "d" => match v.and_then(|v| parse_iso_datetime(v.trim(), cx.date_system)) {
            Some(n) => Value::number(n),
            None => {
                if let Some(v) = v
                    && !v.trim().is_empty()
                {
                    cx.warn("some date cells could not be read");
                }
                Value::Empty
            }
        },
        _ => match v.map(str::trim).filter(|v| !v.is_empty()) {
            Some(t) => match t.parse::<f64>() {
                Ok(n) if n.is_finite() => Value::Number(if n == 0.0 { 0.0 } else { n }),
                _ => {
                    cx.warn("some numeric cells could not be read");
                    Value::Empty
                }
            },
            None => Value::Empty,
        },
    };
    let picture = c.attr("vm").and_then(|vm| {
        if vm == "0" {
            return None;
        }
        let picture = vm.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).and_then(|n| cx.cell_pictures.get(n)).cloned().flatten();
        if picture.is_none() {
            cx.warn("some cells have unsupported or missing value metadata; kept their scalar fallback (cell pictures may be missing)");
        }
        picture
    });
    if let Some(picture) = picture {
        if c.child("f").is_some() {
            cx.warn("formula-generated cell pictures were imported as static pictures; their formulas were not retained");
        }
        sheet.set_picture(pos, picture);
        sheet.set_style(pos, style);
        return;
    }
    let dynamic = c.attr("cm").is_some();
    let formula = c.child("f").and_then(|f| read_formula(cx, st, f, pos, dynamic));
    let value = if formula.is_none() && in_dynamic(st, pos) { Value::Empty } else { value };
    sheet.cells.set(pos, Cell { value, formula: formula.map(Arc::new), style });
}

fn read_formula(cx: &mut Ctx<'_>, st: &mut RowState, f: &El, pos: CellRef, dynamic: bool) -> Option<Formula> {
    // Keep the file's dynamic-array mark, so saving never turns the formula into a legacy one.
    read_formula_text(cx, st, f, pos, dynamic).map(|fm| Formula { dynamic, ..fm })
}

fn read_formula_text(cx: &mut Ctx<'_>, st: &mut RowState, f: &El, pos: CellRef, dynamic: bool) -> Option<Formula> {
    let model = from_file(f.text.trim());
    let text = model.as_str();
    match f.attr("t").unwrap_or("normal") {
        "shared" => {
            let si = f.attr_u32("si")?;
            if !text.is_empty() {
                let expr = gridcraft_formula::parse(text).ok();
                st.shared.insert(si, Shared { anchor: pos, expr, text: text.to_string() });
                Some(Formula::new(text))
            } else {
                let Some(sh) = st.shared.get(&si) else {
                    cx.warn("a shared formula refers to a missing master cell; kept the cached value");
                    return None;
                };
                Some(match &sh.expr {
                    Some(e) => {
                        let dr = pos.row as i64 - sh.anchor.row as i64;
                        let dc = pos.col as i64 - sh.anchor.col as i64;
                        Formula::from_expr(gridcraft_formula::adjust::shift_relative(e.clone(), dr, dc))
                    }
                    None => Formula::new(&sh.text),
                })
            }
        }
        "array" => {
            if text.is_empty() {
                return None;
            }
            let mut fm = Formula::new(text);
            let range = f.attr("ref").and_then(RangeRef::parse).unwrap_or(RangeRef::cell(pos));
            if !range.is_single() {
                push_dynamic(st, range);
            }
            if !dynamic {
                fm.array = Some(range);
            }
            Some(fm)
        }
        "dataTable" => {
            cx.warn("what-if data tables are not supported; kept their values");
            None
        }
        _ => {
            if text.is_empty() {
                None
            } else {
                Some(Formula::new(text))
            }
        }
    }
}

/// `2024-03-15`, `2024-03-15T10:30:00Z`, `10:30:00` → serial.
pub fn parse_iso_datetime(s: &str, sys: DateSystem) -> Option<f64> {
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
        serial = serial_from_ymd(sys, y, m, d)?;
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

pub fn parse_sqref(s: Option<&str>) -> Vec<RangeRef> {
    s.unwrap_or("").split_whitespace().filter_map(RangeRef::parse).collect()
}

/// Worksheet extensions: x14 data validations and sparklines.
fn read_ext(cx: &mut Ctx<'_>, e: &El, sheet: &mut Sheet) {
    for ext in e.kids("ext") {
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
                "conditionalFormattings" => {
                    let mut skipped = false;
                    for r in c.kids("conditionalFormatting").flat_map(|f| f.kids("cfRule")) {
                        let idx = r.attr("id").and_then(|id| cx.cf_ext_ids.iter().find(|(i, _)| i == id)).map(|(_, n)| *n);
                        match (idx.and_then(|n| sheet.cond_formats.get_mut(n)), r.child("dataBar")) {
                            (Some(CondFormat { rule: CfRule::DataBar { gradient, .. }, .. }), Some(db)) => {
                                *gradient = db.flag("gradient", true);
                                // Only the fill style is modelled; borders, lengths, axis or negative colours are lost.
                                skipped |= db.attrs.iter().any(|(k, _)| k != "gradient") || db.children.iter().any(|k| k.name != "cfvo");
                            }
                            _ => skipped = true,
                        }
                    }
                    if skipped {
                        cx.warn("some Excel 2010 conditional format extensions were skipped");
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        assert_eq!(parse_iso_datetime("1900-01-01", DateSystem::D1900), Some(1.0));
        assert_eq!(parse_iso_datetime("2024-01-01T12:00:00Z", DateSystem::D1900), Some(45292.5));
        assert_eq!(parse_iso_datetime("1904-01-02", DateSystem::D1904), Some(1.0));
        assert_eq!(parse_iso_datetime("06:00:00", DateSystem::D1900), Some(0.25));
        assert_eq!(parse_iso_datetime("garbage", DateSystem::D1900), None);
        assert_eq!(parse_iso_datetime("2024-13-01", DateSystem::D1900), None);
    }

    #[test]
    fn sqref_keeps_more_than_ten_thousand_ranges() {
        let sq = (1..=12_000).map(|r| format!("A{r}")).collect::<Vec<_>>().join(" ");
        let ranges = parse_sqref(Some(&sq));
        assert_eq!(ranges.len(), 12_000);
        assert_eq!(ranges.last().copied(), RangeRef::parse("A12000"));
    }
}
