#[cfg(test)]
use super::show::build_params;
use super::show::{form_fields, ok_cancel};
use super::{Dialog, Field, Json, SheetApp, json, vec2};

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

pub(super) fn data_validation_form(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let kind = d.values.get("type").and_then(Json::as_str).unwrap_or("any");
    let operator = d.values.get("operator").and_then(Json::as_str).unwrap_or("between");
    d.fields = data_validation_fields(kind, operator);
    form_fields(app, ui, d);
    ok_cancel(ui, confirm, open);
}

pub(super) fn advanced_filter_form(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
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
    form_fields(app, ui, d);
    // Constrain the button row: the shared right-to-left layout otherwise
    // consumes the window's remaining height instead of fitting the buttons.
    ui.allocate_ui(vec2(ui.available_width(), ui.spacing().interact_size.y + 8.0), |ui| ok_cancel(ui, confirm, open));
}

// Custom Sort lives in its own module; `show` reaches it from here.
pub(super) use super::sort_dialog::sort;

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
