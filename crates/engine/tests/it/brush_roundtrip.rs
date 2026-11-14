use gridcraft_engine::Session;
use gridcraft_engine::brush::{Preset, Style};
use gridcraft_engine::model::{Anchor, AnchorMode, Image};
use serde_json::json;

fn pictures(session: &Session) -> &[Image] {
    &session.doc().unwrap().wb.active().unwrap().images
}

fn assert_same_anchor(actual: Anchor, expected: Anchor) {
    assert_eq!(actual.mode, expected.mode);
    assert_eq!(actual.cell, expected.cell);
    for (a, b) in [(actual.dx, expected.dx), (actual.dy, expected.dy), (actual.width, expected.width), (actual.height, expected.height)] {
        assert!((a - b).abs() < 0.001, "anchor: {actual:?} != {expected:?}");
    }
}

#[test]
fn brush_is_editable_after_xlsx_save_and_uses_one_undo_step() {
    let mut session = Session::new();
    session.new_workbook();
    let style = Style::for_preset(Preset::Chalk);
    session
        .execute(
            "draw.brushStroke",
            json!({"style": style, "samples": [
                {"x": 0.1, "y": 0.2, "pressure": 0.5},
                {"x": 65.0, "y": 40.0, "pressure": 0.75, "timeMs": 10},
                {"x": 140.0, "y": 25.0, "timeMs": 20}
            ]}),
        )
        .unwrap();
    let original = pictures(&session)[0].clone();
    assert_eq!(original.anchor.mode, AnchorMode::Absolute);
    session.execute("edit.undo", json!({})).unwrap();
    assert!(pictures(&session).is_empty());
    session.execute("edit.redo", json!({})).unwrap();
    assert_eq!(pictures(&session)[0], original);

    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "brush.xlsx", "base64": saved["base64"]})).unwrap();
    let pic = &pictures(&reopened)[0];
    assert_eq!(pic.data, original.data);
    assert_same_anchor(pic.anchor, original.anchor);
    let id = pic.id;
    let source = reopened.execute("draw.brushInspect", json!({"id": id})).unwrap();
    assert_eq!(source["style"], json!(style));
    assert_eq!(source["samples"][1]["pressure"], 0.75);

    let restyle = Style { color: [181, 35, 58], ..style };
    reopened.execute("draw.brushEdit", json!({"id": id, "style": restyle})).unwrap();
    assert_ne!(pictures(&reopened)[0].data, original.data);
    assert_same_anchor(pictures(&reopened)[0].anchor, original.anchor);
    reopened.execute("edit.undo", json!({})).unwrap();
    assert_eq!(pictures(&reopened)[0].data, original.data);

    let pdf = reopened.execute("file.exportPdf", json!({})).unwrap();
    let bytes = gridcraft_engine::io::base64_decode(pdf["base64"].as_str().unwrap()).unwrap();
    assert!(bytes.windows(b"/Subtype /Image".len()).any(|w| w == b"/Subtype /Image"));
}

#[test]
fn rejected_brush_does_not_add_a_picture_or_change_undo() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B2", "input": "before"})).unwrap();
    assert!(session.execute("draw.brushStroke", json!({"samples": [{"x": 4.0}, {"x": 800000.0}]})).is_err());
    assert!(pictures(&session).is_empty());
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(session.execute("cell.get", json!({"cell": "B2"})).unwrap()["input"], "");
}

#[test]
fn restyling_at_a_lower_resolution_keeps_the_worksheet_origin() {
    let mut session = Session::new();
    session.new_workbook();
    let style = Style::default();
    let id = session
        .execute("draw.brushStroke", json!({"style": style, "samples": [{"x": 200.0, "y": 160.0}, {"x": 900.0, "y": 610.0}]}))
        .unwrap()["image"]
        .clone();
    let old = gridcraft_engine::brush::source(&pictures(&session)[0].data).unwrap();
    session.execute("draw.brushEdit", json!({"id": id, "style": Style { size: 64.0, ..style }})).unwrap();
    let pic = &pictures(&session)[0];
    let new = gridcraft_engine::brush::source(&pic.data).unwrap();
    assert!(new.pixels_per_unit < old.pixels_per_unit);
    let a = pic.anchor;
    let sheet = session.doc().unwrap().wb.active().unwrap();
    let x = sheet.col_left(a.cell.col) + f64::from(a.dx) - f64::from(new.offset[0]) / new.pixels_per_unit;
    let y = sheet.row_top(a.cell.row) + f64::from(a.dy) - f64::from(new.offset[1]) / new.pixels_per_unit;
    assert!((x - 200.0).abs() < 0.001 && (y - 160.0).abs() < 0.001);
    assert!(a.width > 700.0 && a.height > 450.0);
}

#[test]
fn moved_and_resized_brush_keeps_its_scale_when_restyled() {
    let mut session = Session::new();
    session.new_workbook();
    let style = Style::default();
    let id = session
        .execute("draw.brushStroke", json!({"style": style, "samples": [{"x": 200.0, "y": 160.0}, {"x": 900.0, "y": 610.0}]}))
        .unwrap()["image"]
        .clone();
    let original = pictures(&session)[0].anchor;
    session
        .execute(
            "object.move",
            json!({"kind": "image", "id": id, "at": "G12", "dx": 15.0, "dy": 8.0,
                "width": original.width * 1.5, "height": original.height * 0.75}),
        )
        .unwrap();
    let resized = pictures(&session)[0].anchor;
    session.execute("draw.brushEdit", json!({"id": id, "style": Style { color: [181, 35, 58], ..style }})).unwrap();
    assert_same_anchor(pictures(&session)[0].anchor, resized);
    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "scaled-brush.xlsx", "base64": saved["base64"]})).unwrap();
    assert_same_anchor(pictures(&reopened)[0].anchor, resized);
}
