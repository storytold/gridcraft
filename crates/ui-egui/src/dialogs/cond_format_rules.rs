use super::formatting_rule::{CF_OPERATORS, CF_PERIODS, cf_combo, cf_preview, hex_color};
use super::{Color32, Dialog, Json, Key, Map, Sense, SheetApp, json, vec2};

/// The Rule column of Manage Rules.
fn cf_describe(p: &Json, lang: crate::i18n::Language) -> String {
    let s = |k: &str| match p.get(k) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Null) | None => String::new(),
        Some(v) => v.to_string(),
    };
    let label = |options: &[(&str, &str)], v: &str| {
        options.iter().find(|(k, _)| *k == v).map(|(_, l)| lang.tr(l).to_string()).unwrap_or_else(|| v.to_string())
    };
    let flag = |k: &str| p.get(k).and_then(Json::as_bool).unwrap_or(false);
    match s("type").as_str() {
        "cellIs" => {
            let op = s("operator");
            let symbol = match op.as_str() {
                "equal" => "=",
                "notEqual" => "<>",
                "greater" => ">",
                "less" => "<",
                "greaterOrEqual" => ">=",
                "lessOrEqual" => "<=",
                _ => "",
            };
            if symbol.is_empty() {
                format!("{} {} {} {} {}", lang.tr("Cell Value"), label(&CF_OPERATORS, &op), s("value"), lang.tr("and"), s("value2"))
            } else {
                format!("{} {symbol} {}", lang.tr("Cell Value"), s("value"))
            }
        }
        "expression" => format!("{} {}", lang.tr("Formula:"), s("formula")),
        "containsText" => format!("{} \"{}\"", lang.tr("Cell value contains"), s("text")),
        "notContainsText" => format!("{} \"{}\"", lang.tr("Cell value does not contain"), s("text")),
        "beginsWith" => format!("{} \"{}\"", lang.tr("Cell value begins with"), s("text")),
        "endsWith" => format!("{} \"{}\"", lang.tr("Cell value ends with"), s("text")),
        "blanks" => lang.tr("Blanks").into(),
        "noBlanks" => lang.tr("No Blanks").into(),
        "errors" => lang.tr("Errors").into(),
        "noErrors" => lang.tr("No Errors").into(),
        "duplicate" => lang.tr("Duplicate Values").into(),
        "unique" => lang.tr("Unique Values").into(),
        "timePeriod" => label(&CF_PERIODS, &s("period")),
        "top10" => {
            format!("{} {}{}", lang.tr(if flag("bottom") { "CF|Bottom" } else { "CF|Top" }), s("rank"), if flag("percent") { "%" } else { "" })
        }
        "aboveAverage" => {
            let sd = p.get("stdDev").and_then(Json::as_u64).unwrap_or(0);
            if sd > 0 {
                format!("{sd} {}", lang.tr(if flag("below") { "std dev below average" } else { "std dev above average" }))
            } else {
                lang.tr(match (flag("below"), flag("equal")) {
                    (false, false) => "Above Average",
                    (true, false) => "Below Average",
                    (false, true) => "Equal to or Above Average",
                    (true, true) => "Equal to or Below Average",
                })
                .into()
            }
        }
        "colorScale" => lang.tr("Graded Color Scale").into(),
        "dataBar" => lang.tr("Data Bar").into(),
        "iconSet" => lang.tr("Icon Set").into(),
        other => other.to_string(),
    }
}

/// The Format column of Manage Rules.
fn cf_format_cell(ui: &mut egui::Ui, p: &Json) {
    let size = vec2(130.0, 20.0);
    match p.get("type").and_then(Json::as_str).unwrap_or("") {
        "colorScale" => {
            let cols: Vec<Color32> = p
                .get("colors")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(|c| c.as_str().and_then(hex_color)).collect())
                .unwrap_or_default();
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            let w = r.width() / cols.len().max(1) as f32;
            for (i, c) in cols.iter().enumerate() {
                ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(w * i as f32, 0.0), vec2(w, r.height())), 0.0, *c);
            }
        }
        "dataBar" => {
            let (r, _) = ui.allocate_exact_size(size, Sense::hover());
            let c = p.get("color").and_then(Json::as_str).and_then(hex_color).unwrap_or(Color32::from_rgb(0x63, 0x8E, 0xC6));
            ui.painter().rect_filled(r, 0.0, Color32::WHITE);
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min + vec2(2.0, 3.0), vec2(r.width() * 0.6, r.height() - 6.0)), 0.0, c);
        }
        "iconSet" => {
            let set = p.get("set").and_then(Json::as_str).unwrap_or("3TrafficLights1");
            ui.horizontal(|ui| {
                for i in 0..gridcraft_engine::cf::icon_count(set) {
                    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    crate::grid::paint_cf_icon(ui.painter(), r.shrink(2.0), set, i);
                }
            });
        }
        _ => cf_preview(ui, p.as_object().unwrap_or(&Map::new()), size),
    }
}

