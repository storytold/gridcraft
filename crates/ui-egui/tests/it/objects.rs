//! Floating objects through real pointer and keyboard input: selecting every kind, resizing from
//! each of the eight handles, deleting with Delete and Backspace, and double-click routing.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{ObjKind, ObjRef};
use gridcraft_ui_egui::objects::{Handle, OpenAction, open_action};
use gridcraft_ui_egui::{SheetApp, chartview, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

struct Objects {
    chart: ObjRef,
    image: ObjRef,
    shape: ObjRef,
    text_box: ObjRef,
    equation: ObjRef,
    /// A slicer lying over the chart's right edge: slicers paint on top and take the click.
    slicer: ObjRef,
}

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

fn id(v: &serde_json::Value, key: &str) -> u32 {
    v[key].as_u64().unwrap() as u32
}

fn fixture() -> (Harness, Objects) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Qty"], ["East", 3], ["West", 5], ["East", 2], ["North", 8]]})).unwrap();
    s.execute("insert.table", json!({"range": "A1:B5", "name": "Sales"})).unwrap();
    s.execute("selection.set", json!({"cell": "A2"})).unwrap();
    let slicer = id(&s.execute("insert.slicer", json!({"column": "Region", "at": "G9", "width": 140, "height": 120})).unwrap(), "slicer");
    s.execute("cell.set", json!({"cell": "R30", "input": "keep me"})).unwrap();
    s.execute("selection.set", json!({"cell": "R30"})).unwrap();
    let chart =
        id(&s.execute("insert.chart", json!({"range": "B1:B5", "type": "column", "at": "D8", "width": 220, "height": 140})).unwrap(), "chart");
    let shape = id(&s.execute("insert.shape", json!({"kind": "rectangle", "at": "C20", "width": 120, "height": 80})).unwrap(), "shape");
    let text_box = id(&s.execute("insert.textBox", json!({"at": "H20", "width": 140, "height": 60, "text": "Note"})).unwrap(), "shape");
    let equation = id(&s.execute("insert.equation", json!({"latex": "a^2+b^2=c^2", "cell": "C26", "size": 16})).unwrap(), "equation");
    let image = id(&s.execute("insert.picture", json!({"base64": tiny_png(), "at": "M8", "width": 60, "height": 60})).unwrap(), "image");
    let objs = Objects {
        chart: ObjRef::new(ObjKind::Chart, chart),
        image: ObjRef::new(ObjKind::Image, image),
        shape: ObjRef::new(ObjKind::Shape, shape),
        text_box: ObjRef::new(ObjKind::Shape, text_box),
        equation: ObjRef::new(ObjKind::Equation, equation),
        slicer: ObjRef::new(ObjKind::Slicer, slicer),
    };
    let mut h = egui_kittest::Harness::builder()
        .with_size(vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_os(egui::os::OperatingSystem::Mac)
        .build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            SheetApp::new(s, Default::default()),
        );
    h.run_steps(4);
    (h, objs)
}

fn tiny_png() -> String {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 120, 200, 255]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    gridcraft_engine::io::base64_encode(&bytes)
}

fn rect(h: &Harness, o: ObjRef) -> Rect {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    chartview::object_rect(&Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll), sh, o).unwrap()
}

/// A point that selects `o` without doing anything else: the chart's lower left (clear of the
/// slicer), the slicer's header (clear of its value tiles), the middle of anything else.
fn grab_point(h: &Harness, objs: &Objects, o: ObjRef) -> Pos2 {
    let r = rect(h, o);
    if o == objs.chart {
        r.left_bottom() + vec2(12.0, -12.0)
    } else if o == objs.slicer {
        r.left_top() + vec2(14.0, 10.0)
    } else {
        r.center()
    }
}

fn exists(h: &Harness, o: ObjRef) -> bool {
    h.state().session.doc().unwrap().wb.active().unwrap().has_object(o)
}

fn pointer(h: &mut Harness, p: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.step();
}

fn click(h: &mut Harness, p: Pos2) {
    pointer(h, p, true);
    pointer(h, p, false);
    h.run_steps(2);
}

