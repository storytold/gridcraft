use super::{Dialog, Json, SheetApp, json};
use crate::i18n::Language;

/// The label naming the cell whose word is missing from the proofing dictionary.
///
/// Translate the template first, then substitute: `tr` looks labels up by their catalog text, so a
/// key built with the cell already in it never matches and falls back to English.
fn not_in_dictionary(language: Language, cell: &str) -> String {
    language.tr("Not in Dictionary ({cell}):").replace("{cell}", cell)
}

pub(super) fn spelling(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let language = app.ui.language;
    let r = d.result.clone().unwrap_or(Json::Null);
    if let Some(e) = r.get("error").and_then(Json::as_str) {
        ui.label(language.tr_error(e));
        return;
    }
    let issues = r["issues"].as_array().cloned().unwrap_or_default();
    let Some(it) = issues.get(d.list_index) else {
        ui.label(language.tr("The spelling check is complete for the entire sheet."));
        if ui.button(language.tr("OK")).clicked() {
            *open = false;
        }
        return;
    };
    let word = it["word"].as_str().unwrap_or("").to_string();
    let cell = it["cell"].as_str().unwrap_or("").to_string();
    ui.label(not_in_dictionary(language, &cell));
    ui.label(egui::RichText::new(&word).strong().color(crate::theme::Tokens::get(ui.ctx()).danger));
    let sugg: Vec<String> =
        it["suggestions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    ui.label(language.tr("Suggestions:"));
    let mut pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
    for s in &sugg {
        if ui.selectable_label(pick == *s, s).clicked() {
            pick = s.clone();
        }
    }
    if sugg.is_empty() {
        ui.label(egui::RichText::new(language.tr("(No suggestions)")).italics());
    }
    d.values.insert("pick".into(), json!(pick.clone()));
    let _ = app.session.run("selection.set", json!({"cell": cell}));
    ui.horizontal(|ui| {
        if ui.button(language.tr("Ignore Once")).clicked() {
            d.list_index += 1;
        }
        if ui.button(language.tr("Add to Dictionary")).clicked() {
            let _ = app.session.run("review.addToDictionary", json!({"word": word}));
            d.list_index += 1;
        }
        if !pick.is_empty() {
            if ui.button(language.tr("Change")).clicked() {
                app.run_or_alert("review.changeSpelling", json!({"cell": cell, "word": word, "to": pick}));
                d.list_index += 1;
            }
            if ui.button(language.tr("Change All")).clicked() {
                app.run_or_alert("review.changeSpelling", json!({"word": word, "to": pick, "all": true}));
                d.list_index += 1;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_in_dictionary_translates_the_template_before_naming_the_cell() {
        let label = not_in_dictionary(Language::PtBr, "B2");
        assert!(label.contains("B2"), "{label}");
        assert!(label.to_lowercase().contains("dicionário"), "{label}");
        let english = not_in_dictionary(Language::En, "B2");
        assert!(english.contains("B2"), "{english}");
        assert!(english.contains("Not in Dictionary"), "{english}");
    }
}
