//! The worksheet grid: virtualised painting of only the visible cells, frozen panes,
//! headers, selection, fill handle, column/row resizing, text overflow, borders, conditional
//! formats, tables, comments, filters, sparklines, floating charts/pictures/shapes and the
//! in-cell editor.

use std::sync::Arc;

use egui::{Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::cf::{CfCache, CfLook};
use gridcraft_engine::core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value, col_to_letters};
use gridcraft_engine::model::{BorderLine, BorderStyle, Color, HAlign, LineIndex, PatternType, Sheet, Style, Underline, VAlign, Workbook};
use serde_json::json;

use crate::SheetApp;
use crate::editor::{EditState, REF_COLORS};
use crate::theme::{self, Tokens};

pub const COL_HEADER_H: f32 = 21.0;
/// Sheet units are 96-dpi pixels (as in XLSX); on screen one unit is drawn at this many points,
/// which matches desktop spreadsheet density (a default 20 px row is 16 pt tall).
pub const DISPLAY_SCALE: f32 = 0.8;

/// The zoom percentages a Ctrl+wheel notch steps between: four per doubling (about 20% apart),
/// so 25, 50, 100, 200 and 400 are on it. Excel steps 15 points a notch instead; the status bar's
/// Zoom In / Zoom Out buttons keep their 10% steps.
pub const WHEEL_ZOOM_LADDER: [u16; 22] = [10, 12, 15, 18, 21, 25, 30, 35, 42, 50, 60, 70, 85, 100, 120, 140, 170, 200, 240, 280, 340, 400];

/// Seconds without Ctrl+wheel input after which part of a notch turned so far is forgotten.
const WHEEL_ZOOM_IDLE: f64 = 0.75;

/// The zoom one Ctrl+wheel notch goes to from `zoom`: the next step on [`WHEEL_ZOOM_LADDER`] in
/// that direction, skipping a step less than 5% away (121% out goes to 100%, not 120%), but never
/// 100% or the 10% / 400% limits (99% or 101% towards 100% goes to 100%).
pub fn wheel_zoom_step(zoom: u16, zoom_in: bool) -> u16 {
    let zoom = zoom.clamp(10, 400);
    let z = u32::from(zoom);
    let far = |l: u32| if zoom_in { l * 100 >= z * 105 } else { l * 105 <= z * 100 };
    let mut ahead = WHEEL_ZOOM_LADDER.iter().copied().filter(|&l| if zoom_in { l > zoom } else { l < zoom }).collect::<Vec<_>>();
    if !zoom_in {
        ahead.reverse();
    }
    let limit = ahead.last().copied().unwrap_or(zoom);
    ahead.into_iter().find(|&l| l == 100 || far(u32::from(l))).unwrap_or(limit)
}

/// One frame of the zoom gliding from `from` to `to` percent: the share `t` (0–1) of the way,
/// measured as a ratio so zooming in and out feel alike, and at least one percent, so it arrives.
pub fn zoom_glide_step(from: u16, to: u16, t: f32) -> u16 {
    let (f, g) = (f32::from(from.max(1)), f32::from(to.max(1)));
    let next = (f * (g / f).powf(t.clamp(0.0, 1.0))).round();
    let next = if next.is_finite() { next.clamp(f32::from(from.min(to)), f32::from(from.max(to))) as u16 } else { to };
    match (next == from, from.cmp(&to)) {
        (true, std::cmp::Ordering::Less) => from + 1,
        (true, std::cmp::Ordering::Greater) => from - 1,
        _ => next,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Drag {
    #[default]
    None,
    Select,
    Rows(u32),
    Cols(u32),
    /// Reordering one contiguous whole-column selection by its header.
    MoveCols {
        source: RangeRef,
        before: Option<u32>,
    },
    ResizeCol {
        col: u32,
        start: f32,
        orig: f32,
    },
    ResizeRow {
        row: u32,
        start: f32,
        orig: f32,
    },
    Fill {
        target: Option<RangeRef>,
    },
    /// Dragging a freeze handle: `cols` for the column bar, `to` the line count it would freeze.
    Freeze {
        cols: bool,
        to: Option<u32>,
    },
    /// Moving the selection by its border (cut & paste).
    Move {
        grab: CellRef,
        to: Option<CellRef>,
    },
    Object {
        kind: &'static str,
        id: u32,
        start: Pos2,
        orig: (CellRef, f32, f32, f32, f32),
        resize: bool,
    },
}

#[derive(Default)]
pub struct GridState {
    pub drag: Drag,
    pub ensure_visible: bool,
    /// With `ensure_visible`: scroll to the moving end of the selection instead of the active cell
    /// (Shift/Ctrl+Shift+arrows keep the active cell on the anchor while the far corner moves).
    pub reveal_moving_end: bool,
    pub last_title: Option<String>,
    pub drag_select: bool,
    pub rect: Option<Rect>,
    pub cells_rect: Option<Rect>,
    /// Column and row of the clicked header cell, and where to open the menu.
    pub filter_menu: Option<(u32, u32, Pos2)>,
    pub context_menu: Option<Pos2>,
    pub marching_phase: f32,
    /// The selection outline on its way to a new selection.
    pub glide: crate::selection_glide::SelectionGlide,
    pub last_click: Option<(CellRef, f64)>,
    pub hover_cell: Option<CellRef>,
    pub autosave: bool,
    pub last_autosave: f64,
    pub last_border: String,
    pub last_fill: String,
    pub last_font_color: String,
    pub trace: Option<serde_json::Value>,
    pub renaming_tab: Option<usize>,
    pub rename_text: String,
    pub list_picker: Option<CellRef>,
    /// Open task pane: comments, watch, selection, formatChart.
    pub pane: Option<String>,
    /// Ink stroke being drawn (sheet points).
    pub ink: Vec<[f32; 2]>,
    /// Page ranges for Page Break Preview, cached per (doc uid, revision, sheet).
    pub pages: Option<((u64, u64, usize), Vec<(RangeRef, u32)>)>,
    pub header_menu: Option<(Pos2, bool)>,
    /// Page Layout view pagination, cached per (doc uid, revision, sheet, rows, columns).
    pub paper: Option<crate::paper_view::PaperCache>,
    /// Header or footer being edited in place (Page Layout view).
    pub hf_edit: Option<crate::paper_view::HfEdit>,
    /// A page margin being dragged on a ruler (Page Layout view); committed on release.
    pub ruler_drag: Option<crate::paper_view::RulerDrag>,
    /// Insert › Header & Footer: start editing the first page's header once Page Layout view is up.
    pub hf_start: bool,
    /// The grid geometry of the last frame (hit-testing for agents and tests).
    pub geo: Option<Geo>,
    /// The horizontal scrollbar's track in the sheet tab bar (hit-testing for tests).
    pub hscroll: Option<Rect>,
    /// Ctrl+wheel zoom in progress (see [`WheelZoom`]).
    pub wheel_zoom: Option<WheelZoom>,
}

/// Ctrl+wheel zoom for one (doc uid, sheet). egui eases a notch in over several frames and
/// fine-grained wheels send parts of one, so the turning is counted in notches, and each whole
/// notch steps [`WHEEL_ZOOM_LADDER`]. With animations on the zoom then glides to that step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelZoom {
    pub place: (u64, usize),
    /// Turned so far towards the next step, in notches (+ in, − out).
    pub notches: f32,
    /// When `notches` last changed.
    pub time: f64,
    /// The step the zoom is gliding to, and the percentage set last on the way.
    pub glide: Option<(u16, u16)>,
}

impl WheelZoom {
    /// The state for `place` showing `zoom` percent: fresh for another document or sheet, and
    /// without its glide if the zoom was changed some other way since the glide set it.
    fn at(app: &SheetApp, place: (u64, usize), zoom: u16, now: f64) -> WheelZoom {
        let mut w = app.grid.wheel_zoom.filter(|w| w.place == place).unwrap_or(WheelZoom { place, notches: 0.0, time: now, glide: None });
        if w.glide.is_some_and(|(_, shown)| shown != zoom) {
            w.glide = None;
        }
        w
    }
}

impl GridState {
    pub fn autosave(&self) -> bool {
        self.autosave
    }
    pub fn toggle_autosave(&mut self) {
        self.autosave = !self.autosave;
    }
}

/// Geometry of the grid for one frame (handles zoom and frozen panes).
#[derive(Clone)]
pub struct Geo {
    pub rect: Rect,
    pub cells: Rect,
    pub z: f32,
    pub header_w: f32,
    pub header_h: f32,
    /// The outline bands (grouped rows / columns): left of the row headers and above the column
    /// headers; 0 without an outline (see `outline`).
    pub band_w: f32,
    pub band_h: f32,
    pub row_outline: Arc<crate::outline::Axis>,
    pub col_outline: Arc<crate::outline::Axis>,
    pub fr: u32,
    pub fc: u32,
    pub frozen_w: f32,
    pub frozen_h: f32,
    pub scroll: egui::Vec2,
    /// Sheet Right-to-Left: everything is laid out left-to-right as usual (these "logical"
    /// coordinates are what `x`, `cell_rect`, `col_at`… return) and mirrored across `rect`'s
    /// vertical centre line on screen: painted shapes by `mirror_painted`, widgets by
    /// `mirror_rect`/`mirror_pos`, and pointer input back by `mirror_pos` (an involution).
    pub rtl: bool,
    /// Prefix sums over custom row heights / column widths, built once per frame.
    ri: std::sync::Arc<LineIndex>,
    ci: std::sync::Arc<LineIndex>,
    /// Page Layout view: cells sit on pages (`scroll` is then in page-layout units).
    pub paper: Option<Arc<gridcraft_engine::paper::PageGrid>>,
}

impl Geo {
    pub fn new(sh: &Sheet, rect: Rect, scroll: egui::Vec2) -> Geo {
        let zoom = (sh.zoom as f32 / 100.0).clamp(0.1, 4.0);
        let z = zoom * DISPLAY_SCALE;
        let header_h = if sh.show_headings { (COL_HEADER_H * zoom.max(0.75)).round() } else { 0.0 };
        let ri = std::sync::Arc::new(LineIndex::new(&sh.rows, sh.default_row_height, MAX_ROWS));
        let ci = std::sync::Arc::new(LineIndex::new(&sh.cols, sh.default_col_width, MAX_COLS));
        let last_row = ri.at((scroll.y + rect.height() / z) as f64 + 200.0) + 1;
        let digits = (last_row as f32).log10().floor() as i32 + 1;
        let header_w = if sh.show_headings { ((digits.max(2) as f32 * 7.0 + 14.0) * zoom.max(0.75)).round() } else { 0.0 };
        let (fr, fc) = sh.freeze.unwrap_or((0, 0));
        let frozen_w = (ci.start(fc) as f32) * z;
        let frozen_h = (ri.start(fr) as f32) * z;
        let row_outline = Arc::new(crate::outline::Axis::new(&sh.rows));
        let col_outline = Arc::new(crate::outline::Axis::new(&sh.cols));
        let mut band_w = crate::outline::band(row_outline.levels, zoom);
        let mut band_h = crate::outline::band(col_outline.levels, zoom);
        // Without headings the level buttons still need a corner to sit in.
        if row_outline.levels > 0 && header_h == 0.0 {
            band_h = band_h.max(crate::outline::lane(zoom) + 3.0);
        }
        if col_outline.levels > 0 && header_w == 0.0 {
            band_w = band_w.max(crate::outline::lane(zoom) + 3.0);
        }
        let cells = Rect::from_min_max(pos2(rect.left() + band_w + header_w, rect.top() + band_h + header_h), rect.max);
        Geo {
            rect,
            cells,
            z,
            header_w,
            header_h,
            band_w,
            band_h,
            row_outline,
            col_outline,
            fr,
            fc,
            frozen_w,
            frozen_h,
            scroll,
            rtl: sh.right_to_left,
            ri,
            ci,
            paper: None,
        }
    }
    /// Left edge of the row headers (right of the outline band).
    pub fn head_left(&self) -> f32 {
        self.cells.left() - self.header_w
    }
    /// Top edge of the column headers (below the outline band).
    pub fn head_top(&self) -> f32 {
        self.cells.top() - self.header_h
    }
    /// Page Layout view geometry: `rect` excludes the rulers; cells are drawn at the print scale
    /// on pages laid out by `grid`; frozen panes don't apply (as in Excel).
    pub fn paged(sh: &Sheet, rect: Rect, scroll: egui::Vec2, grid: Arc<gridcraft_engine::paper::PageGrid>) -> Geo {
        let mut g = Geo::new(sh, rect, scroll);
        let zoom = (sh.zoom as f32 / 100.0).clamp(0.1, 4.0);
        g.z = zoom * DISPLAY_SCALE * grid.scale.clamp(0.1, 4.0);
        (g.fr, g.fc, g.frozen_w, g.frozen_h) = (0, 0, 0.0, 0.0);
        g.paper = Some(grid);
        g
    }
    /// The point is over cells (in Page Layout view: on a page, inside its margins).
    pub fn in_cells(&self, p: Pos2) -> bool {
        if !self.cells.contains(p) {
            return false;
        }
        match &self.paper {
            Some(pg) => pg.on_cells(
                self.scroll.x as f64 + ((p.x - self.cells.left()) / self.z) as f64,
                self.scroll.y as f64 + ((p.y - self.cells.top()) / self.z) as f64,
            ),
            None => true,
        }
    }
    /// Scroll offset (in `scroll` units) at which `row` reaches the top of the scrolled area.
    pub fn scroll_top_of(&self, sh: &Sheet, row: u32) -> f32 {
        match &self.paper {
            Some(pg) => pg.row_y(row) as f32,
            None => (sh.row_top(row) - sh.row_top(self.fr)) as f32,
        }
    }
    /// Scroll offset (in `scroll` units) at which `col` reaches the left of the scrolled area.
    pub fn scroll_left_of(&self, sh: &Sheet, col: u32) -> f32 {
        match &self.paper {
            Some(pg) => pg.col_x(col) as f32,
            None => (sh.col_left(col) - sh.col_left(self.fc)) as f32,
        }
    }
    /// Screen x for a logical x (and back): identity unless the sheet is right-to-left.
    pub fn mirror_x(&self, x: f32) -> f32 {
        if self.rtl { self.rect.left() + self.rect.right() - x } else { x }
    }
    pub fn mirror_pos(&self, p: Pos2) -> Pos2 {
        pos2(self.mirror_x(p.x), p.y)
    }
    pub fn mirror_rect(&self, r: Rect) -> Rect {
        if self.rtl { Rect::from_min_max(pos2(self.mirror_x(r.right()), r.top()), pos2(self.mirror_x(r.left()), r.bottom())) } else { r }
    }
    /// On-screen rectangle of a cell (`cell_rect` mirrored for right-to-left sheets).
    pub fn screen_cell_rect(&self, sh: &Sheet, c: CellRef) -> Rect {
        self.mirror_rect(self.cell_rect(sh, c))
    }
    pub fn x(&self, _sh: &Sheet, col: u32) -> f32 {
        if let Some(pg) = &self.paper {
            return self.cells.left() + (pg.col_x(col) - self.scroll.x as f64) as f32 * self.z;
        }
        if col < self.fc {
            self.cells.left() + self.ci.start(col) as f32 * self.z
        } else {
            self.cells.left() + self.frozen_w + (self.ci.start(col) as f32 - self.ci.start(self.fc) as f32 - self.scroll.x) * self.z
        }
    }
    pub fn y(&self, _sh: &Sheet, row: u32) -> f32 {
        if let Some(pg) = &self.paper {
            return self.cells.top() + (pg.row_y(row) - self.scroll.y as f64) as f32 * self.z;
        }
        if row < self.fr {
            self.cells.top() + self.ri.start(row) as f32 * self.z
        } else {
            self.cells.top() + self.frozen_h + (self.ri.start(row) as f32 - self.ri.start(self.fr) as f32 - self.scroll.y) * self.z
        }
    }
    pub fn cell_rect(&self, sh: &Sheet, c: CellRef) -> Rect {
        Rect::from_min_max(
            pos2(self.x(sh, c.col), self.y(sh, c.row)),
            pos2(self.x(sh, c.col) + sh.col_width(c.col) * self.z, self.y(sh, c.row) + sh.row_height(c.row) * self.z),
        )
    }
    pub fn range_rect(&self, sh: &Sheet, r: RangeRef) -> Rect {
        let a = self.cell_rect(sh, r.start);
        let b = self.cell_rect(sh, r.end);
        Rect::from_min_max(a.min, b.max)
    }
    pub fn col_at(&self, _sh: &Sheet, x: f32) -> u32 {
        if let Some(pg) = &self.paper {
            return pg.col_at(self.scroll.x as f64 + ((x - self.cells.left()) / self.z) as f64);
        }
        let dx = (x - self.cells.left()) / self.z;
        if dx < self.frozen_w / self.z {
            self.ci.at(dx.max(0.0) as f64)
        } else {
            self.ci.at(self.ci.start(self.fc) + self.scroll.x as f64 + (dx - self.frozen_w / self.z) as f64)
        }
    }
    pub fn row_at(&self, _sh: &Sheet, y: f32) -> u32 {
        if let Some(pg) = &self.paper {
            return pg.row_at(self.scroll.y as f64 + ((y - self.cells.top()) / self.z) as f64);
        }
        let dy = (y - self.cells.top()) / self.z;
        if dy < self.frozen_h / self.z {
            self.ri.at(dy.max(0.0) as f64)
        } else {
            self.ri.at(self.ri.start(self.fr) + self.scroll.y as f64 + (dy - self.frozen_h / self.z) as f64)
        }
    }
    pub fn cell_at(&self, sh: &Sheet, p: Pos2) -> CellRef {
        CellRef::new(self.row_at(sh, p.y), self.col_at(sh, p.x))
    }
    /// Visible scrolled rows (from the first visible row after the frozen ones).
    fn scroll_rows(&self, _sh: &Sheet) -> (u32, u32) {
        let top = self.ri.start(self.fr) + self.scroll.y as f64;
        let r0 = self.ri.at(top).max(self.fr);
        let r1 = self.ri.at(top + ((self.cells.height() - self.frozen_h) / self.z) as f64).min(MAX_ROWS - 1);
        (r0, r1)
    }
    fn scroll_cols(&self, _sh: &Sheet) -> (u32, u32) {
        let left = self.ci.start(self.fc) + self.scroll.x as f64;
        let c0 = self.ci.at(left).max(self.fc);
        let c1 = self.ci.at(left + ((self.cells.width() - self.frozen_w) / self.z) as f64).min(MAX_COLS - 1);
        (c0, c1)
    }
    /// Screen areas of the panes (frozen rows/columns or the scrolling part) that hold part of `r`.
    /// The panes share one coordinate system, so a range that is not clipped to these is also drawn
    /// over the frozen rows/columns when it is scrolled under them.
    pub fn range_panes(&self, r: RangeRef) -> Vec<Rect> {
        let split = pos2(self.cells.left() + self.frozen_w, self.cells.top() + self.frozen_h);
        let mut xs = Vec::new();
        if r.start.col < self.fc {
            xs.push((self.cells.left(), split.x));
        }
        if r.end.col >= self.fc {
            xs.push((split.x, self.cells.right()));
        }
        let mut ys = Vec::new();
        if r.start.row < self.fr {
            ys.push((self.cells.top(), split.y));
        }
        if r.end.row >= self.fr {
            ys.push((split.y, self.cells.bottom()));
        }
        ys.iter().flat_map(|&(y0, y1)| xs.iter().map(move |&(x0, x1)| Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)))).collect()
    }
    /// True when cell `c` is in the scrolling pane but scrolled entirely under the frozen rows or columns.
    pub fn hidden_under_frozen(&self, sh: &Sheet, c: CellRef) -> bool {
        let r = self.cell_rect(sh, c);
        (self.fc > 0 && c.col >= self.fc && r.right() <= self.cells.left() + self.frozen_w)
            || (self.fr > 0 && c.row >= self.fr && r.bottom() <= self.cells.top() + self.frozen_h)
    }
}

/// Resolved look of a cell for painting.
struct Look {
    style: Style,
    fill: Option<Color32>,
    text_color: Color32,
}

fn col32(c: Color, wb: &Workbook, default: Color32) -> Color32 {
    c.resolve(&wb.theme).map(theme::color32).unwrap_or(default)
}

fn numfmt_color(c: gridcraft_engine::numfmt::FormatColor) -> Color32 {
    use gridcraft_engine::numfmt::FormatColor as F;
    match c {
        F::Black => Color32::BLACK,
        F::Blue => Color32::from_rgb(0, 0, 255),
        F::Cyan => Color32::from_rgb(0, 255, 255),
        F::Green => Color32::from_rgb(0, 128, 0),
        F::Magenta => Color32::from_rgb(255, 0, 255),
        F::Red => Color32::from_rgb(255, 0, 0),
        F::White => Color32::WHITE,
        F::Yellow => Color32::from_rgb(255, 255, 0),
        F::Indexed(_) => Color32::BLACK,
    }
}

/// Last column of a Center Across Selection run starting at `start`: it extends over following columns (up to `limit`) while `continues` holds.
fn center_across_end(start: u32, limit: u32, continues: impl Fn(u32) -> bool) -> u32 {
    let mut end = start;
    while end < limit && end + 1 < MAX_COLS && continues(end + 1) {
        end += 1;
    }
    end
}

