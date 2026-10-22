//! Excel's Power Pivot window: the File/Home/Design/Advanced tab strip, the Data View or Diagram
//! View canvas, and the floating dialogs Excel opens from its ribbon. The model lives in the
//! engine session, so the window reads it back through `dataModel.status`.

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::theme;
use serde_json::{Value as Json, json};

use super::Dialog;

/// A compact accent button, the dialog counterpart of the ribbon's primary actions.
fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let t = theme::Tokens::get(ui.ctx());
    ui.add(egui::Button::new(egui::RichText::new(text).color(Color32::WHITE)).fill(t.accent).min_size(vec2(76.0, 24.0)))
}

/// One Function Library entry: the Excel category, the DAX name and a parseable template.
/// Placeholders are filled from the loaded model by [`power_pivot_fill_template`]:
/// `{table}`, `{numcol}` (first numeric column), `{textcol}` (first text column), `{measure}`.
struct LibraryFunction {
    category: &'static str,
    name: &'static str,
    template: &'static str,
}

/// The evaluator's core functions, grouped as Excel's Function Library groups them.
/// Category → function → template. Templates use the placeholder vocabulary above.
const LIBRARY_FUNCTIONS: &[LibraryFunction] = &[
    // Aggregation
    LibraryFunction { category: "Aggregation", name: "SUM", template: "SUM({table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "AVERAGE", template: "AVERAGE({table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "COUNT", template: "COUNT({table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "COUNTA", template: "COUNTA({table}[{textcol}])" },
    LibraryFunction { category: "Aggregation", name: "COUNTBLANK", template: "COUNTBLANK({table}[{textcol}])" },
    LibraryFunction { category: "Aggregation", name: "COUNTROWS", template: "COUNTROWS({table})" },
    LibraryFunction { category: "Aggregation", name: "DISTINCTCOUNT", template: "DISTINCTCOUNT({table}[{textcol}])" },
    LibraryFunction { category: "Aggregation", name: "MAX", template: "MAX({table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "MIN", template: "MIN({table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "PRODUCT", template: "PRODUCT({table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "SUMX", template: "SUMX({table}, {table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "AVERAGEX", template: "AVERAGEX({table}, {table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "COUNTX", template: "COUNTX({table}, {table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "COUNTAX", template: "COUNTAX({table}, {table}[{textcol}])" },
    LibraryFunction { category: "Aggregation", name: "MAXX", template: "MAXX({table}, {table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "MINX", template: "MINX({table}, {table}[{numcol}])" },
    LibraryFunction { category: "Aggregation", name: "PRODUCTX", template: "PRODUCTX({table}, {table}[{numcol}])" },
    // Logical
    LibraryFunction { category: "Logical", name: "IF", template: "IF(TRUE(), 1, 0)" },
    LibraryFunction { category: "Logical", name: "IFERROR", template: "IFERROR(1/0, 0)" },
    LibraryFunction { category: "Logical", name: "SWITCH", template: "SWITCH(TRUE(), TRUE(), 1, 0)" },
    LibraryFunction { category: "Logical", name: "COALESCE", template: "COALESCE({table}[{numcol}], 0)" },
    LibraryFunction { category: "Logical", name: "AND", template: "AND(TRUE(), FALSE())" },
    LibraryFunction { category: "Logical", name: "OR", template: "OR(TRUE(), FALSE())" },
    LibraryFunction { category: "Logical", name: "NOT", template: "NOT(FALSE())" },
    LibraryFunction { category: "Logical", name: "TRUE", template: "TRUE()" },
    LibraryFunction { category: "Logical", name: "FALSE", template: "FALSE()" },
    // Filter
    LibraryFunction { category: "Filter", name: "CALCULATE", template: "CALCULATE([{measure}], {table}[{textcol}] = \"\")" },
    LibraryFunction { category: "Filter", name: "FILTER", template: "FILTER({table}, {table}[{numcol}] > 0)" },
    LibraryFunction { category: "Filter", name: "ALL", template: "ALL({table})" },
    LibraryFunction { category: "Filter", name: "ALLEXCEPT", template: "ALLEXCEPT({table}, {table}[{textcol}])" },
    LibraryFunction { category: "Filter", name: "ALLSELECTED", template: "ALLSELECTED({table})" },
    LibraryFunction { category: "Filter", name: "VALUES", template: "VALUES({table}[{textcol}])" },
    LibraryFunction { category: "Filter", name: "DISTINCT", template: "DISTINCT({table}[{textcol}])" },
    LibraryFunction { category: "Filter", name: "SELECTEDVALUE", template: "SELECTEDVALUE({table}[{textcol}])" },
    // Table
    LibraryFunction { category: "Table", name: "ROW", template: "ROW(\"Value\", 1)" },
    LibraryFunction { category: "Table", name: "ADDCOLUMNS", template: "ADDCOLUMNS({table}, \"Value\", {table}[{numcol}])" },
    LibraryFunction { category: "Table", name: "SELECTCOLUMNS", template: "SELECTCOLUMNS({table}, \"Value\", {table}[{numcol}])" },
    LibraryFunction { category: "Table", name: "TOPN", template: "TOPN(10, {table}, {table}[{numcol}], 0)" },
    LibraryFunction { category: "Table", name: "UNION", template: "UNION({table}, {table})" },
    LibraryFunction { category: "Table", name: "INTERSECT", template: "INTERSECT({table}, {table})" },
    LibraryFunction { category: "Table", name: "EXCEPT", template: "EXCEPT({table}, {table})" },
    LibraryFunction { category: "Table", name: "CROSSJOIN", template: "CROSSJOIN({table}, {table})" },
    LibraryFunction { category: "Table", name: "DATATABLE", template: "DATATABLE(\"Value\", INTEGER, {{1}, {2}})" },
    // Date
    LibraryFunction { category: "Date", name: "DATE", template: "DATE(2024, 1, 1)" },
    LibraryFunction { category: "Date", name: "YEAR", template: "YEAR(DATE(2024, 1, 1))" },
    LibraryFunction { category: "Date", name: "MONTH", template: "MONTH(DATE(2024, 1, 1))" },
    LibraryFunction { category: "Date", name: "DAY", template: "DAY(DATE(2024, 1, 1))" },
    LibraryFunction { category: "Date", name: "HOUR", template: "HOUR(TIME(12, 30, 0))" },
    LibraryFunction { category: "Date", name: "MINUTE", template: "MINUTE(TIME(12, 30, 0))" },
    LibraryFunction { category: "Date", name: "SECOND", template: "SECOND(TIME(12, 30, 0))" },
    LibraryFunction { category: "Date", name: "WEEKDAY", template: "WEEKDAY(DATE(2024, 1, 1))" },
    LibraryFunction { category: "Date", name: "EOMONTH", template: "EOMONTH(DATE(2024, 1, 1), 0)" },
    LibraryFunction { category: "Date", name: "EDATE", template: "EDATE(DATE(2024, 1, 1), 1)" },
    LibraryFunction { category: "Date", name: "DATEDIFF", template: "DATEDIFF(DATE(2024, 1, 1), DATE(2024, 12, 31), DAY)" },
    LibraryFunction { category: "Date", name: "TIME", template: "TIME(12, 30, 0)" },
    // Math
    LibraryFunction { category: "Math", name: "ABS", template: "ABS(-1)" },
    LibraryFunction { category: "Math", name: "ROUND", template: "ROUND(1.234, 2)" },
    LibraryFunction { category: "Math", name: "ROUNDUP", template: "ROUNDUP(1.234, 2)" },
    LibraryFunction { category: "Math", name: "ROUNDDOWN", template: "ROUNDDOWN(1.234, 2)" },
    LibraryFunction { category: "Math", name: "INT", template: "INT(1.9)" },
    LibraryFunction { category: "Math", name: "MOD", template: "MOD(5, 2)" },
    LibraryFunction { category: "Math", name: "POWER", template: "POWER(2, 3)" },
    LibraryFunction { category: "Math", name: "SQRT", template: "SQRT(9)" },
    LibraryFunction { category: "Math", name: "EXP", template: "EXP(1)" },
    LibraryFunction { category: "Math", name: "LN", template: "LN(2)" },
    LibraryFunction { category: "Math", name: "LOG", template: "LOG(8, 2)" },
    LibraryFunction { category: "Math", name: "LOG10", template: "LOG10(100)" },
    LibraryFunction { category: "Math", name: "SIGN", template: "SIGN(-1)" },
    LibraryFunction { category: "Math", name: "CEILING", template: "CEILING(1.2, 1)" },
    LibraryFunction { category: "Math", name: "FLOOR", template: "FLOOR(1.8, 1)" },
    LibraryFunction { category: "Math", name: "DIVIDE", template: "DIVIDE(10, 2, 0)" },
    LibraryFunction { category: "Math", name: "PI", template: "PI()" },
    LibraryFunction { category: "Math", name: "DEGREES", template: "DEGREES(PI())" },
    LibraryFunction { category: "Math", name: "RADIANS", template: "RADIANS(180)" },
    LibraryFunction { category: "Math", name: "SIN", template: "SIN(0)" },
    LibraryFunction { category: "Math", name: "COS", template: "COS(0)" },
    LibraryFunction { category: "Math", name: "TAN", template: "TAN(0)" },
    LibraryFunction { category: "Math", name: "FACT", template: "FACT(5)" },
    LibraryFunction { category: "Math", name: "GCD", template: "GCD(12, 18)" },
    LibraryFunction { category: "Math", name: "LCM", template: "LCM(4, 6)" },
    // Text
    LibraryFunction { category: "Text", name: "LEN", template: "LEN({table}[{textcol}])" },
    LibraryFunction { category: "Text", name: "UPPER", template: "UPPER({table}[{textcol}])" },
    LibraryFunction { category: "Text", name: "LOWER", template: "LOWER({table}[{textcol}])" },
    LibraryFunction { category: "Text", name: "TRIM", template: "TRIM({table}[{textcol}])" },
    LibraryFunction { category: "Text", name: "LEFT", template: "LEFT({table}[{textcol}], 1)" },
    LibraryFunction { category: "Text", name: "RIGHT", template: "RIGHT({table}[{textcol}], 1)" },
    LibraryFunction { category: "Text", name: "MID", template: "MID({table}[{textcol}], 1, 1)" },
    LibraryFunction { category: "Text", name: "CONCATENATE", template: "CONCATENATE({table}[{textcol}], \"\")" },
    LibraryFunction { category: "Text", name: "EXACT", template: "EXACT({table}[{textcol}], \"\")" },
    LibraryFunction { category: "Text", name: "REPT", template: "REPT({table}[{textcol}], 2)" },
    LibraryFunction { category: "Text", name: "SUBSTITUTE", template: "SUBSTITUTE({table}[{textcol}], \"\", \"\")" },
    LibraryFunction { category: "Text", name: "VALUE", template: "VALUE(\"1\")" },
    LibraryFunction { category: "Text", name: "FIND", template: "FIND(\"\", {table}[{textcol}], 1)" },
    LibraryFunction { category: "Text", name: "SEARCH", template: "SEARCH(\"\", {table}[{textcol}], 1)" },
    LibraryFunction { category: "Text", name: "UNICHAR", template: "UNICHAR(65)" },
    // Information
    LibraryFunction { category: "Information", name: "ISBLANK", template: "ISBLANK(BLANK())" },
    LibraryFunction { category: "Information", name: "ISNUMBER", template: "ISNUMBER(1)" },
    LibraryFunction { category: "Information", name: "ISTEXT", template: "ISTEXT(\"\")" },
    LibraryFunction { category: "Information", name: "ISNONTEXT", template: "ISNONTEXT(1)" },
    LibraryFunction { category: "Information", name: "ISLOGICAL", template: "ISLOGICAL(TRUE())" },
    LibraryFunction { category: "Information", name: "ISERROR", template: "ISERROR(1/0)" },
    LibraryFunction { category: "Information", name: "ISEVEN", template: "ISEVEN(2)" },
    LibraryFunction { category: "Information", name: "ISODD", template: "ISODD(1)" },
    LibraryFunction { category: "Information", name: "ISINTEGER", template: "ISINTEGER(1)" },
    LibraryFunction { category: "Information", name: "ISCURRENCY", template: "ISCURRENCY(1)" },
    // Other
    LibraryFunction { category: "Other", name: "PATH", template: "PATH(1, 2)" },
    LibraryFunction { category: "Other", name: "PATHLENGTH", template: "PATHLENGTH(PATH(1, 2))" },
    LibraryFunction { category: "Other", name: "PATHCONTAINS", template: "PATHCONTAINS(PATH(1, 2), 1)" },
    LibraryFunction { category: "Other", name: "PATHITEM", template: "PATHITEM(PATH(1, 2), 1)" },
    LibraryFunction { category: "Other", name: "BLANK", template: "BLANK()" },
];

const LIBRARY_CATEGORIES: &[&str] = &["All", "Aggregation", "Logical", "Filter", "Table", "Date", "Math", "Text", "Information", "Other"];

fn power_pivot_type_matches(data_type: &str, keys: &[&str]) -> bool {
    let lower = data_type.to_ascii_lowercase();
    keys.iter().any(|key| lower.contains(key))
}

/// Fills a library template from the first loaded table: its name, a numeric column, a text
/// column and the first measure. Falls back to the `Sales[Amount]`/`Sales[Region]` example.
fn power_pivot_fill_template(model: &Json, template: &str) -> String {
    let tables = model.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    let table = tables.first();
    let table_name = table.and_then(|table| table.get("name")).and_then(Json::as_str).unwrap_or("Sales").to_string();
    let columns = table.and_then(|table| table.get("columns")).and_then(Json::as_array).cloned().unwrap_or_default();
    let column_name = |entry: &Json| entry.get("name").and_then(Json::as_str).unwrap_or("").to_string();
    let numeric = columns
        .iter()
        .find(|column| {
            let data_type = column.get("type").and_then(Json::as_str).unwrap_or("");
            power_pivot_type_matches(data_type, &["int", "double", "decimal", "number", "currency", "real", "float"])
        })
        .or_else(|| columns.first())
        .map(column_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Amount".to_string());
    let text = columns
        .iter()
        .find(|column| {
            let data_type = column.get("type").and_then(Json::as_str).unwrap_or("");
            power_pivot_type_matches(data_type, &["string", "text"])
        })
        .or_else(|| columns.first())
        .map(column_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Region".to_string());
    let measure = model
        .get("measures")
        .and_then(Json::as_array)
        .and_then(|measures| measures.first())
        .and_then(|measure| measure.get("name"))
        .and_then(Json::as_str)
        .unwrap_or("Total Sales")
        .to_string();
    template.replace("{table}", &table_name).replace("{numcol}", &numeric).replace("{textcol}", &text).replace("{measure}", &measure)
}

/// Inserts a filled template into the active tab's formula field and closes the picker.
fn power_pivot_insert_function(d: &mut Dialog, template: &str) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let formula = power_pivot_fill_template(&model, template);
    let tab = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    if tab == "Design" {
        d.values.insert("draftFormula".into(), json!(formula));
    } else {
        d.values.insert("formula".into(), json!(formula));
    }
    d.values.insert("showFunctions".into(), json!(false));
}

/// The Function Library picker, opened from the ribbon. It floats over the dialog so it does not
/// displace the Excel Data View layout.
fn power_pivot_functions_panel(ctx: &egui::Context, d: &mut Dialog) {
    let t = theme::Tokens::get(ctx);
    let category = d.values.get("fnCategory").and_then(Json::as_str).unwrap_or("All").to_string();
    let mut search = d.values.get("fnSearch").and_then(Json::as_str).unwrap_or("").to_string();
    let mut close = false;
    egui::Window::new("Function Picker")
        .id(egui::Id::new("power_pivot_functions"))
        .collapsible(false)
        .resizable(false)
        .default_width(620.0)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Insert function").font(theme::ui_bold(13.0)).color(t.text));
                ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search functions").desired_width(240.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Close").clicked() {
                        close = true;
                    }
                });
            });
            if search != d.values.get("fnSearch").and_then(Json::as_str).unwrap_or("") {
                d.values.insert("fnSearch".into(), json!(search));
            }
            ui.horizontal_wrapped(|ui| {
                for candidate in LIBRARY_CATEGORIES {
                    if ui.add(egui::Button::selectable(category == *candidate, egui::RichText::new(*candidate).small())).clicked() {
                        d.values.insert("fnCategory".into(), json!(candidate));
                    }
                }
            });
            ui.separator();
            let needle = search.to_ascii_lowercase();
            let current = d.values.get("fnCategory").and_then(Json::as_str).unwrap_or("All").to_string();
            let model = d.values.get("model").cloned().unwrap_or(Json::Null);
            let mut shown = 0usize;
            egui::ScrollArea::vertical().id_salt("pp_fn_list").max_height(210.0).auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for function in LIBRARY_FUNCTIONS {
                        if current != "All" && function.category != current.as_str() {
                            continue;
                        }
                        if !needle.is_empty() && !function.name.to_ascii_lowercase().contains(&needle) {
                            continue;
                        }
                        shown += 1;
                        let hint = power_pivot_fill_template(&model, function.template);
                        if ui.add(egui::Button::new(function.name).small()).on_hover_text(hint).clicked() {
                            power_pivot_insert_function(d, function.template);
                        }
                    }
                });
                if shown == 0 {
                    ui.label(egui::RichText::new("No functions match.").italics().color(t.text_dim));
                }
            });
        });
    if close {
        d.values.insert("showFunctions".into(), json!(false));
    }
}

