use super::header_footer::HF_FIXED_SIDE_MARGIN;
use super::{
    Anchor, Borders, CellRef, Font, Json, MAX_COLS, MAX_PAGES, MAX_ROWS, Orientation, PX_TO_PT, PageOrder, PatternType, PrintSettings, RangeRef,
    Result, Sheet, StyleId, Visibility, Workbook, bad, bool_param, split_sheet, str_param,
};

/// Cells in a page block above which only stored cells are visited (not every grid position).
pub(super) const DENSE_BLOCK: u64 = 60_000;

/// Total conditional-format `look()` calls allowed across one sheet's print layout.
const MAX_CF_PRINT_LOOKS: usize = 60_000;

/// Modeled grid edge inset (0.48 pt, corresponding to 4 dots at 600 dpi) on each edge of the cell
/// grid, reducing usable width and height between margins by 0.96 pt.
pub(crate) const GRID_EDGE_INSET_PT: f32 = 0.48;

// ---------------------------------------------------------------- paper

/// Paper size in points, as named (see `gridcraft_model::paper`); unknown names, and the empty
/// name of a file that stated no paper, fall back to Letter.
pub fn paper_size(name: &str) -> (f32, f32) {
    gridcraft_model::paper::size_pt(name).unwrap_or((612.0, 792.0))
}

// ---------------------------------------------------------------- layout

/// The pagination of one sheet.
#[derive(Clone, Debug)]
pub struct SheetLayout {
    pub sheet: usize,
    pub settings: PrintSettings,
    pub page_w: f32,
    pub page_h: f32,
    /// Left, right, top, bottom, header, footer margins in points.
    pub margins: [f32; 6],
    /// Horizontal points per sheet column pixel (at 100% scale), modeled from Normal-font
    /// 600-dpi printer digit-width geometry.
    pub col_px_to_pt: f32,
    /// Vertical points per sheet row pixel (at 100% scale), modeled from Normal-font
    /// 600-dpi printer-to-96-dpi screen row-height ratios.
    pub row_px_to_pt: f32,
    /// Rows without `customHeight` whose content font requires a taller 96-dpi row height than
    /// stored: sorted `(row, effective_px, cumulative_delta_px_including_row)`.
    pub(crate) row_deltas: Vec<(u32, f32, f64)>,
    /// Sorted, disjoint inclusive ranges `(start, end)` of hidden columns in the sheet.
    pub(crate) hidden_col_runs: Vec<(u32, u32)>,
    /// Sorted by `(row, col)`: unwrapped text and Center Across Selection cells extending past
    /// their own column into `[start_col, end_col]`.
    pub(crate) text_overflows: Vec<TextOverflow>,
    pub scale: f32,
    pub area: RangeRef,
    pub row_bands: Vec<(u32, u32)>,
    pub col_bands: Vec<(u32, u32)>,
    /// Pages in print order: (row band, column band).
    pub pages: Vec<(usize, usize)>,
    pub title_rows: Option<(u32, u32)>,
    pub title_cols: Option<(u32, u32)>,
    /// Heading sizes in sheet pixels (0 when headings are off).
    pub heading_w: f32,
    pub heading_h: f32,
    /// Manual page breaks apply (they don't when the sheet is fitted to pages).
    pub manual_breaks: bool,
}

impl SheetLayout {
    /// Physical points per 96-dpi pixel at the page scale (borders, padding, MoveOnly/Absolute objects).
    pub(super) fn k(&self) -> f32 {
        PX_TO_PT * self.scale
    }
    /// Horizontal points per sheet column pixel at the page scale.
    pub(super) fn kx(&self) -> f32 {
        self.col_px_to_pt * self.scale
    }
    /// Vertical points per sheet row pixel at the page scale.
    pub(super) fn ky(&self) -> f32 {
        self.row_px_to_pt * self.scale
    }
    /// Effective print height of `row` in 96-dpi sheet pixels (auto-fitted for non-custom rows).
    pub(super) fn row_height(&self, sh: &Sheet, row: u32) -> f32 {
        if let Ok(i) = self.row_deltas.binary_search_by_key(&row, |e| e.0) {
            return self.row_deltas[i].1;
        }
        sh.row_height(row)
    }
    /// Effective top offset of `row` in 96-dpi sheet pixels, including auto-fitted row deltas.
    pub(super) fn row_top(&self, sh: &Sheet, row: u32) -> f64 {
        let idx = self.row_deltas.partition_point(|e| e.0 < row);
        let delta = if idx > 0 { self.row_deltas[idx - 1].2 } else { 0.0 };
        sh.row_top(row) + delta
    }
    pub(crate) fn avail(&self) -> (f32, f32) {
        let [l, r, t, b, ..] = self.margins;
        ((self.page_w - l - r - 2.0 * GRID_EDGE_INSET_PT).max(36.0), (self.page_h - t - b - 2.0 * GRID_EDGE_INSET_PT).max(36.0))
    }
    /// Left and right margins, in points, that header and footer sections line up with: the
    /// page's side margins with Align with Page Margins on, otherwise 0.7 inch on each side, at
    /// most a quarter of the page width.
    pub(crate) fn hf_sides(&self) -> (f32, f32) {
        if self.settings.hf_align_with_margins {
            (self.margins[0], self.margins[1])
        } else {
            let side = HF_FIXED_SIDE_MARGIN.min(self.page_w / 4.0);
            (side, side)
        }
    }
    /// Scale of header and footer text: the print scale with Scale with Document on, otherwise 1.
    pub(crate) fn hf_text_scale(&self) -> f32 {
        if self.settings.hf_scale_with_doc { self.scale } else { 1.0 }
    }
}

/// Options shared by the print commands.
#[derive(Clone, Debug, Default)]
pub struct PrintOptions {
    pub all_sheets: bool,
    /// Areas of the active sheet to print instead of its print area, in order (Print Selection).
    pub ranges: Vec<RangeRef>,
    pub fit_to_page: Option<bool>,
}

impl PrintOptions {
    pub(super) fn from_params(p: &Json, cmd: &str) -> Result<PrintOptions> {
        let all_sheets = match str_param(p, "sheets") {
            None | Some("active") => false,
            Some("all") | Some("workbook") => true,
            Some(other) => return Err(bad(cmd, format!("`sheets` must be active or all, not `{other}`"))),
        };
        let mut ranges = Vec::new();
        if let Some(r) = str_param(p, "range").map(str::trim).filter(|r| !r.is_empty()) {
            for part in split_areas(r) {
                ranges.push(RangeRef::parse(split_sheet(part.trim()).1.trim()).ok_or_else(|| bad(cmd, format!("not a range: `{r}`")))?);
            }
        }
        Ok(PrintOptions { all_sheets, ranges, fit_to_page: bool_param(p, "fitToPage") })
    }
}

