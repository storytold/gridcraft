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
        // The grid gives up the input method; taking it back starts a new context.
        app.grid.ime.observe(ctx.viewport_id(), None);
        return;
    }
    if ctx.memory(|m| m.focused()).is_none() {
        resp.request_focus();
    }
    // The grid owns arrows, Tab and Escape (egui would otherwise move focus between widgets).
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(resp.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
    });
    // The input method is on at the active cell, so typing Vietnamese (or Chinese, Japanese…)
    // straight into a cell composes there, its candidate window at the cell, rather than in a box
    // of the input method's own.
    // The grid has no committed text of its own, so the surrounding text is unavailable.
    if let Some(d) = app.session.active() {
        let at = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
        let r = geo.screen_cell_rect(sh, at);
        let viewport = ctx.viewport_id();
        let observation = egui::ime_context::ImeObservation::new(
            ctx.cumulative_frame_nr_for(viewport),
            ctx.cumulative_pass_nr_for(viewport),
            egui::ime_context::ImeTarget::new(resp.id, 0),
        );
        if let Some(u) = app.grid.ime.observe(viewport, Some(observation)) {
            let mut ime = egui::output::IMEOutput::new(u.owner, u.context_generation, u.purpose, r, r);
            ime.surrounding_text = u.surrounding_text;
            ime.change_cause = u.change_cause;
            ctx.output_mut(|o| o.ime = Some(ime));
        }
    }
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
            egui::Event::Text(_) if shortcut_pressed || shortcut_ran => {}
            egui::Event::Text(text) if text.chars().all(|c| !c.is_control()) && !text.is_empty() => {
                app.begin_edit(Some(text), false);
                if let Some(ed) = app.editor.as_mut() {
                    ed.caret = ed.text.chars().count();
                }
                return;
            }
            // A word an input method finished without composing it goes in as typed text; what
            // the input method sent after it follows it into the editor.
            egui::Event::Ime(egui::ImeEvent::Commit(text)) if text.chars().all(|c| !c.is_control()) && !text.is_empty() => {
                app.begin_edit(Some(text), false);
                if let Some(ed) = app.editor.as_mut() {
                    ed.caret = ed.text.chars().count();
                }
                hand_over_to_the_editor(app, ctx, Vec::new(), events);
                return;
            }
            // The first word being composed opens the cell's editor, empty, and is handed to it
            // once it has the focus (the frame after next), with what followed it: from then on
            // the editor shows the composition in the cell, in the cell's font, as it does for the
            // rest of the word.
            egui::Event::Ime(egui::ImeEvent::Preedit { text, active_range_chars }) if !text.is_empty() && text.chars().all(|c| !c.is_control()) => {
                app.begin_edit(Some(String::new()), false);
                let first = egui::Event::Ime(egui::ImeEvent::Preedit { text, active_range_chars });
                hand_over_to_the_editor(app, ctx, vec![first], events);
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

/// Hands `first` and the typing in the rest of this frame's `events` to the cell's editor the
/// grid just opened, in their original order, once it has the focus ([`ImeHandoff`]); what the
/// input method sends meanwhile follows them. The editor opens in this pass, so the typing is
/// taken out of the pass's input as well: it would otherwise reach the editor twice.
fn hand_over_to_the_editor(app: &mut SheetApp, ctx: &egui::Context, first: Vec<egui::Event>, events: impl Iterator<Item = egui::Event>) {
    let mut held = first;
    held.extend(events.filter(ImeHandoff::holds));
    ctx.input_mut(|i| i.events.retain(|e| !ImeHandoff::holds(e)));
    app.grid.ime_handoff = Some(ImeHandoff { events: held, frames: 2 });
    keep_composition_through_focus_move(ctx);
    ctx.request_repaint();
}

/// Keeps the input method's composition through the focus move to the cell's editor: egui
/// interrupts the input method on every `request_focus` (egui-winit turns it off and on again),
/// and fcitx5 then drops the letters it is composing, so "không" typed into a cell came out
/// "hong". The interrupt is taken back in this frame's output and the next one, where the editor
/// takes the focus.
fn keep_composition_through_focus_move(ctx: &egui::Context) {
    ctx.plugin_or_default::<KeepComposition>().lock().frames = 2;
}

/// Takes back egui's input method interrupt for a few frames ([`keep_composition_through_focus_move`]).
#[derive(Debug, Default)]
struct KeepComposition {
    frames: u8,
}

impl egui::Plugin for KeepComposition {
    fn debug_name(&self) -> &'static str {
        "gridcraft.keep_composition"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        if self.frames > 0 {
            self.frames -= 1;
            if let Some(ime) = output.platform_output.ime.as_mut() {
                ime.should_interrupt_composition = false;
            }
        }
    }
}
