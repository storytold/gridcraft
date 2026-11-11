//! The in-cell editor covers the edited cell exactly, as in Excel, at every zoom (#1755).

use egui::{Color32, Event, Modifiers, PointerButton, Rect, Shape};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn cell_rect(h: &Harness, cell: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap())
}

fn click_cell(h: &mut Harness, cell: &str) {
    let pos = cell_rect(h, cell).center();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(3);
}

/// The white editor background painted from the cell's top-left corner.
fn editor_rect(h: &Harness, cell: Rect) -> Rect {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Rect(r) if r.fill == Color32::WHITE && (r.rect.min - cell.min).length() < 0.01 => Some(r.rect),
            _ => None,
        })
        .next_back()
        .expect("in-cell editor background")
}

fn assert_close(a: Rect, b: Rect, what: &str) {
    assert!((a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01, "{what}: editor {a:?} must match cell {b:?}");
}

#[test]
fn editor_matches_the_cell_at_every_zoom() {
    for zoom in [50u16, 100, 200, 400] {
        let mut h = harness();
        h.state_mut().run("view.zoom", json!({"percent": zoom})).unwrap();
        h.run_steps(3);
        click_cell(&mut h, "B2");
        type_text(&mut h, "abc");
        assert!(h.state().editor.is_some());
        let cell = cell_rect(&h, "B2");
        assert_close(editor_rect(&h, cell), cell, &format!("zoom {zoom}"));
    }
}

#[test]
fn one_long_line_widens_the_editor_but_keeps_the_row_height() {
    let mut h = harness();
    click_cell(&mut h, "B2");
    type_text(&mut h, "a much longer line of text than the cell can hold");
    let cell = cell_rect(&h, "B2");
    let ed = editor_rect(&h, cell);
    assert!(ed.width() > cell.width() + 1.0, "long text widens the editor: {ed:?} vs {cell:?}");
    assert!((ed.height() - cell.height()).abs() < 0.01, "one line keeps the row height: {ed:?} vs {cell:?}");
}

#[test]
fn in_cell_editor_shortcuts_toggle_bold_italic_underline_and_strikethrough_while_preserving_text() {
    let mut h = harness();
    click_cell(&mut h, "B2");
    type_text(&mut h, "hello");
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("hello"));

    let press = |h: &mut Harness, modifiers: Modifiers, key: egui::Key| {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        h.run_steps(3);
    };
    let b2 = CellRef::parse("B2").unwrap();
    let font_of = |h: &Harness, cell: CellRef| {
        let doc = h.state().session.active().unwrap();
        doc.wb.styles.get(doc.wb.active().unwrap().style_id(cell)).font.clone()
    };
    let ctrl_only = Modifiers { ctrl: true, ..Modifiers::NONE };
    let cmd_only = Modifiers { command: true, ..Modifiers::NONE };

    // Test both CTRL-only and COMMAND-only branches for B, I, U, plus CTRL+5 for strikethrough.
    press(&mut h, ctrl_only, egui::Key::B);
    assert!(font_of(&h, b2).bold);
    press(&mut h, cmd_only, egui::Key::B);
    assert!(!font_of(&h, b2).bold);

    press(&mut h, ctrl_only, egui::Key::I);
    assert!(font_of(&h, b2).italic);
    press(&mut h, cmd_only, egui::Key::I);
    assert!(!font_of(&h, b2).italic);

    press(&mut h, ctrl_only, egui::Key::U);
    assert_ne!(font_of(&h, b2).underline, gridcraft_engine::model::Underline::None);
    press(&mut h, cmd_only, egui::Key::U);
    assert_eq!(font_of(&h, b2).underline, gridcraft_engine::model::Underline::None);

    press(&mut h, ctrl_only, egui::Key::Num5);
    assert!(font_of(&h, b2).strike);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("hello"), "editor stays open and preserves its draft text");
}

#[test]
fn in_cell_editor_renders_cell_color_underline_and_strike_for_values_but_not_formulas() {
    let mut h = harness();
    h.state_mut().run("selection.set", json!({"range": "B2"})).unwrap();
    h.state_mut().run("home.fontColor", json!({"color": "#D13438"})).unwrap();
    h.state_mut().run("home.underline", json!({"on": true})).unwrap();
    h.state_mut().run("home.strikethrough", json!({"on": true})).unwrap();

    click_cell(&mut h, "B2");
    type_text(&mut h, "styled");

    let find_editor_section = |h: &Harness, needle: &str| {
        h.output()
            .shapes
            .iter()
            .rfind(|s| matches!(&s.shape, Shape::Text(t) if t.galley.job.text == needle))
            .and_then(|s| match &s.shape {
                Shape::Text(t) => t.galley.job.sections.first().map(|sec| sec.format.clone()),
                _ => None,
            })
            .expect("in-cell editor text shape")
    };

    let fmt = find_editor_section(&h, "styled");
    assert_eq!(fmt.color, Color32::from_rgb(0xD1, 0x34, 0x38));
    assert_ne!(fmt.underline, egui::Stroke::NONE);
    assert_ne!(fmt.strikethrough, egui::Stroke::NONE);

    // Switching the draft to a formula disables the cell's font color/underline/strikethrough in the editor.
    h.state_mut().editor.as_mut().unwrap().text = "=1+2".into();
    h.run_steps(3);
    let formula_fmt = find_editor_section(&h, "=1+2");
    assert_ne!(formula_fmt.color, Color32::from_rgb(0xD1, 0x34, 0x38));
    assert_eq!(formula_fmt.underline, egui::Stroke::NONE);
    assert_eq!(formula_fmt.strikethrough, egui::Stroke::NONE);
}