/// The Power Pivot window. It follows Excel's Power Pivot layout: a ribbon with Home, Design and
/// Diagram tabs, the model tree on the left, a DAX formula bar, and a pivot preview grid.
/// Everything runs through the `dataModel.*` engine commands, so agents and the control channel
/// see the same model the dialog edits.
pub(super) fn power_pivot(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let t = theme::Tokens::get(ui.ctx());
    power_pivot_header(ui, d, &t);
    ui.add_space(3.0);
    power_pivot_tabs(ui, d, &t);
    power_pivot_ribbon(app, ui, d, &t);
    power_pivot_path(app, ui, d, &t);
    ui.add_space(4.0);
    if d.values.get("showFunctions").and_then(Json::as_bool).unwrap_or(false) {
        let ctx = ui.ctx().clone();
        power_pivot_functions_panel(&ctx, d);
    }
    if d.values.get("showPivot").and_then(Json::as_bool).unwrap_or(false) {
        let ctx = ui.ctx().clone();
        power_pivot_preview(&ctx, app, d);
    }
    match d.values.get("tab").and_then(Json::as_str).unwrap_or("Home") {
        "Design" => power_pivot_design(app, ui, d, &t),
        "Diagram" => power_pivot_diagram(ui, d, &t),
        _ => power_pivot_home(app, ui, d, &t),
    }
}

