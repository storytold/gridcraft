#[cfg(test)]
use super::show::build_params;
use super::show::{form_fields, ok_cancel};
use super::*;

/// Initial values of the Data Validation form: the rule stored on the active cell, so OK without
/// edits keeps it; a plain list rule when the cell has none.
pub(super) fn validation_defaults(app: &SheetApp) -> Json {
    let mut v = json!({"type": "list", "operator": "between", "ignoreBlank": true, "dropdown": true, "errorStyle": "stop"});
    let Some(doc) = app.session.active() else { return v };
    let Some(dv) = doc.wb.active().and_then(|s| s.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(doc.selection.active)))) else {
        return v;
    };
    // The enums serialize as their variant names; the command takes them in camelCase.
    let camel = |x: Json| {
        let name = x.as_str().unwrap_or_default();
        let mut chars = name.chars();
        chars.next().map(|c| c.to_lowercase().chain(chars).collect::<String>()).unwrap_or_default()
    };
    v["type"] = json!(camel(json!(dv.kind)));
    v["operator"] = json!(camel(json!(dv.op)));
    v["errorStyle"] = json!(camel(json!(dv.error_style)));
    v["formula1"] = json!(dv.f1);
    if let Some(f2) = &dv.f2 {
        v["formula2"] = json!(f2);
    }
    v["ignoreBlank"] = json!(dv.allow_blank);
    v["dropdown"] = json!(dv.in_cell_dropdown);
    v["inputTitle"] = json!(dv.input_title);
    v["inputMessage"] = json!(dv.input_message);
    v["errorTitle"] = json!(dv.error_title);
    v["errorMessage"] = json!(dv.error_message);
    v
}

/// The range Remove Duplicates starts with. A block selection is used as given; with only the
/// cursor placed, the table's data rows or the current region, as the command picks when it gets
/// no range (a single-cell range would remove nothing).
pub(super) fn remove_duplicates_range(app: &SheetApp, sel: &str) -> String {
    let Some(d) = app.session.active() else { return sel.to_string() };
    let Some(sh) = d.wb.active() else { return sel.to_string() };
    if !d.selection.current().is_single() {
        return sel.to_string();
    }
    let range = match sh.table_at(d.selection.active) {
        Some(t) => t.data_range().map(|dr| gridcraft_engine::core::RangeRef::new(t.range.start, dr.end)).unwrap_or(t.range),
        None => gridcraft_engine::selection::current_region(sh, d.selection.active),
    };
    range.a1()
}

pub(super) fn abs(r: &gridcraft_engine::core::RangeRef) -> String {
    let a = |c: gridcraft_engine::core::CellRef| format!("${}${}", gridcraft_engine::core::col_to_letters(c.col), c.row + 1);
    if r.is_single() { a(r.start) } else { format!("{}:{}", a(r.start), a(r.end)) }
}

