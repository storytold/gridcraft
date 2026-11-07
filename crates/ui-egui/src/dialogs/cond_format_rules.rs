use super::{Json, SheetApp, json};

pub(super) fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui) {
    let lang = app.ui.language;
    let list = app.session.run("home.manageRules", json!({})).unwrap_or(Json::Null);
    let rules = list["rules"].as_array().cloned().unwrap_or_default();
    if rules.is_empty() {
        ui.label(lang.tr("There are no conditional formatting rules on this sheet."));
    }
    for (i, r) in rules.iter().enumerate() {
        ui.horizontal(|ui| {
            let kind = r["rule"].as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
            let ranges = r["ranges"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default();
            ui.label(format!("{}. {kind}  {} {ranges}", i + 1, lang.tr("applies to")));
            if ui.small_button("▲").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveUp": i}));
            }
            if ui.small_button("▼").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveDown": i}));
            }
            if ui.small_button(lang.tr("Delete")).clicked() {
                app.run_or_alert("home.manageRules", json!({"delete": i}));
            }
        });
    }
}
