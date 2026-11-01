//! Quick Analysis: after selecting a block of two or more cells holding numbers, a small round
//! button sits just past the selection's bottom-right corner. It opens a compact card with three
//! tabs (Formatting, Charts, Totals), each a row of large tiles that run the engine command for
//! the selection: conditional formatting, a chart, or totals below the data.
//!
//! The button hides while a cell is being edited, while the pointer drags on the grid, when the
//! selection is a single (or merged) cell, and whenever a dialog or the File view is up. Escape
//! or a click anywhere else closes the card; a new selection closes it too.

use egui::{Align2, Color32, Id, Key, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::core::{CellRef, RangeRef, Value, col_to_letters};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::theme::{self, Cat, Tokens};

const STATE_ID: &str = "gridcraft.quickAnalysis";
/// The button's diameter and its gap from the selection's corner.
const BUTTON: f32 = 24.0;
const GAP: f32 = 4.0;
const TILE: egui::Vec2 = egui::Vec2 { x: 80.0, y: 78.0 };
const TILE_GAP: f32 = 6.0;
/// Cells scanned for a number before giving up (a whole-column selection stays cheap).
const SCAN_LIMIT: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Formatting,
    Charts,
    Totals,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Formatting, Tab::Charts, Tab::Totals];
    fn label(self) -> &'static str {
        match self {
            Tab::Formatting => "Formatting",
            Tab::Charts => "Charts",
            Tab::Totals => "Totals",
        }
    }
    fn blurb(self) -> &'static str {
        match self {
            Tab::Formatting => "Highlight values with rules that follow the data as it changes.",
            Tab::Charts => "Plot the selection as a chart beside the data.",
            Tab::Totals => "Add formulas that total the selection below it.",
        }
    }
}

/// One tile: what it is called, what it does, and how it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    DataBars,
    ColorScale,
    IconSet,
    GreaterThan,
    Clear,
    Column,
    Line,
    Pie,
    Scatter,
    SumRow,
    AverageRow,
    CountRow,
    RunningTotal,
}

impl Action {
    pub fn of(tab: Tab) -> &'static [Action] {
        match tab {
            Tab::Formatting => &[Action::DataBars, Action::ColorScale, Action::IconSet, Action::GreaterThan, Action::Clear],
            Tab::Charts => &[Action::Column, Action::Line, Action::Pie, Action::Scatter],
            Tab::Totals => &[Action::SumRow, Action::AverageRow, Action::CountRow, Action::RunningTotal],
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Action::DataBars => "Data Bars",
            Action::ColorScale => "Color Scale",
            Action::IconSet => "Icon Set",
            Action::GreaterThan => "Greater Than",
            Action::Clear => "Clear",
            Action::Column => "Column",
            Action::Line => "Line",
            Action::Pie => "Pie",
            Action::Scatter => "Scatter",
            Action::SumRow => "Sum",
            Action::AverageRow => "Average",
            Action::CountRow => "Count",
            Action::RunningTotal => "Running Total",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Action::DataBars => "A bar in each cell, as long as its value is large.",
            Action::ColorScale => "Shade the cells from the lowest value to the highest.",
            Action::IconSet => "An icon for low, middle and high values.",
            Action::GreaterThan => "Highlight the values above a number you choose.",
            Action::Clear => "Remove conditional formatting from the selection.",
            Action::Column => "A column chart of the selection.",
            Action::Line => "A line chart of the selection.",
            Action::Pie => "A pie chart of the selection.",
            Action::Scatter => "A scatter chart of the selection.",
            Action::SumRow => "A SUM under each column.",
            Action::AverageRow => "An AVERAGE under each column.",
            Action::CountRow => "A COUNT under each column.",
            Action::RunningTotal => "A row that adds up the columns from the first one to each.",
        }
    }
}

#[derive(Clone, Debug, Default)]
struct State {
    open: bool,
    tab: Tab,
    /// The selection the card was opened for (document, sheet, range): another closes it.
    key: String,
    /// Last frame's card and button, for clicks outside them.
    card: Option<Rect>,
    button: Option<Rect>,
}

fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp::<State>(Id::unique(STATE_ID))).unwrap_or_default()
}

fn set_state(ctx: &egui::Context, s: State) {
    ctx.data_mut(|d| d.insert_temp(Id::unique(STATE_ID), s));
}

/// Whether the card is open (tests and the control channel).
pub fn is_open(ctx: &egui::Context) -> bool {
    state(ctx).open
}

/// Opens the card on a tab (`formatting`, `charts` or `totals`) for the current selection, as a
/// click on the button does (offscreen snapshots and tests).
pub fn open(ctx: &egui::Context, tab: &str) {
    let mut st = state(ctx);
    st.open = true;
    st.tab = match tab {
        "charts" => Tab::Charts,
        "totals" => Tab::Totals,
        _ => Tab::Formatting,
    };
    set_state(ctx, st);
}

/// Where the button was drawn last frame, if it was.
pub fn button_rect(ctx: &egui::Context) -> Option<Rect> {
    state(ctx).button
}

/// The selection Quick Analysis offers itself for: one block of two or more cells (not one
/// merged cell) that holds at least one number, while nothing else has the sheet's attention.
pub fn target(app: &SheetApp) -> Option<RangeRef> {
    if app.backstage.is_some() || app.dialog.is_some() || app.editor.is_some() || app.text_box_editor.is_some() || app.selected_chart.is_some() {
        return None;
    }
    // The Auto Fill Options button takes the same corner after a fill drag.
    if app.message.is_some() || app.grid.drag != crate::grid::Drag::None || app.grid.fill_tag.is_some() {
        return None;
    }
    let doc = app.session.active()?;
    if doc.selection.ranges.len() > 1 {
        return None;
    }
    let range = doc.selection.current();
    let sh = doc.wb.active()?;
    if range.is_single() || sh.merge_at(range.start).is_some_and(|m| m.contains(range.end)) {
        return None;
    }
    let used = sh.cells.used_range()?;
    let r0 = CellRef::new(range.start.row.max(used.start.row), range.start.col.max(used.start.col));
    let r1 = CellRef::new(range.end.row.min(used.end.row), range.end.col.min(used.end.col));
    if r0.row > r1.row || r0.col > r1.col {
        return None;
    }
    let has_number = sh.cells.iter_range(RangeRef::new(r0, r1)).take(SCAN_LIMIT).any(|(_, c)| matches!(c.value, Value::Number(_)));
    has_number.then_some(range)
}

