//! A menu owns the pointer over every one of its items, however far down the sheet it reaches.
//!
//! Reported from the field: with the draw tool on, the Page Layout tab's paper-size menu turned
//! the pointer into the drawing crosshair from its fourth item down — the first three sit above
//! the sheet, and every item below them lies over the grid. The crosshair then stayed until the
//! pointer crossed back out of the grid.

use egui::{CursorIcon, Event, Modifiers, PointerButton, Rect, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn click(h: &mut Harness, pos: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(4);
}

fn hover(h: &mut Harness, pos: egui::Pos2) -> CursorIcon {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(3);
    h.output().platform_output.cursor_icon
}

fn rect(h: &Harness, label: &str) -> Rect {
    h.get_by_label(label).rect()
}

/// The Page Layout tab owns the menus the report is about.
fn page_layout(h: &mut Harness) {
    h.state_mut().ui.ribbon_tab = "Page Layout".into();
    h.run_steps(2);
}

/// Opens Page Layout › Size: twenty paper sizes, so most of the menu lies over the sheet.
fn open_size_menu(h: &mut Harness) {
    page_layout(h);
    let button = rect(h, "Size").center();
    click(h, button);
    assert!(h.query_by_label("A4").is_some(), "the Size menu opens");
}

fn pen_on(h: &mut Harness) {
    h.state_mut().session.execute("draw.pen", json!({"on": true, "color": "#1F5FC9", "width": 2.0})).expect("the pen turns on");
    h.run_steps(2);
}

/// The drawing crosshair must not paint over a menu item, even where the menu lies over the grid.
#[test]
fn the_draw_tool_does_not_paint_its_crosshair_over_a_menu() {
    let mut h = harness();
    pen_on(&mut h);
    open_size_menu(&mut h);
    for label in ["Letter", "Executive", "A3", "B4", "Japanese Postcard"] {
        let item = rect(&h, label).center();
        let cursor = hover(&mut h, item);
        assert_ne!(cursor, CursorIcon::Crosshair, "{label} is a menu item, not a drawing surface");
    }
}

/// The crosshair still belongs to the sheet itself while a draw tool is on.
#[test]
fn the_draw_tool_still_marks_the_sheet() {
    let mut h = harness();
    pen_on(&mut h);
    let grid = h.state().grid.rect.expect("the grid is laid out");
    assert_eq!(hover(&mut h, grid.center()), CursorIcon::Crosshair, "the pen draws on the sheet");
}

/// Every item of a menu keeps the arrow cursor, in both a short and a long menu.
#[test]
fn every_menu_item_keeps_the_arrow_cursor() {
    let mut h = harness();
    page_layout(&mut h);
    let margins = rect(&h, "Margins").center();
    click(&mut h, margins);
    for label in ["Wide", "Narrow", "Custom Margins…"] {
        let item = rect(&h, label).center();
        assert_eq!(hover(&mut h, item), CursorIcon::Default, "{label} must not show the cell cursor");
    }
    // Reopen on the long menu and walk past the items that sit above the sheet.
    let size = rect(&h, "Size").center();
    click(&mut h, size);
    for label in ["Letter", "Tabloid", "A4", "B5", "Japanese Envelope Kaku #3"] {
        let item = rect(&h, label).center();
        assert_eq!(hover(&mut h, item), CursorIcon::Default, "{label} must not show the cell cursor");
    }
}

/// Leaving a menu for the ribbon clears whatever the sheet set.
#[test]
fn leaving_a_menu_restores_the_cursor_over_the_ribbon() {
    let mut h = harness();
    pen_on(&mut h);
    open_size_menu(&mut h);
    let executive = rect(&h, "Executive").center();
    let _ = hover(&mut h, executive);
    let tab = rect(&h, "Home").center();
    assert_eq!(hover(&mut h, tab), CursorIcon::Default, "the cell cursor must not stick after the pointer leaves the grid");
}
