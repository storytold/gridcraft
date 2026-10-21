//! Real-pointer regression coverage for draggable Page Break Preview boundaries.
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::{Session, core::RangeRef, model::PrintSettings};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Opens a synthetic print area with both manual axes, optionally mirrored and zoomed.
fn fixture(rtl: bool, zoom: u32) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell":"M35","input":"outside print area"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"printArea":"B2:L30", "rowBreaks":[12], "colBreaks":["G"]})).unwrap();
    s.execute("view.pageBreakPreview", json!({})).unwrap();
    s.execute("view.zoom", json!({"percent":zoom})).unwrap();
    if rtl {
        s.execute("view.rightToLeft", json!({"on":true})).unwrap();
    }
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 1100.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

/// Copies saved settings so a test can compare them across undo and pointer input.
fn settings(h: &Harness) -> PrintSettings {
    h.state().session.active().unwrap().wb.active().unwrap().print.clone()
}

/// Converts a column/row boundary to the actual screen position, including RTL and scrolling.
fn point(h: &Harness, col: u32, row: u32) -> Pos2 {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    let g = h.state().grid.geo.as_ref().unwrap();
    g.mirror_pos(pos2(g.x(sh, col), g.y(sh, row)))
}

/// Sends actual press, motion and release events, or Escape before release to cancel.
fn drag(h: &mut Harness, from: Pos2, to: Pos2, cancel: bool) {
    let button = |pos, pressed| Event::PointerButton { pos, pressed, button: PointerButton::Primary, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(from), button(from, true)]);
    h.step();
    for i in 1..=6 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
        h.step();
    }
    if cancel {
        h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.step();
    }
    h.input_mut().events.push(button(to, false));
    h.run_steps(3);
}

/// All four outer sides snap to cells, preserve contents and selection, and undo in one step.
#[test]
fn outer_borders_resize_and_undo_in_ltr_and_rtl() {
    for rtl in [false, true] {
        for (from, to, expected) in
            [((12, 5), (8, 5), "B2:H30"), ((1, 5), (3, 5), "D2:L30"), ((4, 30), (4, 22), "B2:L22"), ((4, 1), (4, 4), "B5:L30")]
        {
            let mut h = fixture(rtl, 80);
            let before = settings(&h);
            let selection = h.state().session.active().unwrap().selection.clone();
            let (a, b) = (point(&h, from.0, from.1), point(&h, to.0, to.1));
            drag(&mut h, a, b, false);
            assert_eq!(settings(&h).print_area, RangeRef::parse(expected));
            assert_eq!(h.state().session.active().unwrap().selection, selection);
            h.state_mut().run("edit.undo", json!({})).unwrap();
            assert_eq!(serde_json::to_value(settings(&h)).unwrap(), serde_json::to_value(before).unwrap());
            assert_eq!(
                h.state().session.active().unwrap().wb.active().unwrap().value(gridcraft_engine::core::CellRef::new(34, 12)),
                gridcraft_engine::core::Value::Text("outside print area".into())
            );
        }
    }
}

/// Moving either internal axis replaces its break, keeps the other axis, and Escape cancels.
#[test]
fn internal_breaks_move_and_escape_cancels() {
    for rtl in [false, true] {
        let mut h = fixture(rtl, 100);
        let (a, b) = (point(&h, 4, 11), point(&h, 4, 17));
        drag(&mut h, a, b, true);
        assert_eq!(settings(&h).row_breaks, vec![11]);
        drag(&mut h, a, b, false);
        assert_eq!(settings(&h).row_breaks, vec![17]);
        assert_eq!(settings(&h).col_breaks, vec![6]);
        let (a, b) = (point(&h, 6, 5), point(&h, 8, 5));
        drag(&mut h, a, b, false);
        assert_eq!(settings(&h).col_breaks, vec![8]);
        assert_eq!(settings(&h).row_breaks, vec![17]);
    }
}

/// Scrolled, frozen and zoomed geometry still changes the intended column boundary.
#[test]
fn borders_follow_scrolling_and_frozen_panes() {
    let mut h = fixture(false, 125);
    h.state_mut().run("view.freezePanes", json!({"cell":"C4"})).unwrap();
    h.state_mut().view_mut().unwrap().scroll = vec2(120.0, 80.0);
    h.run_steps(4);
    let (a, b) = (point(&h, 12, 8), point(&h, 9, 8));
    drag(&mut h, a, b, false);
    assert_eq!(settings(&h).print_area, RangeRef::parse("B2:I30"));
}

/// An outer-border click and a cancelled resize must not create a new undoable edit.
#[test]
fn click_and_cancel_leave_settings_unchanged() {
    let mut h = fixture(false, 100);
    let before = serde_json::to_value(settings(&h)).unwrap();
    let a = point(&h, 12, 5);
    drag(&mut h, a, a, false);
    assert_eq!(serde_json::to_value(settings(&h)).unwrap(), before);
    let b = point(&h, 8, 5);
    drag(&mut h, a, b, true);
    assert_eq!(serde_json::to_value(settings(&h)).unwrap(), before);
    assert!(h.state().grid.page_break_drag.is_none());
}

/// Ordinary cell selection retains the release event when its endpoint lands on a blue boundary.
#[test]
fn ordinary_selection_can_end_on_a_page_boundary() {
    let mut h = fixture(false, 100);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    let g = h.state().grid.geo.as_ref().unwrap();
    let from = g.cell_rect(sh, gridcraft_engine::core::CellRef::new(3, 2)).center();
    let to = point(&h, 6, 20);
    drag(&mut h, from, to, false);
    assert_eq!(h.state().grid.drag, gridcraft_ui_egui::grid::Drag::None);
    assert_eq!(settings(&h).print_area, RangeRef::parse("B2:L30"));
    assert_eq!(settings(&h).col_breaks, vec![6]);
}
