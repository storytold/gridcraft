use super::show::ok_cancel;
use super::{Dialog, Json, Key, SheetApp, json};

/// Draws the Find/Replace form with case, whole-cell and workbook options, requests field focus once, and runs find/replace with the current
/// parameters. Find All keeps its hit list in the dialog's results area; a search that finds nothing, and an empty query that is not a search at all,
/// report a no-result line there instead of a message box, while other errors - a missing workbook, a failing Replace All - keep the message-box path.
pub(super) fn find(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language;
    ui.horizontal(|ui| {
        // d.tab keeps English values for the control channel; the interface language shows the label.
        for (key, tab) in [("Find tab|Find", "Find"), ("Replace tab|Replace", "Replace")] {
            if ui.selectable_label(d.tab == tab, lang.tr(key)).clicked() {
                d.tab = tab.into();
            }
        }
    });
    let mut what = d.values.get("what").and_then(Json::as_str).unwrap_or("").to_string();
    let mut with = d.values.get("with").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        let label = ui.label(lang.tr("Find what:"));
        let r = ui.add(egui::TextEdit::singleline(&mut what).desired_width(260.0)).labelled_by(label.id);
        // Like Excel, the cursor starts in "Find what" - asked for once, so clicking another field still works.
        if !d.values.contains_key("focused") {
            r.request_focus();
            d.values.insert("focused".into(), json!(true));
        }
    });
    if d.tab == "Replace" {
        ui.horizontal(|ui| {
            let label = ui.label(lang.tr("Replace with:"));
            ui.add(egui::TextEdit::singleline(&mut with).desired_width(244.0)).labelled_by(label.id);
        });
    }
    let mut case = d.values.get("matchCase").and_then(Json::as_bool).unwrap_or(false);
    let mut whole = d.values.get("wholeCell").and_then(Json::as_bool).unwrap_or(false);
    let mut wb = d.values.get("within").and_then(Json::as_str) == Some("workbook");
    ui.horizontal(|ui| {
        ui.checkbox(&mut case, lang.tr("Match case"));
        ui.checkbox(&mut whole, lang.tr("Match entire cell contents"));
        ui.checkbox(&mut wb, lang.tr("Within workbook"));
    });
    d.values.insert("what".into(), json!(what));
    d.values.insert("with".into(), json!(with));
    d.values.insert("matchCase".into(), json!(case));
    d.values.insert("wholeCell".into(), json!(whole));
    d.values.insert("within".into(), json!(if wb { "workbook" } else { "sheet" }));
    // As in Excel, a selected range (more than one cell) limits the search on the sheet to it.
    // One selected cell, merged or not, searches the sheet, as in Excel.
    let range_selected = app.session.active().is_some_and(|doc| {
        let sel = &doc.selection;
        let one_merged = sel.ranges.len() == 1 && doc.wb.active().and_then(|sh| sh.merge_at(sel.current().start)).is_some_and(|m| m == sel.current());
        !sel.is_single_cell() && !one_merged
    });
    let within = if wb {
        "workbook"
    } else if range_selected {
        "selection"
    } else {
        "sheet"
    };
    let base = json!({"what": what, "matchCase": case, "wholeCell": whole, "within": within});
    ui.horizontal(|ui| {
        if d.tab == "Replace" {
            if ui.button(lang.tr("Replace All")).clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(true);
                match app.run("edit.replace", p) {
                    Ok(r) => app.toast = Some((format!("All done. We made {} replacements.", r["replaced"]), crate::now_ms())),
                    Err(e) => app.message = Some((lang.tr("Find and Replace").to_string(), crate::clean_error(&e))),
                }
            }
            if ui.button(lang.tr("Replace")).clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(false);
                let _ = app.run("edit.replace", p);
                let _ = app.run("edit.find", base.clone());
            }
        }
        if ui.button(lang.tr("Find All")).clicked() {
            if what.is_empty() {
                // An empty query is not a search, so the engine's bad-params error never reaches
                // the user as a message box: the results area says nothing was found.
                d.result = None;
                d.values.insert("notFound".into(), json!(true));
                d.values.remove("foundFor");
            } else {
                let mut p = base.clone();
                p["all"] = json!(true);
                match app.run("edit.find", p) {
                    Ok(r) => {
                        d.result = Some(r);
                        d.values.insert("notFound".into(), json!(false));
                        d.values.remove("foundFor");
                    }
                    Err(e) if e.starts_with("We couldn't find what you were looking for: ") => {
                        // Only the engine's no-match is reported here: the stale list goes, the
                        // dialog's own results area says it instead of a message box, and the line
                        // quotes the query this search actually ran, not whatever the field holds later.
                        d.result = None;
                        d.values.insert("notFound".into(), json!(true));
                        d.values.insert("foundFor".into(), json!(what.clone()));
                    }
                    // No open workbook is an error, not a no-result, and keeps the message-box path.
                    Err(e) => app.message = Some((lang.tr("Find and Replace").to_string(), crate::clean_error(&e))),
                }
            }
        }
        if ui.button(lang.tr("Find Next")).clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            if what.is_empty() {
                d.result = None;
                d.values.insert("notFound".into(), json!(true));
                d.values.remove("foundFor");
            } else {
                match app.run("edit.find", base.clone()) {
                    Ok(_) => {
                        d.values.insert("notFound".into(), json!(false));
                        d.values.remove("foundFor");
                    }
                    Err(e) => {
                        // A failed Find Next keeps the message box, but the engine's "couldn't find"
                        // error is shown as the dialog's own quoted no-result line, so it reads in the
                        // interface language instead of untranslated English.
                        let body = match e.strip_prefix("We couldn't find what you were looking for: ") {
                            Some(q) => lang.tr("Nothing found for “{}”").replace("{}", q),
                            None => crate::clean_error(&e),
                        };
                        app.message = Some((lang.tr("Find and Replace").to_string(), body));
                    }
                }
                app.grid.ensure_visible = true;
            }
        }
    });
    let t = crate::theme::Tokens::get(ui.ctx());
    if d.values.get("notFound").and_then(Json::as_bool) == Some(true) {
        ui.separator();
        // A query was searched: quote it; an empty query has nothing to quote, so the plain line says it.
        match d.values.get("foundFor").and_then(Json::as_str) {
            Some(q) => ui.colored_label(t.danger, lang.tr("Nothing found for “{}”").replace("{}", q)),
            None => ui.colored_label(t.danger, lang.tr("Nothing found")),
        };
    } else if let Some(r) = &d.result
        && let Some(list) = r["results"].as_array()
    {
        ui.separator();
        ui.label(lang.tr("{count} cell(s) found").replace("{count}", &list.len().to_string()));
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for item in list {
                let label = format!(
                    "{}  {}  {}",
                    item["sheet"].as_str().unwrap_or(""),
                    item["cell"].as_str().unwrap_or(""),
                    item["value"].as_str().unwrap_or("")
                );
                if ui.selectable_label(false, label).clicked() {
                    if let Some(s) = item["sheet"].as_str() {
                        let _ = app.run("sheet.activate", json!({"sheet": s}));
                    }
                    let _ = app.run("selection.set", json!({"cell": item["cell"]}));
                    app.grid.ensure_visible = true;
                }
            }
        });
    }
}

