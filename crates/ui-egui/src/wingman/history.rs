//! Wingman uses the workbook's own history, not transcript replay or inverse commands.
use crate::SheetApp;
use egui::{Key, Modifiers, Ui};
use gridcraft_engine::find_command;
use serde_json::{Value, json};
use std::sync::Arc;

/// All steps execute synchronously after one approval. No group spans a model request.
pub(super) fn transaction(
    app: &mut SheetApp,
    label: &str,
    apply: impl FnOnce(&mut gridcraft_engine::Session) -> gridcraft_engine::Result<Value>,
) -> Result<(Value, Option<String>), String> {
    let before = app.session.active().ok_or("No open workbook")?.wb.clone();
    let value = app.run_command_using("wingman.editPlan", Value::Null, |s, _, _| {
        let d = s.active().ok_or("No open workbook")?;
        if s.in_group(d.uid) {
            return Err("Finish the open workbook action before applying a Wingman edit.".into());
        }
        let remembered = d.sheet_selections.clone();
        let queued = s.ui_requests.len();
        let result = s.grouped(label, apply);
        if result.is_err() {
            s.ui_requests.truncate(queued);
            if let Some(d) = s.active_mut() {
                d.sheet_selections = remembered;
            }
        }
        result.map_err(|e| e.to_string())
    })?;
    let undo = app.session.active().filter(|d| !Arc::ptr_eq(&before, &d.wb)).and_then(|d| d.undo.last()).map(|e| e.label.clone());
    Ok((value, undo))
}

pub(super) fn edit(app: &mut SheetApp, id: &'static str, args: Value) -> Result<(Value, Option<String>), String> {
    let before = app.session.active().ok_or("No open workbook")?.wb.clone();
    let result = app.run_command_using(id, args, |s, id, args| {
        let cmd = find_command(id).filter(|c| c.undoable).ok_or("This command cannot create a workbook undo step.")?;
        let doc = s.active().ok_or("No open workbook")?;
        if s.in_group(doc.uid) {
            return Err("Finish the open workbook action before applying a Wingman edit.".into());
        }
        // Open and close inside this one approval; never hold a group across network or UI turns.
        // The engine restores both the workbook and its redo stack when a command fails.
        s.grouped(&format!("Wingman: {}", cmd.label), |s| s.execute(id, args)).map_err(|e| e.to_string())
    })?;
    let undo_label = app.session.active().filter(|d| !Arc::ptr_eq(&before, &d.wb)).and_then(|d| d.undo.last()).map(|entry| entry.label.clone());
    Ok((result, undo_label))
}

pub(super) fn shortcuts(app: &mut SheetApp, ui: &Ui, composer: &super::composer::Composer) {
    let ctx = ui.ctx();
    if !ui.input(|i| i.key_pressed(Key::Z) || i.key_pressed(Key::Y)) {
        return;
    }
    let Some(mut id) = ctx.memory(|m| m.focused()) else { return };
    let sent = id == super::composer::Composer::id()
        && composer.workbook_owns_undo()
        && !ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_) | egui::Event::Text(_) | egui::Event::Ime(_))));
    if ((id == super::composer::Composer::id() || ctx.text_edit_focused()) && !sent)
        || app.editor.is_some()
        || app.text_box_editor.is_some()
        || app.message.is_some()
        || app.macros.question.is_some()
        || app.dialog.as_ref().is_some_and(|d| d.name != "macroEditor")
        || egui::Popup::is_any_open(ctx)
    {
        return;
    }
    // Only claim keys on controls in this dock. A composer, cell, macro text editor, another
    // pane, or the grid keeps its own keyboard handling. Parent IDs do not depend on position.
    for _ in 0..64 {
        if id == ui.unique_id() {
            let cmd = ctx.input_mut(|i| {
                if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y) {
                    Some("edit.redo")
                } else if i.consume_key(Modifiers::COMMAND, Key::Z) {
                    Some("edit.undo")
                } else {
                    None
                }
            });
            if let Some(cmd) = cmd {
                match super::script_editor::history(app, cmd, true) {
                    Some(Err(e)) => app.alert(e),
                    Some(Ok(_)) => {}
                    None => app.run_or_alert(cmd, json!({})),
                }
            }
            break;
        }
        let Some(widget) = ctx.widget_rect_last_pass(id) else { break };
        if widget.parent_id == id {
            break;
        }
        id = widget.parent_id;
    }
}

