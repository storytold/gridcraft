//! Dialogs for ribbon controls whose command only reports data, asks for its input, or drives the
//! window list (#1797): Add-ins, Queries & Connections, Thesaurus, Custom Views and PivotChart.
//! Each shows what its command returned instead of throwing it away.

use super::show::ok_cancel;
use super::{Dialog, Json, Key, SheetApp, json};
use crate::theme;

/// The word the Thesaurus looks up: the active cell's text, as Excel looks up the selected text.
pub(super) fn selected_word(app: &SheetApp) -> String {
    app.session.active().and_then(|d| d.wb.active().map(|sh| sh.value(d.selection.active).display())).unwrap_or_default()
}

pub(super) fn thesaurus_result(app: &mut SheetApp, word: &str) -> Json {
    app.session.run("review.thesaurus", json!({"word": word})).unwrap_or_else(|e| json!({"error": e}))
}

/// A result row's name: its `name` or `title`, or the value itself for a plain string.
fn row_label(row: &Json) -> String {
    for key in ["name", "title"] {
        if let Some(text) = row.get(key).and_then(Json::as_str) {
            return text.to_string();
        }
    }
    detail(row)
}

fn detail(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Draws a query result the way Excel's lists do: a line per row (its name), the row's other
/// fields as a smaller second line, and "(None)" when the list is empty.
pub(super) fn result_rows(app: &SheetApp, ui: &mut egui::Ui, result: Option<&Json>) {
    let r = result.cloned().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(app.ui.language.tr_error(e));
        return;
    }
    let rows = match r {
        Json::Array(a) => a,
        Json::Null => vec![],
        other => vec![other],
    };
    if rows.is_empty() {
        ui.label(egui::RichText::new("(None)").italics());
        return;
    }
    let dim = theme::Tokens::get(ui.ctx()).text_dim;
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        for row in &rows {
            ui.label(row_label(row));
            let rest: Vec<String> = row
                .as_object()
                .map(|m| m.iter().filter(|(k, _)| !matches!(k.as_str(), "name" | "title")).map(|(k, v)| format!("{k}: {}", detail(v))).collect())
                .unwrap_or_default();
            if !rest.is_empty() {
                ui.label(egui::RichText::new(rest.join(" · ")).small().color(dim));
            }
        }
    });
}

/// Home › Add-ins: GridCraft has none, and says where its extensions live instead.
pub(super) fn add_ins(app: &SheetApp, ui: &mut egui::Ui, d: &Dialog) {
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(app.ui.language.tr_error(e));
        return;
    }
    let list = r.get("addIns").and_then(Json::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        ui.label("No add-ins are installed.");
    } else {
        for item in &list {
            ui.label(row_label(item));
        }
    }
    if let Some(hint) = r.get("hint").and_then(Json::as_str) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(app.ui.language.tr(hint)).small());
    }
}

/// Review › Thesaurus: look a word up and list the related words the dictionary knows.
pub(super) fn thesaurus(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let mut word = d.values.get("word").and_then(Json::as_str).unwrap_or("").to_string();
    ui.label("Insert word or phrase:");
    let field = ui.add(egui::TextEdit::singleline(&mut word).desired_width(300.0));
    d.values.insert("word".into(), json!(word));
    let mut look_up = ui.button("Look Up").clicked();
    if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        look_up = true;
    }
    if look_up {
        d.result = Some(thesaurus_result(app, d.values.get("word").and_then(Json::as_str).unwrap_or("").trim()));
    }
    ui.separator();
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(app.ui.language.tr_error(e));
    } else {
        let words: Vec<String> = r.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if words.is_empty() {
            ui.label(egui::RichText::new("No related words found.").italics());
        } else {
            ui.label(egui::RichText::new("Related words").strong());
            egui::ScrollArea::vertical().max_height(200.0).id_salt("thesaurus_words").show(ui, |ui| {
                for w in &words {
                    ui.label(w);
                }
            });
        }
    }
    if ui.button("Close").clicked() {
        *open = false;
    }
}

