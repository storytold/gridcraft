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

/// The list scrolls inside the dialog: a sheet read from a file can carry thousands of rules, and
/// a list laid out in full made the window as tall as all of them (issue #3321).
const LIST_HEIGHT: f32 = 360.0;

pub(super) fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui) {
    let lang = crate::i18n::current(ui.ctx());
    // Read the rules in place: running `home.manageRules` for them each frame copied and
    // serialised every rule and journaled a step 60 times a second.
    let count = app.session.active().and_then(|d| d.wb.active()).map_or(0, |sh| sh.cond_formats.len());
    if count == 0 {
        ui.label(lang.tr("There are no conditional formatting rules on this sheet."));
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
                ui.label(format!("{}. {}  {} {ranges}", i + 1, lang.tr(rule_label(&kind)), lang.tr("applies to")));
                if ui.small_button("▲").clicked() {
                    action = Some(json!({"moveUp": i}));
                }
                if ui.small_button("▼").clicked() {
                    action = Some(json!({"moveDown": i}));
                }
                if ui.small_button(lang.tr("Delete")).clicked() {
                    action = Some(json!({"delete": i}));
                }
            });
        }
    });
    if let Some(params) = action {
        app.run_or_alert("home.manageRules", params);
    }
}
