//! Excel's Power Pivot window: the File/Home/Design/Advanced tab strip, the Data View or Diagram
//! View canvas, and the floating dialogs Excel opens from its ribbon. The model lives in the
//! engine session, so the window reads it back through `dataModel.status`.

use egui::{Color32, Key, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::theme;
use serde_json::{Map, Value as Json, json};

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

fn power_pivot_count(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 { format!("{count} {singular}") } else { format!("{count} {plural}") }
}

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

/// Inserts a filled template into the formula bar (or the Add Measure dialog's formula field).
fn power_pivot_insert_function(d: &mut Dialog, template: &str) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let formula = power_pivot_fill_template(&model, template);
    if d.values.get("overlay").and_then(Json::as_str) == Some("addMeasure") {
        d.values.insert("newMeasureFormula".into(), json!(formula));
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
        .id(egui::Id::unique("power_pivot_functions"))
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

/// The Power Pivot window, following Excel's Power Pivot layout: the File/Home/Design/Advanced
/// tab strip, the ribbon, and either the Data View grid or the Diagram View canvas. Model
/// commands run through the `dataModel.*` engine commands, so agents and the control channel see
/// the same model the dialog edits.
pub(super) fn power_pivot(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    // `ui.dialog.set {field: "tab"}` writes the generic dialog tab; mirror it into this window's
    // own tab value so agents and tests drive the same strip the user clicks.
    if !d.tab.is_empty() && matches!(d.tab.as_str(), "File" | "Home" | "Design" | "Advanced") {
        d.values.insert("tab".into(), json!(d.tab.clone()));
    }
    let t = theme::Tokens::get(ui.ctx());
    power_pivot_header(ui, d, &t);
    ui.add_space(3.0);
    power_pivot_tabs(ui, d, &t);
    let tab = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    if tab == "File" {
        power_pivot_file_menu(app, ui, d, &t, open);
    } else {
        power_pivot_ribbon(app, ui, d, &t);
        if d.values.get("showFunctions").and_then(Json::as_bool).unwrap_or(false) {
            let ctx = ui.ctx().clone();
            power_pivot_functions_panel(&ctx, d);
        }
        ui.add_space(4.0);
        if d.values.get("view").and_then(Json::as_str).unwrap_or("Data") == "Diagram" {
            power_pivot_diagram(app, ui, d, &t);
        } else {
            power_pivot_data_view(app, ui, d, &t);
        }
        ui.add_space(3.0);
        power_pivot_footer(app, ui, d, &t);
    }
    let overlay = d.values.get("overlay").and_then(Json::as_str).unwrap_or("").to_string();
    if !overlay.is_empty() {
        let ctx = ui.ctx().clone();
        power_pivot_overlay(&ctx, app, d, &overlay);
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
            let summary = if loaded {
                format!("{} · {}", power_pivot_count(tables, "table", "tables"), power_pivot_count(measures, "measure", "measures"))
            } else {
                "not loaded".to_string()
            };
            ui.label(egui::RichText::new(summary).small().color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let view = d.values.get("view").and_then(Json::as_str).unwrap_or("Data");
                let label = if loaded { format!("{view} View") } else { "No model loaded".to_string() };
                ui.label(egui::RichText::new(label).small().color(t.text_disabled));
            });
        });
    });
}

/// Excel's in-window tab strip: File, Home, Design and Advanced.
fn power_pivot_tabs(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let current = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    let strip = egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 4, right: 4, top: 0, bottom: 0 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            let file_selected = current == "File";
            let file = ui.add(
                egui::Button::new(egui::RichText::new("File").font(theme::ui_font(12.5)).color(Color32::WHITE))
                    .fill(if file_selected { t.accent_dark } else { t.accent })
                    .min_size(vec2(46.0, 22.0)),
            );
            if file.clicked() {
                d.values.insert("tab".into(), json!("File"));
                d.tab = "File".into();
            }
            for tab in ["Home", "Design", "Advanced"] {
                let selected = current == tab;
                let response = ui.add(egui::Button::selectable(selected, egui::RichText::new(tab).font(theme::ui_font(12.5))));
                if response.clicked() {
                    d.values.insert("tab".into(), json!(tab));
                    d.tab = tab.into();
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
    let (rect, _) = ui.allocate_exact_size(vec2(5.0, 84.0), Sense::hover());
    ui.painter().line_segment([pos2(rect.center().x, rect.top() + 6.0), pos2(rect.center().x, rect.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

/// A big ribbon button with an accessibility label (the painted label is not a widget).
fn power_pivot_button(ui: &mut egui::Ui, icon: Icon, label: &str, tip: &str) -> egui::Response {
    let response = crate::widgets::big_button(ui, icon, label, tip, false);
    let accessible = label.replace('\n', " ");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &accessible));
    response
}

/// A disabled big ribbon action: Excel draws it greyed, with a tooltip that says why.
fn power_pivot_disabled_big(ui: &mut egui::Ui, label: &str, tip: &str, width: f32) -> egui::Response {
    let response = ui.add_enabled(false, egui::Button::new(egui::RichText::new(label).font(theme::ui_font(11.0))).min_size(vec2(width, 60.0)));
    response.on_disabled_hover_text(tip)
}

/// A disabled small ribbon action (Excel's stacked commands).
fn power_pivot_disabled_small(ui: &mut egui::Ui, label: &str, tip: &str) -> egui::Response {
    let response = ui.add_enabled(false, egui::Button::new(egui::RichText::new(label).small()).min_size(vec2(84.0, 18.0)));
    response.on_disabled_hover_text(tip)
}

/// An enabled small ribbon action.
fn power_pivot_small(ui: &mut egui::Ui, label: &str, tip: &str) -> egui::Response {
    let response = ui.add(egui::Button::new(egui::RichText::new(label).small()).min_size(vec2(84.0, 18.0)));
    response.on_hover_text(tip)
}

/// A disabled formatting icon ($, %, comma, decimals) in a compact button-sized slot.
fn power_pivot_disabled_icon(ui: &mut egui::Ui, icon: Icon, tip: &str) {
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 19.0), Sense::hover());
    let t = theme::Tokens::get(ui.ctx());
    icons::paint(ui.painter(), rect.shrink(3.0), icon, t.text_disabled);
    response.on_hover_text(tip);
}

/// A big button that paints like a toggle when it is on (Excel's View group).
fn power_pivot_toggle(ui: &mut egui::Ui, t: &theme::Tokens, icon: Icon, label: &str, on: bool, tip: &str) -> egui::Response {
    let response = power_pivot_button(ui, icon, label, tip);
    if on {
        ui.painter().rect_stroke(response.rect.shrink(2.0), 4.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    response
}

/// A compact stacked toggle, used by View/Calculations so the full Home ribbon fits.
fn power_pivot_small_toggle(ui: &mut egui::Ui, label: &str, on: bool, tip: &str) -> egui::Response {
    let response = ui.add(egui::Button::selectable(on, egui::RichText::new(label).small()).min_size(vec2(96.0, 18.0)));
    response.on_hover_text(tip)
}

/// The tab-specific ribbon, grouped like Excel's Power Pivot window. Excel's commands that this
/// model engine cannot run are present but disabled, as Excel disables them without a selection.
fn power_pivot_ribbon(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let tab = d.values.get("tab").and_then(Json::as_str).unwrap_or("Home").to_string();
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
        egui::ScrollArea::horizontal().id_salt("pp_ribbon").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal(|ui| match tab.as_str() {
                "Design" => power_pivot_ribbon_design(app, ui, d, t),
                "Advanced" => power_pivot_advanced_ribbon(ui, d, t),
                _ => power_pivot_home_ribbon(app, ui, d, t),
            });
        });
    });
}

