use super::show::ok_cancel;
use super::*;

/// Draws the Find/Replace form with case, whole-cell and workbook options, requests field focus once, and runs find/replace with the current parameters.
pub(super) fn find(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.horizontal(|ui| {
        for tab in ["Find", "Replace"] {
            if ui.selectable_label(d.tab == tab, tab).clicked() {
                d.tab = tab.into();
            }
        }
    });
    let mut what = d.values.get("what").and_then(Json::as_str).unwrap_or("").to_string();
    let mut with = d.values.get("with").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        let label = ui.label("Find what:");
        let r = ui.add(egui::TextEdit::singleline(&mut what).desired_width(260.0)).labelled_by(label.id);
        // Like Excel, the cursor starts in "Find what" - asked for once, so clicking another field still works.
        if !d.values.contains_key("focused") {
            r.request_focus();
            d.values.insert("focused".into(), json!(true));
        }
    });
    if d.tab == "Replace" {
        ui.horizontal(|ui| {
            let label = ui.label("Replace with:");
            ui.add(egui::TextEdit::singleline(&mut with).desired_width(244.0)).labelled_by(label.id);
        });
    }
    let mut case = d.values.get("matchCase").and_then(Json::as_bool).unwrap_or(false);
    let mut whole = d.values.get("wholeCell").and_then(Json::as_bool).unwrap_or(false);
    let mut wb = d.values.get("within").and_then(Json::as_str) == Some("workbook");
    ui.horizontal(|ui| {
        ui.checkbox(&mut case, "Match case");
        ui.checkbox(&mut whole, "Match entire cell contents");
        ui.checkbox(&mut wb, "Within workbook");
    });
    d.values.insert("what".into(), json!(what));
    d.values.insert("with".into(), json!(with));
    d.values.insert("matchCase".into(), json!(case));
    d.values.insert("wholeCell".into(), json!(whole));
    d.values.insert("within".into(), json!(if wb { "workbook" } else { "sheet" }));
    let base = json!({"what": what, "matchCase": case, "wholeCell": whole, "within": if wb { "workbook" } else { "sheet" }});
    ui.horizontal(|ui| {
        if d.tab == "Replace" {
            if ui.button("Replace All").clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(true);
                match app.run("edit.replace", p) {
                    Ok(r) => app.toast = Some((format!("All done. We made {} replacements.", r["replaced"]), crate::now_ms())),
                    Err(e) => app.message = Some(("Find and Replace".into(), crate::clean_error(&e))),
                }
            }
            if ui.button("Replace").clicked() {
                let mut p = base.clone();
                p["with"] = json!(with);
                p["all"] = json!(false);
                let _ = app.run("edit.replace", p);
                let _ = app.run("edit.find", base.clone());
            }
        }
        if ui.button("Find All").clicked() {
            let mut p = base.clone();
            p["all"] = json!(true);
            d.result = app.run("edit.find", p).ok();
        }
        if ui.button("Find Next").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Err(e) = app.run("edit.find", base.clone()) {
                app.message = Some(("Find and Replace".into(), crate::clean_error(&e)));
            }
            app.grid.ensure_visible = true;
        }
    });
    if let Some(r) = &d.result
        && let Some(list) = r["results"].as_array()
    {
        ui.separator();
        ui.label(format!("{} cell(s) found", list.len()));
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
