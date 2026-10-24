//! Focus Cell tints the active cell's whole row and column, inside the visible quadrant.

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::Color;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::{Geo, focus_bands};
use serde_json::json;

fn session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

fn geo(session: &Session) -> Geo {
    let sheet = session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 800.0)), egui::Vec2::ZERO)
}

/// Edges line up to a rounding step: a band reaches the next line, a cell rect adds its width.
fn same(a: (f32, f32), b: (f32, f32), what: &str) {
    assert!((a.0 - b.0).abs() < 0.05 && (a.1 - b.1).abs() < 0.05, "{what}: {a:?} vs {b:?}");
}

/// The column band and the row band of `a1`, in the quadrant showing `rows` x `cols`.
fn bands(session: &Session, a1: &str, rows: (u32, u32), cols: (u32, u32)) -> (Option<Rect>, Option<Rect>) {
    let sheet = session.active().unwrap().wb.active().unwrap();
    focus_bands(&geo(session), sheet, CellRef::parse(a1).unwrap(), rows, cols)
}

#[test]
fn the_bands_span_the_quadrant_across_the_active_cell() {
    let mut s = session();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let sheet = s.active().unwrap().wb.active().unwrap();
    let g = geo(&s);
    let (col, row) = bands(&s, "C3", (0, 19), (0, 7));
    let col = col.expect("the column band");
    let row = row.expect("the row band");
    let c = g.cell_rect(sheet, CellRef::parse("C1").unwrap());
    let r = g.cell_rect(sheet, CellRef::parse("A3").unwrap());
    same((col.left(), col.right()), (c.left(), c.right()), "the band covers column C only");
    same((col.top(), col.bottom()), (g.y(sheet, 0), g.y(sheet, 20)), "and the quadrant's rows");
    same((row.top(), row.bottom()), (r.top(), r.bottom()), "the band covers row 3 only");
    same((row.left(), row.right()), (g.x(sheet, 0), g.x(sheet, 8)), "and the quadrant's columns");
}

#[test]
fn a_quadrant_without_the_active_column_draws_no_column_band() {
    // The frozen-pane layout: four quadrants, and each band shows only where its line shows.
    let (col, row) = bands(&session(), "C3", (0, 19), (4, 7));
    assert!(col.is_none(), "column C is not in this quadrant");
    assert!(row.is_some(), "row 3 is");
}

#[test]
fn a_frozen_quadrant_keeps_its_bands_when_the_sheet_scrolls() {
    // Frozen at C3: rows 1-2 and columns A-B stay put while the rest scrolls away.
    let mut s = session();
    s.execute("view.freezePanes", json!({"cell": "C3"})).unwrap();
    let sheet = s.active().unwrap().wb.active().unwrap();
    let g = Geo::new(sheet, Rect::from_min_max(pos2(0.0, 0.0), pos2(1200.0, 800.0)), vec2(400.0, 300.0));
    let b2 = g.cell_rect(sheet, CellRef::parse("B2").unwrap());
    let a1 = g.cell_rect(sheet, CellRef::parse("A1").unwrap());
    let (col, row) = focus_bands(&g, sheet, CellRef::parse("B2").unwrap(), (0, 1), (0, 1));
    let col = col.expect("the column band");
    let row = row.expect("the row band");
    same((col.left(), col.right()), (b2.left(), b2.right()), "the column band keeps column B's width");
    same((col.top(), col.bottom()), (a1.top(), b2.bottom()), "and reaches the last frozen row");
    same((row.top(), row.bottom()), (b2.top(), b2.bottom()), "the row band keeps row 2's height");
    same((row.left(), row.right()), (a1.left(), b2.right()), "and reaches the last frozen column");
}