fn look(wb: &Workbook, si: usize, sh: &Sheet, c: CellRef, cf: &mut CfCache) -> (Look, Option<CfLook>) {
    let mut style = wb.styles.get(sh.style_id(c)).clone();
    if let Some(t) = sh.table_at(c) {
        style = gridcraft_engine::tables::table_cell_style(wb, si, t, c);
    }
    let cfl = cf.look(wb, si, c);
    if let Some(l) = &cfl
        && let Some(st) = &l.style
    {
        if st.fill.pattern != PatternType::None {
            style.fill = st.fill;
        }
        if st.font.color != Color::Auto {
            style.font.color = st.font.color;
        }
        if st.font.bold {
            style.font.bold = true;
        }
        if st.font.italic {
            style.font.italic = true;
        }
        if st.font.underline != Underline::None {
            style.font.underline = st.font.underline;
        }
        if st.font.strike {
            style.font.strike = true;
        }
        for (dst, src) in [
            (&mut style.borders.left, st.borders.left),
            (&mut style.borders.right, st.borders.right),
            (&mut style.borders.top, st.borders.top),
            (&mut style.borders.bottom, st.borders.bottom),
        ] {
            if !src.is_none() {
                *dst = src;
            }
        }
    }
    let mut fill = match style.fill.pattern {
        PatternType::None => None,
        PatternType::Solid => style.fill.fg.resolve(&wb.theme).map(theme::color32),
        _ => {
            // Patterns: approximate with a blend of foreground and background.
            let fg = style.fill.fg.resolve(&wb.theme).unwrap_or([0, 0, 0]);
            let bg = style.fill.bg.resolve(&wb.theme).unwrap_or([255, 255, 255]);
            Some(Color32::from_rgb(
                ((fg[0] as u16 + bg[0] as u16 * 3) / 4) as u8,
                ((fg[1] as u16 + bg[1] as u16 * 3) / 4) as u8,
                ((fg[2] as u16 + bg[2] as u16 * 3) / 4) as u8,
            ))
        }
    };
    if let Some(l) = &cfl
        && let Some(rgb) = l.scale_fill
    {
        fill = Some(theme::color32(rgb));
    }
    let text_color = col32(style.font.color, wb, Color32::BLACK);
    (Look { style, fill, text_color }, cfl)
}

/// Text for a value with General narrowing to the available width.
fn display_text(
    wb: &Workbook,
    sh: &Sheet,
    c: CellRef,
    v: &Value,
    st: &Style,
    width_px: f32,
    char_w: f32,
) -> (String, Option<Color32>, bool, Option<(char, usize)>) {
    if sh.show_formulas
        && let Some(f) = sh.cell(c).and_then(|x| x.formula.as_ref())
    {
        return (format!("={}", f.text), None, false, None);
    }
    if !sh.show_zeros && v.as_f64() == Some(0.0) {
        return (String::new(), None, false, None);
    }
    let code = st.num_fmt.as_str();
    // A checkbox cell draws TRUE/FALSE (and empty) as a box; other values show as General.
    if code == "checkbox" && matches!(v, Value::Bool(_) | Value::Empty) {
        return (String::new(), None, false, None);
    }
    if code == "General"
        && let Value::Number(n) = v
    {
        let max_chars = ((width_px - 4.0) / char_w).floor().max(1.0) as usize;
        return match gridcraft_engine::numfmt::format_general_fit(*n, max_chars.min(11)) {
            Some(t) => (t, None, true, None),
            None => ("#".repeat(max_chars.max(1)), None, true, None),
        };
    }
    let f = gridcraft_engine::display::format(v, code, wb);
    (f.text, f.color.map(numfmt_color), f.numeric, f.fill)
}

pub fn show(app: &mut SheetApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let available = ui.available_rect_before_wrap();
    // A right-to-left sheet has its vertical scrollbar on the left.
    let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
    let bar = 14.0_f32.min(available.width());
    let rect = if rtl {
        Rect::from_min_max(pos2(available.left() + bar, available.top()), available.max)
    } else {
        Rect::from_min_max(available.min, pos2(available.right() - bar, available.bottom()))
    };
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    app.grid.rect = Some(rect);
    let Some(d) = app.session.active() else { return };
    let wb: Arc<Workbook> = d.wb.clone();
    let si = wb.active_sheet;
    let Some(sh) = wb.sheet(si) else { return };
    let sel = d.selection.clone();
    let place = (d.uid, si);
    let mut view = app.view();
    // Page Layout view: pages on a desk, rulers above and left of the headings.
    let paper = sh.view == gridcraft_engine::model::SheetViewMode::PageLayout;
    let ruler = if paper && !app.session.hide_ruler { crate::paper_view::RULER } else { 0.0 };
    // Right-to-left sheets carry the vertical ruler on the right, beside the row headings.
    let grid_rect = if rtl {
        Rect::from_min_max(pos2(rect.left(), (rect.top() + ruler).min(rect.bottom())), pos2((rect.right() - ruler).max(rect.left()), rect.bottom()))
    } else {
        Rect::from_min_max(pos2((rect.left() + ruler).min(rect.right()), (rect.top() + ruler).min(rect.bottom())), rect.max)
    };
    let make_geo = |app: &mut SheetApp, scroll: egui::Vec2| -> Geo {
        if paper {
            let probe = Geo::new(sh, grid_rect, scroll);
            if let Some(g) = crate::paper_view::grid_for(app, &wb, si, sh, probe.cells.size(), scroll) {
                let g = crate::paper_view::with_dragged_margin(app, &wb, si, g);
                return Geo::paged(sh, grid_rect, scroll, g);
            }
        }
        Geo::new(sh, rect, scroll)
    };
    // Scrolling.
    let (scroll_delta, zoom_delta, modifiers, now) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers, i.time));
    if (resp.hovered() || resp.contains_pointer()) && !crate::text_box::contains_pointer(app, ui.ctx()) {
        if zoom_delta != 1.0 {
            // egui's zoom factor is e^(speed × points); one notch is `line_scroll_speed` points.
            let notch = ui.ctx().options(|o| o.input_options.scroll_zoom_speed * o.input_options.line_scroll_speed);
            let turned = zoom_delta.ln() / notch;
            let mut zooming = WheelZoom::at(app, place, sh.zoom, now);
            if now - zooming.time > WHEEL_ZOOM_IDLE {
                zooming.notches = 0.0;
            }
            if turned.is_finite() {
                zooming.notches = (zooming.notches + turned).clamp(-32.0, 32.0);
            }
            zooming.time = now;
            // A notch steps once 60% of it has turned, and the rest of it brings `notches` back to
            // about 0. (Past half, so what's left after a step can't step back.) Steps count from
            // where a glide is going, not from where it has got to.
            let from = zooming.glide.map_or(sh.zoom, |(to, _)| to);
            let mut z = from;
            while zooming.notches >= 0.6 {
                z = wheel_zoom_step(z, true);
                zooming.notches -= 1.0;
            }
            while zooming.notches <= -0.6 {
                z = wheel_zoom_step(z, false);
                zooming.notches += 1.0;
            }
            if z != from {
                zooming.glide = Some((z, sh.zoom));
            }
            app.grid.wheel_zoom = Some(zooming);
        } else if scroll_delta != egui::Vec2::ZERO {
            let z = if paper { make_geo(app, view.scroll).z } else { sh.zoom as f32 / 100.0 * DISPLAY_SCALE };
            let mut d = scroll_delta;
            // A horizontal swipe moves the content the way the fingers go, which is towards
            // higher columns on a mirrored sheet.
            if sh.right_to_left {
                d.x = -d.x;
            }
            if modifiers.shift && d.x == 0.0 {
                d = vec2(d.y, 0.0);
            }
            view.scroll -= d / z;
        }
    }
    // The zoom glides to its step like egui eases a wheel notch (90% of the way in 0.1 s), or goes
    // there at once without animations (as for the selection outline below: View › Animations
    // off, egui's off in the headless harness, or a screenshot on its way).
    let mut zooming = WheelZoom::at(app, place, sh.zoom, now);
    if let Some((to, shown)) = zooming.glide {
        let glides = app.ui.animations && ui.ctx().global_style().animation_time > 0.0 && !app.shots.in_flight();
        let share = egui::emath::exponential_smooth_factor(0.9, 0.1, ui.input(|i| i.stable_dt).min(0.1));
        let z = if glides { zoom_glide_step(shown, to, share) } else { to };
        let _ = app.session.run("view.zoom", json!({"percent": z}));
        zooming.glide = (z != to).then_some((to, z));
        ui.ctx().request_repaint();
    }
    // Store a glide's progress, or that it stopped.
    if app.grid.wheel_zoom.is_some_and(|w| w.glide.is_some()) {
        app.grid.wheel_zoom = Some(zooming);
    }
    view.scroll = view.scroll.max(egui::Vec2::ZERO);
    let (mut max_x, mut max_y) = (sh.col_left(MAX_COLS - 20) as f32, sh.row_top(MAX_ROWS - 40) as f32);
    if paper {
        // Page layout units run past sheet units (margins and gaps); the end is known once the
        // pages reach the last row / column.
        let g = make_geo(app, view.scroll);
        if let Some(pg) = &g.paper {
            let (ex, ey) = pg.extent();
            max_x = if pg.cols_complete { (ex as f32 - g.cells.width() / g.z).max(0.0) } else { 1.0e9 };
            max_y = if pg.rows_complete { (ey as f32 - g.cells.height() / g.z).max(0.0) } else { 1.0e9 };
        }
    }
    view.scroll.x = view.scroll.x.min(max_x);
    view.scroll.y = view.scroll.y.min(max_y);
    let mut geo = make_geo(app, view.scroll);
    // Keep the active cell (or the moving end of an extension) visible after keyboard moves.
    if app.grid.ensure_visible {
        app.grid.ensure_visible = false;
        let cell = if std::mem::take(&mut app.grid.reveal_moving_end) { sel.moving_end() } else { sel.active };
        let target = app.editor.as_ref().and_then(|e| e.point_cell).unwrap_or(cell);
        if paper {
            crate::paper_view::ensure_covers(app, &wb, si, target);
            geo = make_geo(app, view.scroll);
        }
        let cr = geo.cell_rect(sh, target);
        let area = Rect::from_min_max(pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h), geo.cells.max);
        if target.row >= geo.fr {
            if cr.top() < area.top() {
                view.scroll.y -= (area.top() - cr.top()) / geo.z;
            } else if cr.bottom() > area.bottom() - 2.0 {
                view.scroll.y += (cr.bottom() - area.bottom() + 2.0) / geo.z;
            }
        }
        if target.col >= geo.fc {
            if cr.left() < area.left() {
                view.scroll.x -= (area.left() - cr.left()) / geo.z;
            } else if cr.right() > area.right() - 2.0 {
                view.scroll.x += ((cr.right() - area.right() + 2.0) / geo.z).min(cr.left() - area.left());
            }
        }
        view.scroll = view.scroll.max(egui::Vec2::ZERO);
        geo = make_geo(app, view.scroll);
    }
    // Snap scrolling to whole rows/columns at rest like Excel? Keep smooth (better on trackpads).
    if let Some(v) = app.view_mut() {
        *v = view;
    }
    app.grid.cells_rect = Some(geo.mirror_rect(geo.cells));
    // The selection outline glides to a new selection. It stays on its cells when Animations is
    // off, when egui's animations are (the headless harness), and while a screenshot is on its way.
    let outline = app.grid.glide.frame(crate::selection_glide::Frame {
        place,
        range: sel.current(),
        target: geo.range_rect(sh, sel.current()),
        view: geo.cells,
        now,
        frame_nr: ui.ctx().cumulative_frame_nr(),
        animate: app.ui.animations && ui.ctx().global_style().animation_time > 0.0 && !app.shots.in_flight(),
    });
    if app.grid.glide.is_gliding() {
        ui.ctx().request_repaint();
    }
    app.grid.geo = Some(geo.clone());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.grid_bg);
    // Everything up to the headers is painted in logical (left-to-right) coordinates.
    let mirror_from = painter.add(egui::Shape::Noop);
    let mut cf = CfCache::new();
    let quads = if geo.paper.is_some() {
        crate::paper_view::paint_pages(app, &painter, &geo, &wb, si, sh, &sel, outline, &mut cf, &t)
    } else {
        paint_panes(app, &painter, &geo, &wb, si, sh, &sel, outline, &mut cf, &t)
    };
    // Objects above cells.
    let objects = crate::chartview::paint_objects(app, &painter.with_clip_rect(geo.cells), &geo, &wb, si, sh);
    paint_overlays(app, ui, &painter.with_clip_rect(geo.cells), &geo, &wb, si, sh, &sel);
    if sh.view == gridcraft_engine::model::SheetViewMode::PageBreakPreview {
        paint_pages(app, &painter.with_clip_rect(geo.cells), &geo, sh);
    }
    if geo.paper.is_some() {
        crate::paper_view::paint_headers_footers(app, ui, &painter, &geo, &wb, sh);
    }
    // Headers.
    if sh.show_headings {
        paint_headers(&painter, &geo, sh, &sel, &t, &quads);
        paint_freeze_handles(&painter, &geo, sh, &app.grid.drag, &t);
    }
    crate::outline::paint(&painter, &geo, sh, &t);
    if geo.rtl {
        let mirror_to = painter.add(egui::Shape::Noop);
        mirror_painted(ui.ctx(), painter.layer_id(), mirror_from.0..mirror_to.0, &geo, &objects);
    }
    if geo.paper.is_some() {
        crate::paper_view::paint_rulers(app, &painter, rect, &geo, &t);
    }
    // Interaction.
    let previous_popups = (app.grid.filter_menu, app.grid.list_picker, app.grid.header_menu);
    crate::paper_view::interact(app, ui, &resp, &geo, sh, &wb);
    interact(app, ui, &resp, &geo, sh, &wb);
    crate::text_box::show(app, ui, &geo, sh);
    if app.editor.as_ref().is_some_and(|e| !e.from_formula_bar) {
        in_cell_editor(app, ui, &geo, sh, &wb);
    }
    filter_menu(app, ui, &geo, previous_popups.0 == app.grid.filter_menu);
    list_picker(app, ui, &geo, previous_popups.1 == app.grid.list_picker);
    header_menu(app, ui, previous_popups.2 == app.grid.header_menu);
    context_menu(app, ui);
    let track_x = if rtl { (available.left(), rect.left()) } else { (rect.right(), available.right()) };
    vertical_scrollbar(app, ui, sh, &geo, track_x, max_y, &t);
    if app.session.clipboard.is_some() {
        // By the clock, not per frame: a gliding outline repaints more often than the ants step.
        app.grid.marching_phase = crate::selection_glide::ants_phase(now);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
}

/// Normal view: up to four panes (frozen rows/columns), each painted with its own clip.
#[allow(clippy::too_many_arguments)]
fn paint_panes(
    app: &SheetApp,
    painter: &Painter,
    geo: &Geo,
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    sel: &gridcraft_engine::Selection,
    outline: Rect,
    cf: &mut CfCache,
    t: &Tokens,
) -> Vec<((u32, u32), (u32, u32), Rect)> {
    // Quadrants: (rows, cols, clip).
    let (sr0, sr1) = geo.scroll_rows(sh);
    let (sc0, sc1) = geo.scroll_cols(sh);
    let fr_rows = (0, geo.fr.saturating_sub(1));
    let fr_cols = (0, geo.fc.saturating_sub(1));
    let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
    let mut quads = vec![((sr0, sr1), (sc0, sc1), Rect::from_min_max(split, geo.cells.max))];
    if geo.fr > 0 {
        quads.push((fr_rows, (sc0, sc1), Rect::from_min_max(pos2(split.x, geo.cells.top()), pos2(geo.cells.right(), split.y))));
    }
    if geo.fc > 0 {
        quads.push(((sr0, sr1), fr_cols, Rect::from_min_max(pos2(geo.cells.left(), split.y), pos2(split.x, geo.cells.bottom()))));
    }
    if geo.fr > 0 && geo.fc > 0 {
        quads.push((fr_rows, fr_cols, Rect::from_min_max(geo.cells.min, split)));
    }
    for (rows, cols, clip) in &quads {
        if rows.0 > rows.1 || cols.0 > cols.1 {
            continue;
        }
        let p = painter.with_clip_rect(*clip);
        paint_quadrant(&p, geo, wb, si, sh, *rows, *cols, cf, t);
        paint_selection(&p, geo, sh, sel, outline, t, app, *rows, *cols);
    }
    // Freeze lines.
    if geo.fr > 0 {
        painter.line_segment([pos2(geo.cells.left(), split.y), pos2(geo.cells.right(), split.y)], Stroke::new(1.0, Color32::from_gray(150)));
    }
    if geo.fc > 0 {
        painter.line_segment([pos2(split.x, geo.cells.top()), pos2(split.x, geo.cells.bottom())], Stroke::new(1.0, Color32::from_gray(150)));
    }
    quads
}

/// The worksheet is painted virtually, so it needs its own scrollbar rather than a
/// ScrollArea containing all million rows. Keep the scroll extent stable during a drag.
fn vertical_scrollbar(app: &mut SheetApp, ui: &mut egui::Ui, sh: &Sheet, geo: &Geo, track_x: (f32, f32), limit: f32, t: &Tokens) {
    let track = Rect::from_min_max(pos2(track_x.0, geo.cells.top() + geo.frozen_h), pos2(track_x.1, geo.cells.bottom()));
    if track.height() <= 0.0 || track.width() <= 0.0 {
        return;
    }
    let visible = track.height() / geo.z;
    let scroll = app.view().scroll.y;
    let last = sh.used_range().map(|r| r.end.row.saturating_add(10)).unwrap_or(100).clamp(100, MAX_ROWS - 1);
    let content = geo.scroll_top_of(sh, last);
    let extent = (content - visible).max(scroll).max(0.0).min(limit);
    let id = ui.id().with("vscroll");
    let resp = ui.interact(track, id, Sense::click_and_drag());
    let drag_id = id.with("extent");
    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, extent));
    }
    let extent = if resp.dragged() { ui.ctx().data(|d| d.get_temp::<f32>(drag_id)).unwrap_or(extent) } else { extent };
    let thumb_h = (track.height() * visible / (extent + visible)).clamp(24.0_f32.min(track.height()), track.height());
    let travel = track.height() - thumb_h;
    let thumb_top = track.top() + if extent > 0.0 { travel * (scroll / extent).clamp(0.0, 1.0) } else { 0.0 };
    let thumb = Rect::from_min_size(pos2(track.left() + 3.0, thumb_top), vec2((track.width() - 6.0).max(0.0), thumb_h));
    ui.painter().rect_filled(track, 0.0, t.tab_bar);
    ui.painter().rect_filled(thumb, 4.0, if resp.hovered() || resp.dragged() { t.text_dim } else { t.text_disabled });
    let mut next = scroll;
    if resp.dragged() && travel > 0.0 {
        next += resp.drag_delta().y / travel * extent;
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        if p.y < thumb.top() {
            next -= visible;
        } else if p.y > thumb.bottom() {
            next += visible;
        }
    }
    if resp.hovered() {
        next -= ui.input(|i| i.smooth_scroll_delta.y) / geo.z;
    }
    next = next.clamp(0.0, limit);
    if next != scroll {
        if let Some(v) = app.view_mut() {
            v.scroll.y = next;
        }
        ui.ctx().request_repaint();
    }
}

/// The horizontal counterpart of `vertical_scrollbar`, drawn in the sheet tab bar as in Excel. The tab
/// bar is laid out before the grid, so this reads the last frame's geometry and leaves clamping the
/// far end to the grid, which runs later in the same frame.
pub(crate) fn horizontal_scrollbar(app: &mut SheetApp, ui: &mut egui::Ui, track: Rect, t: &Tokens) {
    app.grid.hscroll = Some(track);
    if track.height() <= 0.0 || track.width() <= 0.0 {
        return;
    }
    let Some(geo) = app.grid.geo.clone() else { return };
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let Some(sh) = wb.active() else { return };
    let visible = ((geo.cells.width() - geo.frozen_w) / geo.z).max(0.0);
    let scroll = app.view().scroll.x;
    let last = sh.used_range().map(|r| r.end.col.saturating_add(10)).unwrap_or(26).clamp(26, MAX_COLS - 1);
    let content = geo.scroll_left_of(sh, last);
    let extent = (content - visible).max(scroll).max(0.0);
    let id = ui.id().with("hscroll");
    let resp = ui.interact(track, id, Sense::click_and_drag());
    let drag_id = id.with("extent");
    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, extent));
    }
    let extent = if resp.dragged() { ui.ctx().data(|d| d.get_temp::<f32>(drag_id)).unwrap_or(extent) } else { extent };
    let thumb_w = (track.width() * visible / (extent + visible).max(1.0)).clamp(24.0_f32.min(track.width()), track.width());
    let travel = track.width() - thumb_w;
    let offset = if extent > 0.0 { travel * (scroll / extent).clamp(0.0, 1.0) } else { 0.0 };
    // A right-to-left sheet scrolls towards the left, so its thumb starts at the right end.
    let dir = if geo.rtl { -1.0 } else { 1.0 };
    let thumb_left = if geo.rtl { track.right() - thumb_w - offset } else { track.left() + offset };
    let thumb = Rect::from_min_size(pos2(thumb_left, track.top() + 3.0), vec2(thumb_w, (track.height() - 6.0).max(0.0)));
    ui.painter().rect_filled(track, 0.0, t.tab_bar);
    ui.painter().rect_filled(thumb, 4.0, if resp.hovered() || resp.dragged() { t.text_dim } else { t.text_disabled });
    let mut next = scroll;
    if resp.dragged() && travel > 0.0 {
        next += dir * resp.drag_delta().x / travel * extent;
    } else if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        if p.x < thumb.left() {
            next -= dir * visible;
        } else if p.x > thumb.right() {
            next += dir * visible;
        }
    }
    if resp.hovered() {
        let d = ui.input(|i| i.smooth_scroll_delta);
        next -= (if d.x != 0.0 { dir * d.x } else { d.y }) / geo.z;
    }
    next = next.max(0.0);
    if next != scroll {
        if let Some(v) = app.view_mut() {
            v.scroll.x = next;
        }
        ui.ctx().request_repaint();
    }
}