/// Home: Clipboard, Get External Data, Refresh, PivotTable, Formatting, Sort and Filter, Find,
/// Calculations, View — Excel's order.
fn power_pivot_home_ribbon(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    power_pivot_group(ui, t, "Clipboard", |ui| {
        ui.horizontal(|ui| {
            power_pivot_disabled_big(ui, "Paste", "The model window has no clipboard surface; paste lives in the worksheet.", 54.0);
            ui.vertical(|ui| {
                power_pivot_disabled_small(ui, "Paste Append", "Appending rows is not available in the model window.");
                power_pivot_disabled_small(ui, "Paste Replace", "Replacing model rows is not available.");
                power_pivot_disabled_small(ui, "Copy", "Copying model rows is not available.");
            });
        });
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Get External Data", |ui| {
        power_pivot_disabled_big(ui, "From\nDatabase", "Importing without a model file is not available.", 82.0);
        power_pivot_disabled_big(ui, "From Data\nService", "Data services are not available in GridCraft.", 82.0);
        power_pivot_disabled_big(ui, "From Other\nSources", "Other sources are not available.", 82.0);
        if power_pivot_button(ui, Icon::Link, "Existing\nConnections", "Choose the model file the Data View reads").clicked() {
            d.values.insert("overlay".into(), json!("existingConnections"));
            d.values.insert("error".into(), json!(""));
        }
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Refresh", |ui| {
        if power_pivot_button(ui, Icon::Redo, "Refresh", "Re-read the model from the engine session").clicked() {
            power_pivot_refresh(app, d);
        }
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "PivotTable", |ui| {
        power_pivot_disabled_big(ui, "PivotTable", "Pivot tables live in the worksheet, not inside the Power Pivot window.", 70.0);
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Formatting", |ui| {
        ui.vertical(|ui| {
            let data_type = power_pivot_selected_column_type(d);
            power_pivot_disabled_small(ui, &format!("Data Type : {data_type}"), "Changing a model column's type is not supported.");
            let format = if power_pivot_selected_measure(d).is_some() { "Format : Measure".to_string() } else { format!("Format : {data_type}") };
            power_pivot_disabled_small(ui, &format, "Formatting model columns is not supported.");
            ui.horizontal(|ui| {
                power_pivot_disabled_icon(ui, Icon::Currency, "Currency format");
                power_pivot_disabled_icon(ui, Icon::Percent, "Percent format");
                power_pivot_disabled_icon(ui, Icon::Comma, "Comma format");
                power_pivot_disabled_icon(ui, Icon::DecInc, "Increase Decimal");
                power_pivot_disabled_icon(ui, Icon::DecDec, "Decrease Decimal");
            });
        });
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Sort and Filter", |ui| {
        ui.vertical(|ui| {
            power_pivot_disabled_small(ui, "Sort A to Z", "Sorting model rows is not supported.");
            power_pivot_disabled_small(ui, "Sort Z to A", "Sorting model rows is not supported.");
        });
        ui.vertical(|ui| {
            power_pivot_disabled_small(ui, "Clear Sort", "Clear the model's sort order.");
            power_pivot_disabled_small(ui, "Clear All\nFilters", "Clear the model's filters.");
            power_pivot_disabled_small(ui, "Sort by\nColumn", "Sort by a related column.");
        });
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Find", |ui| {
        power_pivot_disabled_big(ui, "Find", "Find across the model is not available.", 54.0);
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Calculations", |ui| {
        ui.vertical(|ui| {
            power_pivot_disabled_small(ui, "AutoSum", "AutoSum in the model window is not available.");
            power_pivot_disabled_small(ui, "Create KPI", "KPIs are not available for this model.");
        });
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "View", |ui| {
        let view = d.values.get("view").and_then(Json::as_str).unwrap_or("Data").to_string();
        ui.vertical(|ui| {
            if power_pivot_small_toggle(ui, "Data View", view == "Data", "Show the selected table's rows and the Calculation Area").clicked() {
                d.values.insert("view".into(), json!("Data"));
            }
            if power_pivot_small_toggle(ui, "Diagram View", view == "Diagram", "Show table cards and relationships").clicked() {
                d.values.insert("view".into(), json!("Diagram"));
            }
            let hidden = d.values.get("showHidden").and_then(Json::as_bool).unwrap_or(false);
            if power_pivot_small_toggle(ui, "Show Hidden", hidden, "Show hidden model columns").clicked() {
                d.values.insert("showHidden".into(), json!(!hidden));
            }
            let area = d.values.get("calcArea").and_then(Json::as_bool).unwrap_or(true);
            if power_pivot_small_toggle(ui, "Calculation Area", area, "Show the Calculation Area below the grid").clicked() {
                d.values.insert("calcArea".into(), json!(!area));
            }
        });
    });
}

/// Design: Columns, Calculations, Relationships, Table Properties, Mark as Date Table, Date
/// Table and Edit. The Data View / Diagram canvas stays visible below.
fn power_pivot_ribbon_design(_app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    power_pivot_group(ui, t, "Columns", |ui| {
        if power_pivot_button(ui, Icon::Plus, "Add", "Add a measure to the selected table").clicked() {
            power_pivot_open_measure_dialog(d, "Add");
        }
        power_pivot_disabled_big(ui, "Delete", "Deleting a model column is not supported.", 54.0);
        power_pivot_disabled_big(ui, "Freeze", "Freezing columns is a worksheet command.", 54.0);
        power_pivot_disabled_big(ui, "Width", "Column width is a worksheet command.", 54.0);
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Calculations", |ui| {
        if power_pivot_button(ui, Icon::Function, "Insert\nFunction", "Open the Function Library").clicked() {
            let shown = d.values.get("showFunctions").and_then(Json::as_bool).unwrap_or(false);
            d.values.insert("showFunctions".into(), json!(!shown));
        }
        if power_pivot_button(ui, Icon::Calc, "Calculation\nOptions", "Choose Automatic or Manual calculation").clicked() {
            d.values.insert("overlay".into(), json!("calculationOptions"));
        }
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Relationships", |ui| {
        if power_pivot_button(ui, Icon::Link, "Create\nRelationship", "Relate two tables on matching columns").clicked() {
            power_pivot_open_relationship_dialog(d);
        }
        if power_pivot_button(ui, Icon::Link, "Manage\nRelationships", "List, create and delete model relationships").clicked() {
            d.values.insert("overlay".into(), json!("manageRelationships"));
            d.values.insert("relIndex".into(), json!(-1));
        }
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Table Properties", |ui| {
        if power_pivot_button(ui, Icon::Table, "Table\nProperties", "Show the selected table's columns, types and row count").clicked() {
            d.values.insert("overlay".into(), json!("tableProperties"));
        }
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Mark as Date Table", |ui| {
        power_pivot_disabled_big(ui, "Mark as\nDate Table", "Date tables are not supported by this model engine.", 80.0);
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Date Table", |ui| {
        power_pivot_disabled_big(ui, "Date\nTable", "Date table settings are not supported.", 64.0);
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Edit", |ui| {
        power_pivot_disabled_small(ui, "Undo", "Model edits are applied immediately and are not undoable.");
        power_pivot_disabled_small(ui, "Redo", "Model edits are applied immediately and are not undoable.");
    });
}

/// Advanced: Perspectives, Show Implicit Measures, Summarize By and Reporting Properties.
fn power_pivot_advanced_ribbon(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    power_pivot_group(ui, t, "Perspectives", |ui| {
        power_pivot_disabled_big(ui, "Perspectives", "Perspectives are not supported by this model engine.", 84.0);
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Show Implicit Measures", |ui| {
        let shown = d.values.get("showImplicit").and_then(Json::as_bool).unwrap_or(false);
        if power_pivot_toggle(ui, t, Icon::Calc, "Show\nImplicit\nMeasures", shown, "Show measures the model engine derives from columns").clicked() {
            d.values.insert("showImplicit".into(), json!(!shown));
        }
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Summarize By", |ui| {
        power_pivot_disabled_big(ui, "Summarize\nBy", "Changing the default summarization is not supported.", 84.0);
    });
    power_pivot_separator(ui, t);
    power_pivot_group(ui, t, "Reporting Properties", |ui| {
        power_pivot_disabled_big(ui, "Table Detail", "Table detail settings are not supported.", 76.0);
        power_pivot_disabled_big(ui, "Table\nBehavior", "Table behavior settings are not supported.", 76.0);
        power_pivot_disabled_big(ui, "Image URL", "Image URL settings are not supported.", 76.0);
    });
}

/// File tab: Excel's model-window backstage, with the model path moved out of the data canvas.
fn power_pivot_file_menu(_app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens, open: &mut bool) {
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                if ui.add(egui::Button::new("Open Model…").min_size(vec2(230.0, 26.0))).clicked() {
                    d.values.insert("overlay".into(), json!("openModel"));
                    d.values.insert("error".into(), json!(""));
                }
                if ui.add(egui::Button::new("Existing Connections").min_size(vec2(230.0, 26.0))).clicked() {
                    d.values.insert("overlay".into(), json!("existingConnections"));
                    d.values.insert("error".into(), json!(""));
                }
                ui.separator();
                if ui.add(egui::Button::new("Switch to Advanced Mode").min_size(vec2(230.0, 26.0))).clicked() {
                    d.values.insert("tab".into(), json!("Advanced"));
                    d.tab = "Advanced".into();
                }
                ui.separator();
                if ui.add(egui::Button::new("Close").min_size(vec2(230.0, 26.0))).clicked() {
                    *open = false;
                }
                if ui.add(egui::Button::new("Exit").min_size(vec2(230.0, 26.0))).clicked() {
                    *open = false;
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let model = d.values.get("model").cloned().unwrap_or(Json::Null);
                let loaded = model.get("loaded").and_then(Json::as_bool).unwrap_or(false);
                let tables = model.get("tables").and_then(Json::as_array).map_or(0, Vec::len);
                let measures = model.get("measures").and_then(Json::as_array).map_or(0, Vec::len);
                let relationships = model.get("relationships").and_then(Json::as_array).map_or(0, Vec::len);
                ui.label(egui::RichText::new("Power Pivot Data Model").font(theme::ui_bold(14.0)).color(t.text));
                ui.add_space(2.0);
                let summary = if loaded {
                    format!(
                        "{} · {} · {}",
                        power_pivot_count(tables, "table", "tables"),
                        power_pivot_count(measures, "measure", "measures"),
                        power_pivot_count(relationships, "relationship", "relationships")
                    )
                } else {
                    "No model is loaded.".to_string()
                };
                ui.label(egui::RichText::new(summary).color(t.text_dim));
                ui.add_space(6.0);
                let path = d.values.get("path").and_then(Json::as_str).unwrap_or("");
                let picked = d.values.get("modelName").and_then(Json::as_str).unwrap_or("");
                let source = if !picked.is_empty() {
                    picked.to_string()
                } else if path.trim().is_empty() {
                    "(no model file chosen)".to_string()
                } else {
                    path.to_string()
                };
                ui.label(egui::RichText::new("Model file").small().color(t.text_dim));
                ui.monospace(source);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(
                        "Open Model reads a JSON data model through dataModel.load. The model is session state: measure and relationship edits do not change the workbook.",
                    )
                    .small()
                    .color(t.text_dim),
                );
            });
        });
    });
    let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
    if !error.is_empty() {
        ui.label(egui::RichText::new(error).color(t.danger).small());
    }
}