fn double_click(h: &mut Harness, p: Pos2) {
    // Separate the gesture from earlier clicks (egui's triple-click window).
    h.run_steps(45);
    click(h, p);
    click(h, p);
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    h.run_steps(45);
    pointer(h, from, true);
    h.input_mut().events.push(Event::PointerMoved(from + (to - from) * 0.25));
    h.step();
    h.input_mut().events.push(Event::PointerMoved(to));
    h.step();
    pointer(h, to, false);
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn kept(h: &Harness) -> Value {
    h.state().session.doc().unwrap().wb.active().unwrap().value(at("R30"))
}

#[test]
fn a_click_selects_every_kind_and_slicers_win_over_charts() {
    let (mut h, o) = fixture();
    for obj in [o.chart, o.image, o.shape, o.text_box, o.equation, o.slicer] {
        let p = grab_point(&h, &o, obj);
        click(&mut h, p);
        assert_eq!(h.state().selected_object, Some(obj), "{obj:?}");
    }
    let (chart, slicer) = (rect(&h, o.chart), rect(&h, o.slicer));
    assert!(chart.contains(grab_point(&h, &o, o.slicer)) && slicer.intersects(chart), "the slicer's header lies over the chart");
    assert_eq!(h.state().selected_chart(), None, "a slicer is not a chart");
    click(&mut h, chart.right_bottom() + vec2(120.0, 160.0));
    assert_eq!(h.state().selected_object, None, "a click on the cells clears the object selection");
    assert_eq!(kept(&h), Value::text("keep me"));
}

#[test]
fn every_handle_resizes_only_its_own_edges() {
    let (mut h, o) = fixture();
    for obj in [o.shape, o.image, o.equation, o.slicer, o.chart] {
        let p = grab_point(&h, &o, obj);
        click(&mut h, p);
        for handle in Handle::ALL {
            let before = rect(&h, obj);
            let (sx, sy) = handle.sides();
            let d = vec2(f32::from(sx) * 20.0, f32::from(sy) * 16.0);
            let grab = handle.pos(before);
            drag(&mut h, grab, grab + d);
            let after = rect(&h, obj);
            let moved = |edge: f32, was: f32, by: f32| (edge - (was + by)).abs() < 1.0;
            assert!(moved(after.left(), before.left(), if sx < 0 { d.x } else { 0.0 }), "{obj:?} {handle:?} left");
            assert!(moved(after.right(), before.right(), if sx > 0 { d.x } else { 0.0 }), "{obj:?} {handle:?} right");
            assert!(moved(after.top(), before.top(), if sy < 0 { d.y } else { 0.0 }), "{obj:?} {handle:?} top {before:?} {after:?}");
            assert!(moved(after.bottom(), before.bottom(), if sy > 0 { d.y } else { 0.0 }), "{obj:?} {handle:?} bottom");
            assert_eq!(h.state().selected_object, Some(obj), "resizing keeps the selection");
        }
    }
    // Shrinking past the opposite edge stops short of it.
    let before = rect(&h, o.shape);
    click(&mut h, before.center());
    drag(&mut h, Handle::Right.pos(before), Handle::Right.pos(before) - vec2(before.width() + 50.0, 0.0));
    let after = rect(&h, o.shape);
    assert!(after.width() > 1.0 && (after.left() - before.left()).abs() < 1.0);
    // Dragging the body moves the object without resizing it.
    let before = rect(&h, o.image);
    click(&mut h, before.center());
    drag(&mut h, before.center(), before.center() + vec2(30.0, 25.0));
    let after = rect(&h, o.image);
    assert!((after.min - before.min - vec2(30.0, 25.0)).length() < 1.0 && (after.size() - before.size()).length() < 0.5);
    assert_eq!(kept(&h), Value::text("keep me"));
}

#[test]
fn handles_show_resize_cursors_and_the_body_a_move_cursor() {
    let (mut h, o) = fixture();
    let r = rect(&h, o.shape);
    click(&mut h, r.center());
    for (handle, icon) in [
        (Handle::TopLeft, egui::CursorIcon::ResizeNwSe),
        (Handle::Top, egui::CursorIcon::ResizeVertical),
        (Handle::BottomLeft, egui::CursorIcon::ResizeNeSw),
    ] {
        h.input_mut().events.push(Event::PointerMoved(handle.pos(r)));
        h.run_steps(2);
        assert_eq!(h.output().platform_output.cursor_icon, icon, "{handle:?}");
    }
    h.input_mut().events.push(Event::PointerMoved(r.center()));
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::Move);
}

