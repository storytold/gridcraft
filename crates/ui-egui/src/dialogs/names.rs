use super::show::ok_cancel;
use super::{Dialog, Json, Key, SheetApp, json};

/// A defined name is a reusable custom function when its definition starts with `LAMBDA(`.
/// Keep this display-only check deliberately tolerant: damaged or future formulas still belong
/// in Name Manager, and their full text remains available through Edit.
fn is_lambda(n: &Json) -> bool {
    n.get("refersTo").and_then(Json::as_str).is_some_and(|f| {
        let f = f.trim_start_matches('=').trim_start();
        let f = if f.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("_xlfn.")) { &f[6..] } else { f };
        f.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("LAMBDA("))
    })
}

/// A single-line preview prevents a long named LAMBDA from widening the entire dialog. Its full
/// definition is available through the row's hover text and Edit dialog.
fn preview(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().nth(max).is_some() {
        out.push('…');
    }
    out
}

/// Renders the bounded, virtualized Name Manager list and the selected name's details.
pub(super) fn name_manager(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let list = app.session.run("formulas.nameManager", json!({})).unwrap_or(Json::Null);
    let names = list.as_array().cloned().unwrap_or_default();
    let selected = names.get(d.list_index).cloned();
    let field = |n: &Json, k: &str| n.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let lambda_count = names.iter().filter(|n| is_lambda(n)).count();
    ui.horizontal(|ui| {
        if ui.button("New…").clicked() {
            app.open_dialog("defineName", json!({}));
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new("Edit…")).clicked()
            && let Some(n) = &selected
        {
            app.open_dialog(
                "defineName",
                json!({
                    "name": field(n, "name"), "scope": field(n, "scope"), "comment": field(n, "comment"),
                    "refersTo": field(n, "refersTo"), "oldName": field(n, "name"), "oldScope": field(n, "scope"),
                }),
            );
        }
        if ui.add_enabled(selected.is_some(), egui::Button::new("Delete")).clicked()
            && let Some(n) = &selected
        {
            app.run_or_alert("formulas.deleteName", json!({"name": field(n, "name"), "scope": field(n, "scope")}));
        }
    });
    ui.add_space(4.0);
    if lambda_count > 0 {
        ui.label(format!("Custom functions: {lambda_count} LAMBDA name(s)"));
    }
    let row_height = ui.text_style_height(&egui::TextStyle::Button).max(ui.spacing().interact_size.y) + 2.0 * ui.spacing().button_padding.y;
    egui::ScrollArea::vertical().max_height(280.0).auto_shrink([false, true]).show_rows(ui, row_height, names.len() + 1, |ui, rows| {
        egui::Grid::new("names").striped(true).num_columns(4).show(ui, |ui| {
            if rows.contains(&0) {
                ui.strong("Name");
                ui.strong("Value");
                ui.strong("Refers To");
                ui.strong("Scope");
                ui.end_row();
            }
            if names.is_empty() && rows.contains(&0) {
                ui.label(egui::RichText::new("No names defined").italics());
                ui.end_row();
            }
            let start = rows.start.saturating_sub(1);
            let end = rows.end.saturating_sub(1).min(names.len());
            for (i, n) in names.iter().enumerate().skip(start).take(end.saturating_sub(start)) {
                if ui.add_sized([150.0, row_height], egui::Button::new(field(n, "name")).selected(i == d.list_index)).clicked() {
                    d.list_index = i;
                }
                let value = if is_lambda(n) { "LAMBDA".into() } else { preview(&n.get("value").map(|v| v.to_string()).unwrap_or_default(), 18) };
                ui.add_sized([72.0, row_height], egui::Label::new(value));
                let refers_to = field(n, "refersTo");
                ui.add_sized(
                    [260.0, row_height],
                    egui::Label::new(preview(&refers_to.replace(['\n', '\r'], " "), 48)).truncate().show_tooltip_when_elided(false),
                )
                .on_hover_text(refers_to);
                ui.add_sized([90.0, row_height], egui::Label::new(field(n, "scope")));
                ui.end_row();
            }
        });
    });
    if let Some(n) = selected {
        ui.add_space(6.0);
        ui.separator();
        ui.strong(format!("{}{}", field(&n, "name"), if is_lambda(&n) { " (custom LAMBDA function)" } else { "" }));
        let comment = field(&n, "comment");
        if !comment.is_empty() {
            ui.label(comment);
        }
        let refers_to = field(&n, "refersTo");
        ui.label(egui::RichText::new(preview(&refers_to, 180)).monospace()).on_hover_text(refers_to);
    }
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(" Close ").clicked() {
            *open = false;
        }
    });
}