/// The dialog's title strip: a data-model icon and the model's size.
fn power_pivot_header(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            icons::paint(ui.painter(), icon, Icon::Table, t.accent);
            ui.label(egui::RichText::new("Data Model").font(theme::ui_bold(14.0)).color(t.text));
            let model = d.values.get("model").cloned().unwrap_or(Json::Null);
            let loaded = model.get("loaded").and_then(Json::as_bool).unwrap_or(false);
            let tables = model.get("tables").and_then(Json::as_array).map_or(0, Vec::len);
            let measures = model.get("measures").and_then(Json::as_array).map_or(0, Vec::len);
            let summary = if loaded { format!("{tables} tables · {measures} measures") } else { "not loaded".to_string() };
            ui.label(egui::RichText::new(summary).small().color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("Power Pivot prototype").small().color(t.text_disabled));
            });
        });
    });
}

/// Excel's in-window tab strip: Home, Design and Diagram.
fn power_pivot_tabs(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let current = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    let strip = egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            for tab in ["Home", "Design", "Diagram"] {
                let selected = current == tab;
                let response = ui.add(egui::Button::selectable(selected, egui::RichText::new(tab).font(theme::ui_font(12.5))));
                if response.clicked() {
                    d.values.insert("tab".into(), json!(tab));
                }
            }
        });
    });
    let rect = strip.response.rect;
    ui.painter().line_segment([pos2(rect.left(), rect.bottom()), pos2(rect.right(), rect.bottom())], Stroke::new(1.0, t.ribbon_border));
}

/// A ribbon group: its buttons above, Excel's group name underneath.
fn power_pivot_group(ui: &mut egui::Ui, t: &theme::Tokens, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            add(ui);
        });
        ui.add_space(1.0);
        ui.label(egui::RichText::new(label).font(theme::ui_font(10.0)).color(t.text_dim));
    });
}