pub(crate) fn wrapped_cell_text_y(valign: VAlign, rect: Rect, gh: f32, z: f32) -> f32 {
    let pad_v = 1.5 * z;
    let y_top = rect.top() + pad_v;
    let y_bottom = (rect.bottom() - gh - pad_v).max(y_top);
    match valign {
        VAlign::Top => y_top,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => (y_top + y_bottom) / 2.0,
        VAlign::Bottom => y_bottom,
    }
}

pub(crate) fn cell_text_y(valign: VAlign, rect: Rect, gh: f32, z: f32) -> f32 {
    let top_inset = 1.0 * z;
    let y_top = rect.top() - top_inset;
    let y_bottom = (rect.bottom() - gh - 0.5 * z).max(y_top);
    match valign {
        VAlign::Top => y_top,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => (y_top + y_bottom) / 2.0,
        VAlign::Bottom => y_bottom,
    }
}

/// Where to put a `TextShape` holding a galley of `size` turned by `angle` (egui radians, so
/// negative for upward text) for it to sit in `rect` per the cell's alignment. egui turns the
/// galley about `pos`, its top-left corner, so the turned box is placed first and `pos` follows.
/// General keeps the text at the left. Bottom (the default) puts upward text at the bottom, but
/// downward text at the top, where it starts reading.
pub(crate) fn rotated_text_pos(rect: Rect, size: egui::Vec2, angle: f32, h: HAlign, v: VAlign, pad: f32, indent: f32) -> Pos2 {
    let rot = egui::emath::Rot2::from_angle(angle);
    let turned = Rect::from_points(&[vec2(0.0, 0.0), vec2(size.x, 0.0), vec2(0.0, size.y), size].map(|c| (rot * c).to_pos2()));
    let x = match h {
        HAlign::Center | HAlign::CenterAcross | HAlign::Distributed => rect.center().x - turned.width() / 2.0,
        HAlign::Right => rect.right() - pad - indent - turned.width(),
        _ => rect.left() + pad + indent,
    };
    let y = match v {
        VAlign::Center | VAlign::Justify | VAlign::Distributed => rect.center().y - turned.height() / 2.0,
        VAlign::Bottom if angle < 0.0 => rect.bottom() - pad - turned.height(),
        _ => rect.top() + pad,
    };
    pos2(x, y) - turned.min.to_vec2()
}

/// Mirror what the grid painted into `layer` (the shape indices in `range`) for a right-to-left
/// sheet. Geometry is reflected; text and meshes (glyphs, pictures) keep their reading direction
/// and only move so that their box lands mirrored. Floating objects (`objects`: shape index
/// ranges with their logical rect) move as a whole, so a chart keeps its axes and a picture
/// isn't flipped.
fn mirror_painted(ctx: &egui::Context, layer: egui::LayerId, range: std::ops::Range<usize>, geo: &Geo, objects: &[crate::chartview::PaintedObject]) {
    let axis = geo.rect.left() + geo.rect.right();
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for i in range {
            let object = objects.iter().find(|o| o.shapes.contains(&i));
            list.mutate_shape(egui::layers::ShapeIdx(i), |cs| {
                cs.clip_rect = geo.mirror_rect(cs.clip_rect);
                match object {
                    Some(o) => cs.shape.translate(vec2(axis - o.rect.right() - o.rect.left(), 0.0)),
                    None => mirror_shape(&mut cs.shape, axis),
                }
            });
        }
    });
}

/// Reflect one shape across the vertical line `x = axis / 2`.
fn mirror_shape(shape: &mut egui::Shape, axis: f32) {
    use egui::Shape;
    let flip = |p: &mut Pos2| p.x = axis - p.x;
    match shape {
        Shape::Noop | Shape::Callback(_) => {}
        Shape::Vec(v) => v.iter_mut().for_each(|s| mirror_shape(s, axis)),
        Shape::Circle(c) => flip(&mut c.center),
        Shape::Ellipse(e) => flip(&mut e.center),
        Shape::LineSegment { points, .. } => points.iter_mut().for_each(flip),
        Shape::Path(p) => {
            p.points.iter_mut().for_each(flip);
            // Keep the preferred (clockwise) winding for filled polygons.
            p.points.reverse();
        }
        Shape::Rect(r) => {
            r.rect = Rect::from_min_max(pos2(axis - r.rect.right(), r.rect.top()), pos2(axis - r.rect.left(), r.rect.bottom()));
            let c = r.corner_radius;
            r.corner_radius = egui::CornerRadius { nw: c.ne, ne: c.nw, sw: c.se, se: c.sw };
        }
        Shape::QuadraticBezier(b) => b.points.iter_mut().for_each(flip),
        Shape::CubicBezier(b) => b.points.iter_mut().for_each(flip),
        _ => {
            let b = shape.visual_bounding_rect();
            if b.is_finite() {
                shape.translate(vec2(axis - b.right() - b.left(), 0.0));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_quadrant(
    p: &Painter,
    geo: &Geo,
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    rows: (u32, u32),
    cols: (u32, u32),
    cf: &mut CfCache,
    t: &Tokens,
) {
    let z = geo.z;
    let (r0, r1) = rows;
    let (c0, c1) = cols;
    let range = RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1));
    let merges: Vec<RangeRef> = sh.merges.iter().filter(|m| m.intersects(&range)).copied().collect();
    let in_merge = |c: CellRef| merges.iter().find(|m| m.contains(c)).copied();
    // Gridlines.
    if sh.show_gridlines {
        let stroke = Stroke::new(1.0, t.gridline);
        let top = geo.y(sh, r0);
        let bottom = geo.y(sh, r1 + 1);
        let left = geo.x(sh, c0);
        let right = geo.x(sh, c1 + 1);
        for c in c0..=c1 + 1 {
            if c <= c1 && sh.col_width(c) == 0.0 {
                continue;
            }
            let x = geo.x(sh, c).round() - 0.5;
            p.line_segment([pos2(x, top), pos2(x, bottom)], stroke);
        }
        for r in r0..=r1 + 1 {
            if r <= r1 && sh.row_height(r) == 0.0 {
                continue;
            }
            let y = geo.y(sh, r).round() - 0.5;
            p.line_segment([pos2(left, y), pos2(right, y)], stroke);
        }
    }
    // Merged areas: blank them (no inner gridlines) and paint as one cell.
    let mut cells: Vec<CellRef> = Vec::new();
    for r in r0..=r1 {
        if sh.row_height(r) == 0.0 {
            continue;
        }
        for c in c0..=c1 {
            if sh.col_width(c) == 0.0 {
                continue;
            }
            cells.push(CellRef::new(r, c));
        }
    }
    let empty_ok = |c: CellRef| -> bool { sh.value_ref(c).is_none_or(Value::is_empty) && in_merge(c).is_none() };
    // Backgrounds.
    let mut looks: Vec<(CellRef, Rect, Look, Option<CfLook>)> = Vec::with_capacity(cells.len().min(20_000));
    for &c in &cells {
        let (rect, anchor) = match in_merge(c) {
            Some(m) if m.start != c && !(c.row == r0.max(m.start.row) && c.col == c0.max(m.start.col)) => continue,
            Some(m) => (geo.range_rect(sh, m), m.start),
            None => (geo.cell_rect(sh, c), c),
        };
        let (lk, cfl) = look(wb, si, sh, anchor, cf);
        if let Some(f) = lk.fill {
            p.rect_filled(rect.expand(0.0), 0.0, f);
        } else if in_merge(c).is_some() {
            p.rect_filled(rect.shrink(0.5), 0.0, Color32::WHITE);
        }
        looks.push((anchor, rect, lk, cfl));
    }
    // Text and per-cell decorations.
    for (c, rect, lk, cfl) in &looks {
        let c = *c;
        if let Some(picture) = sh.cell_pictures.get(&c) {
            crate::cell_pictures::paint(p, *rect, picture);
            continue;
        }
        let v = sh.value(c);
        if let Some(l) = cfl
            && let Some((frac, rgb, gradient)) = l.bar
        {
            let bar = Rect::from_min_size(pos2(rect.left() + 2.0, rect.top() + 2.0), vec2((rect.width() - 4.0) * frac, rect.height() - 4.0));
            let col = theme::color32(rgb);
            if gradient {
                let mut mesh = egui::Mesh::default();
                let light = Color32::from_rgb(
                    ((rgb[0] as u16 + 255 * 3) / 4) as u8,
                    ((rgb[1] as u16 + 255 * 3) / 4) as u8,
                    ((rgb[2] as u16 + 255 * 3) / 4) as u8,
                );
                let i = mesh.vertices.len() as u32;
                for (pt, cc) in [(bar.left_top(), col), (bar.right_top(), light), (bar.right_bottom(), light), (bar.left_bottom(), col)] {
                    mesh.colored_vertex(pt, cc);
                }
                mesh.add_triangle(i, i + 1, i + 2);
                mesh.add_triangle(i, i + 2, i + 3);
                p.add(mesh);
                p.rect_stroke(bar, 0.0, Stroke::new(1.0, col), StrokeKind::Inside);
            } else {
                p.rect_filled(bar, 0.0, col);
            }
        }
        let mut text_left = rect.left();
        if let Some(l) = cfl
            && let Some((set, idx)) = &l.icon
        {
            let r = Rect::from_min_size(pos2(rect.left() + 2.0, rect.center().y - 6.0 * z), vec2(12.0 * z, 12.0 * z));
            paint_cf_icon(p, r, set, *idx);
            text_left += 15.0 * z;
        }
        if lk.style.num_fmt.as_str() == "checkbox" && matches!(v, Value::Bool(_) | Value::Empty) {
            let s = (11.0 * z * 1.2).clamp(8.0, 24.0);
            let b = Rect::from_center_size(rect.center(), vec2(s, s));
            let on = v == Value::Bool(true);
            p.rect_filled(b, 2.0, if on { Color32::from_rgb(0x10, 0x7C, 0x41) } else { Color32::WHITE });
            p.rect_stroke(
                b,
                2.0,
                Stroke::new(1.0, if on { Color32::from_rgb(0x10, 0x7C, 0x41) } else { Color32::from_gray(110) }),
                StrokeKind::Inside,
            );
            if on {
                crate::icons::paint(p, b.shrink(1.5), crate::icons::Icon::Check, Color32::WHITE);
            }
            continue;
        }
        if cfl.as_ref().is_some_and(|l| l.hide_value) || v.is_empty() {
            continue;
        }
        // Align and clip header text inside the content area, away from the dropdown.
        let filter_space = sh.filter_button_space(c) * z;
        let text_rect = Rect::from_min_max(rect.min, pos2((rect.right() - filter_space).max(rect.left()), rect.bottom()));
        let rect = &text_rect;
        let st = &lk.style;
        let size = st.font.size * 96.0 / 72.0 * z;
        let fam = theme::cell_family(&st.font.name, st.font.bold, st.font.italic);
        let font = FontId::new(size, fam);
        // Digits fit by the cell's own font and size, not the default 11pt (9pt marks in narrow columns).
        let char_w = p.layout_no_wrap("0".to_string(), font.clone(), Color32::PLACEHOLDER).size().x.max(1.0);
        let avail_w = rect.right() - text_left;
        let (mut text, ncolor, numeric, fill_char) = display_text(wb, sh, c, &v, st, avail_w / z.max(0.1) * z, char_w);
        if sh.show_formulas && sh.cell(c).is_some_and(|cell| cell.formula.is_some()) {
            text = crate::formula_locale::display(&text, crate::i18n::current(p.ctx()).formula_locale(), wb, si);
        }
        if text.is_empty() {
            continue;
        }
        let color = ncolor.unwrap_or(lk.text_color);
        let halign = match st.align.h {
            HAlign::General => match v {
                Value::Number(_) => HAlign::Right,
                Value::Bool(_) | Value::Error(_) => HAlign::Center,
                _ if numeric => HAlign::Right,
                _ => HAlign::Left,
            },
            h => h,
        };
        let indent = st.align.indent as f32 * 9.0 * z;
        let pad = 2.5 * z;
        let rotated = st.align.rotation != 0 && st.align.rotation != 255;
        // Rotated text wraps along its own direction (below), not across the column width.
        if !rotated && (st.align.wrap || st.align.h == HAlign::Justify) {
            let wrap_w = (rect.width() - 2.0 * pad - indent).max(4.0);
            let mut job = egui::text::LayoutJob::simple(text.clone(), font.clone(), color, wrap_w);
            job.halign = match halign {
                HAlign::Center | HAlign::CenterAcross => egui::Align::Center,
                HAlign::Right => egui::Align::RIGHT,
                _ => egui::Align::LEFT,
            };
            let galley = p.layout_job(job);
            let x = match halign {
                HAlign::Center | HAlign::CenterAcross => rect.center().x,
                HAlign::Right => rect.right() - pad,
                _ => text_left + pad + indent,
            };
            let y = wrapped_cell_text_y(st.align.v, *rect, galley.size().y, z);
            p.with_clip_rect(rect.intersect(p.clip_rect())).galley(pos2(x, y), galley, color);
            continue;
        }
        let first_line = text.lines().next().unwrap_or("").to_string();
        // Shrink to fit: too-wide content is drawn in a smaller font so it fits the cell (or the
        // merged area): it neither spills, nor is clipped, nor turns into ####.
        let small = (st.align.shrink && !rotated && st.align.h != HAlign::Fill && fill_char.is_none())
            .then(|| shrink_font(p, &first_line, &font, avail_w - 2.0 * pad - indent))
            .flatten();
        let shrunk = small.is_some();
        let font = small.unwrap_or(font);
        let mut galley = p.layout_no_wrap(first_line.clone(), font.clone(), color);
        let mut w = galley.size().x;
        // Numbers and dates never overflow: they show #### when too narrow.
        if !shrunk && (numeric || matches!(v, Value::Number(_))) && w > avail_w - 2.0 * pad && st.align.h != HAlign::Fill {
            let n = ((avail_w - 2.0 * pad) / char_w).floor().max(1.0) as usize;
            galley = p.layout_no_wrap("#".repeat(n), font.clone(), color);
            w = galley.size().x;
        }
        // Repeat fill (`*` in number formats) / Fill alignment.
        if let Some((ch, pos)) = fill_char
            && pos <= first_line.len()
        {
            let base_w = w;
            let cw = p.layout_no_wrap(ch.to_string(), font.clone(), color).size().x.max(1.0);
            let n = (((avail_w - 2.0 * pad) - base_w) / cw).floor().max(0.0) as usize;
            let mut s = first_line.clone();
            if first_line.is_char_boundary(pos) {
                s.insert_str(pos, &ch.to_string().repeat(n));
            }
            galley = p.layout_no_wrap(s, font.clone(), color);
            w = galley.size().x;
        }
        // Center Across Selection: the text is centred over this cell and the empty cells to its right that also use it.
        let across_end = if halign == HAlign::CenterAcross && !rotated && in_merge(c).is_none() {
            center_across_end(c.col, c1 + 20, |col| {
                let n = CellRef::new(c.row, col);
                empty_ok(n) && wb.styles.get(sh.style_id(n)).align.h == HAlign::CenterAcross
            })
        } else {
            c.col
        };
        // Overflow into empty neighbours for text (not upright text, whose width is its line height).
        let mut clip = *rect;
        let overflows = !numeric && !matches!(v, Value::Number(_)) && st.align.rotation.abs() != 90 && w > rect.width() - 2.0 * pad;
        if filter_space == 0.0 && (overflows || across_end > c.col) && in_merge(c).is_none() {
            let mut right = rect.right();
            let mut left = rect.left();
            if across_end > c.col {
                right = geo.x(sh, across_end + 1);
            }
            if matches!(halign, HAlign::Left | HAlign::Center | HAlign::Justify) {
                let mut cc = c.col + 1;
                while right - rect.left() < w + 2.0 * pad && cc < MAX_COLS && empty_ok(CellRef::new(c.row, cc)) && cc <= c1 + 20 {
                    right = geo.x(sh, cc + 1);
                    cc += 1;
                }
            }
            if matches!(halign, HAlign::Right | HAlign::Center) {
                let mut cc = c.col;
                while rect.right() - left < w + 2.0 * pad && cc > 0 && empty_ok(CellRef::new(c.row, cc - 1)) && cc + 20 > c0 {
                    cc -= 1;
                    left = geo.x(sh, cc);
                }
            }
            clip = Rect::from_min_max(pos2(left, rect.top()), pos2(right, rect.bottom()));
            // Paint over the gridlines the text passes through.
            if lk.fill.is_none() {
                let inner = Rect::from_min_max(pos2(clip.left() + 1.0, clip.top() + 0.5), pos2(clip.right() - 1.0, clip.bottom() - 1.0));
                let extra_l = Rect::from_min_max(inner.min, pos2(rect.left() + 1.0, inner.bottom()));
                let extra_r = Rect::from_min_max(pos2(rect.right() - 1.0, inner.top()), inner.max);
                if extra_l.width() > 0.0 {
                    p.rect_filled(extra_l, 0.0, t.grid_bg);
                }
                if extra_r.width() > 0.0 {
                    p.rect_filled(extra_r, 0.0, t.grid_bg);
                }
            }
        }
        let x = match halign {
            HAlign::CenterAcross => clip.center().x - w / 2.0,
            HAlign::Center | HAlign::Distributed => rect.center().x - w / 2.0,
            HAlign::Right => rect.right() - pad - w - indent,
            _ => text_left + pad + indent,
        };
        let gh = galley.size().y;
        let y = cell_text_y(st.align.v, *rect, gh, z);
        let cp = p.with_clip_rect(clip.intersect(p.clip_rect()));
        if rotated {
            let angle = -(st.align.rotation as f32).to_radians();
            // Upright (±90°) wrapped text wraps to the row height: lines then stack across the column.
            let galley = if st.align.wrap && st.align.rotation.abs() == 90 {
                let job = egui::text::LayoutJob::simple(text.clone(), font.clone(), color, (rect.height() - 2.0 * pad).max(4.0));
                p.layout_job(job)
            } else {
                galley.clone()
            };
            let pos = rotated_text_pos(*rect, galley.size(), angle, st.align.h, st.align.v, pad, indent);
            let mut shape = egui::epaint::TextShape::new(pos, galley, color);
            shape.angle = angle;
            cp.add(shape);
        } else {
            cp.galley(pos2(x, y), galley.clone(), color);
        }
        let baseline = y + gh * 0.82;
        match st.font.underline {
            Underline::None => {}
            Underline::Double | Underline::DoubleAccounting => {
                cp.line_segment([pos2(x, baseline + 1.0), pos2(x + w, baseline + 1.0)], Stroke::new(1.0, color));
                cp.line_segment([pos2(x, baseline + 3.0), pos2(x + w, baseline + 3.0)], Stroke::new(1.0, color));
            }
            _ => {
                cp.line_segment([pos2(x, baseline + 1.0), pos2(x + w, baseline + 1.0)], Stroke::new(1.0, color));
            }
        }
        if st.font.strike {
            cp.line_segment([pos2(x, y + gh * 0.55), pos2(x + w, y + gh * 0.55)], Stroke::new(1.0, color));
        }
    }
    // Borders (over fills and text).
    for (c, rect, lk, _) in &looks {
        let b = &lk.style.borders;
        let merge = merges.iter().find(|m| m.start == *c);
        let edges = match merge {
            // A merged area's outline lives on the cells along each of its edges (Excel writes the
            // right edge on the last column's cells and the bottom edge on the last row's), so each
            // edge is drawn cell by cell from the cell it belongs to.
            Some(m) => merged_edges(wb, si, sh, geo, *m, *rect, (r0, r1), (c0, c1), cf),
            None => vec![
                (b.top, [rect.left_top(), rect.right_top()]),
                (b.bottom, [rect.left_bottom(), rect.right_bottom()]),
                (b.left, [rect.left_top(), rect.left_bottom()]),
                (b.right, [rect.right_top(), rect.right_bottom()]),
            ],
        };
        for (line, pts) in edges {
            if line.is_none() {
                continue;
            }
            let col = col32(line.color, wb, Color32::BLACK);
            paint_border(p, pts, line.style, col);
        }
        if !b.diag_down.is_none() {
            paint_border(p, [rect.left_top(), rect.right_bottom()], b.diag_down.style, col32(b.diag_down.color, wb, Color32::BLACK));
        }
        if !b.diag_up.is_none() {
            paint_border(p, [rect.left_bottom(), rect.right_top()], b.diag_up.style, col32(b.diag_up.color, wb, Color32::BLACK));
        }
        // Comment / note indicator.
        if let Some(cm) = sh.comments.get(c) {
            let tri_col = if cm.threaded { Color32::from_rgb(0x88, 0x55, 0xC8) } else { Color32::from_rgb(0xD0, 0x2E, 0x2E) };
            let s = 6.0 * z.max(0.7);
            p.add(egui::Shape::convex_polygon(
                vec![rect.right_top(), pos2(rect.right(), rect.top() + s), pos2(rect.right() - s, rect.top())],
                tri_col,
                Stroke::NONE,
            ));
        }
    }
    // Sparklines.
    for sp in &sh.sparklines {
        if !range.contains(sp.cell) {
            continue;
        }
        let rect = geo.cell_rect(sh, sp.cell).shrink(3.0);
        let vals: Vec<Option<f64>> = match gridcraft_engine::calc::evaluate(wb, si, sp.cell, &sp.source) {
            Value::Array(a) => a.data.iter().map(Value::as_f64).collect(),
            v => vec![v.as_f64()],
        };
        let col = sp.color.resolve(&wb.theme).unwrap_or([0x2B, 0x7C, 0xD3]);
        let prims = gridcraft_chart::render_sparkline(sp.kind, &vals, [col[0], col[1], col[2], 255], sp.markers, rect.width(), rect.height());
        crate::chartview::paint_prims(p, rect.min, &prims, 1.0);
    }
    // Filter buttons.
    let mut buttons: Vec<(CellRef, bool)> = Vec::new();
    if let Some(af) = &sh.autofilter {
        for col in af.range.start.col..=af.range.end.col {
            let active = af.criteria.iter().any(|(o, _)| af.range.start.col + o == col);
            buttons.push((CellRef::new(af.range.start.row, col), active));
        }
    }
    for tb in &sh.tables {
        if tb.filter_button && tb.header_row {
            for col in tb.range.start.col..=tb.range.end.col {
                buttons.push((CellRef::new(tb.range.start.row, col), false));
            }
        }
    }
    for (c, active) in buttons {
        if !range.contains(c) {
            continue;
        }
        let r = filter_button_rect(geo, sh, c);
        p.rect_filled(r, 2.0, Color32::from_gray(250));
        p.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(170)), StrokeKind::Inside);
        crate::icons::paint(p, r.shrink(1.5), if active { crate::icons::Icon::Filter } else { crate::icons::Icon::Chevron }, Color32::from_gray(70));
    }
}

