//! Floating pictures on the sheet, with original synthetic pictures.

use std::io::Cursor;

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Shape, TextureId, pos2, vec2};
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::ObjRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn png(color: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(120, 60, image::Rgba(color));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::blank_harness(4)
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

fn picture_clips(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Mesh(mesh) if mesh.texture_id != TextureId::default() => Some(shape.clip_rect),
            _ => None,
        })
        .collect()
}

fn handle_clips(h: &egui_kittest::Harness<'static, SheetApp>, cells: Rect) -> Vec<Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Circle(circle) if (circle.radius - gridcraft_ui_egui::frame::HANDLE_R).abs() < 0.01 && shape.clip_rect.intersects(cells) => {
                Some(shape.clip_rect)
            }
            _ => None,
        })
        .collect()
}

fn current_geo(h: &egui_kittest::Harness<'static, SheetApp>) -> Geo {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll)
}

fn click_screen(h: &mut egui_kittest::Harness<'static, SheetApp>, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn insert_picture(h: &mut egui_kittest::Harness<'static, SheetApp>, at: &str, width: f32, height: f32, color: [u8; 4]) -> u32 {
    let data = gridcraft_engine::io::base64_encode(&png(color));
    let out = h.state_mut().run("insert.picture", json!({"base64": data, "at": at, "width": width, "height": height})).unwrap();
    out["image"].as_u64().unwrap() as u32
}

/// Regression test for #4727: scrolling down with frozen rows must clip images at the freeze
/// boundary, keep selection handles clipped, and leave clicks on frozen cells hitting those cells.
#[test]
fn scrolled_picture_clips_at_frozen_rows_and_disappears_behind_them() {
    let mut h = harness();
    let id = insert_picture(&mut h, "C4", 160.0, 100.0, [210, 50, 50, 255]);
    h.state_mut().run("selection.set", json!({"cell": "A3"})).unwrap();
    h.state_mut().run("view.freezePanes", json!({})).unwrap();
    h.run_steps(3);

    // Scroll so the image at C4 straddles the horizontal freeze line (top behind frozen rows 1-2).
    h.state_mut().view_mut().unwrap().scroll = vec2(0.0, 45.0);
    h.state_mut().select_object_id(Some(id));
    h.run_steps(3);

    let geo = current_geo(&h);
    let split_y = geo.cells.top() + geo.frozen_h;
    let clips = picture_clips(&h);
    assert_eq!(clips.len(), 1, "partially visible image paints once in scrolling pane");
    assert!(clips[0].top() >= split_y - 0.01, "image clip {:?} must not enter frozen rows (< {split_y})", clips[0]);

    let handles = handle_clips(&h, geo.cells);
    assert!(!handles.is_empty(), "selected image still shows handles in scrolling pane");
    assert!(handles.iter().all(|c| c.top() >= split_y - 0.01), "selection handles must not paint over frozen rows: {handles:?}");

    // The top-center resize handle scrolled behind the frozen rows must not be grabbable.
    let hidden_top_handle = {
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        let rect = gridcraft_ui_egui::chartview::object_rect(&geo, sh, ObjRef::new(gridcraft_engine::model::ObjKind::Image, id)).unwrap();
        rect.center_top()
    };
    assert!(hidden_top_handle.y < split_y, "test setup expects top handle above split_y");
    {
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        assert_eq!(
            gridcraft_ui_egui::chartview::selected_handle(h.state(), &geo, sh, hidden_top_handle),
            None,
            "hidden resize handle behind frozen rows must not hit"
        );
    }

    // Clicking in frozen row 2 directly above the scrolled-under top of the image must select the
    // frozen cell C2, not keep/select the hidden portion of the image.
    let frozen_pt = {
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        geo.cell_rect(sh, CellRef::parse("C2").unwrap()).center()
    };
    click_screen(&mut h, frozen_pt);
    assert_eq!(h.state().selected_object, None, "click on frozen header cell must not hit hidden image");
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::parse("C2").unwrap());

    // Clicking the visible bottom part of the image below the freeze line selects it.
    let visible_pt = {
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        let rect = gridcraft_ui_egui::chartview::object_rect(&geo, sh, ObjRef::new(gridcraft_engine::model::ObjKind::Image, id)).unwrap();
        pos2(rect.center().x, (split_y + rect.bottom()) * 0.5)
    };
    click_screen(&mut h, visible_pt);
    assert_eq!(h.state().selected_object.map(|o| o.id), Some(id));

    // Scrolling the image wholly above the freeze line removes both image and handles from painting.
    h.state_mut().view_mut().unwrap().scroll = vec2(0.0, 200.0);
    h.run_steps(3);
    assert!(picture_clips(&h).is_empty(), "fully scrolled-under image must not paint");
    assert!(handle_clips(&h, geo.cells).is_empty(), "fully scrolled-under selected image must not paint handles");
}