#[test]
fn a_hidden_active_row_draws_no_row_band() {
    let mut s = session();
    s.execute("home.hideRows", json!({"rows": "3:3"})).unwrap();
    let (col, row) = bands(&s, "C3", (0, 19), (0, 7));
    assert!(row.is_none(), "a hidden row has no height");
    assert!(col.is_some(), "the column still draws");
}

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// The View tab, with the ribbon step run so its controls answer to a query.
fn view_harness() -> Harness {
    let mut session = session();
    session.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.state_mut().ui.ribbon_tab = "View".into();
    h.run_steps(4);
    h
}

fn active(h: &Harness) -> &gridcraft_engine::model::Sheet {
    h.state().session.active().unwrap().wb.active().unwrap()
}

/// A press and a release at `pos`, as the pointer sends them.
fn click_at(h: &mut Harness, pos: Pos2) {
    for pressed in [true, false] {
        h.input_mut()
            .events
            .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        h.step();
    }
    h.run_steps(2);
}

/// Every filled rectangle of the last frame, in paint order.
fn fills(shapes: &[egui::epaint::ClippedShape]) -> Vec<(Rect, egui::Color32)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(Rect, egui::Color32)>) {
        match shape {
            egui::Shape::Rect(r) => out.push((r.rect, r.fill)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn overflowing_text_keeps_the_tint_under_it() {
    // A1's text runs on into the active row: clearing the gridlines must not clear the tint.
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [["A long label that runs across many empty columns of the sheet"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "F1"})).unwrap();
    s.execute("view.focusCell", json!({"on": true})).unwrap();
    s.execute("view.focusCellColor", json!({"color": "#C00000"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    let app = h.state();
    let sheet = active(&h);
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let d1 = geo.cell_rect(sheet, CellRef::parse("D1").unwrap());
    let probe = pos2(d1.center().x, d1.top() + 3.0);
    let last = fills(&h.output().shapes).into_iter().rfind(|(r, c)| r.contains(probe) && *c != egui::Color32::TRANSPARENT);
    let tint = egui::Color32::from_rgba_unmultiplied(0xC0, 0x00, 0x00, 0x24);
    assert_eq!(last.map(|(_, c)| c), Some(tint), "the row tint is the top fill under the text in D1");
}

#[test]
fn the_view_ribbon_focus_cell_button_toggles_the_setting() {
    let mut h = view_harness();
    assert!(!active(&h).focus_cell, "off to start with");
    h.get_by_role_and_label(Role::Button, "Focus Cell").click();
    h.run_steps(2);
    assert!(active(&h).focus_cell, "the click switches the tint on");
    h.get_by_role_and_label(Role::Button, "Focus Cell").click();
    h.run_steps(2);
    assert!(!active(&h).focus_cell, "and the next click switches it off");
}

#[test]
fn the_focus_cell_control_reads_with_the_show_group() {
    let h = view_harness();
    let button = h.get_by_role_and_label(Role::Button, "Focus Cell").rect();
    // The name sits inside the button, so the button is wider than its icon and its arrow alone.
    assert!(button.width() > 60.0, "the button carries its own name: {button:?}");
    // The control takes the first checkbox row, so the group reads as one block.
    let ruler = h.get_by_label("Ruler").rect();
    assert!((button.top() - ruler.top()).abs() <= 1.5, "level with the Ruler row: {button:?} vs {ruler:?}");
}

#[test]
fn the_split_arrow_opens_the_color_palette_and_picks_a_tint() {
    let mut h = view_harness();
    let main = h.get_by_role_and_label(Role::Button, "Focus Cell").rect();
    // The arrow is the narrow strip to the right of the toggle.
    click_at(&mut h, pos2(main.right() + 6.0, main.center().y));
    assert!(h.query_by_label("Theme Colors").is_some(), "the shared color palette opens");
    assert!(!active(&h).focus_cell, "the arrow does not toggle the setting");
    // The first standard color is the leading swatch of that row.
    let row = h.get_by_label("Standard Colors").rect();
    click_at(&mut h, pos2(row.left() + 8.0, row.bottom() + 12.0));
    assert_eq!(active(&h).focus_cell_color, Some(Color::Rgb(0xC00000)), "the swatch sets the tint");
}
