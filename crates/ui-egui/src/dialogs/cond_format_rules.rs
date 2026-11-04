//! Conditional formatting dialogs: the rules manager, and the New / Edit Formatting Rule editor
//! with its 2- and 3-colour scale stops.

use super::show::{ok_cancel_enabled, track_pick};
use super::{Dialog, Json, RefPick, SheetApp, json};
use egui::{Color32, Sense, Stroke, StrokeKind, vec2};
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::model::{CfRule, CfValueKind, Color as CellColor, Theme};

pub(super) fn manage_rules(app: &mut SheetApp, ui: &mut egui::Ui) {
    let lang = app.ui.language;
    let list = app.session.run("home.manageRules", json!({})).unwrap_or(Json::Null);
    let rules = list["rules"].as_array().cloned().unwrap_or_default();
    if rules.is_empty() {
        ui.label("There are no conditional formatting rules on this sheet.");
    }
    for (i, r) in rules.iter().enumerate() {
        ui.horizontal(|ui| {
            let kind = r["rule"].as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
            ui.label(format!(
                "{}. {kind}  applies to {}",
                i + 1,
                r["ranges"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default()
            ));
            if kind.contains("ColorScale") && ui.small_button(lang.tr("Edit")).clicked() {
                app.open_dialog("newFormattingRule", json!({"edit": i, "rule": r["rule"].clone()}));
            }
            if ui.small_button("▲").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveUp": i}));
            }
            if ui.small_button("▼").clicked() {
                app.run_or_alert("home.manageRules", json!({"moveDown": i}));
            }
            if ui.small_button("Delete").clicked() {
                app.run_or_alert("home.manageRules", json!({"delete": i}));
            }
        });
    }
}

/// The New Formatting Rule dialog, or the rule editor Manage Rules opens on an existing colour
/// scale (`edit` and `rule` in the params).
pub(super) fn new_rule_dialog(app: &mut SheetApp, params: &Json) -> Dialog {
    let theme = app.session.active().map(|d| d.wb.theme.clone()).unwrap_or_default();
    // Excel titles the Manage Rules path "Edit Formatting Rule": OK replaces the rule rather than
    // adding one, and the window should say so.
    let editing = params.get("edit").is_some();
    let title = if editing { "Edit Formatting Rule" } else { "New Formatting Rule" };
    let mut d = Dialog::custom("newFormattingRule", title, new_rule_defaults(params, &theme));
    d.command = Some(if editing { "home.manageRules" } else { "home.conditionalFormat" });
    // The sheet and selection the rule was opened on: Excel's "Applies to" stays put
    // even if the selection moves or another sheet is shown before OK.
    let (sheet, applies_to) = app
        .session
        .active()
        .map(|doc| {
            (
                doc.wb.active().map(|sh| sh.name.clone()).unwrap_or_default(),
                doc.selection.ranges.iter().map(RangeRef::a1).collect::<Vec<_>>().join(","),
            )
        })
        .unwrap_or_default();
    d.values.insert("sheet".into(), json!(sheet));
    d.values.insert("applies_to".into(), json!(applies_to));
    sync_rule_pick(&mut d);
    d
}

/// The command params OK sends: the rule the editor describes, on the scope it opened with.
pub(super) fn rule_params(d: &Dialog) -> Json {
    let mut out = match d.values.get("ruleType").and_then(Json::as_str).unwrap_or("allCells") {
        "formula" => {
            let formula = d.values.get("formula").and_then(Json::as_str).unwrap_or("").to_string();
            let preset = d.values.get("preset").and_then(Json::as_str).unwrap_or("lightRedFillDarkRedText");
            json!({"rule": {"type": "expression", "formula": formula, "preset": preset}})
        }
        _ => {
            let stops: Vec<Json> = d.values.get("stops").and_then(Json::as_array).map(|a| a.iter().map(cf_stop_params).collect()).unwrap_or_default();
            json!({"rule": {"type": "colorScale", "stops": stops}})
        }
    };
    // The rule covers the sheet and range the dialog opened on, not whatever the
    // selection shows now: a formula rule's pick box holds the same opening scope, and
    // colour scales have no pick box but need it just as much to edit in place.
    let sheet = d.values.get("sheet").and_then(Json::as_str).unwrap_or_default();
    let applies_to = d.values.get("applies_to").and_then(Json::as_str).unwrap_or_default();
    if !sheet.is_empty() && !applies_to.is_empty() {
        out["sheet"] = json!(sheet);
        out["range"] = json!(applies_to);
    }
    if let Some(edit) = d.values.get("edit").and_then(Json::as_u64) {
        out["edit"] = json!(edit);
    }
    out
}

