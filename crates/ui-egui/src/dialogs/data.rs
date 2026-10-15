#[cfg(test)]
use super::show::build_params;
use super::show::{form_fields, ok_cancel};
use super::{Dialog, Field, Json, Sense, SheetApp, SortChoicesCache, json, vec2};

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

pub(super) fn sort(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let mut header = d.values.get("header").and_then(Json::as_bool).unwrap_or(true);
    let was_header = header;
    let range = d.values.get("range").and_then(Json::as_str).and_then(gridcraft_engine::core::RangeRef::parse);
    let (doc_uid, sheet_index, revision) =
        app.session.active().map(|doc| (Some(doc.uid), Some(doc.wb.active_sheet()), Some(doc.revision))).unwrap_or_default();
    let range_key = range.map(|r| r.a1());
    let cache_is_valid = d.sort_choices_cache.as_ref().is_some_and(|cache| {
        cache.doc_uid == doc_uid
            && cache.sheet_index == sheet_index
            && cache.revision == revision
            && cache.range == range_key
            && cache.header == header
    });
    let choices = if cache_is_valid {
        d.sort_choices_cache.as_ref().map(|cache| cache.choices.clone()).unwrap_or_default()
    } else {
        let choices: Vec<(String, String, bool)> = range
            .map(|r| {
                let doc = app.session.active();
                let sh = doc.and_then(|d| d.wb.active());
                let body_start = r.start.row.saturating_add(u32::from(header));
                let cols = r.end.col.saturating_sub(r.start.col).saturating_add(1).max(1) as usize;
                // Total sampled cells are bounded across all columns, not per column.
                let rows_per_col = (200_000 / cols).max(1);
                (r.start.col..=r.end.col)
                    .map(|c| {
                        let letters = gridcraft_engine::core::col_to_letters(c);
                        let text = match (header, sh) {
                            (true, Some(sh)) => sh.value(gridcraft_engine::core::CellRef::new(r.start.row, c)).display().trim().to_string(),
                            _ => String::new(),
                        };
                        let label = if text.is_empty() { format!("Column {letters}") } else { text };
                        let numeric = sh.is_some_and(|sh| {
                            sort_values_are_numeric(
                                (body_start..=r.end.row).take(rows_per_col).map(|row| sh.value(gridcraft_engine::core::CellRef::new(row, c))),
                            )
                        });
                        (letters, label, numeric)
                    })
                    .collect()
            })
            .unwrap_or_default();
        d.sort_choices_cache = Some(SortChoicesCache { doc_uid, sheet_index, revision, range: range_key, header, choices: choices.clone() });
        choices
    };
    let mut levels: Vec<Json> = d.values.get("levels").and_then(Json::as_array).cloned().unwrap_or_default();
    let mut selected = d.list_index.min(levels.len().saturating_sub(1));
    ui.horizontal_wrapped(|ui| {
        // New levels must use an unused column in the selected sort range.
        let used: Vec<&str> = levels.iter().filter_map(|l| l["column"].as_str()).collect();
        let next_column = choices.iter().map(|(l, _, _)| l).find(|l| !used.contains(&l.as_str())).cloned();
        if sort_text_icon_button(
            ui,
            crate::icons::Icon::Plus,
            crate::theme::Tokens::get(ui.ctx()).accent,
            "Add Level",
            levels.len() < 64 && next_column.is_some(),
        )
        .clicked()
            && let Some(next) = next_column
        {
            levels.push(json!({"column": next, "order": "asc"}));
            selected = levels.len() - 1;
        }
        if sort_text_icon_button(ui, crate::icons::Icon::Close, crate::theme::Tokens::get(ui.ctx()).danger, "Delete Level", !levels.is_empty())
            .clicked()
            && selected < levels.len()
        {
            levels.remove(selected);
            selected = selected.saturating_sub(1);
        }
        if sort_text_icon_button(
            ui,
            crate::icons::Icon::Copy,
            crate::theme::Tokens::get(ui.ctx()).text,
            "Copy Level",
            !levels.is_empty() && levels.len() < 64,
        )
        .clicked()
            && let Some(lv) = levels.get(selected).cloned()
        {
            levels.insert(selected + 1, lv);
            selected += 1;
        }
        if sort_icon_button(ui, crate::icons::Icon::ChevronUp, crate::theme::Tokens::get(ui.ctx()).text, "Move Up", selected > 0).clicked() {
            levels.swap(selected, selected - 1);
            selected -= 1;
        }
        if sort_icon_button(ui, crate::icons::Icon::Chevron, crate::theme::Tokens::get(ui.ctx()).text, "Move Down", selected + 1 < levels.len())
            .clicked()
        {
            levels.swap(selected, selected + 1);
            selected += 1;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut header, "My list has headers");
        });
    });
    d.values.insert("header".into(), json!(header));
    if header != was_header
        && let Some(r) = range
    {
        let shown = if header && r.height() > 1 {
            gridcraft_engine::core::RangeRef::new(gridcraft_engine::core::CellRef::new(r.start.row + 1, r.start.col), r.end)
        } else {
            r
        };
        let _ = app.run("selection.set", json!({"range": shown.a1()}));
    }
    ui.add_space(4.0);
    let table_height = (ui.available_height() - 48.0).max(60.0);
    egui::ScrollArea::vertical().max_height(table_height).min_scrolled_height(table_height).auto_shrink([false, false]).show(ui, |ui| {
        let gap = ui.spacing().item_spacing.x;
        let label_w = 64.0;
        let row_h = 26.0;
        let rest = (ui.available_width() - 16.0 - label_w - 3.0 * gap).max(300.0);
        let w = rest / 3.0;
        let (w_col, w_on, w_ord) = (w, w, w);
        let cell = |ui: &mut egui::Ui, w: f32, h: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.allocate_ui_with_layout(vec2(w, h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_min_size(vec2(w, h));
                add(ui);
            });
        };
        ui.horizontal(|ui| {
            cell(ui, label_w + gap + w_col, 18.0, &mut |ui| {
                ui.strong("Column");
            });
            cell(ui, w_on, 18.0, &mut |ui| {
                ui.strong("Sort On");
            });
            cell(ui, w_ord, 18.0, &mut |ui| {
                ui.strong("Order");
            });
        });
        let accent = crate::theme::Tokens::get(ui.ctx()).accent;
        for (i, lv) in levels.iter_mut().enumerate() {
            let row_rect = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), row_h));
            let row = ui.interact(row_rect, ui.scope_id().with(("sortRow", i)), Sense::click());
            if i == selected {
                ui.painter().rect_filled(row_rect, 3.0, accent.gamma_multiply(0.18));
            } else if row.hovered() {
                ui.painter().rect_filled(row_rect, 3.0, accent.gamma_multiply(0.07));
            }
            if row.clicked() {
                selected = i;
            }
            ui.allocate_ui_with_layout(row_rect.size(), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                cell(ui, label_w, row_h, &mut |ui| {
                    ui.add(egui::Label::new(if i == 0 { "Sort by" } else { "Then by" }).selectable(false));
                });
                let mut col = lv["column"].as_str().unwrap_or("A").to_string();
                let shown = choices.iter().find(|(l, _, _)| *l == col).map(|(_, t, _)| t.clone()).unwrap_or_else(|| format!("Column {col}"));
                let r = craft_ui::ComboBox::from_id_salt(("col", i)).selected_text(shown.clone()).width(w_col).show_ui(ui, |ui| {
                    for (letters, label, _) in &choices {
                        ui.selectable_value(&mut col, letters.clone(), label);
                    }
                });
                r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sort column"));
                if r.response.clicked() {
                    selected = i;
                }
                lv["column"] = json!(col);
                let mut by = lv["by"].as_str().unwrap_or("values").to_string();
                let by_label = match by.as_str() {
                    "cellColor" => "Cell Color",
                    "fontColor" => "Font Color",
                    _ => "Cell Values",
                };
                let r = craft_ui::ComboBox::from_id_salt(("by", i)).selected_text(by_label).width(w_on).show_ui(ui, |ui| {
                    ui.selectable_value(&mut by, "values".into(), "Cell Values");
                    ui.selectable_value(&mut by, "cellColor".into(), "Cell Color");
                    ui.selectable_value(&mut by, "fontColor".into(), "Font Color");
                });
                r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sort on"));
                if r.response.clicked() {
                    selected = i;
                }
                lv["by"] = json!(by);
                let numeric = choices.iter().find(|(l, _, _)| lv["column"].as_str() == Some(l.as_str())).is_some_and(|(_, _, n)| *n);
                let (asc_label, desc_label) = if numeric { ("Smallest to Largest", "Largest to Smallest") } else { ("A to Z", "Z to A") };
                let mut desc = lv["order"].as_str() == Some("desc");
                let order_label = if desc { desc_label } else { asc_label };
                let r = craft_ui::ComboBox::from_id_salt(("order", i)).selected_text(order_label).width(w_ord).show_ui(ui, |ui| {
                    ui.selectable_value(&mut desc, false, asc_label);
                    ui.selectable_value(&mut desc, true, desc_label);
                });
                r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sort order"));
                if r.response.clicked() {
                    selected = i;
                }
                lv["order"] = json!(if desc { "desc" } else { "asc" });
            });
        }
    });
    d.list_index = selected;
    d.values.insert("levels".into(), Json::Array(levels.clone()));
    ok_cancel(ui, confirm, open);
    if *confirm && !levels.is_empty() {
        let mut p = json!({"header": header, "keys": levels});
        if let Some(r) = d.values.get("range").and_then(Json::as_str) {
            p["range"] = json!(r);
        }
        app.run_or_alert("data.sort", p);
    }
}