/// Splits a multi-area reference at the commas outside quoted sheet names, so each area keeps
/// its own optional `Sheet!` prefix.
fn split_areas(r: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (i, c) in r.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            ',' if !quoted => {
                parts.push(&r[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&r[start..]);
    parts
}

/// Modeled printer column-width scale (points per model column pixel at 100% scale).
///
/// Model column widths are stored in 96-dpi pixels assuming a 7-px screen MDW (`col_px ≈ chars × 7`).
/// To approximate Windows Excel 600-dpi print pagination, stored column widths are scaled from the
/// workbook's Normal style font (`StyleId::DEFAULT`) maximum digit width modeled at 600 dpi:
/// `lfHeight = round(size_pt × 600 / 72)`, `mdw_dots = round(lfHeight × digit_advance / upm)`,
/// `mdw_pt = mdw_dots × 72 / 600`.
pub(crate) fn excel_col_px_to_pt(wb: &Workbook) -> f32 {
    let font = &wb.styles.get(StyleId::DEFAULT).font;
    let size = if font.size.is_finite() && font.size > 0.0 { font.size.clamp(1.0, 409.0) } else { 11.0 };
    let lower = font.name.trim().to_ascii_lowercase();
    let digit_ratio: f32 = match lower.as_str() {
        "calibri" | "calibri light" | "carlito" => 1038.0 / 2048.0,
        "arial" | "arial narrow" | "helvetica" | "liberation sans" | "arimo" => 1139.0 / 2048.0,
        "aptos" | "aptos narrow" => 1113.0 / 2048.0,
        "segoe ui" => 1104.0 / 2048.0,
        "times new roman" | "times" | "liberation serif" => 1024.0 / 2048.0,
        "verdana" => 1302.0 / 2048.0,
        "tahoma" => 1119.0 / 2048.0,
        "courier new" | "courier" => 1229.0 / 2048.0,
        _ => Font::styled(Font::family_for(&font.name), false, false).char_width('0') as f32 / 1000.0,
    };
    let lf_height = (size * (600.0 / 72.0)).round().max(1.0);
    let mdw_dots = (lf_height * digit_ratio).round().max(1.0);
    let mdw_pt = mdw_dots * (72.0 / 600.0);
    mdw_pt / 7.0
}

/// Modeled printer vertical row-height scale (points per 96-dpi sheet row pixel at 100% scale).
///
/// Modeled for Windows Excel 600-dpi PDF pagination using the ratio of the workbook Normal
/// font's modeled 600-dpi row height to its 96-dpi screen row height:
/// - Arial 10 pt:   103 dots (12.36 pt) / 17 px => 0.72706 pt/px (~0.9694 × 0.75)
/// - Calibri 11 pt: 121 dots (14.52 pt) / 20 px => 0.72600 pt/px (~0.9680 × 0.75)
pub(crate) fn excel_row_px_to_pt(wb: &Workbook) -> f32 {
    let font = &wb.styles.get(StyleId::DEFAULT).font;
    let size = if font.size.is_finite() && font.size > 0.0 { font.size.clamp(1.0, 409.0) } else { 11.0 };
    let lower = font.name.trim().to_ascii_lowercase();
    let (ad96, ad600, pad96, pad600): (f32, f32, f32, f32) = match lower.as_str() {
        "arial" | "arial narrow" | "helvetica" | "liberation sans" | "arimo" => (2288.0 / 2048.0, 2355.0 / 2048.0, 2.0, 8.0),
        "calibri" | "calibri light" | "carlito" => (2500.0 / 2048.0, 2500.0 / 2048.0, 2.0, 9.0),
        "segoe ui" => (2724.0 / 2048.0, 2724.0 / 2048.0, 2.0, 9.0),
        "verdana" => (2489.0 / 2048.0, 2489.0 / 2048.0, 2.0, 9.0),
        "tahoma" => (2472.0 / 2048.0, 2472.0 / 2048.0, 2.0, 9.0),
        "times new roman" | "times" | "liberation serif" => (2268.0 / 2048.0, 2268.0 / 2048.0, 2.0, 9.0),
        _ => return PX_TO_PT * (121.0 / 125.0),
    };
    let lf96 = (size * (96.0 / 72.0)).round().max(1.0);
    let px96 = ((lf96 * ad96).round() + pad96).max(1.0);
    let lf600 = (size * (600.0 / 72.0)).round().max(1.0);
    let dots600 = ((lf600 * ad600).round() + pad600).max(1.0);
    (dots600 * (72.0 / 600.0)) / px96
}

/// Windows 96-dpi single-line auto-fit row height in pixels for a cell font.
fn font_autofit_row_px(font: &gridcraft_model::Font) -> f32 {
    let size = if font.size.is_finite() && font.size > 0.0 { font.size.clamp(1.0, 409.0) } else { 11.0 };
    let lower = font.name.trim().to_ascii_lowercase();
    let ad96: f32 = match lower.as_str() {
        "segoe ui" => 2724.0 / 2048.0,
        "calibri" | "calibri light" | "carlito" => 2500.0 / 2048.0,
        "verdana" => 2489.0 / 2048.0,
        "tahoma" => 2472.0 / 2048.0,
        "arial" | "arial narrow" | "helvetica" | "liberation sans" | "arimo" => 2288.0 / 2048.0,
        "times new roman" | "times" | "liberation serif" => 2268.0 / 2048.0,
        _ => 2500.0 / 2048.0,
    };
    let lf96 = (size * (96.0 / 72.0)).round().max(1.0);
    ((lf96 * ad96).round() + 2.0).max(1.0)
}

/// Computes auto-fit row height deltas for non-custom, non-hidden rows in `0..=end_row` whose
/// content cells require a taller Windows 96-dpi single-line height than stored.
fn compute_row_deltas(wb: &Workbook, sh: &Sheet, end_row: u32) -> Vec<(u32, f32, f64)> {
    let mut max_px: std::collections::BTreeMap<u32, f32> = std::collections::BTreeMap::new();
    for (&row, info) in sh.rows.range(0..=end_row) {
        if info.hidden || info.custom {
            continue;
        }
        if let Some(sid) = info.style {
            let px = font_autofit_row_px(&wb.styles.get(sid).font);
            let e = max_px.entry(row).or_insert(0.0);
            if px > *e {
                *e = px;
            }
        }
    }
    let scan = RangeRef::new(CellRef::new(0, 0), CellRef::new(end_row, MAX_COLS - 1));
    for (pos, cell) in sh.cells.iter_range(scan) {
        if !cell.has_content() {
            continue;
        }
        if sh.rows.get(&pos.row).is_some_and(|info| info.hidden || info.custom) {
            continue;
        }
        let px = font_autofit_row_px(&wb.styles.get(sh.style_id(pos)).font);
        let e = max_px.entry(pos.row).or_insert(0.0);
        if px > *e {
            *e = px;
        }
    }
    let mut out = Vec::new();
    let mut cum = 0.0f64;
    for (row, auto_px) in max_px {
        let stored = sh.row_height(row);
        if auto_px > stored + 0.01 {
            cum += f64::from(auto_px - stored);
            out.push((row, auto_px, cum));
        }
    }
    out
}

/// True when `sid` paints something visible on paper even for an empty cell: a visible fill,
/// any border, or a cell checkbox format.
fn style_ref_is_printable(wb: &Workbook, style: &super::Style) -> bool {
    let has_fill = match style.fill.pattern {
        PatternType::None => false,
        PatternType::Solid => style.fill.fg.resolve(&wb.theme).is_some(),
        _ => true,
    };
    has_fill || style.borders != Borders::default() || style.num_fmt.as_str() == "checkbox"
}

fn style_is_printable(wb: &Workbook, sid: StyleId) -> bool {
    style_ref_is_printable(wb, wb.styles.get(sid))
}

fn cf_rule_can_paint(wb: &Workbook, rule: &gridcraft_model::CfRule) -> bool {
    match rule {
        gridcraft_model::CfRule::ColorScale { .. } | gridcraft_model::CfRule::DataBar { .. } => true,
        _ => rule.style().is_some_and(|s| style_ref_is_printable(wb, s)),
    }
}

fn cf_look_is_printable(wb: &Workbook, look: &crate::cf::CfLook) -> bool {
    look.scale_fill.is_some() || look.bar.is_some() || look.style.as_ref().is_some_and(|s| style_ref_is_printable(wb, s))
}

/// Object bounding rectangle `(x0, y0, x1, y1)` in sheet-pixel coordinates scaled to match
/// `objects::anchor_printed_rect`: `MoveOnly` and `Absolute` draw physical `PX_TO_PT` sizes
/// while rows/columns print at `(col_px_to_pt, row_px_to_pt)`, so their printed footprint in
/// grid-pixel units scales by `PX_TO_PT / px_to_pt`; width and height clamp this ratio to
/// `>= 1.0` to conservatively avoid shrinking an object's print bounds below its sheet-pixel extent.
fn anchor_extent_px(sh: &Sheet, row_top: impl Fn(u32) -> f64, a: &Anchor, col_px_to_pt: f32, row_px_to_pt: f32) -> (f64, f64, f64, f64) {
    let rx = f64::from(PX_TO_PT / col_px_to_pt.max(0.01));
    let ry = f64::from(PX_TO_PT / row_px_to_pt.max(0.01));
    let cx = sh.col_left(a.cell.col);
    let cy = row_top(a.cell.row);
    let (x, y, w, h) = match a.mode {
        gridcraft_model::AnchorMode::Absolute => (
            cx + f64::from(a.dx) * rx,
            cy + f64::from(a.dy) * ry,
            f64::from(a.width.max(0.0)) * rx.max(1.0),
            f64::from(a.height.max(0.0)) * ry.max(1.0),
        ),
        gridcraft_model::AnchorMode::MoveOnly => {
            (cx + f64::from(a.dx), cy + f64::from(a.dy), f64::from(a.width.max(0.0)) * rx.max(1.0), f64::from(a.height.max(0.0)) * ry.max(1.0))
        }
        gridcraft_model::AnchorMode::MoveAndSize => {
            (cx + f64::from(a.dx), cy + f64::from(a.dy), f64::from(a.width.max(0.0)), f64::from(a.height.max(0.0)))
        }
    };
    (x, y, x + w, y + h)
}

fn union_anchor(sh: &Sheet, r: &mut Option<RangeRef>, a: &Anchor, col_px_to_pt: f32, row_px_to_pt: f32) {
    let (x, y, x1, y1) = anchor_extent_px(sh, |row| sh.row_top(row), a, col_px_to_pt, row_px_to_pt);
    let end_x = if x1 > x { (x1 - 0.01).max(x) } else { x };
    let end_y = if y1 > y { (y1 - 0.01).max(y) } else { y };
    let end = CellRef::new(sh.row_at(end_y.max(0.0)), sh.col_at(end_x.max(0.0)));
    let ar = RangeRef::new(a.cell, end);
    *r = Some(r.map_or(ar, |u| u.union(&ar)));
}

/// Printable cell range without workbook styles (legacy public signature).
pub fn print_range(sh: &Sheet) -> Option<RangeRef> {
    if let Some(a) = sh.print.print_area {
        return Some(trim_full_used(sh.used_range(), a));
    }
    let mut r = sh.used_range();
    for c in &sh.charts {
        union_anchor(sh, &mut r, &c.anchor, PX_TO_PT, PX_TO_PT);
    }
    for i in &sh.images {
        union_anchor(sh, &mut r, &i.anchor, PX_TO_PT, PX_TO_PT);
    }
    for s in &sh.shapes {
        union_anchor(sh, &mut r, &s.anchor, PX_TO_PT, PX_TO_PT);
    }
    r
}

/// The range a sheet prints, matching Excel: its explicit print area, else the tightest range
/// covering cells with values/formulas/visible fills/visible borders, spills, cell pictures,
/// sparklines, tables, printable merges, and floating objects. Empty cells carrying only font,
/// alignment, or number-format styling do not expand the print range.
pub(crate) fn sheet_print_range(wb: &Workbook, si: usize, sh: &Sheet) -> Option<RangeRef> {
    let mut cf = crate::cf::CfCache::new();
    let mut cf_budget = MAX_CF_PRINT_LOOKS;
    let hidden_cols = hidden_col_runs(sh);
    sheet_print_range_with_cf(wb, si, sh, &hidden_cols, &mut cf, &mut cf_budget).0
}

/// An unwrapped text or Center Across Selection cell on `row` whose rendered text extends into
/// adjacent columns `[start_col, end_col]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextOverflow {
    pub(crate) row: u32,
    pub(crate) col: u32,
    pub(crate) start_col: u32,
    pub(crate) end_col: u32,
}

/// Sorted, disjoint inclusive ranges `(start, end)` of hidden columns in `sh`.
pub(super) fn hidden_col_runs(sh: &Sheet) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for (&col, info) in &sh.cols {
        if !info.hidden || col >= MAX_COLS {
            continue;
        }
        if let Some(last) = runs.last_mut()
            && col == last.1 + 1
        {
            last.1 = col;
        } else {
            runs.push((col, col));
        }
    }
    runs
}

