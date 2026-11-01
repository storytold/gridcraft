//! The Quick Analysis button: a small button at the bottom-right corner of a selected range that
//! opens a pop-out of one-click Formatting, Charts, Totals and Tables actions. Every action runs
//! an engine command on the selection, so the pop-out stays a thin layer over the command set.

use crate::SheetApp;
use egui::{Rect, Sense, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;

const TABS: [&str; 4] = ["Formatting", "Charts", "Totals", "Tables"];
const ITEM: egui::Vec2 = vec2(92.0, 24.0);

/// One pop-out action: label, command id, params (or a dialog to open when the id starts `ui:`).
type Item = (&'static str, &'static str, serde_json::Value);

fn items(tab: usize) -> Vec<Item> {
    match tab {
        0 => vec![
            ("Data Bars", "home.conditionalFormat", json!({"rule": {"type": "dataBar", "color": "#638EC6"}})),
            ("Color Scale", "home.conditionalFormat", json!({"rule": {"type": "colorScale", "colors": ["#63BE7B", "#FFEB84", "#F8696B"]}})),
            ("Icon Set", "home.conditionalFormat", json!({"rule": {"type": "iconSet", "set": "3Arrows"}})),
            ("Greater Than…", "ui:cfQuick", json!({"type": "cellIs", "operator": "greater", "title": "Greater Than…"})),
            ("Top 10%", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "percent": true}})),
            ("Clear Format", "home.clearRules", json!({})),
        ],
        1 => vec![
            ("Clustered Column", "insert.chart", json!({"type": "column", "subtype": "clustered"})),
            ("Line", "insert.chart", json!({"type": "line"})),
            ("Pie", "insert.chart", json!({"type": "pie"})),
            ("Bar", "insert.chart", json!({"type": "bar", "subtype": "clustered"})),
            ("Area", "insert.chart", json!({"type": "area"})),
        ],
        2 => vec![
            ("Sum", "formulas.autoSum", json!({"function": "SUM"})),
            ("Average", "formulas.autoSum", json!({"function": "AVERAGE"})),
            ("Count", "formulas.autoSum", json!({"function": "COUNT"})),
            ("Max", "formulas.autoSum", json!({"function": "MAX"})),
            ("Min", "formulas.autoSum", json!({"function": "MIN"})),
        ],
        _ => vec![("Table", "insert.table", json!({"header": true}))],
    }
}

/// Paints the button at `corner` (the selection's bottom-right, in screen space) and shows the
/// pop-out while it is open.
pub fn button(app: &mut SheetApp, ui: &mut Ui, corner: egui::Pos2) -> Option<Rect> {
    let lang = crate::i18n::current(ui.ctx());
    let rect = Rect::from_min_size(corner + vec2(2.0, 2.0), vec2(22.0, 22.0));
    let resp = ui.interact(rect, egui::Id::unique("gridcraft.quick_analysis"), Sense::click()).on_hover_text(lang.tr("Quick Analysis"));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, lang.tr("Quick Analysis")));
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, if resp.hovered() { t.hover } else { t.window });
    p.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    // A small table with a lightning bolt over it.
    let c = rect.center();
    let ink = egui::Stroke::new(1.0, t.text);
    let grid = Rect::from_center_size(c, vec2(12.0, 12.0));
    p.rect_stroke(grid, 0.0, ink, egui::StrokeKind::Inside);
    p.line_segment([pos2(c.x - 2.0, grid.top()), pos2(c.x - 2.0, grid.bottom())], ink);
    p.line_segment([pos2(c.x + 2.0, grid.top()), pos2(c.x + 2.0, grid.bottom())], ink);
    p.line_segment([pos2(grid.left(), c.y - 2.0), pos2(grid.right(), c.y - 2.0)], ink);
    p.line_segment([pos2(grid.left(), c.y + 2.0), pos2(grid.right(), c.y + 2.0)], ink);
    p.add(egui::Shape::convex_polygon(
        vec![
            pos2(c.x + 3.0, c.y - 7.0),
            pos2(c.x - 3.0, c.y + 1.0),
            pos2(c.x, c.y + 1.0),
            pos2(c.x - 2.0, c.y + 7.0),
            pos2(c.x + 4.0, c.y - 1.5),
            pos2(c.x + 1.0, c.y - 1.5),
        ],
        t.accent,
        egui::Stroke::NONE,
    ));
    let tab_id = egui::Id::unique("gridcraft.quick_analysis.tab");
    let mut chosen: Option<Item> = None;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let mut tab = ui.ctx().data(|d| d.get_temp::<usize>(tab_id)).unwrap_or(0).min(TABS.len() - 1);
        ui.horizontal(|ui| {
            for (i, name) in TABS.iter().enumerate() {
                if ui.selectable_label(tab == i, lang.tr(name)).clicked() {
                    tab = i;
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
        ui.separator();
        let list = items(tab);
        ui.set_min_width(ITEM.x * 3.0 + 16.0);
        for row in list.chunks(3) {
            ui.horizontal(|ui| {
                for item in row {
                    if ui.add(egui::Button::new(lang.tr(item.0)).min_size(ITEM)).clicked() {
                        chosen = Some(item.clone());
                        ui.close();
                    }
                }
            });
        }
    });
    if let Some((_, id, params)) = chosen {
        match id.strip_prefix("ui:") {
            Some(dialog) => app.open_dialog(dialog, params),
            None => app.run_or_alert(id, params),
        }
    }
    Some(rect)
}