pub fn filter_button_rect(geo: &Geo, sh: &Sheet, c: CellRef) -> Rect {
    let rect = geo.cell_rect(sh, c);
    // Keep the button within the same header slot that AutoFit reserves.
    let pad = 2.0 * geo.z;
    let s = (rect.height() - 4.0).clamp(10.0, 18.0).min((sh.filter_button_space(c) * geo.z - pad).max(0.0));
    Rect::from_min_size(pos2(rect.right() - s - pad, rect.bottom() - s - pad), vec2(s, s))
}

/// The outline segments of merged area `m` drawn at `rect`: the top and left edges come from the
/// first row's and first column's cells, the bottom and right edges from the last row's and last
/// column's cells, one segment per visible cell (limited to the visible `rows` × `cols`).
#[allow(clippy::too_many_arguments)]
fn merged_edges(
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    geo: &Geo,
    m: RangeRef,
    rect: Rect,
    rows: (u32, u32),
    cols: (u32, u32),
    cf: &mut CfCache,
) -> Vec<(BorderLine, [Pos2; 2])> {
    let mut out = Vec::new();
    let (cs, ce) = (m.start.col.max(cols.0), m.end.col.min(cols.1));
    for col in cs..=ce {
        if sh.col_width(col) == 0.0 {
            continue;
        }
        let (x0, x1) = (geo.x(sh, col), geo.x(sh, col.saturating_add(1)));
        let top = look(wb, si, sh, CellRef::new(m.start.row, col), cf).0.style.borders.top;
        let bottom = look(wb, si, sh, CellRef::new(m.end.row, col), cf).0.style.borders.bottom;
        out.push((top, [pos2(x0, rect.top()), pos2(x1, rect.top())]));
        out.push((bottom, [pos2(x0, rect.bottom()), pos2(x1, rect.bottom())]));
    }
    let (rs, re) = (m.start.row.max(rows.0), m.end.row.min(rows.1));
    for row in rs..=re {
        if sh.row_height(row) == 0.0 {
            continue;
        }
        let (y0, y1) = (geo.y(sh, row), geo.y(sh, row.saturating_add(1)));
        let left = look(wb, si, sh, CellRef::new(row, m.start.col), cf).0.style.borders.left;
        let right = look(wb, si, sh, CellRef::new(row, m.end.col), cf).0.style.borders.right;
        out.push((left, [pos2(rect.left(), y0), pos2(rect.left(), y1)]));
        out.push((right, [pos2(rect.right(), y0), pos2(rect.right(), y1)]));
    }
    out
}

/// The smaller font that makes `text` fit in `room` points of width (Shrink to fit), or `None`
/// when it already fits at its own size.
fn shrink_font(p: &Painter, text: &str, font: &FontId, room: f32) -> Option<FontId> {
    let natural = p.layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER).size().x;
    let room = room.max(1.0);
    if natural.is_nan() || natural <= room {
        return None;
    }
    // Glyph advances don't scale exactly linearly (rounding), so leave a little slack.
    let size = (font.size * room / natural * 0.97).max(1.0);
    Some(FontId::new(size, font.family.clone()))
}

pub(crate) fn paint_border(p: &Painter, pts: [Pos2; 2], style: BorderStyle, col: Color32) {
    let w = style.width();
    let (a, b) = (pts[0], pts[1]);
    match style {
        BorderStyle::Double => {
            let horiz = (a.y - b.y).abs() < 0.5;
            let off = if horiz { vec2(0.0, 1.5) } else { vec2(1.5, 0.0) };
            p.line_segment([a - off, b - off], Stroke::new(1.0, col));
            p.line_segment([a + off, b + off], Stroke::new(1.0, col));
        }
        BorderStyle::Dashed
        | BorderStyle::MediumDashed
        | BorderStyle::DashDot
        | BorderStyle::MediumDashDot
        | BorderStyle::DashDotDot
        | BorderStyle::MediumDashDotDot
        | BorderStyle::SlantDashDot => {
            p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(w, col), 4.0, 2.0));
        }
        BorderStyle::Dotted | BorderStyle::Hair => {
            p.extend(egui::Shape::dotted_line(&[a, b], col, 2.0, w * 0.5));
        }
        _ => {
            p.line_segment([a, b], Stroke::new(w, col));
        }
    }
}

fn paint_cf_icon(p: &Painter, r: Rect, set: &str, idx: usize) {
    let n = gridcraft_engine::cf::icon_count(set);
    let palette3 = [Color32::from_rgb(0xD1, 0x3B, 0x2F), Color32::from_rgb(0xF2, 0xB8, 0x2E), Color32::from_rgb(0x2E, 0x9E, 0x4F)];
    let col = if n <= 3 {
        palette3.get(idx).copied().unwrap_or(Color32::GRAY)
    } else {
        let t = idx as f32 / (n - 1).max(1) as f32;
        Color32::from_rgb(
            (0xD1 as f32 * (1.0 - t) + 0x2E as f32 * t) as u8,
            (0x3B as f32 * (1.0 - t) + 0x9E as f32 * t) as u8,
            (0x2F as f32 * (1.0 - t) + 0x4F as f32 * t) as u8,
        )
    };
    let lower = set.to_ascii_lowercase();
    if lower.contains("arrow") {
        let c = r.center();
        let s = r.width() / 2.0;
        let pts = if idx + 1 == n {
            vec![pos2(c.x, c.y - s), pos2(c.x + s, c.y + 0.1 * s), pos2(c.x - s, c.y + 0.1 * s)]
        } else if idx == 0 {
            vec![pos2(c.x, c.y + s), pos2(c.x - s, c.y - 0.1 * s), pos2(c.x + s, c.y - 0.1 * s)]
        } else {
            vec![pos2(c.x + s, c.y), pos2(c.x - 0.1 * s, c.y - s), pos2(c.x - 0.1 * s, c.y + s)]
        };
        p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    } else if lower.contains("rating") || lower.contains("quarter") {
        p.circle_stroke(r.center(), r.width() / 2.0 - 0.5, Stroke::new(1.0, Color32::GRAY));
        let frac = idx as f32 / (n - 1).max(1) as f32;
        p.rect_filled(
            Rect::from_min_size(pos2(r.left() + 2.0, r.bottom() - 2.0 - (r.height() - 4.0) * frac), vec2(r.width() - 4.0, (r.height() - 4.0) * frac)),
            1.0,
            Color32::from_rgb(0x2B, 0x7C, 0xD3),
        );
    } else {
        p.circle_filled(r.center(), r.width() / 2.0 - 0.5, col);
    }
}

/// Where the active cell is on screen: the whole merged cell when it is part of one.
fn active_rect(geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection) -> Rect {
    match sh.merge_at(sel.active) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, sel.active),
    }
}

/// The rectangles shaded for a selection: every area except the active cell, which stays clear so
/// it is plain where typing goes. A single selected cell has no fill. `outline` is where the
/// current area is drawn this frame: it glides to a new selection, and its fill with it.
pub fn selection_fill(geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection, outline: Rect) -> Vec<Rect> {
    selection_fill_where(geo, sh, sel, outline, |_| true)
}

/// `selection_fill` for the areas `keep` accepts (a frozen pane draws only the areas it shows).
fn selection_fill_where(geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection, outline: Rect, keep: impl Fn(&RangeRef) -> bool) -> Vec<Rect> {
    if sel.is_single_cell() {
        return vec![];
    }
    let active = active_rect(geo, sh, sel);
    let area = |r: &RangeRef| if *r == sel.current() { outline } else { geo.range_rect(sh, *r) };
    sel.ranges.iter().filter(|r| keep(r)).flat_map(|r| rect_around(area(r), active)).collect()
}

/// `area` without `hole`: the strips above and below the hole, then what is left beside it. The
/// selection fill is translucent, so the active cell can't be cleared by painting over it; the
/// pieces never overlap for the same reason. Pieces with no size, or whose numbers aren't finite,
/// are dropped.
fn rect_around(area: Rect, hole: Rect) -> impl Iterator<Item = Rect> {
    // `intersect` takes the sane side of a NaN, which would make a broken hole cover the area.
    let hole = if hole.is_finite() { area.intersect(hole) } else { Rect::NOTHING };
    let parts = if hole.is_positive() {
        [
            Rect::from_min_max(area.min, pos2(area.max.x, hole.min.y)),
            Rect::from_min_max(pos2(area.min.x, hole.max.y), area.max),
            Rect::from_min_max(pos2(area.min.x, hole.min.y), pos2(hole.min.x, hole.max.y)),
            Rect::from_min_max(pos2(hole.max.x, hole.min.y), pos2(area.max.x, hole.max.y)),
        ]
    } else {
        [area, Rect::NOTHING, Rect::NOTHING, Rect::NOTHING]
    };
    parts.into_iter().filter(|r| r.is_positive() && r.is_finite())
}

/// True when `r` covers at least one cell of the pane that shows `rows` x `cols` (inclusive).
/// The panes of a frozen sheet share one coordinate system, so a range that lives only in the
/// scrolling pane would otherwise be drawn over the frozen pane as well.
fn range_in_pane(r: RangeRef, rows: (u32, u32), cols: (u32, u32)) -> bool {
    r.start.row <= rows.1 && r.end.row >= rows.0 && r.start.col <= cols.1 && r.end.col >= cols.0
}

/// The four sides of a `width` wide outline centred on `rect`, cut to `clip` (a little beyond it).
///
/// A stroked rectangle that spans a whole column (20 million points tall) loses its precision in
/// the tessellator and its left and right sides vanish; sides filled from rectangles that stay
/// near the viewport do not. Sides wholly outside `clip` are left out.
pub(crate) fn outline_bands(rect: Rect, width: f32, clip: Rect) -> Vec<Rect> {
    if !rect.is_finite() || !clip.is_finite() {
        return Vec::new();
    }
    let h = width / 2.0;
    let clip = clip.expand(width * 2.0);
    let sides = [
        Rect::from_min_max(pos2(rect.left() - h, rect.top() - h), pos2(rect.left() + h, rect.bottom() + h)),
        Rect::from_min_max(pos2(rect.right() - h, rect.top() - h), pos2(rect.right() + h, rect.bottom() + h)),
        Rect::from_min_max(pos2(rect.left() - h, rect.top() - h), pos2(rect.right() + h, rect.top() + h)),
        Rect::from_min_max(pos2(rect.left() - h, rect.bottom() - h), pos2(rect.right() + h, rect.bottom() + h)),
    ];
    sides.into_iter().map(|s| s.intersect(clip)).filter(|s| s.is_finite() && s.width() > 0.0 && s.height() > 0.0).collect()
}

/// `outline` is where the current area's border is drawn this frame (see `selection_glide`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_selection(
    p: &Painter,
    geo: &Geo,
    sh: &Sheet,
    sel: &gridcraft_engine::Selection,
    outline: Rect,
    t: &Tokens,
    app: &SheetApp,
    rows: (u32, u32),
    cols: (u32, u32),
) {
    // Fill all areas except the active cell.
    for rect in selection_fill_where(geo, sh, sel, outline, |r| range_in_pane(*r, rows, cols)) {
        p.rect_filled(rect, 0.0, t.sel_fill);
    }
    let active_range = sh.merge_at(sel.active).unwrap_or_else(|| RangeRef::cell(sel.active));
    let active = active_rect(geo, sh, sel);
    let active_here = range_in_pane(active_range, rows, cols);
    let cur_here = range_in_pane(sel.current(), rows, cols);
    // Editing a formula: outline its references instead of the selection.
    if let Some(ed) = &app.editor
        && ed.is_formula()
    {
        for (_, _, r, sheet, idx) in crate::editor::formula_refs(&ed.text) {
            if sheet.as_deref().is_some_and(|s| !s.eq_ignore_ascii_case(&sh.name)) {
                continue;
            }
            if !range_in_pane(r, rows, cols) {
                continue;
            }
            let rr = geo.range_rect(sh, r);
            let col = REF_COLORS[idx];
            p.rect_filled(rr, 0.0, col.gamma_multiply(0.12));
            p.rect_stroke(rr, 0.0, Stroke::new(1.5, col), StrokeKind::Inside);
            for corner in [rr.left_top(), rr.right_top(), rr.left_bottom(), rr.right_bottom()] {
                p.rect_filled(Rect::from_center_size(corner, vec2(4.0, 4.0)), 0.0, col);
            }
        }
    }
    if cur_here {
        for side in outline_bands(outline.expand(0.5), 2.0, p.clip_rect()) {
            p.rect_filled(side, 0.0, t.sel_border);
        }
    }
    if active_here && sel.ranges.len() > 1 {
        p.rect_stroke(active, 0.0, Stroke::new(1.0, t.sel_border), StrokeKind::Inside);
    }
    // Fill handle (bottom-right of the current area).
    if app.editor.is_none() && range_in_pane(RangeRef::cell(sel.current().end), rows, cols) {
        let h = Rect::from_center_size(outline.right_bottom(), vec2(6.0, 6.0));
        p.rect_filled(h.expand(1.0), 0.0, Color32::WHITE);
        p.rect_filled(h, 0.0, t.sel_border);
    }
    // Fill-handle drag preview.
    if let crate::grid::Drag::Fill { target: Some(tr) } = &app.grid.drag
        && range_in_pane(*tr, rows, cols)
    {
        let rr = geo.range_rect(sh, *tr);
        p.extend(egui::Shape::dashed_line(
            &[rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()],
            Stroke::new(1.5, Color32::from_gray(90)),
            4.0,
            3.0,
        ));
    }
    if let crate::grid::Drag::Move { to: Some(to), .. } = &app.grid.drag {
        let r = sel.current();
        let moved = RangeRef::new(*to, to.offset_clamped(r.height() as i64 - 1, r.width() as i64 - 1));
        if range_in_pane(moved, rows, cols) {
            let rr = geo.range_rect(sh, moved);
            p.rect_stroke(rr, 0.0, Stroke::new(2.0, Color32::from_gray(110)), StrokeKind::Middle);
        }
    }
    if let crate::grid::Drag::MoveCols { before: Some(before), .. } = &app.grid.drag {
        let x = geo.x(sh, *before);
        p.line_segment([pos2(x, geo.rect.top()), pos2(x, geo.rect.bottom())], Stroke::new(3.0, t.accent));
    }
    // Marching ants around the copied range.
    if let Some(clip) = &app.session.clipboard
        && app.session.active().is_some_and(|d| d.uid == clip.doc_uid && d.wb.active_sheet == clip.sheet)
        && range_in_pane(clip.range, rows, cols)
    {
        // A multi-area copy marks each area as selected.
        let areas = if clip.areas.is_empty() { std::slice::from_ref(&clip.range) } else { clip.areas.as_slice() };
        for area in areas {
            let rr = geo.range_rect(sh, *area).expand(0.5);
            let phase = app.grid.marching_phase;
            let pts = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
            p.extend(egui::Shape::dashed_line_with_offset(&pts, Stroke::new(2.0, t.sel_border), &[4.0], &[4.0], phase));
        }
    }
}

pub(crate) fn paint_headers(
    p: &Painter,
    geo: &Geo,
    sh: &Sheet,
    sel: &gridcraft_engine::Selection,
    t: &Tokens,
    quads: &[((u32, u32), (u32, u32), Rect)],
) {
    let font = theme::ui_font((11.5 * (geo.z / DISPLAY_SCALE).max(0.75)).min(16.0));
    // The headers' own area: inside the outline bands.
    let r = Rect::from_min_max(pos2(geo.head_left(), geo.head_top()), geo.rect.max);
    let top = Rect::from_min_max(pos2(geo.cells.left(), r.top()), pos2(r.right(), geo.cells.top()));
    let left = Rect::from_min_max(pos2(r.left(), geo.cells.top()), pos2(geo.cells.left(), r.bottom()));
    p.rect_filled(top, 0.0, t.header_bg);
    p.rect_filled(left, 0.0, t.header_bg);
    let bounds = sel.bounds();
    let col_sel = |c: u32| sel.ranges.iter().any(|x| c >= x.start.col && c <= x.end.col);
    let row_sel = |rr: u32| sel.ranges.iter().any(|x| rr >= x.start.row && rr <= x.end.row);
    let full_col = |c: u32| sel.ranges.iter().any(|x| x.is_full_cols() && c >= x.start.col && c <= x.end.col);
    let full_row = |rr: u32| sel.ranges.iter().any(|x| x.is_full_rows() && rr >= x.start.row && rr <= x.end.row);
    let _ = bounds;
    let mut done_cols = std::collections::HashSet::new();
    let mut done_rows = std::collections::HashSet::new();
    for (rows, cols, clip) in quads {
        let pc = p.with_clip_rect(Rect::from_min_max(pos2(clip.left(), top.top()), pos2(clip.right(), top.bottom())));
        if cols.0 <= cols.1 {
            for c in cols.0..=cols.1 {
                if !done_cols.insert((c, clip.left() as i32)) {
                    continue;
                }
                let w = sh.col_width(c) * geo.z;
                if w <= 0.0 {
                    continue;
                }
                let x = geo.x(sh, c);
                let cr = Rect::from_min_size(pos2(x, top.top()), vec2(w, top.height()));
                if col_sel(c) {
                    pc.rect_filled(cr, 0.0, if full_col(c) { t.header_all_bg } else { t.header_sel_bg });
                    pc.line_segment([pos2(cr.left(), cr.bottom() - 1.0), pos2(cr.right(), cr.bottom() - 1.0)], Stroke::new(2.0, t.accent));
                }
                pc.line_segment([pos2(cr.right() - 0.5, cr.top() + 3.0), pos2(cr.right() - 0.5, cr.bottom())], Stroke::new(1.0, t.header_line));
                let color = if col_sel(c) { t.header_sel_text } else { t.header_text };
                pc.text(cr.center(), Align2::CENTER_CENTER, col_to_letters(c), font.clone(), color);
            }
        }
        let pr = p.with_clip_rect(Rect::from_min_max(pos2(left.left(), clip.top()), pos2(left.right(), clip.bottom())));
        if rows.0 <= rows.1 {
            for rr in rows.0..=rows.1 {
                if !done_rows.insert((rr, clip.top() as i32)) {
                    continue;
                }
                let h = sh.row_height(rr) * geo.z;
                if h <= 0.0 {
                    continue;
                }
                let y = geo.y(sh, rr);
                let cr = Rect::from_min_size(pos2(left.left(), y), vec2(left.width(), h));
                if row_sel(rr) {
                    pr.rect_filled(cr, 0.0, if full_row(rr) { t.header_all_bg } else { t.header_sel_bg });
                    pr.line_segment([pos2(cr.right() - 1.0, cr.top()), pos2(cr.right() - 1.0, cr.bottom())], Stroke::new(2.0, t.accent));
                }
                pr.line_segment([pos2(cr.left() + 3.0, cr.bottom() - 0.5), pos2(cr.right(), cr.bottom() - 0.5)], Stroke::new(1.0, t.header_line));
                let color = if row_sel(rr) { t.header_sel_text } else { t.header_text };
                pr.text(cr.center(), Align2::CENTER_CENTER, (rr as u64 + 1).to_string(), font.clone(), color);
            }
        }
    }
    // Corner (select all) and edges.
    let corner = Rect::from_min_max(r.min, geo.cells.min);
    p.rect_filled(corner, 0.0, t.header_bg);
    let tri = vec![
        pos2(corner.right() - 3.0, corner.top() + 5.0),
        pos2(corner.right() - 3.0, corner.bottom() - 3.0),
        pos2(corner.left() + 7.0, corner.bottom() - 3.0),
    ];
    p.add(egui::Shape::convex_polygon(tri, Color32::from_gray(190), Stroke::NONE));
    p.line_segment([pos2(r.left(), geo.cells.top() - 0.5), pos2(r.right(), geo.cells.top() - 0.5)], Stroke::new(1.0, t.header_line));
    p.line_segment([pos2(geo.cells.left() - 0.5, r.top()), pos2(geo.cells.left() - 0.5, r.bottom())], Stroke::new(1.0, t.header_line));
}