fn power_pivot_separator(ui: &mut egui::Ui, t: &theme::Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 84.0), Sense::hover());
    ui.painter().line_segment([pos2(rect.center().x, rect.top() + 6.0), pos2(rect.center().x, rect.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

/// A big ribbon button with an accessibility label (the painted label is not a widget).
fn power_pivot_button(ui: &mut egui::Ui, icon: Icon, label: &str, tip: &str) -> egui::Response {
    let response = crate::widgets::big_button(ui, icon, label, tip, false);
    let accessible = label.replace('\n', " ");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &accessible));
    response
}

/// A disabled ribbon action: Excel's Add Measure/Delete are placeholders in this prototype.
fn power_pivot_placeholder(ui: &mut egui::Ui, label: &str, tip: &str) {
    let response = ui.add_enabled(false, egui::Button::new(label).min_size(vec2(86.0, 60.0)));
    response.on_disabled_hover_text(tip);
}

/// The tab-specific ribbon, grouped like Excel's Power Pivot window.
fn power_pivot_ribbon(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let tab = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.horizontal(|ui| match tab.as_str() {
            "Design" => {
                power_pivot_group(ui, t, "Tables", |ui| {
                    if power_pivot_button(ui, Icon::Link, "Manage\nRelationships", "Show the model's relationships").clicked() {
                        let shown = d.values.get("showRelationships").and_then(Json::as_bool).unwrap_or(false);
                        d.values.insert("showRelationships".into(), json!(!shown));
                    }
                });
                power_pivot_separator(ui, t);
                power_pivot_group(ui, t, "Calculations", |ui| {
                    power_pivot_placeholder(ui, "Add Measure", "Adding measures is not available in this prototype");
                    power_pivot_placeholder(ui, "Delete", "Deleting measures is not available in this prototype");
                    if power_pivot_button(ui, Icon::Function, "Functions", "Open the Function Library").clicked() {
                        let shown = d.values.get("showFunctions").and_then(Json::as_bool).unwrap_or(false);
                        d.values.insert("showFunctions".into(), json!(!shown));
                    }
                });
            }
            "Diagram" => {
                power_pivot_group(ui, t, "View", |ui| {
                    if power_pivot_button(ui, Icon::Zoom, "Fit", "Fit the table cards to the window").clicked() {
                        d.values.insert("diagramFit".into(), json!(true));
                    }
                    if power_pivot_button(ui, Icon::Table, "Data View", "Return to the table data view").clicked() {
                        d.values.insert("tab".into(), json!("Home"));
                    }
                    if power_pivot_button(ui, Icon::Redo, "Refresh", "Re-read the model from the engine session").clicked() {
                        power_pivot_refresh(app, d);
                    }
                });
            }
            _ => {
                power_pivot_group(ui, t, "View", |ui| {
                    if power_pivot_button(ui, Icon::Table, "Data View", "Show the table data and calculations").clicked() {
                        d.values.insert("tab".into(), json!("Home"));
                    }
                    if power_pivot_button(ui, Icon::PivotTable, "Diagram View", "Show table cards and relationships").clicked() {
                        d.values.insert("tab".into(), json!("Diagram"));
                    }
                    if power_pivot_button(ui, Icon::PivotTable, "Pivot Preview", "Show or hide the pivot preview").clicked() {
                        let shown = d.values.get("showPivot").and_then(Json::as_bool).unwrap_or(false);
                        d.values.insert("showPivot".into(), json!(!shown));
                    }
                });
                power_pivot_separator(ui, t);
                power_pivot_group(ui, t, "Calculations", |ui| {
                    power_pivot_placeholder(ui, "Add Measure", "Adding measures is not available in this prototype");
                    power_pivot_placeholder(ui, "Delete", "Deleting measures is not available in this prototype");
                    if power_pivot_button(ui, Icon::Function, "Functions", "Open the Function Library").clicked() {
                        let shown = d.values.get("showFunctions").and_then(Json::as_bool).unwrap_or(false);
                        d.values.insert("showFunctions".into(), json!(!shown));
                    }
                });
                power_pivot_separator(ui, t);
                power_pivot_group(ui, t, "Data", |ui| {
                    if power_pivot_button(ui, Icon::Redo, "Refresh", "Re-read the model from the engine session").clicked() {
                        power_pivot_refresh(app, d);
                    }
                    if power_pivot_button(ui, Icon::Calc, "Recalculate", "Re-evaluate the current formula").clicked() {
                        power_pivot_recalculate(app, d);
                    }
                });
            }
        });
    });
}

/// The model file path, kept visible on every tab.
fn power_pivot_path(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, _t: &theme::Tokens) {
    ui.add_space(5.0);
    ui.horizontal(|ui| {
        ui.label("Model file");
        let mut path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
        let field = (ui.available_width() - 86.0).max(140.0);
        if ui.add_sized([field, 23.0], egui::TextEdit::singleline(&mut path).hint_text("model.json")).changed() {
            d.values.insert("path".into(), json!(path));
        }
        if accent_button(ui, "Load").clicked() {
            power_pivot_load(app, d);
        }
    });
}

/// Home's Data View: formula bar, the selected table's grid, calculations, then the table tabs.
/// The pivot preview is a secondary panel, not the main surface.
fn power_pivot_home(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    power_pivot_formula(app, ui, d, t);
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("pp_home_view").max_height(420.0).auto_shrink([false, false]).show(ui, |ui| {
        power_pivot_data_grid(ui, d, t);
        ui.add_space(4.0);
        power_pivot_calculations(ui, d, t);
        ui.add_space(4.0);
        power_pivot_result(ui, d, t);
    });
    ui.add_space(4.0);
    power_pivot_table_tabs(app, ui, d, t);
    let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
    if !error.is_empty() {
        ui.add_space(2.0);
        ui.label(egui::RichText::new(error).color(t.danger).small());
    }
}

/// The data grid for the selected table, headed by its columns.
fn power_pivot_data_grid(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let data = d.values.get("tableData").cloned().unwrap_or(Json::Null);
    let rows = data.get("rows").and_then(Json::as_array).map_or(0, Vec::len);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Table Data").strong().color(t.text));
        if !data.is_null() {
            ui.label(egui::RichText::new(format!("{rows} rows")).small().color(t.text_dim));
        } else {
            ui.label(egui::RichText::new("no table selected").small().color(t.text_dim));
        }
    });
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(6).show(ui, |ui| {
        ui.set_min_height(190.0);
        ui.set_min_width(ui.available_width());
        if data.get("columns").is_none() {
            ui.label(egui::RichText::new("Load a model and choose a table tab to see its rows.").italics().color(t.text_dim));
            return;
        }
        let columns = data.get("columns").and_then(Json::as_array).cloned().unwrap_or_default();
        let rows = data.get("rows").and_then(Json::as_array).cloned().unwrap_or_default();
        power_pivot_grid(ui, "pp_data_grid", &columns, &rows, t, 180.0);
    });
}

/// Excel's calculation area: measures as a Name / Formula / Format grid below the data.
fn power_pivot_calculations(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let measures = model.get("measures").and_then(Json::as_array).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Calculation Area").strong().color(t.text));
        ui.label(egui::RichText::new(format!("{} measures", measures.len())).small().color(t.text_dim));
    });
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(6).show(ui, |ui| {
        ui.set_min_height(64.0);
        ui.set_min_width(ui.available_width());
        if measures.is_empty() {
            ui.label(egui::RichText::new("No measures in this model.").italics().color(t.text_dim));
            return;
        }
        egui::ScrollArea::vertical().id_salt("pp_calculations").max_height(140.0).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("pp_calculations_grid").striped(true).spacing(vec2(14.0, 3.0)).show(ui, |ui| {
                for heading in ["Name", "Formula", "Format"] {
                    egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                        ui.label(egui::RichText::new(heading).font(theme::ui_bold(12.0)).color(t.header_text));
                    });
                }
                ui.end_row();
                for measure in &measures {
                    let name = measure.get("name").and_then(Json::as_str).unwrap_or("");
                    let formula = measure.get("formula").and_then(Json::as_str).unwrap_or("");
                    let format = measure.get("format").and_then(Json::as_str).unwrap_or("");
                    if ui.add(egui::Button::new(name).frame(false)).clicked() {
                        d.values.insert("formula".into(), json!(format!("[{name}]")));
                    }
                    ui.label(egui::RichText::new(formula).color(t.text));
                    ui.label(egui::RichText::new(format).color(t.text_dim));
                    ui.end_row();
                }
            });
        });
    });
}

