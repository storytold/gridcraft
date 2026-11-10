//! The grid owns the IME while a cell is selected, as Excel's sheet does: the first kana opens the
//! cell editor, so Japanese can be typed without pressing F2 first.

use egui::{Event, ImeEvent, Key};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

fn ime(h: &mut Harness, event: ImeEvent) {
    h.input_mut().events.push(Event::Ime(event));
    h.run_steps(4);
}

fn key(h: &mut Harness, key: Key) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(4);
}

/// Whether the cell editor's text field has the keyboard focus.
fn editor_focused(h: &Harness) -> bool {
    h.ctx.memory(|m| m.focused()) == Some(egui::Id::unique("gridcraft.cell_editor"))
}

#[test]
fn a_preedit_on_a_selected_cell_previews_the_composition_without_taking_focus() {
    // The shared harness has no Japanese face (`MissingGlyphPolicy::Panic`), so the composition is
    // Latin here; the path is the same for any script.
    let mut h = harness();
    assert!(h.state().editor.is_none());
    ime(&mut h, ImeEvent::Preedit { text: "ni".into(), active_range_chars: None });
    let ed = h.state().editor.as_ref().expect("the IME composition opens the cell editor");
    assert_eq!(ed.text, "ni", "the first preedit is shown, not lost");
    assert!(ed.ime_preview, "the editor previews the composition");
    assert!(!editor_focused(&h), "the composition stays with the grid: moving focus would interrupt it");

    // A further preedit replaces the composition instead of appending to it.
    ime(&mut h, ImeEvent::Preedit { text: "nihon".into(), active_range_chars: None });
    assert_eq!(h.state().editor.as_ref().map(|ed| ed.text.as_str()), Some("nihon"));
    assert!(!editor_focused(&h));

    // The commit keeps the composed text and hands the editor the keyboard once the composition
    // has ended, when moving focus can no longer interrupt it.
    ime(&mut h, ImeEvent::Commit("nihon".into()));
    let ed = h.state().editor.as_ref().expect("the editor stays open");
    assert_eq!(ed.text, "nihon");
    assert!(!ed.ime_preview, "the composition preview is over");
    assert!(editor_focused(&h), "the editor takes the keyboard");
}

#[test]
fn an_empty_preedit_cancels_the_composition_preview() {
    let mut h = harness();
    ime(&mut h, ImeEvent::Preedit { text: "ni".into(), active_range_chars: None });
    assert!(h.state().editor.is_some(), "the composition opened the editor");
    ime(&mut h, ImeEvent::Preedit { text: String::new(), active_range_chars: None });
    assert!(h.state().editor.is_none(), "a composition cleared with nothing committed cancels the edit");
}

#[test]
fn escape_cancels_a_preview_without_an_ime_clearing_event() {
    let mut h = harness();
    ime(&mut h, ImeEvent::Preedit { text: "ni".into(), active_range_chars: None });
    assert!(h.state().editor.is_some(), "the composition opened the editor");
    key(&mut h, Key::Escape);
    assert!(h.state().editor.is_none(), "Escape cancels the preview when the IME sends no clearing event");
}

#[test]
fn leaving_the_grid_discards_an_uncommitted_preview() {
    let mut h = harness();
    ime(&mut h, ImeEvent::Preedit { text: "ni".into(), active_range_chars: None });
    assert!(h.state().editor.as_ref().is_some_and(|ed| ed.ime_preview));
    h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("gridcraft.name_box")));
    h.run_steps(4);
    assert!(h.state().editor.is_none(), "the uncommitted preedit must not stay as a draft");
}

#[test]
fn leaving_the_grid_keeps_text_the_ime_committed() {
    let mut h = harness();
    // A commit followed by a new preedit in one batch: "a" is committed, "b" is still composing.
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("a".into())));
    h.input_mut().events.push(Event::Ime(ImeEvent::Preedit { text: "b".into(), active_range_chars: None }));
    h.run_steps(4);
    assert_eq!(h.state().editor.as_ref().map(|ed| (ed.text.as_str(), ed.ime_preview)), Some(("ab", true)));
    h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("gridcraft.name_box")));
    h.run_steps(4);
    let ed = h.state().editor.as_ref().expect("the committed text stays as an open draft");
    assert_eq!(ed.text, "a", "only the uncommitted preedit is discarded");
    assert!(!ed.ime_preview);
    assert!(!editor_focused(&h), "the open draft must not pull focus back from the Name Box");
}

#[test]
fn escape_in_the_name_box_keeps_the_committed_draft() {
    let mut h = harness();
    // Leave committed text as an open draft, then move the keyboard to the Name Box.
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("a".into())));
    h.input_mut().events.push(Event::Ime(ImeEvent::Preedit { text: "b".into(), active_range_chars: None }));
    h.run_steps(4);
    h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("gridcraft.name_box")));
    h.run_steps(4);
    assert_eq!(h.state().editor.as_ref().map(|ed| ed.text.as_str()), Some("a"));

    // Escape closes the Name Box; its focus-loss frame must not also cancel the cell draft.
    key(&mut h, Key::Escape);
    let ed = h.state().editor.as_ref().expect("Escape in the Name Box must not cancel the cell draft");
    assert_eq!(ed.text, "a");
}

#[test]
fn a_commit_on_a_selected_cell_starts_an_edit_with_its_text() {
    let mut h = harness();
    ime(&mut h, ImeEvent::Commit("abc".into()));
    assert_eq!(h.state().editor.as_ref().map(|ed| ed.text.as_str()), Some("abc"));
    assert!(editor_focused(&h));
}
