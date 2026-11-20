use super::{Json, SheetApp, json};
use crate::widgets::tr;

/// The list scrolls inside the dialog: a sheet read from a file can carry thousands of rules, and
/// a list laid out in full made the window as tall as all of them (issue #3321).
const LIST_HEIGHT: f32 = 360.0;

pub(super) fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui) {
    // Read the rules in place: running `home.manageRules` for them each frame copied and
    // serialised every rule and journaled a step 60 times a second.
    let count = app.session.active().and_then(|d| d.wb.active()).map_or(0, |sh| sh.cond_formats.len());
    if count == 0 {
        ui.label(tr(ui, "There are no conditional formatting rules on this sheet."));
        return;
    }
    let mut action: Option<Json> = None;
    let row_height = ui.spacing().interact_size.y;
    egui::ScrollArea::both().max_height(LIST_HEIGHT).auto_shrink([false, true]).show_rows(ui, row_height, count, |ui, rows| {
        let Some(sh) = app.session.active().and_then(|d| d.wb.active()) else { return };
        for i in rows {
            let Some(cf) = sh.cond_formats.get(i) else { break };
            let kind = serde_json::to_value(&cf.rule).ok().and_then(|r| r.as_object().and_then(|o| o.keys().next().cloned())).unwrap_or_default();
            let ranges = cf.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(",");
            ui.horizontal(|ui| {
                ui.label(format!("{}. {kind}  applies to {ranges}", i + 1));
                if ui.small_button("▲").clicked() {
                    action = Some(json!({"moveUp": i}));
                }
                if ui.small_button("▼").clicked() {
                    action = Some(json!({"moveDown": i}));
                }
                if ui.small_button(tr(ui, "Delete")).clicked() {
                    action = Some(json!({"delete": i}));
                }
            });
        }
    });
    if let Some(params) = action {
        app.run_or_alert("home.manageRules", params);
    }
}