/// Excel's table tabs at the bottom of the Data View.
fn power_pivot_table_tabs(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let tables = model.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    let selected = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
    egui::Frame::NONE.fill(t.tab_bar).stroke(Stroke::new(1.0, t.ribbon_border)).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.horizontal(|ui| {
            if tables.is_empty() {
                ui.label(egui::RichText::new("No table tabs").small().color(t.text_dim));
            }
            for (index, table) in tables.iter().enumerate() {
                let name = table.get("name").and_then(Json::as_str).unwrap_or("Table");
                if ui.add(egui::Button::selectable(index == selected, egui::RichText::new(name).font(theme::ui_font(12.0)))).clicked() {
                    power_pivot_select_table(app, d, index);
                }
            }
        });
    });
}

fn power_pivot_load(app: &mut SheetApp, d: &mut Dialog) {
    let path = d.values.get("path").and_then(Json::as_str).unwrap_or("").trim().to_string();
    if path.is_empty() {
        d.values.insert("error".into(), json!("Enter the path to a model JSON file."));
        return;
    }
    match app.session.run("dataModel.load", json!({"path": path})) {
        Ok(model) => {
            d.values.insert("model".into(), model);
            d.values.insert("error".into(), json!(""));
            d.values.insert("pivotError".into(), json!(""));
            d.values.insert("pivotNote".into(), json!(""));
            d.values.remove("pivot");
            d.values.remove("tableData");
            d.values.insert("tableIndex".into(), json!(0));
            d.values.insert("draftIndex".into(), json!(-1));
            d.values.insert("draftName".into(), json!(""));
            d.values.insert("draftFormula".into(), json!(""));
            d.result = None;
            power_pivot_select_table(app, d, 0);
        }
        Err(e) => {
            d.values.insert("error".into(), json!(crate::clean_error(&e)));
            d.result = None;
        }
    }
}

fn power_pivot_refresh(app: &mut SheetApp, d: &mut Dialog) {
    match app.session.run("dataModel.status", json!({})) {
        Ok(model) => {
            d.values.insert("model".into(), model);
            d.values.insert("error".into(), json!(""));
            let index = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
            power_pivot_select_table(app, d, index);
        }
        Err(e) => {
            d.values.insert("error".into(), json!(crate::clean_error(&e)));
        }
    }
}

/// Selects a Data View table tab and fetches its rows as a table expression (`Table` + `table`).
fn power_pivot_select_table(app: &mut SheetApp, d: &mut Dialog, index: usize) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let tables = model.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    let Some(table) = tables.get(index) else {
        d.values.remove("tableData");
        return;
    };
    let Some(name) = table.get("name").and_then(Json::as_str) else {
        d.values.remove("tableData");
        return;
    };
    d.values.insert("tableIndex".into(), json!(index));
    match app.session.run("dataModel.evaluate", json!({"formula": name, "table": true})) {
        Ok(data) => {
            d.values.insert("tableData".into(), data);
            d.values.insert("error".into(), json!(""));
        }
        Err(e) => {
            d.values.remove("tableData");
            d.values.insert("error".into(), json!(crate::clean_error(&e)));
        }
    }
}

/// Recalculates the visible formula, as Excel's Recalculate does for the model.
fn power_pivot_recalculate(app: &mut SheetApp, d: &mut Dialog) {
    power_pivot_refresh(app, d);
    let tab = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    let formula = if tab == "Design" {
        d.values.get("draftFormula").and_then(Json::as_str).unwrap_or("").trim().to_string()
    } else {
        d.values.get("formula").and_then(Json::as_str).unwrap_or("").trim().to_string()
    };
    if !formula.is_empty() {
        power_pivot_evaluate(app, d);
    }
}

/// Runs `dataModel.evaluate` for the active tab's formula (or its selected measure).
fn power_pivot_evaluate(app: &mut SheetApp, d: &mut Dialog) {
    let tab = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    let design = tab == "Design";
    let (formula, table, measure) = if design {
        (
            d.values.get("draftFormula").and_then(Json::as_str).unwrap_or("").trim().to_string(),
            false,
            d.values.get("draftName").and_then(Json::as_str).unwrap_or("").trim().to_string(),
        )
    } else {
        (
            d.values.get("formula").and_then(Json::as_str).unwrap_or("").trim().to_string(),
            d.values.get("table").and_then(Json::as_bool).unwrap_or(false),
            d.values.get("measure").and_then(Json::as_str).unwrap_or("").trim().to_string(),
        )
    };
    let formula = formula.trim_start_matches('=').trim().to_string();
    let had_formula = !formula.is_empty();
    let outcome = if had_formula {
        app.session.run("dataModel.evaluate", json!({"formula": formula, "table": table}))
    } else if !measure.is_empty() {
        app.session.run("dataModel.evaluate", json!({"measure": measure}))
    } else {
        Err(if design { "Choose a measure or enter a DAX formula.".to_string() } else { "Enter a DAX formula or choose a measure.".to_string() })
    };
    match outcome {
        Ok(result) => {
            let kind = if had_formula { if table { "Table" } else { "Scalar" } } else { "Measure" };
            d.values.insert("error".into(), json!(""));
            d.values.insert("resultKind".into(), json!(kind));
            d.result = Some(result);
        }
        Err(e) => {
            d.values.insert("error".into(), json!(crate::clean_error(&e)));
            d.result = None;
        }
    }
}

/// Runs `dataModel.pivot` for the preview, with column fields when present.
fn power_pivot_run_pivot(app: &mut SheetApp, d: &mut Dialog) {
    let measure = d.values.get("measure").and_then(Json::as_str).unwrap_or("").trim().to_string();
    let rows_text = d.values.get("rows").and_then(Json::as_str).unwrap_or("").to_string();
    let columns_text = d.values.get("columns").and_then(Json::as_str).unwrap_or("").to_string();
    let rows: Vec<String> = rows_text.split(',').map(str::trim).filter(|field| !field.is_empty()).map(str::to_string).collect();
    let columns: Vec<String> = columns_text.split(',').map(str::trim).filter(|field| !field.is_empty()).map(str::to_string).collect();
    if measure.is_empty() {
        d.values.insert("pivotError".into(), json!("Enter the measure to pivot, e.g. Total Sales."));
        return;
    }
    if rows.is_empty() {
        d.values.insert("pivotError".into(), json!("Enter at least one row field, e.g. Sales[Region]."));
        return;
    }
    let mut params = json!({"measure": measure, "rows": rows});
    if !columns.is_empty() {
        params["columns"] = json!(columns);
    }
    match app.session.run("dataModel.pivot", params) {
        Ok(result) => {
            d.values.insert("pivot".into(), result);
            d.values.insert("pivotError".into(), json!(""));
            d.values.insert("pivotNote".into(), json!(""));
        }
        // The engine may predate column fields: retry rows-only rather than losing the pivot.
        Err(_) if !columns.is_empty() => match app.session.run("dataModel.pivot", json!({"measure": measure, "rows": rows})) {
            Ok(result) => {
                d.values.insert("pivot".into(), result);
                d.values.insert("pivotError".into(), json!(""));
                d.values.insert("pivotNote".into(), json!("Column fields are not supported by this engine yet; showing rows only."));
            }
            Err(e) => {
                d.values.insert("pivotError".into(), json!(crate::clean_error(&e)));
                d.values.insert("pivotNote".into(), json!(""));
                d.values.remove("pivot");
            }
        },
        Err(e) => {
            d.values.insert("pivotError".into(), json!(crate::clean_error(&e)));
            d.values.insert("pivotNote".into(), json!(""));
            d.values.remove("pivot");
        }
    }
}