/// First non-hidden column `>= col` below `MAX_COLS`, jumping over contiguous hidden runs.
pub(super) fn next_visible_col(runs: &[(u32, u32)], col: u32) -> Option<u32> {
    if col >= MAX_COLS {
        return None;
    }
    let idx = runs.partition_point(|&(_, end)| end < col);
    let next = match runs.get(idx) {
        Some(&(start, end)) if start <= col => end.saturating_add(1),
        _ => col,
    };
    (next < MAX_COLS).then_some(next)
}

/// Last non-hidden column `<= col`, jumping over contiguous hidden runs.
pub(super) fn prev_visible_col(runs: &[(u32, u32)], col: u32) -> Option<u32> {
    if col >= MAX_COLS {
        return None;
    }
    let idx = runs.partition_point(|&(_, end)| end < col);
    match runs.get(idx) {
        Some(&(start, _)) if start <= col => start.checked_sub(1),
        _ => Some(col),
    }
}

fn sheet_print_range_with_cf(
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    hidden_cols: &[(u32, u32)],
    cf: &mut crate::cf::CfCache,
    cf_budget: &mut usize,
) -> (Option<RangeRef>, Vec<TextOverflow>) {
    if let Some(a) = sh.print.print_area {
        let (used, overflows) = printable_cells_range(wb, si, sh, hidden_cols, cf, cf_budget);
        return (Some(trim_full_used(used, a)), overflows);
    }
    let (mut r, overflows) = printable_cells_range(wb, si, sh, hidden_cols, cf, cf_budget);
    let col_px_to_pt = excel_col_px_to_pt(wb);
    let row_px_to_pt = excel_row_px_to_pt(wb);
    for c in &sh.charts {
        union_anchor(sh, &mut r, &c.anchor, col_px_to_pt, row_px_to_pt);
    }
    for i in &sh.images {
        union_anchor(sh, &mut r, &i.anchor, col_px_to_pt, row_px_to_pt);
    }
    for s in &sh.shapes {
        union_anchor(sh, &mut r, &s.anchor, col_px_to_pt, row_px_to_pt);
    }
    (r, overflows)
}

