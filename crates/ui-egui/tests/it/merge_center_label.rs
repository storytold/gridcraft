//! Home › Merge & Center is a split button: clicking its text label runs `home.mergeCenter`, like
//! its icon does.

use egui::{Event, Shape, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

#[test]
fn clicking_the_label_merges_and_centers() {
    for language in [Language::En, Language::De] {
        click_label(language);
    }
}

fn click_label(language: Language) {
    let label = language.tr("Merge & Center");
    let mut s = Session::new();
    s.new_workbook();
    s.execute("selection.set", json!({"range": "A1:B1"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        {
            let mut app = crate::common::app(s);
            app.ui.language = language;
            app
        },
    );
    h.run_steps(4);
    let at = h
        .output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == label => Some(t.pos + t.galley.size() / 2.0),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the Home tab shows {label}"));
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.mergeCenter"), "{label}");
}