/// The criterion controls in Excel's Settings tab depend on both Allow and Data.
/// `formula1` is the sole source for a list or custom rule; `formula2` is a
/// second bound only for Between and Not between.
pub(super) fn data_validation_fields(kind: &str, operator: &str) -> Vec<Field> {
    use Field::*;
    let mut fields = vec![Choice {
        key: "type",
        label: "Allow",
        options: vec![
            ("any", "Any value"),
            ("whole", "Whole number"),
            ("decimal", "Decimal"),
            ("list", "List"),
            ("date", "Date"),
            ("time", "Time"),
            ("textLength", "Text length"),
            ("custom", "Custom"),
        ],
    }];
    let two_bounds = matches!(operator, "between" | "notBetween");
    match kind {
        "list" => {
            fields.push(Text { key: "formula1", label: "Source:" });
            fields.push(Check { key: "ignoreBlank", label: "Ignore blank" });
            fields.push(Check { key: "dropdown", label: "In-cell dropdown" });
        }
        "custom" => {
            fields.push(Text { key: "formula1", label: "Formula:" });
            fields.push(Check { key: "ignoreBlank", label: "Ignore blank" });
        }
        "whole" | "decimal" | "date" | "time" | "textLength" => {
            fields.push(Choice {
                key: "operator",
                label: "Data",
                options: vec![
                    ("between", "between"),
                    ("notBetween", "not between"),
                    ("equal", "equal to"),
                    ("notEqual", "not equal to"),
                    ("greater", "greater than"),
                    ("less", "less than"),
                    ("greaterOrEqual", "greater than or equal to"),
                    ("lessOrEqual", "less than or equal to"),
                ],
            });
            let lower = matches!(operator, "greater" | "greaterOrEqual");
            let (first, second) = match kind {
                "date" => (
                    if two_bounds || lower {
                        "Start date:"
                    } else if matches!(operator, "less" | "lessOrEqual") {
                        "End date:"
                    } else {
                        "Date:"
                    },
                    "End date:",
                ),
                "time" => (
                    if two_bounds || lower {
                        "Start time:"
                    } else if matches!(operator, "less" | "lessOrEqual") {
                        "End time:"
                    } else {
                        "Time:"
                    },
                    "End time:",
                ),
                _ => (
                    if two_bounds || lower {
                        "Minimum:"
                    } else if matches!(operator, "less" | "lessOrEqual") {
                        "Maximum:"
                    } else {
                        "Value:"
                    },
                    "Maximum:",
                ),
            };
            fields.push(Text { key: "formula1", label: first });
            if two_bounds {
                fields.push(Text { key: "formula2", label: second });
            }
            fields.push(Check { key: "ignoreBlank", label: "Ignore blank" });
        }
        _ => {}
    }
    fields.push(Text { key: "inputMessage", label: "Input message:" });
    fields.push(Choice {
        key: "errorStyle",
        label: "Error style",
        options: vec![("stop", "Stop"), ("warning", "Warning"), ("information", "Information")],
    });
    fields.push(Text { key: "errorMessage", label: "Error message:" });
    fields
}

pub(super) fn data_validation_form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let kind = d.values.get("type").and_then(Json::as_str).unwrap_or("any");
    let operator = d.values.get("operator").and_then(Json::as_str).unwrap_or("between");
    d.fields = data_validation_fields(kind, operator);
    form_fields(ui, d);
    ok_cancel(ui, confirm, open);
}

pub(super) fn advanced_filter_form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    use Field::*;
    let copy = d.values.get("action").and_then(Json::as_str) == Some("copy");
    d.fields = vec![
        Choice {
            key: "action",
            label: "Action",
            options: vec![("inPlace", "Show matching rows in place"), ("copy", "Copy matching rows elsewhere")],
        },
        Text { key: "range", label: "List range:" },
        Text { key: "criteria", label: "Criteria range (optional):" },
    ];
    if copy {
        d.fields.push(Text { key: "copyTo", label: "Copy to:" });
    }
    d.fields.push(Check { key: "unique", label: "Unique rows only" });
    form_fields(ui, d);
    // Constrain the button row: the shared right-to-left layout otherwise
    // consumes the window's remaining height instead of fitting the buttons.
    ui.allocate_ui(vec2(ui.available_width(), ui.spacing().interact_size.y + 8.0), |ui| ok_cancel(ui, confirm, open));
}