#[test]
fn delete_and_backspace_delete_the_selected_object_not_the_cells() {
    let (mut h, o) = fixture();
    let p = rect(&h, o.shape).center();
    click(&mut h, p);
    key(&mut h, Key::Delete);
    assert!(!exists(&h, o.shape));
    assert_eq!(h.state().selected_object, None);
    assert_eq!(kept(&h), Value::text("keep me"));

    let p = rect(&h, o.image).center();
    click(&mut h, p);
    key(&mut h, Key::Backspace);
    assert!(!exists(&h, o.image));
    assert!(h.state().editor.is_none(), "Backspace on an object must not start a cell editor");
    assert_eq!(kept(&h), Value::text("keep me"));

    let p = grab_point(&h, &o, o.slicer);
    click(&mut h, p);
    assert_eq!(h.state().selected_object, Some(o.slicer));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().selected_object, None, "Escape drops the object selection");
    key(&mut h, Key::Delete);
    assert!(exists(&h, o.slicer), "with nothing selected, Delete is for the cells");
    assert_eq!(kept(&h), Value::Empty);

    // Undo brings the object back; a redo that removes the selected object clears the selection.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert!(exists(&h, o.image));
    h.state_mut().selected_object = Some(o.image);
    h.state_mut().run("edit.redo", json!({})).unwrap();
    h.run_steps(2);
    assert!(!exists(&h, o.image));
    assert_eq!(h.state().selected_object, None);
}

#[test]
fn double_clicks_route_by_kind() {
    let (mut h, o) = fixture();
    {
        let sh = h.state().session.doc().unwrap().wb.active().unwrap();
        assert_eq!(open_action(sh, o.text_box), OpenAction::EditText(o.text_box.id));
        assert_eq!(open_action(sh, o.chart), OpenAction::FormatChart(o.chart.id));
        assert_eq!(open_action(sh, o.equation), OpenAction::EditEquation(o.equation.id));
        for obj in [o.shape, o.image, o.slicer] {
            assert_eq!(open_action(sh, obj), OpenAction::Select, "{obj:?}");
        }
    }
    let p = rect(&h, o.text_box).center();
    double_click(&mut h, p);
    assert!(h.state().text_box_editor.as_ref().is_some_and(|e| e.id == o.text_box.id), "text boxes keep in-place editing");
    let outside = rect(&h, o.text_box).right_bottom() + vec2(200.0, 150.0);
    click(&mut h, outside);
    assert!(h.state().text_box_editor.is_none());

    let p = grab_point(&h, &o, o.chart);
    double_click(&mut h, p);
    assert_eq!(h.state().grid.pane.as_deref(), Some("formatChart"));
    assert_eq!(h.state().selected_object, Some(o.chart));

    let p = grab_point(&h, &o, o.equation);
    double_click(&mut h, p);
    assert_eq!(h.state().selected_object, Some(o.equation));
    assert_eq!(h.state().dialog.as_ref().map(|d| (d.name(), d.title.as_str())), Some(("equation", "Edit Equation")), "equations open the editor");
    h.state_mut().dialog = None;

    for obj in [o.shape, o.image, o.slicer] {
        let p = grab_point(&h, &o, obj);
        double_click(&mut h, p);
        assert_eq!(h.state().selected_object, Some(obj));
        assert!(h.state().text_box_editor.is_none() && h.state().editor.is_none(), "{obj:?} opens no text editor");
    }
    assert_eq!(kept(&h), Value::text("keep me"));
}