fn printable_cells_range(
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    hidden_cols: &[(u32, u32)],
    cf: &mut crate::cf::CfCache,
    cf_budget: &mut usize,
) -> (Option<RangeRef>, Vec<TextOverflow>) {
    let mut out: Option<RangeRef> = None;
    let mut extend = |rng: RangeRef| {
        out = Some(out.map_or(rng, |u| u.union(&rng)));
    };
    for (pos, cell) in sh.cells.iter() {
        if sh.is_row_hidden(pos.row) || sh.is_col_hidden(pos.col) {
            continue;
        }
        if cell.has_content() || style_is_printable(wb, sh.style_id(pos)) {
            extend(RangeRef::cell(pos));
        }
    }
    // A row with a visible row-level fill or border paints across the sheet's used columns
    // (matching Excel for full-width header banners set via `<row s="...">`).
    if let Some(used) = sh.used_range() {
        for (&row, info) in &sh.rows {
            if !info.hidden && info.style.is_some_and(|sid| style_is_printable(wb, sid)) {
                extend(RangeRef::new(CellRef::new(row, used.start.col), CellRef::new(row, used.end.col)));
            }
        }
    }
    for sr in sh.spill_ranges.values() {
        extend(*sr);
    }
    for &pos in sh.spill.keys() {
        extend(RangeRef::cell(pos));
    }
    for &pos in sh.cell_pictures.keys() {
        extend(RangeRef::cell(pos));
    }
    for sp in &sh.sparklines {
        extend(RangeRef::cell(sp.cell));
    }
    for t in &sh.tables {
        extend(t.range);
    }
    for m in &sh.merges {
        let anchor_printable = sh.cells.get(m.start).is_some_and(|c| c.has_content())
            || style_is_printable(wb, sh.style_id(m.start))
            || style_is_printable(wb, sh.style_id(m.end));
        if anchor_printable {
            extend(*m);
        }
    }
    extend_printable_cf_cells(wb, si, sh, cf, cf_budget, &mut out);
    let overflows = if let Some(cur_out) = out {
        let (ovs, min_col, max_col) = collect_text_overflows(wb, sh, hidden_cols, cur_out.start.col, cur_out.end.col);
        let mut updated = cur_out;
        if min_col < updated.start.col {
            updated = updated.union(&RangeRef::cell(CellRef::new(updated.start.row, min_col)));
        }
        if max_col > updated.end.col {
            updated = updated.union(&RangeRef::cell(CellRef::new(updated.start.row, max_col)));
        }
        out = Some(updated);
        ovs
    } else {
        Vec::new()
    };
    (out, overflows)
}

/// Includes cells whose conditional format produces a visible fill, border, colour scale, or data bar.
fn extend_printable_cf_cells(wb: &Workbook, si: usize, sh: &Sheet, cf: &mut crate::cf::CfCache, look_budget: &mut usize, out: &mut Option<RangeRef>) {
    if sh.cond_formats.is_empty() {
        return;
    }
    let used = sh.used_range();
    let mut seen: std::collections::HashSet<RangeRef> = std::collections::HashSet::new();
    for cond in &sh.cond_formats {
        if !cf_rule_can_paint(wb, &cond.rule) {
            continue;
        }
        for &rng in &cond.ranges {
            if !seen.insert(rng) {
                continue;
            }
            if out.is_some_and(|u| u.contains(rng.start) && u.contains(rng.end)) {
                continue;
            }
            if rng.count() <= DENSE_BLOCK {
                if *look_budget == 0 {
                    *out = Some(out.map_or(rng, |u| u.union(&rng)));
                    continue;
                }
                let mut exhausted = false;
                for pos in rng.iter() {
                    if out.is_some_and(|u| u.contains(pos)) || sh.cells.get(pos).is_some_and(|cell| cell.has_content()) {
                        continue;
                    }
                    if *look_budget == 0 {
                        exhausted = true;
                        break;
                    }
                    *look_budget -= 1;
                    if cf.look(wb, si, pos).is_some_and(|l| cf_look_is_printable(wb, &l)) {
                        let r = RangeRef::cell(pos);
                        *out = Some(out.map_or(r, |u| u.union(&r)));
                    }
                }
                if exhausted {
                    *out = Some(out.map_or(rng, |u| u.union(&rng)));
                }
            } else {
                let Some(sub) = used.and_then(|u| rng.intersection(&u)) else { continue };
                if out.is_some_and(|u| u.contains(sub.start) && u.contains(sub.end)) {
                    continue;
                }
                if *look_budget == 0 {
                    *out = Some(out.map_or(sub, |u| u.union(&sub)));
                    continue;
                }
                let mut exhausted = false;
                for (pos, cell) in sh.cells.iter_range(sub) {
                    if out.is_some_and(|u| u.contains(pos)) || cell.has_content() {
                        continue;
                    }
                    if *look_budget == 0 {
                        exhausted = true;
                        break;
                    }
                    *look_budget -= 1;
                    if cf.look(wb, si, pos).is_some_and(|l| cf_look_is_printable(wb, &l)) {
                        let r = RangeRef::cell(pos);
                        *out = Some(out.map_or(r, |u| u.union(&r)));
                    }
                }
                if exhausted {
                    *out = Some(out.map_or(sub, |u| u.union(&sub)));
                }
            }
        }
    }
}

/// Collects overflow spans of unwrapped text and Center Across Selection cells into empty
/// neighbours and returns `(overflows, min_col, max_col)` with the leftmost and rightmost columns
/// reached beyond `initial_min_col..=initial_max_col`.
fn collect_text_overflows(
    wb: &Workbook,
    sh: &Sheet,
    hidden_cols: &[(u32, u32)],
    initial_min_col: u32,
    initial_max_col: u32,
) -> (Vec<TextOverflow>, u32, u32) {
    const MAX_INDEXED_MERGE_ROWS: u32 = 64;
    let mut merges_by_row: std::collections::HashMap<u32, Vec<(u32, u32)>> = std::collections::HashMap::new();
    let mut tall_merges: Vec<RangeRef> = Vec::new();
    for &m in &sh.merges {
        if m.end.row.saturating_sub(m.start.row) < MAX_INDEXED_MERGE_ROWS {
            for r in m.start.row..=m.end.row {
                merges_by_row.entry(r).or_default().push((m.start.col, m.end.col));
            }
        } else {
            tall_merges.push(m);
        }
    }
    for spans in merges_by_row.values_mut() {
        spans.sort_unstable_by_key(|&(c0, _)| c0);
    }
    let col_in_row_merges = |row_merges: &[(u32, u32)], row: u32, col: u32| -> bool {
        let idx = row_merges.partition_point(|&(_, c1)| c1 < col);
        if row_merges.get(idx).is_some_and(|&(c0, _)| c0 <= col) {
            return true;
        }
        !tall_merges.is_empty() && tall_merges.iter().any(|m| m.contains(CellRef::new(row, col)))
    };
    let col_kx = excel_col_px_to_pt(wb);
    let pad = 2.0 * PX_TO_PT;
    let mut overflows = Vec::new();
    let mut min_col = initial_min_col;
    let mut max_col = initial_max_col;
    for (pos, cell) in sh.cells.iter() {
        if sh.is_row_hidden(pos.row) || sh.is_col_hidden(pos.col) {
            continue;
        }
        let st = wb.styles.get(sh.style_id(pos));
        if st.align.h == super::HAlign::CenterAcross {
            if !cell.has_content() || (!sh.show_formulas && cell.value.is_empty()) || st.align.wrap || st.align.rotation != 0 {
                continue;
            }
            let row_merges = merges_by_row.get(&pos.row).map(Vec::as_slice).unwrap_or(&[]);
            if col_in_row_merges(row_merges, pos.row, pos.col) {
                continue;
            }
            let mut ext = 0.0f32;
            let mut col = pos.col + 1;
            let mut reached = pos.col;
            while ext < 20_000.0 {
                let Some(vcol) = next_visible_col(hidden_cols, col) else {
                    break;
                };
                let nc = CellRef::new(pos.row, vcol);
                if sh.value_ref(nc).is_some_and(|v| !v.is_empty())
                    || col_in_row_merges(row_merges, pos.row, vcol)
                    || wb.styles.get(sh.style_id(nc)).align.h != super::HAlign::CenterAcross
                {
                    break;
                }
                reached = vcol;
                ext += sh.col_width(vcol);
                col = vcol + 1;
            }
            if reached > pos.col {
                overflows.push(TextOverflow { row: pos.row, col: pos.col, start_col: pos.col, end_col: reached });
                max_col = max_col.max(reached);
                continue;
            }
        }
        if !matches!(&cell.value, gridcraft_core::Value::Text(_)) && !sh.show_formulas {
            continue;
        }
        if st.align.wrap || st.align.shrink || st.align.rotation != 0 || st.align.h == super::HAlign::Fill {
            continue;
        }
        let cell_w = sh.col_width(pos.col) * col_kx;
        let text = if sh.show_formulas {
            crate::display::cell_text(wb, sh, pos)
        } else {
            crate::display::value_text_as(wb, &cell.value, st.num_fmt.as_str())
        };
        let text = text.replace(['\n', '\r'], " ");
        let size = st.font.size.clamp(1.0, 409.0);
        let indent = st.align.indent as f32 * 9.0 * PX_TO_PT;
        if text.is_empty() || (text.len() as f32) * size * 0.85 + 2.0 * pad + indent <= cell_w {
            continue;
        }
        let row_merges = merges_by_row.get(&pos.row).map(Vec::as_slice).unwrap_or(&[]);
        if col_in_row_merges(row_merges, pos.row, pos.col) {
            continue;
        }
        let direction = super::text::text_direction(st.align.reading_order.unwrap_or_default()).resolve(&text);
        let h_align = super::text::text_align(st.align.h, &cell.value, direction, sh.right_to_left);
        let font = Font::styled(Font::family_for(&st.font.name), st.font.bold, st.font.italic);
        let excess = gridcraft_pdf::text_width_with_direction(font, size, &text, direction) + 2.0 * pad + indent - cell_w;
        if excess <= 0.5 {
            continue;
        }
        let grow_right = |mut rem: f32| -> u32 {
            let mut col = pos.col + 1;
            let mut reached = pos.col;
            while rem > 0.5 {
                let Some(vcol) = next_visible_col(hidden_cols, col) else {
                    break;
                };
                let nc = CellRef::new(pos.row, vcol);
                if sh.value_ref(nc).is_some_and(|v| !v.is_empty()) || col_in_row_merges(row_merges, pos.row, vcol) {
                    break;
                }
                reached = vcol;
                rem -= sh.col_width(vcol) * col_kx;
                col = vcol + 1;
            }
            reached
        };
        let grow_left = |mut rem: f32| -> u32 {
            let mut col = pos.col.checked_sub(1);
            let mut reached = pos.col;
            while rem > 0.5 {
                let Some(raw_col) = col else {
                    break;
                };
                let Some(vcol) = prev_visible_col(hidden_cols, raw_col) else {
                    break;
                };
                let nc = CellRef::new(pos.row, vcol);
                if sh.value_ref(nc).is_some_and(|v| !v.is_empty()) || col_in_row_merges(row_merges, pos.row, vcol) {
                    break;
                }
                reached = vcol;
                rem -= sh.col_width(vcol) * col_kx;
                col = vcol.checked_sub(1);
            }
            reached
        };
        let (start_col, end_col) = match h_align {
            super::HAlign::Right => (grow_left(excess), pos.col),
            super::HAlign::Center => (grow_left(excess / 2.0), grow_right(excess / 2.0)),
            _ => (pos.col, grow_right(excess)),
        };
        if start_col < pos.col || end_col > pos.col {
            overflows.push(TextOverflow { row: pos.row, col: pos.col, start_col, end_col });
            min_col = min_col.min(start_col);
            max_col = max_col.max(end_col);
        }
    }
    overflows.sort_unstable_by_key(|o| (o.row, o.col));
    (overflows, min_col, max_col)
}

