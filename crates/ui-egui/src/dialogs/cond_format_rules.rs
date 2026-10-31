use super::{Json, SheetApp, json};

/// The interface label of a conditional-formatting rule kind as the engine names it.
fn rule_label(kind: &str) -> &str {
    match kind {
        "CellIs" => "Cell Values",
        "Expression" => "Formula",
        "ContainsText" => "Text that Contains",
        "NotContainsText" => "Text that Does Not Contain",
        "BeginsWith" => "Begins With",
        "EndsWith" => "Ends With",
        "Blanks" => "Blanks",
        "NoBlanks" => "No Blanks",
        "Errors" => "Errors",
        "NoErrors" => "No Errors",
        "Duplicate" => "Duplicate Values",
        "Unique" => "Unique Values",
        "Top10" => "Top 10",
        "AboveAverage" => "Above Average",
        "TimePeriod" => "Date Occurring",
        "ColorScale" => "Color Scales",
        "DataBar" => "Data Bars",
        "IconSet" => "Icon Sets",
        other => other,
    }
}

pub(super) fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui) {
    let lang = crate::i18n::current(ui.ctx());
    let list = app.session.run("home.manageRules", json!({})).unwrap_or(Json::Null);
    let rules = list["rules"].as_array().cloned().unwrap_or_default();
    if rules.is_empty() {
        ui.label(lang.tr("There are no conditional formatting rules on this sheet."));
    }
    for (i, r) in rules.iter().enumerate() {
        ui.horizontal(|ui| {
            let kind = r["rule"].as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
            ui.label(format!(
                "{}. {}  {} {}",
                i + 1,
                lang.tr(rule_label(&kind)),
                lang.tr("applies to"),
                r["ranges"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default()
            ));
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