/// Excel's Data View: the formula bar, the selected table's grid, then the Calculation Area.
fn power_pivot_data_view(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    power_pivot_formula(app, ui, d, t);
    ui.add_space(3.0);
    power_pivot_data_grid(ui, d, t);
    if d.values.get("calcArea").and_then(Json::as_bool).unwrap_or(true) {
        ui.add_space(4.0);
        power_pivot_calculations(app, ui, d, t);
    }
    let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
    if !error.is_empty() {
        ui.add_space(2.0);
        ui.label(egui::RichText::new(error).color(t.danger).small());
    }
}

/// Excel's Data View grid: a row-number gutter, column headers with filter arrows, the
/// green "Add Column" header, striped rows and both scroll directions. At most 200 rows display.
fn power_pivot_data_grid(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    const MAX_ROWS: usize = 200;
    let data = d.values.get("tableData").cloned().unwrap_or(Json::Null);
    let columns: Vec<String> = data
        .get("columns")
        .and_then(Json::as_array)
        .map(|columns| columns.iter().filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let rows = data.get("rows").and_then(Json::as_array).cloned().unwrap_or_default();
    let selected = d
        .values
        .get("columnIndex")
        .and_then(Json::as_i64)
        .filter(|index| *index >= 0)
        .map(|index| index as usize)
        .filter(|index| *index < columns.len());
    let filter_column = d.values.get("filterColumn").and_then(Json::as_str).unwrap_or("").to_string();
    let filter_value = d.values.get("filterValue").and_then(Json::as_str).unwrap_or("").to_string();
    let filter_index = columns.iter().position(|column| column == &filter_column);
    let visible: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| match filter_index {
            Some(column) if !filter_column.is_empty() => row.get(column).map(|cell| json_scalar_text(cell) == filter_value).unwrap_or(false),
            _ => true,
        })
        .map(|(index, _)| index)
        .collect();
    let mut select_column = None;
    let mut choose_filter: Option<(usize, Option<String>)> = None;
    let mut add_column = false;
    egui::Frame::NONE.fill(t.grid_bg).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(2.0).inner_margin(0).show(ui, |ui| {
        ui.set_min_height(260.0);
        ui.set_min_width(ui.available_width());
        if columns.is_empty() {
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Load a model and choose a table tab to see its rows.").italics().color(t.text_dim));
            return;
        }
        egui::ScrollArea::both().id_salt("pp_data_grid").max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("pp_data_grid_rows").striped(true).spacing(vec2(0.0, 0.0)).show(ui, |ui| {
                egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                    ui.add_sized([30.0, 18.0], egui::Label::new(""));
                });
                for (index, name) in columns.iter().enumerate() {
                    let active = selected == Some(index);
                    egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let text =
                                egui::RichText::new(name).font(theme::ui_bold(12.0)).color(if active { t.header_sel_text } else { t.header_text });
                            if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                select_column = Some(index);
                            }
                            let arrow = if filter_column == *name { "▼" } else { "▾" };
                            let mut filter_opened = false;
                            ui.push_id(("pp_filter", index), |ui| {
                                filter_opened = ui
                                    .menu_button(arrow, |ui| {
                                        ui.set_min_width(160.0);
                                        if ui.button("All").clicked() {
                                            choose_filter = Some((index, None));
                                        }
                                        if ui.button("Clear Filter").clicked() {
                                            choose_filter = Some((index, Some(String::new())));
                                        }
                                        ui.separator();
                                        for value in power_pivot_distinct_values(&rows, index).into_iter().take(50) {
                                            let chosen = filter_column == *name && filter_value == value;
                                            if ui.selectable_label(chosen, &value).clicked() {
                                                choose_filter = Some((index, Some(value)));
                                            }
                                        }
                                    })
                                    .response
                                    .clicked();
                            });
                            if filter_opened {
                                select_column = Some(index);
                            }
                        });
                    });
                }
                egui::Frame::NONE.fill(t.accent_soft).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    let text = egui::RichText::new("Add Column").italics().color(t.accent);
                    if ui.add(egui::Button::new(text).frame(false)).on_hover_text("Add a measure to this table").clicked() {
                        add_column = true;
                    }
                });
                ui.end_row();
                for row_index in visible.iter().take(MAX_ROWS) {
                    let row = &rows[*row_index];
                    let gutter = egui::RichText::new(format!("{}", row_index + 1)).small().color(t.text_dim);
                    egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 1)).show(ui, |ui| {
                        ui.add_sized([30.0, 18.0], egui::Label::new(gutter));
                    });
                    for cell in row.as_array().into_iter().flatten() {
                        ui.add(egui::Label::new(egui::RichText::new(json_scalar_text(cell)).color(t.cell_text)).selectable(false));
                    }
                    ui.label("");
                    ui.end_row();
                }
            });
            ui.add_space(2.0);
            if visible.len() > MAX_ROWS {
                ui.label(egui::RichText::new(format!("Showing the first {MAX_ROWS} of {} rows.", visible.len())).small().color(t.text_dim));
            }
        });
    });
    if let Some(index) = select_column {
        let name = columns[index].clone();
        d.values.insert("columnIndex".into(), json!(index));
        d.values.insert("column".into(), json!(name));
        d.values.insert("measureIndex".into(), json!(-1));
        d.values.insert("error".into(), json!(""));
    }
    if let Some((index, value)) = choose_filter {
        let name = columns[index].clone();
        match value {
            None => {
                d.values.insert("filterColumn".into(), json!(""));
                d.values.insert("filterValue".into(), json!(""));
            }
            Some(value) if value.is_empty() => {
                d.values.insert("filterColumn".into(), json!(""));
                d.values.insert("filterValue".into(), json!(""));
            }
            Some(value) => {
                d.values.insert("filterColumn".into(), json!(name));
                d.values.insert("filterValue".into(), json!(value));
            }
        }
    }
    if add_column {
        power_pivot_open_measure_dialog(d, "Add");
    }
}

/// Distinct values in one column, for the header filter menu.
fn power_pivot_distinct_values(rows: &[Json], column: usize) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        if let Some(cell) = row.get(column) {
            seen.insert(json_scalar_text(cell));
        }
    }
    seen.into_iter().collect()
}