#[test]
fn on_a_right_to_left_sheet_the_mirrored_handles_resize() {
    let (mut h, o) = fixture();
    h.state_mut().run("view.rightToLeft", json!({"on": true})).unwrap();
    h.run_steps(3);
    let screen = |h: &Harness| {
        let app = h.state();
        let sh = app.session.doc().unwrap().wb.active().unwrap();
        let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
        geo.mirror_rect(chartview::object_rect(&geo, sh, o.image).unwrap())
    };
    let before = screen(&h);
    click(&mut h, before.center());
    assert_eq!(h.state().selected_object, Some(o.image), "the picture is hit where it is drawn");
    // The logical bottom-right handle is drawn at the bottom-left; dragging it leftwards widens the picture.
    let grab = before.left_bottom();
    h.input_mut().events.push(Event::PointerMoved(grab));
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeNeSw);
    drag(&mut h, grab, grab + vec2(-20.0, 16.0));
    let after = screen(&h);
    assert!((after.left() - (before.left() - 20.0)).abs() < 1.0 && (after.right() - before.right()).abs() < 1.0, "{before:?} {after:?}");
    assert!((after.bottom() - (before.bottom() + 16.0)).abs() < 1.0 && (after.top() - before.top()).abs() < 1.0, "{before:?} {after:?}");
}

#[test]
fn a_handle_under_the_headers_does_not_take_the_press() {
    let (mut h, o) = fixture();
    h.state_mut().run("object.move", json!({"kind": "shape", "id": o.shape.id, "at": "A1", "dx": 0, "dy": 0})).unwrap();
    h.run_steps(2);
    let before = rect(&h, o.shape);
    click(&mut h, before.center());
    assert_eq!(h.state().selected_object, Some(o.shape));
    let cells = h.state().grid.cells_rect.unwrap();
    assert_eq!(before.left_top(), cells.left_top(), "the shape's top-left handle sits on the header corner");
    // Within the top-left handle's reach, but over the column header: the press is the header's.
    let grab = Handle::TopLeft.pos(before) + vec2(4.0, -4.0);
    assert!(grab.y < cells.top());
    drag(&mut h, grab, grab + vec2(30.0, 40.0));
    assert_eq!(rect(&h, o.shape), before, "the shape is neither moved nor resized");
    assert_eq!(h.state().selected_object, None, "the press went to the column header");
    // Within the left handle's reach, but over the row header.
    let grab = Handle::Left.pos(before) + vec2(-4.0, 0.0);
    assert!(grab.x < cells.left());
    h.state_mut().selected_object = Some(o.shape);
    drag(&mut h, grab, grab + vec2(40.0, 0.0));
    assert_eq!(rect(&h, o.shape), before, "the shape is neither moved nor resized");
}

#[test]
fn each_drag_of_an_object_is_its_own_undo_step() {
    let (mut h, o) = fixture();
    let start = rect(&h, o.image);
    click(&mut h, start.center());
    drag(&mut h, start.center(), start.center() + vec2(30.0, 20.0));
    let moved = rect(&h, o.image);
    assert!((moved.min - start.min - vec2(30.0, 20.0)).length() < 1.0);
    let grab = Handle::BottomRight.pos(moved);
    drag(&mut h, grab, grab + vec2(24.0, 18.0));
    let resized = rect(&h, o.image);
    assert!((resized.size() - moved.size() - vec2(24.0, 18.0)).length() < 1.0);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(rect(&h, o.image), moved, "one undo takes back the resize only");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(rect(&h, o.image), start, "a second undo takes back the move");
}

#[test]
fn dragging_an_unselected_object_by_its_edge_moves_it() {
    let (mut h, o) = fixture();
    assert_eq!(h.state().selected_object, None);
    let before = rect(&h, o.shape);
    // On the bottom-right corner, where the handle would be if the shape were selected; the
    // pointer shows Move there, and the press selects and moves the shape.
    let grab = Handle::BottomRight.pos(before) - vec2(2.0, 2.0);
    h.input_mut().events.push(Event::PointerMoved(grab));
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::Move);
    drag(&mut h, grab, grab + vec2(30.0, 20.0));
    let after = rect(&h, o.shape);
    assert_eq!(h.state().selected_object, Some(o.shape));
    assert!((after.size() - before.size()).length() < 0.5, "not resized: {before:?} {after:?}");
    assert!((after.min - before.min - vec2(30.0, 20.0)).length() < 1.0, "moved with the pointer: {before:?} {after:?}");
}
