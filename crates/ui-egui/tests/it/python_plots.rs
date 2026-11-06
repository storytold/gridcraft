//! What a cell that gave a picture shows and does: its card, the Python Output menu, Display Plot over
//! Cells, and what a linked picture shows. A fake worker stands in for Python.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use egui::{Event, Modifiers, Rect, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::core::{CellError, CellRef, Value};
use gridcraft_engine::model::{ObjKind, ObjRef};
use gridcraft_engine::python::{Interpreter, InterpreterKind, Version};
use gridcraft_engine::{OTHER_WORKBOOK, PythonEnvironment, PythonState, Session};
use gridcraft_python::fake::FakeTransport;
use gridcraft_python::protocol::Output;
use gridcraft_python::{Picture, PyError};
use gridcraft_ui_egui::published_picture::{self, PICTURE_COPIED_TEXT, color_image_hash, rgba_hash};
use gridcraft_ui_egui::python_plots::{COPY_IMAGE, CREATE_REFERENCE, DISPLAY_OVER_CELLS, IMAGE_COPIED, IMAGE_SAVED, OUTPUT_MENU, SAVE_IMAGE};
use gridcraft_ui_egui::{Services, SheetApp, theme};
use serde_json::json;

use crate::common::Harness;

const PYTHON: &str = "/home/you/.gridcraft/python/bin/python";

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba([10, 120, 200, 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn answer(code: &str) -> Result<Output, PyError> {
    match code {
        "plot" => Ok(Output::picture(Picture::from_bytes(png(80, 60), 2).unwrap())),
        "other" => Ok(Output::picture(Picture::from_bytes(png(60, 60), 1).unwrap())),
        "fail" => Err(PyError { message: "ValueError: boom".into(), ..PyError::default() }),
        _ => Ok(Output::Value(Value::Number(1.0))),
    }
}

fn report() -> PythonEnvironment {
    PythonEnvironment {
        state: PythonState::Ready,
        diagnostic: None,
        sandbox: "a test sandbox".into(),
        uv: None,
        interpreter: Some(Interpreter {
            path: PathBuf::from(PYTHON),
            run_as: PathBuf::from(PYTHON),
            version: Version { major: 3, minor: 12, patch: 7 },
            kind: InterpreterKind::UvEnvironment,
            site_dirs: Vec::new(),
            read_paths: Vec::new(),
        }),
        packages: Vec::new(),
    }
}

fn harness_with(services: Services) -> Harness {
    harness_sized(services, vec2(1300.0, 900.0))
}

fn harness_sized(services: Services, size: egui::Vec2) -> Harness {
    let t = FakeTransport::answering(|code, _| answer(code));
    let mut s = Session::new();
    s.new_workbook();
    let mut app = crate::common::prepare(SheetApp::new(s, services));
    app.ui.python.enabled = true;
    app.python.set_sandbox(Ok("a test sandbox".into()));
    app.python.report = Some(report());
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    let mut h = crate::common::harness_with(crate::common::builder().with_size(size), app, 3);
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.run_steps(2);
    h
}

fn harness() -> Harness {
    harness_with(Services::default())
}

fn settle(h: &mut Harness) {
    for _ in 0..1000 {
        h.step();
        if !h.state().session.python_busy() {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("Python never settled");
}

fn c(a1: &str) -> CellRef {
    CellRef::parse(a1).unwrap()
}

fn plot_at(h: &mut Harness, cell: &str) {
    h.state_mut().run("python.set", json!({"cell": cell, "code": "plot"})).unwrap();
    settle(h);
    h.state_mut().session.execute("selection.set", json!({"cell": cell})).unwrap();
    h.run_steps(2);
}

fn card_on(h: &mut Harness, cell: &str) {
    plot_at(h, cell);
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().grid.py_card.is_some(), "the card is open");
}

fn sheet_images(h: &Harness) -> usize {
    h.state().session.active().unwrap().wb.active().unwrap().images.len()
}

fn click(h: &mut Harness, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(4);
}

fn toast(h: &Harness) -> Option<String> {
    h.state().toast.as_ref().map(|(text, _)| text.clone())
}

fn in_cell(h: &Harness, cell: &str) -> bool {
    h.state().session.active().unwrap().wb.active().unwrap().picture_at(c(cell)).is_some()
}

#[test]
fn the_cell_says_image_and_the_card_shows_the_picture_and_its_size() {
    let mut h = harness();
    card_on(&mut h, "B2");
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(c("B2")), Value::text("Image"));
    assert!(h.query_by_label("Image").is_some());
    assert!(h.query_by_label("size").is_some());
    assert!(h.query_by_label("40, 30").is_some(), "its own size, not the pixels it is drawn with");
    assert!(h.query_by_label("Python Object").is_some());
    assert!(h.query_by_label("Excel Value").is_some());
    assert!(h.query_by_label(COPY_IMAGE).is_some());
    assert!(h.query_by_label(DISPLAY_OVER_CELLS).is_some());
    h.check_accessibility();
}

#[test]
fn the_cards_corner_resizes_it_within_the_window() {
    let mut h = harness();
    card_on(&mut h, "B2");
    let area = |h: &Harness| h.ctx.memory(|m| m.area_rect(egui::Id::unique("python_card"))).expect("the card is shown");
    let before = area(&h);
    let drag = |h: &mut Harness, by: f32| {
        let grip = h.get_by_label("Resize").accesskit_node().bounding_box().expect("a place on screen");
        let at = egui::pos2(((grip.x0 + grip.x1) / 2.0) as f32, ((grip.y0 + grip.y1) / 2.0) as f32);
        let press = |pos, pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        h.input_mut().events.push(Event::PointerMoved(at));
        h.step();
        h.input_mut().events.push(press(at, true));
        h.step();
        let to = at + vec2(by, 0.0);
        h.input_mut().events.push(Event::PointerMoved(to));
        h.step();
        h.input_mut().events.push(press(to, false));
        h.run_steps(3);
    };
    drag(&mut h, 120.0);
    assert!(area(&h).width() > before.width() + 60.0, "{:?} from {:?}", area(&h), before);
    // However far it goes, it stays in the window.
    drag(&mut h, 5000.0);
    let window: Rect = h.ctx.content_rect();
    assert!(area(&h).width() <= window.width(), "{:?}", area(&h));
}

#[test]
fn copy_image_gives_the_clipboard_the_pixels() {
    let copied: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let seen = copied.clone();
    let services = Services {
        copy_image: Some(Box::new(move |image| {
            seen.borrow_mut().push((image.width(), image.height()));
            Ok(())
        })),
        ..Services::default()
    };
    let mut h = harness_with(services);
    card_on(&mut h, "B2");
    click(&mut h, COPY_IMAGE);
    assert_eq!(*copied.borrow(), [(80, 60)], "the picture's own pixels");
    assert_eq!(toast(&h).as_deref(), Some(IMAGE_COPIED));
}

#[test]
fn save_image_asks_where_and_cancelling_does_nothing() {
    let dir = std::env::temp_dir().join(format!("gridcraft-py-plots-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("saved.png");
    let answer_with: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let pick = answer_with.clone();
    let asked: Rc<RefCell<Vec<String>>> = Rc::default();
    let names = asked.clone();
    let services = Services {
        pick_save: Some(Box::new(move |name: &str| {
            names.borrow_mut().push(name.to_string());
            pick.borrow().clone()
        })),
        ..Services::default()
    };
    let mut h = harness_with(services);
    card_on(&mut h, "B2");
    click(&mut h, SAVE_IMAGE);
    assert!(asked.borrow()[0].ends_with("-B2.png"), "{:?}", asked.borrow());
    assert!(!path.exists(), "cancelled");
    assert_eq!(toast(&h), None);
    *answer_with.borrow_mut() = Some(path.to_str().unwrap().to_string());
    click(&mut h, SAVE_IMAGE);
    assert_eq!(std::fs::read(&path).unwrap(), png(80, 60));
    assert_eq!(toast(&h).as_deref(), Some(IMAGE_SAVED));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_output_menu_sets_the_return_type_and_needs_a_cell() {
    let mut h = harness();
    plot_at(&mut h, "B2");
    assert!(!in_cell(&h, "B2"));
    h.get_by_label(OUTPUT_MENU).click();
    h.run_steps(3);
    assert_eq!(h.query_all_by_label(OUTPUT_MENU).count(), 2, "the button, and the menu's title");
    h.get_by_label("Excel Value").click();
    settle(&mut h);
    assert!(in_cell(&h, "B2"), "the picture is in the cell now");
    h.get_by_label(OUTPUT_MENU).click();
    h.run_steps(3);
    h.get_by_label("Python Object").click();
    settle(&mut h);
    assert!(!in_cell(&h, "B2"));
    // Off a Python cell the menu isn't there.
    h.state_mut().session.execute("selection.set", json!({"cell": "F9"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label(OUTPUT_MENU).is_none());
    // On a draft there is no cell to set it on yet.
    h.state_mut().insert_python();
    h.run_steps(3);
    assert!(h.get_by_label(OUTPUT_MENU).accesskit_node().is_disabled());
}

#[test]
fn the_shortcut_and_the_button_display_a_plot_over_the_cells() {
    let mut h = harness();
    plot_at(&mut h, "B2");
    let shortcut = |h: &mut Harness| {
        let modifiers = Modifiers { alt: true, ctrl: true, shift: true, ..Modifiers::NONE };
        h.input_mut().events.push(Event::Key { key: egui::Key::C, physical_key: None, pressed: true, repeat: false, modifiers });
        h.input_mut().events.push(Event::Key { key: egui::Key::C, physical_key: None, pressed: false, repeat: false, modifiers });
        h.run_steps(3);
    };
    shortcut(&mut h);
    assert_eq!(sheet_images(&h), 1);
    // On a cell with no picture, it does nothing.
    h.state_mut().run("python.set", json!({"cell": "D2", "code": "number"})).unwrap();
    settle(&mut h);
    h.state_mut().session.execute("selection.set", json!({"cell": "D2"})).unwrap();
    h.run_steps(2);
    shortcut(&mut h);
    assert_eq!(sheet_images(&h), 1);
    h.state_mut().session.execute("selection.set", json!({"cell": "F9"})).unwrap();
    h.run_steps(2);
    shortcut(&mut h);
    assert_eq!(sheet_images(&h), 1);
    // From the card.
    card_on(&mut h, "B4");
    click(&mut h, DISPLAY_OVER_CELLS);
    assert_eq!(sheet_images(&h), 2);
}

#[test]
fn a_selected_linked_picture_offers_create_reference_and_an_ordinary_one_doesnt() {
    let mut h = harness();
    plot_at(&mut h, "B2");
    h.state_mut().run("python.displayOverCells", json!({"cell": "B2", "at": "D2"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label(CREATE_REFERENCE).is_none(), "not until it is selected");
    let id = h.state().session.active().unwrap().wb.active().unwrap().images[0].id;
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Image, id));
    h.run_steps(3);
    click(&mut h, CREATE_REFERENCE);
    assert_eq!(sheet_images(&h), 2);
    assert_eq!(h.state().selected_object, Some(ObjRef::new(ObjKind::Image, id)), "the click stays with the button");
    // A picture placed by hand has no cell to refer to.
    let bytes = gridcraft_engine::io::base64_encode(&png(4, 4));
    h.state_mut().run("insert.picture", json!({"base64": bytes, "at": "H2"})).unwrap();
    let plain = h.state().session.active().unwrap().wb.active().unwrap().images[2].id;
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Image, plain));
    h.run_steps(3);
    assert!(h.query_by_label(CREATE_REFERENCE).is_none());
}

#[test]
fn a_picture_whose_cell_failed_says_so_and_the_next_picture_clears_it() {
    let mut h = harness();
    plot_at(&mut h, "B2");
    h.state_mut().run("python.displayOverCells", json!({"cell": "B2", "at": "D2"})).unwrap();
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "fail"})).unwrap();
    settle(&mut h);
    assert_eq!(sheet_images(&h), 1, "a failure deletes nothing");
    let said = format!("B2: {} This picture shows the last result that cell gave.", CellError::Python.as_str());
    assert!(h.query_by_label(&said).is_some(), "{said}");
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "other"})).unwrap();
    settle(&mut h);
    assert!(h.query_by_label(&said).is_none());
    assert_eq!(sheet_images(&h), 1);
    // And code that gives no picture takes it away, with Undo to bring both back.
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "number"})).unwrap();
    settle(&mut h);
    assert_eq!(sheet_images(&h), 0);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    settle(&mut h);
    assert_eq!(sheet_images(&h), 1);
}