/// Excel's Calculation Area: measures as a Name / Formula / Format grid, with Add, Update and
/// Delete wired to `dataModel.addMeasure`, `dataModel.updateMeasure` and
/// `dataModel.deleteMeasure`.
fn power_pivot_calculations(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let measures = model.get("measures").and_then(Json::as_array).cloned().unwrap_or_default();
    let selected = d.values.get("measureIndex").and_then(Json::as_i64).unwrap_or(-1);
    let mut select = None;
    let mut add = false;
    let mut update = false;
    let mut delete = false;
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(6).show(ui, |ui| {
        ui.set_min_height(72.0);
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Calculation Area").strong().color(t.text));
            ui.label(egui::RichText::new(power_pivot_count(measures.len(), "measure", "measures")).small().color(t.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if accent_button(ui, "Add Measure").clicked() {
                    add = true;
                }
                let has_selection = selected >= 0 && (selected as usize) < measures.len();
                if ui.add_enabled(has_selection, egui::Button::new("Update")).clicked() {
                    update = true;
                }
                if ui.add_enabled(has_selection, egui::Button::new("Delete")).clicked() {
                    delete = true;
                }
            });
        });
        ui.add_space(2.0);
        if measures.is_empty() {
            ui.label(egui::RichText::new("No measures in this model. Add Measure starts one.").italics().color(t.text_dim));
            return;
        }
        egui::ScrollArea::vertical().id_salt("pp_calculations").max_height(108.0).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("pp_calculations_grid").striped(true).spacing(vec2(14.0, 3.0)).show(ui, |ui| {
                for heading in ["Name", "Formula", "Format"] {
                    egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                        ui.label(egui::RichText::new(heading).font(theme::ui_bold(12.0)).color(t.header_text));
                    });
                }
                ui.end_row();
                for (index, measure) in measures.iter().enumerate() {
                    let name = measure.get("name").and_then(Json::as_str).unwrap_or("");
                    let formula = measure.get("formula").and_then(Json::as_str).unwrap_or("");
                    let format = measure.get("format").and_then(Json::as_str).unwrap_or("");
                    let chosen = selected == index as i64;
                    if ui.add(egui::Button::selectable(chosen, egui::RichText::new(name))).clicked() {
                        select = Some(index);
                    }
                    ui.label(egui::RichText::new(formula).color(t.text));
                    ui.label(egui::RichText::new(if format.is_empty() { "General" } else { format }).color(t.text_dim));
                    ui.end_row();
                }
            });
        });
    });
    if let Some(index) = select
        && let Some(measure) = measures.get(index)
    {
        let name = measure.get("name").and_then(Json::as_str).unwrap_or("");
        d.values.insert("measureIndex".into(), json!(index));
        d.values.insert("formula".into(), json!(format!("[{name}]")));
        d.values.insert("column".into(), json!(""));
        d.values.insert("columnIndex".into(), json!(-1));
        d.values.insert("resultKind".into(), json!(""));
        d.values.insert("resultText".into(), json!(""));
        d.values.insert("error".into(), json!(""));
    }
    if add {
        power_pivot_open_measure_dialog(d, "Add");
    }
    if update {
        if selected >= 0 && (selected as usize) < measures.len() {
            power_pivot_open_measure_dialog(d, "Update");
        } else {
            d.values.insert("error".into(), json!("Select a measure in the Calculation Area first."));
        }
    }
    if delete && selected >= 0 && (selected as usize) < measures.len() {
        let name = measures[selected as usize].get("name").and_then(Json::as_str).unwrap_or("").to_string();
        match app.session.run("dataModel.deleteMeasure", json!({"name": name})) {
            Ok(status) => {
                d.values.insert("model".into(), status);
                d.values.insert("error".into(), json!(""));
                d.values.insert("formula".into(), json!(""));
                d.values.insert("measureIndex".into(), json!(-1));
                d.values.insert("resultKind".into(), json!(""));
                d.values.insert("resultText".into(), json!(""));
                d.result = None;
            }
            Err(e) => {
                d.values.insert("error".into(), json!(crate::clean_error(&e)));
            }
        }
    }
}

/// The table tabs and the bottom-right Data View / Diagram View control, as Excel has both.
fn power_pivot_footer(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let tables = model.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    let selected = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
    let view = d.values.get("view").and_then(Json::as_str).unwrap_or("Data").to_string();
    let mut choose = None;
    egui::Frame::NONE.fill(t.tab_bar).stroke(Stroke::new(1.0, t.ribbon_border)).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.horizontal(|ui| {
            if tables.is_empty() {
                ui.label(egui::RichText::new("No table tabs").small().color(t.text_dim));
            }
            let tabs_width = (ui.available_width() - 200.0).max(100.0);
            ui.allocate_ui_with_layout(vec2(tabs_width, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                egui::ScrollArea::horizontal().id_salt("pp_table_tabs").auto_shrink([false, true]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (index, table) in tables.iter().enumerate() {
                            let name = table.get("name").and_then(Json::as_str).unwrap_or("Table");
                            if ui.add(egui::Button::selectable(index == selected, egui::RichText::new(name).font(theme::ui_font(12.0)))).clicked() {
                                choose = Some(index);
                            }
                        }
                    });
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::selectable(view == "Diagram", egui::RichText::new("Diagram View").small())).clicked() {
                    d.values.insert("view".into(), json!("Diagram"));
                }
                if ui.add(egui::Button::selectable(view == "Data", egui::RichText::new("Data View").small())).clicked() {
                    d.values.insert("view".into(), json!("Data"));
                }
            });
        });
    });
    if let Some(index) = choose {
        power_pivot_select_table(app, d, index);
    }
}