/// The formula bar above the data grid: `fx`, the DAX expression, the table toggle and Evaluate.
fn power_pivot_formula(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Formula").strong().color(t.text));
        ui.label(egui::RichText::new("DAX formula, measures as [Name]").small().color(t.text_dim));
    });
    ui.add_space(2.0);
    let mut formula = d.values.get("formula").and_then(Json::as_str).unwrap_or("").to_string();
    egui::Frame::NONE.fill(t.input_bg).stroke(Stroke::new(1.0, t.input_border)).corner_radius(3.0).inner_margin(egui::Margin::symmetric(5, 2)).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), icon, Icon::Function, t.text_dim);
                let width = ui.available_width();
                if ui
                    .add_sized([width, 20.0], egui::TextEdit::singleline(&mut formula).frame(egui::Frame::NONE).hint_text("SUM(Sales[Amount])"))
                    .changed()
                {
                    d.values.insert("formula".into(), json!(formula));
                }
            });
        },
    );
    ui.add_space(3.0);
    let mut table = d.values.get("table").and_then(Json::as_bool).unwrap_or(false);
    ui.horizontal(|ui| {
        if ui.checkbox(&mut table, "Table (--table)").changed() {
            d.values.insert("table".into(), json!(table));
        }
        ui.label(egui::RichText::new("Leave Formula empty to evaluate the pivot preview's measure.").small().color(t.text_dim));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if accent_button(ui, "Evaluate").clicked() {
                power_pivot_evaluate(app, d);
            }
        });
    });
    // Keep a typed formula alive across the closure.
    d.values.insert("formula".into(), json!(formula));
}

/// The result surface: a scalar, or `{columns, rows}` as a headed grid.
fn power_pivot_result(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let kind = d.values.get("resultKind").and_then(Json::as_str).unwrap_or("Result").to_string();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Result").strong().color(t.text));
        if d.result.is_some() {
            ui.label(egui::RichText::new(kind).small().color(t.text_dim));
        }
    });
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(6).show(ui, |ui| {
        ui.set_min_height(132.0);
        ui.set_min_width(ui.available_width());
        let Some(result) = d.result.as_ref() else {
            ui.label(egui::RichText::new("Evaluate a formula to see the result here.").italics().color(t.text_dim));
            return;
        };
        if let Some(columns) = result.get("columns").and_then(Json::as_array) {
            let rows = result.get("rows").and_then(Json::as_array).cloned().unwrap_or_default();
            power_pivot_grid(ui, "pp_result_grid", columns, &rows, t, 140.0);
        } else if let Some(value) = result.get("value") {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Value").small().color(t.text_dim));
                ui.label(egui::RichText::new(json_scalar_text(value)).font(theme::ui_bold(20.0)).color(t.text));
            });
        } else {
            ui.monospace(serde_json::to_string_pretty(result).unwrap_or_default());
        }
    });
}

/// A headed, striped grid. Rows are capped so a large table cannot bloat the dialog.
fn power_pivot_grid(ui: &mut egui::Ui, id: &str, columns: &[Json], rows: &[Json], t: &theme::Tokens, max_height: f32) {
    const MAX_ROWS: usize = 200;
    egui::ScrollArea::both().id_salt(id).max_height(max_height).auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new(id).striped(true).spacing(vec2(14.0, 3.0)).show(ui, |ui| {
            for column in columns {
                let name = column.as_str().unwrap_or("");
                egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                    ui.label(egui::RichText::new(name).font(theme::ui_bold(12.0)).color(t.header_text));
                });
            }
            ui.end_row();
            for row in rows.iter().take(MAX_ROWS) {
                let cells = row.as_array().cloned().unwrap_or_default();
                for cell in &cells {
                    ui.label(egui::RichText::new(json_scalar_text(cell)).color(t.text));
                }
                ui.end_row();
            }
        });
        if rows.len() > MAX_ROWS {
            ui.label(egui::RichText::new(format!("Showing the first {MAX_ROWS} of {} rows.", rows.len())).small().color(t.text_dim));
        }
    });
}

/// The pivot preview: a secondary floating surface with measure, row and column fields and the
/// headed result grid.
fn power_pivot_preview(ctx: &egui::Context, app: &mut SheetApp, d: &mut Dialog) {
    let t = theme::Tokens::get(ctx);
    let mut close = false;
    egui::Window::new("Pivot Preview")
        .id(egui::Id::new("power_pivot_pivot"))
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::paint(ui.painter(), icon, Icon::PivotTable, t.accent);
                ui.label(egui::RichText::new("Pivot Result").font(theme::ui_bold(12.5)).color(t.text));
                ui.label(egui::RichText::new("measure, row fields, then column fields").small().color(t.text_dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Close").clicked() {
                        close = true;
                    }
                });
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Measure").small().color(t.text_dim));
                let mut measure = d.values.get("measure").and_then(Json::as_str).unwrap_or("").to_string();
                if ui.add_sized([130.0, 23.0], egui::TextEdit::singleline(&mut measure).hint_text("Total Sales")).changed() {
                    d.values.insert("measure".into(), json!(measure));
                }
                ui.label(egui::RichText::new("Rows").small().color(t.text_dim));
                let width = (ui.available_width() - 10.0).max(120.0);
                let mut rows_text = d.values.get("rows").and_then(Json::as_str).unwrap_or("").to_string();
                if ui.add_sized([width, 23.0], egui::TextEdit::singleline(&mut rows_text).hint_text("Sales[Region]")).changed() {
                    d.values.insert("rows".into(), json!(rows_text));
                }
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Columns").small().color(t.text_dim));
                let width = (ui.available_width() - 92.0).max(120.0);
                let mut columns_text = d.values.get("columns").and_then(Json::as_str).unwrap_or("").to_string();
                if ui.add_sized([width, 23.0], egui::TextEdit::singleline(&mut columns_text).hint_text("Sales[Year]")).changed() {
                    d.values.insert("columns".into(), json!(columns_text));
                }
                if accent_button(ui, "Pivot").clicked() {
                    power_pivot_run_pivot(app, d);
                }
            });
            ui.add_space(3.0);
            egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(6).show(ui, |ui| {
                ui.set_min_height(170.0);
                ui.set_min_width(ui.available_width());
                let error = d.values.get("pivotError").and_then(Json::as_str).unwrap_or("").to_string();
                if !error.is_empty() {
                    ui.label(egui::RichText::new(error).color(t.danger).small());
                    return;
                }
                let Some(pivot) = d.values.get("pivot").filter(|pivot| !pivot.is_null()) else {
                    ui.label(egui::RichText::new("Choose a measure and row fields, then choose Pivot.").italics().color(t.text_dim));
                    return;
                };
                let columns = pivot.get("columns").and_then(Json::as_array).cloned().unwrap_or_default();
                let rows = pivot.get("rows").and_then(Json::as_array).cloned().unwrap_or_default();
                power_pivot_grid(ui, "pp_pivot_grid", &columns, &rows, &t, 154.0);
            });
            let note = d.values.get("pivotNote").and_then(Json::as_str).unwrap_or("").to_string();
            if !note.is_empty() {
                ui.label(egui::RichText::new(note).small().color(t.text_dim));
            }
        });
    if close {
        d.values.insert("showPivot".into(), json!(false));
    }
}

