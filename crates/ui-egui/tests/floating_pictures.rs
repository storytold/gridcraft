//! Floating pictures on the sheet, with original synthetic pictures.

use std::io::Cursor;

use egui::{Shape, TextureId, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn png(color: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(120, 60, image::Rgba(color));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn picture_textures(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<TextureId> {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Mesh(mesh) if mesh.texture_id != TextureId::default() => Some(mesh.texture_id),
            _ => None,
        })
        .collect()
}

/// Picture ids are per workbook, so two open workbooks both have a picture 1. Each must show its
/// own picture, even when the two are the same number of bytes: the texture cache used to tell
/// pictures apart by id and size alone, for the whole app.
#[test]
fn each_workbook_shows_its_own_picture() {
    let (green, red) = (png([24, 150, 110, 255]), png([200, 40, 40, 255]));
    assert_eq!(green.len(), red.len());
    let mut h = harness();
    h.state_mut().run("insert.picture", json!({"base64": gridcraft_engine::io::base64_encode(&green), "at": "B2"})).unwrap();
    h.run_steps(4);
    let first = picture_textures(&h);
    assert_eq!(first.len(), 1, "the picture is painted");

    h.state_mut().run("file.new", json!({})).unwrap();
    h.state_mut().run("insert.picture", json!({"base64": gridcraft_engine::io::base64_encode(&red), "at": "B2"})).unwrap();
    h.run_steps(4);
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.wb.active().unwrap().images[0].id, h.state().session.documents()[0].wb.active().unwrap().images[0].id);
    let second = picture_textures(&h);
    assert_eq!(second.len(), 1);
    assert_ne!(second, first, "another workbook's picture 1 has a texture of its own");

    // Closing the workbook and painting again leaves the first one's picture as it was.
    h.state_mut().run("file.close", json!({"force": true})).unwrap();
    h.run_steps(4);
    assert_eq!(picture_textures(&h), first);
}
