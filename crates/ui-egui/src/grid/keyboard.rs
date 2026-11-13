use super::geo::Geo;
use super::menus::is_checkbox;
use super::{CellRef, Drag, Sheet, SheetApp, json};

/// Grid keyboard handling when no text field has focus.
pub(super) fn keyboard(app: &mut SheetApp, ctx: &egui::Context, resp: &egui::Response, geo: &Geo, sh: &Sheet) {
    // Focus left on an editor that no longer exists goes back to the grid.
    let stale = [egui::Id::unique("gridcraft.cell_editor"), egui::Id::unique("gridcraft.formula_bar"), crate::text_box::editor_id()];
    let editing = app.editor.is_some() || app.text_box_editor.is_some();
    if !editing && ctx.memory(|m| m.focused()).is_some_and(|f| stale.contains(&f)) {
        resp.request_focus();
    }
    let other_focus = ctx.memory(|m| m.focused()).is_some_and(|f| f != resp.id && (editing || !stale.contains(&f)));
    // An open filter menu owns the keyboard even when a click inside it left focus on nothing (#1516).
    // So do the fill menu (its Esc closes it, not the copy) and the right-drag that opens it (an
    // arrow key would move the selection the menu fills from), and a macro's question.
    if editing
        || app.dialog.is_some()
        || other_focus
        || app.message.is_some()
        || app.grid.filter_menu.is_some()
        || app.grid.fill_menu.is_some()
        || matches!(app.grid.drag, Drag::Fill { menu: true, .. })
        || app.macros.question.is_some()
    {
        return;
    }
    if ctx.memory(|m| m.focused()).is_none() {
        resp.request_focus();
    }
    // The grid owns arrows, Tab and Escape (egui would otherwise move focus between widgets).
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(resp.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
    });
    request_grid_ime(app, ctx, resp, geo, sh);
    let events = grid_events(app, ctx, sh);
    // A shortcut key that also types a character (Shift+Space sends Text(" ")) must not start an edit.
    let shortcut_pressed = events.iter().any(|e| {
        matches!(e, egui::Event::Key { pressed: true, modifiers: m, key, .. }
            if !m.alt && (m.ctrl || m.command || (m.shift && *key == egui::Key::Space)))
    });
    let page = ((geo.cells.height() / geo.z) / sh.default_row_height).floor().max(1.0) as i64;
    // Alt shortcuts type their character too on Windows and Linux (Alt+= sends Text("=")): once
    // a shortcut ran, that text must not start an edit over what it just did.
    let mut shortcut_ran = false;
    for ev in events {
        // IME events are handled outside the match to keep this loop's complexity down. Any
        // non-commit IME event (preedit, delete-surrounding) falls through to `_ => {}` below.
        if handle_ime_commit(app, &ev, shortcut_pressed, shortcut_ran) {
            return;
        }
        match ev {
            egui::Event::Copy => {
                app.copy_to_clipboard(ctx, "edit.copy");
            }
            egui::Event::Cut => {
                app.copy_to_clipboard(ctx, "edit.cut");
            }
            egui::Event::Paste(text) => app.paste_external(text),
            // A picture with no text on the clipboard (a screenshot, a browser image) is pasted as
            // a floating picture at the active cell, as Excel does.
            egui::Event::PasteImage(image) => app.paste_color_image(&image),
            // PRIMARY middle-click paste belongs to egui's TextEdit; the grid has no caret to
            // insert text at, so a middle click on a cell is deliberately not a paste.
            egui::Event::MiddleClickPaste { .. } => {}
            egui::Event::Text(_) if shortcut_pressed || shortcut_ran => {}
            egui::Event::Text(text) if text.chars().all(|c| !c.is_control()) && !text.is_empty() => {
                seed_editor(app, text);
                return;
            }
            egui::Event::Key { key, physical_key, pressed: true, modifiers: m, .. } => {
                use egui::Key;
                if crate::keymap::dispatch(app, ctx, key, physical_key, m, geo, sh) {
                    shortcut_ran = true;
                    continue;
                }
                let cmd = m.command;
                let shift = m.shift;
                let mv = |app: &mut SheetApp, dr: i64, dc: i64| {
                    let from = app.session.active().map(|d| if shift { d.selection.moving_end() } else { d.selection.active });
                    let _ = app.session.run("selection.move", json!({"dr": dr, "dc": dc, "extend": shift, "jump": cmd}));
                    app.grid.ensure_visible = true;
                    app.grid.reveal_moving_end = shift;
                    app.grid.reveal_from = from;
                };
                match key {
                    Key::ArrowUp => mv(app, -1, 0),
                    Key::ArrowDown => mv(app, 1, 0),
                    // Arrows follow the screen: on a right-to-left sheet Left goes to the next column.
                    Key::ArrowLeft => mv(app, 0, if sh.right_to_left { 1 } else { -1 }),
                    Key::ArrowRight => mv(app, 0, if sh.right_to_left { -1 } else { 1 }),
                    Key::PageDown if !cmd => mv(app, page, 0),
                    Key::PageUp if !cmd => mv(app, -page, 0),
                    Key::PageDown => app.run_or_alert("sheet.next", json!({})),
                    Key::PageUp => app.run_or_alert("sheet.previous", json!({})),
                    Key::Home if cmd => {
                        let r = sh.freeze.map(|f| f.0).unwrap_or(0);
                        let c = sh.freeze.map(|f| f.1).unwrap_or(0);
                        let _ = app.session.run("selection.set", json!({"cell": CellRef::new(r, c).a1()}));
                        app.grid.ensure_visible = true;
                    }
                    Key::End if cmd => {
                        let end = sh.used_range().map(|u| u.end).unwrap_or_default();
                        let _ = app.session.run("selection.set", json!({"cell": end.a1()}));
                        app.grid.ensure_visible = true;
                    }
                    Key::Home => {
                        let a = app.session.active().map(|d| d.selection.active).unwrap_or_default();
                        let _ = app.session.run("selection.set", json!({"cell": CellRef::new(a.row, 0).a1()}));
                        app.grid.ensure_visible = true;
                    }
                    // Excel: Enter while a copied range is marked pastes it and ends copy mode.
                    Key::Enter if !m.alt && !m.ctrl && !shift && !cmd && app.session.clipboard.is_some() => {
                        app.run_or_alert("edit.paste", json!({}));
                        app.session.clipboard = None;
                        app.grid.ensure_visible = true;
                    }
                    Key::Enter => {
                        if m.alt || m.ctrl {
                            app.begin_edit(None, false);
                        } else {
                            app.move_after_enter(if shift { -1 } else { 1 }, 0);
                        }
                    }
                    Key::Tab => app.move_after_enter(0, if shift { -1 } else { 1 }),
                    Key::F2 => app.begin_edit(None, false),
                    // With an object selected, Delete and Backspace delete it, not the cells.
                    Key::Delete if app.delete_selected_object() => {}
                    Key::Delete => app.run_or_alert("edit.clearContents", json!({})),
                    Key::Backspace if app.delete_selected_object() => {}
                    Key::Backspace => {
                        // Excel for Mac: Delete clears the cell and starts editing it.
                        app.run_or_alert("edit.clearContents", json!({}));
                        if app.session.active().is_some_and(|d| d.selection.is_single_cell()) {
                            app.begin_edit(Some(String::new()), false);
                        }
                    }
                    Key::Escape if app.selected_object.is_some() => app.selected_object = None,
                    Key::Escape => {
                        app.session.clipboard = None;
                        app.session.format_painter = None;
                    }
                    Key::Space if !cmd && !shift && is_checkbox(sh, app) => app.run_or_alert("cell.toggleCheckbox", json!({})),
                    Key::F8 if m.alt => {
                        app.open_dialog("macros", json!({}));
                    }
                    Key::F9 => app.run_or_alert("formulas.calculateNow", json!({})),
                    Key::F4 if !cmd => {}
                    // Ctrl+Shift+letter (⌘⇧letter) runs the macro with that shortcut, unless GridCraft uses it.
                    _ if cmd && shift && !m.alt && crate::macro_ui::run_shortcut(app, key) => {}
                    _ if cmd && !crate::keymap::altgr_text(ctx, m, ctx.os() == egui::os::OperatingSystem::Mac) => {
                        crate::ribbon::shortcut(app, key, m)
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Open the in-cell editor with typed or committed text and put the caret after it.
fn seed_editor(app: &mut SheetApp, text: String) {
    app.begin_edit(Some(text), false);
    if let Some(ed) = app.editor.as_mut() {
        ed.caret = ed.text.chars().count();
    }
}

/// Seed the editor from a finished composition, if `ev` is one.
///
/// Only `ImeEvent::Commit` is finished text; a preedit is the input method's in-progress
/// spelling and must not seed the editor. `Event::Ime` and `Event::Text` are mutually
/// exclusive for one keystroke, so accepting a commit cannot double-insert a character.
/// Returns whether the caller should stop handling this frame's events.
fn handle_ime_commit(app: &mut SheetApp, ev: &egui::Event, shortcut_pressed: bool, shortcut_ran: bool) -> bool {
    let egui::Event::Ime(egui::ImeEvent::Commit(text)) = ev else { return false };
    if shortcut_pressed || shortcut_ran || text.is_empty() {
        return false;
    }
    seed_editor(app, text.clone());
    true
}

/// Ask the OS to route keystrokes through the input method while the grid owns keyboard input.
///
/// egui enables IME only for a widget that publishes `PlatformOutput::ime` in that frame. The
/// in-cell `TextEdit` opens *after* the grid has consumed the first keystroke, so without this
/// the first key of a CJK composition is delivered as raw `Event::Text` and lands in the cell
/// (Wubi `xta` writes `x` and the input method only ever sees `ta`). The request must be
/// published on every focused frame, not in response to a key: winit applies it at the end of
/// the frame, so a key arriving while IME is still off cannot be recovered.
///
/// `PlatformOutput` is rebuilt every frame, so returning early (an editor, a dialog or another
/// widget taking focus) simply leaves `ime` as `None` and the input method turns off.
fn request_grid_ime(app: &SheetApp, ctx: &egui::Context, resp: &egui::Response, geo: &Geo, sh: &Sheet) {
    if !ctx.memory(|m| m.owns_ime_events(resp.id)) {
        return;
    }
    let Some(d) = app.session.active() else { return };
    let active = d.selection.active;
    let merged = sh.merge_at(active);
    // `begin_edit` refuses a locked cell on a protected sheet. Publishing an IME request for
    // one would let the user compose text that is then discarded, so skip it up front, using
    // the same merged-cell anchor and protection check as `SheetApp::begin_edit`.
    let anchor = merged.map(|m| m.start).unwrap_or(active);
    if app.session.locked_cell_message(d.wb.active_sheet(), anchor).is_some() {
        return;
    }
    // `cell_rect`/`range_rect` are logical coordinates, but `IMEOutput` needs screen ones.
    // On a right-to-left sheet the grid is mirrored, so publishing the logical rect puts the
    // candidate window on the opposite side. After mirroring, `rect.min` is the screen-side
    // start of the cell in both layouts, which is where the caret belongs.
    let rect = match merged {
        Some(m) => geo.mirror_rect(geo.range_rect(sh, m)),
        None => geo.mirror_rect(geo.cell_rect(sh, active)),
    };
    let cursor_rect = egui::Rect::from_min_size(rect.min, egui::vec2(1.0, rect.height()));
    // egui 0.37 identifies an IME context by (owner, generation). The generation must change
    // when this owner gains focus and stay stable for ordinary text and selection updates.
    let gained_focus = resp.gained_focus();
    let generation = ctx.data_mut(|data| {
        let g: &mut u64 = data.get_temp_mut_or_default(egui::Id::unique("gridcraft.grid_ime_generation"));
        if gained_focus || *g == 0 {
            *g = g.wrapping_add(1);
        }
        *g
    });
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput::new(resp.id, generation, egui::IMEPurpose::Normal, rect, cursor_rect));
    });
}

/// This frame's input events, without those the focused form control took.
fn grid_events(app: &mut SheetApp, ctx: &egui::Context, sh: &Sheet) -> Vec<egui::Event> {
    let events = ctx.input(|i| i.events.clone());
    events.into_iter().filter(|ev| !form_control_event(app, sh, ev)).collect()
}

/// Whether the focused form control takes `ev`: arrows, Page Up and Down, Home, End, Space and
/// Enter without Ctrl, Cmd or Alt, and the space a Space press types when it must not start a
/// cell edit: for a button, which Space presses, and on a checkbox cell, which the Space key
/// toggles (a slider or spin button doesn't take Space, so Space on a cell does what it does with
/// no control focused: it toggles a checkbox cell, or its space starts a cell edit like any other
/// character). A key press the control doesn't take goes to the grid, and a focused button gives
/// the keyboard back with it.
fn form_control_event(app: &mut SheetApp, sh: &Sheet, ev: &egui::Event) -> bool {
    match ev {
        egui::Event::Text(text) => {
            text == " "
                && app.selected_object.is_none()
                && app.grid.focused_control.is_some()
                && (crate::form_controls::focused_button(app, sh) || is_checkbox(sh, app))
        }
        egui::Event::Key { key, pressed: true, repeat, modifiers: m, .. } => {
            if !m.command && !m.ctrl && !m.alt && crate::form_controls::key(app, sh, *key, *repeat) {
                return true;
            }
            crate::form_controls::key_to_grid(app, sh);
            false
        }
        _ => false,
    }
}