/// Draws the button and, when open, the card. Called once a frame after the grid.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let mut st = state(ctx);
    let target = target(app);
    // The pointer pressed on the grid (a drag about to start) hides it too, unless the press is
    // on the button or the card.
    let pointer = ctx.input(|i| (i.pointer.any_down(), i.pointer.press_origin()));
    let (last_button, last_card) = (st.button, st.card);
    let on_ours = |p: Option<Pos2>| p.is_some_and(|p| last_button.is_some_and(|b| b.contains(p)) || last_card.is_some_and(|c| c.contains(p)));
    let pressing_elsewhere = pointer.0 && !on_ours(pointer.1);
    let placed = target.and_then(|range| place(app, range).map(|b| (range, b)));
    let Some((range, button)) = placed.filter(|_| !pressing_elsewhere) else {
        if st.open && !pointer.0 && target.is_none() {
            st.open = false;
        }
        st.button = None;
        st.card = None;
        if pressing_elsewhere {
            st.open = false;
        }
        set_state(ctx, st);
        return;
    };
    let key = format!(
        "{}:{}:{}",
        app.session.active().map(|d| d.uid).unwrap_or(0),
        app.session.active().map(|d| d.wb.active_sheet()).unwrap_or(0),
        range.a1()
    );
    if st.key != key {
        // Opened before any selection was seen ([`open`]) keeps it; a new selection closes it.
        if !st.key.is_empty() {
            st.open = false;
        }
        st.key = key;
    }
    let t = Tokens::get(ctx);
    st.button = Some(button);

    egui::Area::new(Id::unique("quickAnalysis.button")).order(egui::Order::Middle).fixed_pos(button.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(button.size(), Sense::click());
        let p = ui.painter();
        let hovered = resp.hovered() || st.open;
        p.circle_filled(r.center() + vec2(0.0, 1.5), r.width() / 2.0 + 0.5, t.shadow);
        p.circle_filled(r.center(), r.width() / 2.0, if hovered { t.accent_soft.lerp_to_gamma(t.menu_bg, 0.3) } else { t.menu_bg });
        p.circle_stroke(r.center(), r.width() / 2.0 - 0.5, Stroke::new(1.0, if hovered { t.accent } else { t.ribbon_border }));
        chart_glyph(p, Rect::from_center_size(r.center(), vec2(12.0, 12.0)), t.accent);
        let resp = resp.on_hover_text(app.ui.language.tr("Quick Analysis"));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Quick Analysis"));
        if resp.clicked() {
            st.open = !st.open;
        }
    });

    if !st.open {
        st.card = None;
        set_state(ctx, st);
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        st.open = false;
        st.card = None;
        set_state(ctx, st);
        return;
    }
    // A press anywhere but the button and the card closes it.
    if ctx.input(|i| i.pointer.any_pressed()) && !on_ours(ctx.input(|i| i.pointer.interact_pos())) {
        st.open = false;
        st.card = None;
        set_state(ctx, st);
        return;
    }

    let n = Action::of(Tab::Formatting).len() as f32;
    let width = n * TILE.x + (n - 1.0) * TILE_GAP + 24.0;
    let height = 176.0;
    let screen = ctx.content_rect();
    let mut pos = pos2(button.left(), button.bottom() + 6.0);
    if pos.x + width > screen.right() - 8.0 {
        pos.x = (button.right() - width).max(screen.left() + 8.0);
    }
    if pos.y + height > screen.bottom() - 8.0 {
        pos.y = (button.top() - 6.0 - height).max(screen.top() + 8.0);
    }
    let mut run: Option<Action> = None;
    let anim = crate::widgets::animations_on(ctx);
    let shown = if anim { ctx.animate_bool_with_time(Id::unique("quickAnalysis.fade"), true, 0.12) } else { 1.0 };
    let area = egui::Area::new(Id::unique("quickAnalysis.card"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos + vec2(0.0, (1.0 - shown) * -6.0))
        .show(ctx, |ui| {
            ui.set_opacity(shown);
            egui::Frame::new()
                .fill(t.menu_bg)
                .stroke(Stroke::new(1.0, t.ribbon_border))
                .corner_radius(10.0)
                .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: t.shadow })
                .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 10 })
                .show(ui, |ui| {
                    ui.set_width(width - 24.0);
                    // Tabs: text, the current one in bold over a short accent bar.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for tab in Tab::ALL {
                            let text = app.ui.language.tr(tab.label());
                            let on = tab == st.tab;
                            let font = if on { theme::ui_bold(12.5) } else { theme::ui_font(12.5) };
                            let g = ui.painter().layout_no_wrap(text.to_string(), font, if on { t.text } else { t.text_dim });
                            let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0, 28.0), Sense::click());
                            if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered() && !on, t.hover) {
                                ui.painter().rect_filled(r.shrink2(vec2(0.0, 2.0)), 6.0, fill);
                            }
                            let col = if on { t.text } else { t.text_dim };
                            ui.painter().galley(pos2(r.center().x - g.size().x / 2.0, r.center().y - g.size().y / 2.0 - 1.0), g, col);
                            if on {
                                let bar = Rect::from_center_size(pos2(r.center().x, r.bottom() - 1.5), vec2((r.width() - 16.0).max(12.0), 3.0));
                                ui.painter().rect_filled(bar, 1.5, t.accent);
                            }
                            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, text));
                            if resp.clicked() {
                                st.tab = tab;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    let mut hint = st.tab.blurb();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = TILE_GAP;
                        for &a in Action::of(st.tab) {
                            let resp = tile(ui, &t, app, a);
                            if resp.hovered() {
                                hint = a.hint();
                            }
                            if resp.clicked() {
                                run = Some(a);
                            }
                        }
                    });
                    ui.add_space(8.0);
                    let mut job = egui::text::LayoutJob::simple_singleline(app.ui.language.tr(hint).to_string(), theme::ui_font(11.5), t.text_dim);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(width - 24.0);
                    ui.label(job);
                });
        });
    st.card = Some(area.response.rect);
    if let Some(a) = run {
        st.open = false;
        st.card = None;
        set_state(ctx, st);
        apply(app, a, range);
        return;
    }
    set_state(ctx, st);
}

