//! Quick Analysis (Ctrl+Q): a small button at the bottom-right corner of a multi-cell selection
//! that opens a popup of one-click analyses. The popup is a thin front end: every tile runs an
//! engine command through `app.run` (conditional formats, charts, `analysis.addTotal`, tables,
//! pivot tables, `analysis.addSparklines`), so nothing here is UI-only logic worth testing apart
//! from which command a tile maps to ([`tiles`]).

use egui::{Align2, Color32, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::core::RangeRef;
use gridcraft_engine::model::Sheet;
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::grid::{Drag, Geo};
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Side of the corner button, and its gap to the selection's corner (clear of the fill handle).
const BUTTON: f32 = 22.0;
const GAP: f32 = 6.0;
const TILE: egui::Vec2 = vec2(66.0, 62.0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Formatting,
    Charts,
    Totals,
    Tables,
    Sparklines,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Formatting, Tab::Charts, Tab::Totals, Tab::Tables, Tab::Sparklines];
    fn label(self) -> &'static str {
        match self {
            Tab::Formatting => "Formatting",
            Tab::Charts => "Charts",
            Tab::Totals => "Totals",
            Tab::Tables => "Tables",
            Tab::Sparklines => "Sparklines",
        }
    }
}

#[derive(Default)]
pub struct State {
    /// The selection the popup was opened for; it closes when the selection changes.
    pub open: Option<RangeRef>,
    pub tab: Tab,
    /// Totals by row (results to the right) instead of by column (results below).
    pub by_rows: bool,
}

