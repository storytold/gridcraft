use super::geo::Geo;
use super::menus::is_checkbox;
use super::{CellRef, Drag, Sheet, SheetApp, json};

/// Focuses the grid's Ready-mode cell IME target (`cell_editor_id`).
pub(super) fn focus_grid(ctx: &egui::Context) {
    ctx.memory_mut(|m| m.request_focus(crate::formula_bar::cell_editor_id()));
}

/// Grid keyboard handling when no text field has focus.
pub(super) fn keyboard(app: &mut SheetApp, ctx: &egui::Context, resp: &egui::Response, geo: &Geo, sh: &Sheet) {
    let cell_id = crate::formula_bar::cell_editor_id();
    // Focus left on an editor that no longer exists goes back to the grid.
    let stale = [egui::Id::unique("gridcraft.formula_bar"), crate::text_box::editor_id(), resp.id];
    let editing = app.editor.is_some() || app.text_box_editor.is_some();
    if !editing && ctx.memory(|m| m.focused()).is_some_and(|f| stale.contains(&f)) {
        focus_grid(ctx);
    }
    let other_focus = ctx.memory(|m| m.focused()).is_some_and(|f| f != cell_id && (editing || !stale.contains(&f)));
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
        app.grid.ime_ready_cell = None;
        return;
    }
    if ctx.memory(|m| m.focused()).is_none() {
        focus_grid(ctx);
    }
    // The grid owns arrows, Tab and Escape (egui would otherwise move focus between widgets).
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(cell_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
    });
    let active_place = |app: &SheetApp| app.session.active().map(|d| (d.uid, d.wb.active_sheet() as u64, d.selection.active));
    let before_place = active_place(app);
    if app.grid.ime_was_editing || app.grid.ime_ready_cell != before_place {
        app.grid.ime_was_editing = false;
        app.grid.ime_ready_cell = before_place;
        app.grid.ime_ready_gen = app.grid.ime_ready_gen.wrapping_add(1).max(1);
        ctx.memory_mut(|m| m.interrupt_ime());
    }
    let events = grid_events(app, ctx, sh);
    // A shortcut key that also types a character (Shift+Space sends Text(" ")) must not start an edit.
    let shortcut_pressed = events.iter().any(|(_, e)| {
        matches!(e, egui::Event::Key { pressed: true, modifiers: m, key, .. }
            if !m.alt && (m.ctrl || m.command || (m.shift && *key == egui::Key::Space)))
    });
    let checkbox_space = is_checkbox(sh, app)
        && events.iter().any(|(_, e)| {
            matches!(e, egui::Event::Key { key: egui::Key::Space, pressed: true, modifiers: m, .. }
                if !m.command && !m.ctrl && !m.alt && !m.shift)
        });
    let page = ((geo.cells.height() / geo.z) / sh.default_row_height).floor().max(1.0) as i64;
    // Alt shortcuts type their character too on Windows and Linux (Alt+= sends Text("=")): once
    // a shortcut ran, that text must not start an edit over what it just did.
    let mut shortcut_ran = false;
    let mut skipped_indices: Vec<usize> = Vec::new();
    for (idx, ev) in events {
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
            egui::Event::Text(_) | egui::Event::Ime(egui::ImeEvent::Commit(_)) if shortcut_pressed || shortcut_ran => {
                skipped_indices.push(idx);
            }
            egui::Event::Ime(egui::ImeEvent::Preedit { .. }) if shortcut_pressed || shortcut_ran => {
                skipped_indices.push(idx);
            }
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) if !text.is_empty() => {
                app.begin_edit(Some(String::new()), false);
                strip_events_through(ctx, idx, false);
                app.grid.ime_ready_cell = None;
                return;
            }
            egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text))
                if !(checkbox_space && text == " ") && text.chars().all(|c| !c.is_control()) && !text.is_empty() =>
            {
                app.begin_edit(Some(text), false);
                if let Some(ed) = app.editor.as_mut() {
                    ed.caret = ed.text.chars().count();
                }
                strip_events_through(ctx, idx, true);
                app.grid.ime_ready_cell = None;
                return;
            }
            egui::Event::Key { key, physical_key, pressed: true, modifiers: m, .. } => {
                if handle_ready_key(app, ctx, geo, sh, key, physical_key, m, page) {
                    shortcut_ran = true;
                    skipped_indices.push(idx);
                    continue;
                }
                if app.editor.is_some() {
                    strip_events_through(ctx, idx, true);
                    app.grid.ime_ready_cell = None;
                    return;
                }
            }
            _ => {}
        }
    }
    if app.editor.is_some() {
        if !skipped_indices.is_empty() {
            strip_event_indices(ctx, &skipped_indices);
        }
        app.grid.ime_ready_cell = None;
        return;
    }
    let after_place = active_place(app);
    if app.grid.ime_ready_cell != after_place {
        app.grid.ime_ready_cell = after_place;
        app.grid.ime_ready_gen = app.grid.ime_ready_gen.wrapping_add(1).max(1);
        ctx.memory_mut(|m| m.interrupt_ime());
    }
    if ctx.memory(|m| m.focused()) == Some(cell_id)
        && let Some((_, _, cell)) = after_place
    {
        emit_ready_ime(app, ctx, geo, sh, cell_id, cell);
    }
}