#[test]
fn the_python_tab_offers_it_with_the_reason_when_it_cant() {
    let mut h = harness();
    plot_at(&mut h, "B2");
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Python"})).unwrap();
    h.run_steps(3);
    h.get_by_label(DISPLAY_OVER_CELLS).click();
    h.run_steps(4);
    assert_eq!(sheet_images(&h), 1);
}

// ---- Copy, cut and paste of a floating picture

/// A system clipboard and a temporary folder: one thing on the clipboard at a time, as on a real host.
/// Harnesses made from clones of one of these are two processes on one machine.
#[derive(Clone, Default)]
struct Host {
    text: Rc<RefCell<Option<String>>>,
    picture: Rc<RefCell<Option<image::RgbaImage>>>,
    publishes: Rc<RefCell<Vec<(u32, u32)>>>,
    note: Rc<RefCell<Option<u64>>>,
}

impl Host {
    fn services(&self) -> Services {
        let (text, picture, copy) = (self.text.clone(), self.picture.clone(), self.clone());
        let (noted, read) = (self.note.clone(), self.note.clone());
        Services {
            paste_text: Some(Box::new(move || text.borrow().clone())),
            paste_image: Some(Box::new(move || picture.borrow().clone())),
            copy_image: Some(Box::new(move |image| {
                copy.publishes.borrow_mut().push((image.width(), image.height()));
                copy.put_picture(image.clone());
                Ok(())
            })),
            note_published_picture: Some(Box::new(move |hash| *noted.borrow_mut() = hash)),
            read_published_picture: Some(Box::new(move || *read.borrow())),
            ..Services::default()
        }
    }

