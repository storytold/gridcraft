//! Workbook Statistics, Accessibility, Error Checking, Evaluate Formula and the sheet's comments:
//! the engine's report drawn as labelled rows in the interface language, not as raw JSON.

use super::Json;
use crate::i18n::Language;

/// A report value as the user reads it: text and numbers unquoted, a cell error as its code.
fn text(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::String(s) => s.clone(),
        // JSON writes a whole float as `3.0`; the sheet shows 3.
        Json::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |f| f.to_string()),
        Json::Object(m) if m.contains_key("error") => text(&m["error"]),
        other => other.to_string(),
    }
}

/// The widest a report column grows before its text wraps, so a long formula stays in the window.
const MAX_COL_WIDTH: f32 = 280.0;
/// The rows a report table draws per page; a sheet can report up to 10,000 error cells.
const MAX_ROWS: usize = 200;

fn rows(ui: &mut egui::Ui, lang: Language, id: &str, report: &Json, rows: &[(&str, &str)]) {
    egui::Grid::new(id).num_columns(2).spacing([24.0, 4.0]).max_col_width(MAX_COL_WIDTH).striped(true).show(ui, |ui| {
        for (label, key) in rows {
            ui.label(lang.tr(label));
            ui.add(egui::Label::new(text(&report[*key])).wrap());
            ui.end_row();
        }
    });
}

/// A table of `items`, a page of [`MAX_ROWS`] at a time; each row's cells are built by `row` only
/// when drawn. Longer reports get Previous/Next buttons, and the page is kept per table `id`.
fn table(ui: &mut egui::Ui, lang: Language, id: &str, headers: &[&str], items: &[Json], row: impl Fn(&Json) -> Vec<String>) {
    let start = page(ui, lang, id, items.len()) * MAX_ROWS;
    egui::Grid::new(id).num_columns(headers.len()).spacing([24.0, 4.0]).max_col_width(MAX_COL_WIDTH).striped(true).show(ui, |ui| {
        for h in headers {
            ui.strong(lang.tr(h));
        }
        ui.end_row();
        for item in items.iter().skip(start).take(MAX_ROWS) {
            for cell in row(item) {
                ui.add(egui::Label::new(cell).wrap());
            }
            ui.end_row();
        }
    });
}

/// The page of the `total` rows to draw. Above the table of a report longer than [`MAX_ROWS`],
/// it draws "Rows 201–400 of 1000" between Previous and Next.
fn page(ui: &mut egui::Ui, lang: Language, id: &str, total: usize) -> usize {
    let pages = total.div_ceil(MAX_ROWS).max(1);
    let key = ui.make_persistent_id((id, "page"));
    let mut page = ui.data(|d| d.get_temp::<usize>(key)).unwrap_or(0).min(pages - 1);
    if pages > 1 {
        ui.horizontal(|ui| {
            if ui.add_enabled(page > 0, egui::Button::new(lang.tr("Previous"))).clicked() {
                page -= 1;
            }
            let first = page * MAX_ROWS + 1;
            let last = (first + MAX_ROWS - 1).min(total);
            ui.label(
                lang.tr("Rows {first}–{last} of {total}")
                    .replace("{first}", &first.to_string())
                    .replace("{last}", &last.to_string())
                    .replace("{total}", &total.to_string()),
            );
            if ui.add_enabled(page + 1 < pages, egui::Button::new(lang.tr("Next"))).clicked() {
                page += 1;
            }
        });
        ui.data_mut(|d| d.insert_temp(key, page));
    }
    page
}

/// A report command's error in the interface language. The engine's "no workbook open" comes
/// wrapped as "command `…` is not available right now: no workbook open", which `tr_error` can't
/// strip, so it is mapped to the catalogs' own sentence first.
fn error_message(lang: Language, error: &str) -> String {
    if error == "no workbook open" || error.ends_with(": no workbook open") {
        return lang.tr("No workbook is open.").to_string();
    }
    lang.tr_error(error)
}