/// What the Go To dialog lists: the named ranges of every sheet, then the references gone to
/// before, newest first.
pub(super) fn go_to_entries(app: &SheetApp) -> Vec<String> {
    app.session
        .active()
        .map(|doc| {
            let mut v = gridcraft_engine::cmd::edit::go_to_names(&doc.wb, doc.wb.active_sheet());
            v.extend(doc.go_to_recent.iter().cloned());
            v
        })
        .unwrap_or_default()
}

/// Draws a virtualized name/reference list and editable reference field, links to Go To Special, and confirms with `edit.goTo`.
pub(super) fn go_to(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let lang = app.ui.language;
    let entries: Vec<String> =
        d.result.as_ref().and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(str::to_owned).collect()).unwrap_or_default();
    let current = d.values.get("reference").and_then(Json::as_str).unwrap_or("").to_string();
    // A workbook can hold thousands of names: only the rows in view are laid out.
    let row_height = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    egui::ScrollArea::vertical().max_height(160.0).auto_shrink([false, true]).show_rows(ui, row_height, entries.len(), |ui, rows| {
        for n in entries.get(rows).unwrap_or_default() {
            let resp = ui.selectable_label(current == *n, n);
            if resp.clicked() || resp.double_clicked() {
                d.values.insert("reference".into(), json!(n));
            }
            if resp.double_clicked() {
                *confirm = true;
            }
        }
    });
    let mut r = d.values.get("reference").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        let label = ui.label(lang.tr("Reference:"));
        ui.text_edit_singleline(&mut r).labelled_by(label.id).request_focus();
    });
    d.values.insert("reference".into(), json!(r.clone()));
    if ui.button("Special…").clicked() {
        app.open_dialog("goToSpecial", json!({}));
        return;
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm {
        app.run_or_alert("edit.goTo", json!({"reference": r, "record": true}));
        app.grid.ensure_visible = true;
    }
    if !open {
        *confirm = true;
    }
}