    fn put_text(&self, text: &str) {
        *self.text.borrow_mut() = Some(text.to_string());
        *self.picture.borrow_mut() = None;
    }

    fn put_picture(&self, picture: image::RgbaImage) {
        *self.picture.borrow_mut() = Some(picture);
        *self.text.borrow_mut() = None;
    }

    /// Cmd+V: what the window system gives egui, text first.
    fn press_paste(&self, h: &mut Harness) {
        let event = match (self.text.borrow().clone(), self.picture.borrow().as_ref()) {
            (Some(text), _) => Some(Event::Paste(text)),
            (None, Some(picture)) => Some(Event::PasteImage(Arc::new(color_image(picture)))),
            (None, None) => None,
        };
        h.input_mut().events.extend(event);
        h.run_steps(3);
    }
}

fn color_image(picture: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([picture.width() as usize, picture.height() as usize], picture.as_raw())
}

fn screenshot() -> image::RgbaImage {
    image::RgbaImage::from_pixel(5, 4, image::Rgba([200, 30, 30, 255]))
}

/// B2 gave a picture, shown over the cells at D2, and selected there.
fn picture_at_d2(h: &mut Harness) -> u32 {
    plot_at(h, "B2");
    h.state_mut().run("python.displayOverCells", json!({"cell": "B2", "at": "D2"})).unwrap();
    let id = images(h).last().unwrap().id;
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Image, id));
    h.run_steps(2);
    id
}

fn images(h: &Harness) -> Vec<gridcraft_engine::model::Image> {
    h.state().session.active().unwrap().wb.active().unwrap().images.clone()
}

fn send(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(2);
}

/// Sends a copy or cut, and the text it asks egui to put on the clipboard.
fn send_copy(h: &mut Harness, event: Event) -> Option<String> {
    h.input_mut().events.push(event);
    h.step();
    let text = h.output().platform_output.commands.iter().find_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None });
    h.step();
    text
}

/// Off the picture, on a cell, as after a click on it: Paste has somewhere to go.
fn select_cell(h: &mut Harness, cell: &str) {
    h.state_mut().selected_object = None;
    h.state_mut().session.execute("selection.set", json!({"cell": cell})).unwrap();
    h.run_steps(2);
}

#[test]
fn cmd_c_on_a_selected_picture_publishes_it_and_leaves_the_cells_alone() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    picture_at_d2(&mut h);
    assert_eq!(send_copy(&mut h, Event::Copy), None, "no text replaces the picture");
    assert_eq!(*host.publishes.borrow(), [(80, 60)], "its own pixels");
    assert!(h.state().session.clipboard.is_none(), "no cells were copied");
    assert!(h.state().session.object_clipboard.is_some());
    assert_eq!(*host.note.borrow(), Some(rgba_hash(host.picture.borrow().as_ref().unwrap())));
    assert_eq!(images(&h).len(), 1, "a copy leaves it");
    assert_eq!(toast(&h), None);

    // With no picture selected, cells are copied as before.
    select_cell(&mut h, "F6");
    assert!(send_copy(&mut h, Event::Copy).is_some());
    assert_eq!(host.publishes.borrow().len(), 1);
    assert!(h.state().session.clipboard.is_some());
    assert!(h.state().session.object_clipboard.is_none(), "one clipboard at a time");
    assert_eq!(*host.note.borrow(), None, "the published picture no longer stands");
}

