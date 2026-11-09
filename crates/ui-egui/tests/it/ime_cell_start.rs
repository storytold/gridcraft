//! The grid owns the IME while a cell is selected, as Excel's sheet does: the first kana opens the
//! cell editor, so Japanese can be typed without pressing F2 first.

use egui::{Event, ImeEvent};
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

#[test]
fn a_preedit_on_a_selected_cell_opens_the_editor() {
    let mut h = harness();
    assert!(h.state().editor.is_none());
    ime(&mut h, ImeEvent::Preedit { text: "に".into(), active_range_chars: None });
    assert!(h.state().editor.is_some(), "the IME composition starts the cell editor");
}

#[test]
fn a_commit_on_a_selected_cell_starts_an_edit_with_its_text() {
    // The shared harness has no Japanese face (`MissingGlyphPolicy::Panic`), so the committed
    // text is Latin here; the commit path is the same for any script.
    let mut h = harness();
    ime(&mut h, ImeEvent::Commit("abc".into()));
    assert_eq!(h.state().editor.as_ref().map(|ed| ed.text.as_str()), Some("abc"));
}
