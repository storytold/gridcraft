//! Exercise UI paste routing with host image/snapshot fixtures, without OS clipboard or font dependencies.

use crate::SheetApp;
use gridcraft_engine::Session;
use serde_json::json;

fn fixture() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range":"A1","values":[["X","Y"],[1,2],[2,4],[3,8],[4,16]]})).unwrap();
    let id = session.execute("insert.chart", json!({"range":"A1:B5","type":"scatter","title":"Clipboard audit"})).unwrap()["chart"].as_u64().unwrap()
        as u32;
    session
        .execute("chart.addElement", json!({"chart":id,"element":"trendline","type":"exponential","displayEquation":true,"displayRSquared":true}))
        .unwrap();
    let mut app = SheetApp::new(session, Default::default());
    app.select_object_id(Some(id));
    app
}

fn clipboard(app: &mut SheetApp, as_picture: bool) -> image::RgbaImage {
    let color_image = egui::ColorImage::filled([40, 40], egui::Color32::WHITE);
    app.chart_clipboard = (!as_picture).then(|| super::ChartClipboard {
        snapshot: app.selected_chart_snapshot().unwrap(),
        document: app.session.active().unwrap().uid,
        fingerprint: super::fingerprint(&color_image),
    });
    image::RgbaImage::from_pixel(40, 40, image::Rgba([255, 255, 255, 255]))
}

#[test]
fn editable_clipboard_preserves_trendlines_on_the_same_and_another_sheet() {
    let mut app = fixture();
    let original = app.selected_chart_snapshot().unwrap();
    let image = clipboard(&mut app, false);
    app.services.paste_image = Some(Box::new(move || Some(image.clone())));
    for another_sheet in [false, true] {
        if another_sheet {
            app.run("home.insertSheet", json!({"name":"Copied charts"})).unwrap();
        }
        app.paste_from_clipboard();
        let sheet = app.session.doc().unwrap().wb.active().unwrap();
        assert!(sheet.images.is_empty(), "an owned chart snapshot remains editable");
        let pasted = sheet.charts.last().unwrap().clone();
        assert_ne!(pasted.id, original.id);
        assert_eq!(pasted.series, original.series);
        assert_eq!(pasted.format, original.format);
        app.run("edit.undo", json!({})).unwrap();
        assert!(!app.session.doc().unwrap().wb.active().unwrap().charts.iter().any(|c| c.id == pasted.id));
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().wb.active().unwrap().charts.last().unwrap(), &pasted);
    }
}

#[test]
fn picture_copies_changed_clipboards_and_other_workbooks_paste_images() {
    for mode in ["picture", "changed", "other workbook"] {
        let mut app = fixture();
        let mut image = clipboard(&mut app, mode == "picture");
        if mode == "changed" {
            image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        }
        if mode == "other workbook" {
            app.session.new_workbook();
        }
        let before = app.session.doc().unwrap().wb.active().unwrap().charts.clone();
        app.services.paste_image = Some(Box::new(move || Some(image.clone())));
        app.paste_from_clipboard();
        let sh = app.session.doc().unwrap().wb.active().unwrap();
        assert_eq!(sh.charts, before, "{mode}");
        assert_eq!(sh.images.len(), 1, "{mode}");
        assert_eq!(sh.images[0].mime, "image/png");
    }
}
