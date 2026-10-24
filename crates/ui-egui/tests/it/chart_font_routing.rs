use egui::{Event, Modifiers, PointerButton, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

#[test]
fn clicking_chart_title_routes_home_font_to_title_not_cells() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1","input":"1"})).unwrap();
    let result = session.execute("insert.chart", json!({"range":"A1","type":"column","title":"Sales","at":"D3","width":300,"height":200})).unwrap();
    let id = result["chart"].as_u64().unwrap() as u32;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let cells = serde_json::to_value(&sh.cells).unwrap();
    let a = sh.charts[0].anchor;
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let point = geo.cell_rect(sh, a.cell).min + vec2(a.dx + a.width / 2.0, a.dy + 15.0) * geo.z;
    h.input_mut().events.push(Event::PointerMoved(point));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: point, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    assert_eq!(h.state().selected_chart, Some(id));
    h.state_mut().run("home.fontSize", json!({"size":24})).unwrap();
    h.run_steps(2);
    let sh = h.state().session.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.charts[0].format.text.get("title").unwrap().size, Some(24.0));
    assert_eq!(serde_json::to_value(&sh.cells).unwrap(), cells);
}