/// True when page `(rband, cband)` contains at least one printable cell, visible style, merge,
/// table, conditional format, overflowing text span, or floating object. Used to trim completely
/// blank trailing pages.
#[allow(clippy::too_many_arguments)]
fn page_has_content(
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    lay: Option<&SheetLayout>,
    rband: (u32, u32),
    cband: (u32, u32),
    overflows: &[TextOverflow],
    cf: &mut crate::cf::CfCache,
    look_budget: &mut usize,
) -> bool {
    if overflows.iter().any(|o| o.row >= rband.0 && o.row <= rband.1 && o.end_col >= cband.0 && o.start_col <= cband.1) {
        return true;
    }
    let block = RangeRef::new(CellRef::new(rband.0, cband.0), CellRef::new(rband.1, cband.1));
    for (pos, cell) in sh.cells.iter_range(block) {
        if sh.is_row_hidden(pos.row) || sh.is_col_hidden(pos.col) {
            continue;
        }
        if cell.has_content() || style_is_printable(wb, sh.style_id(pos)) {
            return true;
        }
    }
    for (_, info) in sh.rows.range(rband.0..=rband.1) {
        if !info.hidden && info.style.is_some_and(|sid| style_is_printable(wb, sid)) {
            return true;
        }
    }
    if sh.spill.range(block.start..=block.end).any(|(c, _)| block.contains(*c)) {
        return true;
    }
    if sh.cell_pictures.range(block.start..=block.end).any(|(c, _)| block.contains(*c)) {
        return true;
    }
    if sh.sparklines.iter().any(|sp| block.contains(sp.cell)) {
        return true;
    }
    if sh.tables.iter().any(|t| t.range.intersects(&block)) {
        return true;
    }
    if sh.merges.iter().any(|m| {
        m.intersects(&block)
            && (sh.cells.get(m.start).is_some_and(|c| c.has_content())
                || style_is_printable(wb, sh.style_id(m.start))
                || style_is_printable(wb, sh.style_id(m.end)))
    }) {
        return true;
    }
    if !sh.cond_formats.is_empty() {
        let mut seen: std::collections::HashSet<RangeRef> = std::collections::HashSet::new();
        for cond in &sh.cond_formats {
            if !cf_rule_can_paint(wb, &cond.rule) {
                continue;
            }
            for &rng in &cond.ranges {
                let Some(sub) = rng.intersection(&block) else { continue };
                if !seen.insert(sub) {
                    continue;
                }
                if sub.count() <= DENSE_BLOCK {
                    for pos in sub.iter() {
                        if *look_budget == 0 {
                            return true;
                        }
                        *look_budget -= 1;
                        if cf.look(wb, si, pos).is_some_and(|l| cf_look_is_printable(wb, &l)) {
                            return true;
                        }
                    }
                } else {
                    for (pos, _) in sh.cells.iter_range(sub) {
                        if *look_budget == 0 {
                            return true;
                        }
                        *look_budget -= 1;
                        if cf.look(wb, si, pos).is_some_and(|l| cf_look_is_printable(wb, &l)) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    let (col_px_to_pt, row_px_to_pt) = lay.map_or_else(|| (excel_col_px_to_pt(wb), excel_row_px_to_pt(wb)), |l| (l.col_px_to_pt, l.row_px_to_pt));
    let row_top = |r: u32| lay.map_or_else(|| sh.row_top(r), |l| l.row_top(sh, r));
    let (sx0, sy0) = (sh.col_left(block.start.col), row_top(block.start.row));
    let (sx1, sy1) = (sh.col_left(block.end.col.saturating_add(1)), row_top(block.end.row.saturating_add(1)));
    let hits_anchor = |a: &Anchor| {
        if a.width <= 0.0 || a.height <= 0.0 {
            return false;
        }
        let (x0, y0, x1, y1) = anchor_extent_px(sh, row_top, a, col_px_to_pt, row_px_to_pt);
        x0 < sx1 && x1 > sx0 && y0 < sy1 && y1 > sy0
    };
    sh.charts.iter().any(|c| hits_anchor(&c.anchor))
        || sh.images.iter().any(|i| hits_anchor(&i.anchor))
        || sh.shapes.iter().any(|s| hits_anchor(&s.anchor))
}

/// Whole rows/columns are trimmed to the printable/used range.
fn trim_full_used(used: Option<RangeRef>, r: RangeRef) -> RangeRef {
    if !(r.is_full_cols() || r.is_full_rows()) {
        return r;
    }
    match used {
        Some(u) => r.intersection(&RangeRef::new(CellRef::new(0, 0), u.end)).unwrap_or(RangeRef::cell(r.start)),
        None => RangeRef::cell(r.start),
    }
}

/// Splits `start..=end` into bands whose sizes fit `avail(band_start)`; `breaks` force a new
/// band before a line. Stops after `cap` bands.
pub(super) fn bands(start: u32, end: u32, size: impl Fn(u32) -> f32, avail: impl Fn(u32) -> f32, breaks: &[u32], cap: usize) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut cur = start;
    let mut used = 0.0f32;
    let mut room = avail(start);
    let mut line = start;
    loop {
        let sz = size(line);
        let manual = line > cur && breaks.binary_search(&line).is_ok();
        if line > cur && (manual || used + sz > room + 0.01) {
            out.push((cur, line - 1));
            if out.len() >= cap {
                return out;
            }
            cur = line;
            used = 0.0;
            room = avail(line);
        }
        used += sz;
        if line >= end {
            break;
        }
        line += 1;
    }
    out.push((cur, end));
    out
}

fn title_span(t: Option<(u32, u32)>, max: u32) -> Option<(u32, u32)> {
    let (a, b) = t?;
    let (a, b) = (a.min(b), a.max(b));
    if a >= max {
        return None;
    }
    // Repeated titles are capped (Excel needs them to fit on a page anyway).
    Some((a, b.min(max - 1).min(a.saturating_add(499))))
}

/// Paginates one sheet.
pub fn layout_sheet(wb: &Workbook, si: usize, opts: &PrintOptions, range_override: Option<RangeRef>) -> Option<SheetLayout> {
    let sh = wb.sheet(si)?;
    let st = sh.print.clone();
    let (mut pw, mut ph) = paper_size(&st.paper);
    if st.orientation == Orientation::Landscape {
        std::mem::swap(&mut pw, &mut ph);
    }
    let inch = |i: usize| st.margins.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.5).clamp(0.0, 20.0) * 72.0;
    let mut margins = [inch(0), inch(1), inch(2), inch(3), inch(4), inch(5)];
    // Keep at least an inch of printable space.
    for (a, b, total) in [(0usize, 1usize, pw), (2, 3, ph)] {
        let sum = margins[a] + margins[b];
        if total - sum < 72.0 {
            let f = ((total - 72.0).max(0.0)) / sum.max(1.0);
            margins[a] *= f;
            margins[b] *= f;
        }
    }
    let mut cf = crate::cf::CfCache::new();
    let mut cf_budget = MAX_CF_PRINT_LOOKS;
    let hidden_col_runs = hidden_col_runs(sh);
    let (area, overflows) = match range_override {
        Some(r) => {
            let (used, ov) = printable_cells_range(wb, si, sh, &hidden_col_runs, &mut cf, &mut cf_budget);
            (trim_full_used(used, r), ov)
        }
        None => {
            let (used, ov) = sheet_print_range_with_cf(wb, si, sh, &hidden_col_runs, &mut cf, &mut cf_budget);
            (used.unwrap_or(RangeRef::cell(CellRef::new(0, 0))), ov)
        }
    };
    // A print area from a file or JSON can name coordinates past the grid; pagination walks every
    // line of the area (zero-height rows never fill a band), so it is held to the grid first.
    let area = RangeRef::new(
        CellRef::new(area.start.row.min(MAX_ROWS - 1), area.start.col.min(MAX_COLS - 1)),
        CellRef::new(area.end.row.min(MAX_ROWS - 1), area.end.col.min(MAX_COLS - 1)),
    );
    let title_rows = title_span(st.title_rows, MAX_ROWS);
    let title_cols = title_span(st.title_cols, MAX_COLS);
    let digits = format!("{}", area.end.row + 1).len() as f32;
    let (heading_w, heading_h) = if st.headings { (10.0 + 7.0 * digits, sh.default_row_height.clamp(10.0, 60.0)) } else { (0.0, 0.0) };
    let fit = match opts.fit_to_page {
        Some(true) => Some((1u16, 1u16)),
        Some(false) => None,
        None => st.fit_to.filter(|(w, h)| *w > 0 || *h > 0),
    };
    let col_px_to_pt = excel_col_px_to_pt(wb);
    let row_px_to_pt = excel_row_px_to_pt(wb);
    let delta_end = title_rows.map_or(area.end.row, |(_, b)| area.end.row.max(b));
    let row_deltas = compute_row_deltas(wb, sh, delta_end);
    let mut lay = SheetLayout {
        sheet: si,
        settings: st.clone(),
        page_w: pw,
        page_h: ph,
        margins,
        col_px_to_pt,
        row_px_to_pt,
        row_deltas,
        hidden_col_runs,
        text_overflows: overflows,
        scale: (st.scale.clamp(10, 400) as f32) / 100.0,
        area,
        row_bands: vec![],
        col_bands: vec![],
        pages: vec![],
        title_rows,
        title_cols,
        heading_w,
        heading_h,
        manual_breaks: fit.is_none(),
    };
    let can_trim = !lay.settings.gridlines
        && !lay.settings.headings
        && !lay.settings.has_header_footer()
        && sh.print.print_area.is_none()
        && range_override.is_none()
        && lay.title_rows.is_none()
        && lay.title_cols.is_none();
    let mut paginate = |lay: &mut SheetLayout, manual: bool| {
        lay.manual_breaks = manual;
        (lay.row_bands, lay.col_bands) = split_bands(sh, lay, MAX_PAGES);
        lay.pages = page_sequence(lay.row_bands.len(), lay.col_bands.len(), lay.settings.page_order, MAX_PAGES);
        let truncated = lay.row_bands.len().saturating_mul(lay.col_bands.len()) > lay.pages.len();
        if !truncated && can_trim {
            while lay.pages.len() > 1 {
                let Some(&(ri, ci)) = lay.pages.last() else { break };
                let (Some(&rb), Some(&cb)) = (lay.row_bands.get(ri), lay.col_bands.get(ci)) else { break };
                if page_has_content(wb, si, sh, Some(lay), rb, cb, &lay.text_overflows, &mut cf, &mut cf_budget) {
                    break;
                }
                lay.pages.pop();
            }
        }
    };
    match fit {
        Some((fw, fh)) => {
            let total_w: f32 = (area.start.col..=area.end.col).map(|c| sh.col_width(c)).sum::<f32>() + heading_w;
            let total_h: f32 = if area.height() > 200_000 {
                f32::INFINITY
            } else {
                (area.start.row..=area.end.row).map(|r| lay.row_height(sh, r)).sum::<f32>() + heading_h
            };
            let (aw, ah) = lay.avail();
            let sw = if fw > 0 { aw * fw as f32 / (total_w * col_px_to_pt).max(1.0) } else { f32::INFINITY };
            let sh_ = if fh > 0 { ah * fh as f32 / (total_h * row_px_to_pt).max(1.0) } else { f32::INFINITY };
            lay.scale = sw.min(sh_).min(1.0).clamp(0.1, 1.0);
            for _ in 0..40 {
                paginate(&mut lay, false);
                let ok_w = fw == 0 || lay.col_bands.len() <= fw as usize;
                let ok_h = fh == 0 || lay.row_bands.len() <= fh as usize;
                if (ok_w && ok_h) || lay.scale <= 0.1 {
                    break;
                }
                lay.scale = (lay.scale * 0.97).max(0.1);
            }
        }
        None => paginate(&mut lay, true),
    }
    Some(lay)
}

/// The pages of a sheet `rows` pages tall and `cols` pages wide as (row band, column band), in
/// print order, at most `cap` of them: down each column of pages, then over to the next (Excel's
/// default), or over each row of pages, then down to the next.
pub(crate) fn page_sequence(rows: usize, cols: usize, order: PageOrder, cap: usize) -> Vec<(usize, usize)> {
    let (outer, inner) = match order {
        PageOrder::DownThenOver => (cols, rows),
        PageOrder::OverThenDown => (rows, cols),
    };
    (0..outer)
        .flat_map(|o| {
            (0..inner).map(move |i| match order {
                PageOrder::DownThenOver => (i, o),
                PageOrder::OverThenDown => (o, i),
            })
        })
        .take(cap)
        .collect()
}

/// Inclusive-start/end index spans of the pages along one axis.
pub(crate) type Bands = Vec<(u32, u32)>;

/// Splits a layout's area into row and column bands that fit its pages (at most `cap` of each),
/// honouring manual breaks (when `lay.manual_breaks`), print headings and repeated titles. The
/// one pagination rule shared by printing, PDF export, Page Break Preview and Page Layout view.
pub(crate) fn split_bands(sh: &Sheet, lay: &SheetLayout, cap: usize) -> (Bands, Bands) {
    let kx = lay.kx();
    let ky = lay.ky();
    let (aw, ah) = lay.avail();
    let (aw, ah) = (aw / kx, ah / ky);
    let st = &lay.settings;
    let mut rb: Vec<u32> = if lay.manual_breaks { st.row_breaks.clone() } else { vec![] };
    let mut cb: Vec<u32> = if lay.manual_breaks { st.col_breaks.clone() } else { vec![] };
    rb.sort_unstable();
    cb.sort_unstable();
    let (title_rows, title_cols) = (lay.title_rows, lay.title_cols);
    let title_h = |start: u32| -> f32 {
        match title_rows {
            Some((a, b)) if a < start => (a..=b.min(start - 1)).map(|r| lay.row_height(sh, r)).sum(),
            _ => 0.0,
        }
    };
    let title_w = |start: u32| -> f32 {
        match title_cols {
            Some((a, b)) if a < start => (a..=b.min(start - 1)).map(|c| sh.col_width(c)).sum(),
            _ => 0.0,
        }
    };
    let area = lay.area;
    let mut rows = bands(area.start.row, area.end.row, |r| lay.row_height(sh, r), |start| (ah - lay.heading_h - title_h(start)).max(1.0), &rb, cap);
    let mut cols = bands(area.start.col, area.end.col, |c| sh.col_width(c), |start| (aw - lay.heading_w - title_w(start)).max(1.0), &cb, cap);
    while rows.len() > 1 && rows.last().is_some_and(|&(a, b)| (a..=b).all(|r| lay.row_height(sh, r) <= 0.0)) {
        rows.pop();
    }
    while cols.len() > 1 && cols.last().is_some_and(|&(a, b)| (a..=b).all(|c| sh.col_width(c) <= 0.0)) {
        cols.pop();
    }
    (rows, cols)
}

/// Lays out every sheet a print command covers.
pub fn layouts(wb: &Workbook, opts: &PrintOptions) -> Vec<SheetLayout> {
    let mut out = Vec::new();
    let mut pages = 0usize;
    let sheets: Vec<usize> = if opts.all_sheets {
        (0..wb.sheets.len()).filter(|i| wb.sheet(*i).is_some_and(|s| s.visibility == Visibility::Visible)).collect()
    } else {
        vec![wb.active_sheet()]
    };
    for si in sheets {
        if pages >= MAX_PAGES {
            break;
        }
        let areas: Vec<Option<RangeRef>> =
            if si == wb.active_sheet() && !opts.ranges.is_empty() { opts.ranges.iter().copied().map(Some).collect() } else { vec![None] };
        if opts.all_sheets && areas == [None] && wb.sheet(si).is_some_and(|s| sheet_print_range(wb, si, s).is_none()) {
            continue;
        }
        for ovr in areas {
            if pages >= MAX_PAGES {
                break;
            }
            if let Some(mut l) = layout_sheet(wb, si, opts, ovr) {
                l.pages.truncate(MAX_PAGES - pages);
                pages += l.pages.len();
                out.push(l);
            }
        }
    }
    if out.is_empty() && opts.all_sheets {
        out.extend(layout_sheet(wb, wb.active_sheet(), opts, None));
    }
    out
}

/// Segments of a page along one axis: repeated titles (before the band) then the band.
pub(super) fn segments(titles: Option<(u32, u32)>, band: (u32, u32)) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    if let Some((a, b)) = titles
        && a < band.0
    {
        v.push((a, b.min(band.0 - 1)));
    }
    v.push(band);
    v
}

pub(super) fn seg_size(lay: &SheetLayout, sh: &Sheet, seg: (u32, u32), rows: bool) -> f32 {
    if rows { (seg.0..=seg.1).map(|r| lay.row_height(sh, r)).sum() } else { (seg.0..=seg.1).map(|c| sh.col_width(c)).sum() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_print_area_past_the_grid_is_held_to_the_grid() {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        sh.default_row_height = 0.0;
        sh.print.print_area = Some(RangeRef { start: CellRef::new(0, 0), end: CellRef::new(u32::MAX, u32::MAX) });
        let lay = layout_sheet(&wb, 0, &PrintOptions::default(), None).unwrap();
        assert_eq!((lay.area.end.row, lay.area.end.col), (MAX_ROWS - 1, MAX_COLS - 1));
        assert!(lay.pages.len() <= MAX_PAGES);
    }

    #[test]
    fn repeated_cf_ranges_finish_print_layout_within_budget() {
        let mut wb = Workbook::new();
        let fill = gridcraft_model::Style {
            fill: gridcraft_model::Fill { pattern: PatternType::Solid, fg: gridcraft_model::Color::Rgb(0xFFCC00), bg: gridcraft_model::Color::Auto },
            ..Default::default()
        };
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::Number(1.0));
        let block = RangeRef::new(CellRef::new(0, 0), CellRef::new(1999, 24)); // A1:Y2000 (50,000 cells)
        sh.cond_formats.push(gridcraft_model::CondFormat {
            ranges: vec![block; 2_000],
            rule: gridcraft_model::CfRule::CellIs { op: gridcraft_model::CfOperator::Equal, a: "999".into(), b: None, style: Box::new(fill) },
            priority: 1,
            stop_if_true: false,
        });
        let mut cf = crate::cf::CfCache::new();
        let mut budget = MAX_CF_PRINT_LOOKS;
        let sh_ref = wb.sheet(0).unwrap();
        let mut out = None;
        extend_printable_cf_cells(&wb, 0, sh_ref, &mut cf, &mut budget, &mut out);
        assert_eq!(
            budget,
            MAX_CF_PRINT_LOOKS - 49_999,
            "2,000 repeated A1:Y2000 ranges must deduplicate to one pass and skip populated A1 before spending budget"
        );

        let lay = layout_sheet(&wb, 0, &PrintOptions::default(), None).unwrap();
        let pdf = super::super::render_pdf(&wb, &[lay], "test.xlsx", "").unwrap();
        assert!(pdf.bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn hidden_styled_rows_do_not_expand_print_range_or_keep_blank_pages() {
        let mut wb = Workbook::new();
        let fill = wb.styles.intern(gridcraft_model::Style {
            fill: gridcraft_model::Fill { pattern: PatternType::Solid, fg: gridcraft_model::Color::Rgb(0xEEEEEE), bg: gridcraft_model::Color::Auto },
            ..Default::default()
        });
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::text("Visible"));
        sh.rows.insert(150, gridcraft_model::LineInfo { size: None, hidden: true, style: Some(fill), ..Default::default() });
        let sh = wb.sheet(0).unwrap();
        let range = sheet_print_range(&wb, 0, sh).unwrap();
        assert_eq!(range.end.row, 0, "hidden row with row-level fill must not expand print range");
        let mut cf = crate::cf::CfCache::new();
        let mut budget = MAX_CF_PRINT_LOOKS;
        assert!(!page_has_content(&wb, 0, sh, None, (100, 160), (0, 5), &[], &mut cf, &mut budget));
    }

    #[test]
    fn cf_budget_exhaustion_conservatively_retains_cf_only_trailing_page() {
        let mut wb = Workbook::new();
        let fill = gridcraft_model::Style {
            fill: gridcraft_model::Fill { pattern: PatternType::Solid, fg: gridcraft_model::Color::Rgb(0xFFCC00), bg: gridcraft_model::Color::Auto },
            ..Default::default()
        };
        let sh = wb.sheet_mut(0).unwrap();
        // Sparse L-shaped sheet -> 2x2 bands with trailing page (1, 1) holding no cell values.
        for r in 0..60 {
            sh.set_value(CellRef::new(r, 0), gridcraft_core::Value::Number(r as f64));
        }
        sh.set_value(CellRef::new(0, 11), gridcraft_core::Value::text("Wide header"));
        // Rule 1: 59,980 non-matching cells outside A1:L60 leave only 20 CF looks in the sheet budget.
        sh.cond_formats.push(gridcraft_model::CondFormat {
            ranges: vec![RangeRef::new(CellRef::new(60, 0), CellRef::new(3058, 19))],
            rule: gridcraft_model::CfRule::CellIs { op: gridcraft_model::CfOperator::Equal, a: "999".into(), b: None, style: Box::new(fill.clone()) },
            priority: 1,
            stop_if_true: false,
        });
        // Rule 2: CF range on trailing page (1, 1) with >20 cells where only the last cell matches.
        sh.cond_formats.push(gridcraft_model::CondFormat {
            ranges: vec![RangeRef::new(CellRef::new(45, 9), CellRef::new(59, 11))],
            rule: gridcraft_model::CfRule::Expression { formula: "AND(ROW()=60,COLUMN()=12)".into(), style: Box::new(fill) },
            priority: 2,
            stop_if_true: false,
        });
        let lay = layout_sheet(&wb, 0, &PrintOptions::default(), None).unwrap();
        assert_eq!(lay.row_bands.len(), 2);
        assert_eq!(lay.col_bands.len(), 2);
        assert_eq!(lay.pages.len(), 4, "exhausting CF look budget must conservatively retain CF-only trailing page (1, 1)");

        // Direct check: exhausting the budget both mid-scan and at budget == 0 returns true.
        let sh_ref = wb.sheet(0).unwrap();
        let mut cf = crate::cf::CfCache::new();
        let mut small_budget = 5;
        assert!(page_has_content(&wb, 0, sh_ref, Some(&lay), lay.row_bands[1], lay.col_bands[1], &[], &mut cf, &mut small_budget));
        assert_eq!(small_budget, 0);
        let mut zero_budget = 0;
        assert!(page_has_content(&wb, 0, sh_ref, Some(&lay), lay.row_bands[1], lay.col_bands[1], &[], &mut cf, &mut zero_budget));
    }

    #[test]
    fn cf_budget_exhaustion_preserves_unscanned_cf_ranges_in_print_area() {
        let mut wb = Workbook::new();
        let fill = gridcraft_model::Style {
            fill: gridcraft_model::Fill { pattern: PatternType::Solid, fg: gridcraft_model::Color::Rgb(0xFFCC00), bg: gridcraft_model::Color::Auto },
            ..Default::default()
        };
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::text("Top"));
        // Rule 1: 60,000 non-matching lookups (A100:T3099) exhaust the entire sheet CF look budget.
        sh.cond_formats.push(gridcraft_model::CondFormat {
            ranges: vec![RangeRef::new(CellRef::new(99, 0), CellRef::new(3098, 19))],
            rule: gridcraft_model::CfRule::CellIs { op: gridcraft_model::CfOperator::Equal, a: "999".into(), b: None, style: Box::new(fill.clone()) },
            priority: 1,
            stop_if_true: false,
        });
        // Rule 2: Blanks rule filling L60 (row 59, col 11), encountered after budget reaches 0.
        let l60 = CellRef::new(59, 11);
        sh.cond_formats.push(gridcraft_model::CondFormat {
            ranges: vec![RangeRef::cell(l60)],
            rule: gridcraft_model::CfRule::Blanks { style: Box::new(fill) },
            priority: 2,
            stop_if_true: false,
        });
        let sh_ref = wb.sheet(0).unwrap();
        let range = sheet_print_range(&wb, 0, sh_ref).unwrap();
        assert!(range.contains(l60), "sheet_print_range {range:?} must conservatively cover unscanned CF cell L60");
        let lay = layout_sheet(&wb, 0, &PrintOptions::default(), None).unwrap();
        assert!(lay.area.contains(l60), "lay.area {:?} must conservatively cover unscanned CF cell L60", lay.area);
    }

    #[test]
    fn move_only_and_absolute_anchors_are_not_clipped_at_bottom_of_print_range() {
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::text("A"));
        sh.shapes.push(gridcraft_model::Shape {
            id: 1,
            kind: gridcraft_model::ShapeKind::Rectangle,
            anchor: Anchor { cell: CellRef::new(0, 0), dx: 0.0, dy: 0.0, width: 80.0, height: 298.0, mode: gridcraft_model::AnchorMode::MoveOnly },
            fill: gridcraft_model::Color::Rgb(0x336699),
            line: gridcraft_model::Color::Auto,
            text: String::new(),
            format: gridcraft_model::ShapeFormat::default(),
            rotation: 0.0,
            math: gridcraft_model::EquationCache::default(),
        });
        let lay = layout_sheet(&wb, 0, &PrintOptions::default(), None).unwrap();
        let sh_ref = wb.sheet(0).unwrap();
        let area_bottom_pt = lay.row_top(sh_ref, lay.area.end.row + 1) * f64::from(lay.ky());
        let shape_bottom_pt = 298.0 * f64::from(PX_TO_PT * lay.scale);
        assert!(
            area_bottom_pt >= shape_bottom_pt,
            "MoveOnly shape bottom ({shape_bottom_pt}pt) must not exceed printed area bottom ({area_bottom_pt}pt)"
        );

        // Trailing page containing only the printed overhang of a MoveOnly shape must not be trimmed.
        let mut wb2 = Workbook::new();
        wb2.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), gridcraft_core::Value::text("Top"));
        // Pick a shape height whose raw pixels end on page 1, while printed height (h * k/ky) crosses onto page 2.
        let lay1 = layout_sheet(&wb2, 0, &PrintOptions::default(), None).unwrap();
        let avail_h_px = f64::from(lay1.avail().1 / lay1.ky());
        let sh2 = wb2.sheet(0).unwrap();
        let page1_rows = (avail_h_px / f64::from(sh2.default_row_height)).floor() as u32;
        let page1_bottom_px = sh2.row_top(page1_rows);
        let overhang_h = (page1_bottom_px - 4.0) as f32;
        wb2.sheet_mut(0).unwrap().shapes.push(gridcraft_model::Shape {
            id: 2,
            kind: gridcraft_model::ShapeKind::Rectangle,
            anchor: Anchor {
                cell: CellRef::new(0, 0),
                dx: 0.0,
                dy: 0.0,
                width: 80.0,
                height: overhang_h,
                mode: gridcraft_model::AnchorMode::MoveOnly,
            },
            fill: gridcraft_model::Color::Rgb(0x336699),
            line: gridcraft_model::Color::Auto,
            text: String::new(),
            format: gridcraft_model::ShapeFormat::default(),
            rotation: 0.0,
            math: gridcraft_model::EquationCache::default(),
        });
        let lay2 = layout_sheet(&wb2, 0, &PrintOptions::default(), None).unwrap();
        assert!(lay2.pages.len() >= 2, "trailing page holding MoveOnly printed overhang ({overhang_h}px * k/ky > {page1_bottom_px}px) must be kept");
    }
}
