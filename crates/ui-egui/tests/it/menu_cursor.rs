//! A menu owns the pointer over every one of its items, however far down the sheet it reaches.
//!
//! Reported from the field: with the draw tool on, the Page Layout tab's paper-size menu turned
//! the pointer into the drawing crosshair from its fourth item down — the first three sit above
//! the sheet, and every item below them lies over the grid. The crosshair then stayed until the
//! pointer crossed back out of the grid.

use egui::{CursorIcon, Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// An empty workbook in a 1200×800 window, laid out and settled.
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

/// Move to `pos`, press and release the primary button there, and settle.
fn click(h: &mut Harness, pos: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(4);
}

/// Move to `pos` and return the cursor the app asks for, after it settles.
fn hover(h: &mut Harness, pos: egui::Pos2) -> CursorIcon {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(3);
    h.output().platform_output.cursor_icon
}

/// The on-screen rectangle of the widget labelled `label`.
fn rect(h: &Harness, label: &str) -> Rect {
    h.get_by_label(label).rect()
}

/// Press the primary button at `pos` and keep it down: a stroke needs a held drag.
fn press(h: &mut Harness, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

/// Release the primary button at `pos`.
fn release(h: &mut Harness, pos: Pos2) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
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

/// Turn the pen on with the default blue at 2 pt.
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

/// A dialog is a `Middle`-order window, not a `Foreground` area: it owns the pointer just the same.
#[test]
fn the_draw_tool_does_not_paint_its_crosshair_over_a_dialog() {
    let mut h = harness();
    pen_on(&mut h);
    page_layout(&mut h);
    let margins = rect(&h, "Margins").center();
    click(&mut h, margins);
    let custom = rect(&h, "Custom Margins…").center();
    click(&mut h, custom);
    // Probe the bottom of the dialog: the window hangs over the cells, and the mirrored point
    // must be a cell, or the check is vacuous.
    let win = rect(&h, "Page Setup");
    let probe = pos2(win.center().x, win.bottom() - 24.0);
    let g = h.state().grid.geo.clone().expect("the grid is laid out");
    assert!(win.intersects(g.cells), "the Page Setup dialog opens over the sheet");
    assert!(g.in_cells(g.mirror_pos(probe)), "the probe lies in a cell under the dialog");
    assert_ne!(hover(&mut h, probe), CursorIcon::Crosshair, "the Page Setup dialog is not a drawing surface");
}

/// The layer lookup works in screen coordinates: on a right-to-left sheet the mirrored grid
/// position would land outside the menu and leak the crosshair through.
#[test]
fn the_draw_tool_keeps_the_arrow_over_a_menu_on_a_right_to_left_sheet() {
    let mut h = harness();
    h.state_mut().session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    h.run_steps(2);
    pen_on(&mut h);
    let grid = h.state().grid.rect.expect("the grid is laid out");
    assert_eq!(hover(&mut h, grid.center()), CursorIcon::Crosshair, "the pen still draws on a mirrored sheet");
    open_size_menu(&mut h);
    for label in ["Letter", "Executive", "A3", "B4", "Japanese Postcard"] {
        let item = rect(&h, label).center();
        assert_eq!(hover(&mut h, item), CursorIcon::Default, "{label} must not show the crosshair on a mirrored sheet");
    }
}

/// Every committed ink stroke, its points mapped back to the screen the way the preview draws it.
fn ink_strokes_on_screen(h: &Harness) -> Vec<Vec<Pos2>> {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    let g = h.state().grid.geo.clone().expect("the grid is laid out");
    let mut strokes = vec![];
    for s in sh.shapes.iter().filter(|s| s.kind == gridcraft_engine::model::ShapeKind::Ink) {
        let ox = sh.col_left(s.anchor.cell.col) as f32 + s.anchor.dx;
        let oy = sh.row_top(s.anchor.cell.row) as f32 + s.anchor.dy;
        let mut points = vec![];
        for q in s.text.split(';').filter(|q| !q.is_empty()) {
            let (x, y) = q.split_once(',').expect("an ink point is x,y");
            let (ax, ay) = (ox + x.parse::<f32>().unwrap(), oy + y.parse::<f32>().unwrap());
            let (col, row) = (sh.col_at(f64::from(ax)), sh.row_at(f64::from(ay)));
            points.push(g.mirror_pos(pos2(g.x(sh, col) + (ax - sh.col_left(col) as f32) * g.z, g.y(sh, row) + (ay - sh.row_top(row) as f32) * g.z)));
        }
        strokes.push(points);
    }
    strokes
}

/// A stroke that starts on the sheet, crosses an open menu and comes back commits no ink over the
/// menu, and no single stroke joins the two sides across the covered cells.
#[test]
fn a_stroke_does_not_run_under_an_open_menu() {
    let mut h = harness();
    pen_on(&mut h);
    open_size_menu(&mut h);
    let items: Vec<Rect> = ["Letter", "Executive", "A3", "B4", "Japanese Postcard"].iter().map(|l| rect(&h, l)).collect();
    let menu = items.iter().fold(items[0], |a, r| a.union(*r));
    // A horizontal pass at the height of an item that lies over the sheet, from a cell left of
    // the menu to the cells right of it.
    let y = items[3].center().y;
    let start = pos2(items[0].left() - 180.0, y);
    let end = pos2(items[0].right() + 160.0, y);
    press(&mut h, start);
    for t in 1..=12 {
        let step = (end - start) * (t as f32 / 12.0);
        h.input_mut().events.push(Event::PointerMoved(start + step));
        h.run_steps(2);
    }
    release(&mut h, end);
    let strokes = ink_strokes_on_screen(&h);
    assert!(!strokes.is_empty() && strokes.iter().all(|s| !s.is_empty()), "the pen still commits ink on this pass");
    for (i, stroke) in strokes.iter().enumerate() {
        let left = stroke.iter().any(|p| p.x < menu.left());
        let right = stroke.iter().any(|p| p.x > menu.right());
        assert!(!(left && right), "stroke {i} joins across the menu instead of ending at it");
        for (k, p) in stroke.iter().enumerate() {
            assert!(!menu.contains(*p), "ink point {k} of stroke {i} lies under the menu");
        }
    }
}

/// The crosshair must not appear under a menu when the pointer reaches it during a stroke, not
/// only when it is merely hovering: the drag fell through to the cell cursor there.
#[test]
fn a_stroke_dragged_into_a_menu_shows_no_cell_cursor() {
    let mut h = harness();
    pen_on(&mut h);
    open_size_menu(&mut h);
    let items: Vec<Rect> = ["Letter", "Executive", "A3", "B4", "Japanese Postcard"].iter().map(|l| rect(&h, l)).collect();
    let y = items[3].center().y;
    let start = pos2(items[0].left() - 180.0, y);
    press(&mut h, start);
    let over = items[3].center();
    for t in 1..=8 {
        h.input_mut().events.push(Event::PointerMoved(start + (over - start) * (t as f32 / 8.0)));
        h.run_steps(2);
    }
    assert_eq!(h.output().platform_output.cursor_icon, CursorIcon::Default, "a stroke under a menu shows no cell cursor");
    release(&mut h, over);
}

/// The eraser owns its gesture over a popup too: it erases nothing through the menu, and the drag
/// does not fall through to selecting the cells underneath.
#[test]
fn an_eraser_drag_under_a_menu_erases_and_selects_nothing() {
    let mut h = harness();
    open_size_menu(&mut h);
    let items: Vec<Rect> = ["Letter", "Executive", "A3", "B4", "Japanese Postcard"].iter().map(|l| rect(&h, l)).collect();
    let menu = items.iter().fold(items[0], |a, r| a.union(*r));
    let probe = items[3].center();
    // Ink under the menu, in sheet coordinates, so "erases nothing" is not vacuous.
    let g = h.state().grid.geo.clone().expect("the grid is laid out");
    let (sx, sy) = {
        let d = h.state().session.active().expect("a document is open");
        let sh = d.wb.active().expect("the workbook has a sheet");
        let c = g.cell_at(sh, g.mirror_pos(probe));
        (sh.col_left(c.col) as f32 + (probe.x - g.x(sh, c.col)) / g.z, sh.row_top(c.row) as f32 + (probe.y - g.y(sh, c.row)) / g.z)
    };
    h.state_mut().session.execute("draw.stroke", json!({"points": [[sx - 30.0, sy], [sx, sy + 4.0], [sx + 30.0, sy]]})).expect("ink is committed");
    h.state_mut().session.execute("draw.eraser", json!({"on": true})).expect("the eraser turns on");
    h.run_steps(3);
    let ink_count = |h: &Harness| {
        let d = h.state().session.active().expect("a document is open");
        let sh = d.wb.active().expect("the workbook has a sheet");
        sh.shapes.iter().filter(|s| s.kind == gridcraft_engine::model::ShapeKind::Ink).count()
    };
    assert_eq!(ink_count(&h), 1, "the ink is there to be erased");
    let before = h.state().session.active().unwrap().selection.current().a1();
    let start = pos2(menu.left() - 180.0, probe.y);
    press(&mut h, start);
    h.input_mut().events.push(Event::PointerMoved(probe));
    h.run_steps(3);
    let during = h.state().session.active().unwrap().selection.current().a1();
    release(&mut h, probe);
    assert_eq!(during, before, "an eraser drag under a menu must not select cells");
    assert_eq!(ink_count(&h), 1, "the eraser deletes nothing through the menu");
}