/// View › Custom Views: the saved views, with Show and Delete, and the current view saved under
/// a name — Excel's Custom Views dialog.
pub(super) fn custom_views(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(app.ui.language.tr_error(e));
        return;
    }
    let names: Vec<String> = r.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    ui.label(egui::RichText::new("Views").strong());
    egui::ScrollArea::vertical().max_height(180.0).id_salt("custom_views").show(ui, |ui| {
        for (i, name) in names.iter().enumerate() {
            if ui.selectable_label(d.list_index == i, name).clicked() {
                d.list_index = i;
            }
        }
        if names.is_empty() {
            ui.label(egui::RichText::new("(No views saved)").italics());
        }
    });
    let picked = names.get(d.list_index).cloned();
    let mut refresh = false;
    ui.horizontal(|ui| {
        if ui.add_enabled(picked.is_some(), egui::Button::new("Show")).clicked()
            && let Some(name) = &picked
        {
            app.run_or_alert("view.customViews", json!({"show": name}));
            *open = false;
        }
        if ui.add_enabled(picked.is_some(), egui::Button::new("Delete")).clicked()
            && let Some(name) = &picked
        {
            if let Err(e) = app.session.run("view.customViews", json!({"delete": name})) {
                app.message = Some((d.title.clone(), crate::clean_error(&e)));
            }
            d.list_index = 0;
            refresh = true;
        }
    });
    ui.separator();
    ui.label("Add the current view:");
    let mut name = d.values.get("name").and_then(Json::as_str).unwrap_or("").to_string();
    if ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0)).changed() {
        d.values.insert("name".into(), json!(name));
    }
    let blank = d.values.get("name").and_then(Json::as_str).is_none_or(|n| n.trim().is_empty());
    if ui.add_enabled(!blank, egui::Button::new("Add")).clicked() {
        let typed = d.values.get("name").and_then(Json::as_str).unwrap_or("").trim().to_string();
        if let Err(e) = app.session.run("view.customViews", json!({"save": typed})) {
            app.message = Some((d.title.clone(), crate::clean_error(&e)));
        }
        d.values.insert("name".into(), json!(""));
        refresh = true;
    }
    if refresh {
        d.result = Some(app.session.run("view.customViews", json!({})).unwrap_or_else(|e| json!({"error": e})));
        // The refreshed list can reorder or grow (a new view may sort before the selected one), so
        // select the row by name; Show keeps targeting the view that was selected.
        let after: Vec<String> = d.result.as_ref().and_then(Json::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
        d.list_index = picked.as_ref().and_then(|name| after.iter().position(|n| n == name)).unwrap_or(0);
    }
}

/// Insert › PivotChart without a PivotTable at the active cell: pick the report to chart, as
/// Excel's Create PivotChart asks. With no PivotTable in the workbook it says so instead of
/// failing when the chart is asked for.
pub(super) fn pivot_chart(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(app.ui.language.tr_error(e));
        return;
    }
    let pivots = r.as_array().cloned().unwrap_or_default();
    if pivots.is_empty() {
        ui.label("This workbook has no PivotTable to chart.");
        ui.label(egui::RichText::new("Insert › PivotTable builds one from the selected data; PivotChart then charts that report.").small());
        if ui.button("Close").clicked() {
            *open = false;
        }
        return;
    }
    ui.label("PivotTable:");
    let selected = d.values.get("pivot").cloned().unwrap_or(Json::Null);
    // The report at the active cell is preselected; otherwise the first one, so OK always targets
    // one. The selection is submitted as the row's `id`: the engine reads a string `pivot` as a
    // name or a cell, so a report named `A1` would be taken for a cell reference.
    let mut picked = pivots
        .iter()
        .find(|p| selected.as_str() == p.get("name").and_then(Json::as_str) || p.get("id").is_some_and(|id| id == &selected))
        .or_else(|| pivots.first())
        .and_then(|p| p.get("id").cloned());
    for p in &pivots {
        let name = p.get("name").and_then(Json::as_str).unwrap_or("");
        let sheet = p.get("sheet").and_then(Json::as_str).unwrap_or("");
        let anchor = p.get("anchor").and_then(Json::as_str).unwrap_or("");
        if ui.radio(picked.as_ref() == p.get("id"), format!("{name} ({sheet}!{anchor})")).clicked() {
            picked = p.get("id").cloned();
        }
    }
    if let Some(id) = picked {
        d.values.insert("pivot".into(), id);
    }
    ui.horizontal(|ui| {
        ui.label("Chart type:");
        let mut kind = d.values.get("type").and_then(Json::as_str).unwrap_or("column").to_string();
        craft_ui::ComboBox::from_id_salt("pivot_chart_type").selected_text(kind.clone()).width(140.0).show_ui(ui, |ui| {
            for k in ["column", "bar", "line", "pie", "area", "scatter"] {
                if ui.selectable_label(kind == k, k).clicked() {
                    kind = k.to_string();
                }
            }
        });
        d.values.insert("type".into(), json!(kind));
    });
    ok_cancel(ui, confirm, open);
}

/// Insert › Recommended PivotTables: the suggestions the engine found for the selected data, picked
/// by index. OK applies the pick (`{"apply": i}`), which builds that PivotTable.
pub(super) fn recommended_pivot_tables(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let suggestions = d.result.as_ref().and_then(|r| r.get("suggestions")).and_then(Json::as_array).cloned().unwrap_or_default();
    if suggestions.is_empty() {
        ui.label("No PivotTable suggestion for the selected data.");
        ui.label(
            egui::RichText::new("Select a range with a header row and at least one category or number column; Excel's suggestions use them.").small(),
        );
        if ui.button("Close").clicked() {
            *open = false;
        }
        return;
    }
    ui.label("PivotTable suggestions for the selected data:");
    let title = |s: &Json| s.get("title").and_then(Json::as_str).unwrap_or("PivotTable").to_string();
    egui::ScrollArea::vertical().max_height(220.0).id_salt("recommended_pivots").show(ui, |ui| {
        for (i, s) in suggestions.iter().enumerate() {
            if ui.selectable_label(d.list_index == i, title(s)).clicked() {
                d.list_index = i;
                d.values.insert("apply".into(), json!(i));
            }
        }
    });
    ok_cancel(ui, confirm, open);
}