// ------------------------------------------------------------------ interaction

/// How close (in points) a press must be to an object's bottom-right corner to resize it instead of moving it.
const OBJECT_RESIZE_RADIUS: f32 = 10.0;

fn interact(app: &mut SheetApp, ui: &mut egui::Ui, resp: &egui::Response, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    // Text selection and editing must not also move the object or edit worksheet cells.
    if app.text_box_editor.is_some() || app.grid.hf_edit.is_some() {
        return;
    }
    let ctx = ui.ctx().clone();
    // Enter/Space synthesize egui clicks; only real pointer clicks use sheet coordinates.
    let primary_clicked = resp.clicked_by(egui::PointerButton::Primary);
    let pointer = ctx.input(|i| i.pointer.clone());
    let mods = ctx.input(|i| i.modifiers);
    // Hit-testing works in logical coordinates (see `Geo::rtl`).
    let pos = pointer.interact_pos().or(pointer.hover_pos()).map(|p| geo.mirror_pos(p));
    let in_cells = pos.is_some_and(|p| geo.in_cells(p));
    let in_col_header = pos.is_some_and(|p| p.y < geo.cells.top() && p.y >= geo.head_top() && p.x >= geo.cells.left());
    let in_row_header = pos.is_some_and(|p| p.x < geo.cells.left() && p.x >= geo.head_left() && p.y >= geo.cells.top());
    let in_corner = pos.is_some_and(|p| p.x < geo.cells.left() && p.y < geo.cells.top() && p.x >= geo.head_left() && p.y >= geo.head_top());
    let sel = app.session.active().map(|d| d.selection.clone()).unwrap_or_default();
    let cur_rect = geo.range_rect(sh, sel.current());
    let handle = Rect::from_center_size(cur_rect.right_bottom(), vec2(9.0, 9.0));
    // The fill handle and the move border only react in the panes where they are painted, not
    // over the frozen rows/columns that a scrolled selection lies under.
    let handle_panes = geo.range_panes(RangeRef::cell(sel.current().end));
    let border_panes = geo.range_panes(sel.current());
    let on_handle = |p: Pos2| handle.contains(p) && handle_panes.iter().any(|r| r.contains(p));
    let on_cur_border = |p: Pos2| on_border(cur_rect, p) && border_panes.iter().any(|r| r.contains(p));

    let (freeze_cols, freeze_rows) = freeze_handles(geo);
    // Hover cursors.
    if resp.hovered()
        && let Some(p) = pos
        && (freeze_cols.contains(p) || freeze_rows.contains(p) || matches!(app.grid.drag, Drag::Freeze { .. }))
    {
        let cols = match app.grid.drag {
            Drag::Freeze { cols, .. } => cols,
            _ => freeze_cols.contains(p),
        };
        ctx.set_cursor_icon(if cols { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical });
        if app.grid.drag == Drag::None && !pointer.any_down() {
            let tip = if cols { "Drag to freeze columns" } else { "Drag to freeze rows" };
            resp.clone().on_hover_text_at_pointer(crate::i18n::current(&ctx).tr(tip));
        }
    } else if resp.hovered()
        && let Some(p) = pos
    {
        let near_col_edge = in_col_header && col_edge(geo, sh, p.x).is_some();
        let near_row_edge = in_row_header && row_edge(geo, sh, p.y).is_some();
        let selected_object = crate::chartview::hit(app, geo, sh, p).filter(|&(_, id, _)| app.selected_chart == Some(id));
        let near_corner = selected_object.is_some_and(|(_, _, rect)| (p - rect.right_bottom()).length() < OBJECT_RESIZE_RADIUS);
        let near_object_corner = app.grid.drag == Drag::None && near_corner;
        if near_object_corner || matches!(app.grid.drag, Drag::Object { resize: true, .. }) {
            ctx.set_cursor_icon(CursorIcon::ResizeNwSe);
        } else if near_col_edge {
            ctx.set_cursor_icon(CursorIcon::ResizeColumn);
        } else if near_row_edge {
            ctx.set_cursor_icon(CursorIcon::ResizeRow);
        } else if on_handle(p) && app.editor.is_none() {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
            if app.grid.drag == Drag::None && !pointer.any_down() {
                resp.clone()
                    .on_hover_text_at_pointer(crate::i18n::current(&ctx).tr("Drag to fill cells. Drag from inside a cell to select a range."));
            }
        } else if in_cells && app.editor.is_none() && on_cur_border(p) {
            ctx.set_cursor_icon(CursorIcon::Move);
        } else if in_cells {
            ctx.set_cursor_icon(CursorIcon::Cell);
        } else if in_col_header {
            let col = geo.col_at(sh, p.x);
            let selected = sel.ranges.len() == 1 && sel.current().is_full_cols() && sel.current().contains(CellRef::new(0, col));
            ctx.set_cursor_icon(if selected { CursorIcon::Move } else { CursorIcon::Default });
        }
        app.grid.hover_cell = in_cells.then(|| geo.cell_at(sh, p));
    }

    // Draw tab tools: the pen records a stroke, the eraser deletes ink under the pointer.
    if !app.session.draw_tool.is_empty() && in_cells {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
        if app.session.draw_tool == "pen" {
            if let Some(p) = pos
                && resp.dragged()
            {
                // Through the cell under the pointer, so frozen panes and pages map right.
                let c = geo.cell_at(sh, p);
                let sx = sh.col_left(c.col) as f32 + (p.x - geo.x(sh, c.col)) / geo.z;
                let sy = sh.row_top(c.row) as f32 + (p.y - geo.y(sh, c.row)) / geo.z;
                app.grid.ink.push([sx, sy]);
            }
            if !app.grid.ink.is_empty() {
                // Live preview.
                let pts: Vec<Pos2> = app
                    .grid
                    .ink
                    .iter()
                    .map(|q| {
                        let (col, row) = (sh.col_at(q[0].max(0.0) as f64), sh.row_at(q[1].max(0.0) as f64));
                        geo.mirror_pos(pos2(
                            geo.x(sh, col) + (q[0] - sh.col_left(col) as f32) * geo.z,
                            geo.y(sh, row) + (q[1] - sh.row_top(row) as f32) * geo.z,
                        ))
                    })
                    .collect();
                ui.painter().add(egui::epaint::PathShape::line(pts, egui::epaint::PathStroke::new(2.0 * geo.z, Color32::from_rgb(0x1F, 0x5F, 0xC9))));
            }
            if resp.drag_stopped() && app.grid.ink.len() >= 2 {
                let pts: Vec<[f32; 2]> = std::mem::take(&mut app.grid.ink);
                let _ = app.run("draw.stroke", json!({"points": pts}));
            }
            return;
        }
        if app.session.draw_tool == "eraser" && (primary_clicked || resp.dragged()) {
            if let Some(p) = pos
                && let Some(("shape", id, _)) = crate::chartview::hit(app, geo, sh, p)
                && sh.shapes.iter().any(|s| s.id == id && s.kind == gridcraft_engine::model::ShapeKind::Ink)
            {
                let _ = app.run("object.delete", json!({"kind": "shape", "id": id}));
            }
            return;
        }
    }
    // A plain click on a slicer value tile toggles it (Ctrl/Cmd for multi-select); the clear
    // button resets it. Runs before the generic object select/drag so tiles stay clickable.
    if primary_clicked
        && !resp.drag_started()
        && let Some(p) = pos
        && let Some((id, act)) = crate::chartview::slicer_click(geo, sh, wb, wb.active_sheet, p)
    {
        app.selected_chart = Some(id);
        match act {
            crate::chartview::SlicerAction::Toggle(value) => {
                let _ = app.run("slicer.toggle", json!({"slicer": id, "value": value, "multi": mods.command || mods.ctrl}));
            }
            crate::chartview::SlicerAction::Clear => {
                let _ = app.run("slicer.clear", json!({"slicer": id}));
            }
        }
        return;
    }
    // Objects (charts, pictures, shapes, slicers) take clicks first.
    if resp.drag_started() || primary_clicked {
        // A drag is recognised only after the pointer has moved, so grab the object where the press began.
        let at = if resp.drag_started() { pointer.press_origin().or(pos) } else { pos };
        if let Some(p) = at
            && let Some((kind, id, rect)) = crate::chartview::hit(app, geo, sh, p)
        {
            app.selected_chart = Some(id);
            if kind == "shape"
                && resp.double_clicked()
                && sh.shapes.iter().any(|s| s.id == id && s.kind == gridcraft_engine::model::ShapeKind::TextBox)
            {
                app.begin_text_box_edit(id);
                return;
            }
            if kind == "chart" && resp.double_clicked() {
                app.grid.pane = Some("formatChart".into());
            }
            if resp.drag_started() {
                let a = object_anchor(sh, kind, id);
                let resize = (p - rect.right_bottom()).length() < OBJECT_RESIZE_RADIUS;
                if let Some(a) = a {
                    app.grid.drag = Drag::Object { kind, id, start: p, orig: (a.cell, a.dx, a.dy, a.width, a.height), resize };
                }
            }
            return;
        } else if primary_clicked || resp.drag_started() {
            app.selected_chart = None;
        }
    }

    // A double-click is also a click: handle header AutoFit before resize setup returns.
    if resp.double_clicked()
        && let Some(p) = pos
    {
        let autofit = if in_col_header {
            col_edge(geo, sh, p.x).map(|(col, _)| {
                let selected = sel.ranges.iter().any(|r| r.is_full_cols() && col >= r.start.col && col <= r.end.col);
                ("home.autofitColumnWidth", if selected { json!({}) } else { json!({"cols": RangeRef::cols(col, col).a1()}) })
            })
        } else if in_row_header {
            row_edge(geo, sh, p.y).map(|(row, _)| {
                let selected = sel.ranges.iter().any(|r| r.is_full_rows() && row >= r.start.row && row <= r.end.row);
                ("home.autofitRowHeight", if selected { json!({}) } else { json!({"rows": RangeRef::rows(row, row).a1()}) })
            })
        } else {
            None
        };
        if let Some((command, params)) = autofit {
            app.grid.drag = Drag::None;
            app.grid.drag_select = false;
            app.run_or_alert(command, params);
            resp.request_focus();
            return;
        }
    }

    // Double-click the fill handle: a double-click is also a click, so run it before the handle press returns.
    if resp.double_clicked()
        && let Some(p) = pos
        && on_handle(p)
    {
        // Fill down as far as the neighbouring column goes.
        let src = sel.current();
        let neighbour = if src.start.col > 0 { src.start.col - 1 } else { src.end.col + 1 };
        let mut last = src.end.row;
        while last + 1 < MAX_ROWS && sh.value_ref(CellRef::new(last + 1, neighbour)).is_some_and(|v| !v.is_empty()) && last - src.end.row < 1_000_000
        {
            last += 1;
        }
        if last > src.end.row {
            let target = RangeRef::new(src.start, CellRef::new(last, src.end.col));
            let _ = app.run("edit.autoFill", json!({"source": src.a1(), "target": target.a1()}));
        }
        app.grid.drag = Drag::None;
        app.grid.drag_select = false;
        resp.request_focus();
        return;
    }

    // Press: decide the drag mode.
    if resp.drag_started() || (primary_clicked && app.grid.drag == Drag::None) {
        // Where the press began (a drag is only recognised after the pointer has moved).
        let Some(p) = pointer.press_origin().map(|p| geo.mirror_pos(p)).or(pos) else { return };
        // The outline band's − / + and level buttons act on release, like buttons.
        if let Some(action) = crate::outline::hit(geo, sh, p) {
            if primary_clicked {
                let (command, params) = action.command();
                app.run_or_alert(command, params);
            }
            return;
        }
        // Freeze handles sit on the header edges (and the corner's edges when nothing is frozen).
        if freeze_cols.contains(p) || freeze_rows.contains(p) {
            app.grid.drag = Drag::Freeze { cols: freeze_cols.contains(p), to: None };
            return;
        }
        if in_corner {
            let _ = app.session.run("selection.set", json!({"range": RangeRef::all().a1()}));
            return;
        }
        if in_col_header {
            if let Some((c, x)) = col_edge(geo, sh, p.x) {
                app.grid.drag = Drag::ResizeCol { col: c, start: p.x, orig: sh.col_width(c).max(0.0) * 0.0 + (x - geo.x(sh, c)) / geo.z };
                return;
            }
            let c = geo.col_at(sh, p.x);
            if point_header(app, wb, sh, RangeRef::cols(c, c)) {
                return;
            }
            if resp.drag_started()
                && !mods.any()
                && sel.ranges.len() == 1
                && sel.current().is_full_cols()
                && sel.current().contains(CellRef::new(0, c))
            {
                app.grid.drag = Drag::MoveCols { source: sel.current(), before: None };
                return;
            }
            let range = if mods.shift { RangeRef::cols(sel.anchor.col.min(c), sel.anchor.col.max(c)) } else { RangeRef::cols(c, c) };
            select_ranges(app, range, mods.command, CellRef::new(geo.row_at(sh, geo.cells.top() + 1.0), c));
            app.grid.drag = Drag::Cols(if mods.shift { sel.anchor.col } else { c });
            return;
        }
        if in_row_header {
            if let Some((r, y)) = row_edge(geo, sh, p.y) {
                app.grid.drag = Drag::ResizeRow { row: r, start: p.y, orig: (y - geo.y(sh, r)) / geo.z };
                return;
            }
            let r = geo.row_at(sh, p.y);
            if point_header(app, wb, sh, RangeRef::rows(r, r)) {
                return;
            }
            let range = if mods.shift { RangeRef::rows(sel.anchor.row.min(r), sel.anchor.row.max(r)) } else { RangeRef::rows(r, r) };
            select_ranges(app, range, mods.command, CellRef::new(r, geo.col_at(sh, geo.cells.left() + 1.0)));
            app.grid.drag = Drag::Rows(if mods.shift { sel.anchor.row } else { r });
            return;
        }
        if !in_cells {
            return;
        }
        let c = geo.cell_at(sh, p);
        // Data validation list arrow next to the active cell.
        if let Some(r) = validation_arrow(sh, geo, sel.active)
            && r.contains(p)
        {
            app.grid.list_picker = Some(sel.active);
            return;
        }
        // Filter dropdown buttons.
        if is_filter_button(sh, geo, p) {
            app.grid.filter_menu = Some((c.col, c.row, geo.mirror_pos(p)));
            return;
        }
        // Point mode while editing a formula: clicking inserts a reference.
        if let Some(ed) = app.editor.as_mut()
            && ed.can_point()
        {
            let sheet_name = (ed.sheet != wb.active_sheet).then(|| sh.name.clone());
            let r = match sheet_name {
                Some(n) => format!("{}!{}", gridcraft_engine::formula::quote_sheet(&n), c.a1()),
                None => c.a1(),
            };
            ed.insert_ref(&r);
            ed.request_focus = true;
            ed.point_cell = Some(c);
            app.grid.drag = Drag::Select;
            app.grid.drag_select = true;
            if resp.drag_started()
                && let Some(p) = pos
            {
                select_to_pointer(app, geo, sh, wb, p);
            }
            return;
        }
        if app.editor.is_some() && !app.commit_edit(0, 0, false, false) {
            return;
        }
        if on_handle(p) {
            app.grid.drag = Drag::Fill { target: None };
            return;
        }
        if on_cur_border(p) && resp.drag_started() {
            app.grid.drag = Drag::Move { grab: c, to: None };
            return;
        }
        // Double-click edits.
        let now = crate::now_ms();
        if let Some((last, at)) = app.grid.last_click
            && last == c
            && now - at < 450.0
            && !mods.shift
        {
            app.grid.last_click = None;
            // Following a hyperlink requires a single click in Excel; double-click edits.
            app.begin_edit(None, false);
            return;
        }
        app.grid.last_click = Some((c, now));
        if mods.shift {
            let _ = app.session.run("selection.set", json!({"range": RangeRef::new(sel.anchor, c).a1(), "active": sel.active.a1()}));
            if let Some(d) = app.session.active_mut() {
                d.selection.anchor = sel.anchor;
            }
        } else if mods.command {
            select_ranges(app, RangeRef::cell(c), true, c);
        } else {
            let _ = app.session.run("selection.set", json!({"cell": c.a1()}));
            if primary_clicked && wb.styles.get(sh.style_id(c)).num_fmt.as_str() == "checkbox" {
                let _ = app.run("cell.toggleCheckbox", json!({"cell": c.a1()}));
            }
            // A click on a hyperlink's text follows it (like Excel); elsewhere in the cell selects.
            if let Some(h) = sh.hyperlinks.get(&c).cloned()
                && primary_clicked
                && !mods.any()
            {
                let r = geo.cell_rect(sh, c);
                if p.x < r.left() + r.width() * 0.8 {
                    follow_link(app, &h.target);
                }
            }
        }
        app.grid.drag = Drag::Select;
        app.grid.drag_select = true;
        if resp.drag_started()
            && let Some(p) = pos
        {
            // The first recognised drag already includes movement away from the press.
            select_to_pointer(app, geo, sh, wb, p);
        }
        return;
    }

    // Dragging.
    if resp.dragged()
        && let Some(p) = pos
    {
        // Auto-scroll near edges.
        let edge = 24.0;
        let mut auto = egui::Vec2::ZERO;
        if p.y > geo.cells.bottom() - edge {
            auto.y = 20.0;
        }
        if p.y < geo.cells.top() + edge * 0.5 && !matches!(app.grid.drag, Drag::ResizeRow { .. }) {
            auto.y = -20.0;
        }
        if p.x > geo.cells.right() - edge {
            auto.x = 30.0;
        }
        if p.x < geo.cells.left() + edge * 0.5 && !matches!(app.grid.drag, Drag::ResizeCol { .. }) {
            auto.x = -30.0;
        }
        let pc = CellRef::new(
            geo.row_at(sh, p.y.clamp(geo.cells.top() + 1.0, geo.cells.bottom() - 1.0)),
            geo.col_at(sh, p.x.clamp(geo.cells.left() + 1.0, geo.cells.right() - 1.0)),
        );
        match app.grid.drag.clone() {
            Drag::Select => {
                select_to_pointer(app, geo, sh, wb, p);
                scroll_by(app, auto);
            }
            Drag::Rows(anchor) => {
                let r = geo.row_at(sh, p.y);
                let _ =
                    app.session.run("selection.set", json!({"range": RangeRef::rows(anchor.min(r), anchor.max(r)).a1(), "active": sel.active.a1()}));
                scroll_by(app, vec2(0.0, auto.y));
            }
            Drag::Cols(anchor) => {
                let c = geo.col_at(sh, p.x);
                let _ =
                    app.session.run("selection.set", json!({"range": RangeRef::cols(anchor.min(c), anchor.max(c)).a1(), "active": sel.active.a1()}));
                scroll_by(app, vec2(auto.x, 0.0));
            }
            Drag::MoveCols { source, .. } => {
                let col = geo.col_at(sh, p.x.clamp(geo.cells.left() + 1.0, geo.cells.right() - 1.0));
                let midpoint = geo.x(sh, col) + sh.col_width(col) * geo.z * 0.5;
                let before = (col + u32::from(p.x >= midpoint)).min(MAX_COLS - 1);
                let before = (!(source.start.col..=source.end.col + 1).contains(&before)).then_some(before);
                app.grid.drag = Drag::MoveCols { source, before };
                scroll_by(app, vec2(auto.x, 0.0));
            }
            Drag::ResizeCol { col, start, orig } => {
                let w = (orig + (p.x - start) / geo.z).max(0.0);
                let text = app
                    .ui
                    .language
                    .tr("Width: {width} ({pixels} pixels)")
                    .replace("{width}", &format!("{:.2}", gridcraft_engine::cmd::format::points_to_chars(w as f64)))
                    .replace("{pixels}", &w.round().to_string());
                app.toast = Some((text, crate::now_ms()));
                // Inside a selection of whole columns, every selected column takes the new width
                // (Ctrl-selected areas included); otherwise only the dragged one.
                let cols = if sel.ranges.iter().any(|r| r.is_full_cols() && col >= r.start.col && col <= r.end.col) {
                    sel.ranges.iter().filter(|r| r.is_full_cols()).map(|r| r.a1()).collect::<Vec<_>>().join(",")
                } else {
                    RangeRef::cols(col, col).a1()
                };
                let _ = app.session.run("home.columnWidth", json!({"cols": cols, "width": w.round()}));
                coalesce_last_undo(app);
            }
            Drag::ResizeRow { row, start, orig } => {
                let h = (orig + (p.y - start) / geo.z).max(0.0);
                let text = app
                    .ui
                    .language
                    .tr("Height: {height} ({pixels} pixels)")
                    .replace("{height}", &format!("{:.2}", h * 0.75))
                    .replace("{pixels}", &h.round().to_string());
                app.toast = Some((text, crate::now_ms()));
                let rows = if sel.ranges.iter().any(|r| r.is_full_rows() && row >= r.start.row && row <= r.end.row) {
                    sel.ranges
                        .iter()
                        .filter(|r| r.is_full_rows())
                        .map(|r| format!("{}:{}", r.start.row + 1, r.end.row + 1))
                        .collect::<Vec<_>>()
                        .join(",")
                } else {
                    format!("{}:{}", row + 1, row + 1)
                };
                let _ = app.session.run("home.rowHeight", json!({"rows": rows, "height": h.round()}));
                coalesce_last_undo(app);
            }
            Drag::Freeze { cols, .. } => {
                app.grid.drag = Drag::Freeze { cols, to: Some(freeze_boundary(geo, sh, p, cols)) };
            }
            Drag::Fill { .. } => {
                let src = sel.current();
                let target = fill_target(src, pc);
                app.grid.drag = Drag::Fill { target: Some(target) };
                scroll_by(app, auto);
            }
            Drag::Move { grab, .. } => {
                let src = sel.current();
                let dr = pc.row as i64 - grab.row as i64;
                let dc = pc.col as i64 - grab.col as i64;
                app.grid.drag = Drag::Move { grab, to: Some(src.start.offset_clamped(dr, dc)) };
                scroll_by(app, auto);
            }
            Drag::Object { kind, id, start, orig, resize } => {
                let d = (p - start) / geo.z;
                let (cell, dx, dy, w, h) = orig;
                let params = if resize {
                    json!({"kind": kind, "id": id, "width": (w + d.x).max(20.0), "height": (h + d.y).max(20.0)})
                } else {
                    // Re-anchor at the cell under the new top-left.
                    let x0 = sh.col_left(cell.col) as f32 + dx + d.x;
                    let y0 = sh.row_top(cell.row) as f32 + dy + d.y;
                    let nc = CellRef::new(sh.row_at(y0.max(0.0) as f64), sh.col_at(x0.max(0.0) as f64));
                    json!({"kind": kind, "id": id, "at": nc.a1(), "dx": x0.max(0.0) - sh.col_left(nc.col) as f32, "dy": y0.max(0.0) - sh.row_top(nc.row) as f32})
                };
                let _ = app.session.run("object.move", params);
                coalesce_last_undo(app);
            }
            Drag::None => {}
        }
    }

    // Release.
    if resp.drag_stopped() || (!pointer.any_down() && app.grid.drag != Drag::None && !resp.dragged()) {
        match std::mem::take(&mut app.grid.drag) {
            Drag::Select if resp.drag_stopped() => {
                if let Some(p) = pos {
                    select_to_pointer(app, geo, sh, wb, p);
                }
            }
            Drag::Fill { target: Some(t) } => {
                let src = sel.current();
                if t != src {
                    // Ctrl (Cmd on macOS) swaps copying and extending, as in Excel.
                    let mode = if mods.command { "alternate" } else { "series" };
                    if let Err(e) = app.run("edit.autoFill", json!({"source": src.a1(), "target": t.a1(), "mode": mode})) {
                        app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                    }
                }
            }
            Drag::Freeze { cols, to: Some(to) } => {
                let (rows_now, cols_now) = (geo.fr, geo.fc);
                let (rows, cols) = if cols { (rows_now, to) } else { (to, cols_now) };
                if (rows, cols) != (rows_now, cols_now) {
                    // A1 (nothing left frozen) unfreezes.
                    app.run_or_alert("view.freezePanes", json!({"cell": CellRef::new(rows, cols).a1()}));
                }
            }
            Drag::Move { to: Some(to), .. } => {
                let src = sel.current();
                if to != src.start {
                    let _ = app.session.run("edit.cut", json!({"range": src.a1()}));
                    let _ = app.run("edit.paste", json!({"at": to.a1()}));
                }
            }
            Drag::MoveCols { source, before: Some(before) } => {
                if let Err(e) = app.run("sheet.moveColumns", json!({"cols": source.a1(), "before": before})) {
                    app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                }
            }
            _ => {}
        }
        app.grid.drag_select = false;
    }

    // Right-click: context menu.
    if resp.secondary_clicked()
        && let Some(p) = pos
    {
        if in_cells && !sel.contains(geo.cell_at(sh, p)) {
            let _ = app.session.run("selection.set", json!({"cell": geo.cell_at(sh, p).a1()}));
        }
        if in_col_header || in_row_header {
            let (rows, idx) = if in_row_header { (true, geo.row_at(sh, p.y)) } else { (false, geo.col_at(sh, p.x)) };
            let inside = sel.ranges.iter().any(|r| {
                if rows {
                    r.is_full_rows() && idx >= r.start.row && idx <= r.end.row
                } else {
                    r.is_full_cols() && idx >= r.start.col && idx <= r.end.col
                }
            });
            if !inside {
                let range = if rows { RangeRef::rows(idx, idx) } else { RangeRef::cols(idx, idx) };
                let _ = app.session.run("selection.set", json!({"range": range.a1()}));
            }
            app.grid.header_menu = Some((geo.mirror_pos(p), rows));
        } else {
            app.grid.context_menu = Some(geo.mirror_pos(p));
        }
    }

    if primary_clicked || resp.drag_started() {
        resp.request_focus();
    }
    keyboard(app, &ctx, resp, geo, sh);
}