/// Design: the measure/field surface. Measure edits are drafts; Evaluate runs the draft formula.
fn power_pivot_design(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let loaded = model.get("loaded").and_then(Json::as_bool).unwrap_or(false);
    let measures = model.get("measures").and_then(Json::as_array).cloned().unwrap_or_default();
    let tables = model.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    if d.values.get("showRelationships").and_then(Json::as_bool).unwrap_or(false) {
        power_pivot_relationships(ui, &model, t);
        ui.add_space(4.0);
    }
    ui.horizontal_top(|ui| {
        egui::Frame::NONE.fill(t.window).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(6).show(ui, |ui| {
            ui.set_width(250.0);
            ui.set_min_height(470.0);
            egui::ScrollArea::vertical().id_salt("pp_design_list").auto_shrink([false, false]).show(ui, |ui| {
                ui.label(egui::RichText::new("MEASURES").small().font(theme::ui_bold(10.5)).color(t.text_dim));
                if !loaded {
                    ui.label(egui::RichText::new("No data model loaded.").italics().color(t.text_dim));
                } else if measures.is_empty() {
                    ui.label(egui::RichText::new("(none)").small().italics().color(t.text_dim));
                }
                let selected = d.values.get("draftIndex").and_then(Json::as_i64).unwrap_or(-1);
                for (index, measure) in measures.iter().enumerate() {
                    let name = measure.get("name").and_then(Json::as_str).unwrap_or("Measure");
                    let formula = measure.get("formula").and_then(Json::as_str).unwrap_or("");
                    let chosen = selected == index as i64;
                    if ui.add(egui::Button::selectable(chosen, egui::RichText::new(name).font(theme::ui_font(12.0)))).clicked() {
                        d.values.insert("draftIndex".into(), json!(index));
                        d.values.insert("draftName".into(), json!(name));
                        d.values.insert("draftFormula".into(), json!(formula));
                        d.values.insert("error".into(), json!(""));
                    }
                    if !formula.is_empty() {
                        ui.label(egui::RichText::new(formula).small().color(t.text_dim));
                    }
                }
                ui.add_space(7.0);
                ui.separator();
                ui.label(egui::RichText::new("FIELDS").small().font(theme::ui_bold(10.5)).color(t.text_dim));
                if tables.is_empty() && loaded {
                    ui.label(egui::RichText::new("(none)").small().italics().color(t.text_dim));
                }
                for table in &tables {
                    let name = table.get("name").and_then(Json::as_str).unwrap_or("Table");
                    ui.label(egui::RichText::new(name).strong().color(t.text));
                    for column in table.get("columns").and_then(Json::as_array).into_iter().flatten() {
                        let column_name = column.get("name").and_then(Json::as_str).unwrap_or("");
                        let data_type = column.get("type").and_then(Json::as_str).unwrap_or("");
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("  {column_name}")).small().color(t.text));
                            if !data_type.is_empty() {
                                ui.label(egui::RichText::new(data_type).small().color(t.text_dim));
                            }
                        });
                    }
                }
            });
        });
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Measure definition").strong().color(t.text));
                let selected = d.values.get("draftIndex").and_then(Json::as_i64).unwrap_or(-1);
                if selected >= 0 {
                    ui.label(egui::RichText::new("draft").small().color(t.text_dim));
                }
            });
            ui.add_space(2.0);
            ui.label(egui::RichText::new("Name").small().color(t.text_dim));
            let mut name = d.values.get("draftName").and_then(Json::as_str).unwrap_or("").to_string();
            let width = ui.available_width().min(360.0);
            if ui.add_sized([width, 23.0], egui::TextEdit::singleline(&mut name).hint_text("Total Sales")).changed() {
                d.values.insert("draftName".into(), json!(name));
            }
            ui.add_space(3.0);
            ui.label(egui::RichText::new("Formula").small().color(t.text_dim));
            let mut formula = d.values.get("draftFormula").and_then(Json::as_str).unwrap_or("").to_string();
            egui::Frame::NONE.fill(t.input_bg).stroke(Stroke::new(1.0, t.input_border)).corner_radius(3.0).inner_margin(4).show(ui, |ui| {
                let width = ui.available_width();
                if ui
                    .add_sized(
                        [width, 86.0],
                        egui::TextEdit::multiline(&mut formula).frame(egui::Frame::NONE).hint_text("SUM(Sales[Amount])").desired_rows(4),
                    )
                    .changed()
                {
                    d.values.insert("draftFormula".into(), json!(formula));
                }
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Measure edits are a draft; Evaluate runs this formula. Add/Delete are placeholders.")
                        .small()
                        .color(t.text_dim),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if accent_button(ui, "Evaluate").clicked() {
                        power_pivot_evaluate(app, d);
                    }
                });
            });
            ui.add_space(4.0);
            power_pivot_result(ui, d, t);
            let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
            if !error.is_empty() {
                ui.add_space(2.0);
                ui.label(egui::RichText::new(error).color(t.danger).small());
            }
        });
    });
}

/// Design › Tables › Manage Relationships: the read-only relationship list from `dataModel.status`.
fn power_pivot_relationships(ui: &mut egui::Ui, model: &Json, t: &theme::Tokens) {
    let relationships = model.get("relationships").and_then(Json::as_array).cloned().unwrap_or_default();
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(6).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Relationships").strong().color(t.text));
            ui.label(egui::RichText::new(format!("{} defined", relationships.len())).small().color(t.text_dim));
        });
        if relationships.is_empty() {
            ui.label(egui::RichText::new("No relationships are exposed by this model.").italics().color(t.text_dim));
            return;
        }
        egui::ScrollArea::vertical().id_salt("pp_relationships").max_height(110.0).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("pp_relationships_grid").striped(true).spacing(vec2(18.0, 3.0)).show(ui, |ui| {
                for heading in ["From", "To"] {
                    egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                        ui.label(egui::RichText::new(heading).font(theme::ui_bold(12.0)).color(t.header_text));
                    });
                }
                ui.end_row();
                for relationship in &relationships {
                    let from_table = relationship.get("from_table").and_then(Json::as_str).unwrap_or("");
                    let from_column = relationship.get("from_column").and_then(Json::as_str).unwrap_or("");
                    let to_table = relationship.get("to_table").and_then(Json::as_str).unwrap_or("");
                    let to_column = relationship.get("to_column").and_then(Json::as_str).unwrap_or("");
                    ui.label(egui::RichText::new(format!("{from_table}[{from_column}]")).color(t.text));
                    ui.label(egui::RichText::new(format!("→ {to_table}[{to_column}]")).color(t.text));
                    ui.end_row();
                }
            });
        });
    });
}

