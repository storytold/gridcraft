//! Dark sheet: sparklines, shapes, text boxes, Draw ink and Page Layout headers are mapped like
//! cells (pixel tests; they return early where there is no GPU to render with).

use egui::vec2;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(Session::new()),
    );
    h.run_steps(4);
    h
}

/// Every pixel of the cell area (rows and columns), or `None` where there is no GPU.
fn grid_pixels(h: &mut Harness) -> Option<Vec<[u8; 3]>> {
    let image = h.render().ok()?;
    let r = h.state().grid.geo.as_ref()?.cells;
    let ppp = h.ctx.pixels_per_point();
    let (x0, x1, y0, y1) = ((r.left() * ppp) as u32, (r.right() * ppp) as u32, (r.top() * ppp) as u32, (r.bottom() * ppp) as u32);
    Some((y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| image.get_pixel(x, y)).map(|p| [p[0], p[1], p[2]]).collect())
}

fn go_dark(h: &mut Harness) {
    h.state_mut().run("view.darkSheet", json!({"on": true})).unwrap();
    h.run_steps(3);
}

/// No pixel is darker than the sheet's #333333 (authored dark colours would be).
fn nothing_darker_than_the_sheet(px: &[[u8; 3]]) -> bool {
    px.iter().all(|p| p.iter().all(|&v| v >= 0x30))
}

#[test]
fn sparklines_keep_their_contrast_on_the_dark_sheet() {
    let mut h = harness();
    let app = h.state_mut();
    for (i, v) in [3, 9, 1, 8, 2, 7].iter().enumerate() {
        app.run("cell.set", json!({"cell": format!("{}1", (b'A' + i as u8) as char), "input": v.to_string()})).unwrap();
    }
    app.run("insert.sparkline", json!({"range": "A1:F1", "location": "H8", "type": "column"})).unwrap();
    app.run("selection.set", json!({"cell": "A20"})).unwrap();
    h.run_steps(3);
    go_dark(&mut h);
    let Some(px) = grid_pixels(&mut h) else { return };
    assert!(nothing_darker_than_the_sheet(&px), "no authored dark ink on #333333");
    // The sparkline's authored #2B7CD3 is mapped to something lighter than the dark paper.
    assert!(!px.contains(&[0x2B, 0x7C, 0xD3]), "the authored sparkline colour is not painted on the dark sheet");
}

#[test]
fn a_text_box_with_automatic_text_is_readable_on_the_dark_sheet() {
    let mut h = harness();
    let app = h.state_mut();
    app.run("insert.textBox", json!({"at": "D8", "width": 200, "height": 60, "text": "WWWWWWWW"})).unwrap();
    app.run("selection.set", json!({"cell": "A20"})).unwrap();
    h.run_steps(3);
    go_dark(&mut h);
    let Some(px) = grid_pixels(&mut h) else { return };
    assert!(px.iter().any(|p| p.iter().all(|&v| v > 200)), "light glyphs on the dark sheet");
    assert!(px.iter().filter(|p| p.iter().all(|&v| v < 0x20)).count() == 0, "no black text or outline left on #333333");
}

#[test]
fn a_shape_fill_and_line_are_mapped_on_the_dark_sheet() {
    let mut h = harness();
    let app = h.state_mut();
    app.run("insert.shape", json!({"kind": "rectangle", "at": "D8", "width": 160, "height": 60, "fill": "DDEBF7"})).unwrap();
    app.run("selection.set", json!({"cell": "A20"})).unwrap();
    h.run_steps(3);
    let Some(light) = grid_pixels(&mut h) else { return };
    assert!(light.contains(&[0xDD, 0xEB, 0xF7]), "light sheet: the fill as authored");
    go_dark(&mut h);
    let px = grid_pixels(&mut h).unwrap();
    assert!(!px.contains(&[0xDD, 0xEB, 0xF7]), "dark sheet: the pale fill is mapped");
    assert!(px.contains(&[0x39, 0x44, 0x4E]), "the mapped fill is #39444E");
}

#[test]
fn page_layout_header_text_is_light_on_the_dark_paper() {
    let mut h = harness();
    let app = h.state_mut();
    app.run("pageLayout.headerFooter", json!({"header": "&CWWWWWWWW", "footer": "&CWWWWWWWW"})).unwrap();
    app.run("view.pageLayout", json!({})).unwrap();
    h.run_steps(4);
    let Some(light) = grid_pixels(&mut h) else { return };
    assert!(light.iter().any(|p| p.iter().all(|&v| v < 60)), "light sheet: black header text");
    go_dark(&mut h);
    let px = grid_pixels(&mut h).unwrap();
    assert!(px.iter().any(|p| p.iter().all(|&v| v > 200)), "light header text on the dark paper");
    assert!(px.iter().filter(|p| p.iter().all(|&v| v < 0x20)).count() == 0, "no black text on the dark paper");
}