#[test]
fn cmd_x_on_a_selected_picture_takes_it_away_and_undo_brings_it_back() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    picture_at_d2(&mut h);
    send(&mut h, Event::Cut);
    assert_eq!(*host.publishes.borrow(), [(80, 60)]);
    assert!(images(&h).is_empty());
    assert!(h.state().selected_object.is_none());
    assert!(h.state().session.clipboard.is_none());
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(images(&h)[0].python_source.as_deref(), Some("Sheet1!$B$2"));
}

#[test]
fn a_cut_that_a_protected_sheet_refuses_publishes_nothing() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    picture_at_d2(&mut h);
    h.state_mut().run("review.protectSheet", json!({})).unwrap();
    h.run_steps(2);
    send(&mut h, Event::Cut);
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("protected")), "{:?}", h.state().message);
    assert_eq!(images(&h).len(), 1);
    assert!(host.publishes.borrow().is_empty());
    assert!(h.state().session.object_clipboard.is_none());
}

#[test]
fn cmd_c_on_another_kind_of_object_still_copies_the_cells() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    let chart = {
        h.state_mut().run("range.setValues", json!({"range": "H1", "values": [["x"], [1], [2]]})).unwrap();
        h.state_mut().run("insert.chart", json!({"range": "H1:H3"})).unwrap()["chart"].as_u64().unwrap() as u32
    };
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Chart, chart));
    h.run_steps(2);
    send(&mut h, Event::Copy);
    assert!(host.publishes.borrow().is_empty());
    assert!(h.state().session.clipboard.is_some());
    assert!(h.state().session.object_clipboard.is_none());
}

#[test]
fn the_same_picture_pastes_linked_by_key_and_by_ribbon() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    picture_at_d2(&mut h);
    send(&mut h, Event::Copy);
    select_cell(&mut h, "F6");
    host.press_paste(&mut h);
    assert_eq!(images(&h).len(), 2);
    let copy = images(&h)[1].clone();
    assert_eq!(copy.python_source.as_deref(), Some("Sheet1!$B$2"), "linked");
    assert_eq!(copy.anchor.cell, c("F6"), "at the active cell");
    assert_eq!(h.state().selected_object, Some(ObjRef::new(ObjKind::Image, copy.id)), "selected, as a pasted picture is");
    select_cell(&mut h, "F16");
    h.state_mut().paste_from_clipboard();
    h.run_steps(2);
    assert_eq!(images(&h).len(), 3);
    assert_eq!(images(&h)[2].python_source.as_deref(), Some("Sheet1!$B$2"));
    assert_eq!(images(&h)[2].anchor.cell, c("F16"));
    // Both change with the code.
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "other"})).unwrap();
    settle(&mut h);
    assert_eq!(*images(&h)[1].data, png(60, 60));
    assert_eq!(*images(&h)[2].data, png(60, 60));
}

#[test]
fn a_cut_picture_pastes_linked_on_another_sheet() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    picture_at_d2(&mut h);
    send(&mut h, Event::Cut);
    h.state_mut().run("home.insertSheet", json!({"name": "Plots"})).unwrap();
    select_cell(&mut h, "C3");
    host.press_paste(&mut h);
    assert_eq!(images(&h)[0].python_source.as_deref(), Some("Sheet1!$B$2"));
    assert_eq!(images(&h)[0].anchor.cell, c("C3"));
}

#[test]
fn later_text_pastes_as_text_and_the_picture_is_forgotten() {
    for by_key in [true, false] {
        let host = Host::default();
        let mut h = harness_with(host.services());
        picture_at_d2(&mut h);
        send(&mut h, Event::Copy);
        select_cell(&mut h, "F6");
        host.put_text("hello");
        if by_key {
            host.press_paste(&mut h);
        } else {
            h.state_mut().paste_from_clipboard();
            h.run_steps(2);
        }
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(c("F6")), Value::text("hello"), "by key: {by_key}");
        assert_eq!(images(&h).len(), 1);
        assert!(h.state().session.object_clipboard.is_none());
        assert_eq!(*host.note.borrow(), None, "the picture is no longer on the clipboard");
        // The same pixels back on the clipboard came from somewhere else now.
        host.put_picture(image::RgbaImage::from_pixel(80, 60, image::Rgba([10, 120, 200, 255])));
        select_cell(&mut h, "F16");
        host.press_paste(&mut h);
        assert_eq!(images(&h).len(), 2);
        assert_eq!(images(&h)[1].python_source, None);
    }
}

#[test]
fn a_different_picture_pastes_as_a_static_picture() {
    for by_key in [true, false] {
        let host = Host::default();
        let mut h = harness_with(host.services());
        picture_at_d2(&mut h);
        send(&mut h, Event::Copy);
        select_cell(&mut h, "F6");
        host.put_picture(screenshot());
        if by_key {
            host.press_paste(&mut h);
        } else {
            h.state_mut().paste_from_clipboard();
            h.run_steps(2);
        }
        assert_eq!(images(&h).len(), 2, "by key: {by_key}");
        assert_eq!(images(&h)[1].python_source, None, "by key: {by_key}");
        assert_eq!(images(&h)[1].anchor.cell, c("F6"));
        assert!(h.state().session.object_clipboard.is_none(), "the clipboard moved on");
    }
}

#[test]
fn another_workbook_refuses_with_an_alert_and_the_first_can_still_paste() {
    for by_key in [true, false] {
        let host = Host::default();
        let mut h = harness_with(host.services());
        picture_at_d2(&mut h);
        send(&mut h, Event::Copy);
        h.state_mut().run("file.new", json!({})).unwrap();
        h.run_steps(2);
        assert_eq!(h.state().session.documents().len(), 2);
        let steps = h.state().session.active().unwrap().undo.len();
        if by_key {
            host.press_paste(&mut h);
        } else {
            h.state_mut().paste_from_clipboard();
            h.run_steps(2);
        }
        assert_eq!(h.state().message.as_ref().map(|(_, m)| m.as_str()), Some(OTHER_WORKBOOK), "by key: {by_key}");
        assert!(images(&h).is_empty());
        assert_eq!(h.state().session.active().unwrap().undo.len(), steps);
        assert!(h.state().session.object_clipboard.is_some(), "it stays for its own workbook");
        h.state_mut().message = None;
        h.state_mut().session.set_active(0);
        h.run_steps(2);
        select_cell(&mut h, "F6");
        host.press_paste(&mut h);
        assert_eq!(images(&h).len(), 2);
        assert_eq!(images(&h)[1].python_source.as_deref(), Some("Sheet1!$B$2"));
    }
}