fn power_pivot_load(app: &mut SheetApp, d: &mut Dialog) {
    // A browser picker hands back the file's bytes as JSON text; native hosts pass a filesystem
    // path. Pasted JSON is accepted too, so a browser without a picker still works.
    let picked = d.values.get("modelJson").and_then(Json::as_str).map(str::trim).filter(|json| !json.is_empty()).map(str::to_string);
    let path = d.values.get("path").and_then(Json::as_str).unwrap_or("").trim().to_string();
    let params = if let Some(json) = picked {
        json!({"json": json})
    } else if path.is_empty() {
        d.values.insert("error".into(), json!("Choose a model JSON file, enter its path, or paste the model JSON."));
        return;
    } else if path.starts_with('{') || path.starts_with('[') {
        json!({"json": path})
    } else {
        json!({"path": path})
    };
    match app.session.run("dataModel.load", params) {
        Ok(model) => {
            d.values.insert("model".into(), model);
            d.values.insert("error".into(), json!(""));
            d.values.remove("tableData");
            d.values.insert("tableIndex".into(), json!(0));
            d.values.insert("column".into(), json!(""));
            d.values.insert("columnIndex".into(), json!(0));
            d.values.insert("measureIndex".into(), json!(-1));
            d.values.insert("formula".into(), json!(""));
            d.values.insert("filterColumn".into(), json!(""));
            d.values.insert("filterValue".into(), json!(""));
            d.values.insert("resultKind".into(), json!(""));
            d.values.insert("resultText".into(), json!(""));
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
            d.values.insert("model".into(), model.clone());
            d.values.insert("error".into(), json!(""));
            let measures = model.get("measures").and_then(Json::as_array).map_or(0, Vec::len) as i64;
            let selected = d.values.get("measureIndex").and_then(Json::as_i64).unwrap_or(-1);
            if selected >= measures {
                d.values.insert("measureIndex".into(), json!(-1));
                d.values.insert("formula".into(), json!(""));
                d.values.insert("resultKind".into(), json!(""));
                d.values.insert("resultText".into(), json!(""));
            }
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
    let previous = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
    let changed = previous != index;
    d.values.insert("tableIndex".into(), json!(index));
    if changed {
        d.values.insert("filterColumn".into(), json!(""));
        d.values.insert("filterValue".into(), json!(""));
        d.values.insert("formula".into(), json!(""));
        d.values.insert("measureIndex".into(), json!(-1));
        d.values.insert("resultKind".into(), json!(""));
        d.values.insert("resultText".into(), json!(""));
        d.result = None;
    }
    let columns: Vec<String> = table
        .get("columns")
        .and_then(Json::as_array)
        .map(|columns| columns.iter().filter_map(|column| column.get("name")).filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let current = d.values.get("column").and_then(Json::as_str).unwrap_or("").to_string();
    let position = if changed { None } else { columns.iter().position(|column| column == &current) };
    let column_index = position.unwrap_or(0).min(columns.len().saturating_sub(1));
    d.values.insert("columnIndex".into(), json!(column_index));
    d.values.insert("column".into(), json!(columns.get(column_index).cloned().unwrap_or_default()));
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

/// Runs `dataModel.evaluate` for the formula bar. A bare `[Name]` evaluates that measure, which is
/// how the name box and the Calculation Area hand a measure to the bar.
fn power_pivot_evaluate(app: &mut SheetApp, d: &mut Dialog) {
    let raw = d.values.get("formula").and_then(Json::as_str).unwrap_or("").trim().to_string();
    let raw = raw.strip_prefix('=').unwrap_or(&raw).trim().to_string();
    let selected = d.values.get("measureIndex").and_then(Json::as_i64).unwrap_or(-1);
    let selected_name =
        power_pivot_selected_measure(d).and_then(|measure| measure.get("name").and_then(Json::as_str).map(str::to_string)).unwrap_or_default();
    let bare_measure =
        raw.strip_prefix('[').and_then(|inner| inner.strip_suffix(']')).filter(|inner| !inner.is_empty() && !inner.contains(']')).map(str::to_string);
    let (formula, measure) = if !raw.is_empty() {
        (if bare_measure.is_some() { String::new() } else { raw.clone() }, bare_measure.unwrap_or_default())
    } else if selected >= 0 {
        (String::new(), selected_name)
    } else {
        (String::new(), String::new())
    };
    let outcome = if !formula.is_empty() {
        app.session.run("dataModel.evaluate", json!({"formula": formula}))
    } else if !measure.is_empty() {
        app.session.run("dataModel.evaluate", json!({"measure": measure}))
    } else {
        Err("Enter a DAX formula or select a measure.".to_string())
    };
    match outcome {
        Ok(result) => {
            let kind = if formula.is_empty() { "Measure" } else { "Scalar" };
            let text = result.get("value").map(json_scalar_text).unwrap_or_default();
            d.values.insert("error".into(), json!(""));
            d.values.insert("resultKind".into(), json!(kind));
            d.values.insert("resultText".into(), json!(text));
            d.result = Some(result);
        }
        Err(e) => {
            d.values.insert("error".into(), json!(crate::clean_error(&e)));
            d.values.insert("resultKind".into(), json!(""));
            d.values.insert("resultText".into(), json!(""));
            d.result = None;
        }
    }
}

/// The measure selected in the Calculation Area, cloned from the cached status.
fn power_pivot_selected_measure(d: &Dialog) -> Option<Json> {
    let model = d.values.get("model")?;
    let measures = model.get("measures")?.as_array()?;
    let index = d.values.get("measureIndex").and_then(Json::as_i64).unwrap_or(-1);
    if index < 0 {
        return None;
    }
    measures.get(index as usize).cloned()
}

/// The type the selected column reports in `dataModel.status`, for the ribbon's Data Type label.
fn power_pivot_selected_column_type(d: &Dialog) -> String {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let index = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
    let column = d.values.get("column").and_then(Json::as_str).unwrap_or("");
    model
        .get("tables")
        .and_then(Json::as_array)
        .and_then(|tables| tables.get(index))
        .and_then(|table| table.get("columns"))
        .and_then(Json::as_array)
        .and_then(|columns| columns.iter().find(|entry| entry.get("name").and_then(Json::as_str) == Some(column)))
        .and_then(|entry| entry.get("type"))
        .and_then(Json::as_str)
        .unwrap_or("Text")
        .to_string()
}

/// Opens the Add Measure / Update Measure dialog, prefilled for the selected table or measure.
fn power_pivot_open_measure_dialog(d: &mut Dialog, mode: &str) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let tables = model.get("tables").and_then(Json::as_array).cloned().unwrap_or_default();
    let table_index = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
    let table = tables.get(table_index).and_then(|table| table.get("name")).and_then(Json::as_str).unwrap_or("").to_string();
    if mode == "Update" {
        let Some(measure) = power_pivot_selected_measure(d) else {
            d.values.insert("error".into(), json!("Select a measure in the Calculation Area first."));
            return;
        };
        let name = measure.get("name").and_then(Json::as_str).unwrap_or("").to_string();
        let formula = measure.get("formula").and_then(Json::as_str).unwrap_or("").to_string();
        let format = measure.get("format").and_then(Json::as_str).unwrap_or("").to_string();
        let measure_table = measure.get("table").and_then(Json::as_str).unwrap_or("").to_string();
        d.values.insert("newMeasureName".into(), json!(name));
        d.values.insert("newMeasureFormula".into(), json!(formula));
        d.values.insert("newMeasureFormat".into(), json!(format));
        d.values.insert("newMeasureTable".into(), json!(if measure_table.is_empty() { table } else { measure_table }));
    } else {
        d.values.insert("newMeasureName".into(), json!(""));
        d.values.insert("newMeasureFormula".into(), json!(""));
        d.values.insert("newMeasureFormat".into(), json!(""));
        d.values.insert("newMeasureTable".into(), json!(table));
    }
    d.values.insert("measureDialogMode".into(), json!(mode));
    d.values.insert("overlay".into(), json!("addMeasure"));
    d.values.insert("error".into(), json!(""));
}

/// Opens the Create Relationship dialog with the first two tables (or the only table twice).
fn power_pivot_open_relationship_dialog(d: &mut Dialog) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let tables: Vec<String> = model
        .get("tables")
        .and_then(Json::as_array)
        .map(|tables| tables.iter().filter_map(|table| table.get("name")).filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let from = tables.first().cloned().unwrap_or_default();
    let to = tables.get(1).cloned().unwrap_or_else(|| from.clone());
    let from_column = power_pivot_column_names(&model, &from).into_iter().next().unwrap_or_default();
    let to_column = power_pivot_column_names(&model, &to).into_iter().next().unwrap_or_default();
    d.values.insert("relFromTable".into(), json!(from));
    d.values.insert("relFromColumn".into(), json!(from_column));
    d.values.insert("relToTable".into(), json!(to));
    d.values.insert("relToColumn".into(), json!(to_column));
    d.values.insert("relBidirectional".into(), json!(false));
    d.values.insert("overlay".into(), json!("createRelationship"));
    d.values.insert("error".into(), json!(""));
}

/// Column names of a model table, from `dataModel.status`.
fn power_pivot_column_names(model: &Json, table: &str) -> Vec<String> {
    model
        .get("tables")
        .and_then(Json::as_array)
        .and_then(|tables| tables.iter().find(|entry| entry.get("name").and_then(Json::as_str) == Some(table)))
        .and_then(|entry| entry.get("columns"))
        .and_then(Json::as_array)
        .map(|columns| columns.iter().filter_map(|column| column.get("name")).filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// The formula bar above the data grid: name box, cancel (X), enter (✓), fx, the DAX expression,
/// and the result of the last Evaluate.
fn power_pivot_formula(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let table_index = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
    let columns: Vec<String> = model
        .get("tables")
        .and_then(Json::as_array)
        .and_then(|tables| tables.get(table_index))
        .and_then(|table| table.get("columns"))
        .and_then(Json::as_array)
        .map(|columns| columns.iter().filter_map(|column| column.get("name")).filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let selected_name = d.values.get("column").and_then(Json::as_str).unwrap_or("").to_string();
    let selected_index = columns.iter().position(|column| column == &selected_name);
    let measure_name =
        power_pivot_selected_measure(d).and_then(|measure| measure.get("name").and_then(Json::as_str).map(str::to_string)).unwrap_or_default();
    let display = if !selected_name.is_empty() {
        format!("[{selected_name}]")
    } else if !measure_name.is_empty() {
        format!("[{measure_name}]")
    } else {
        "[Select column]".to_string()
    };
    let mut formula = d.values.get("formula").and_then(Json::as_str).unwrap_or("").to_string();
    let mut evaluate = false;
    let mut clear = false;
    let mut chosen_column = None;
    let mut functions = false;
    egui::Frame::NONE.fill(t.input_bg).stroke(Stroke::new(1.0, t.input_border)).corner_radius(2.0).inner_margin(egui::Margin::symmetric(4, 2)).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("pp_name_box").width(180.0).selected_text(egui::RichText::new(display.clone()).color(t.text)).show_ui(
                    ui,
                    |ui| {
                        if columns.is_empty() {
                            ui.label(egui::RichText::new("No columns").italics().color(t.text_dim));
                        }
                        for (index, name) in columns.iter().enumerate() {
                            let chosen = selected_index == Some(index);
                            if ui.selectable_label(chosen, format!("[{name}]")).clicked() {
                                chosen_column = Some(index);
                            }
                        }
                    },
                );
                if crate::widgets::icon_button(ui, Icon::Close, t.text_dim, "Cancel (X): clear the formula", vec2(22.0, 20.0)).clicked() {
                    clear = true;
                }
                if crate::widgets::icon_button(ui, Icon::Check, t.accent, "Enter (✓): evaluate the formula", vec2(22.0, 20.0)).clicked() {
                    evaluate = true;
                }
                if crate::widgets::icon_button(ui, Icon::Function, t.text_dim, "Insert Function (fx)", vec2(22.0, 20.0)).clicked() {
                    functions = true;
                }
                let width = ui.available_width();
                let response =
                    ui.add_sized([width, 22.0], egui::TextEdit::singleline(&mut formula).frame(egui::Frame::NONE).hint_text("SUM(Sales[Amount])"));
                if response.changed() {
                    d.values.insert("formula".into(), json!(formula));
                }
                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    evaluate = true;
                }
            });
        },
    );
    if clear {
        formula.clear();
        d.values.insert("formula".into(), json!(""));
        d.values.insert("error".into(), json!(""));
        d.values.insert("resultKind".into(), json!(""));
        d.values.insert("resultText".into(), json!(""));
        d.result = None;
    }
    if let Some(index) = chosen_column {
        let name = columns[index].clone();
        d.values.insert("columnIndex".into(), json!(index));
        d.values.insert("column".into(), json!(name));
        d.values.insert("measureIndex".into(), json!(-1));
        d.values.insert("formula".into(), json!(""));
        d.values.insert("resultKind".into(), json!(""));
        d.values.insert("resultText".into(), json!(""));
        d.values.insert("error".into(), json!(""));
        d.result = None;
    }
    if functions {
        let shown = d.values.get("showFunctions").and_then(Json::as_bool).unwrap_or(false);
        d.values.insert("showFunctions".into(), json!(!shown));
    }
    if evaluate {
        power_pivot_evaluate(app, d);
    }
    let kind = d.values.get("resultKind").and_then(Json::as_str).unwrap_or("").to_string();
    let text = d.values.get("resultText").and_then(Json::as_str).unwrap_or("").to_string();
    if !kind.is_empty() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Result").small().color(t.text_dim));
            ui.label(egui::RichText::new(format!("{kind}: {text}")).font(theme::ui_bold(12.0)).color(t.text));
        });
    }
}