/// Draws the rule-type list and the editor for the chosen type, then OK/Cancel. OK stays disabled
/// while the rule description is incomplete (a formula stop or rule with no formula, or a rule
/// type this PR does not implement yet).
pub(super) fn new_formatting_rule(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    ui.label(lang.tr("Select a Rule Type"));
    let mut rule_type = d.values.get("ruleType").and_then(Json::as_str).unwrap_or("allCells").to_string();
    for (v, label) in RULE_TYPES {
        ui.selectable_value(&mut rule_type, (*v).to_string(), lang.tr(label));
    }
    d.values.insert("ruleType".into(), json!(rule_type));
    // Only a formula rule's box takes grid references, and it keeps the sheet and "Applies to"
    // range the dialog opened on.
    sync_rule_pick(d);
    ui.separator();
    ui.label(lang.tr("Edit the Rule Description"));
    let supported = match rule_type.as_str() {
        "allCells" => {
            colour_scale_editor(ui, d, lang);
            // A formula stop needs a formula: one whose value is missing, not a string, or
            // blank would create a rule that never applies. Other stops confirm as they are.
            d.values.get("stops").and_then(Json::as_array).is_none_or(|stops| {
                stops.iter().all(|s| {
                    s.get("type").and_then(Json::as_str) != Some("formula")
                        || s.get("value").and_then(Json::as_str).is_some_and(|f| !f.trim().is_empty())
                })
            })
        }
        "formula" => {
            ui.label(lang.tr("Format values where this formula is true:"));
            let mut formula = d.values.get("formula").and_then(Json::as_str).unwrap_or("").to_string();
            let r = ui.add(egui::TextEdit::singleline(&mut formula).desired_width(360.0));
            if r.changed() {
                d.values.insert("formula".into(), json!(formula.clone()));
            }
            if let Some(pick) = d.pick.as_mut().filter(|pick| pick.key == "formula") {
                track_pick(pick, &r, r.changed());
            }
            ui.horizontal(|ui| {
                ui.label(lang.tr("with"));
                preset_combo(ui, d, lang);
            });
            // A formula rule with no formula never applies; OK waits for one.
            !formula.trim().is_empty()
        }
        _ => {
            ui.label(lang.tr("This rule type isn't available yet."));
            false
        }
    };
    ok_cancel_enabled(ui, confirm, open, supported);
}

fn cf_stop_json(kind: CfValueKind, color: &CellColor, theme: &Theme) -> Json {
    let (ty, value) = match kind {
        CfValueKind::Min => ("min", Json::Null),
        CfValueKind::Max => ("max", Json::Null),
        CfValueKind::Number(n) => ("num", json!(n)),
        CfValueKind::Percent(n) => ("percent", json!(n)),
        CfValueKind::Percentile(n) => ("percentile", json!(n)),
        CfValueKind::Formula(f) => ("formula", json!(f)),
    };
    let hex = color.hex(theme).unwrap_or_else(|| "#000000".into());
    // `source` keeps a theme colour by reference: OK without touching the picker sends it back
    // unchanged, instead of freezing the theme's current RGB into the rule.
    json!({"type": ty, "value": value, "color": hex.clone(), "source": color, "sourceHex": hex})
}

/// The stop as the engine takes it, dropping the dialog's bookkeeping. A colour the user did not
/// change keeps its original theme reference; a changed one is sent as the picked RGB.
fn cf_stop_params(stop: &Json) -> Json {
    let mut obj = stop.as_object().cloned().unwrap_or_default();
    let source = obj.remove("source").and_then(|v| serde_json::from_value::<CellColor>(v).ok());
    let source_hex = obj.remove("sourceHex").and_then(|v| v.as_str().map(str::to_string));
    if let (Some(source), Some(source_hex)) = (source, source_hex)
        && obj.get("color").and_then(Json::as_str).is_some_and(|c| c.eq_ignore_ascii_case(&source_hex))
    {
        obj.insert(
            "color".into(),
            match source {
                CellColor::Auto => json!("auto"),
                CellColor::Rgb(v) => json!(format!("#{v:06X}")),
                CellColor::Theme(i, tint) => json!({"theme": i, "tint": tint as f64 / 1000.0}),
            },
        );
    }
    Json::Object(obj)
}