fn list(v: &Json) -> &[Json] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// Draws the report dialog `name` from its engine result `r`.
pub(super) fn report(ui: &mut egui::Ui, name: &str, r: &Json) {
    let lang = crate::i18n::current(ui.ctx());
    // A failed command (no formula to evaluate, no workbook open) says why, not "nothing found".
    if let Some(e) = r.get("error") {
        ui.label(error_message(lang, &text(e)));
        return;
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| match name {
        "statistics" => {
            let sheet = &r["sheet"];
            ui.strong(lang.tr("Current sheet: {name}").replace("{name}", &text(&sheet["name"])));
            rows(
                ui,
                lang,
                "stats_sheet",
                sheet,
                &[
                    ("End of sheet", "endOfSheet"),
                    ("Cells with data", "cellsWithData"),
                    ("Tables", "tables"),
                    ("Formulas", "formulas"),
                    ("Charts", "charts"),
                    ("Comments", "comments"),
                ],
            );
            ui.add_space(8.0);
            ui.strong(lang.tr("Workbook:"));
            rows(
                ui,
                lang,
                "stats_workbook",
                &r["workbook"],
                &[("Sheets", "sheets"), ("Cells with data", "cellsWithData"), ("Tables", "tables"), ("Charts", "charts"), ("Names", "names")],
            );
        }
        "accessibility" => {
            let issues = list(r);
            if issues.is_empty() {
                ui.label(lang.tr("No accessibility issues found."));
                return;
            }
            table(ui, lang, "accessibility_issues", &["Sheet", "Issue", "Details"], issues, |i| {
                let detail = match i.get("count") {
                    Some(n) => lang.tr("Merged ranges: {count}").replace("{count}", &text(n)),
                    None => text(&i["object"]),
                };
                vec![text(&i["sheet"]), lang.tr(&text(&i["issue"])).to_string(), detail]
            });
        }
        "errorChecking" => {
            let (errors, circular) = (list(&r["errors"]), list(&r["circular"]));
            if errors.is_empty() && circular.is_empty() {
                ui.label(lang.tr("No errors found."));
                return;
            }
            if !errors.is_empty() {
                table(ui, lang, "error_cells", &["Cell", "Error", "Formula"], errors, |e| {
                    vec![format!("{}!{}", text(&e["sheet"]), text(&e["cell"])), text(&e["error"]), text(&e["formula"])]
                });
            }
            if !circular.is_empty() {
                ui.add_space(8.0);
                table(ui, lang, "circular_references", &["Circular References"], circular, |c| vec![text(c)]);
            }
        }
        "evaluateFormula" => {
            rows(ui, lang, "evaluate_formula", r, &[("Formula", "formula"), ("Result", "result")]);
            let steps = list(&r["steps"]);
            if !steps.is_empty() {
                ui.add_space(8.0);
                table(ui, lang, "evaluate_steps", &["Expression", "Value"], steps, |s| vec![text(&s["expression"]), text(&s["value"])]);
            }
        }
        _ => {
            let comments = list(r);
            if comments.is_empty() {
                ui.label(lang.tr("This sheet has no comments."));
                return;
            }
            table(ui, lang, "sheet_comments", &["Cell", "Author", "Text"], comments, |c| {
                vec![text(&c["cell"]), text(&c["author"]), text(&c["text"])]
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_no_workbook_error_reads_in_the_interface_language() {
        let unavailable = "command `formulas.errorChecking` is not available right now: no workbook open";
        assert_eq!(error_message(Language::De, unavailable), "Es ist keine Arbeitsmappe geöffnet.");
        assert_eq!(error_message(Language::De, "no workbook open"), "Es ist keine Arbeitsmappe geöffnet.");
        assert_eq!(error_message(Language::En, unavailable), "No workbook is open.");
        assert_eq!(error_message(Language::En, "The cell has no formula."), "The cell has no formula.");
    }
}
