//! Right-clicking the status bar opens a menu that turns its selection statistics on and off.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    for (cell, input) in [("A1", "4"), ("A2", "-1"), ("A3", "9")] {
        session.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    session.execute("selection.set", json!({"range": "A1:A3"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn right_click(h: &mut Harness, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
}

#[test]
fn right_clicking_the_status_bar_turns_on_minimum() {
    let mut h = harness();
    assert!(h.query_by_label("Sum: 12").is_some(), "Sum is on by default");
    assert!(h.query_by_label("Minimum: -1").is_none(), "Minimum is off by default");
    // An empty stretch of the 26-point status bar, between the mode text and the statistics.
    right_click(&mut h, pos2(300.0, 787.0));
    h.get_by_label("Minimum").click();
    h.run_steps(4);
    assert!(h.state().session.prefs.status_bar.minimum);
    assert!(h.query_by_label("Minimum: -1").is_some(), "the status bar shows the minimum of the selection");
}

#[test]
fn right_clicking_the_status_bar_text_opens_the_menu() {
    for text in ["Sum: 12", "Ready"] {
        let mut h = harness();
        let pos = h.get_by_label(text).rect().center();
        right_click(&mut h, pos);
        assert!(h.query_by_label("Numerical Count").is_some(), "a right-click on {text:?} opens the statistics menu");
    }
}

#[test]
fn selection_statistics_use_the_interface_language() {
    for (language, labels, menu_labels) in [
        ("en", ["Average: 4", "Count: 3", "Numerical Count: 3", "Minimum: -1", "Maximum: 9", "Sum: 12"], ["Count", "Sum"]),
        ("tr", ["Ortalama: 4", "Sayı: 3", "Sayısal Sayı: 3", "En Küçük: -1", "En Büyük: 9", "Toplam: 12"], ["Sayı", "Toplam"]),
        ("pt-BR", ["Média: 4", "Contagem: 3", "Numerical Count: 3", "Minimum: -1", "Maximum: 9", "Soma: 12"], ["Contagem", "Soma"]),
    ] {
        let mut h = harness();
        h.state_mut().run("app.language.set", json!({"language": language})).unwrap();
        h.state_mut().run("view.statusBar", json!({"numericalCount": true, "minimum": true, "maximum": true})).unwrap();
        h.run_steps(3);
        for label in labels {
            assert!(h.query_by_label(label).is_some(), "{language}: missing {label}");
        }
        right_click(&mut h, pos2(300.0, 787.0));
        for label in menu_labels {
            assert!(h.query_by_label(label).is_some(), "{language}: missing menu label {label}");
        }
    }
}