/// What a tile does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Run(&'static str, Json),
    Dialog(&'static str, Json),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Glyph {
    Icon(Icon),
    Text(&'static str),
    Bars,
    Scale,
    GreenBars,
    BlueScale,
    GreenScale,
    Dots,
    Steps,
    MiniColumns,
    WinLoss,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    label: &'static str,
    glyph: Glyph,
    pub action: Action,
}

fn tile(label: &'static str, glyph: Glyph, action: Action) -> Tile {
    Tile { label, glyph, action }
}

/// The tiles of a tab for the selection `range` (A1 text) on the sheet called `sheet_name`.
pub fn tiles(tab: Tab, range: &str, sheet_name: &str, by_rows: bool) -> Vec<Tile> {
    use Action::{Dialog, Run};
    let cf = |rule: Json| Run("home.conditionalFormat", json!({"range": range, "rule": rule}));
    let chart = |kind: &str| Run("insert.chart", json!({"range": range, "type": kind}));
    let total = |f: &str| Run("analysis.addTotal", json!({"range": range, "function": f, "direction": if by_rows { "rows" } else { "columns" }}));
    let spark = |kind: &str| Run("analysis.addSparklines", json!({"range": range, "type": kind}));
    match tab {
        Tab::Formatting => vec![
            tile("Data Bars", Glyph::Bars, cf(json!({"type": "dataBar"}))),
            tile("Color Scale", Glyph::Scale, cf(json!({"type": "colorScale"}))),
            tile("Icon Set", Glyph::Dots, cf(json!({"type": "iconSet"}))),
            tile("Greater Than…", Glyph::Text(">"), Dialog("cfQuick", json!({"type": "cellIs", "operator": "greater", "title": "Greater Than…"}))),
            tile("Top 10 Items", Glyph::Text("10"), cf(json!({"type": "top10", "rank": 10}))),
            tile("Clear Rules", Glyph::Icon(Icon::Eraser), Run("home.clearRules", json!({"range": range}))),
            tile("Green Data Bars", Glyph::GreenBars, cf(json!({"type": "dataBar", "color": "#63BE7B"}))),
            tile("Blue-White-Red Scale", Glyph::BlueScale, cf(json!({"type": "colorScale", "colors": ["#5A8AC6", "#FCFCFF", "#F8696B"]}))),
            tile("White-Green Scale", Glyph::GreenScale, cf(json!({"type": "colorScale", "colors": ["#FCFCFF", "#63BE7B"]}))),
            tile("Top 10%", Glyph::Text("10%"), cf(json!({"type": "top10", "rank": 10, "percent": true}))),
            tile("Below Average", Glyph::Text("<avg"), cf(json!({"type": "aboveAverage", "below": true}))),
            tile("Duplicate Values", Glyph::Text("=="), cf(json!({"type": "duplicate"}))),
        ],
        Tab::Charts => vec![
            tile("Clustered Column", Glyph::Icon(Icon::ChartBar), chart("column")),
            tile("Line", Glyph::Icon(Icon::ChartLine), chart("line")),
            tile("Pie", Glyph::Icon(Icon::ChartPie), chart("pie")),
            tile("Bar", Glyph::Icon(Icon::Chart), chart("bar")),
            tile("Scatter", Glyph::Icon(Icon::ChartScatter), chart("scatter")),
            tile("Recommended Charts", Glyph::Icon(Icon::Chart), Run("insert.recommendedCharts", json!({"range": range}))),
        ],
        Tab::Totals => vec![
            tile("Sum", Glyph::Icon(Icon::Sum), total("sum")),
            tile("Average", Glyph::Text("avg"), total("average")),
            tile("Count", Glyph::Text("#"), total("count")),
            tile("% Total", Glyph::Icon(Icon::Percent), total("percentTotal")),
            tile("Running Total", Glyph::Steps, total("runningTotal")),
        ],
        Tab::Tables => vec![
            tile("Table", Glyph::Icon(Icon::Table), Run("insert.table", json!({"range": range}))),
            tile(
                "PivotTable",
                Glyph::Icon(Icon::PivotTable),
                Run("insert.pivotTable", json!({"source": format!("'{}'!{range}", sheet_name.replace('\'', "''"))})),
            ),
        ],
        Tab::Sparklines => vec![
            tile("Line", Glyph::Icon(Icon::Sparkline), spark("line")),
            tile("Column", Glyph::MiniColumns, spark("column")),
            tile("Win/Loss", Glyph::WinLoss, spark("winLoss")),
        ],
    }
}

/// Ctrl+Q and the `ui.quickAnalysis` command: opens the popup for the selection (or closes it).
/// Errors for a single cell, where there is nothing to analyse.
pub fn toggle(app: &mut SheetApp, on: Option<bool>) -> Result<Json, String> {
    let d = app.session.active().ok_or("no workbook open")?;
    if d.selection.ranges.len() > 1 {
        return Err("Quick Analysis works on one block of cells; select a single contiguous range.".into());
    }
    let sel = d.selection.current();
    let want = on.unwrap_or(app.grid.quick_analysis.open.is_none());
    if !want {
        app.grid.quick_analysis.open = None;
    } else if sel.is_single() {
        return Err("Select at least two cells to use Quick Analysis.".into());
    } else {
        app.grid.quick_analysis.open = Some(sel);
    }
    Ok(json!({"open": app.grid.quick_analysis.open.is_some()}))
}

/// Whether the button (and popup) stay out of the way: a single cell or several areas, editing,
/// an open dialog or menu, and the paged views have no corner to hang them on.
pub fn is_hidden(app: &SheetApp, geo: &Geo, sh: &Sheet) -> bool {
    let Some(d) = app.session.active() else { return true };
    d.selection.ranges.len() > 1
        || d.selection.current().is_single()
        || app.editor.is_some()
        || app.dialog.is_some()
        || app.text_box_editor.is_some()
        || app.grid.context_menu.is_some()
        || geo.paper.is_some()
        || sh.view == gridcraft_engine::model::SheetViewMode::PageLayout
}

/// Where the corner button sits for `sel` (on screen), and whether the selection's corner is in
/// view. Below and right of the corner, clear of the fill handle; flipped inside at the grid edge.
pub fn button_rect(geo: &Geo, sh: &Sheet, sel: RangeRef) -> (Rect, bool) {
    let corner = geo.range_rect(sh, sel).right_bottom();
    let panes = geo.range_panes(RangeRef::cell(sel.end));
    let visible = panes.iter().any(|p| p.contains(pos2(corner.x - 1.0, corner.y - 1.0)));
    let area = geo.cells.shrink(2.0);
    let x = if corner.x + GAP + BUTTON > area.right() { corner.x - GAP - BUTTON } else { corner.x + GAP };
    let y = if corner.y + GAP + BUTTON > area.bottom() { corner.y - GAP - BUTTON } else { corner.y + GAP };
    let at = pos2(x.clamp(area.left(), (area.right() - BUTTON).max(area.left())), y.clamp(area.top(), (area.bottom() - BUTTON).max(area.top())));
    (geo.mirror_rect(Rect::from_min_size(at, vec2(BUTTON, BUTTON))), visible)
}

/// Paints the corner button and, when open, the popup. Called once per grid frame.
pub fn show(app: &mut SheetApp, ui: &mut Ui, geo: &Geo, sh: &Sheet) {
    let Some(d) = app.session.active() else { return };
    let sel = d.selection.current();
    let hidden = is_hidden(app, geo, sh);
    // The popup belongs to one selection; a new one (or editing) closes it.
    if hidden || app.grid.quick_analysis.open.is_some_and(|r| r != sel) {
        app.grid.quick_analysis.open = None;
    }
    if hidden {
        return;
    }
    let sheet_name = sh.name.clone();
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let (button, corner_visible) = button_rect(geo, sh, sel);
    let open = app.grid.quick_analysis.open.is_some();
    // While the mouse is down (selecting, filling) the button stays out of the way.
    let busy = app.grid.drag != Drag::None || ctx.input(|i| i.pointer.any_down());
    let lang = crate::i18n::current(&ctx);
    // Right after a fill-handle drag the Auto Fill Options button sits at the same corner; it wins
    // until that fill is no longer current (Ctrl+Q still opens the popup).
    let fill_tag = app.grid.fill_tag.is_some();
    if corner_visible && (open || (!busy && !fill_tag)) {
        let mut clicked = false;
        egui::Area::new(Id::new("quick_analysis_button")).fixed_pos(button.min).order(Order::Foreground).show(&ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(button.size(), Sense::click());
            let fill = if resp.is_pointer_button_down_on() || open {
                t.pressed
            } else if resp.hovered() {
                t.hover
            } else {
                t.menu_bg
            };
            ui.painter().rect_filled(r, 4.0, fill);
            ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), Icon::QuickAnalysis, t.accent);
            clicked = resp.on_hover_text(lang.tr("Quick Analysis (Ctrl+Q)")).clicked();
        });
        if clicked {
            let _ = toggle(app, None);
        }
    }
    let Some(opened_for) = app.grid.quick_analysis.open else { return };
    let pos = pos2(button.left(), button.bottom() + 4.0);
    let mut picked: Option<Action> = None;
    let resp = egui::Area::new(Id::new("quick_analysis_popup")).fixed_pos(pos).order(Order::Foreground).constrain(true).show(&ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            let state = &mut app.grid.quick_analysis;
            ui.horizontal(|ui| {
                for tab in Tab::ALL {
                    if ui.selectable_label(state.tab == tab, lang.tr(tab.label())).clicked() {
                        state.tab = tab;
                    }
                }
            });
            ui.separator();
            if state.tab == Tab::Totals {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut state.by_rows, false, lang.tr("By Column"));
                    ui.selectable_value(&mut state.by_rows, true, lang.tr("By Row"));
                });
            }
            let items = tiles(state.tab, &opened_for.a1(), &sheet_name, state.by_rows);
            for row in items.chunks(6) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for item in row {
                        if paint_tile(ui, &t, item, lang.tr(item.label)) {
                            picked = Some(item.action.clone());
                        }
                    }
                });
            }
        });
    });
    let outside_click = ctx.input(|i| i.pointer.any_pressed())
        && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| !resp.response.rect.contains(p) && !button.contains(p));
    if let Some(action) = picked {
        app.grid.quick_analysis.open = None;
        match action {
            Action::Run(id, params) => app.run_or_alert(id, params),
            Action::Dialog(name, params) => app.open_dialog(name, params),
        }
    } else if outside_click || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.grid.quick_analysis.open = None;
    }
}