fn default_scale_stops(style: &str) -> Vec<Json> {
    if style == "2color" {
        vec![json!({"type": "min", "value": null, "color": "#F8696B"}), json!({"type": "max", "value": null, "color": "#63BE7B"})]
    } else {
        vec![
            json!({"type": "min", "value": null, "color": "#F8696B"}),
            json!({"type": "percentile", "value": 50, "color": "#FFEB84"}),
            json!({"type": "max", "value": null, "color": "#63BE7B"}),
        ]
    }
}

fn new_rule_defaults(params: &Json, theme: &Theme) -> Json {
    // Excel's New Formatting Rule opens on the first type. Manage Rules edits a colour scale,
    // so an edit always stays on that branch.
    let mut rule_type = params.get("ruleType").and_then(Json::as_str).unwrap_or("allCells").to_string();
    if params.get("edit").is_some() || !matches!(rule_type.as_str(), "allCells" | "formula") {
        rule_type = "allCells".into();
    }
    let mut style = params.get("style").and_then(Json::as_str).unwrap_or("3color").to_string();
    if !matches!(style.as_str(), "2color" | "3color") {
        style = "3color".into();
    }
    let existing = params.get("rule").and_then(|rule| serde_json::from_value::<CfRule>(rule.clone()).ok()).and_then(|rule| match rule {
        CfRule::ColorScale { stops } if (2..=3).contains(&stops.len()) => {
            Some(stops.into_iter().map(|(kind, color)| cf_stop_json(kind, &color, theme)).collect::<Vec<_>>())
        }
        _ => None,
    });
    let stops = match existing {
        Some(stops) => {
            style = if stops.len() == 2 { "2color".into() } else { "3color".into() };
            stops
        }
        None => default_scale_stops(&style),
    };
    json!({
        "ruleType": rule_type,
        "style": style,
        "stops": stops,
        "formula": "",
        "preset": "lightRedFillDarkRedText",
        "edit": params.get("edit").cloned(),
    })
}

fn parse_hex_color(s: &str) -> Color32 {
    let h = s.trim_start_matches('#');
    u32::from_str_radix(h.get(..6).unwrap_or("000000"), 16)
        .map(|v| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
        .unwrap_or(Color32::WHITE)
}

fn hex_color(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

fn scale_preview(ui: &mut egui::Ui, stops: &[Json]) {
    let (rect, _) = ui.allocate_exact_size(vec2(280.0, 26.0), Sense::hover());
    let colors: Vec<Color32> = stops.iter().map(|s| parse_hex_color(s.get("color").and_then(Json::as_str).unwrap_or("#FFFFFF"))).collect();
    if colors.is_empty() {
        return;
    }
    let n = 64;
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let pos = t * (colors.len() - 1) as f32;
        let lo = pos.floor() as usize;
        let hi = (lo + 1).min(colors.len() - 1);
        let f = pos - lo as f32;
        let a = colors[lo];
        let b = colors[hi];
        let c = Color32::from_rgb(
            (a.r() as f32 + (b.r() as f32 - a.r() as f32) * f) as u8,
            (a.g() as f32 + (b.g() as f32 - a.g() as f32) * f) as u8,
            (a.b() as f32 + (b.b() as f32 - a.b() as f32) * f) as u8,
        );
        let x = rect.left() + rect.width() * i as f32 / n as f32;
        ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(rect.width() / n as f32 + 0.5, rect.height())), 0.0, c);
    }
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(190)), StrokeKind::Inside);
}