/// Conditional Formatting Rules Manager: which rules to show, New/Edit/
/// Delete/Duplicate and priority arrows, the rule table, and the selected rule's Applies to.
/// Changes apply at once; Cancel puts back the rules the dialog opened with.
pub(super) fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = app.ui.language;
    // Every action targets the sheet the dialog opened on, the one Cancel restores.
    let sheet = d.values.get("sheet").cloned().unwrap_or(Json::Null);
    let show = d.values.get("show").and_then(Json::as_str).unwrap_or("selection").to_string();
    let list =
        app.session.run("home.manageRules", on_sheet(&sheet, json!({"show": show, "selection": d.values.get("selection")}))).unwrap_or(Json::Null);
    let rules = list["rules"].as_array().cloned().unwrap_or_default();
    let total = list["total"].as_u64().unwrap_or(0);
    // The selected rule is followed by its place on the sheet: a move swaps it with a neighbour
    // the Current Selection view may hide, so its row in the list can stay where it was.
    if let Some(at) = d.values.get("selectedRule").and_then(Json::as_u64)
        && let Some(i) = rules.iter().position(|r| r["index"].as_u64() == Some(at))
    {
        d.list_index = i;
    }
    if d.list_index >= rules.len() {
        d.list_index = rules.len().saturating_sub(1);
    }
    let selected = rules.get(d.list_index).cloned();
    let index = selected.as_ref().and_then(|r| r["index"].as_u64());
    ui.horizontal(|ui| {
        ui.label(lang.tr("Show formatting rules for:"));
        let mut m = Map::new();
        m.insert("show".into(), json!(show));
        if cf_combo(ui, "show", &mut m, "show", &[("selection", "Current Selection"), ("sheet", "This Worksheet")], 160.0) {
            d.values.insert("show".into(), m.get("show").cloned().unwrap_or(Json::Null));
            d.list_index = 0;
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button(lang.tr("New Rule…")).clicked() {
            app.open_dialog("newFormattingRule", json!({"sheet": sheet, "range": d.values.get("selection")}));
        }
        let some = index.is_some();
        if ui.add_enabled(some, egui::Button::new(lang.tr("Edit Rule…"))).clicked()
            && let Some(r) = &selected
        {
            app.open_dialog("newFormattingRule", json!({"index": index, "rule": r["params"], "sheet": sheet}));
        }
        if ui.add_enabled(some, egui::Button::new(lang.tr("Delete Rule"))).clicked() {
            app.run_or_alert("home.manageRules", on_sheet(&sheet, json!({"delete": index})));
        }
        if ui.add_enabled(some, egui::Button::new(lang.tr("Duplicate Rule"))).clicked() {
            app.run_or_alert("home.manageRules", on_sheet(&sheet, json!({"duplicate": index})));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let first = index == Some(0);
            let last = index.is_some_and(|i| i + 1 >= total);
            if ui.add_enabled(some && !last, egui::Button::new(lang.tr("Move Down"))).clicked() {
                app.run_or_alert("home.manageRules", on_sheet(&sheet, json!({"moveDown": index})));
                d.values.insert("selectedRule".into(), json!(index.map(|i| i + 1)));
            }
            if ui.add_enabled(some && !first, egui::Button::new(lang.tr("Move Up"))).clicked() {
                app.run_or_alert("home.manageRules", on_sheet(&sheet, json!({"moveUp": index})));
                d.values.insert("selectedRule".into(), json!(index.map(|i| i.saturating_sub(1))));
            }
        });
    });
    ui.add_space(4.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_height(220.0);
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            egui::Grid::new("cfRules").striped(true).num_columns(4).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                ui.strong(lang.tr("Rule (applied in order shown)"));
                ui.strong(lang.tr("CF|Format"));
                ui.strong(lang.tr("Applies to"));
                ui.strong(lang.tr("Stop If True"));
                ui.end_row();
                for (i, r) in rules.iter().enumerate() {
                    let text: String = cf_describe(&r["params"], lang).chars().take(48).collect();
                    let resp = ui.add(egui::Button::selectable(i == d.list_index, text).min_size(vec2(220.0, 20.0)));
                    if resp.clicked() {
                        d.list_index = i;
                        d.values.insert("selectedRule".into(), r["index"].clone());
                        d.values.remove("appliesTo");
                    }
                    if resp.double_clicked() {
                        app.open_dialog("newFormattingRule", json!({"index": r["index"], "rule": r["params"], "sheet": sheet}));
                    }
                    cf_format_cell(ui, &r["params"]);
                    ui.label(r["appliesTo"].as_str().unwrap_or(""));
                    // Colour scales, data bars and icon sets don't stop other rules.
                    let styled = !matches!(r["params"]["type"].as_str(), Some("colorScale" | "dataBar" | "iconSet"));
                    let mut stop = r["stopIfTrue"].as_bool().unwrap_or(false);
                    if ui.add_enabled(styled, egui::Checkbox::without_text(&mut stop)).changed() {
                        app.run_or_alert("home.manageRules", on_sheet(&sheet, json!({"update": r["index"], "stopIfTrue": stop})));
                    }
                    ui.end_row();
                }
            });
            if rules.is_empty() {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(lang.tr(if show == "selection" {
                        "No rules apply to the selected cells. Show This Worksheet to see every rule, or click New Rule…"
                    } else {
                        "There are no conditional formatting rules on this sheet."
                    }))
                    .italics()
                    .weak(),
                );
            }
        });
    });
    ui.add_space(6.0);
    ui.label(lang.tr("Applies to:"));
    ui.horizontal(|ui| {
        let current = selected.as_ref().and_then(|r| r["appliesTo"].as_str()).unwrap_or("").to_string();
        let mut text = d.values.get("appliesTo").and_then(Json::as_str).map(str::to_string).unwrap_or_else(|| current.clone());
        let edited = text != current;
        let field = ui.add_enabled(index.is_some(), egui::TextEdit::singleline(&mut text).desired_width(ui.available_width() - 140.0));
        if field.changed() {
            d.values.insert("appliesTo".into(), json!(text));
        }
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if ui.add_enabled(edited, egui::Button::new(lang.tr("Apply"))).clicked() || (enter && edited) {
            // A refused range stays in the box to be corrected.
            if app.run("home.manageRules", on_sheet(&sheet, json!({"update": index, "appliesTo": text}))).is_ok() {
                d.values.remove("appliesTo");
            } else {
                app.message = Some((d.title.clone(), lang.tr("Applies to must be a range such as =$A$1:$B$10.").into()));
            }
        }
        if ui.add_enabled(edited, egui::Button::new(lang.tr("Cancel"))).clicked() {
            d.values.remove("appliesTo");
        }
    });
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let ok = ui.add(
            egui::Button::new(egui::RichText::new(format!("   {}   ", lang.tr("OK"))).color(Color32::WHITE))
                .fill(crate::theme::Tokens::get(ui.ctx()).accent),
        );
        if ok.clicked() {
            // A pending Applies to edit is kept on OK; a refused one keeps the dialog open.
            let pending = d.values.get("appliesTo").and_then(Json::as_str).map(str::to_string);
            let applied = pending.is_none_or(|t| app.run("home.manageRules", on_sheet(&sheet, json!({"update": index, "appliesTo": t}))).is_ok());
            if applied {
                d.values.remove("appliesTo");
                d.values.insert("ok".into(), json!(true));
                *confirm = true;
            } else {
                app.message = Some((d.title.clone(), lang.tr("Applies to must be a range such as =$A$1:$B$10.").into()));
            }
        }
        if ui.button(format!(" {} ", lang.tr("Cancel"))).clicked() {
            *open = false;
        }
    });
}

/// `params` aimed at `sheet` (Manage Rules works on the sheet it opened on).
fn on_sheet(sheet: &Json, mut params: Json) -> Json {
    params["sheet"] = sheet.clone();
    params
}

/// Manage Rules closed without OK: put back the sheet's rules as they were when it opened.
pub(super) fn manage_rules_revert(app: &mut SheetApp, d: &Dialog) {
    let Some(saved) = d.values.get("snapshot") else { return };
    let now = app.session.run("home.manageRules", json!({"sheet": d.values.get("sheet")})).unwrap_or(Json::Null);
    let now = now["rules"]
        .as_array()
        .map(|a| a.iter().map(|r| json!({"rule": r["rule"], "appliesTo": r["appliesTo"], "stopIfTrue": r["stopIfTrue"]})).collect::<Vec<_>>());
    if now.as_ref() != saved.as_array() {
        app.run_or_alert("home.manageRules", json!({"sheet": d.values.get("sheet"), "set": saved}));
    }
}
