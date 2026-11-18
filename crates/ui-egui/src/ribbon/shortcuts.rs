use super::{Key, Modifiers, SheetApp, json};

/// The command a ribbon shortcut runs for this key, when that does not depend on the selection.
/// Insert and delete stay out: those pick a command from the selection, and each branch checks
/// the binding itself.
pub(crate) fn bound_command(key: Key, m: Modifiers) -> Option<&'static str> {
    let shift = m.shift;
    Some(match key {
        Key::B => "home.bold",
        Key::I => "home.italic",
        Key::U => "home.underline",
        Key::Z if shift => "edit.redo",
        Key::Z => "edit.undo",
        Key::Y => "edit.redo",
        Key::A => "edit.selectAll",
        Key::D => "edit.fillDown",
        Key::R => "edit.fillRight",
        Key::S if shift => "file.saveAs",
        Key::S => "file.save",
        Key::O => "file.open",
        Key::N => "file.new",
        Key::W => "file.close",
        Key::P => "file.print",
        Key::K => "insert.link",
        Key::F if shift => "data.filter",
        Key::F => "edit.find",
        Key::H => "edit.replace",
        Key::G => "edit.goTo",
        Key::T if shift => "formulas.autoSum",
        Key::T => "insert.table",
        Key::E => "edit.flashFill",
        Key::Num1 => "home.formatCells",
        Key::Num9 if shift => "home.unhideRows",
        Key::Num9 => "home.hideRows",
        Key::Num0 if shift => "home.unhideColumns",
        Key::Num0 => "home.hideColumns",
        Key::Backtick => "formulas.showFormulas",
        _ => return None,
    })
}

/// Command-key shortcuts when the grid has focus.
pub fn shortcut(app: &mut SheetApp, key: Key, m: Modifiers) {
    // A command the user moved no longer answers this chord. Its new chord is handled first.
    if bound_command(key, m).is_some_and(|id| app.ui.command_hotkeys.contains_key(id)) {
        return;
    }
    let shift = m.shift;
    let id: Option<(&str, serde_json::Value)> = match key {
        Key::B => Some(("home.bold", json!({}))),
        Key::I => Some(("home.italic", json!({}))),
        Key::U => Some(("home.underline", json!({}))),
        Key::Z if shift => Some(("edit.redo", json!({}))),
        Key::Z => Some(("edit.undo", json!({}))),
        Key::Y => Some(("edit.redo", json!({}))),
        Key::A => Some(("edit.selectAll", json!({}))),
        Key::D => Some(("edit.fillDown", json!({}))),
        Key::R => Some(("edit.fillRight", json!({}))),
        Key::S if shift => Some(("file.saveAs", json!({}))),
        Key::S => Some(("file.save", json!({}))),
        Key::O => Some(("file.open", json!({}))),
        Key::N => Some(("file.new", json!({}))),
        Key::W => Some(("file.close", json!({}))),
        Key::P => {
            app.print_now(json!({}));
            None
        }
        Key::K => {
            app.open_dialog("insertLink", json!({}));
            None
        }
        Key::F if shift => Some(("data.filter", json!({}))),
        Key::F => {
            app.open_dialog("find", json!({}));
            None
        }
        Key::H => {
            app.open_dialog("find", json!({"replace": true}));
            None
        }
        Key::G => {
            app.open_dialog("goTo", json!({}));
            None
        }
        Key::T if shift => Some(("formulas.autoSum", json!({}))),
        Key::T => Some(("insert.table", json!({}))),
        Key::E => Some(("edit.flashFill", json!({}))),
        Key::Num1 => {
            app.open_dialog("formatCells", json!({}));
            None
        }
        Key::Num9 if shift => Some(("home.unhideRows", json!({}))),
        Key::Num9 => Some(("home.hideRows", json!({}))),
        Key::Num0 if shift => Some(("home.unhideColumns", json!({}))),
        Key::Num0 => Some(("home.hideColumns", json!({}))),
        // As in Excel: with whole rows or whole columns selected, Ctrl+Shift+= / Ctrl+- insert or
        // delete them at once; any other selection asks which way to shift.
        Key::Minus => match whole_lines(app) {
            Some(true) => Some(("home.deleteRows", json!({}))),
            Some(false) => Some(("home.deleteColumns", json!({}))),
            None => {
                app.open_dialog("deleteCells", json!({}));
                None
            }
        },
        // Insert works on one area at a time, so several areas still go through the dialog.
        // The numpad's + arrives as Plus without Shift; Excel takes Ctrl with it as well.
        Key::Plus | Key::Equals if shift || key == Key::Plus => match whole_lines(app).filter(|_| single_area(app)) {
            Some(true) => Some(("home.insertRows", json!({}))),
            Some(false) => Some(("home.insertColumns", json!({}))),
            None => {
                app.open_dialog("insertCells", json!({}));
                None
            }
        },
        Key::Semicolon => {
            // Insert today's date.
            let text = app
                .session
                .active()
                .map(|d| {
                    let today = gridcraft_engine::core::date::serial_in_system(d.wb.date_system, gridcraft_engine::calc::now_serial()).floor();
                    gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(today), "m/d/yyyy", &d.wb).text
                })
                .unwrap_or_default();
            app.begin_edit(Some(text), false);
            None
        }
        Key::Backtick => Some(("formulas.showFormulas", json!({}))),
        _ => None,
    };
    if let Some((id, p)) = id {
        // Insert and delete choose the command from the selection, so the check is here rather
        // than in `bound_command`.
        if !app.ui.command_hotkeys.contains_key(id) {
            app.run_or_alert(id, p);
            app.grid.ensure_visible = true;
        }
    }
}

/// Whether the selection is a single area.
fn single_area(app: &SheetApp) -> bool {
    app.session.active().is_some_and(|d| d.selection.ranges.len() == 1)
}

/// `Some(true)` when every selected area is whole rows, `Some(false)` when every one is whole
/// columns, `None` otherwise (including the whole sheet).
fn whole_lines(app: &SheetApp) -> Option<bool> {
    let sel = &app.session.active()?.selection;
    let rows = sel.ranges.iter().all(|r| r.is_full_rows() && !r.is_full_cols());
    let cols = sel.ranges.iter().all(|r| r.is_full_cols() && !r.is_full_rows());
    if rows {
        Some(true)
    } else if cols {
        Some(false)
    } else {
        None
    }
}