/// Handles a single Ready-mode key press; returns `true` when `keymap::dispatch` handled it.
#[expect(clippy::too_many_arguments)]
fn handle_ready_key(
    app: &mut SheetApp,
    ctx: &egui::Context,
    geo: &Geo,
    sh: &Sheet,
    key: egui::Key,
    physical_key: Option<egui::Key>,
    m: egui::Modifiers,
    page: i64,
) -> bool {
    use egui::Key;
    if crate::keymap::dispatch(app, ctx, key, physical_key, m, geo, sh) {
        return true;
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
        _ if cmd && !crate::keymap::altgr_text(ctx, m, ctx.os() == egui::os::OperatingSystem::Mac) => crate::ribbon::shortcut(app, key, m),
        _ => {}
    }
    false
}

/// Registers the Ready-mode IME owner in `used_ids` and publishes `Output::ime` at the active cell.
fn emit_ready_ime(app: &SheetApp, ctx: &egui::Context, geo: &Geo, sh: &Sheet, cell_id: egui::Id, cell: CellRef) {
    let rect = geo.mirror_rect(match sh.merge_at(cell) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, cell),
    });
    let visible_rect = if rect.width() > 0.0 && rect.height() > 0.0 && rect.intersects(geo.cells) { rect.intersect(geo.cells) } else { geo.cells };
    // Keep the Ready-mode IME owner in `used_ids` so `Memory::end_pass` does not clear
    // focus every second frame and force a spurious `interrupt_ime()`.
    ctx.check_for_id_clash(cell_id, visible_rect, "cell_editor");
    if ctx.input(|i| i.focused) && app.keytips.prefix().is_none() {
        let cursor_rect = egui::Rect::from_min_max(visible_rect.left_top(), egui::pos2(visible_rect.left() + 1.0, visible_rect.bottom()));
        let generation = app.grid.ime_ready_gen.max(1);
        ctx.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput::new(cell_id, generation, egui::IMEPurpose::Normal, visible_rect, cursor_rect));
        });
    }
}

/// Drops events up to (`inclusive`) or strictly before `cutoff` so a newly opened cell editor does
/// not re-process the event that opened it while still receiving trailing events in the same frame.
fn strip_events_through(ctx: &egui::Context, cutoff: usize, inclusive: bool) {
    let mut index = 0usize;
    ctx.input_mut(|i| {
        i.events.retain(|_| {
            let keep = if inclusive { index > cutoff } else { index >= cutoff };
            index += 1;
            keep
        });
    });
}

/// Drops shortcut/trailing events by their original indices in `ctx.events` (for example `Alt+=`
/// opening `=SUM()` and its trailing `Text("=")` event).
fn strip_event_indices(ctx: &egui::Context, indices: &[usize]) {
    let mut index = 0usize;
    ctx.input_mut(|i| {
        i.events.retain(|_| {
            let keep = !indices.contains(&index);
            index += 1;
            keep
        });
    });
}

/// This frame's input events paired with their indices, without those the focused form control took.
fn grid_events(app: &mut SheetApp, ctx: &egui::Context, sh: &Sheet) -> Vec<(usize, egui::Event)> {
    let events = ctx.input(|i| i.events.clone());
    events.into_iter().enumerate().filter(|(_, ev)| !form_control_event(app, sh, ev)).collect()
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
        egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
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