#[test]
fn a_picture_from_another_process_is_refused_and_others_are_not() {
    let host = Host::default();
    let mut a = harness_with(host.services());
    picture_at_d2(&mut a);
    send(&mut a, Event::Copy);
    assert!(host.note.borrow().is_some());

    let mut b = harness_with(host.services());
    select_cell(&mut b, "C3");
    host.press_paste(&mut b);
    assert_eq!(b.state().message.as_ref().map(|(_, m)| m.as_str()), Some(OTHER_WORKBOOK));
    assert!(images(&b).is_empty());
    b.state_mut().message = None;
    b.state_mut().paste_from_clipboard();
    b.run_steps(2);
    assert_eq!(b.state().message.as_ref().map(|(_, m)| m.as_str()), Some(OTHER_WORKBOOK), "by ribbon too");
    assert!(images(&b).is_empty());
    b.state_mut().message = None;
    assert_eq!(*host.publishes.borrow(), [(80, 60)]);

    // A screenshot is no one's floating object.
    host.put_picture(screenshot());
    b.run_steps(2);
    host.press_paste(&mut b);
    assert_eq!(images(&b).len(), 1);
    assert_eq!(images(&b)[0].python_source, None);

    // Now the second process copies a picture: the first refuses it, and its own is gone.
    b.state_mut().selected_object = Some(ObjRef::new(ObjKind::Image, images(&b)[0].id));
    b.run_steps(2);
    send(&mut b, Event::Copy);
    select_cell(&mut a, "F6");
    host.press_paste(&mut a);
    assert_eq!(a.state().message.as_ref().map(|(_, m)| m.as_str()), Some(OTHER_WORKBOOK));
    assert_eq!(images(&a).len(), 1);
}

#[test]
fn the_hash_of_a_picture_is_the_same_either_way_it_arrives() {
    // Partly transparent pixels too, which don't survive egui's premultiplied colours exactly.
    let mut picture = image::RgbaImage::new(64, 32);
    for (x, y, p) in picture.enumerate_pixels_mut() {
        *p = image::Rgba([(x * 3) as u8, (y * 7) as u8, (x * y) as u8, ((x + y) * 5) as u8]);
    }
    let transparent = picture.pixels().filter(|p| !matches!(p.0[3], 0 | 255)).count();
    assert!(transparent > 1000);
    let image = color_image(&picture);
    assert_eq!(rgba_hash(&picture), color_image_hash(&image));
    // And it says something about the size and the pixels.
    let mut one_pixel = picture.clone();
    one_pixel.get_pixel_mut(10, 10).0[0] ^= 0x40;
    assert_ne!(rgba_hash(&picture), rgba_hash(&one_pixel));
    let flat = image::RgbaImage::from_pixel(8, 2, image::Rgba([1, 2, 3, 255]));
    let turned = image::RgbaImage::from_pixel(2, 8, image::Rgba([1, 2, 3, 255]));
    assert_ne!(rgba_hash(&flat), rgba_hash(&turned));
    // The same in every process and on every run: a value worked out apart from this code.
    assert_eq!(rgba_hash(&image::RgbaImage::new(0, 0)), 0xa8c7_f832_281a_39c5);
    assert_eq!(rgba_hash(&image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255]))), 0xef74_213a_80c9_4fac);
}

#[test]
fn a_transparent_picture_pastes_linked_even_when_the_host_changes_its_pixels() {
    let picture: Rc<RefCell<Option<image::RgbaImage>>> = Rc::default();
    let (put, get) = (picture.clone(), picture.clone());
    let services = Services {
        // As a host writing premultiplied pixels does: what comes back isn't quite what went in.
        copy_image: Some(Box::new(move |image| {
            let mut changed = image.clone();
            changed.pixels_mut().for_each(|p| p.0[0] = p.0[0].saturating_sub(1));
            *put.borrow_mut() = Some(changed);
            Ok(())
        })),
        paste_image: Some(Box::new(move || get.borrow().clone())),
        ..Services::default()
    };
    let mut h = harness_with(services);
    picture_at_d2(&mut h);
    send(&mut h, Event::Copy);
    select_cell(&mut h, "F6");
    let back = picture.borrow().clone().unwrap();
    h.input_mut().events.push(Event::PasteImage(Arc::new(color_image(&back))));
    h.run_steps(3);
    assert_eq!(images(&h).len(), 2);
    assert_eq!(images(&h)[1].python_source.as_deref(), Some("Sheet1!$B$2"));
}

#[test]
fn with_no_host_clipboard_for_pictures_the_internal_clipboard_is_used() {
    let mut h = harness();
    picture_at_d2(&mut h);
    assert_eq!(send_copy(&mut h, Event::Copy).as_deref(), Some(PICTURE_COPIED_TEXT), "something for Paste to go by");
    select_cell(&mut h, "F6");
    // The ribbon's Paste, which reads no clipboard here.
    h.state_mut().paste_from_clipboard();
    h.run_steps(2);
    assert_eq!(images(&h)[1].python_source.as_deref(), Some("Sheet1!$B$2"));
    // Cmd+V, which finds that text.
    select_cell(&mut h, "F16");
    send(&mut h, Event::Paste(PICTURE_COPIED_TEXT.into()));
    assert_eq!(images(&h)[2].python_source.as_deref(), Some("Sheet1!$B$2"));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(c("F16")), Value::Empty);
    // Anything else on the clipboard is newer.
    select_cell(&mut h, "H6");
    send(&mut h, Event::Paste("newer".into()));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(c("H6")), Value::text("newer"));
    assert_eq!(images(&h).len(), 3);
    assert!(h.state().session.object_clipboard.is_none());
}