/// The names Paste Names offers in the active workbook.
pub(super) fn pasteable_names(app: &SheetApp) -> Vec<String> {
    app.session
        .active()
        .map(|doc| gridcraft_engine::cmd::formulas::pasteable_names(&doc.wb).into_iter().map(|(n, _)| n).collect())
        .unwrap_or_default()
}

/// Puts a defined name into the formula being edited, at the caret. Without an edit under way it
/// starts one in the active cell with `=` and the name.
pub(crate) fn insert_name(app: &mut SheetApp, name: &str) {
    match app.editor.as_mut() {
        Some(ed) => ed.insert_at_caret(name),
        None => app.begin_edit(Some(format!("={name}")), false),
    }
}

/// Paste Name (F3, Formulas › Use in Formula › Paste Names…): OK, or a double-click, inserts the
/// selected name (`formulas.pasteName` with `name`); Paste List writes every name and what it
/// refers to from the active cell.
pub(super) fn paste_name(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let names: Vec<String> =
        d.result.as_ref().and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_str).map(str::to_owned).collect()).unwrap_or_default();
    // The `name` value is the selection, so `ui.dialog.set` can choose one too.
    if let Some(i) = d.values.get("name").and_then(Json::as_str).and_then(|n| names.iter().position(|x| x == n)) {
        d.list_index = i;
    }
    let (down, up) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)));
    let last = names.len().saturating_sub(1);
    if down {
        d.list_index = (d.list_index + 1).min(last);
    }
    if up {
        d.list_index = d.list_index.saturating_sub(1);
    }
    d.list_index = d.list_index.min(last);
    ui.label(app.ui.language.tr("Paste name"));
    if names.is_empty() {
        ui.label(egui::RichText::new(app.ui.language.tr("No names defined")).italics());
    }
    // A workbook can hold thousands of names: only the rows in view are laid out.
    let row_height = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    let step = row_height + ui.spacing().item_spacing.y;
    egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show_rows(ui, row_height, names.len(), |ui, rows| {
        let top = ui.cursor().top() - rows.start as f32 * step;
        for (i, n) in names.iter().enumerate().skip(rows.start).take(rows.len()) {
            let r = ui.selectable_label(i == d.list_index, n);
            if r.clicked() {
                d.list_index = i;
            }
            if r.double_clicked() {
                *confirm = true;
            }
        }
        // The arrow keys can move the selection onto a row that isn't laid out.
        if down || up {
            let rect = egui::Rect::from_min_size(egui::pos2(ui.min_rect().left(), top + d.list_index as f32 * step), egui::vec2(1.0, row_height));
            if !ui.clip_rect().contains_rect(rect) {
                ui.scroll_to_rect(rect, Some(if down { egui::Align::Max } else { egui::Align::Min }));
            }
        }
    });
    ui.add_space(4.0);
    // Paste List is disabled while a cell is being edited.
    if ui.add_enabled(!names.is_empty() && app.editor.is_none(), egui::Button::new(app.ui.language.tr("Paste List"))).clicked() {
        app.run_or_alert("formulas.pasteName", json!({"list": true}));
        *open = false;
        return;
    }
    if let Some(n) = names.get(d.list_index) {
        d.values.insert("name".into(), json!(n));
    }
    ok_cancel(ui, confirm, open);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_imported_lambda_names() {
        assert!(is_lambda(&json!({"refersTo": "=_xlfn.LAMBDA(_xlpm.x,_xlpm.x)"})), "file spelling is still a LAMBDA");
        assert!(is_lambda(&json!({"refersTo": "=LAMBDA(x,x)"})));
        assert!(!is_lambda(&json!({"refersTo": "=SUM(A1:A2)"})));
    }

    #[test]
    fn previews_are_bounded_without_losing_short_text() {
        assert_eq!(preview("LAMBDA(x,x)", 20), "LAMBDA(x,x)");
        assert_eq!(preview("LAMBDA(x,x)", 6), "LAMBDA…");
    }
}
