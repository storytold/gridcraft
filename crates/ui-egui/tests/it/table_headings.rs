//! A Table's header names and filter buttons stand in for the column letters while its header row
//! is scrolled out of view and the active cell is inside it, as in Excel (#1423).

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::model::Sheet;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::{Geo, heading_table, table_headings};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A Table on `range` (header row 3) with these header names and numbers below.
fn table(s: &mut Session, range: &str, names: &[&str]) {
    let r = RangeRef::parse(range).unwrap();
    let mut rows = vec![names.iter().map(|n| json!(n)).collect::<Vec<_>>()];
    for i in r.start.row + 1..=r.end.row {
        rows.push(names.iter().map(|_| json!(i)).collect());
    }
    s.execute("range.setValues", json!({"range": r.start.a1(), "values": rows})).unwrap();
    s.execute("insert.table", json!({"range": range, "header": true})).unwrap();
}

fn session(active: &str) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    table(&mut s, "A3:E40", &["Bank", "Date", "Type", "Amount", "Ref"]);
    s.execute("selection.set", json!({"cell": active})).unwrap();
    s
}

fn sheet(s: &Session) -> Sheet {
    s.active().unwrap().wb.active().unwrap().clone()
}

/// The grid scrolled so that `row` is the first one under the frozen rows.
fn geo_at(sh: &Sheet, row: u32) -> Geo {
    let fr = sh.freeze.map_or(0, |f| f.0);
    let scroll = vec2(0.0, (sh.row_top(row) - sh.row_top(fr)) as f32);
    Geo::new(sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0)), scroll)
}

fn names(geo: &Geo, sh: &Sheet, active: &str) -> Vec<(u32, String)> {
    table_headings(geo, sh, CellRef::parse(active).unwrap()).into_iter().map(|h| (h.col, h.name)).collect()
}

fn owned(v: &[(u32, &str)]) -> Vec<(u32, String)> {
    v.iter().map(|(c, n)| (*c, n.to_string())).collect()
}

#[test]
fn header_names_replace_the_letters_only_while_the_header_row_is_out_of_view() {
    let s = session("C20");
    let sh = sheet(&s);
    let c20 = CellRef::parse("C20").unwrap();
    assert!(heading_table(&geo_at(&sh, 0), &sh, c20).is_none(), "header row 3 on screen: letters");
    assert!(heading_table(&geo_at(&sh, 2), &sh, c20).is_none(), "header row 3 at the top: letters");
    assert_eq!(heading_table(&geo_at(&sh, 3), &sh, c20).map(|t| t.range.a1()).as_deref(), Some("A3:E40"));
    assert_eq!(names(&geo_at(&sh, 10), &sh, "C20"), owned(&[(0, "Bank"), (1, "Date"), (2, "Type"), (3, "Amount"), (4, "Ref")]));
    // Scrolled past the table's last row, the letters come back.
    assert!(heading_table(&geo_at(&sh, 40), &sh, c20).is_none());
}

#[test]
fn an_active_cell_outside_the_table_keeps_the_letters() {
    let s = session("G20");
    let sh = sheet(&s);
    // The table's rows fill the screen, so only where the active cell is tells the two apart.
    assert!(heading_table(&geo_at(&sh, 10), &sh, CellRef::parse("G20").unwrap()).is_none());
    assert!(heading_table(&geo_at(&sh, 10), &sh, CellRef::parse("E20").unwrap()).is_some());
}

#[test]
fn a_header_row_in_the_frozen_pane_keeps_the_letters() {
    let mut s = session("C20");
    s.execute("view.freezePanes", json!({"cell": "A4"})).unwrap();
    let sh = sheet(&s);
    assert_eq!(sh.freeze, Some((3, 0)));
    assert!(heading_table(&geo_at(&sh, 10), &sh, CellRef::parse("C20").unwrap()).is_none());

    // Frozen above the header row, the header row scrolls under the frozen rows.
    s.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    let sh = sheet(&s);
    assert_eq!(sh.freeze, Some((1, 0)));
    assert!(heading_table(&geo_at(&sh, 2), &sh, CellRef::parse("C20").unwrap()).is_none());
    assert!(heading_table(&geo_at(&sh, 3), &sh, CellRef::parse("C20").unwrap()).is_some());
}

#[test]
fn with_two_tables_the_names_come_from_the_active_cells_table() {
    let mut s = Session::new();
    s.new_workbook();
    table(&mut s, "A3:C40", &["Bank", "Date", "Type"]);
    table(&mut s, "E3:G40", &["Region", "Units", "Price"]);
    let sh = sheet(&s);
    let geo = geo_at(&sh, 10);
    assert_eq!(names(&geo, &sh, "F20"), owned(&[(4, "Region"), (5, "Units"), (6, "Price")]));
    assert_eq!(names(&geo, &sh, "B20"), owned(&[(0, "Bank"), (1, "Date"), (2, "Type")]));
    assert_eq!(names(&geo, &sh, "D20"), owned(&[]));
}

#[test]
fn hidden_columns_leave_the_other_names_over_their_own_columns() {
    let mut s = session("C20");
    s.execute("home.hideColumns", json!({"cols": "B:B"})).unwrap();
    let sh = sheet(&s);
    assert_eq!(sh.col_width(1), 0.0);
    let geo = geo_at(&sh, 10);
    let headings = table_headings(&geo, &sh, CellRef::parse("C20").unwrap());
    let got: Vec<(u32, &str)> = headings.iter().map(|h| (h.col, h.name.as_str())).collect();
    assert_eq!(got, [(0, "Bank"), (2, "Type"), (3, "Amount"), (4, "Ref")]);
    for h in &headings {
        let cell = geo.cell_rect(&sh, CellRef::new(10, h.col));
        assert_eq!((h.rect.left(), h.rect.right()), (cell.left(), cell.right()), "{} sits over column {}", h.name, h.col);
        let b = h.button.unwrap();
        assert!(b.left() > cell.left() && b.right() < cell.right() && b.top() >= geo.head_top() && b.bottom() <= geo.cells.top());
    }
}

fn click(h: &mut Harness, at: Pos2) {
    let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(at), press(true)]);
    h.step();
    h.input_mut().events.push(press(false));
    h.run_steps(3);
}

#[test]
fn the_button_in_a_replaced_heading_opens_that_columns_filter_menu() {
    let mut s = session("C20");
    s.execute("home.hideColumns", json!({"cols": "B:B"})).unwrap();
    s.execute("selection.set", json!({"cell": "C20"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    let sh = sheet(&h.state().session);
    let scroll = sh.row_top(10) as f32;
    h.state_mut().view_mut().unwrap().scroll.y = scroll;
    h.run_steps(2);
    if let Ok(path) = std::env::var("GRIDCRAFT_TABLE_HEADINGS_SCREENSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    let app = h.state();
    let geo = Geo::new(&sh, app.grid.rect.unwrap(), app.view().scroll);
    let headings = table_headings(&geo, &sh, CellRef::parse("C20").unwrap());
    let amount = headings.iter().find(|x| x.name == "Amount").unwrap();
    assert_eq!(amount.col, 3);

    click(&mut h, amount.button.unwrap().center());
    let menu = h.state().grid.filter_menu.map(|(c, r, _)| (c, r));
    assert_eq!(menu, Some((3, 2)), "the menu of column D, keyed by the table's header row");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "C20", "the button doesn't select");

    // Beside the button the heading still selects the whole column.
    h.state_mut().grid.filter_menu = None;
    click(&mut h, pos2(amount.rect.left() + 6.0, amount.rect.center().y));
    assert_eq!(h.state().grid.filter_menu, None);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "D:D");
}