/// One option of the popup: a small picture over a wrapped label. True when clicked.
fn paint_tile(ui: &mut Ui, t: &Tokens, item: &Tile, label: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(TILE, Sense::click());
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
    }
    let art = Rect::from_center_size(pos2(rect.center().x, rect.top() + 18.0), vec2(26.0, 26.0));
    paint_glyph(ui, t, item.glyph, art);
    let galley = ui.painter().layout(label.to_string(), theme::ui_font(11.0), t.text, TILE.x - 4.0);
    let at = pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 34.0);
    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(at, galley, t.text);
    resp.clicked()
}

fn paint_glyph(ui: &Ui, t: &Tokens, glyph: Glyph, r: Rect) {
    let p = ui.painter();
    let (green, blue, red, yellow) = (icons::GREEN, icons::BLUE, icons::RED, icons::YELLOW);
    let cell = |i: f32, j: f32| Rect::from_min_size(r.min + vec2(i * r.width() / 3.0, j * r.height() / 3.0), vec2(r.width() / 3.0, r.height() / 3.0));
    match glyph {
        Glyph::Icon(icon) => icons::paint(p, r, icon, t.text),
        Glyph::Text(s) => {
            p.text(r.center(), Align2::CENTER_CENTER, s, theme::ui_bold(15.0), t.accent);
        }
        Glyph::Bars => {
            for (j, w) in [0.9, 0.55, 0.3].into_iter().enumerate() {
                let row = cell(0.0, j as f32).shrink2(vec2(0.0, 1.0));
                p.rect_filled(Rect::from_min_size(row.min, vec2(r.width() * w, row.height())), 1.0, blue);
            }
        }
        Glyph::Scale => {
            for (i, c) in [red, yellow, green].into_iter().enumerate() {
                p.rect_filled(cell(i as f32, 1.0).shrink(0.5), 1.0, c);
            }
        }
        Glyph::GreenBars => {
            for (j, w) in [0.9, 0.55, 0.3].into_iter().enumerate() {
                let row = cell(0.0, j as f32).shrink2(vec2(0.0, 1.0));
                p.rect_filled(Rect::from_min_size(row.min, vec2(r.width() * w, row.height())), 1.0, green);
            }
        }
        Glyph::BlueScale | Glyph::GreenScale => {
            let cols = if glyph == Glyph::BlueScale { [blue, Color32::WHITE, red] } else { [Color32::WHITE, green.gamma_multiply(0.5), green] };
            for (i, c) in cols.into_iter().enumerate() {
                p.rect_filled(cell(i as f32, 1.0).shrink(0.5), 1.0, c);
            }
            p.rect_stroke(
                Rect::from_min_size(cell(0.0, 1.0).min, vec2(r.width(), r.height() / 3.0)),
                1.0,
                Stroke::new(1.0, t.separator),
                StrokeKind::Inside,
            );
        }
        Glyph::Dots => {
            for (j, c) in [green, yellow, red].into_iter().enumerate() {
                p.circle_filled(cell(1.0, j as f32).center(), 3.2, c);
            }
        }
        Glyph::Steps => {
            for i in 0..3 {
                let h = r.height() * (0.35 + 0.28 * i as f32);
                p.rect_filled(
                    Rect::from_min_max(
                        pos2(r.left() + i as f32 * r.width() / 3.0 + 1.0, r.bottom() - h),
                        pos2(r.left() + (i + 1) as f32 * r.width() / 3.0 - 1.0, r.bottom()),
                    ),
                    1.0,
                    green,
                );
            }
        }
        Glyph::MiniColumns => {
            for (i, h) in [0.5, 0.9, 0.65].into_iter().enumerate() {
                let x = r.left() + i as f32 * r.width() / 3.0 + 2.0;
                p.rect_filled(Rect::from_min_max(pos2(x, r.bottom() - r.height() * h), pos2(x + r.width() / 3.0 - 4.0, r.bottom())), 1.0, blue);
            }
        }
        Glyph::WinLoss => {
            let mid = r.center().y;
            for (i, up) in [true, false, true].into_iter().enumerate() {
                let x = r.left() + i as f32 * r.width() / 3.0 + 2.0;
                let (a, b) = if up { (mid - 7.0, mid - 1.0) } else { (mid + 1.0, mid + 7.0) };
                p.rect_filled(Rect::from_min_max(pos2(x, a), pos2(x + r.width() / 3.0 - 4.0, b)), 1.0, if up { green } else { red });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;

    /// A header row, a label column and two numeric columns on Sheet1.
    fn session() -> (Session, String) {
        let mut s = Session::new();
        s.new_workbook();
        for (a, v) in [("A1", "Item"), ("B1", "Q1"), ("C1", "Q2"), ("A2", "x"), ("A3", "y"), ("A4", "z")] {
            s.execute("cell.set", json!({"cell": a, "input": v})).unwrap();
        }
        for (r, (q1, q2)) in [(1, 10), (2, 20), (3, 30)].into_iter().enumerate() {
            s.execute("cell.set", json!({"cell": format!("B{}", r + 2), "input": q1.to_string()})).unwrap();
            s.execute("cell.set", json!({"cell": format!("C{}", r + 2), "input": q2.to_string()})).unwrap();
        }
        let name = s.doc().unwrap().wb.active().unwrap().name.clone();
        (s, name)
    }

    #[test]
    fn every_engine_tile_runs_on_a_selection() {
        for tab in Tab::ALL {
            for by_rows in [false, true] {
                let n = tiles(tab, "A1:C4", "Sheet1", by_rows).len();
                for i in 0..n {
                    let (mut s, name) = session();
                    let item = tiles(tab, "A1:C4", &name, by_rows).remove(i);
                    if let Action::Run(id, params) = &item.action {
                        s.execute(id, params.clone()).unwrap_or_else(|e| panic!("{} / {}: {e}", tab.label(), item.label));
                    }
                }
            }
        }
    }

    #[test]
    fn dialog_tile_and_single_cell_are_not_engine_runs() {
        let formatting = tiles(Tab::Formatting, "A1:C4", "Sheet1", false);
        assert!(formatting.iter().any(|t| matches!(&t.action, Action::Dialog("cfQuick", _))));
        // A programmatic open on a single cell is an error, not a popup.
        let (s, _) = session();
        let mut app = SheetApp::new(s, Default::default());
        assert!(toggle(&mut app, Some(true)).is_err());
        app.run("selection.set", json!({"range": "A1:C4"})).unwrap();
        assert_eq!(toggle(&mut app, Some(true)).unwrap(), json!({"open": true}));
        assert_eq!(app.run("ui.quickAnalysis", json!({"open": false})).unwrap(), json!({"open": false}));
    }
}