fn stop_combo(ui: &mut egui::Ui, id: (&'static str, usize), current: &mut String, options: &[(&str, &str)], lang: crate::i18n::Language) {
    let shown = options.iter().find(|(v, _)| *v == current).map(|(_, l)| lang.tr(l)).unwrap_or(current.as_str());
    craft_ui::ComboBox::from_id_salt(id).selected_text(shown).width(120.0).show_ui(ui, |ui| {
        for (v, l) in options {
            if ui.selectable_label(current == v, lang.tr(l)).clicked() {
                *current = (*v).to_string();
            }
        }
    });
}

fn colour_scale_editor(ui: &mut egui::Ui, d: &mut Dialog, lang: crate::i18n::Language) {
    ui.label(lang.tr("Format all cells based on their values"));
    ui.horizontal(|ui| {
        ui.label(lang.tr("Format Style:"));
        let mut style = d.values.get("style").and_then(Json::as_str).unwrap_or("3color").to_string();
        craft_ui::ComboBox::from_id_salt("newFormattingRule.style")
            .selected_text(lang.tr(if style == "2color" { "2-Color Scale" } else { "3-Color Scale" }))
            .width(150.0)
            .show_ui(ui, |ui| {
                for (v, l) in [("2color", "2-Color Scale"), ("3color", "3-Color Scale")] {
                    if ui.selectable_label(style == v, lang.tr(l)).clicked() {
                        style = v.to_string();
                    }
                }
            });
        d.values.insert("style".into(), json!(style));
        // The midpoint comes back as the default #FFEB84 rather than a copy of the minimum's
        // colour, so 3 → 2 → 3 restores Excel's scale.
        let mut stops = d.values.get("stops").and_then(Json::as_array).cloned().unwrap_or_default();
        if style == "2color" && stops.len() == 3 {
            stops.remove(1);
        } else if style == "3color" && stops.len() == 2 {
            stops.insert(1, json!({"type": "percentile", "value": 50, "color": "#FFEB84"}));
        }
        d.values.insert("stops".into(), Json::Array(stops));
    });
    let mut stops = d.values.get("stops").and_then(Json::as_array).cloned().unwrap_or_default();
    let count = stops.len();
    for i in 0..count {
        let mut obj = stops.get(i).and_then(Json::as_object).cloned().unwrap_or_default();
        let mut ty = obj.get("type").and_then(Json::as_str).unwrap_or("num").to_string();
        let options: &[(&str, &str)] = if i == 0 {
            &[("min", "Lowest value"), ("num", "Number"), ("percent", "Percent"), ("percentile", "Percentile"), ("formula", "Formula")]
        } else if i + 1 == count {
            &[("max", "Highest value"), ("num", "Number"), ("percent", "Percent"), ("percentile", "Percentile"), ("formula", "Formula")]
        } else {
            &[("num", "Number"), ("percent", "Percent"), ("percentile", "Percentile"), ("formula", "Formula")]
        };
        ui.horizontal(|ui| {
            ui.label(lang.tr(if i == 0 {
                "Minimum"
            } else if i + 1 == count {
                "Maximum"
            } else {
                "Midpoint"
            }));
            stop_combo(ui, ("newFormattingRule.stopType", i), &mut ty, options, lang);
            match ty.as_str() {
                "min" | "max" => {
                    ui.label(lang.tr(if ty == "min" { "Lowest value" } else { "Highest value" }));
                }
                "formula" => {
                    let mut f = obj.get("value").and_then(Json::as_str).unwrap_or("").to_string();
                    ui.add(egui::TextEdit::singleline(&mut f).desired_width(120.0).hint_text(lang.tr("Formula")));
                    // Store what the field shows as a string even when it was not edited: a
                    // stop switched from Number still holds that number, which is no formula.
                    obj.insert("value".into(), json!(f));
                }
                _ => {
                    let mut n = obj.get("value").and_then(Json::as_f64).unwrap_or(0.0);
                    let speed = if ty == "num" { 1.0 } else { 0.1 };
                    if ui.add(egui::DragValue::new(&mut n).speed(speed).range(if ty == "num" { f64::MIN..=f64::MAX } else { 0.0..=100.0 })).changed()
                    {
                        obj.insert("value".into(), json!(n));
                    }
                    // A numeric stop must carry a number: a stop switched from Lowest or
                    // Highest value still holds `null`, which the engine refuses.
                    if obj.get("value").and_then(Json::as_f64).is_none() {
                        obj.insert("value".into(), json!(n));
                    }
                }
            }
            let c = parse_hex_color(obj.get("color").and_then(Json::as_str).unwrap_or("#FFFFFF"));
            let mut rgb = [c.r(), c.g(), c.b()];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                obj.insert("color".into(), json!(hex_color(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))));
            }
        });
        obj.insert("type".into(), json!(ty));
        stops[i] = Json::Object(obj);
    }
    d.values.insert("stops".into(), Json::Array(stops.clone()));
    scale_preview(ui, &stops);
}

