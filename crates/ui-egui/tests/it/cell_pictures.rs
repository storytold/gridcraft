//! Original synthetic pictures exercise the UI without an external image fixture.

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use egui::{Rect, Shape, TextureId, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{Inbox, Services, SheetApp, grid::Geo};
use serde_json::json;

fn png() -> Vec<u8> {
    let image = image::RgbaImage::from_fn(120, 60, |x, _| {
        if x < 40 {
            image::Rgba([24, 150, 110, 255])
        } else if x < 80 {
            image::Rgba([245, 190, 55, 255])
        } else {
            image::Rgba([62, 104, 204, 255])
        }
    });
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn harness(services: Services) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    h
}

fn confirm(h: &mut egui_kittest::Harness<'static, SheetApp>) {
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
}

fn image_meshes(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<(Rect, Rect)> {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Mesh(mesh) if mesh.texture_id != TextureId::default() => {
                let mut bounds = Rect::NOTHING;
                for vertex in &mesh.vertices {
                    bounds.extend_with(vertex.pos);
                }
                Some((bounds, shape.clip_rect))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn picture_file_pickers_preserve_placement_and_isolate_canceled_results() {
    let bytes = png();
    let path = std::env::temp_dir().join(format!("gridcraft-cell-picture-ui-{}.png", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let chosen = path.to_string_lossy().to_string();
    let mut h = harness(Services { pick_picture: Some(Box::new(move || Some(chosen.clone()))), ..Default::default() });
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(6);
    h.get_by_label("Pictures").click();
    h.run_steps(6);
    h.get_by_label("Place in Cell").click();
    h.run_steps(6);
    h.get_by_label("Browse…").click();
    h.run_steps(6);
    // Moving the selection while the chooser is open must not retarget the insert.
    h.state_mut().run("selection.set", json!({"cell": "B2"})).unwrap();
    confirm(&mut h);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.cell_pictures.get(&CellRef::new(0, 0)).unwrap().data, bytes);
    assert!(sheet.cell(CellRef::new(1, 1)).is_none());
    assert!(sheet.images.is_empty());
    std::fs::remove_file(path).unwrap();

    let requests: Arc<Mutex<Vec<Inbox>>> = Arc::default();
    let send = requests.clone();
    let mut web = harness(Services { pick_picture_async: Some(Box::new(move |inbox| send.lock().unwrap().push(inbox))), ..Default::default() });
    web.state_mut().open_dialog("insertPicture", json!({"placement": "cell"}));
    web.run_steps(6);
    web.get_by_label("Browse…").click();
    web.run_steps(6);
    let canceled = requests.lock().unwrap().pop().unwrap();
    web.state_mut().dialog = None;
    web.state_mut().open_dialog("insertPicture", json!({"placement": "overCells"}));
    canceled.lock().unwrap().push(("late.png".into(), bytes.clone()));
    web.run_steps(6);
    assert!(!web.state().dialog.as_ref().unwrap().values.contains_key("base64"));
    web.get_by_label("Browse…").click();
    web.run_steps(6);
    requests.lock().unwrap().pop().unwrap().lock().unwrap().push(("stripes.png".into(), bytes));
    web.run_steps(6);
    confirm(&mut web);
    let sheet = web.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.images.len(), 1);
    assert_eq!(sheet.images[0].alt, "stripes.png");
    assert!(sheet.cell(CellRef::new(0, 0)).is_none());
}

#[test]
fn pictures_fit_current_geometry_and_selection_cannot_erase_them() {
    let mut h = harness(Default::default());
    h.state_mut()
        .run("insert.picture", json!({"base64": gridcraft_engine::io::base64_encode(&png()), "placement": "cell", "alt": "Three stripes"}))
        .unwrap();
    h.state_mut().run("home.rowHeight", json!({"height": 100})).unwrap();
    h.state_mut().run("home.columnWidth", json!({"width": 150})).unwrap();
    h.run_steps(6);
    let meshes = image_meshes(&h);
    assert_eq!(meshes.len(), 1);
    assert!((meshes[0].0.width() / meshes[0].0.height() - 2.0).abs() < 0.001);
    let d = h.state().session.active().unwrap();
    let sheet = d.wb.active().unwrap();
    let geo = Geo::new(sheet, h.state().grid.rect.unwrap(), h.state().view().scroll);
    let cell = geo.cell_rect(sheet, CellRef::new(0, 0));
    assert!(cell.contains_rect(meshes[0].0));
    assert_eq!(cell.center(), meshes[0].0.center());
    h.get_by_label("Picture: Three stripes");
    h.state_mut().begin_edit(None, false);
    assert!(h.state().editor.is_none());
    h.state_mut().begin_edit(None, true);
    assert!(h.state().editor.is_none());

    h.state_mut().run("home.mergeCells", json!({"range": "A1:B2"})).unwrap();
    h.state_mut().run("view.zoom", json!({"percent": 150})).unwrap();
    h.state_mut().run("view.freezePanes", json!({"cell": "C3"})).unwrap();
    h.run_steps(6);
    let resized = image_meshes(&h);
    assert_eq!(resized.len(), 1);
    assert!(resized[0].0.width() > meshes[0].0.width());
    assert!((resized[0].0.width() / resized[0].0.height() - 2.0).abs() < 0.001);

    h.state_mut().run("home.unmergeCells", json!({"range": "A1:B2"})).unwrap();
    h.state_mut().run("home.rowHeight", json!({"rows": "A1", "height": 0})).unwrap();
    h.run_steps(6);
    assert!(image_meshes(&h).is_empty());
    h.state_mut().run("home.rowHeight", json!({"rows": "A1", "height": 100})).unwrap();
    h.state_mut().begin_edit(Some("replacement".into()), false);
    h.state_mut().commit_edit(0, 0, false, false);
    h.run_steps(6);
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sheet.value(CellRef::new(0, 0)), Value::from("replacement"));
    assert!(!sheet.cell_pictures.contains_key(&CellRef::new(0, 0)));
}