#[test]
fn a_host_that_cant_take_the_picture_leaves_it_on_the_internal_clipboard() {
    let host = Host::default();
    let mut services = host.services();
    services.copy_image = Some(Box::new(|_| Err("no clipboard".into())));
    let mut h = harness_with(services);
    picture_at_d2(&mut h);
    let text = send_copy(&mut h, Event::Copy).unwrap();
    assert_eq!(text, PICTURE_COPIED_TEXT);
    assert!(h.state().message.is_none(), "the copy worked");
    assert_eq!(*host.note.borrow(), None, "nothing was published");
    host.put_text(&text);
    select_cell(&mut h, "F6");
    host.press_paste(&mut h);
    assert_eq!(images(&h)[1].python_source.as_deref(), Some("Sheet1!$B$2"));
}

#[test]
fn a_copy_from_an_agent_is_newer_than_what_the_clipboard_holds_until_that_changes() {
    let host = Host::default();
    host.put_text("old");
    let mut h = harness_with(host.services());
    let id = picture_at_d2(&mut h);
    h.state_mut().run("object.copy", json!({"id": id})).unwrap();
    assert!(host.publishes.borrow().is_empty(), "it leaves the system clipboard alone");
    select_cell(&mut h, "F6");
    host.press_paste(&mut h);
    assert_eq!(images(&h)[1].python_source.as_deref(), Some("Sheet1!$B$2"), "by key");
    select_cell(&mut h, "F16");
    h.state_mut().paste_from_clipboard();
    h.run_steps(2);
    assert_eq!(images(&h)[2].python_source.as_deref(), Some("Sheet1!$B$2"), "by ribbon");
    host.put_text("new");
    select_cell(&mut h, "H6");
    host.press_paste(&mut h);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(c("H6")), Value::text("new"));
    assert_eq!(images(&h).len(), 3);

    // The same with a picture on the clipboard, and with nothing at all.
    host.put_picture(screenshot());
    h.state_mut().run("object.copy", json!({"id": id})).unwrap();
    select_cell(&mut h, "J6");
    host.press_paste(&mut h);
    assert_eq!(images(&h)[3].python_source.as_deref(), Some("Sheet1!$B$2"));
    assert_eq!(*host.note.borrow(), None, "no other process has to refuse a screenshot");
    host.put_picture(image::RgbaImage::from_pixel(3, 3, image::Rgba([1, 2, 3, 255])));
    select_cell(&mut h, "J16");
    host.press_paste(&mut h);
    assert_eq!(images(&h)[4].python_source, None, "a later picture is itself");
    *host.picture.borrow_mut() = None;
    h.state_mut().run("object.copy", json!({"id": id})).unwrap();
    select_cell(&mut h, "L6");
    h.state_mut().paste_from_clipboard();
    h.run_steps(2);
    assert_eq!(images(&h)[5].python_source.as_deref(), Some("Sheet1!$B$2"));
}

#[test]
fn copy_image_on_the_card_ends_the_copy_and_pastes_as_a_plain_picture() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    picture_at_d2(&mut h);
    send(&mut h, Event::Copy);
    assert!(host.note.borrow().is_some());
    h.state_mut().selected_object = None;
    h.state_mut().session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    h.run_steps(2);
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    click(&mut h, COPY_IMAGE);
    assert_eq!(toast(&h).as_deref(), Some(IMAGE_COPIED));
    assert!(h.state().session.object_clipboard.is_none());
    h.state_mut().grid.py_card = None;
    select_cell(&mut h, "F6");
    h.state_mut().paste_from_clipboard();
    h.run_steps(2);
    assert_eq!(images(&h).len(), 2);
    assert_eq!(images(&h)[1].python_source, None);
    // It stays out of another workbook, by key and by ribbon, as a floating picture does.
    assert!(host.note.borrow().is_some(), "and out of another process's");
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(2);
    h.state_mut().paste_from_clipboard();
    h.run_steps(2);
    assert_eq!(h.state().message.as_ref().map(|(_, m)| m.as_str()), Some(OTHER_WORKBOOK));
    assert!(images(&h).is_empty());
    h.state_mut().message = None;
    host.press_paste(&mut h);
    assert_eq!(h.state().message.as_ref().map(|(_, m)| m.as_str()), Some(OTHER_WORKBOOK));
    assert!(images(&h).is_empty());
}

#[test]
fn the_pictures_menu_cuts_copies_and_pastes_it() {
    let host = Host::default();
    let mut h = harness_with(host.services());
    let id = picture_at_d2(&mut h);
    let open = |h: &mut Harness| {
        h.state_mut().grid.object_menu = Some((egui::pos2(500.0, 400.0), "image", id));
        h.run_steps(2);
    };
    let choose = |h: &mut Harness, label: &str| {
        let items: Vec<_> = h.query_all_by_label(label).collect();
        let item = items.last().expect("on the menu");
        item.click();
        h.run_steps(3);
    };
    open(&mut h);
    choose(&mut h, "Copy");
    assert_eq!(host.publishes.borrow().len(), 1);
    assert_eq!(images(&h).len(), 1);
    assert!(h.state().grid.object_menu.is_none());
    h.state_mut().session.execute("selection.set", json!({"cell": "F6"})).unwrap();
    open(&mut h);
    choose(&mut h, "Paste");
    assert_eq!(images(&h).len(), 2);
    assert_eq!(images(&h)[1].anchor.cell, c("F6"));
    open(&mut h);
    choose(&mut h, "Cut");
    assert_eq!(images(&h).len(), 1);
    assert_ne!(images(&h)[0].id, id, "the first one is the one that went");
    assert_eq!(host.publishes.borrow().len(), 2);
}