/// The button's rectangle: just past the selection's bottom-right corner, on screen and inside the
/// cells area; `None` when that corner is scrolled out of view.
fn place(app: &SheetApp, range: RangeRef) -> Option<Rect> {
    let geo = app.grid.geo.as_ref()?;
    let sh = app.session.active()?.wb.active()?;
    let r = geo.mirror_rect(geo.range_rect(sh, range));
    let corner = if geo.rtl { r.left_bottom() } else { r.right_bottom() };
    if !geo.cells.expand(1.0).contains(corner) {
        return None;
    }
    let x = if geo.rtl { corner.x - GAP - BUTTON } else { corner.x + GAP };
    let mut b = Rect::from_min_size(pos2(x, corner.y + GAP), vec2(BUTTON, BUTTON));
    // Kept inside the cells (the last visible row or column moves it back in).
    let cells = geo.cells.shrink(2.0);
    if b.right() > cells.right() {
        b = b.translate(vec2(cells.right() - b.right(), 0.0));
    }
    if b.left() < cells.left() {
        b = b.translate(vec2(cells.left() - b.left(), 0.0));
    }
    if b.bottom() > cells.bottom() {
        b = b.translate(vec2(0.0, cells.bottom() - b.bottom()));
    }
    Some(b)
}

/// One large tile: its drawing over its name.
fn tile(ui: &mut egui::Ui, t: &Tokens, app: &SheetApp, a: Action) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(TILE, Sense::click());
    let p = ui.painter();
    // A soft tile at rest, tinted with the accent under the pointer.
    p.rect_filled(r, 10.0, t.window);
    if resp.is_pointer_button_down_on() {
        p.rect_filled(r, 10.0, t.pressed);
    } else if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.accent_soft) {
        p.rect_filled(r, 10.0, fill);
    }
    let art = Rect::from_center_size(pos2(r.center().x, r.top() + 29.0), vec2(34.0, 34.0));
    tile_art(p, art, t, a);
    let label = app.ui.language.tr(a.label());
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), theme::ui_font(11.5), t.text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 6.0);
    job.halign = egui::Align::Center;
    let g = p.layout_job(job);
    p.galley(pos2(r.center().x, r.bottom() - 13.0 - g.size().y / 2.0), g, t.text);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp
}

/// The button's glyph: three columns of a little chart.
fn chart_glyph(p: &egui::Painter, r: Rect, c: Color32) {
    let w = r.width() / 4.2;
    for (i, h) in [0.45f32, 1.0, 0.7].iter().enumerate() {
        let x = r.left() + i as f32 * (w + w * 0.6);
        let bar = Rect::from_min_max(pos2(x, r.bottom() - r.height() * h), pos2(x + w, r.bottom()));
        p.rect_filled(bar, 1.0, c);
    }
}