/// Diagram View: Excel's table cards, with relationship lines when the model exposes relationships.
fn power_pivot_diagram(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens) {
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if accent_button(ui, "Data View").clicked() {
                d.values.insert("view".into(), json!("Data"));
            }
            if power_pivot_small(ui, "Refresh", "Re-read the model from the engine session").clicked() {
                power_pivot_refresh(app, d);
            }
            if power_pivot_small(ui, "Fit", "Fit the table cards to the window").clicked() {
                d.values.insert("diagramFit".into(), json!(true));
            }
        });
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

/// One floating dialog for the ribbon commands Excel opens in separate windows.
fn power_pivot_overlay(ctx: &egui::Context, app: &mut SheetApp, d: &mut Dialog, overlay: &str) {
    let t = theme::Tokens::get(ctx);
    let mode = d.values.get("measureDialogMode").and_then(Json::as_str).unwrap_or("Add");
    let title = match overlay {
        "openModel" => "Open Model",
        "existingConnections" => "Existing Connections",
        "manageRelationships" => "Manage Relationships",
        "createRelationship" => "Create Relationship",
        "tableProperties" => "Table Properties",
        "calculationOptions" => "Calculation Options",
        "addMeasure" if mode == "Update" => "Update Measure",
        "addMeasure" => "Add Measure",
        _ => "Power Pivot",
    };
    let mut close = false;
    egui::Window::new(title)
        .id(egui::Id::unique("power_pivot_overlay"))
        .collapsible(false)
        .resizable(false)
        .default_width(470.0)
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -30.0))
        .show(ctx, |ui| {
            ui.set_min_width(440.0);
            match overlay {
                "openModel" => power_pivot_open_model_dialog(ui, app, d, &t, &mut close, false),
                "existingConnections" => power_pivot_open_model_dialog(ui, app, d, &t, &mut close, true),
                "manageRelationships" => power_pivot_manage_relationships(ui, app, d, &t, &mut close),
                "createRelationship" => power_pivot_create_relationship(ui, app, d, &t, &mut close),
                "tableProperties" => power_pivot_table_properties(ui, d, &t, &mut close),
                "calculationOptions" => power_pivot_calculation_options(ui, d, &t, &mut close),
                "addMeasure" => power_pivot_measure_dialog(ui, app, d, &t, &mut close),
                _ => close = true,
            }
        });
    if close {
        d.values.insert("overlay".into(), json!(""));
        // A pick still in flight must not load a model into a later Open Model dialog.
        d.model_inbox = None;
    }
}

/// The Open Model / Existing Connections dialog: the model path now lives here, not in the canvas.
fn power_pivot_open_model_dialog(ui: &mut egui::Ui, app: &mut SheetApp, d: &mut Dialog, t: &theme::Tokens, close: &mut bool, existing: bool) {
    // A browser picker delivers the chosen file's bytes asynchronously (the workbook inbox opens
    // its files as workbooks, so this dialog owns a private one). Load them as soon as they arrive.
    let arrived = d.model_inbox.as_ref().and_then(|inbox| inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).pop());
    if let Some((name, bytes)) = arrived {
        d.model_inbox = None;
        match String::from_utf8(bytes) {
            Ok(text) => {
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
                d.values.insert("modelName".into(), json!(name));
                d.values.insert("modelJson".into(), json!(text));
                d.values.insert("error".into(), json!(""));
                power_pivot_load(app, d);
                if d.values.get("error").and_then(Json::as_str).unwrap_or("").is_empty() {
                    *close = true;
                }
            }
            Err(_) => {
                d.values.insert("error".into(), json!("The model file is not UTF-8 JSON."));
            }
        }
    }
    ui.label(egui::RichText::new(if existing { "Connections" } else { "Model file" }).strong().color(t.text));
    if existing {
        let path = d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string();
        let label = if path.trim().is_empty() { "No connection yet".to_string() } else { path };
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            icons::paint(ui.painter(), rect, Icon::Link, t.accent);
            ui.label(egui::RichText::new(label).color(t.text));
        });
        ui.add_space(4.0);
    }
    let web = app.services.open_async.is_some();
    ui.label(egui::RichText::new(if web { "Model JSON" } else { "Model file (JSON)" }).small().color(t.text_dim));
    let picked = d.values.get("modelJson").and_then(Json::as_str).map(str::to_string);
    let chosen = d.values.get("modelName").and_then(Json::as_str).unwrap_or("").to_string();
    if !chosen.is_empty() {
        ui.label(egui::RichText::new(format!("Selected file: {chosen}")).small().color(t.text));
    }
    let mut path = picked.unwrap_or_else(|| d.values.get("path").and_then(Json::as_str).unwrap_or("").to_string());
    let width = ui.available_width().max(320.0);
    let changed = if web {
        // The browser has no filesystem path; pick a file below or paste its JSON here.
        ui.add_sized([width, 90.0], egui::TextEdit::multiline(&mut path).hint_text("{\"tables\": [...]}")).changed()
    } else {
        ui.add_sized([width, 24.0], egui::TextEdit::singleline(&mut path).hint_text("C:\\models\\model.json")).changed()
    };
    if changed {
        // An edited box is the model source now, so the picked file name no longer describes it.
        d.values.remove("modelName");
        d.values.remove("path");
        d.values.insert("modelJson".into(), json!(path));
    }
    ui.label(
        egui::RichText::new("The Data View loads this file with dataModel.load. Model edits stay in the session, not the workbook.")
            .small()
            .color(t.text_dim),
    );
    let mut load = false;
    let mut browse = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if (app.services.pick_model_async.is_some() || app.services.pick_open.is_some()) && ui.button("Browse…").clicked() {
            browse = true;
        }
        if accent_button(ui, "Load").clicked() {
            load = true;
        }
        if ui.button("Cancel").clicked() {
            *close = true;
        }
    });
    if browse {
        if let Some(start) = &app.services.pick_model_async {
            // Replace any previous pick: a late result from the old picker has nowhere to land.
            let inbox = crate::Inbox::default();
            start(inbox.clone());
            d.model_inbox = Some(inbox);
            d.values.insert("error".into(), json!(""));
        } else if let Some(pick) = &app.services.pick_open
            && let Some(path) = pick()
        {
            d.values.remove("modelJson");
            d.values.remove("modelName");
            d.values.insert("path".into(), json!(path));
            d.values.insert("error".into(), json!(""));
        }
    }
    if load {
        power_pivot_load(app, d);
        if d.values.get("error").and_then(Json::as_str).unwrap_or("").is_empty() {
            *close = true;
        }
    }
    let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
    if !error.is_empty() {
        ui.label(egui::RichText::new(error).color(t.danger).small());
    }
}