/// Diagram: Excel's table cards, with relationship lines when the model exposes relationships.
fn power_pivot_diagram(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let loaded = model.get("loaded").and_then(Json::as_bool).unwrap_or(false);
    let tables: Vec<Json> = model.get("tables").and_then(Json::as_array).cloned().unwrap_or_default().into_iter().take(60).collect();
    let relationships = model.get("relationships").and_then(Json::as_array).cloned().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Model Diagram").strong().color(t.text));
        if loaded {
            ui.label(egui::RichText::new(format!("{} tables · {} relationships", tables.len(), relationships.len())).small().color(t.text_dim));
        } else {
            ui.label(egui::RichText::new("No data model loaded.").italics().color(t.text_dim));
        }
    });
    ui.add_space(2.0);
    if !loaded || tables.is_empty() {
        ui.label(egui::RichText::new("Load a model to see its tables.").small().color(t.text_dim));
        let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
        if !error.is_empty() {
            ui.label(egui::RichText::new(error).color(t.danger).small());
        }
        return;
    }

    let available = (ui.available_width() - 4.0).max(320.0);
    let mut zoom = d.values.get("diagramZoom").and_then(Json::as_f64).unwrap_or(1.0) as f32;
    if d.values.get("diagramFit").and_then(Json::as_bool).unwrap_or(false) {
        let count = tables.len().max(1) as f32;
        let gap = 24.0;
        let base = 210.0;
        zoom = ((available - gap * (count - 1.0)) / (count * base)).clamp(0.55, 1.25);
        d.values.insert("diagramZoom".into(), json!(zoom));
        d.values.insert("diagramFit".into(), json!(false));
    }
    zoom = zoom.clamp(0.55, 1.25);
    const MAX_COLUMNS: usize = 12;
    let card_w = 210.0 * zoom;
    let header_h = 24.0 * zoom;
    let row_h = 18.0 * zoom;
    let pad = 6.0 * zoom;
    let gap = 24.0;
    let max_columns =
        tables.iter().map(|table| table.get("columns").and_then(Json::as_array).map_or(0, Vec::len).min(MAX_COLUMNS)).max().unwrap_or(0);
    let has_more = tables.iter().any(|table| table.get("columns").and_then(Json::as_array).map_or(0, Vec::len) > MAX_COLUMNS);
    let lines = max_columns + usize::from(has_more);
    let card_h = header_h + lines as f32 * row_h + pad * 2.0;
    let per_row = (((available + gap) / (card_w + gap)).floor() as usize).max(1);
    let row_count = tables.len().div_ceil(per_row).max(1);
    let used_cols = tables.len().min(per_row).max(1);
    let used_w = used_cols as f32 * card_w + used_cols.saturating_sub(1) as f32 * gap;
    let total_w = available.max(used_w);
    let total_h = row_count as f32 * card_h + row_count.saturating_sub(1) as f32 * gap;
    egui::ScrollArea::both().id_salt("pp_diagram").max_height(480.0).auto_shrink([false, false]).show(ui, |ui| {
        let (canvas, _) = ui.allocate_exact_size(vec2(total_w, total_h.max(80.0)), Sense::hover());
        let cards: Vec<(String, Rect)> = tables
            .iter()
            .enumerate()
            .map(|(index, table)| {
                let row = index / per_row;
                let column = index % per_row;
                let min = canvas.min + vec2(column as f32 * (card_w + gap), row as f32 * (card_h + gap));
                (table.get("name").and_then(Json::as_str).unwrap_or("Table").to_string(), Rect::from_min_size(min, vec2(card_w, card_h)))
            })
            .collect();
        {
            let painter = ui.painter();
            for relationship in &relationships {
                let from = relationship.get("from_table").and_then(Json::as_str).unwrap_or("");
                let to = relationship.get("to_table").and_then(Json::as_str).unwrap_or("");
                let Some(from_card) = cards.iter().find(|(name, _)| name == from) else { continue };
                let Some(to_card) = cards.iter().find(|(name, _)| name == to) else { continue };
                let start = pos2(from_card.1.right(), from_card.1.center().y);
                let end = pos2(to_card.1.left(), to_card.1.center().y);
                let mid = (start.x + end.x) / 2.0;
                painter.add(egui::Shape::line(vec![start, pos2(mid, start.y), pos2(mid, end.y), end], Stroke::new(1.5, t.accent)));
                painter.circle_filled(start, 2.5, t.accent);
                painter.circle_filled(end, 2.5, t.accent);
            }
            for (index, table) in tables.iter().enumerate() {
                let card = cards[index].1;
                painter.rect_filled(card, 4.0, t.ribbon);
                painter.rect_stroke(card, 4.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
                let header = Rect::from_min_size(card.min, vec2(card.width(), header_h));
                painter.rect_filled(header, 4.0, t.header_bg);
                let shown = table.get("columns").and_then(Json::as_array).map_or(0, Vec::len).min(MAX_COLUMNS);
                let extra = table.get("columns").and_then(Json::as_array).map_or(0, Vec::len).saturating_sub(MAX_COLUMNS);
                if extra > 0 {
                    let y = header.bottom() + shown as f32 * row_h;
                    let row_rect = Rect::from_min_size(pos2(card.left() + 6.0, y), vec2(card.width() - 12.0, row_h));
                    painter.with_clip_rect(row_rect).text(
                        row_rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        format!("+{extra} more columns"),
                        theme::ui_font((10.0 * zoom).max(7.0)),
                        t.text_dim,
                    );
                }
            }
        }
        for (index, table) in tables.iter().enumerate() {
            let card = cards[index].1;
            let header = Rect::from_min_size(card.min, vec2(card.width(), header_h));
            let name = table.get("name").and_then(Json::as_str).unwrap_or("Table");
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(header.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(name).font(theme::ui_bold((12.0 * zoom).max(8.0))).color(t.header_text))
                            .truncate()
                            .selectable(false),
                    );
                },
            );
            for (row, column) in table.get("columns").and_then(Json::as_array).into_iter().flatten().take(MAX_COLUMNS).enumerate() {
                let column = column.get("name").and_then(Json::as_str).unwrap_or("");
                let y = header.bottom() + row as f32 * row_h;
                let row_rect = Rect::from_min_size(pos2(card.left() + 6.0, y), vec2(card.width() - 12.0, row_h));
                ui.scope_builder(egui::UiBuilder::new().max_rect(row_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(column).font(theme::ui_font((11.0 * zoom).max(7.0))).color(t.text))
                            .truncate()
                            .selectable(false),
                    );
                });
            }
        }
    });
    if relationships.is_empty() {
        ui.label(egui::RichText::new("This model exposes no relationships; showing table cards only.").small().color(t.text_dim));
    }
    let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
    if !error.is_empty() {
        ui.label(egui::RichText::new(error).color(t.danger).small());
    }
}

/// A model value as the grid shows it: blank for null, Excel-style booleans, no trailing `.0`.
fn json_scalar_text(value: &Json) -> String {
    match value {
        Json::Null => String::new(),
        Json::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Json::String(s) => s.clone(),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 { format!("{f:.0}") } else { f.to_string() }
            } else {
                n.to_string()
            }
        }
        other => other.to_string(),
    }
}
