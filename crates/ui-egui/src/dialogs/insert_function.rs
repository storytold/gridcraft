use super::show::ok_cancel;
use super::*;

/// Insert Function: searches names (as the formula language spells them) and descriptions, shows
/// each hit's category and description, and starts editing `=NAME(` (or appends to the formula).
pub(super) fn insert_function(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let language = app.ui.language;
    ui.horizontal(|ui| {
        ui.label(language.tr("Search:"));
        let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(300.0).hint_text(language.tr("e.g. lookup, average, date")));
        r.request_focus();
    });
    let locale = language.formula_locale();
    let list = gridcraft_engine::cmd::formulas::function_list();
    // The names and descriptions searched are the ones shown, so in the interface language.
    let q = d.search.to_lowercase();
    let hits: Vec<&Json> = list
        .iter()
        .filter(|f| {
            q.is_empty()
                || f["name"].as_str().is_some_and(|n| locale.function_name(n).to_lowercase().contains(&q))
                || f["description"].as_str().is_some_and(|n| language.function_description(n).unwrap_or(n).to_lowercase().contains(&q))
        })
        .take(300)
        .collect();
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (i, f) in hits.iter().enumerate() {
            let name = locale.function_name(f["name"].as_str().unwrap_or(""));
            let category = language.tr(function_category_label(f["category"].as_str().unwrap_or("")));
            let r = ui.add(egui::Button::selectable(i == d.list_index, format!("{name:<18} {category}")).min_size(vec2(480.0, 20.0)));
            if r.clicked() {
                d.list_index = i;
            }
            if r.double_clicked() {
                d.list_index = i;
                *confirm = true;
            }
        }
    });
    if let Some(f) = hits.get(d.list_index) {
        let name = f["name"].as_str().unwrap_or("");
        ui.separator();
        ui.label(egui::RichText::new(crate::formula_locale::signature(name, locale, language).unwrap_or_default()).strong());
        ui.label(crate::formula_locale::description(name, locale, language).unwrap_or_default());
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm && let Some(n) = hits.get(d.list_index).and_then(|f| f["name"].as_str()) {
        let n = app.session.active().map(|doc| crate::formula_locale::completion_name(n, locale, &doc.wb, doc.wb.active_sheet())).unwrap_or(n);
        let cur = app.editor.as_ref().map(|e| e.text.clone());
        let text = match cur {
            Some(t) if t.starts_with('=') => format!("{t}{n}("),
            _ => format!("={n}("),
        };
        app.begin_edit(Some(text), false);
    }
    if !open {
        *confirm = true;
    }
}

/// Excel's name for a function category, given the catalog's id (`MathTrig`).
fn function_category_label(id: &str) -> &str {
    match id {
        "DateTime" => "Date & Time",
        "Lookup" => "Lookup & Reference",
        "MathTrig" => "Math & Trig",
        "AddIn" => "Add-in and Automation",
        // Financial, Logical, Text, Statistical, Engineering, Database, Information, …
        other => other,
    }
}
