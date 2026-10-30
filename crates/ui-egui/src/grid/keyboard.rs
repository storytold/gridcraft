use super::geo::Geo;
use super::interact::delete_selected_object;
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
    app.grid.typed.clear();
    allow_ime(app, ctx, resp, geo, sh);
    let events = ctx.input(|i| i.events.clone());
    // A shortcut key that also types a character (Shift+Space sends Text(" ")) must not start an edit.
    let shortcut_pressed = events.iter().any(|e| {
        matches!(e, egui::Event::Key { pressed: true, modifiers: m, key, .. }
            if !m.alt && (m.ctrl || m.command || (m.shift && *key == egui::Key::Space)))
    });
    let page = ((geo.cells.height() / geo.z) / sh.default_row_height).floor().max(1.0) as i64;
    // Alt shortcuts type their character too on Windows and Linux (Alt+= sends Text("=")): once
    // a shortcut ran, that text must not start an edit over what it just did.
    let mut shortcut_ran = false;
    let mut events = events.into_iter();
    while let Some(ev) = events.next() {
        match ev {
            egui::Event::Copy => {
                app.copy_to_clipboard(ctx, "edit.copy");
            }
            egui::Event::Cut => {
                app.copy_to_clipboard(ctx, "edit.cut");
            }
            egui::Event::Paste(text) => app.paste_external(text),
            // GridCraft has no image-clipboard pipeline and no cell-picture insert-from-clipboard
            // command. Choosing to ignore `PasteImage` is explicit: inserting the bytes as a
            // floating picture without a chosen anchor/size would be a new feature, not parity.
            // A text representation, when the clipboard has one, still arrives as `Paste` above.
            egui::Event::PasteImage(_) => {}
            // PRIMARY middle-click paste belongs to egui's TextEdit; the grid has no caret to
            // insert text at, so a middle click on a cell is deliberately not a paste.
            egui::Event::MiddleClickPaste { .. } => {}
            egui::Event::Text(_) | egui::Event::Ime(_) if shortcut_pressed || shortcut_ran => {}
            egui::Event::Text(ref text) | egui::Event::Ime(egui::ImeEvent::Commit(ref text) | egui::ImeEvent::Preedit { ref text, .. })
                if text.chars().all(|c| !c.is_control()) && !text.is_empty() =>
            {
                start_typing(app, ctx, std::iter::once(ev).chain(events).collect());
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
                    Key::Delete if delete_selected_object(app, sh) => {}
                    Key::Delete => app.run_or_alert("edit.clearContents", json!({})),
                    Key::Backspace if delete_selected_object(app, sh) => {}
                    Key::Backspace => {
                        // Excel for Mac: Delete clears the cell and starts editing it.
                        app.run_or_alert("edit.clearContents", json!({}));
                        if app.session.active().is_some_and(|d| d.selection.is_single_cell()) {
                            app.begin_edit(Some(String::new()), false);
                        }
                    }
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

/// Lets the input method compose while the grid has the keyboard. Otherwise the first key of an
/// entry arrives as a lone jamo (or kana) that the next keys can no longer combine with, and the
/// in-cell editor, which allows the input method, only gets the keys after it. Not on a touch
/// screen, where an input method context brings up the on-screen keyboard.
fn allow_ime(app: &mut SheetApp, ctx: &egui::Context, resp: &egui::Response, geo: &Geo, sh: &Sheet) {
    if ctx.input(|i| i.has_touch_screen()) {
        return;
    }
    let Some(active) = app.session.active().map(|d| d.selection.active) else { return };
    let cell = match sh.merge_at(active) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, active),
    };
    let caret = Rect::from_min_size(cell.min + vec2(2.0, 0.0), vec2(1.0, cell.height()));
    // A new context each time the grid takes the keyboard back, as a text field does on focus.
    let pass = ctx.cumulative_pass_nr();
    if app.grid.ime_pass.is_none_or(|last| last.saturating_add(1) < pass) {
        app.grid.ime_generation = app.grid.ime_generation.wrapping_add(1).max(1);
    }
    app.grid.ime_pass = Some(pass);
    let to_global = ctx.layer_transform_to_global(resp.layer_id).unwrap_or_default();
    let ime = egui::output::IMEOutput::new(
        resp.id,
        app.grid.ime_generation,
        egui::IMEPurpose::Normal,
        to_global * geo.mirror_rect(cell),
        to_global * geo.mirror_rect(caret),
    );
    ctx.output_mut(|o| o.ime = Some(ime));
}

/// Starts editing the cell with the keys typed over it. The in-cell editor gets those keys itself,
/// once it has the keyboard: egui interrupts the input method when the focus moves, which would
/// cancel the composition just started (the next key starts a new one, so a Korean syllable comes
/// apart into its jamo). So the keys are held back from this pass, which is discarded, and the
/// repeat pass, where the editor has the focus and the input method context is its own, replays
/// them: the interrupt was asked for the grid's context and doesn't reach the platform.
fn start_typing(app: &mut SheetApp, ctx: &egui::Context, typed: Vec<egui::Event>) {
    app.begin_edit(Some(String::new()), false);
    if app.editor.is_none() {
        return;
    }
    ctx.input_mut(|i| i.events.retain(|e| !typed.contains(e)));
    app.grid.typed = typed;
    ctx.request_discard("the in-cell editor takes the keys that started the edit");
}