#[cfg(test)]
mod tests {
    use crate::wingman::workbook::execute;
    use crate::{Services, SheetApp};
    use gridcraft_engine::Session;
    use serde_json::json;

    #[test]
    fn navigation_on_a_read_only_workbook_obeys_native_selection_protection() {
        use gridcraft_engine::model::SheetProtection;
        let mut s = Session::new();
        s.new_workbook();
        let d = s.active_mut().unwrap();
        d.read_only_source = Some("book.xlsx".into());
        let sh = std::sync::Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap();
        let sheet = sh.name.clone();
        sh.protection = Some(SheetProtection { select_locked: true, ..Default::default() });
        let mut app = SheetApp::new(s, Services::default());
        let args = json!({"sheet":sheet,"range":"B2"});
        assert!(execute(&mut app, "select_range", &args, false).is_err());
        execute(&mut app, "select_range", &args, true).unwrap();
        let wb = std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().wb);
        wb.sheet_mut(0).unwrap().protection.as_mut().unwrap().select_locked = false;
        assert!(execute(&mut app, "select_range", &json!({"sheet":sheet,"range":"C3"}), true).is_err());
        assert_eq!(app.session.active().unwrap().selection.active.a1(), "B2");
    }

    #[test]
    fn moving_a_note_requires_approval_and_undo_restores_it_along_with_the_header() {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let cell = |app: &mut SheetApp, at: &str| execute(app, "inspect_cell", &json!({"sheet":sheet,"cell":at}), false).unwrap();
        let note = "=1+1\nkeep this as text";
        execute(&mut app, "set_note", &json!({"sheet":sheet,"cell":"AB9","text":note}), true).unwrap();
        let args = json!({"sheet":sheet,"sourceColumn":"AB","destinationColumn":"AC","headerText":"xyz","mode":"move"});
        assert!(execute(&mut app, "comments_to_column", &args, false).is_err());
        let original = cell(&mut app, "AB9")["comment"].clone();
        assert_eq!(original["text"], note);
        execute(&mut app, "comments_to_column", &args, true).unwrap();
        let extracted = cell(&mut app, "AC9");
        assert_eq!(extracted["value"], note);
        assert!(extracted.get("formula").is_none());
        assert!(cell(&mut app, "AB9")["comment"].is_null());
        assert_eq!(cell(&mut app, "AC1")["value"], "xyz");
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(cell(&mut app, "AB9")["comment"], original);
        assert!(cell(&mut app, "AC9")["value"].is_null());
        assert!(cell(&mut app, "AC1")["value"].is_null());
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(cell(&mut app, "AC9")["value"], note);
    }

    #[test]
    fn an_approved_matrix_round_trips_as_one_native_step_among_manual_edits() {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let value = |app: &mut SheetApp, at: &str| execute(app, "inspect_cell", &json!({"sheet":sheet,"cell":at}), false).unwrap()["value"].clone();
        app.run("cell.set", json!({"cell":"A1","input":"original"})).unwrap();
        let args = json!({"sheet":sheet,"range":"A1:B1","values":[["reviewed",13]]});
        execute(&mut app, "set_values", &args, true).unwrap();
        let label = app.run("edit.undo", json!({})).unwrap();
        assert!(label["undone"].as_str().unwrap().starts_with("Wingman:"));
        assert_eq!(value(&mut app, "A1"), "original");
        assert!(value(&mut app, "B1").is_null());
        // A rejected call neither consumes the native redo step nor joins another editor's group.
        assert!(execute(&mut app, "set_values", &args, false).is_err());
        let open = app.session.group_begin("Manual action").unwrap();
        assert!(execute(&mut app, "set_values", &args, true).is_err());
        app.session.group_end(open).unwrap();
        assert_eq!(app.run("edit.redo", json!({})).unwrap()["redone"], label["undone"]);
        assert_eq!(value(&mut app, "A1"), "reviewed");
        assert_eq!(value(&mut app, "B1"), 13.0);
    }
}