#[test]
fn the_note_file_round_trips_and_ignores_anything_else() {
    let dir = std::env::temp_dir().join(format!("gridcraft-published-note-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(published_picture::NOTE_FILE);
    assert_eq!(published_picture::read(&path), None, "no note");
    published_picture::write(&path, Some(0x0123_4567_89ab_cdef)).unwrap();
    assert_eq!(published_picture::read(&path), Some(0x0123_4567_89ab_cdef));
    published_picture::write(&path, Some(7)).unwrap();
    assert_eq!(published_picture::read(&path), Some(7), "the next one replaces it");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing left beside it");
    published_picture::write(&path, None).unwrap();
    assert_eq!(published_picture::read(&path), None);
    published_picture::write(&path, None).unwrap();
    for garbage in ["", "not a number", "12345678901234567890123", "-1"] {
        std::fs::write(&path, garbage).unwrap();
        assert_eq!(published_picture::read(&path), None, "{garbage}");
    }
    std::fs::write(&path, "1".repeat(1 << 20)).unwrap();
    assert_eq!(published_picture::read(&path), None, "too large to be one");
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(published_picture::read(&path), None, "a folder");
    assert!(published_picture::write(&path, Some(1)).is_err(), "it says so, and the caller carries on");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "nothing left beside it");
    std::fs::remove_dir(&path).unwrap();
    #[cfg(unix)]
    {
        let target = dir.join("someone else's file");
        std::fs::write(&target, "kept").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        published_picture::write(&path, Some(9)).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "kept", "a link left there is replaced, not followed");
        assert_eq!(published_picture::read(&path), Some(9));
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A cell's area on the screen, and what opens its card.
fn on_screen(h: &Harness, cell: &str) -> (Rect, Rect) {
    let app = h.state();
    let d = app.session.active().unwrap();
    let sh = d.wb.active().unwrap();
    let geo = app.grid.geo.as_ref().unwrap();
    let at = c(cell);
    (geo.screen_cell_rect(sh, at), geo.mirror_rect(gridcraft_ui_egui::python_card::tag_at(&app.session, geo, &d.wb, sh, at).unwrap()))
}

/// The texts painted starting in a cell, with their fonts.
fn texts_in(h: &Harness, cell: &str) -> Vec<(String, egui::FontId)> {
    let app = h.state();
    let area = app.grid.geo.as_ref().unwrap().screen_cell_rect(app.session.active().unwrap().wb.active().unwrap(), c(cell));
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if area.contains(Rect::from_min_size(t.pos, t.galley.size()).left_center()) => {
                Some((t.galley.job.text.clone(), t.galley.job.sections.first()?.format.font_id.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The first text painted starting in a cell: its font and its width.
fn word_in(h: &Harness, cell: &str) -> (egui::FontId, f32) {
    let app = h.state();
    let area = app.grid.geo.as_ref().unwrap().screen_cell_rect(app.session.active().unwrap().wb.active().unwrap(), c(cell));
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if area.contains(Rect::from_min_size(t.pos, t.galley.size()).left_center()) => {
                Some((t.galley.job.sections.first()?.format.font_id.clone(), t.galley.size().x))
            }
            _ => None,
        })
        .unwrap()
}

/// The test fonts with the cell's typeface `scale` times the ribbon's, as two different typefaces
/// are on a real machine. The fonts of the tests otherwise have one face for both.
fn cell_typeface_scaled(h: &mut Harness, scale: f32) {
    let mut fonts = theme::pinned_font_definitions();
    let family = |name: &str| egui::FontFamily::Name(name.into());
    let first = fonts.families[&family(theme::CELL)][0].clone();
    let mut face = (*fonts.font_data[&first]).clone();
    face.tweak.scale = scale;
    fonts.font_data.insert("scaled cell face".to_string(), Arc::new(face));
    for name in [theme::CELL, theme::CELL_BOLD] {
        fonts.families.get_mut(&family(name)).unwrap().insert(0, "scaled cell face".to_string());
    }
    h.ctx.set_fonts(fonts);
    h.run_steps(2);
}

/// Whether lines are painted inside `rect`.
fn lines_in(h: &Harness, rect: Rect) -> bool {
    h.output()
        .shapes
        .iter()
        .any(|s| !matches!(s.shape, egui::Shape::Text(_) | egui::Shape::Noop) && rect.contains_rect(s.shape.visual_bounding_rect()))
}

fn point_at(h: &mut Harness, at: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
}

fn click_at(h: &mut Harness, at: egui::Pos2) {
    for event in [
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ] {
        h.input_mut().events.push(event);
        h.run_steps(1);
    }
    h.run_steps(3);
}

#[test]
fn a_cell_that_says_image_has_a_double_square_that_fills_it_and_the_ribbons_typeface_at_the_cells_size() {
    let mut h = harness();
    h.state_mut().session.execute("cell.set", json!({"cell": "C2", "input": "plain"})).unwrap();
    plot_at(&mut h, "B2");
    let plain = texts_in(&h, "C2")[0].1.size;
    assert_eq!(texts_in(&h, "B2"), [("Image".to_string(), theme::ui_font(plain))], "the size of any other cell's text, and no PY tag");
    assert_ne!(texts_in(&h, "C2")[0].1.family, theme::ui_font(plain).family, "another cell's text has the cell's typeface");

    let (cell, mark) = on_screen(&h, "B2");
    assert!(lines_in(&h, mark.expand(1.0)), "the double square is painted where a click opens the card");
    assert!(mark.height() > 0.9 * cell.height() && cell.contains_rect(mark), "it fills the cell: {mark:?} in {cell:?}");

    // Bold in the cell makes the word bold, and a larger size a larger word and a larger square.
    h.state_mut().run("home.bold", json!({"range": "B2"})).unwrap();
    h.run_steps(2);
    assert_eq!(texts_in(&h, "B2"), [("Image".to_string(), theme::ui_bold(plain))]);
    h.state_mut().run("home.fontSize", json!({"range": "B2", "size": 22})).unwrap();
    h.run_steps(2);
    assert_eq!(texts_in(&h, "B2"), [("Image".to_string(), theme::ui_bold(2.0 * plain))]);
    let (cell, larger) = on_screen(&h, "B2");
    assert!(larger.height() > mark.height() && cell.contains_rect(larger), "{larger:?} in {cell:?}");
}

#[test]
fn the_word_is_as_wide_as_the_same_word_in_another_cell_when_the_ribbons_typeface_is_narrower() {
    let mut h = harness();
    h.state_mut().session.execute("cell.set", json!({"cell": "C2", "input": "Image"})).unwrap();
    cell_typeface_scaled(&mut h, 1.07);
    plot_at(&mut h, "B2");
    let (word, word_w) = word_in(&h, "B2");
    let (plain, plain_w) = word_in(&h, "C2");
    assert_eq!(word.family, theme::ui_font(1.0).family, "the ribbon's typeface");
    assert_eq!(plain.family, theme::cell_family("Calibri", false, false), "the cell's typeface");
    assert!(word.size > plain.size, "{word:?} against {plain:?}");
    assert!((word_w - plain_w).abs() < 1.0, "the word is {word_w} wide and the other cell's is {plain_w}");

    // So it is in bold, at another size and at another zoom.
    h.state_mut().run("home.bold", json!({"range": "B2:C2"})).unwrap();
    h.state_mut().run("home.fontSize", json!({"range": "B2:C2", "size": 22})).unwrap();
    h.run_steps(2);
    let (word, word_w) = word_in(&h, "B2");
    let (_, plain_w) = word_in(&h, "C2");
    assert_eq!(word.family, theme::ui_bold(1.0).family);
    assert!((word_w - plain_w).abs() < 1.5, "the word is {word_w} wide and the other cell's is {plain_w}");
}

#[test]
fn a_very_wide_cell_typeface_does_not_make_the_word_much_larger() {
    let mut h = harness();
    h.state_mut().session.execute("cell.set", json!({"cell": "C2", "input": "Image"})).unwrap();
    cell_typeface_scaled(&mut h, 1.6);
    plot_at(&mut h, "B2");
    let (word, _) = word_in(&h, "B2");
    let (plain, _) = word_in(&h, "C2");
    assert!((word.size / plain.size - 1.1).abs() < 0.001, "{word:?} against {plain:?}");
}

#[test]
fn only_a_ready_picture_kept_as_an_object_has_the_double_square() {
    let mut h = harness();
    plot_at(&mut h, "B2");
    let ribbon = theme::ui_font(1.0).family;

    // Excel Value: the picture is in the cell, in place of the word.
    h.state_mut().run("python.output", json!({"cell": "B2", "mode": "value"})).unwrap();
    settle(&mut h);
    assert!(in_cell(&h, "B2"));
    assert_eq!(texts_in(&h, "B2"), []);

    // An ordinary result keeps the tag and the cell's font.
    h.state_mut().run("python.set", json!({"cell": "C2", "code": "x"})).unwrap();
    settle(&mut h);
    let texts = texts_in(&h, "C2");
    assert!(texts.iter().any(|(text, _)| text == "PY"), "{texts:?}");
    assert!(texts.iter().all(|(text, font)| text == "PY" || font.family != ribbon), "{texts:?}");

    // An error says so on the tag.
    h.state_mut().run("python.set", json!({"cell": "D2", "code": "plot"})).unwrap();
    settle(&mut h);
    assert_eq!(texts_in(&h, "D2")[0].0, "Image");
    h.state_mut().run("python.set", json!({"cell": "D2", "code": "fail"})).unwrap();
    settle(&mut h);
    let texts = texts_in(&h, "D2");
    assert!(texts.iter().any(|(text, _)| text == "PY"), "{texts:?}");
    assert!(texts.iter().all(|(_, font)| font.family != ribbon), "{texts:?}");
}

#[test]
fn a_click_on_the_double_square_shows_the_picture_beside_the_cell() {
    let mut h = harness_sized(Services::default(), vec2(760.0, 620.0));
    h.state_mut().session.execute("selection.set", json!({"cell": "A32"})).unwrap();
    h.state_mut().run("python.set", json!({"cell": "A32", "code": "plot"})).unwrap();
    settle(&mut h);
    // Away from the cell, so a click on the mark is what selects it.
    h.state_mut().grid.ensure_visible = true;
    h.run_steps(6);
    h.state_mut().session.execute("selection.set", json!({"cell": "C30"})).unwrap();
    h.run_steps(2);
    let (cell, tag) = on_screen(&h, "A32");
    let view = h.state().grid.geo.as_ref().unwrap().cells;
    assert!(view.contains_rect(cell), "the cell is low in the window, in view: {cell:?} in {view:?}");

    // It says it can be clicked, and only there.
    point_at(&mut h, tag.center());
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::PointingHand);
    point_at(&mut h, cell.right_center() - vec2(4.0, 0.0));
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::Cell);

    // Anywhere on the square, up to its top corner, which is above where a PY tag would end.
    let corner = tag.left_top() + vec2(1.0, 1.0);
    point_at(&mut h, corner);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::PointingHand);
    click_at(&mut h, corner);
    assert_eq!(h.state().grid.py_card.map(|card| card.cell), Some(c("A32")));
    assert_eq!(h.state().session.active().map(|d| d.selection.active), Some(c("A32")));
    assert!(h.query_by_label("40, 30").is_some(), "the card is the picture's");
    let card = h.ctx.memory(|m| m.area_rect(egui::Id::unique("python_card"))).expect("the card is shown");
    assert!((card.left() - cell.right()).abs() < 3.0, "beside the cell: {card:?}, {cell:?}");
    assert!(!card.intersects(cell), "and the cell stays in view: {card:?}, {cell:?}");
    let window = h.ctx.content_rect();
    assert!(window.contains_rect(card), "in the window, however low the cell is: {card:?} in {window:?}");
    let picture =
        h.output().shapes.iter().any(|s| s.shape.texture_id() != egui::TextureId::default() && card.contains_rect(s.shape.visual_bounding_rect()));
    assert!(picture, "the picture itself is painted on the card");
}
