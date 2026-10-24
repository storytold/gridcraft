//! Freezing panes by dragging the freeze handles on the header edges (#195).

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::{Geo, freeze_handles};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn geo(h: &Harness) -> Geo {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll)
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(from), press(from, true)]);
    h.step();
    for i in 1..=6 {
        let t = i as f32 / 6.0;
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.input_mut().events.push(press(to, false));
    h.run_steps(3);
}

fn freeze(h: &Harness) -> Option<(u32, u32)> {
    h.state().session.active().unwrap().wb.active().unwrap().freeze
}

#[test]
fn dragging_the_handles_freezes_moves_and_unfreezes() {
    let mut h = harness();
    assert_eq!(freeze(&h), None);
    // Column handle from the corner to just past column C's left edge: two columns frozen.
    let g = geo(&h);
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let (cols, _) = freeze_handles(&g);
    let target = pos2(g.cell_rect(&sh, CellRef::new(0, 2)).left() + 3.0, g.cells.top() + 40.0);
    drag(&mut h, cols.center(), target);
    assert_eq!(freeze(&h), Some((0, 2)));

    // Row handle down to row 4's top: three rows frozen, columns kept.
    let g = geo(&h);
    let (_, rows) = freeze_handles(&g);
    let target = pos2(g.cells.left() + 40.0, g.cell_rect(&sh, CellRef::new(3, 4)).top() + 2.0);
    drag(&mut h, rows.center(), target);
    assert_eq!(freeze(&h), Some((3, 2)));

    // Both handles back to the corner unfreeze.
    let g = geo(&h);
    let (cols, _) = freeze_handles(&g);
    drag(&mut h, cols.center(), pos2(g.cells.left() - 5.0, cols.center().y));
    assert_eq!(freeze(&h), Some((3, 0)));
    let g = geo(&h);
    let (_, rows) = freeze_handles(&g);
    drag(&mut h, rows.center(), pos2(rows.center().x, g.cells.top() - 5.0));
    assert_eq!(freeze(&h), None);
}