/// Point mode on a header: inserts the whole-column or whole-row reference instead of selecting.
fn point_header(app: &mut SheetApp, wb: &Workbook, sh: &Sheet, range: RangeRef) -> bool {
    let Some(ed) = app.editor.as_mut().filter(|ed| ed.can_point()) else { return false };
    let r = range.a1();
    let r = if ed.sheet != wb.active_sheet { format!("{}!{r}", gridcraft_engine::formula::quote_sheet(&sh.name)) } else { r };
    ed.insert_ref(&r);
    ed.request_focus = true;
    true
}

fn select_to_pointer(app: &mut SheetApp, geo: &Geo, sh: &Sheet, wb: &Workbook, p: Pos2) {
    let cell = CellRef::new(
        geo.row_at(sh, p.y.clamp(geo.cells.top() + 1.0, geo.cells.bottom() - 1.0)),
        geo.col_at(sh, p.x.clamp(geo.cells.left() + 1.0, geo.cells.right() - 1.0)),
    );
    if let Some(ed) = app.editor.as_mut()
        && ed.point.is_some()
    {
        if let Some(start) = ed.point_cell {
            let range = RangeRef::new(start, cell);
            let text = if range.is_single() { range.start.a1() } else { range.a1() };
            let text = if ed.sheet != wb.active_sheet { format!("{}!{text}", gridcraft_engine::formula::quote_sheet(&sh.name)) } else { text };
            ed.insert_ref(&text);
            ed.point_cell = Some(start);
        }
    } else if let Some(d) = app.session.active() {
        // Read the current anchor: starting a drag may have just replaced the selection.
        let anchor = d.selection.anchor;
        let active = d.selection.active;
        let mut ranges: Vec<String> = d.selection.ranges.iter().map(|r| r.a1()).collect();
        ranges.pop();
        ranges.push(RangeRef::new(anchor, cell).a1());
        let _ = app.session.run("selection.set", json!({"range": ranges.join(","), "active": active.a1()}));
        if let Some(d) = app.session.active_mut() {
            d.selection.anchor = anchor;
        }
    }
}

fn coalesce_last_undo(app: &mut SheetApp) {
    // Merge consecutive drag steps into one undo entry.
    if let Some(d) = app.session.active_mut()
        && d.undo.len() >= 2
    {
        let n = d.undo.len();
        if d.undo[n - 1].label == d.undo[n - 2].label {
            d.undo.remove(n - 1);
        }
    }
}

fn scroll_by(app: &mut SheetApp, d: egui::Vec2) {
    if d != egui::Vec2::ZERO
        && let Some(v) = app.view_mut()
    {
        v.scroll = (v.scroll + d).max(egui::Vec2::ZERO);
    }
}

fn fill_target(src: RangeRef, pc: CellRef) -> RangeRef {
    let below = pc.row as i64 - src.end.row as i64;
    let above = src.start.row as i64 - pc.row as i64;
    let right = pc.col as i64 - src.end.col as i64;
    let left = src.start.col as i64 - pc.col as i64;
    let vert = below.max(above);
    let horiz = right.max(left);
    if vert <= 0 && horiz <= 0 {
        return src;
    }
    if vert >= horiz {
        if below > 0 {
            RangeRef::new(src.start, CellRef::new(pc.row, src.end.col))
        } else {
            RangeRef::new(CellRef::new(pc.row, src.start.col), src.end)
        }
    } else if right > 0 {
        RangeRef::new(src.start, CellRef::new(src.end.row, pc.col))
    } else {
        RangeRef::new(CellRef::new(src.start.row, pc.col), src.end)
    }
}

fn on_border(r: Rect, p: Pos2) -> bool {
    let outer = r.expand(3.0);
    let inner = r.shrink(3.0);
    outer.contains(p) && !inner.contains(p)
}

/// The freeze handles, as in Excel for the web: a bar in the upper part of the column-header strip
/// on the frozen columns' right edge, and one in the left part of the row-header strip on the
/// frozen rows' bottom edge. With nothing frozen they sit on the corner's edges. The rest of the
/// header edge keeps resizing the column or row.
pub fn freeze_handles(geo: &Geo) -> (Rect, Rect) {
    let x = geo.cells.left() + geo.frozen_w;
    let y = geo.cells.top() + geo.frozen_h;
    let cols = Rect::from_min_max(pos2(x - 3.0, geo.head_top()), pos2(x + 3.0, geo.head_top() + geo.header_h * 0.4));
    let rows = Rect::from_min_max(pos2(geo.head_left(), y - 3.0), pos2(geo.head_left() + geo.header_w * 0.4, y + 3.0));
    (cols, rows)
}

/// How many columns (or rows) a freeze handle dropped at `p` freezes: the line boundary nearest
/// the pointer, limited so the frozen pane leaves room for the scrolling one.
fn freeze_boundary(geo: &Geo, sh: &Sheet, p: Pos2, cols: bool) -> u32 {
    if cols {
        if p.x <= geo.cells.left() {
            return 0;
        }
        let c = geo.col_at(sh, p.x);
        let (l, r) = (geo.x(sh, c), geo.x(sh, c.saturating_add(1).min(MAX_COLS - 1)));
        let b = if p.x - l > r - p.x { c.saturating_add(1) } else { c };
        let room = geo.ci.at((((geo.cells.width() - 40.0) / geo.z).max(0.0)) as f64);
        b.min(room).min(MAX_COLS - 1)
    } else {
        if p.y <= geo.cells.top() {
            return 0;
        }
        let rr = geo.row_at(sh, p.y);
        let (t, b2) = (geo.y(sh, rr), geo.y(sh, rr.saturating_add(1).min(MAX_ROWS - 1)));
        let b = if p.y - t > b2 - p.y { rr.saturating_add(1) } else { rr };
        let room = geo.ri.at((((geo.cells.height() - 40.0) / geo.z).max(0.0)) as f64);
        b.min(room).min(MAX_ROWS - 1)
    }
}

/// Paints the freeze handles and, while one is dragged, where the freeze line will go.
fn paint_freeze_handles(p: &Painter, geo: &Geo, sh: &Sheet, drag: &Drag, t: &Tokens) {
    let (cols, rows) = freeze_handles(geo);
    let bar = Color32::from_gray(150);
    p.rect_filled(Rect::from_center_size(cols.center(), vec2(3.0, cols.height())), 1.0, bar);
    p.rect_filled(Rect::from_center_size(rows.center(), vec2(rows.width(), 3.0)), 1.0, bar);
    if let Drag::Freeze { cols, to: Some(to) } = *drag {
        let stroke = Stroke::new(2.0, t.accent);
        if cols {
            let x = geo.cells.left() + sh.col_left(to) as f32 * geo.z;
            p.line_segment([pos2(x, geo.head_top()), pos2(x, geo.cells.bottom())], stroke);
        } else {
            let y = geo.cells.top() + sh.row_top(to) as f32 * geo.z;
            p.line_segment([pos2(geo.head_left(), y), pos2(geo.cells.right(), y)], stroke);
        }
    }
}

fn col_edge(geo: &Geo, sh: &Sheet, x: f32) -> Option<(u32, f32)> {
    let c = geo.col_at(sh, x);
    for cc in [c, c.saturating_sub(1)] {
        let edge = geo.x(sh, cc) + sh.col_width(cc) * geo.z;
        if (x - edge).abs() <= 4.0 {
            return Some((cc, edge));
        }
    }
    None
}

fn row_edge(geo: &Geo, sh: &Sheet, y: f32) -> Option<(u32, f32)> {
    let r = geo.row_at(sh, y);
    for rr in [r, r.saturating_sub(1)] {
        let edge = geo.y(sh, rr) + sh.row_height(rr) * geo.z;
        if (y - edge).abs() <= 3.0 {
            return Some((rr, edge));
        }
    }
    None
}

fn is_filter_button(sh: &Sheet, geo: &Geo, p: Pos2) -> bool {
    let c = geo.cell_at(sh, p);
    sh.has_filter_button(c) && filter_button_rect(geo, sh, c).contains(p)
}

fn object_anchor(sh: &Sheet, kind: &str, id: u32) -> Option<gridcraft_engine::model::Anchor> {
    match kind {
        "chart" => sh.charts.iter().find(|c| c.id == id).map(|c| c.anchor),
        "image" => sh.images.iter().find(|c| c.id == id).map(|c| c.anchor),
        "slicer" => sh.slicers.iter().find(|c| c.id == id).map(|c| c.anchor),
        _ => sh.shapes.iter().find(|c| c.id == id).map(|c| c.anchor),
    }
}

fn delete_selected_object(app: &mut SheetApp, sh: &Sheet) -> bool {
    let Some(id) = app.selected_chart else { return false };
    let kind = if sh.charts.iter().any(|object| object.id == id) {
        "chart"
    } else if sh.images.iter().any(|object| object.id == id) {
        "image"
    } else if sh.shapes.iter().any(|object| object.id == id) {
        "shape"
    } else {
        return false;
    };
    app.run_or_alert("object.delete", json!({"kind": kind, "id": id}));
    app.selected_chart = None;
    true
}

/// A click on a hyperlink. References select; web and e-mail addresses ask first (the dialog's
/// OK runs `link.open`); every other kind is refused with a message. The decision is the
/// engine's ([`gridcraft_engine::links::classify`]), the same one `link.open` applies.
pub fn follow_link(app: &mut SheetApp, target: &str) {
    use gridcraft_engine::links::{LinkKind, classify};
    match classify(target) {
        LinkKind::Internal(r) => {
            let _ = app.session.run("edit.goTo", json!({"reference": r.trim_start_matches('#')}));
            app.grid.ensure_visible = true;
        }
        LinkKind::External(url) => app.open_dialog("openLink", json!({"url": url})),
        LinkKind::Refused(why) => app.message = Some(("Hyperlink".into(), why)),
    }
}

/// The areas after a click: with `add` (Ctrl) the clicked area is added, or removed when it is already
/// selected (unless it is the only one, so the selection never ends up empty).
fn toggled_ranges(mut ranges: Vec<String>, clicked: String, add: bool) -> Vec<String> {
    if !add {
        return vec![clicked];
    }
    if ranges.len() > 1 && ranges.contains(&clicked) {
        ranges.retain(|x| *x != clicked);
    } else if !ranges.contains(&clicked) {
        ranges.push(clicked);
    }
    ranges
}

fn select_ranges(app: &mut SheetApp, r: RangeRef, add: bool, active: CellRef) {
    let current: Vec<String> =
        if add { app.session.active().map(|d| d.selection.ranges.iter().map(|x| x.a1()).collect()).unwrap_or_default() } else { vec![] };
    let clicked = r.a1();
    let ranges = toggled_ranges(current, clicked.clone(), add);
    // A removed area takes the active cell with it; `selection.set` then puts it at the start of the last area.
    let params =
        if ranges.contains(&clicked) { json!({"range": ranges.join(","), "active": active.a1()}) } else { json!({"range": ranges.join(",")}) };
    let _ = app.session.run("selection.set", params);
}

/// Grid keyboard handling when no text field has focus.
fn keyboard(app: &mut SheetApp, ctx: &egui::Context, resp: &egui::Response, geo: &Geo, sh: &Sheet) {
    // Focus left on an editor that no longer exists goes back to the grid.
    let stale = [egui::Id::new("gridcraft.cell_editor"), egui::Id::new("gridcraft.formula_bar"), crate::text_box::editor_id()];
    let editing = app.editor.is_some() || app.text_box_editor.is_some();
    if !editing && ctx.memory(|m| m.focused()).is_some_and(|f| stale.contains(&f)) {
        resp.request_focus();
    }
    let other_focus = ctx.memory(|m| m.focused()).is_some_and(|f| f != resp.id && (editing || !stale.contains(&f)));
    if editing || app.dialog.is_some() || other_focus || app.message.is_some() {
        return;
    }
    if ctx.memory(|m| m.focused()).is_none() {
        resp.request_focus();
    }
    // The grid owns arrows, Tab and Escape (egui would otherwise move focus between widgets).
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(resp.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
    });
    let events = ctx.input(|i| i.events.clone());
    // A shortcut key that also types a character (Shift+Space sends Text(" ")) must not start an edit.
    let shortcut_pressed = events.iter().any(|e| {
        matches!(e, egui::Event::Key { pressed: true, modifiers: m, key, .. }
            if !m.alt && (m.ctrl || m.command || (m.shift && *key == egui::Key::Space)))
    });
    let page = ((geo.cells.height() / geo.z) / sh.default_row_height).floor().max(1.0) as i64;
    for ev in events {
        match ev {
            egui::Event::Copy => {
                app.copy_to_clipboard(ctx, "edit.copy");
            }
            egui::Event::Cut => {
                app.copy_to_clipboard(ctx, "edit.cut");
            }
            egui::Event::Paste(text) => app.paste_external(text),
            egui::Event::Text(_) if shortcut_pressed => {}
            egui::Event::Text(text) if text.chars().all(|c| !c.is_control()) && !text.is_empty() => {
                app.begin_edit(Some(text), false);
                if let Some(ed) = app.editor.as_mut() {
                    ed.caret = ed.text.chars().count();
                }
                return;
            }
            egui::Event::Key { key, physical_key, pressed: true, modifiers: m, .. } => {
                use egui::Key;
                if crate::keymap::dispatch(app, ctx, key, physical_key, m, geo, sh) {
                    continue;
                }
                let cmd = m.command;
                let shift = m.shift;
                let mv = |app: &mut SheetApp, dr: i64, dc: i64| {
                    let _ = app.session.run("selection.move", json!({"dr": dr, "dc": dc, "extend": shift, "jump": cmd}));
                    app.grid.ensure_visible = true;
                    app.grid.reveal_moving_end = shift;
                };
                match key {
                    Key::ArrowUp => mv(app, -1, 0),
                    Key::ArrowDown => mv(app, 1, 0),
                    // Arrows follow the screen: on a right-to-left sheet Left goes to the next column.
                    Key::ArrowLeft => mv(app, 0, if sh.right_to_left { 1 } else { -1 }),
                    Key::ArrowRight => mv(app, 0, if sh.right_to_left { -1 } else { 1 }),
                    Key::PageDown if !cmd => mv(app, page, 0),
                    Key::PageUp if !cmd => mv(app, -page, 0),
                    Key::PageDown => app.run_or_alert("sheet.next", json!({})),
                    Key::PageUp => app.run_or_alert("sheet.previous", json!({})),
                    Key::Home if cmd => {
                        let r = sh.freeze.map(|f| f.0).unwrap_or(0);
                        let c = sh.freeze.map(|f| f.1).unwrap_or(0);
                        let _ = app.session.run("selection.set", json!({"cell": CellRef::new(r, c).a1()}));
                        app.grid.ensure_visible = true;
                    }
                    Key::End if cmd => {
                        let end = sh.used_range().map(|u| u.end).unwrap_or_default();
                        let _ = app.session.run("selection.set", json!({"cell": end.a1()}));
                        app.grid.ensure_visible = true;
                    }
                    Key::Home => {
                        let a = app.session.active().map(|d| d.selection.active).unwrap_or_default();
                        let _ = app.session.run("selection.set", json!({"cell": CellRef::new(a.row, 0).a1()}));
                        app.grid.ensure_visible = true;
                    }
                    // Excel: Enter while a copied range is marked pastes it and ends copy mode.
                    Key::Enter if !m.alt && !m.ctrl && !shift && !cmd && app.session.clipboard.is_some() => {
                        app.run_or_alert("edit.paste", json!({}));
                        app.session.clipboard = None;
                        app.grid.ensure_visible = true;
                    }
                    Key::Enter => {
                        if m.alt || m.ctrl {
                            app.begin_edit(None, false);
                        } else {
                            app.move_after_enter(if shift { -1 } else { 1 }, 0);
                        }
                    }
                    Key::Tab => app.move_after_enter(0, if shift { -1 } else { 1 }),
                    Key::F2 => app.begin_edit(None, false),
                    Key::Delete if delete_selected_object(app, sh) => {}
                    Key::Delete => app.run_or_alert("edit.clearContents", json!({})),
                    Key::Backspace if delete_selected_object(app, sh) => {}
                    Key::Backspace => {
                        // Excel for Mac: Delete clears the cell and starts editing it.
                        app.run_or_alert("edit.clearContents", json!({}));
                        if app.session.active().is_some_and(|d| d.selection.is_single_cell()) {
                            app.begin_edit(Some(String::new()), false);
                        }
                    }
                    Key::Escape => {
                        app.session.clipboard = None;
                        app.session.format_painter = None;
                    }
                    Key::Space if !cmd && !shift && is_checkbox(sh, app) => app.run_or_alert("cell.toggleCheckbox", json!({})),
                    Key::F8 if m.alt => {
                        app.open_dialog("macros", json!({}));
                    }
                    Key::F9 => app.run_or_alert("formulas.calculateNow", json!({})),
                    Key::F4 if !cmd => {}
                    _ if cmd => crate::ribbon::shortcut(app, key, m),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn in_cell_editor(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    let Some(ed) = app.editor.as_ref() else { return };
    if ed.sheet != wb.active_sheet {
        return;
    }
    let cell = ed.cell;
    let rect = match sh.merge_at(cell) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, cell),
    };
    let st = wb.styles.get(sh.style_id(cell)).clone();
    let size = st.font.size * 96.0 / 72.0 * geo.z;
    let font = FontId::new(size, theme::cell_family(&st.font.name, st.font.bold, st.font.italic));
    let text_w = ui.painter().layout_no_wrap(ed.text.clone(), font.clone(), Color32::BLACK).size().x;
    let lines = ed.text.lines().count().max(1) as f32 + if ed.text.ends_with('\n') { 1.0 } else { 0.0 };
    let w = (text_w + 14.0).max(rect.width()).min(geo.cells.right() - rect.left());
    let h = (size * 1.3 * lines + 4.0).max(rect.height());
    // Grows to the left on a right-to-left sheet.
    let erect = geo.mirror_rect(Rect::from_min_size(rect.min, vec2(w, h)));
    let fill = match st.fill.pattern {
        PatternType::Solid => st.fill.fg.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::WHITE),
        _ => Color32::WHITE,
    };
    let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
    let quadrant = Rect::from_min_max(
        pos2(if cell.col >= geo.fc { split.x } else { geo.cells.left() }, if cell.row >= geo.fr { split.y } else { geo.cells.top() }),
        pos2(if cell.col >= geo.fc { geo.cells.right() } else { split.x }, if cell.row >= geo.fr { geo.cells.bottom() } else { split.y }),
    );
    let clip = ui.clip_rect().intersect(quadrant);
    let painter = ui.painter().with_clip_rect(clip);
    painter.rect_filled(erect, 0.0, fill);
    painter.rect_stroke(erect.expand(1.0), 0.0, Stroke::new(2.0, Tokens::get(ui.ctx()).sel_border), StrokeKind::Middle);
    let id = egui::Id::new("gridcraft.cell_editor");
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(erect.shrink2(vec2(2.0, 1.0))));
    child.set_clip_rect(clip);
    crate::formula_bar::editor_widget(app, &mut child, id, font, false, erect.width() - 4.0);
}