fn sort_icon_button(ui: &mut egui::Ui, icon: crate::icons::Icon, color: egui::Color32, tip: &str, enabled: bool) -> egui::Response {
    let color = if enabled { color } else { crate::theme::Tokens::get(ui.ctx()).text_disabled };
    let response = ui.add_enabled_ui(enabled, |ui| crate::widgets::icon_button(ui, icon, color, tip, vec2(24.0, 24.0))).inner;
    sort_button_border(ui, response.rect);
    response
}

fn sort_text_icon_button(ui: &mut egui::Ui, icon: crate::icons::Icon, color: egui::Color32, label: &str, enabled: bool) -> egui::Response {
    let response = ui.add_enabled_ui(enabled, |ui| crate::widgets::small_button_with_color(ui, icon, color, label, label, false)).inner;
    sort_button_border(ui, response.rect);
    response
}

fn sort_button_border(ui: &egui::Ui, rect: egui::Rect) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
}

fn sort_values_are_numeric(values: impl IntoIterator<Item = gridcraft_engine::core::Value>) -> bool {
    let mut has_number = false;
    for value in values {
        match value {
            gridcraft_engine::core::Value::Empty => {}
            gridcraft_engine::core::Value::Number(_) => has_number = true,
            _ => return false,
        }
    }
    has_number
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