/// Draws the sort-level editor: header toggle, column/by/order controls, add/remove level, and `data.sort` on confirmation.
pub(super) fn sort(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let lang = app.ui.language;
    let mut header = d.values.get("header").and_then(Json::as_bool).unwrap_or(true);
    ui.checkbox(&mut header, "My list has headers");
    d.values.insert("header".into(), json!(header));
    let mut levels: Vec<Json> = d.values.get("levels").and_then(Json::as_array).cloned().unwrap_or_default();
    // Columns of the region around the active cell, labelled with their header (or first value).
    let columns: Vec<(String, String)> = app
        .session
        .active()
        .and_then(|d| {
            let sh = d.wb.active()?;
            let region = gridcraft_engine::selection::current_region(sh, d.selection.active);
            Some(
                (region.start.col..=region.end.col)
                    .map(|c| {
                        let letter = gridcraft_engine::core::col_to_letters(c);
                        // The region starts at the header row when one is present, so the
                        // label always comes from start.row: header text with headers on,
                        // first data value with them off.
                        let label_row = region.start.row;
                        let text = gridcraft_engine::display::cell_text(&d.wb, sh, gridcraft_engine::core::CellRef::new(label_row, c));
                        let text: String = text.chars().take(24).collect();
                        (letter.clone(), if text.is_empty() { letter } else { format!("{letter}: {text}") })
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    let mut remove = None;
    for (i, lv) in levels.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let label = ui.label(lang.tr(if i == 0 { "Sort by" } else { "Then by" }));
            let mut col = lv["column"].as_str().unwrap_or("A").to_string();
            if columns.is_empty() {
                ui.add(egui::TextEdit::singleline(&mut col).desired_width(50.0)).labelled_by(label.id);
            } else {
                egui::ComboBox::from_id_salt(("sort_col", i))
                    .selected_text(col.clone())
                    .show_ui(ui, |ui| {
                        for (letter, label) in &columns {
                            ui.selectable_value(&mut col, letter.clone(), label.as_str());
                        }
                    })
                    .response
                    .labelled_by(label.id);
            }
            lv["column"] = json!(col.to_ascii_uppercase());
            let mut by = lv["by"].as_str().unwrap_or("values").to_string();
            egui::ComboBox::from_id_salt(("by", i))
                .selected_text(match by.as_str() {
                    "cellColor" => "Cell Color",
                    "fontColor" => "Font Color",
                    _ => "Cell Values",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut by, "values".into(), "Cell Values");
                    ui.selectable_value(&mut by, "cellColor".into(), "Cell Color");
                    ui.selectable_value(&mut by, "fontColor".into(), "Font Color");
                })
                .response
                .accessible_name(lang.tr("Sort On"));
            lv["by"] = json!(by);
            let mut desc = lv["order"].as_str() == Some("desc");
            ui.radio_value(&mut desc, false, "A to Z");
            ui.radio_value(&mut desc, true, "Z to A");
            lv["order"] = json!(if desc { "desc" } else { "asc" });
            if crate::widgets::icon_button(ui, Icon::Close, crate::theme::Tokens::get(ui.ctx()).text_dim, "Remove level", vec2(18.0, 18.0)).clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove
        && levels.len() > 1
    {
        levels.remove(i);
    }
    if ui.button("+ Add Level").clicked() {
        let used: Vec<String> = levels.iter().filter_map(|l| l.get("column").and_then(Json::as_str).map(str::to_string)).collect();
        let next = columns
            .iter()
            .map(|(l, _)| l)
            .find(|l| !used.contains(l))
            .cloned()
            .or_else(|| columns.last().map(|(l, _)| l.clone()))
            .unwrap_or_else(|| "B".into());
        levels.push(json!({"column": next, "order": "asc"}));
    }
    d.values.insert("levels".into(), Json::Array(levels.clone()));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        app.run_or_alert("data.sort", json!({"header": header, "keys": levels}));
    }
    if !open {
        *confirm = true;
    }
}

#[cfg(test)]
mod data_validation_tests {
    use super::*;

    #[test]
    fn list_uses_one_source_and_does_not_submit_a_second_list() {
        let fields = data_validation_fields("list", "between");
        assert!(fields.iter().any(|f| matches!(f, Field::Text { key: "formula1", label: "Source:" })));
        assert!(!fields.iter().any(|f| matches!(f, Field::Text { key: "formula2", .. } | Field::Choice { key: "operator", .. })));

        let dialog = Dialog::form(
            "dataValidation",
            "Data Validation",
            "data.validation",
            fields,
            json!({"type": "list", "operator": "between", "formula1": "ABC,DEF", "formula2": "GHI,JKL", "dropdown": true}),
        );
        let params = build_params(&dialog);
        assert_eq!(params["formula1"], "ABC,DEF");
        assert!(params.get("formula2").is_none());
        assert!(params.get("operator").is_none());
    }

    #[test]
    fn only_between_operators_show_and_submit_two_bounds() {
        let two = data_validation_fields("whole", "between");
        assert!(two.iter().any(|f| matches!(f, Field::Text { key: "formula2", label: "Maximum:" })));
        let one = data_validation_fields("date", "lessOrEqual");
        assert!(one.iter().any(|f| matches!(f, Field::Text { key: "formula1", label: "End date:" })));
        assert!(!one.iter().any(|f| matches!(f, Field::Text { key: "formula2", .. })));

        let dialog = Dialog::form(
            "dataValidation",
            "Data Validation",
            "data.validation",
            one,
            json!({"type": "date", "operator": "lessOrEqual", "formula1": "2026-10-12", "formula2": "2030-01-01"}),
        );
        let params = build_params(&dialog);
        assert_eq!(params["operator"], "lessOrEqual");
        assert_eq!(params["formula1"], "2026-10-12");
        assert!(params.get("formula2").is_none());
    }
}
