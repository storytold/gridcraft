//! Import Brush Texture: a picked file imports, a rejected pick clears every earlier source (a
//! typed path included), and the dialog states the image limits.

use std::io::Cursor;
use std::sync::{Arc, Mutex};

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use serde_json::json;

/// A small real PNG, the image a picker returns.
fn png(w: u32, h: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 40, 90, 255]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// A harness whose picture picker reports through an inbox, as the browser build does.
fn harness(services: Services) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::prepare(SheetApp::new(session, services)),
    );
    h.run_steps(4);
    h
}

/// The requests a test's picker received, so it can answer the one the dialog made.
fn picker() -> (Services, Arc<Mutex<Vec<Inbox>>>) {
    let requests: Arc<Mutex<Vec<Inbox>>> = Arc::default();
    let send = requests.clone();
    (Services { pick_picture_async: Some(Box::new(move |inbox| send.lock().unwrap().push(inbox))), ..Default::default() }, requests)
}

#[test]
fn an_accepted_pick_imports_the_texture() {
    let (services, requests) = picker();
    let mut h = harness(services);
    h.state_mut().open_dialog("importTexture", json!({}));
    h.run_steps(4);
    h.get_by_label("Browse…").click();
    h.run_steps(4);
    requests.lock().unwrap().pop().expect("the dialog asked for a pick").lock().unwrap().push(("stripes.png".into(), png(8, 4)));
    h.run_steps(4);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.ink_textures.len(), 1);
    assert_eq!(wb.ink_textures[0].name, "stripes.png");
    assert_eq!((wb.ink_textures[0].width, wb.ink_textures[0].height), (8, 4));
}

#[test]
fn a_rejected_pick_clears_a_typed_path_too() {
    let (services, requests) = picker();
    let mut h = harness(services);
    // A path typed before the pick (a scripted dialog can carry one) must not survive a pick the
    // picker refused, or OK would import it while the dialog says no image was chosen.
    h.state_mut().open_dialog("importTexture", json!({}));
    h.run_steps(2);
    h.state_mut().dialog.as_mut().expect("the dialog is open").values.insert("path".into(), json!("earlier.png"));
    h.run_steps(2);
    h.get_by_label("Browse…").click();
    h.run_steps(4);
    requests.lock().unwrap().pop().expect("a pick was asked for").lock().unwrap().push(("huge.png".into(), Vec::new()));
    h.run_steps(4);
    let values = &h.state().dialog.as_ref().expect("the dialog is open").values;
    assert!(!values.contains_key("path"), "the refused pick clears the earlier path: {values:?}");
    assert!(!values.contains_key("base64") && !values.contains_key("name"));
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().session.active().unwrap().wb.ink_textures.is_empty(), "nothing was imported");
}

#[test]
fn an_oversized_pick_is_refused_and_the_dialog_states_the_limits() {
    let (services, requests) = picker();
    let mut h = harness(services);
    h.state_mut().open_dialog("importTexture", json!({}));
    h.run_steps(4);
    // The limits are on the dialog itself, not only in the message after a refusal.
    let limits = "PNG or JPEG, up to 16 MiB, 8192 pixels a side and 16 million pixels.";
    assert!(h.query_by_label(limits).is_some(), "the dialog states the image limits");
    h.get_by_label("Browse…").click();
    h.run_steps(4);
    requests.lock().unwrap().pop().expect("a pick was asked for").lock().unwrap().push(("big.png".into(), vec![0u8; 16 * 1024 * 1024 + 1]));
    h.run_steps(4);
    assert!(!h.state().dialog.as_ref().expect("the dialog is open").values.contains_key("base64"));
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().session.active().unwrap().wb.ink_textures.is_empty(), "an oversized image is not imported");
}
