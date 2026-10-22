//! Format Painter through the real ribbon button and a click on the grid.

use egui::accesskit::Toggled;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{NodeT, Queryable};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::Style;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["source"]]})).unwrap();
    session.execute("home.bold", json!({"range": "A1", "on": true})).unwrap();
    session.execute("home.fillColor", json!({"range": "A1", "color": "#FFFF00"})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn style(h: &Harness, a1: &str) -> Style {
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    doc.wb.styles.get(sheet.style_id(CellRef::parse(a1).unwrap())).clone()
}

fn point(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
}

fn pressed(h: &Harness) -> bool {
    h.get_by_label("Format Painter").accesskit_node().toggled() == Some(Toggled::True)
}

fn click_cell(h: &mut Harness, a1: &str) {
    let at = point(h, a1);
    for event in [
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        h.input_mut().events.push(event);
        h.run_steps(1);
    }
    h.run_steps(2);
}

#[test]
fn ribbon_button_picks_up_the_format_and_a_click_on_a_cell_pastes_it() {
    let mut h = harness();
    let source = style(&h, "A1");
    assert!(source.font.bold);
    assert_ne!(style(&h, "C3"), source);

    h.get_by_label("Format Painter").click();
    h.run_steps(2);
    assert!(h.state().session.format_painter.is_some(), "the button must arm the painter");
    assert!(pressed(&h), "the armed button must show as pressed");

    click_cell(&mut h, "C3");
    assert_eq!(style(&h, "C3"), source, "clicking C3 must paste A1's format");
    assert!(h.state().session.format_painter.is_none(), "a single-click painter disarms after one use");
    assert!(!pressed(&h));
}

fn header_point(h: &Harness, header: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    match header.parse::<u32>() {
        Ok(row) => {
            let cell = geo.cell_rect(sheet, CellRef::new(row - 1, 0)).center();
            Pos2::new(geo.cells.left() - geo.header_w * 0.5, cell.y)
        }
        Err(_) => {
            let cell = geo.cell_rect(sheet, CellRef::parse(&format!("{header}1")).unwrap()).center();
            Pos2::new(cell.x, geo.cells.top() - geo.header_h * 0.5)
        }
    }
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    for event in [
        Event::PointerMoved(from),
        Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerMoved(to),
        Event::PointerMoved(to + egui::vec2(1.0, 0.0)),
        Event::PointerButton { pos: to + egui::vec2(1.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        h.input_mut().events.push(event);
        h.run_steps(1);
    }
    h.run_steps(2);
}

fn stored_cells(h: &Harness) -> usize {
    h.state().session.active().unwrap().wb.active().unwrap().cells.len()
}

#[test]
fn a_click_on_a_row_header_pastes_the_format_on_the_whole_row() {
    let mut h = harness();
    let source = style(&h, "A1");
    h.get_by_label("Format Painter").click();
    h.run_steps(2);

    let at = header_point(&h, "5");
    drag(&mut h, at, at);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "5:5");
    for a1 in ["A5", "K5", "XFD5"] {
        assert_eq!(style(&h, a1), source, "{a1} must take A1's format");
    }
    assert_ne!(style(&h, "A6"), source, "row 6 was not painted");
    // The row takes a row format: its 16384 cells are not stored one by one.
    assert_eq!(stored_cells(&h), 1);
    assert!(h.state().session.format_painter.is_none(), "a single-click painter disarms after one use");
    assert!(!pressed(&h));
}

#[test]
fn dragging_over_column_headers_with_a_kept_painter_formats_every_column() {
    let mut h = harness();
    let source = style(&h, "A1");
    h.state_mut().session.execute("edit.formatPainter", json!({"sticky": true})).unwrap();
    h.run_steps(2);

    let (from, to) = (header_point(&h, "C"), header_point(&h, "E"));
    drag(&mut h, from, to);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "C:E");
    for a1 in ["C2", "D500", "E1048576"] {
        assert_eq!(style(&h, a1), source, "{a1} must take A1's format");
    }
    for a1 in ["B2", "F2"] {
        assert_ne!(style(&h, a1), source, "{a1} is outside the dragged headers");
    }
    assert_eq!(stored_cells(&h), 1, "whole columns take a column format, not a million cell styles");
    assert!(h.state().session.format_painter.is_some(), "a double-click painter stays armed");

    let at = header_point(&h, "8");
    drag(&mut h, at, at);
    assert_eq!(style(&h, "Z8"), source, "the kept painter paints the next header clicked too");
}

#[test]
fn dragging_from_the_header_of_the_picked_up_column_paints_instead_of_moving_it() {
    let mut h = harness();
    let session = &mut h.state_mut().session;
    session.execute("range.setValues", json!({"range": "C1", "values": [["kept"]]})).unwrap();
    session.execute("home.italic", json!({"range": "C:C", "on": true})).unwrap();
    session.execute("selection.set", json!({"range": "C:C"})).unwrap();
    session.execute("edit.formatPainter", json!({})).unwrap();
    h.run_steps(2);
    let source = style(&h, "C2");

    let (from, to) = (header_point(&h, "C"), header_point(&h, "E"));
    drag(&mut h, from, to);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::parse("C1").unwrap()), "kept".into(), "column C must stay where it is");
    assert_eq!(style(&h, "E2"), source, "E must take C's format");
    assert!(h.state().session.format_painter.is_none());
}

#[test]
fn a_two_cell_source_on_a_column_header_paints_a_column_format() {
    let mut h = harness();
    let source = style(&h, "A1");
    let session = &mut h.state_mut().session;
    session.execute("home.italic", json!({"range": "A2", "on": true})).unwrap();
    session.execute("home.fillColor", json!({"range": "7:7", "color": "#00FF00"})).unwrap();
    session.execute("selection.set", json!({"range": "A1:A2"})).unwrap();
    session.execute("edit.formatPainter", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(stored_cells(&h), 2);

    let at = header_point(&h, "C");
    drag(&mut h, at, at);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "C:C");
    assert_eq!(style(&h, "C1048575"), source, "the column takes the pattern's first cell as its format");
    assert_eq!(style(&h, "C7"), source, "row 7's own format must not hide the painted column");
    // Only C7 is stored on top of A1:A2: not a million cells, one per pattern repeat.
    assert_eq!(stored_cells(&h), 3);
}
