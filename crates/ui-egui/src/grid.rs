//! The worksheet grid: virtualised painting of only the visible cells, frozen panes,
//! headers, selection, fill handle, column/row resizing, text overflow, borders, conditional
//! formats, tables, comments, filters, sparklines, floating charts/pictures/shapes and the
//! in-cell editor.

use std::sync::Arc;

use egui::{Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::cf::{CfCache, CfLook};
use gridcraft_engine::core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value, col_to_letters};
use gridcraft_engine::model::{BorderStyle, Color, HAlign, LineIndex, PatternType, Sheet, Style, Underline, VAlign, Workbook};
use serde_json::json;

use crate::SheetApp;
use crate::editor::{EditState, REF_COLORS};
use crate::theme::{self, Tokens};

pub const COL_HEADER_H: f32 = 21.0;
/// Sheet units are 96-dpi pixels (as in XLSX); on screen one unit is drawn at this many points,
/// which matches desktop spreadsheet density (a default 20 px row is 16 pt tall).
pub const DISPLAY_SCALE: f32 = 0.8;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Drag {
    #[default]
    None,
    Select,
    Rows(u32),
    Cols(u32),
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
    pub last_title: Option<String>,
    pub drag_select: bool,
    pub rect: Option<Rect>,
    pub cells_rect: Option<Rect>,
    pub filter_menu: Option<(u32, Pos2)>,
    pub context_menu: Option<Pos2>,
    pub marching_phase: f32,
    pub last_click: Option<(CellRef, f64)>,
    pub hover_cell: Option<CellRef>,
    pub autosave: bool,
    pub last_autosave: f64,
    pub last_border: String,
    pub last_fill: String,
    pub last_font_color: String,
    pub system_clipboard: Option<String>,
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
    /// Frame the open menu or picker was opened in, so the opening click does not dismiss it.
    pub popup_frame: u64,
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
    pub fr: u32,
    pub fc: u32,
    pub frozen_w: f32,
    pub frozen_h: f32,
    pub scroll: egui::Vec2,
    /// Prefix sums over custom row heights / column widths, built once per frame.
    ri: std::sync::Arc<LineIndex>,
    ci: std::sync::Arc<LineIndex>,
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
        let cells = Rect::from_min_max(pos2(rect.left() + header_w, rect.top() + header_h), rect.max);
        Geo { rect, cells, z, header_w, header_h, fr, fc, frozen_w, frozen_h, scroll, ri, ci }
    }
    pub fn x(&self, _sh: &Sheet, col: u32) -> f32 {
        if col < self.fc {
            self.cells.left() + self.ci.start(col) as f32 * self.z
        } else {
            self.cells.left() + self.frozen_w + (self.ci.start(col) as f32 - self.ci.start(self.fc) as f32 - self.scroll.x) * self.z
        }
    }
    pub fn y(&self, _sh: &Sheet, row: u32) -> f32 {
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
        let dx = (x - self.cells.left()) / self.z;
        if dx < self.frozen_w / self.z {
            self.ci.at(dx.max(0.0) as f64)
        } else {
            self.ci.at(self.ci.start(self.fc) + self.scroll.x as f64 + (dx - self.frozen_w / self.z) as f64)
        }
    }
    pub fn row_at(&self, _sh: &Sheet, y: f32) -> u32 {
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
    if code == "checkbox" {
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
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    app.grid.rect = Some(rect);
    let Some(d) = app.session.active() else { return };
    let wb: Arc<Workbook> = d.wb.clone();
    let si = wb.active_sheet;
    let Some(sh) = wb.sheet(si) else { return };
    let sel = d.selection.clone();
    let mut view = app.view();
    // Scrolling.
    let (scroll_delta, zoom_delta, modifiers) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
    if resp.hovered() || resp.contains_pointer() {
        if zoom_delta != 1.0 {
            let z = (sh.zoom as f32 * zoom_delta).clamp(10.0, 400.0).round();
            let _ = app.session.run("view.zoom", json!({"percent": z}));
        } else if scroll_delta != egui::Vec2::ZERO {
            let z = sh.zoom as f32 / 100.0 * DISPLAY_SCALE;
            let mut d = scroll_delta;
            if modifiers.shift && d.x == 0.0 {
                d = vec2(d.y, 0.0);
            }
            view.scroll -= d / z;
        }
    }
    view.scroll = view.scroll.max(egui::Vec2::ZERO);
    let max_y = sh.row_top(MAX_ROWS - 40) as f32;
    let max_x = sh.col_left(MAX_COLS - 20) as f32;
    view.scroll.x = view.scroll.x.min(max_x);
    view.scroll.y = view.scroll.y.min(max_y);
    let mut geo = Geo::new(sh, rect, view.scroll);
    // Keep the active cell visible after keyboard moves.
    if app.grid.ensure_visible {
        app.grid.ensure_visible = false;
        let target = app.editor.as_ref().and_then(|e| e.point_cell).unwrap_or(sel.active);
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
        geo = Geo::new(sh, rect, view.scroll);
    }
    // Snap scrolling to whole rows/columns at rest like Excel? Keep smooth (better on trackpads).
    if let Some(v) = app.view_mut() {
        *v = view;
    }
    app.grid.cells_rect = Some(geo.cells);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, t.grid_bg);
    let mut cf = CfCache::new();
    let char_w = ui.fonts_mut(|f| f.glyph_width(&FontId::new(11.0 * 96.0 / 72.0 * geo.z, egui::FontFamily::Name(theme::CELL.into())), '0')).max(1.0);
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
        paint_quadrant(&p, &geo, &wb, si, sh, *rows, *cols, char_w, &mut cf, &t);
        paint_selection(&p, &geo, sh, &sel, &t, app);
    }
    // Freeze lines.
    if geo.fr > 0 {
        painter.line_segment([pos2(geo.cells.left(), split.y), pos2(geo.cells.right(), split.y)], Stroke::new(1.0, Color32::from_gray(150)));
    }
    if geo.fc > 0 {
        painter.line_segment([pos2(split.x, geo.cells.top()), pos2(split.x, geo.cells.bottom())], Stroke::new(1.0, Color32::from_gray(150)));
    }
    // Objects above cells.
    crate::chartview::paint_objects(app, &painter.with_clip_rect(geo.cells), &geo, &wb, si, sh);
    paint_overlays(app, ui, &painter.with_clip_rect(geo.cells), &geo, &wb, si, sh, &sel);
    if app.session.view_mode == "pageBreakPreview" || app.session.view_mode == "pageLayout" {
        paint_pages(app, &painter.with_clip_rect(geo.cells), &geo, sh);
    }
    // Headers.
    if sh.show_headings {
        paint_headers(&painter, &geo, sh, &sel, &t, &quads);
    }
    // Interaction.
    interact(app, ui, &resp, &geo, sh, &wb);
    if app.editor.as_ref().is_some_and(|e| !e.from_formula_bar) {
        in_cell_editor(app, ui, &geo, sh, &wb);
    }
    filter_menu(app, ui, &geo);
    list_picker(app, ui, &geo);
    header_menu(app, ui);
    context_menu(app, ui);
    if app.session.clipboard.is_some() {
        app.grid.marching_phase = (app.grid.marching_phase + 0.5) % 8.0;
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_quadrant(
    p: &Painter,
    geo: &Geo,
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    rows: (u32, u32),
    cols: (u32, u32),
    char_w: f32,
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
        if lk.style.num_fmt.as_str() == "checkbox" {
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
        let st = &lk.style;
        let size = st.font.size * 96.0 / 72.0 * z;
        let fam = theme::cell_family(&st.font.name, st.font.bold, st.font.italic);
        let font = FontId::new(size, fam);
        let avail_w = rect.right() - text_left;
        let (text, ncolor, numeric, fill_char) = display_text(wb, sh, c, &v, st, avail_w / z.max(0.1) * z, char_w);
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
        if st.align.wrap || st.align.h == HAlign::Justify || text.contains('\n') && st.align.wrap {
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
            let y = match st.align.v {
                VAlign::Top => rect.top() + 1.0,
                VAlign::Center => rect.center().y - galley.size().y / 2.0,
                _ => rect.bottom() - galley.size().y - 1.0,
            };
            p.with_clip_rect(rect.intersect(p.clip_rect())).galley(pos2(x, y), galley, color);
            continue;
        }
        let first_line = text.lines().next().unwrap_or("").to_string();
        let mut galley = p.layout_no_wrap(first_line.clone(), font.clone(), color);
        let mut w = galley.size().x;
        // Numbers and dates never overflow: they show #### when too narrow.
        if (numeric || matches!(v, Value::Number(_))) && w > avail_w - 2.0 * pad && st.align.h != HAlign::Fill {
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
        // Overflow into empty neighbours for text.
        let mut clip = *rect;
        if !numeric && !matches!(v, Value::Number(_)) && w > rect.width() - 2.0 * pad && in_merge(c).is_none() {
            let mut right = rect.right();
            let mut left = rect.left();
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
            HAlign::Center | HAlign::CenterAcross | HAlign::Distributed => rect.center().x - w / 2.0,
            HAlign::Right => rect.right() - pad - w - indent,
            _ => text_left + pad + indent,
        };
        let gh = galley.size().y;
        let y = match st.align.v {
            VAlign::Top => rect.top() + 1.0,
            VAlign::Center | VAlign::Justify | VAlign::Distributed => rect.center().y - gh / 2.0,
            VAlign::Bottom => rect.bottom() - gh - 0.5 * z,
        };
        let cp = p.with_clip_rect(clip.intersect(p.clip_rect()));
        if st.align.rotation != 0 && st.align.rotation != 255 {
            let angle = -(st.align.rotation as f32).to_radians();
            let mut shape = egui::epaint::TextShape::new(pos2(rect.left() + pad, rect.bottom() - pad), galley.clone(), color);
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
        let edges = [
            (b.top, [rect.left_top(), rect.right_top()]),
            (b.bottom, [rect.left_bottom(), rect.right_bottom()]),
            (b.left, [rect.left_top(), rect.left_bottom()]),
            (b.right, [rect.right_top(), rect.right_bottom()]),
        ];
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
    let s = (rect.height() - 4.0).clamp(10.0, 18.0);
    Rect::from_min_size(pos2(rect.right() - s - 2.0, rect.bottom() - s - 2.0), vec2(s, s))
}

fn paint_border(p: &Painter, pts: [Pos2; 2], style: BorderStyle, col: Color32) {
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

fn paint_selection(p: &Painter, geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection, t: &Tokens, app: &SheetApp) {
    // Fill all areas except the active cell.
    for r in &sel.ranges {
        if r.is_single() && sel.ranges.len() == 1 {
            continue;
        }
        let rect = geo.range_rect(sh, *r);
        p.rect_filled(rect, 0.0, t.sel_fill);
    }
    let active = geo.cell_rect(sh, sh.merge_at(sel.active).map(|m| m.start).unwrap_or(sel.active));
    let active = match sh.merge_at(sel.active) {
        Some(m) => geo.range_rect(sh, m),
        None => active,
    };
    if !(sel.ranges.len() == 1 && sel.current().is_single()) {
        p.rect_filled(active.shrink(1.0), 0.0, t.grid_bg.gamma_multiply(0.0));
    }
    let cur = geo.range_rect(sh, sel.current());
    // Editing a formula: outline its references instead of the selection.
    if let Some(ed) = &app.editor
        && ed.is_formula()
    {
        for (_, _, r, sheet, idx) in crate::editor::formula_refs(&ed.text) {
            if sheet.as_deref().is_some_and(|s| !s.eq_ignore_ascii_case(&sh.name)) {
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
    p.rect_stroke(cur.expand(0.5), 0.0, Stroke::new(2.0, t.sel_border), StrokeKind::Middle);
    if sel.ranges.len() > 1 {
        p.rect_stroke(active, 0.0, Stroke::new(1.0, t.sel_border), StrokeKind::Inside);
    }
    // Fill handle (bottom-right of the current area).
    if app.editor.is_none() {
        let h = Rect::from_center_size(cur.right_bottom(), vec2(6.0, 6.0));
        p.rect_filled(h.expand(1.0), 0.0, Color32::WHITE);
        p.rect_filled(h, 0.0, t.sel_border);
    }
    // Fill-handle drag preview.
    if let crate::grid::Drag::Fill { target: Some(tr) } = &app.grid.drag {
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
        let rr = geo.range_rect(sh, moved);
        p.rect_stroke(rr, 0.0, Stroke::new(2.0, Color32::from_gray(110)), StrokeKind::Middle);
    }
    // Marching ants around the copied range.
    if let Some(clip) = &app.session.clipboard
        && app.session.active().is_some_and(|d| d.uid == clip.doc_uid && d.wb.active_sheet == clip.sheet)
    {
        let rr = geo.range_rect(sh, clip.range).expand(0.5);
        let phase = app.grid.marching_phase;
        let pts = [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom(), rr.left_top()];
        p.extend(egui::Shape::dashed_line_with_offset(&pts, Stroke::new(2.0, t.sel_border), &[4.0], &[4.0], phase));
    }
}

fn paint_headers(p: &Painter, geo: &Geo, sh: &Sheet, sel: &gridcraft_engine::Selection, t: &Tokens, quads: &[((u32, u32), (u32, u32), Rect)]) {
    let font = theme::ui_font((11.5 * (geo.z / DISPLAY_SCALE).max(0.75)).min(16.0));
    let r = geo.rect;
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

fn interact(app: &mut SheetApp, ui: &mut egui::Ui, resp: &egui::Response, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    let ctx = ui.ctx().clone();
    let pointer = ctx.input(|i| i.pointer.clone());
    let mods = ctx.input(|i| i.modifiers);
    let pos = pointer.interact_pos().or(pointer.hover_pos());
    let in_cells = pos.is_some_and(|p| geo.cells.contains(p));
    let in_col_header = pos.is_some_and(|p| p.y < geo.cells.top() && p.y >= geo.rect.top() && p.x >= geo.cells.left());
    let in_row_header = pos.is_some_and(|p| p.x < geo.cells.left() && p.x >= geo.rect.left() && p.y >= geo.cells.top());
    let in_corner = pos.is_some_and(|p| p.x < geo.cells.left() && p.y < geo.cells.top() && geo.rect.contains(p));
    let sel = app.session.active().map(|d| d.selection.clone()).unwrap_or_default();
    let cur_rect = geo.range_rect(sh, sel.current());
    let handle = Rect::from_center_size(cur_rect.right_bottom(), vec2(9.0, 9.0));

    // Hover cursors.
    if resp.hovered()
        && let Some(p) = pos
    {
        let near_col_edge = in_col_header && col_edge(geo, sh, p.x).is_some();
        let near_row_edge = in_row_header && row_edge(geo, sh, p.y).is_some();
        if near_col_edge {
            ctx.set_cursor_icon(CursorIcon::ResizeColumn);
        } else if near_row_edge {
            ctx.set_cursor_icon(CursorIcon::ResizeRow);
        } else if handle.contains(p) && app.editor.is_none() {
            ctx.set_cursor_icon(CursorIcon::Crosshair);
        } else if in_cells && app.editor.is_none() && on_border(cur_rect, p) {
            ctx.set_cursor_icon(CursorIcon::Move);
        } else if in_cells {
            ctx.set_cursor_icon(CursorIcon::Cell);
        } else if in_col_header {
            ctx.set_cursor_icon(CursorIcon::Default);
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
                let sx = sh.col_left(geo.fc) as f32 + geo.scroll.x + (p.x - geo.cells.left() - geo.frozen_w) / geo.z;
                let sy = sh.row_top(geo.fr) as f32 + geo.scroll.y + (p.y - geo.cells.top() - geo.frozen_h) / geo.z;
                app.grid.ink.push([sx, sy]);
            }
            if !app.grid.ink.is_empty() {
                // Live preview.
                let pts: Vec<Pos2> = app
                    .grid
                    .ink
                    .iter()
                    .map(|q| {
                        pos2(
                            geo.cells.left() + geo.frozen_w + (q[0] - sh.col_left(geo.fc) as f32 - geo.scroll.x) * geo.z,
                            geo.cells.top() + geo.frozen_h + (q[1] - sh.row_top(geo.fr) as f32 - geo.scroll.y) * geo.z,
                        )
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
        if app.session.draw_tool == "eraser" && (resp.clicked() || resp.dragged()) {
            if let Some(p) = pos
                && let Some(("shape", id, _)) = crate::chartview::hit(app, geo, sh, p)
                && sh.shapes.iter().any(|s| s.id == id && s.kind == gridcraft_engine::model::ShapeKind::Ink)
            {
                let _ = app.run("object.delete", json!({"kind": "shape", "id": id}));
            }
            return;
        }
    }
    // Objects (charts, pictures, shapes) take clicks first.
    if resp.drag_started() || resp.clicked() {
        if let Some(p) = pos
            && let Some((kind, id, rect)) = crate::chartview::hit(app, geo, sh, p)
        {
            app.selected_chart = Some(id);
            if kind == "chart" && resp.double_clicked() {
                app.grid.pane = Some("formatChart".into());
            }
            if resp.drag_started() {
                let a = object_anchor(sh, kind, id);
                let resize = (p - rect.right_bottom()).length() < 10.0;
                if let Some(a) = a {
                    app.grid.drag = Drag::Object { kind, id, start: p, orig: (a.cell, a.dx, a.dy, a.width, a.height), resize };
                }
            }
            return;
        } else if resp.clicked() || resp.drag_started() {
            app.selected_chart = None;
        }
    }

    // Press: decide the drag mode.
    if resp.drag_started() || (resp.clicked() && app.grid.drag == Drag::None) {
        // Where the press began (a drag is only recognised after the pointer has moved).
        let Some(p) = pointer.press_origin().or(pos) else { return };
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
            // The arrow toggles the list (Excel behaviour: pressing it again closes it).
            app.grid.list_picker = (app.grid.list_picker != Some(sel.active)).then_some(sel.active);
            app.grid.popup_frame = ui.ctx().cumulative_frame_nr();
            return;
        }
        // Filter dropdown buttons.
        if is_filter_button(sh, geo, p) {
            app.grid.filter_menu = Some((c.col, p));
            app.grid.popup_frame = ui.ctx().cumulative_frame_nr();
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
            return;
        }
        if app.editor.is_some() && !app.commit_edit(0, 0, false, false) {
            return;
        }
        if handle.contains(p) {
            app.grid.drag = Drag::Fill { target: None };
            return;
        }
        if on_border(cur_rect, p) && resp.drag_started() {
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
            let mut ranges: Vec<String> = sel.ranges.iter().map(|r| r.a1()).collect();
            ranges.push(c.a1());
            let _ = app.session.run("selection.set", json!({"range": ranges.join(","), "active": c.a1()}));
        } else {
            let _ = app.session.run("selection.set", json!({"cell": c.a1()}));
            if resp.clicked() && wb.styles.get(sh.style_id(c)).num_fmt.as_str() == "checkbox" {
                let _ = app.run("cell.toggleCheckbox", json!({"cell": c.a1()}));
            }
            // A click on a hyperlink's text follows it (like Excel); elsewhere in the cell selects.
            if let Some(h) = sh.hyperlinks.get(&c).cloned()
                && resp.clicked()
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
                if let Some(ed) = app.editor.as_mut()
                    && ed.point.is_some()
                {
                    if let Some(start) = ed.point_cell {
                        let r = RangeRef::new(start, pc);
                        let text = if r.is_single() { r.start.a1() } else { r.a1() };
                        let keep = ed.point_cell;
                        ed.insert_ref(&text);
                        ed.point_cell = keep;
                    }
                } else {
                    let anchor = sel.anchor;
                    let mut ranges: Vec<String> = sel.ranges.iter().map(|r| r.a1()).collect();
                    ranges.pop();
                    ranges.push(RangeRef::new(anchor, pc).a1());
                    let _ = app.session.run("selection.set", json!({"range": ranges.join(","), "active": sel.active.a1()}));
                    if let Some(d) = app.session.active_mut() {
                        d.selection.anchor = anchor;
                    }
                }
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
            Drag::ResizeCol { col, start, orig } => {
                let w = (orig + (p.x - start) / geo.z).max(0.0);
                app.toast =
                    Some((format!("Width: {:.2} ({} pixels)", gridcraft_engine::cmd::format::points_to_chars(w as f64), w.round()), crate::now_ms()));
                let cols = if sel.ranges.iter().any(|r| r.is_full_cols() && col >= r.start.col && col <= r.end.col) {
                    sel.current().a1()
                } else {
                    RangeRef::cols(col, col).a1()
                };
                let _ = app.session.run("home.columnWidth", json!({"cols": cols, "width": w.round()}));
                coalesce_last_undo(app);
            }
            Drag::ResizeRow { row, start, orig } => {
                let h = (orig + (p.y - start) / geo.z).max(0.0);
                app.toast = Some((format!("Height: {:.2} ({} pixels)", h * 0.75, h.round()), crate::now_ms()));
                let rows = if sel.ranges.iter().any(|r| r.is_full_rows() && row >= r.start.row && row <= r.end.row) {
                    format!("{}:{}", sel.current().start.row + 1, sel.current().end.row + 1)
                } else {
                    format!("{}:{}", row + 1, row + 1)
                };
                let _ = app.session.run("home.rowHeight", json!({"rows": rows, "height": h.round()}));
                coalesce_last_undo(app);
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
            Drag::Fill { target: Some(t) } => {
                let src = sel.current();
                if t != src {
                    let mode = if mods.command { "copy" } else { "series" };
                    if let Err(e) = app.run("edit.autoFill", json!({"source": src.a1(), "target": t.a1(), "mode": mode})) {
                        app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                    }
                }
            }
            Drag::Move { to: Some(to), .. } => {
                let src = sel.current();
                if to != src.start {
                    let _ = app.session.run("edit.cut", json!({"range": src.a1()}));
                    let _ = app.run("edit.paste", json!({"at": to.a1()}));
                }
            }
            _ => {}
        }
        app.grid.drag_select = false;
    }

    // Double-click on header edges autofits.
    if resp.double_clicked()
        && let Some(p) = pos
    {
        if in_col_header && let Some((c, _)) = col_edge(geo, sh, p.x) {
            let _ = app.run("home.autofitColumnWidth", json!({"cols": RangeRef::cols(c, c).a1()}));
        } else if in_row_header && let Some((r, _)) = row_edge(geo, sh, p.y) {
            let _ = app.run("home.autofitRowHeight", json!({"rows": format!("{}:{}", r + 1, r + 1)}));
        } else if handle.contains(p) {
            // Double-click the fill handle: fill down as far as the neighbouring column goes.
            let src = sel.current();
            let neighbour = if src.start.col > 0 { src.start.col - 1 } else { src.end.col + 1 };
            let mut last = src.end.row;
            while last + 1 < MAX_ROWS
                && sh.value_ref(CellRef::new(last + 1, neighbour)).is_some_and(|v| !v.is_empty())
                && last - src.end.row < 1_000_000
            {
                last += 1;
            }
            if last > src.end.row {
                let target = RangeRef::new(src.start, CellRef::new(last, src.end.col));
                let _ = app.run("edit.autoFill", json!({"source": src.a1(), "target": target.a1()}));
            }
        }
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
            app.grid.popup_frame = ui.ctx().cumulative_frame_nr();
            app.grid.header_menu = Some((p, rows));
        } else {
            app.grid.context_menu = Some(p);
        }
    }

    if resp.clicked() || resp.drag_started() {
        resp.request_focus();
    }
    keyboard(app, &ctx, resp, geo, sh);
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

fn col_edge(geo: &Geo, sh: &Sheet, x: f32) -> Option<(u32, f32)> {
    let c = geo.col_at(sh, x);
    for cc in [c, c.saturating_sub(1)] {
        let edge = geo.x(sh, cc + 1);
        if (x - edge).abs() <= 4.0 {
            return Some((cc, edge));
        }
    }
    None
}

fn row_edge(geo: &Geo, sh: &Sheet, y: f32) -> Option<(u32, f32)> {
    let r = geo.row_at(sh, y);
    for rr in [r, r.saturating_sub(1)] {
        let edge = geo.y(sh, rr + 1);
        if (y - edge).abs() <= 3.0 {
            return Some((rr, edge));
        }
    }
    None
}

fn is_filter_button(sh: &Sheet, geo: &Geo, p: Pos2) -> bool {
    let c = geo.cell_at(sh, p);
    let header = sh.autofilter.as_ref().is_some_and(|af| af.range.start.row == c.row && c.col >= af.range.start.col && c.col <= af.range.end.col)
        || sh
            .tables
            .iter()
            .any(|t| t.filter_button && t.header_row && t.range.start.row == c.row && c.col >= t.range.start.col && c.col <= t.range.end.col);
    header && filter_button_rect(geo, sh, c).contains(p)
}

fn object_anchor(sh: &Sheet, kind: &str, id: u32) -> Option<gridcraft_engine::model::Anchor> {
    match kind {
        "chart" => sh.charts.iter().find(|c| c.id == id).map(|c| c.anchor),
        "image" => sh.images.iter().find(|c| c.id == id).map(|c| c.anchor),
        _ => sh.shapes.iter().find(|c| c.id == id).map(|c| c.anchor),
    }
}

pub fn follow_link(app: &mut SheetApp, target: &str) {
    if target.contains("://") || target.starts_with("mailto:") {
        if let Some(f) = &app.services.open_url {
            f(target);
        }
    } else {
        let t = target.trim_start_matches('#');
        let _ = app.session.run("edit.goTo", json!({"reference": t}));
        app.grid.ensure_visible = true;
    }
}

fn select_ranges(app: &mut SheetApp, r: RangeRef, add: bool, active: CellRef) {
    let mut ranges: Vec<String> =
        if add { app.session.active().map(|d| d.selection.ranges.iter().map(|x| x.a1()).collect()).unwrap_or_default() } else { vec![] };
    ranges.push(r.a1());
    let _ = app.session.run("selection.set", json!({"range": ranges.join(","), "active": active.a1()}));
}

/// Grid keyboard handling when no text field has focus.
fn keyboard(app: &mut SheetApp, ctx: &egui::Context, resp: &egui::Response, geo: &Geo, sh: &Sheet) {
    // Focus left on an editor that no longer exists goes back to the grid.
    let stale = [egui::Id::new("gridcraft.cell_editor"), egui::Id::new("gridcraft.formula_bar")];
    if app.editor.is_none() && ctx.memory(|m| m.focused()).is_some_and(|f| stale.contains(&f)) {
        resp.request_focus();
    }
    let other_focus = ctx.memory(|m| m.focused()).is_some_and(|f| f != resp.id && !(app.editor.is_none() && stale.contains(&f)));
    if app.editor.is_some() || app.dialog.is_some() || other_focus || app.message.is_some() {
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
    let page = ((geo.cells.height() / geo.z) / sh.default_row_height).floor().max(1.0) as i64;
    for ev in events {
        match ev {
            egui::Event::Copy => {
                if let Ok(r) = app.run("edit.copy", json!({}))
                    && let Some(t) = r.get("text").and_then(|t| t.as_str())
                {
                    ctx.copy_text(t.to_string());
                }
            }
            egui::Event::Cut => {
                if let Ok(r) = app.run("edit.cut", json!({}))
                    && let Some(t) = r.get("text").and_then(|t| t.as_str())
                {
                    ctx.copy_text(t.to_string());
                }
            }
            egui::Event::Paste(text) => app.run_or_alert("edit.paste", json!({"text": text})),
            egui::Event::Text(text) => {
                if text.chars().all(|c| !c.is_control()) && !text.is_empty() {
                    app.begin_edit(Some(text), false);
                    if let Some(ed) = app.editor.as_mut() {
                        ed.caret = ed.text.chars().count();
                    }
                    return;
                }
            }
            egui::Event::Key { key, pressed: true, modifiers: m, .. } => {
                use egui::Key;
                let cmd = m.command;
                let shift = m.shift;
                let mv = |app: &mut SheetApp, dr: i64, dc: i64| {
                    let _ = app.session.run("selection.move", json!({"dr": dr, "dc": dc, "extend": shift, "jump": cmd}));
                    app.grid.ensure_visible = true;
                };
                match key {
                    Key::ArrowUp => mv(app, -1, 0),
                    Key::ArrowDown => mv(app, 1, 0),
                    Key::ArrowLeft => mv(app, 0, -1),
                    Key::ArrowRight => mv(app, 0, 1),
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
                    Key::Enter => {
                        if m.alt || m.ctrl {
                            app.begin_edit(None, false);
                        } else {
                            app.move_after_enter(if shift { -1 } else { 1 }, 0);
                        }
                    }
                    Key::Tab => app.move_after_enter(0, if shift { -1 } else { 1 }),
                    Key::F2 => app.begin_edit(None, false),
                    Key::Delete => app.run_or_alert("edit.clearContents", json!({})),
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
                    Key::Space if cmd && !shift => app.run_or_alert("selection.column", json!({})),
                    Key::Space if shift && !cmd => app.run_or_alert("selection.row", json!({})),
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
    let erect = Rect::from_min_size(rect.min, vec2(w, h));
    let fill = match st.fill.pattern {
        PatternType::Solid => st.fill.fg.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::WHITE),
        _ => Color32::WHITE,
    };
    ui.painter().rect_filled(erect, 0.0, fill);
    ui.painter().rect_stroke(erect.expand(1.0), 0.0, Stroke::new(2.0, Tokens::get(ui.ctx()).sel_border), StrokeKind::Middle);
    let id = egui::Id::new("gridcraft.cell_editor");
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(erect.shrink2(vec2(2.0, 1.0))));
    crate::formula_bar::editor_widget(app, &mut child, id, font, false, erect.width() - 4.0);
}

/// True when a press landed outside `rect` on a later frame than `frame`: clicking anywhere else
/// in the app dismisses a transient menu or picker, but the click that opened it does not.
fn pressed_outside(ui: &egui::Ui, frame: u64, rect: Rect) -> bool {
    ui.ctx().cumulative_frame_nr() != frame && ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)))
}

fn filter_menu(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo) {
    let Some((col, at)) = app.grid.filter_menu else { return };
    let Some(d) = app.session.active() else { return };
    let wb = d.wb.clone();
    let si = wb.active_sheet;
    let Some(sh) = wb.sheet(si) else { return };
    let _ = geo;
    // Tables get an autofilter on demand.
    let table_range = sh
        .tables
        .iter()
        .find(|t| t.range.start.row <= at.y as u32 || true)
        .filter(|t| col >= t.range.start.col && col <= t.range.end.col)
        .map(|t| t.range);
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
    let mut close = false;
    let area = egui::Area::new(egui::Id::new("filter_menu")).fixed_pos(at + vec2(-180.0, 10.0)).order(egui::Order::Foreground);
    let resp = area.show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(230.0);
            if ui.button("↑  Sort Ascending").clicked() {
                let _ = app.run("data.sortAscending", json!({"column": col_to_letters(col)}));
                close = true;
            }
            if ui.button("↓  Sort Descending").clicked() {
                let _ = app.run("data.sortDescending", json!({"column": col_to_letters(col)}));
                close = true;
            }
            ui.separator();
            ui.label(egui::RichText::new("Filter").strong());
            let key = egui::Id::new(("filter_sel", col));
            let mut checks: Vec<(String, bool)> = ui.ctx().data_mut(|d| d.get_temp::<Vec<(String, bool)>>(key)).unwrap_or(values.clone());
            let all = checks.iter().all(|(_, b)| *b);
            let mut all_new = all;
            if ui.checkbox(&mut all_new, "(Select All)").changed() {
                for c in checks.iter_mut() {
                    c.1 = all_new;
                }
            }
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for (v, on) in checks.iter_mut() {
                    ui.checkbox(on, v.as_str());
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(key, checks.clone()));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Clear Filter").clicked() {
                    let _ = app.run("data.filterBy", json!({"column": col_to_letters(col), "clear": true}));
                    ui.ctx().data_mut(|d| d.remove::<Vec<(String, bool)>>(key));
                    close = true;
                }
                if ui.button("Apply").clicked() {
                    let blanks = checks.iter().any(|(v, on)| v == "(Blanks)" && *on);
                    let vals: Vec<String> = checks.iter().filter(|(v, on)| *on && v != "(Blanks)").map(|(v, _)| v.clone()).collect();
                    let p = if checks.iter().all(|(_, b)| *b) {
                        json!({"column": col_to_letters(col), "clear": true})
                    } else {
                        json!({"column": col_to_letters(col), "values": vals, "blanks": blanks})
                    };
                    let _ = app.run("data.filterBy", p);
                    ui.ctx().data_mut(|d| d.remove::<Vec<(String, bool)>>(key));
                    close = true;
                }
            });
        });
    });
    if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) || pressed_outside(ui, app.grid.popup_frame, resp.response.rect) {
        app.grid.filter_menu = None;
    }
}

fn context_menu(app: &mut SheetApp, ui: &mut egui::Ui) {
    let Some(at) = app.grid.context_menu else { return };
    let mut close = false;
    let resp = egui::Area::new(egui::Id::new("cell_context")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(200.0);
            let items: &[(&str, &str)] = &[
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
            for (label, id) in items {
                if *label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(*label).frame(false).min_size(vec2(190.0, 20.0))).clicked() {
                    close = true;
                    match *id {
                        "ui:pasteSpecial" => app.open_dialog("pasteSpecial", json!({})),
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
                            if let Ok(r) = app.run(id, json!({}))
                                && let Some(t) = r.get("text").and_then(|t| t.as_str())
                            {
                                ui.ctx().copy_text(t.to_string());
                            }
                        }
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
        let anchor = ed.point.and_then(|(s, e)| ed.text.get(s..e)).and_then(|t| CellRef::parse(t.split(':').next().unwrap_or(""))).unwrap_or(from);
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
    if dv.kind != gridcraft_engine::model::ValidationKind::List || !dv.in_cell_dropdown {
        return None;
    }
    let r = geo.cell_rect(sh, c);
    let s = r.height().clamp(14.0, 22.0);
    Some(Rect::from_min_size(pos2(r.right() + 1.0, r.bottom() - s), vec2(s, s)))
}

#[allow(clippy::too_many_arguments)]
fn paint_overlays(
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
    // Input message of the active cell's validation.
    if app.editor.is_none()
        && let Some(dv) = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(sel.active)))
        && dv.show_input
        && !(dv.input_title.is_empty() && dv.input_message.is_empty())
    {
        let r = geo.cell_rect(sh, sel.active);
        let at = pos2(r.left() + 8.0, r.bottom() + 6.0);
        egui::Area::new(egui::Id::new("dv_input")).fixed_pos(at).order(egui::Order::Tooltip).interactable(false).show(ui.ctx(), |ui| {
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
    for (c, cm) in &sh.comments {
        let show = cm.visible || hover == Some(*c);
        if !show || app.grid.drag != Drag::None {
            continue;
        }
        let r = geo.cell_rect(sh, *c);
        if !p.clip_rect().intersects(r) {
            continue;
        }
        let at = pos2(r.right() + 10.0, r.top());
        p.line_segment([r.right_top(), at + vec2(0.0, 6.0)], Stroke::new(1.0, Color32::from_gray(90)));
        egui::Area::new(egui::Id::new(("note", c.row, c.col))).fixed_pos(at).order(egui::Order::Tooltip).interactable(false).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).fill(if cm.threaded { Color32::WHITE } else { Color32::from_rgb(0xFF, 0xFF, 0xE1) }).show(ui, |ui| {
                ui.set_max_width(240.0);
                ui.label(egui::RichText::new(format!("{}:", cm.author)).strong().color(Color32::BLACK));
                ui.label(egui::RichText::new(&cm.text).color(Color32::BLACK));
                for (a, t) in &cm.replies {
                    ui.separator();
                    ui.label(egui::RichText::new(format!("{a}: {t}")).color(Color32::BLACK));
                }
            });
        });
    }
    // Hyperlink tooltip.
    if let Some(h) = hover.and_then(|c| sh.hyperlinks.get(&c))
        && app.editor.is_none()
    {
        let tip = h.tooltip.clone().unwrap_or_else(|| format!("{}\nClick once to follow. Click and hold to select this cell.", h.target));
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
                p.rect_stroke(tr, 0.0, Stroke::new(1.5, blue), StrokeKind::Inside);
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

fn list_picker(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo) {
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
    let mut close = false;
    let resp = egui::Area::new(egui::Id::new("dv_list")).fixed_pos(r.left_bottom()).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
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
    if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) || pressed_outside(ui, app.grid.popup_frame, resp.response.rect) {
        app.grid.list_picker = None;
    }
}

fn header_menu(app: &mut SheetApp, ui: &mut egui::Ui) {
    let Some((at, rows)) = app.grid.header_menu else { return };
    let mut close = false;
    let resp = egui::Area::new(egui::Id::new("header_menu")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(190.0);
            let items: Vec<(&str, &str)> = if rows {
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
            for (label, id) in items {
                if label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(label).frame(false).min_size(vec2(180.0, 20.0))).clicked() {
                    close = true;
                    match id {
                        "ui:rowHeight" => app.open_dialog("rowHeight", json!({})),
                        "ui:columnWidth" => app.open_dialog("columnWidth", json!({})),
                        "edit.copy" | "edit.cut" => {
                            if let Ok(r) = app.run(id, json!({}))
                                && let Some(t) = r.get("text").and_then(|t| t.as_str())
                            {
                                ui.ctx().copy_text(t.to_string());
                            }
                        }
                        other => app.run_or_alert(other, json!({})),
                    }
                }
            }
        });
    });
    if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) || pressed_outside(ui, app.grid.popup_frame, resp.response.rect) {
        app.grid.header_menu = None;
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
            format!("Page {n}"),
            theme::ui_bold((rr.height().min(rr.width()) / 6.0).clamp(14.0, 72.0)),
            Color32::from_black_alpha(45),
        );
    }
    p.rect_stroke(pr, 0.0, Stroke::new(3.0, blue), StrokeKind::Outside);
}