fn preset_combo(ui: &mut egui::Ui, d: &mut Dialog, lang: crate::i18n::Language) {
    const PRESETS: &[(&str, &str)] = &[
        ("lightRedFillDarkRedText", "Light Red Fill with Dark Red Text"),
        ("yellowFillDarkYellowText", "Yellow Fill with Dark Yellow Text"),
        ("greenFillDarkGreenText", "Green Fill with Dark Green Text"),
        ("lightRedFill", "Light Red Fill"),
        ("redText", "Red Text"),
        ("redBorder", "Red Border"),
    ];
    let mut preset = d.values.get("preset").and_then(Json::as_str).unwrap_or("lightRedFillDarkRedText").to_string();
    let shown = PRESETS.iter().find(|(v, _)| *v == preset).map(|(_, l)| lang.tr(l)).unwrap_or(preset.as_str());
    craft_ui::ComboBox::from_id_salt("newFormattingRule.preset").selected_text(shown).width(260.0).show_ui(ui, |ui| {
        for (v, l) in PRESETS {
            if ui.selectable_label(preset == *v, lang.tr(l)).clicked() {
                preset = (*v).to_string();
            }
        }
    });
    d.values.insert("preset".into(), json!(preset));
}

/// Excel's six rule types, in its order.
const RULE_TYPES: &[(&str, &str)] = &[
    ("allCells", "Format all cells based on their values"),
    ("cellsThatContain", "Format only cells that contain"),
    ("topBottom", "Format only top or bottom ranked values"),
    ("aboveBelow", "Format only values that are above or below average"),
    ("uniqueDuplicate", "Format only unique or duplicate values"),
    ("formula", "Use a formula to determine which cells to format"),
];

/// Gives a formula rule's box point mode, on the sheet and selection the dialog opened on;
/// other rule types take clicks on the grid as selection.
fn sync_rule_pick(d: &mut Dialog) {
    if d.values.get("ruleType").and_then(Json::as_str) != Some("formula") {
        d.pick = None;
        return;
    }
    if d.pick.is_some() {
        return;
    }
    let sheet = d.values.get("sheet").and_then(Json::as_str).unwrap_or_default().to_string();
    let applies_to = d.values.get("applies_to").and_then(Json::as_str).unwrap_or_default().to_string();
    d.pick = Some(RefPick { key: "formula", sheet, caret: 0, point: None, anchor: CellRef::new(0, 0), refocus: false, applies_to });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_rule_picks_references_and_keeps_the_opening_range() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        session.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
        let mut app = SheetApp::new(session, Default::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let mut d = Dialog::open(&mut app, "newFormattingRule", json!({"ruleType": "formula"})).unwrap();
        assert!(d.picks_references());
        assert_eq!(d.pick.as_ref().unwrap().sheet, sheet);

        // A grid click writes the reference into the formula box.
        d.pick_reference(CellRef::new(0, 0), &sheet, false);
        assert_eq!(d.values["formula"], "=$A$1");

        // Moving the selection before OK must not move the rule.
        app.session.execute("selection.set", json!({"range": "D2:E6"})).unwrap();
        let params = super::super::build_params(&d);
        assert_eq!(params["sheet"], sheet.as_str());
        assert_eq!(params["range"], "B2:C5");
    }

    #[test]
    fn a_rule_switched_to_formula_picks_on_the_opening_scope() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        session.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
        let mut app = SheetApp::new(session, Default::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        // The dialog opens on the default (colour scale) type; choosing the formula type later
        // turns point mode on, still on the sheet and range it opened with.
        let mut d = Dialog::open(&mut app, "newFormattingRule", json!({})).unwrap();
        assert!(!d.picks_references());
        d.values.insert("ruleType".into(), json!("formula"));
        sync_rule_pick(&mut d);
        assert!(d.picks_references());

        app.session.execute("selection.set", json!({"range": "D2:E6"})).unwrap();
        let params = super::super::build_params(&d);
        assert_eq!(params["sheet"], sheet.as_str());
        assert_eq!(params["range"], "B2:C5");
    }

    #[test]
    fn colour_scale_rule_keeps_the_opening_range_without_grid_references() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        session.execute("selection.set", json!({"range": "B2:C5"})).unwrap();
        let mut app = SheetApp::new(session, Default::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let d = Dialog::open(&mut app, "newFormattingRule", json!({})).unwrap();
        assert!(!d.picks_references());
        // No pick box, but the rule still covers the range and sheet the dialog opened on.
        let params = super::super::build_params(&d);
        assert_eq!(params["sheet"], sheet.as_str());
        assert_eq!(params["range"], "B2:C5");
    }
}