/// Test the popup itself, not whether the pointer is over any part of the app.
/// A trigger can sit outside its popup, so its opening click must be ignored.
fn popup_dismissed(response: &egui::Response, was_open: bool) -> bool {
    response.ctx.input(|i| !i.focused || i.key_pressed(egui::Key::Escape)) || (was_open && response.clicked_elsewhere())
}

/// The table whose header cell (`row`, `col`) was clicked, not just the first table in the column.
fn filter_table_range(sh: &Sheet, col: u32, row: u32) -> Option<RangeRef> {
    sh.tables.iter().find(|t| t.range.start.row == row && col >= t.range.start.col && col <= t.range.end.col).map(|t| t.range)
}

fn filter_menu(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, was_open: bool) {
    let Some((col, row, at)) = app.grid.filter_menu else { return };
    let Some(d) = app.session.active() else { return };
    let wb = d.wb.clone();
    let si = wb.active_sheet;
    let Some(sh) = wb.sheet(si) else { return };
    let _ = geo;
    // Tables get an autofilter on demand.
    let table_range = filter_table_range(sh, col, row);
    if sh.autofilter.is_none()
        && let Some(r) = table_range
    {
        let _ = app.session.run("data.filter", json!({"range": r.a1()}));
        let _ = app.session.run("data.filter", json!({"range": r.a1()}));
        if let Some(sh2) = app.session.active().and_then(|d| d.wb.active()).map(|s| s.autofilter.is_some())
            && !sh2
        {
            let d = app.session.active_mut();
            if let Some(d) = d {
                let wbm = Arc::make_mut(&mut d.wb);
                if let Some(s) = wbm.sheet_mut(si) {
                    s.autofilter = Some(gridcraft_engine::model::AutoFilter { range: r, criteria: vec![] });
                }
            }
        }
    }
    let wb = app.session.active().map(|d| d.wb.clone()).unwrap_or(wb);
    let values = gridcraft_engine::cmd::data::filter_values(&wb, si, col);
    let key = egui::Id::new(("filter_sel", col));
    let search_key = egui::Id::new(("filter_search", col));
    let mut close = false;
    let area = egui::Area::new(egui::Id::new("filter_menu")).fixed_pos(at + vec2(-180.0, 10.0)).order(egui::Order::Foreground);
    let response = area.show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(230.0);
            let lang = crate::i18n::current(ui.ctx());
            if ui.button(lang.tr("↑  Sort Ascending")).clicked() {
                let _ = app.run("data.sortAscending", json!({"column": col_to_letters(col)}));
                close = true;
            }
            if ui.button(lang.tr("↓  Sort Descending")).clicked() {
                let _ = app.run("data.sortDescending", json!({"column": col_to_letters(col)}));
                close = true;
            }
            ui.separator();
            ui.label(egui::RichText::new(lang.tr("Filter")).strong());
            let mut checks: Vec<(String, bool)> = ui.ctx().data_mut(|d| d.get_temp::<Vec<(String, bool)>>(key)).unwrap_or(values.clone());
            // A search narrows the list; Select All and Apply then only cover the matches.
            let mut search: String = ui.ctx().data_mut(|d| d.get_temp(search_key)).unwrap_or_default();
            let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text(lang.tr("Search")).desired_width(f32::INFINITY));
            if !was_open {
                field.request_focus();
            }
            let needle = search.trim().to_lowercase();
            let matches = |v: &str| needle.is_empty() || v.to_lowercase().contains(&needle);
            let all = checks.iter().filter(|(v, _)| matches(v)).all(|(_, b)| *b);
            let mut all_new = all;
            let label = if needle.is_empty() { "(Select All)" } else { "(Select All Search Results)" };
            if ui.checkbox(&mut all_new, lang.tr(label)).changed() {
                for c in checks.iter_mut().filter(|(v, _)| matches(v)) {
                    c.1 = all_new;
                }
            }
            // Full width: shrunk to the longest value, the scroll bar sat mid-menu and the wheel only
            // worked over the values themselves (#432).
            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                for (v, on) in checks.iter_mut().filter(|(v, _)| matches(v)) {
                    // "(Blanks)" is the list's own entry; the other values are cell text.
                    ui.checkbox(on, if v == "(Blanks)" { lang.tr(v) } else { v.as_str() });
                }
            });
            ui.ctx().data_mut(|d| {
                d.insert_temp(key, checks.clone());
                d.insert_temp(search_key, search.clone());
            });
            let checks: Vec<(String, bool)> = checks.iter().map(|(v, on)| (v.clone(), *on && matches(v))).collect();
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(lang.tr("Clear Filter")).clicked() {
                    let _ = app.run("data.filterBy", json!({"column": col_to_letters(col), "clear": true}));
                    close = true;
                }
                if ui.button(lang.tr("Apply")).clicked() {
                    let blanks = checks.iter().any(|(v, on)| v == "(Blanks)" && *on);
                    let vals: Vec<String> = checks.iter().filter(|(v, on)| *on && v != "(Blanks)").map(|(v, _)| v.clone()).collect();
                    let p = if checks.iter().all(|(_, b)| *b) {
                        json!({"column": col_to_letters(col), "clear": true})
                    } else {
                        json!({"column": col_to_letters(col), "values": vals, "blanks": blanks})
                    };
                    let _ = app.run("data.filterBy", p);
                    close = true;
                }
            });
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        ui.ctx().data_mut(|d| {
            d.remove::<Vec<(String, bool)>>(key);
            d.remove::<String>(search_key);
        });
        app.grid.filter_menu = None;
    }
}

fn context_menu(app: &mut SheetApp, ui: &mut egui::Ui) {
    let Some(at) = app.grid.context_menu else { return };
    let mut close = false;
    let resp = egui::Area::new(egui::Id::new("cell_context")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(200.0);
            let insert_copied = insert_copied_item(app);
            let mut items: Vec<(&str, &str)> = vec![
                ("Cut", "edit.cut"),
                ("Copy", "edit.copy"),
                ("Paste", "edit.paste"),
                ("Paste Special…", "ui:pasteSpecial"),
                ("-", ""),
                ("Insert…", "ui:insertCells"),
                ("Delete…", "ui:deleteCells"),
                ("Clear Contents", "edit.clearContents"),
                ("-", ""),
                ("Filter by Selected Cell's Value", "ui:filterValue"),
                ("Sort A to Z", "data.sortAscending"),
                ("Sort Z to A", "data.sortDescending"),
                ("-", ""),
                ("New Comment", "ui:newComment"),
                ("New Note", "ui:newNote"),
                ("-", ""),
                ("Format Cells…", "ui:formatCells"),
                ("Pick From Drop-down List…", "ui:pickList"),
                ("Define Name…", "ui:defineName"),
                ("Link…", "ui:insertLink"),
            ];
            if let Some(item) = insert_copied {
                items.insert(6, item);
            }
            let lang = crate::i18n::current(ui.ctx());
            for (label, id) in items {
                if label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(190.0, 20.0))).clicked() {
                    close = true;
                    match id {
                        "ui:pasteSpecial" => app.open_dialog("pasteSpecial", json!({})),
                        "ui:insertPaste" => app.open_dialog("insertPaste", json!({})),
                        "ui:insertCells" => app.open_dialog("insertCells", json!({})),
                        "ui:deleteCells" => app.open_dialog("deleteCells", json!({})),
                        "ui:formatCells" => app.open_dialog("formatCells", json!({})),
                        "ui:newComment" => app.open_dialog("comment", json!({"threaded": true})),
                        "ui:newNote" => app.open_dialog("comment", json!({"threaded": false})),
                        "ui:defineName" => app.open_dialog("defineName", json!({})),
                        "ui:insertLink" => app.open_dialog("insertLink", json!({})),
                        "ui:pickList" => app.open_dialog("pickList", json!({})),
                        "ui:filterValue" => {
                            if let Some(d) = app.session.active() {
                                let a = d.selection.active;
                                let text = d.wb.active().map(|sh| gridcraft_engine::display::cell_text(&d.wb, sh, a)).unwrap_or_default();
                                let has = d.wb.active().is_some_and(|sh| sh.autofilter.is_some());
                                if !has {
                                    let _ = app.run("data.filter", json!({}));
                                }
                                app.run_or_alert("data.filterBy", json!({"column": col_to_letters(a.col), "values": [text]}));
                            }
                        }
                        "edit.copy" | "edit.cut" => {
                            app.copy_to_clipboard(ui.ctx(), id);
                        }
                        "edit.paste" => app.paste_from_clipboard(),
                        other => app.run_or_alert(other, json!({})),
                    }
                }
            }
        });
    });
    if close
        || ui.input(|i| i.key_pressed(egui::Key::Escape))
        || (ui.input(|i| i.pointer.any_pressed())
            && !resp.response.contains_pointer()
            && !resp.response.hovered()
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !resp.response.rect.contains(p)))
    {
        app.grid.context_menu = None;
    }
}

/// Used by `EditState` users to keep the active cell visible while pointing.
pub fn point_move(app: &mut SheetApp, ed: &mut EditState, dr: i64, dc: i64, extend: bool) {
    let Some(d) = app.session.active() else { return };
    let from = ed.point_cell.unwrap_or(ed.cell);
    let to = from.offset_clamped(dr, dc);
    let text = if extend {
        // The anchor is the corner opposite the moving end: the written range is normalized, so
        // after extending up or left its first cell is the moving end, not the anchor.
        let pointed = ed.point.and_then(|(s, e)| ed.text.get(s..e)).and_then(RangeRef::parse);
        let anchor = pointed
            .map(|r| {
                CellRef::new(
                    if from.row == r.start.row { r.end.row } else { r.start.row },
                    if from.col == r.start.col { r.end.col } else { r.start.col },
                )
            })
            .unwrap_or(from);
        RangeRef::new(anchor, to).a1()
    } else {
        to.a1()
    };
    let _ = d;
    ed.insert_ref(&text);
    ed.point_cell = Some(to);
    app.grid.ensure_visible = true;
}

/// The dropdown arrow beside a cell with an in-cell list validation.
pub fn validation_arrow(sh: &Sheet, geo: &Geo, c: CellRef) -> Option<Rect> {
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(c)))?;
    if dv.kind != gridcraft_engine::model::ValidationKind::List || !dv.in_cell_dropdown || geo.hidden_under_frozen(sh, c) {
        return None;
    }
    let r = geo.cell_rect(sh, c);
    let s = r.height().clamp(14.0, 22.0);
    Some(Rect::from_min_size(pos2(r.right() + 1.0, r.bottom() - s), vec2(s, s)))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_overlays(
    app: &mut SheetApp,
    ui: &egui::Ui,
    p: &Painter,
    geo: &Geo,
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    sel: &gridcraft_engine::Selection,
) {
    // Validation dropdown arrow for the active cell.
    if app.editor.is_none()
        && let Some(r) = validation_arrow(sh, geo, sel.active)
    {
        p.rect_filled(r, 2.0, Color32::from_gray(245));
        p.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_gray(160)), StrokeKind::Inside);
        crate::icons::paint(p, r.shrink(3.0), crate::icons::Icon::Chevron, Color32::from_gray(60));
    }
    // Input message of the active cell's validation, hidden while its list is open: it would cover the items.
    if app.editor.is_none()
        && app.grid.list_picker != Some(sel.active)
        && let Some(dv) = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(sel.active)))
        && dv.show_input
        && !(dv.input_title.is_empty() && dv.input_message.is_empty())
        && !geo.hidden_under_frozen(sh, sel.active)
    {
        let r = geo.cell_rect(sh, sel.active);
        let (at, pivot) = (geo.mirror_pos(pos2(r.left() + 8.0, r.bottom() + 6.0)), if geo.rtl { Align2::RIGHT_TOP } else { Align2::LEFT_TOP });
        egui::Area::new(egui::Id::new("dv_input")).fixed_pos(at).pivot(pivot).order(egui::Order::Tooltip).interactable(false).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::from_rgb(0xFF, 0xFF, 0xE1)).show(ui, |ui| {
                ui.set_max_width(220.0);
                if !dv.input_title.is_empty() {
                    ui.label(egui::RichText::new(&dv.input_title).strong().color(Color32::BLACK));
                }
                ui.label(egui::RichText::new(&dv.input_message).color(Color32::BLACK));
            });
        });
    }
    // Notes: shown on hover (or always when marked visible).
    let hover = app.grid.hover_cell;
    let all_notes = app.session.show_all_notes;
    for (c, cm) in &sh.comments {
        let show = cm.visible || (all_notes && !cm.threaded) || hover == Some(*c);
        if !show || app.grid.drag != Drag::None {
            continue;
        }
        let r = geo.cell_rect(sh, *c);
        if !p.clip_rect().intersects(r) || geo.hidden_under_frozen(sh, *c) {
            continue;
        }
        let at = pos2(r.right() + 10.0, r.top());
        p.line_segment([r.right_top(), at + vec2(0.0, 6.0)], Stroke::new(1.0, Color32::from_gray(90)));
        let (at, pivot) = (geo.mirror_pos(at), if geo.rtl { Align2::RIGHT_TOP } else { Align2::LEFT_TOP });
        egui::Area::new(egui::Id::new(("note", c.row, c.col))).fixed_pos(at).pivot(pivot).order(egui::Order::Tooltip).interactable(false).show(
            ui.ctx(),
            |ui| {
                egui::Frame::popup(ui.style()).fill(if cm.threaded { Color32::WHITE } else { Color32::from_rgb(0xFF, 0xFF, 0xE1) }).show(ui, |ui| {
                    ui.set_max_width(240.0);
                    ui.label(egui::RichText::new(format!("{}:", cm.author)).strong().color(Color32::BLACK));
                    ui.label(egui::RichText::new(&cm.text).color(Color32::BLACK));
                    for (a, t) in &cm.replies {
                        ui.separator();
                        ui.label(egui::RichText::new(format!("{a}: {t}")).color(Color32::BLACK));
                    }
                });
            },
        );
    }
    // Hyperlink tooltip.
    if let Some(h) = hover.and_then(|c| sh.hyperlinks.get(&c))
        && app.editor.is_none()
    {
        let tip = h.tooltip.clone().unwrap_or_else(|| {
            format!("{}\n{}", h.target, crate::i18n::current(ui.ctx()).tr("Click once to follow. Click and hold to select this cell."))
        });
        egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::new("link_tip"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(tip);
        });
    }
    // Trace precedents / dependents arrows.
    if let Some(trace) = app.grid.trace.clone()
        && let Some(items) = trace.as_array()
    {
        let blue = Color32::from_rgb(0x1F, 0x5F, 0xC9);
        let active = geo.cell_rect(sh, sel.active).center();
        for it in items {
            if it["sheet"].as_str().is_some_and(|s| !s.eq_ignore_ascii_case(&sh.name)) {
                continue;
            }
            let target = it["range"].as_str().or(it["cell"].as_str()).and_then(RangeRef::parse);
            let Some(t) = target else { continue };
            let tr = geo.range_rect(sh, t);
            if !t.is_single() {
                for pane in geo.range_panes(t) {
                    p.with_clip_rect(pane.intersect(p.clip_rect())).rect_stroke(tr, 0.0, Stroke::new(1.5, blue), StrokeKind::Inside);
                }
            }
            let (from, to) = if it.get("range").is_some() { (tr.center(), active) } else { (active, tr.center()) };
            p.line_segment([from, to], Stroke::new(1.5, blue));
            p.circle_filled(from, 3.0, blue);
            let d = (to - from).normalized();
            let n = vec2(-d.y, d.x);
            p.add(egui::Shape::convex_polygon(vec![to, to - d * 9.0 + n * 4.0, to - d * 9.0 - n * 4.0], blue, Stroke::NONE));
        }
    }
    let _ = (wb, si);
}

fn list_picker(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, was_open: bool) {
    let Some(c) = app.grid.list_picker else { return };
    let Some(d) = app.session.active() else { return };
    let wb = d.wb.clone();
    let si = wb.active_sheet;
    let Some(sh) = wb.sheet(si) else { return };
    let Some(dv) = sh.validations.iter().find(|x| x.ranges.iter().any(|r| r.contains(c))).cloned() else {
        app.grid.list_picker = None;
        return;
    };
    let items = gridcraft_engine::cmd::data::list_items(&wb, si, &dv);
    let r = geo.cell_rect(sh, c);
    let (at, pivot) = (geo.mirror_pos(r.left_bottom()), if geo.rtl { Align2::RIGHT_TOP } else { Align2::LEFT_TOP });
    let mut close = false;
    let response = egui::Area::new(egui::Id::new("dv_list")).fixed_pos(at).pivot(pivot).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
            ui.set_min_width(r.width().max(100.0));
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                for it in &items {
                    if ui.add(egui::Button::new(it.as_str()).frame(false).min_size(vec2(r.width().max(100.0), 18.0))).clicked() {
                        app.run_or_alert("cell.set", json!({"cell": c.a1(), "input": it}));
                        close = true;
                    }
                }
            });
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        app.grid.list_picker = None;
    }
}

fn header_menu(app: &mut SheetApp, ui: &mut egui::Ui, was_open: bool) {
    let Some((at, rows)) = app.grid.header_menu else { return };
    let mut close = false;
    let response = egui::Area::new(egui::Id::new("header_menu")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(190.0);
            let insert_copied = insert_copied_item(app);
            let mut items: Vec<(&str, &str)> = if rows {
                vec![
                    ("Cut", "edit.cut"),
                    ("Copy", "edit.copy"),
                    ("Paste", "edit.paste"),
                    ("-", ""),
                    ("Insert", "home.insertRows"),
                    ("Delete", "home.deleteRows"),
                    ("Clear Contents", "edit.clearContents"),
                    ("-", ""),
                    ("Row Height…", "ui:rowHeight"),
                    ("AutoFit Row Height", "home.autofitRowHeight"),
                    ("Hide", "home.hideRows"),
                    ("Unhide", "home.unhideRows"),
                    ("Group", "data.group"),
                ]
            } else {
                vec![
                    ("Cut", "edit.cut"),
                    ("Copy", "edit.copy"),
                    ("Paste", "edit.paste"),
                    ("-", ""),
                    ("Insert", "home.insertColumns"),
                    ("Delete", "home.deleteColumns"),
                    ("Clear Contents", "edit.clearContents"),
                    ("-", ""),
                    ("Column Width…", "ui:columnWidth"),
                    ("AutoFit Column Width", "home.autofitColumnWidth"),
                    ("Hide", "home.hideColumns"),
                    ("Unhide", "home.unhideColumns"),
                    ("Sort A to Z", "data.sortAscending"),
                    ("Group", "data.group"),
                ]
            };
            if let Some(item) = insert_copied {
                items.insert(5, item);
            }
            let lang = crate::i18n::current(ui.ctx());
            for (label, id) in items {
                if label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(180.0, 20.0))).clicked() {
                    close = true;
                    match id {
                        "ui:insertPaste" => app.open_dialog("insertPaste", json!({})),
                        "ui:rowHeight" => app.open_dialog("rowHeight", json!({})),
                        "ui:columnWidth" => app.open_dialog("columnWidth", json!({})),
                        "edit.copy" | "edit.cut" => {
                            app.copy_to_clipboard(ui.ctx(), id);
                        }
                        "edit.paste" => app.paste_from_clipboard(),
                        other => app.run_or_alert(other, json!({})),
                    }
                }
            }
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        app.grid.header_menu = None;
    }
}

