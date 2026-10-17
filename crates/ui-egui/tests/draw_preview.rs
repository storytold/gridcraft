//! Draw tab: the live stroke preview is the ink that will be committed.

use egui::{Event, Modifiers, PointerButton};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
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

#[test]
fn the_preview_uses_the_selected_pens_colour_and_width() {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = harness(s);
    h.state_mut().run("draw.pen", json!({"on": true, "color": "#D13B2F", "width": 8.0})).unwrap();

    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let b2 = geo.cell_rect(sheet, CellRef::new(1, 1));
    let start = b2.center() - egui::vec2(8.0, 4.0);
    let mid = b2.center();
    h.input_mut().events.extend([
        Event::PointerMoved(start),
        Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerMoved(mid),
    ]);
    h.run_steps(3);

    let z = h.state().grid.geo.as_ref().unwrap().z;
    let red = egui::Color32::from_rgb(0xD1, 0x3B, 0x2F);
    let found = h.output().shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Path(path)
            if matches!(&path.stroke.color, egui::epaint::ColorMode::Solid(c) if *c == red)
                && (path.stroke.width - 8.0 * z).abs() < 0.01)
    });
    assert!(found, "the in-progress stroke is painted in the pen's colour at the pen's width");
}