/// A tile's picture, drawn in code with the theme's category colours.
fn tile_art(p: &egui::Painter, r: Rect, t: &Tokens, a: Action) {
    let line = t.text_dim.gamma_multiply(0.6);
    let cell = |x0: f32, y0: f32, w: f32, h: f32| {
        Rect::from_min_size(pos2(r.left() + x0 * r.width(), r.top() + y0 * r.height()), vec2(w * r.width(), h * r.height()))
    };
    // A column of four cells, the common base of the formatting tiles.
    let column = |fill: &dyn Fn(usize) -> Option<Color32>| {
        for i in 0..4 {
            let c = cell(0.1, 0.06 + i as f32 * 0.22, 0.8, 0.2);
            p.rect_filled(c, 2.0, fill(i).unwrap_or(t.paper));
            p.rect_stroke(c, 2.0, Stroke::new(1.0, line), StrokeKind::Inside);
        }
    };
    match a {
        Action::DataBars => {
            column(&|_| None);
            for (i, w) in [0.62f32, 0.3, 0.75, 0.45].iter().enumerate() {
                let bar = cell(0.14, 0.09 + i as f32 * 0.22, w * 0.72 / 0.8, 0.14);
                p.rect_filled(bar, 1.5, t.cat(Cat::Blue));
            }
        }
        Action::ColorScale => {
            let ramp = [t.cat(Cat::Green), t.cat(Cat::Amber), t.cat(Cat::Orange), t.cat(Cat::Red)];
            column(&|i| ramp.get(i).map(|c| c.gamma_multiply(0.75)));
        }
        Action::IconSet => {
            column(&|_| None);
            for (i, c) in [Cat::Green, Cat::Amber, Cat::Red, Cat::Green].iter().enumerate() {
                let b = cell(0.1, 0.06 + i as f32 * 0.22, 0.8, 0.2);
                p.circle_filled(pos2(b.left() + b.height() * 0.75, b.center().y), b.height() * 0.3, t.cat(*c));
                p.hline(b.left() + b.height() * 1.4..=b.right() - 3.0, b.center().y, Stroke::new(1.2, line));
            }
        }
        Action::GreaterThan => {
            let hit = [false, true, false, true];
            column(&|i| hit.get(i).copied().unwrap_or(false).then(|| t.cat(Cat::Red).gamma_multiply(0.35)));
            let g = r.center();
            let s = Stroke::new(2.0, t.cat(Cat::Red));
            p.line_segment([pos2(g.x - 4.0, g.y - 5.0), pos2(g.x + 4.0, g.y)], s);
            p.line_segment([pos2(g.x + 4.0, g.y), pos2(g.x - 4.0, g.y + 5.0)], s);
        }
        Action::Clear => icons::paint_ribbon(p, r.shrink(3.0), Icon::Eraser, t.text),
        Action::Column => icons::paint_ribbon(p, r, Icon::ChartBar, t.text),
        Action::Line => icons::paint_ribbon(p, r, Icon::ChartLine, t.text),
        Action::Pie => icons::paint_ribbon(p, r, Icon::ChartPie, t.text),
        Action::Scatter => icons::paint_ribbon(p, r, Icon::ChartScatter, t.text),
        Action::SumRow | Action::AverageRow | Action::CountRow | Action::RunningTotal => {
            // Three data cells over an accent total cell with the function's mark.
            for i in 0..3 {
                let c = cell(0.1, 0.04 + i as f32 * 0.2, 0.8, 0.18);
                p.rect_filled(c, 2.0, t.paper);
                p.rect_stroke(c, 2.0, Stroke::new(1.0, line), StrokeKind::Inside);
            }
            let total = cell(0.1, 0.66, 0.8, 0.3);
            p.rect_filled(total, 3.0, t.accent_soft);
            p.rect_stroke(total, 3.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            let mark = match a {
                Action::SumRow => "SUM",
                Action::AverageRow => "AVG",
                Action::CountRow => "123",
                _ => "",
            };
            if mark.is_empty() {
                // A rising line: the running total.
                let pts = [(0.2, 0.9), (0.4, 0.83), (0.6, 0.76), (0.8, 0.68)];
                let pts: Vec<Pos2> = pts.iter().map(|(x, y)| pos2(r.left() + x * r.width(), r.top() + y * r.height())).collect();
                p.add(egui::Shape::line(pts, Stroke::new(1.6, t.accent)));
            } else {
                p.text(total.center(), Align2::CENTER_CENTER, mark, theme::ui_bold(8.5), t.accent_dark);
            }
        }
    }
}

/// The command a tile runs for `range`, with its parameters. Greater Than and Running Total are
/// handled by [`apply`] itself (a dialog, and formulas written as values).
pub fn command(a: Action, range: RangeRef) -> Option<(&'static str, Json)> {
    let r = range.a1();
    Some(match a {
        Action::DataBars => ("home.conditionalFormat", json!({"range": r, "rule": {"type": "dataBar"}})),
        Action::ColorScale => ("home.conditionalFormat", json!({"range": r, "rule": {"type": "colorScale"}})),
        Action::IconSet => ("home.conditionalFormat", json!({"range": r, "rule": {"type": "iconSet"}})),
        Action::Clear => ("home.clearRules", json!({"range": r})),
        Action::Column => ("insert.chart", json!({"range": r, "type": "column"})),
        Action::Line => ("insert.chart", json!({"range": r, "type": "line"})),
        Action::Pie => ("insert.chart", json!({"range": r, "type": "pie"})),
        Action::Scatter => ("insert.chart", json!({"range": r, "type": "scatter"})),
        Action::SumRow => ("formulas.autoSum", json!({"range": r, "function": "SUM"})),
        Action::AverageRow => ("formulas.autoSum", json!({"range": r, "function": "AVERAGE"})),
        Action::CountRow => ("formulas.autoSum", json!({"range": r, "function": "COUNT"})),
        Action::GreaterThan | Action::RunningTotal => return None,
    })
}

/// The running-total row under `range`: in its last row when that row is empty, else the row
/// below. Each column's cell adds the block from the first column through its own, so the last
/// cell is the grand total. `None` when there is no room below or a target cell holds data.
pub fn running_total(app: &SheetApp, range: RangeRef) -> Option<(String, Json)> {
    let sh = app.session.active()?.wb.active()?;
    let used = sh.cells.used_range()?;
    let last_col = range.end.col.min(used.end.col);
    let empty = |row: u32| (range.start.col..=last_col).all(|c| sh.value(CellRef::new(row, c)).is_empty());
    // A whole-column selection ends at the sheet's last row: stop just past the data instead.
    let last_row = range.end.row.min(used.end.row.saturating_add(1));
    let (data_end, out) = if last_row > range.start.row && empty(last_row) {
        (last_row - 1, last_row)
    } else {
        (range.end.row.min(used.end.row), range.end.row.min(used.end.row).checked_add(1)?)
    };
    if out >= gridcraft_engine::core::MAX_ROWS || !empty(out) {
        return None;
    }
    let first = format!("${}${}", col_to_letters(range.start.col), range.start.row + 1);
    let row: Vec<Json> = (range.start.col..=last_col).map(|c| json!(format!("=SUM({first}:{}{})", col_to_letters(c), data_end + 1))).collect();
    Some((CellRef::new(out, range.start.col).a1(), json!([row])))
}

/// Runs a tile for `range`.
pub fn apply(app: &mut SheetApp, a: Action, range: RangeRef) {
    match a {
        Action::GreaterThan => {
            // The rule's dialog, as from Highlight Cells Rules, with the selection's average to start.
            let avg = average(app, range);
            app.open_dialog("cfQuick", json!({"type": "cellIs", "operator": "greater", "title": "Greater Than"}));
            if let (Some(d), Some(v)) = (app.dialog.as_mut(), avg) {
                d.set_value("value", json!(format_number(v)));
            }
        }
        Action::RunningTotal => match running_total(app, range) {
            Some((at, values)) => app.run_or_alert("range.setValues", json!({"range": at, "values": values})),
            None => {
                app.message = Some(("Quick Analysis".into(), "There's data below the selection, so there's no room for the running total.".into()))
            }
        },
        _ => {
            if let Some((id, params)) = command(a, range) {
                app.run_or_alert(id, params);
            }
        }
    }
}

fn average(app: &SheetApp, range: RangeRef) -> Option<f64> {
    let sh = app.session.active()?.wb.active()?;
    let (mut sum, mut n) = (0.0, 0usize);
    for (_, c) in sh.cells.iter_range(range).take(SCAN_LIMIT) {
        if let Value::Number(v) = c.value {
            sum += v;
            n += 1;
        }
    }
    (n > 0).then(|| sum / n as f64)
}

/// A threshold as typed: whole numbers bare, others to two decimals.
fn format_number(v: f64) -> String {
    if v.fract().abs() < 1e-9 { format!("{v:.0}") } else { format!("{v:.2}") }
}