/// Manage Relationships: the model's list, with Delete and a way into Create Relationship.
fn power_pivot_manage_relationships(ui: &mut egui::Ui, app: &mut SheetApp, d: &mut Dialog, t: &theme::Tokens, close: &mut bool) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let relationships = model.get("relationships").and_then(Json::as_array).cloned().unwrap_or_default();
    let mut selected = d.values.get("relIndex").and_then(Json::as_i64).unwrap_or(-1);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Relationships").strong().color(t.text));
        ui.label(egui::RichText::new(format!("{} defined", relationships.len())).small().color(t.text_dim));
    });
    ui.add_space(3.0);
    if relationships.is_empty() {
        ui.label(egui::RichText::new("No relationships are exposed by this model.").italics().color(t.text_dim));
    } else {
        egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(4).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("pp_manage_relationships").max_height(210.0).auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new("pp_manage_relationships_grid").striped(true).spacing(vec2(16.0, 3.0)).show(ui, |ui| {
                    for heading in ["From", "To", "Cross filter"] {
                        egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                            ui.label(egui::RichText::new(heading).font(theme::ui_bold(12.0)).color(t.header_text));
                        });
                    }
                    ui.end_row();
                    for (index, relationship) in relationships.iter().enumerate() {
                        let from_table = relationship.get("from_table").and_then(Json::as_str).unwrap_or("");
                        let from_column = relationship.get("from_column").and_then(Json::as_str).unwrap_or("");
                        let to_table = relationship.get("to_table").and_then(Json::as_str).unwrap_or("");
                        let to_column = relationship.get("to_column").and_then(Json::as_str).unwrap_or("");
                        let chosen = selected == index as i64;
                        if ui.add(egui::Button::selectable(chosen, format!("{from_table}[{from_column}]"))).clicked() {
                            selected = index as i64;
                        }
                        if ui.add(egui::Button::selectable(chosen, format!("{to_table}[{to_column}]"))).clicked() {
                            selected = index as i64;
                        }
                        let bidirectional = relationship.get("bidirectional").and_then(Json::as_bool).unwrap_or(false);
                        ui.label(egui::RichText::new(if bidirectional { "Both" } else { "Single" }).color(t.text_dim));
                        ui.end_row();
                    }
                });
            });
        });
    }
    ui.add_space(4.0);
    let mut delete = false;
    let mut create = false;
    ui.horizontal(|ui| {
        if accent_button(ui, "Create Relationship…").clicked() {
            create = true;
        }
        let valid = selected >= 0 && (selected as usize) < relationships.len();
        if ui.add_enabled(valid, egui::Button::new("Delete")).clicked() {
            delete = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                *close = true;
            }
        });
    });
    d.values.insert("relIndex".into(), json!(selected));
    if create {
        power_pivot_open_relationship_dialog(d);
        return;
    }
    if delete {
        let relationship = &relationships[selected as usize];
        let params = json!({
            "from_table": relationship.get("from_table").cloned().unwrap_or(Json::Null),
            "from_column": relationship.get("from_column").cloned().unwrap_or(Json::Null),
            "to_table": relationship.get("to_table").cloned().unwrap_or(Json::Null),
            "to_column": relationship.get("to_column").cloned().unwrap_or(Json::Null),
        });
        match app.session.run("dataModel.deleteRelationship", params) {
            Ok(status) => {
                d.values.insert("model".into(), status);
                d.values.insert("error".into(), json!(""));
                d.values.insert("relIndex".into(), json!(-1));
            }
            Err(e) => {
                d.values.insert("error".into(), json!(crate::clean_error(&e)));
            }
        }
    }
    let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
    if !error.is_empty() {
        ui.label(egui::RichText::new(error).color(t.danger).small());
    }
}

/// Create Relationship: From table/column, To table/column, and the bidirectional check box.
fn power_pivot_create_relationship(ui: &mut egui::Ui, app: &mut SheetApp, d: &mut Dialog, t: &theme::Tokens, close: &mut bool) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let tables: Vec<String> = model
        .get("tables")
        .and_then(Json::as_array)
        .map(|tables| tables.iter().filter_map(|table| table.get("name")).filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    if tables.is_empty() {
        ui.label(egui::RichText::new("Load a model before creating a relationship.").italics().color(t.text_dim));
        if ui.button("Close").clicked() {
            *close = true;
        }
        return;
    }
    let mut from_table = d.values.get("relFromTable").and_then(Json::as_str).unwrap_or("").to_string();
    let mut to_table = d.values.get("relToTable").and_then(Json::as_str).unwrap_or("").to_string();
    if !tables.contains(&from_table) {
        from_table = tables[0].clone();
    }
    if !tables.contains(&to_table) {
        to_table = tables.get(1).cloned().unwrap_or_else(|| tables[0].clone());
    }
    let from_columns = power_pivot_column_names(&model, &from_table);
    let to_columns = power_pivot_column_names(&model, &to_table);
    let mut from_column = d.values.get("relFromColumn").and_then(Json::as_str).unwrap_or("").to_string();
    let mut to_column = d.values.get("relToColumn").and_then(Json::as_str).unwrap_or("").to_string();
    if !from_columns.contains(&from_column) {
        from_column = from_columns.first().cloned().unwrap_or_default();
    }
    if !to_columns.contains(&to_column) {
        to_column = to_columns.first().cloned().unwrap_or_default();
    }
    let mut bidirectional = d.values.get("relBidirectional").and_then(Json::as_bool).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("From").strong().color(t.text));
        ui.label(egui::RichText::new("To").strong().color(t.text));
    });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("pp_rel_from_table").width(190.0).selected_text(from_table.clone()).show_ui(ui, |ui| {
            for table in &tables {
                ui.selectable_value(&mut from_table, table.clone(), table);
            }
        });
        egui::ComboBox::from_id_salt("pp_rel_to_table").width(190.0).selected_text(to_table.clone()).show_ui(ui, |ui| {
            for table in &tables {
                ui.selectable_value(&mut to_table, table.clone(), table);
            }
        });
    });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("pp_rel_from_column").width(190.0).selected_text(from_column.clone()).show_ui(ui, |ui| {
            for column in &from_columns {
                ui.selectable_value(&mut from_column, column.clone(), column);
            }
        });
        egui::ComboBox::from_id_salt("pp_rel_to_column").width(190.0).selected_text(to_column.clone()).show_ui(ui, |ui| {
            for column in &to_columns {
                ui.selectable_value(&mut to_column, column.clone(), column);
            }
        });
    });
    ui.checkbox(&mut bidirectional, "Make this relationship bidirectional");
    d.values.insert("relFromTable".into(), json!(from_table));
    d.values.insert("relFromColumn".into(), json!(from_column));
    d.values.insert("relToTable".into(), json!(to_table));
    d.values.insert("relToColumn".into(), json!(to_column));
    d.values.insert("relBidirectional".into(), json!(bidirectional));
    let mut create = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if accent_button(ui, "Create").clicked() {
            create = true;
        }
        if ui.button("Cancel").clicked() {
            *close = true;
        }
    });
    if create {
        let params = json!({
            "from_table": from_table,
            "from_column": from_column,
            "to_table": to_table,
            "to_column": to_column,
            "bidirectional": bidirectional,
        });
        match app.session.run("dataModel.addRelationship", params) {
            Ok(status) => {
                d.values.insert("model".into(), status);
                d.values.insert("error".into(), json!(""));
                d.values.insert("relIndex".into(), json!(-1));
                *close = true;
            }
            Err(e) => {
                d.values.insert("error".into(), json!(crate::clean_error(&e)));
            }
        }
    }
    let error = d.values.get("error").and_then(Json::as_str).unwrap_or("").to_string();
    if !error.is_empty() {
        ui.label(egui::RichText::new(error).color(t.danger).small());
    }
}