#[test]
fn scrolled_picture_clips_at_frozen_columns_and_both_panes() {
    let mut h = harness();
    let scrolled_id = insert_picture(&mut h, "D4", 140.0, 90.0, [210, 50, 50, 255]);
    let frozen_id = insert_picture(&mut h, "A1", 50.0, 24.0, [40, 160, 80, 255]);
    h.state_mut().run("selection.set", json!({"cell": "C3"})).unwrap();
    h.state_mut().run("view.freezePanes", json!({})).unwrap();
    h.run_steps(3);

    // Scroll right and down so D4 crosses both frozen columns (A:B) and frozen rows (1:2).
    h.state_mut().view_mut().unwrap().scroll = vec2(120.0, 40.0);
    h.run_steps(3);

    let geo = current_geo(&h);
    let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
    let clips = picture_clips(&h);
    assert_eq!(clips.len(), 2, "one image in scrolling pane and one in top-left frozen corner");
    assert!(
        clips.iter().any(|c| c.left() >= split.x - 0.01 && c.top() >= split.y - 0.01),
        "scrolled image must clip to scrolling pane ({split:?}): {clips:?}"
    );
    assert!(
        clips.iter().any(|c| c.right() <= split.x + 0.01 && c.bottom() <= split.y + 0.01),
        "frozen corner image must stay inside top-left frozen pane ({split:?}): {clips:?}"
    );

    // Clicking inside the frozen-column band where the scrolled D4 image's unclipped rect overlaps
    // it must select the frozen-column cell there, not the hidden portion of D4.
    let (frozen_col_pt, expected_cell) = {
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        let scrolled_rect =
            gridcraft_ui_egui::chartview::object_rect(&geo, sh, ObjRef::new(gridcraft_engine::model::ObjKind::Image, scrolled_id)).unwrap();
        let frozen_col_pane = Rect::from_min_max(pos2(geo.cells.left(), split.y), pos2(split.x, geo.cells.bottom()));
        let overlap = scrolled_rect.intersect(frozen_col_pane);
        assert!(overlap.is_positive(), "test setup expects scrolled D4 ({scrolled_rect:?}) to overlap frozen columns ({frozen_col_pane:?})");
        let pt = overlap.center();
        (pt, geo.cell_at(sh, pt))
    };
    click_screen(&mut h, frozen_col_pt);
    assert_ne!(h.state().selected_object.map(|o| o.id), Some(scrolled_id));
    assert_eq!(h.state().session.active().unwrap().selection.active, expected_cell);

    // Clicking the visible frozen-corner image selects it.
    let frozen_img_pt = {
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        gridcraft_ui_egui::chartview::object_rect(&geo, sh, ObjRef::new(gridcraft_engine::model::ObjKind::Image, frozen_id)).unwrap().center()
    };
    click_screen(&mut h, frozen_img_pt);
    assert_eq!(h.state().selected_object.map(|o| o.id), Some(frozen_id));
}

#[test]
fn scrolled_picture_clips_behind_frozen_columns_in_rtl_and_zoom() {
    let mut h = harness();
    let id = insert_picture(&mut h, "D3", 140.0, 80.0, [50, 90, 210, 255]);
    h.state_mut().run("view.rightToLeft", json!({"on": true})).unwrap();
    h.state_mut().run("view.zoom", json!({"percent": 150})).unwrap();
    h.state_mut().run("selection.set", json!({"cell": "C1"})).unwrap();
    h.state_mut().run("view.freezePanes", json!({})).unwrap();
    h.run_steps(3);

    h.state_mut().view_mut().unwrap().scroll = vec2(120.0, 0.0);
    h.run_steps(3);

    let geo = current_geo(&h);
    let logical_split_x = geo.cells.left() + geo.frozen_w;
    let screen_split_x = geo.mirror_x(logical_split_x);
    let clips = picture_clips(&h);
    assert_eq!(clips.len(), 1);
    assert!(
        clips[0].right() <= screen_split_x + 0.01,
        "in RTL, scrolled image clip {:?} must stay left of mirrored frozen split {screen_split_x}",
        clips[0]
    );

    // Clicking in the mirrored frozen-column band where the scrolled image's unclipped rect
    // overlaps it must select the frozen cell there, not the hidden portion of the image.
    let (frozen_pt, expected_cell) = {
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        let scrolled_rect = gridcraft_ui_egui::chartview::object_rect(&geo, sh, ObjRef::new(gridcraft_engine::model::ObjKind::Image, id)).unwrap();
        let logical_frozen_pane = Rect::from_min_max(geo.cells.min, pos2(logical_split_x, geo.cells.bottom()));
        let overlap = scrolled_rect.intersect(logical_frozen_pane);
        assert!(overlap.is_positive(), "test setup expects scrolled D3 ({scrolled_rect:?}) to overlap frozen columns ({logical_frozen_pane:?})");
        let logical_pt = overlap.center();
        (geo.mirror_pos(logical_pt), geo.cell_at(sh, logical_pt))
    };
    click_screen(&mut h, frozen_pt);
    assert_ne!(h.state().selected_object.map(|o| o.id), Some(id));
    assert_eq!(h.state().session.active().unwrap().selection.active, expected_cell);
}

#[test]
fn picture_spanning_freeze_line_splits_cleanly_across_panes() {
    let mut h = harness();
    let _id = insert_picture(&mut h, "B1", 120.0, 100.0, [180, 80, 190, 255]);
    h.state_mut().run("selection.set", json!({"cell": "A3"})).unwrap();
    h.state_mut().run("view.freezePanes", json!({})).unwrap();
    h.run_steps(3);

    let geo = current_geo(&h);
    let split_y = geo.cells.top() + geo.frozen_h;
    let clips = picture_clips(&h);
    assert_eq!(clips.len(), 2, "unscrolled spanning image paints in both frozen and scrolling panes");
    assert!(clips.iter().any(|c| c.bottom() <= split_y + 0.01), "top part clipped to frozen pane: {clips:?}");
    assert!(clips.iter().any(|c| c.top() >= split_y - 0.01), "bottom part clipped to scrolling pane: {clips:?}");

    // Scrolling down moves the bottom portion out of the scrolling pane while keeping the top pinned.
    h.state_mut().view_mut().unwrap().scroll = vec2(0.0, 120.0);
    h.run_steps(3);
    let scrolled_clips = picture_clips(&h);
    assert_eq!(scrolled_clips.len(), 1, "only the pinned frozen portion remains after scrolling past it");
    assert!(scrolled_clips[0].bottom() <= split_y + 0.01, "remaining clip must stay in frozen pane: {scrolled_clips:?}");
}