/// The context-menu entry for Insert Copied Cells / Insert Cut Cells, when the clipboard holds a
/// copy or cut of this app: whole rows or columns insert at once, a block asks which way to shift
/// (Insert Paste). A cut block can only be pasted, so it gets no entry.
fn insert_copied_item(app: &SheetApp) -> Option<(&'static str, &'static str)> {
    let clip = app.session.clipboard.as_ref()?;
    let lines = clip.selected.is_full_rows() != clip.selected.is_full_cols();
    let cut = clip.cut && app.session.active().is_some_and(|d| d.uid == clip.doc_uid);
    match (cut, lines) {
        (true, true) => Some(("Insert Cut Cells", "home.insertCopiedCells")),
        (true, false) => None,
        (false, true) => Some(("Insert Copied Cells", "home.insertCopiedCells")),
        (false, false) => Some(("Insert Copied Cells…", "ui:insertPaste")),
    }
}

fn is_checkbox(sh: &Sheet, app: &SheetApp) -> bool {
    let Some(d) = app.session.active() else { return false };
    d.wb.styles.get(sh.style_id(d.selection.active)).num_fmt.as_str() == "checkbox"
}

fn paint_pages(app: &mut SheetApp, p: &Painter, geo: &Geo, sh: &Sheet) {
    let Some(d) = app.session.active() else { return };
    let key = (d.uid, d.revision, d.wb.active_sheet);
    if app.grid.pages.as_ref().is_none_or(|(k, _)| *k != key) {
        let pages: Vec<(RangeRef, u32)> = app
            .session
            .run("file.printPreview", json!({}))
            .ok()
            .and_then(|v| {
                v["pageList"].as_array().map(|a| {
                    a.iter()
                        .filter(|pg| pg["sheet"].as_str() == Some(sh.name.as_str()))
                        .filter_map(|pg| Some((RangeRef::parse(pg["range"].as_str()?)?, pg["page"].as_u64()? as u32)))
                        .collect()
                })
            })
            .unwrap_or_default();
        app.grid.pages = Some((key, pages));
    }
    let Some((_, pages)) = &app.grid.pages else { return };
    if pages.is_empty() {
        return;
    }
    // Grey everything, then clear the printed pages.
    let mut printed = pages[0].0;
    for (r, _) in pages {
        printed = printed.union(r);
    }
    let pr = geo.range_rect(sh, printed);
    let grey = Color32::from_black_alpha(70);
    let c = p.clip_rect();
    for r in [
        Rect::from_min_max(c.min, pos2(c.right(), pr.top())),
        Rect::from_min_max(pos2(c.left(), pr.bottom()), c.max),
        Rect::from_min_max(pos2(c.left(), pr.top()), pos2(pr.left(), pr.bottom())),
        Rect::from_min_max(pos2(pr.right(), pr.top()), pos2(c.right(), pr.bottom())),
    ] {
        if r.is_positive() {
            p.rect_filled(r, 0.0, grey);
        }
    }
    let blue = Color32::from_rgb(0x2E, 0x6F, 0xD8);
    for (r, n) in pages {
        let rr = geo.range_rect(sh, *r);
        if !rr.intersects(c) {
            continue;
        }
        p.extend(egui::Shape::dashed_line(
            &[rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()],
            Stroke::new(2.0, blue),
            6.0,
            3.0,
        ));
        p.text(
            rr.center(),
            Align2::CENTER_CENTER,
            app.ui.language.tr("Page {n}").replace("{n}", &n.to_string()),
            theme::ui_bold((rr.height().min(rr.width()) / 6.0).clamp(14.0, 72.0)),
            Color32::from_black_alpha(45),
        );
    }
    p.rect_stroke(pr, 0.0, Stroke::new(3.0, blue), StrokeKind::Outside);
}

#[cfg(test)]
mod selection_fill_tests {
    use super::*;

    #[test]
    fn rect_around_leaves_the_hole_and_never_overlaps() {
        let area = Rect::from_min_max(pos2(100.0, 50.0), pos2(400.0, 250.0));
        let check = |hole: Rect, pieces: usize| {
            let parts: Vec<Rect> = rect_around(area, hole).collect();
            assert_eq!(parts.len(), pieces, "{hole:?}: {parts:?}");
            let cut = area.intersect(hole);
            let cut_area = if cut.is_positive() { cut.area() } else { 0.0 };
            let shaded: f32 = parts.iter().map(Rect::area).sum();
            assert!((shaded - (area.area() - cut_area)).abs() < 0.5, "everything but the hole is shaded: {hole:?}");
            for (i, a) in parts.iter().enumerate() {
                assert!(area.contains_rect(*a), "nothing outside the area: {a:?}");
                assert!(!a.shrink(0.01).intersects(hole), "the hole stays clear: {a:?} vs {hole:?}");
                // The fill is translucent: a part shaded twice would show darker.
                assert!(parts.iter().skip(i + 1).all(|b| !a.shrink(0.01).intersects(*b)), "{parts:?}");
            }
        };
        let cell = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(64.0, 16.0));
        // In the middle, in each corner (where a drag began), along an edge.
        check(cell(200.0, 100.0), 4);
        check(cell(100.0, 50.0), 2);
        check(cell(336.0, 50.0), 2);
        check(cell(100.0, 234.0), 2);
        check(cell(336.0, 234.0), 2);
        check(cell(100.0, 100.0), 3);
        // A merged cell as wide, or as tall, as the area; one that is all of it, or more.
        check(Rect::from_min_max(pos2(100.0, 100.0), pos2(400.0, 132.0)), 2);
        check(Rect::from_min_max(pos2(200.0, 50.0), pos2(264.0, 250.0)), 2);
        check(area, 0);
        check(area.expand(30.0), 0);
        // The active cell is in another area, touches this one, or is only partly inside it.
        check(cell(500.0, 300.0), 1);
        check(cell(400.0, 50.0), 1);
        check(cell(380.0, 240.0), 2);
    }

    #[test]
    fn rect_around_survives_hostile_numbers() {
        let sane = Rect::from_min_max(pos2(0.0, 0.0), pos2(64.0, 16.0));
        let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, f32::MIN, -1.0, 0.0];
        for a in bad {
            for b in bad {
                let weird = Rect::from_min_max(pos2(a, b), pos2(b, a));
                for (area, hole) in [(sane, weird), (weird, sane), (weird, weird)] {
                    for part in rect_around(area, hole) {
                        assert!(part.is_positive() && part.is_finite(), "{area:?} around {hole:?}: {part:?}");
                    }
                }
                // A hole that makes no sense leaves the whole area shaded.
                if !weird.intersects(sane) {
                    assert_eq!(rect_around(sane, weird).collect::<Vec<_>>(), vec![sane]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_zoom_steps_along_the_ladder_and_stops_at_100() {
        for (from, zoom_in, to) in [
            (100, true, 120),
            (120, false, 100),
            (100, false, 85),
            // Between steps, the next one each way (Excel: 112% goes to 115% or 100%).
            (112, true, 120),
            (112, false, 100),
            // A step less than 5% away is skipped…
            (121, false, 100),
            (86, false, 70),
            (139, true, 170),
            // …but never 100% or a limit.
            (99, true, 100),
            (101, false, 100),
            (96, true, 100),
            (385, true, 400),
            (11, false, 10),
            (400, true, 400),
            (10, false, 10),
            // Out-of-range zooms (from a file) come into range first.
            (5, true, 12),
            (1000, false, 340),
            (0, false, 10),
            (u16::MAX, true, 400),
        ] {
            assert_eq!(wheel_zoom_step(from, zoom_in), to, "{from}% {}", if zoom_in { "in" } else { "out" });
        }
        // Every step on the ladder is 15–25% from the next, and a notch in then out returns.
        // (The glide between steps is `zoom_glide_moves_by_ratio_and_always_arrives`.)
        for w in WHEEL_ZOOM_LADDER.windows(2) {
            let ratio = f32::from(w[1]) / f32::from(w[0]);
            assert!((1.15..=1.25).contains(&ratio), "{w:?}");
            assert_eq!(wheel_zoom_step(w[0], true), w[1]);
            assert_eq!(wheel_zoom_step(w[1], false), w[0]);
        }
    }

    #[test]
    fn zoom_glide_moves_by_ratio_and_always_arrives() {
        // A third of the way per frame, as at 60 frames a second.
        for (from, to) in [(100, 120), (120, 100), (10, 12), (340, 400), (400, 10), (85, 100)] {
            let (mut z, mut frames) = (from, 0);
            while z != to {
                let next = zoom_glide_step(z, to, 0.32);
                assert!(next != z && next.abs_diff(to) < z.abs_diff(to), "{from} → {to}: {z} then {next}");
                z = next;
                frames += 1;
                assert!(frames < 40, "{from} → {to} arrives");
            }
            assert!(frames >= 2, "{from} → {to} glides over more than one frame ({frames})");
        }
        assert_eq!(zoom_glide_step(100, 120, 0.32), 106, "a third of the way by ratio");
        assert_eq!(zoom_glide_step(120, 100, 0.32), 113);
        // No progress asked or nonsense: still a percent closer, or straight there.
        assert_eq!(zoom_glide_step(100, 120, 0.0), 101);
        assert_eq!(zoom_glide_step(120, 100, f32::NAN), 100);
        assert_eq!(zoom_glide_step(100, 120, 5.0), 120);
        assert_eq!(zoom_glide_step(120, 120, 0.5), 120);
        assert_eq!(zoom_glide_step(0, 10, 0.5), 3);
    }

    #[test]
    fn center_across_run_stops_at_first_non_continuing_column() {
        assert_eq!(center_across_end(0, 20, |c| c <= 2), 2);
        assert_eq!(center_across_end(0, 20, |_| false), 0);
        assert_eq!(center_across_end(0, 1, |_| true), 1);
    }

    #[test]
    fn filter_menu_uses_the_table_whose_header_was_clicked() {
        use gridcraft_engine::model::Table;
        let table = |id: u32, r0: u32, r1: u32| Table {
            id,
            name: format!("T{id}"),
            range: RangeRef::new(CellRef::new(r0, 0), CellRef::new(r1, 0)),
            header_row: true,
            totals_row: false,
            columns: vec![],
            style: String::new(),
            banded_rows: true,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: true,
            filter: vec![],
            table_type: None,
            connection_id: None,
        };
        let mut sh = Sheet::new("S");
        sh.tables.push(table(1, 0, 2));
        sh.tables.push(table(2, 4, 7));
        assert_eq!(filter_table_range(&sh, 0, 4), Some(sh.tables[1].range));
        assert_eq!(filter_table_range(&sh, 0, 0), Some(sh.tables[0].range));
        assert_eq!(filter_table_range(&sh, 1, 4), None);
    }

    #[test]
    fn cell_text_y_moves_monotonically_in_standard_and_tall_rows() {
        let rect_std = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 20.0));
        let gh = 18.5;
        let z = 1.0;

        let y_top_std = cell_text_y(VAlign::Top, rect_std, gh, z);
        let y_center_std = cell_text_y(VAlign::Center, rect_std, gh, z);
        let y_bottom_std = cell_text_y(VAlign::Bottom, rect_std, gh, z);

        assert!(y_top_std < y_center_std, "Top ({y_top_std}) must be higher than Center ({y_center_std})");
        assert!(y_center_std < y_bottom_std, "Center ({y_center_std}) must be higher than Bottom ({y_bottom_std})");

        // Verify tall row (e.g. 40px)
        let rect_tall = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 40.0));
        let y_top_tall = cell_text_y(VAlign::Top, rect_tall, gh, z);
        let y_center_tall = cell_text_y(VAlign::Center, rect_tall, gh, z);
        let y_bottom_tall = cell_text_y(VAlign::Bottom, rect_tall, gh, z);

        assert!(y_top_tall < y_center_tall, "Top ({y_top_tall}) must be higher than Center ({y_center_tall})");
        assert!(y_center_tall < y_bottom_tall, "Center ({y_center_tall}) must be higher than Bottom ({y_bottom_tall})");
    }

    #[test]
    fn wrapped_cell_text_y_positions_correctly_and_safeguards_overflow() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 40.0));
        let gh = 30.0;
        let z = 1.0;

        let y_top = wrapped_cell_text_y(VAlign::Top, rect, gh, z);
        let y_center = wrapped_cell_text_y(VAlign::Center, rect, gh, z);
        let y_bottom = wrapped_cell_text_y(VAlign::Bottom, rect, gh, z);

        assert!(y_top < y_center);
        assert!(y_center < y_bottom);

        // Overflow case: gh exceeds rect height
        let gh_overflow = 60.0;
        let y_bottom_ovf = wrapped_cell_text_y(VAlign::Bottom, rect, gh_overflow, z);
        assert!(y_bottom_ovf >= rect.top());
    }

    /// The screen box of a galley of `size` drawn at `pos` turned by `angle` about `pos`, as egui
    /// draws a rotated `TextShape`.
    fn rotated_box(pos: Pos2, size: egui::Vec2, angle: f32) -> Rect {
        let rot = egui::emath::Rot2::from_angle(angle);
        let pts = [vec2(0.0, 0.0), vec2(size.x, 0.0), vec2(0.0, size.y), size].map(|v| pos + rot * v);
        Rect::from_points(&pts)
    }

    #[test]
    fn rotated_text_box_follows_the_cell_alignment() {
        // A tall, narrow merged block (one 3-character column over four 60pt rows).
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(40.0, 340.0));
        let size = vec2(90.0, 15.0);
        let pad = 2.5;
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        for deg in [90i16, -90, 45, -45] {
            let angle = -(deg as f32).to_radians();
            for v in [VAlign::Top, VAlign::Center, VAlign::Bottom] {
                for h in [HAlign::Left, HAlign::Center, HAlign::Right] {
                    let bb = rotated_box(rotated_text_pos(rect, size, angle, h, v, pad, 0.0), size, angle);
                    let what = format!("{deg}° {h:?}/{v:?}: {bb:?} in {rect:?}");
                    assert!(near(bb.height(), rotated_box(Pos2::ZERO, size, angle).height()), "{what}");
                    assert!(bb.top() >= rect.top() && bb.bottom() <= rect.bottom(), "{what}");
                    // A narrow column can't hold a 45° line across: only check where it fits.
                    let fits_x = bb.width() <= rect.width() - 2.0 * pad;
                    match h {
                        HAlign::Left => assert!(near(bb.left(), rect.left() + pad), "{what}"),
                        HAlign::Right => assert!(near(bb.right(), rect.right() - pad), "{what}"),
                        _ => assert!(near(bb.center().x, rect.center().x), "{what}"),
                    }
                    if fits_x {
                        assert!(bb.left() >= rect.left() && bb.right() <= rect.right(), "{what}");
                    }
                    match v {
                        VAlign::Top => assert!(near(bb.top(), rect.top() + pad), "{what}"),
                        VAlign::Center => assert!(near(bb.center().y, rect.center().y), "{what}"),
                        // Bottom is the default: downward text keeps starting at the top, as before.
                        _ if deg < 0 => assert!(near(bb.top(), rect.top() + pad), "{what}"),
                        _ => assert!(near(bb.bottom(), rect.bottom() - pad), "{what}"),
                    }
                }
            }
        }
    }

    #[test]
    fn upright_text_with_default_alignment_keeps_its_place() {
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(40.0, 340.0));
        let size = vec2(90.0, 15.0);
        let pad = 2.5;
        let near = |a: Pos2, b: Pos2| (a - b).length() < 0.01;
        // Upward text starts at the bottom-left corner, downward text one block-height in from the top-left.
        let up = rotated_text_pos(rect, size, -std::f32::consts::FRAC_PI_2, HAlign::General, VAlign::Bottom, pad, 0.0);
        assert!(near(up, pos2(rect.left() + pad, rect.bottom() - pad)), "{up:?}");
        let down = rotated_text_pos(rect, size, std::f32::consts::FRAC_PI_2, HAlign::General, VAlign::Bottom, pad, 0.0);
        assert!(near(down, pos2(rect.left() + pad + size.y, rect.top() + pad)), "{down:?}");
    }

    #[test]
    fn upward_text_with_middle_align_is_centred_in_a_merged_block() {
        // Issue #435: "Licht Strom (?)" turned up in A1:A4 with Middle Align was drawn at the bottom.
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(40.0, 340.0));
        let size = vec2(90.0, 15.0);
        let angle = -std::f32::consts::FRAC_PI_2;
        let bb = rotated_box(rotated_text_pos(rect, size, angle, HAlign::Center, VAlign::Center, 2.5, 0.0), size, angle);
        assert!((bb.center().y - rect.center().y).abs() < 0.01, "{bb:?} not centred in {rect:?}");
        assert!((bb.center().x - rect.center().x).abs() < 0.01, "{bb:?} not centred in {rect:?}");
    }
}

#[cfg(test)]
mod pane_tests {
    use super::*;

    fn rng(r0: u32, c0: u32, r1: u32, c1: u32) -> RangeRef {
        RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1))
    }

    #[test]
    fn selection_in_scrolling_pane_is_not_drawn_in_frozen_pane() {
        // Frozen: 2 rows and 2 columns; the scrolling pane shows rows 2..=20 and columns 2..=10.
        let frozen_cols = ((2, 20), (0, 1));
        let frozen_rows = ((0, 1), (2, 10));
        let corner = ((0, 1), (0, 1));
        let scrolled = ((2, 20), (2, 10));
        let sel = rng(5, 4, 7, 6);
        assert!(range_in_pane(sel, scrolled.0, scrolled.1));
        assert!(!range_in_pane(sel, frozen_cols.0, frozen_cols.1));
        assert!(!range_in_pane(sel, frozen_rows.0, frozen_rows.1));
        assert!(!range_in_pane(sel, corner.0, corner.1));
    }

    #[test]
    fn ctrl_click_on_a_selected_header_deselects_it() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(toggled_ranges(s(&["A:A", "C:C"]), "C:C".into(), true), s(&["A:A"]));
        assert_eq!(toggled_ranges(s(&["A:A"]), "C:C".into(), true), s(&["A:A", "C:C"]));
        assert_eq!(toggled_ranges(s(&["A:A"]), "A:A".into(), true), s(&["A:A"]));
        assert_eq!(toggled_ranges(s(&["A1", "B2", "3:3"]), "B2".into(), true), s(&["A1", "3:3"]));
        assert_eq!(toggled_ranges(s(&["A:A", "C:C"]), "C:C".into(), false), s(&["C:C"]));
    }

    #[test]
    fn selection_across_the_freeze_line_is_drawn_in_both_panes() {
        let sel = rng(5, 0, 7, 6);
        assert!(range_in_pane(sel, (2, 20), (0, 1)));
        assert!(range_in_pane(sel, (2, 20), (2, 10)));
    }

    /// Two frozen rows and columns, scrolled right so that columns C:E lie under the frozen columns.
    fn frozen_geo(sh: &mut Sheet) -> Geo {
        sh.freeze = Some((2, 2));
        let scroll = vec2(3.0 * sh.default_col_width + 8.0, 0.0);
        Geo::new(sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)), scroll)
    }

    #[test]
    fn overlays_skip_cells_scrolled_under_the_frozen_columns() {
        let mut sh = Sheet::new("S");
        let geo = frozen_geo(&mut sh);
        assert!(geo.hidden_under_frozen(&sh, CellRef::new(5, 2)));
        assert!(geo.hidden_under_frozen(&sh, CellRef::new(5, 4)));
        assert!(!geo.hidden_under_frozen(&sh, CellRef::new(5, 5)));
        assert!(!geo.hidden_under_frozen(&sh, CellRef::new(5, 0)));
        let unscrolled = Geo::new(&sh, geo.rect, vec2(0.0, 0.0));
        assert!(!unscrolled.hidden_under_frozen(&sh, CellRef::new(5, 2)));
    }

    #[test]
    fn ranges_are_clipped_to_the_panes_that_hold_them() {
        let mut sh = Sheet::new("S");
        let geo = frozen_geo(&mut sh);
        let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
        assert_eq!(geo.range_panes(rng(5, 3, 7, 6)), [Rect::from_min_max(split, geo.cells.max)]);
        assert_eq!(geo.range_panes(rng(5, 0, 7, 6)).len(), 2);
        assert_eq!(geo.range_panes(rng(0, 0, 7, 6)).len(), 4);
        assert_eq!(geo.range_panes(rng(0, 0, 1, 1)), [Rect::from_min_max(geo.cells.min, split)]);
        // The fill handle of D6:E7 sits over the frozen columns, where nothing of it is shown.
        let handle = geo.range_rect(&sh, rng(5, 3, 6, 4)).right_bottom();
        assert!(handle.x < split.x);
        assert!(!geo.range_panes(rng(6, 4, 6, 4)).iter().any(|r| r.contains(handle)));
    }
}

#[cfg(test)]
mod outline_band_tests {
    use super::*;

    #[test]
    fn whole_column_outline_keeps_left_and_right_sides_near_viewport() {
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
        let col = Rect::from_min_max(pos2(100.0, 40.0), pos2(180.0, 20_971_540.0));
        let sides = outline_bands(col, 2.0, clip);
        // Left, right and top (in view); the bottom is 20 million points away.
        assert_eq!(sides.len(), 3);
        assert!(sides.iter().all(|s| s.bottom() <= clip.bottom() + 4.0 + f32::EPSILON));
        assert!(sides.iter().any(|s| s.left() < 100.0 && s.right() > 100.0 && s.height() > 500.0));
        assert!(sides.iter().any(|s| s.left() < 180.0 && s.right() > 180.0 && s.height() > 500.0));
    }

    #[test]
    fn outline_outside_the_view_and_non_finite_draw_nothing() {
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 600.0));
        assert!(outline_bands(Rect::from_min_max(pos2(900.0, 0.0), pos2(950.0, 20.0)), 2.0, clip).is_empty());
        assert!(outline_bands(Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(1.0, 1.0)), 2.0, clip).is_empty());
    }
}