/// Table Properties: read-only name, columns/types and row count for the selected table.
fn power_pivot_table_properties(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens, close: &mut bool) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let index = d.values.get("tableIndex").and_then(Json::as_u64).unwrap_or(0) as usize;
    let table = model.get("tables").and_then(Json::as_array).and_then(|tables| tables.get(index)).cloned();
    let Some(table) = table else {
        ui.label(egui::RichText::new("No table is selected.").italics().color(t.text_dim));
        if ui.button("Close").clicked() {
            *close = true;
        }
        return;
    };
    let name = table.get("name").and_then(Json::as_str).unwrap_or("Table");
    let columns = table.get("columns").and_then(Json::as_array).cloned().unwrap_or_default();
    let rows = table.get("rows").and_then(Json::as_u64).unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Table name").small().color(t.text_dim));
        ui.label(egui::RichText::new(name).font(theme::ui_bold(13.0)).color(t.text));
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Rows").small().color(t.text_dim));
        ui.label(egui::RichText::new(rows.to_string()).color(t.text));
        ui.label(egui::RichText::new("Columns").small().color(t.text_dim));
        ui.label(egui::RichText::new(columns.len().to_string()).color(t.text));
    });
    ui.add_space(4.0);
    egui::Frame::NONE.fill(t.ribbon).stroke(Stroke::new(1.0, t.ribbon_border)).corner_radius(3.0).inner_margin(4).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("pp_table_properties").max_height(220.0).auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("pp_table_properties_grid").striped(true).spacing(vec2(20.0, 3.0)).show(ui, |ui| {
                for heading in ["Column", "Type"] {
                    egui::Frame::NONE.fill(t.header_bg).inner_margin(egui::Margin::symmetric(5, 2)).show(ui, |ui| {
                        ui.label(egui::RichText::new(heading).font(theme::ui_bold(12.0)).color(t.header_text));
                    });
                }
                ui.end_row();
                for column in &columns {
                    let column_name = column.get("name").and_then(Json::as_str).unwrap_or("");
                    let data_type = column.get("type").and_then(Json::as_str).unwrap_or("Text");
                    ui.label(egui::RichText::new(column_name).color(t.text));
                    ui.label(egui::RichText::new(data_type).color(t.text_dim));
                    ui.end_row();
                }
            });
        });
    });
    ui.add_space(4.0);
    if ui.button("Close").clicked() {
        *close = true;
    }
}

/// Calculation Options: Automatic / Manual. Manual is UI state only; edits still commit at once.
fn power_pivot_calculation_options(ui: &mut egui::Ui, d: &mut Dialog, t: &theme::Tokens, close: &mut bool) {
    let mut mode = d.values.get("calcMode").and_then(Json::as_str).unwrap_or("Automatic").to_string();
    ui.label(egui::RichText::new("Calculation").strong().color(t.text));
    ui.radio_value(&mut mode, "Automatic".to_string(), "Automatic");
    ui.radio_value(&mut mode, "Manual".to_string(), "Manual");
    ui.label(
        egui::RichText::new("Manual calculation is a window setting only: measure and relationship commands commit immediately.")
            .small()
            .color(t.text_dim),
    );
    d.values.insert("calcMode".into(), json!(mode));
    ui.add_space(4.0);
    if ui.button("Close").clicked() {
        *close = true;
    }
}

/// Add Measure / Update Measure: Name, Table, Formula and Format.
fn power_pivot_measure_dialog(ui: &mut egui::Ui, app: &mut SheetApp, d: &mut Dialog, t: &theme::Tokens, close: &mut bool) {
    let model = d.values.get("model").cloned().unwrap_or(Json::Null);
    let tables: Vec<String> = model
        .get("tables")
        .and_then(Json::as_array)
        .map(|tables| tables.iter().filter_map(|table| table.get("name")).filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let mode = d.values.get("measureDialogMode").and_then(Json::as_str).unwrap_or("Add").to_string();
    let mut name = d.values.get("newMeasureName").and_then(Json::as_str).unwrap_or("").to_string();
    let mut table = d.values.get("newMeasureTable").and_then(Json::as_str).unwrap_or("").to_string();
    if !tables.is_empty() && !tables.contains(&table) {
        table = tables[0].clone();
    }
    let mut formula = d.values.get("newMeasureFormula").and_then(Json::as_str).unwrap_or("").to_string();
    let mut format = d.values.get("newMeasureFormat").and_then(Json::as_str).unwrap_or("").to_string();
    ui.label(egui::RichText::new("Name").small().color(t.text_dim));
    if ui.add_sized([ui.available_width().max(320.0), 23.0], egui::TextEdit::singleline(&mut name).hint_text("Total Sales")).changed() {
        d.values.insert("newMeasureName".into(), json!(name));
    }
    ui.add_space(2.0);
    ui.label(egui::RichText::new("Table").small().color(t.text_dim));
    egui::ComboBox::from_id_salt("pp_new_measure_table").width(220.0).selected_text(if table.is_empty() { "Select…" } else { &table }).show_ui(
        ui,
        |ui| {
            for candidate in &tables {
                ui.selectable_value(&mut table, candidate.clone(), candidate);
            }
        },
    );
    ui.add_space(2.0);
    ui.label(egui::RichText::new("Formula").small().color(t.text_dim));
    let width = ui.available_width().max(320.0);
    egui::Frame::NONE.fill(t.input_bg).stroke(Stroke::new(1.0, t.input_border)).corner_radius(3.0).inner_margin(4).show(ui, |ui| {
        if ui
            .add_sized(
                [width - 10.0, 70.0],
                egui::TextEdit::multiline(&mut formula).frame(egui::Frame::NONE).hint_text("SUM(Sales[Amount])").desired_rows(3),
            )
            .changed()
        {
            d.values.insert("newMeasureFormula".into(), json!(formula));
        }
    });
    ui.add_space(2.0);
    ui.label(egui::RichText::new("Format").small().color(t.text_dim));
    if ui.add_sized([220.0, 23.0], egui::TextEdit::singleline(&mut format).hint_text("Currency")).changed() {
        d.values.insert("newMeasureFormat".into(), json!(format));
    }
    d.values.insert("newMeasureTable".into(), json!(table));
    d.values.insert("newMeasureName".into(), json!(name));
    d.values.insert("newMeasureFormula".into(), json!(formula));
    d.values.insert("newMeasureFormat".into(), json!(format));
    ui.add_space(4.0);
    let mut submit = false;
    let update = mode == "Update";
    ui.horizontal(|ui| {
        if accent_button(ui, if update { "Update" } else { "Add" }).clicked() {
            submit = true;
        }
        if ui.button("Cancel").clicked() {
            *close = true;
        }
    });
    if submit {
        let outcome = if update {
            let selected = power_pivot_selected_measure(d);
            match selected {
                Some(measure) => {
                    let old_name = measure.get("name").and_then(Json::as_str).unwrap_or("").to_string();
                    let mut params = Map::new();
                    params.insert("name".into(), json!(old_name));
                    if !name.trim().is_empty() && name.trim() != old_name {
                        params.insert("new_name".into(), json!(name.trim()));
                    }
                    if !formula.trim().is_empty() {
                        params.insert("formula".into(), json!(formula.trim()));
                    }
                    params.insert("format".into(), if format.trim().is_empty() { Json::Null } else { json!(format.trim()) });
                    app.session.run("dataModel.updateMeasure", Json::Object(params))
                }
                None => Err("Select a measure in the Calculation Area first.".to_string()),
            }
        } else {
            let mut params = Map::new();
            params.insert("name".into(), json!(name.trim()));
            params.insert("formula".into(), json!(formula.trim()));
            if !table.trim().is_empty() {
                params.insert("table".into(), json!(table.trim()));
            }
            if !format.trim().is_empty() {
                params.insert("format".into(), json!(format.trim()));
            }
            app.session.run("dataModel.addMeasure", Json::Object(params))
        };
        match outcome {
            Ok(status) => {
                let chosen = name.trim().to_string();
                let index = status
                    .get("measures")
                    .and_then(Json::as_array)
                    .and_then(|measures| measures.iter().position(|measure| measure.get("name").and_then(Json::as_str) == Some(&chosen)));
                d.values.insert("model".into(), status);
                d.values.insert("error".into(), json!(""));
                d.values.insert("resultKind".into(), json!(""));
                d.values.insert("resultText".into(), json!(""));
                d.values.insert("column".into(), json!(""));
                d.values.insert("columnIndex".into(), json!(-1));
                d.values.insert("measureIndex".into(), json!(index.map_or(-1, |index| index as i64)));
                d.values.insert("formula".into(), json!(if index.is_some() { format!("[{chosen}]") } else { String::new() }));
                d.result = None;
                *close = true;
            }
            Err(e) => {
                d.values.insert("error".into(), json!(crate::clean_error(&e)));
            }
        }
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
