//! What a Python Object cell's card shows and does: its sized name, its table, the actions on it, and
//! what a text does with its lines. A fake worker stands in for Python.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use egui::{OutputCommand, Rect, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::core::{CellError, CellRef, Value};
use gridcraft_engine::python::{Interpreter, InterpreterKind, Version};
use gridcraft_engine::{PythonEnvironment, PythonState, Session};
use gridcraft_python::fake::FakeTransport;
use gridcraft_python::protocol::{MAX_TABLE_COLS, MAX_TABLE_ROWS, Output, Table};
use gridcraft_ui_egui::python_card::{
    self, COPIED_CUT, COPY_CSV, COPY_MARKDOWN, NO_COLUMNS, NO_ROWS, RUN_TO_SEE, SAVE_CSV, SAVED, SAVED_CUT, SPILL_HERE, SPILL_LINES,
};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::{Value as Json, json};

use crate::common::Harness;

const PYTHON: &str = "/home/you/.gridcraft/python/bin/python";
const LONG: &str = "A note that goes on for much longer than any column in a card could ever show. ";

fn frame(rows: usize, cols: usize, total_rows: u64, total_cols: u64) -> Table {
    Table {
        columns: (0..cols).map(|c| format!("col{c}")).collect(),
        rows,
        cols,
        data: (0..rows * cols).map(|i| Value::Number(i as f64)).collect(),
        total_rows,
        total_cols,
        ..Table::default()
    }
}

fn object(type_name: &str, size: Vec<u64>, table: Table) -> Output {
    Output::Object { type_name: type_name.into(), preview: "the repr".into(), size, table: Some(table), picture: None }
}

/// What the worker answers each code with.
fn answer(code: &str) -> Output {
    match code {
        "small" => object("DataFrame", vec![2, 2], frame(2, 2, 2, 2)),
        "one row" => {
            let table = Table { data: vec![Value::text("only"), Value::Number(1.5)], ..frame(1, 2, 1, 2) };
            object("DataFrame", vec![1, 2], table)
        }
        "no rows" => object("DataFrame", vec![0, 3], frame(0, 3, 0, 3)),
        "no columns" => object("DataFrame", vec![0, 0], Table::default()),
        "cut" => object("DataFrame", vec![1000, 50], frame(MAX_TABLE_ROWS, MAX_TABLE_COLS, 1000, 50)),
        "wide" => {
            let long = LONG.repeat(40);
            let table = Table {
                columns: vec![long.clone(), "b".into()],
                data: vec![Value::text(long.as_str()), Value::Number(1.0), Value::text("two\nlines"), Value::Error(CellError::NA)],
                text_cut: true,
                ..frame(2, 2, 2, 2)
            };
            object("DataFrame", vec![2, 2], table)
        }
        "unnamed" => {
            let table = Table { columns: Vec::new(), ..frame(1, 2, 1, 2) };
            object("ndarray", vec![1, 2], table)
        }
        "labels" => {
            let table = Table {
                columns: vec!["key".into(), "x".into()],
                data: vec![Value::text("p"), Value::Number(1.0)],
                row_labels: true,
                ..frame(1, 2, 1, 1)
            };
            object("DataFrame", vec![1, 1], table)
        }
        "guarded" => {
            let table = Table { columns: vec!["=a".into()], data: vec![Value::text("=1+1")], ..frame(1, 1, 1, 1) };
            object("DataFrame", vec![1, 1], table)
        }
        "flat" => Output::Object { type_name: "list".into(), preview: "[1, 2]".into(), size: vec![2], table: None, picture: None },
        "dict" => Output::Object { type_name: "dict".into(), preview: "{'a': 1}".into(), size: vec![1], table: None, picture: None },
        "text" => Output::Value(Value::text("one\ntwo\nthree")),
        _ => Output::Value(Value::Number(1.0)),
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

fn app_with(services: Services) -> SheetApp {
    let t = FakeTransport::answering(|code, _| Ok(answer(code)));
    let mut s = Session::new();
    s.new_workbook();
    let mut app = crate::common::prepare(SheetApp::new(s, services));
    app.ui.python.enabled = true;
    app.python.set_sandbox(Ok("a test sandbox".into()));
    app.python.report = Some(report());
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    app
}

fn harness_sized(app: SheetApp, size: egui::Vec2) -> Harness {
    let mut h = crate::common::harness_with(crate::common::builder().with_size(size), app, 3);
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.run_steps(2);
    h
}

fn harness_with_services(services: Services) -> Harness {
    harness_sized(app_with(services), vec2(1300.0, 800.0))
}

fn harness() -> Harness {
    harness_with_services(Services::default())
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

/// A Python Object cell with this code, with its card open.
fn card_on(h: &mut Harness, cell: &str, code: &str) {
    h.state_mut().run("python.set", json!({"cell": cell, "code": code, "output": "object"})).unwrap();
    settle(h);
    h.state_mut().session.execute("selection.set", json!({"cell": cell})).unwrap();
    h.run_steps(2);
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().grid.py_card.is_some(), "the card is open");
}

fn value(h: &Harness, cell: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(c(cell))
}

fn copied(h: &Harness) -> Option<String> {
    h.output().platform_output.commands.iter().find_map(|o| match o {
        OutputCommand::CopyText(text) => Some(text.clone()),
        _ => None,
    })
}

fn click(h: &mut Harness, label: &str) -> Option<String> {
    h.get_by_label(label).click();
    // The click lands on one frame, and what it copies is in that frame's output.
    let mut text = None;
    for _ in 0..4 {
        h.step();
        text = text.or_else(|| copied(h));
    }
    text
}

fn toast(h: &Harness) -> Option<String> {
    h.state().toast.as_ref().map(|(text, _)| text.clone())
}

fn card_rect(h: &Harness) -> Rect {
    h.ctx.memory(|m| m.area_rect(egui::Id::unique("python_card"))).expect("the card is shown")
}

/// A node's place on screen, by its label.
fn rect_of(h: &Harness, label: &str) -> Rect {
    let b = h.get_by_label(label).accesskit_node().bounding_box().expect("a place on screen");
    Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
}

#[test]
fn the_cell_and_the_card_name_the_object_and_its_size() {
    let mut h = harness();
    card_on(&mut h, "B2", "small");
    assert_eq!(value(&h, "B2"), Value::text("DataFrame 2x2"));
    // The cell's text in the formula bar is the code, so the label here is the card's title.
    assert!(h.query_by_label("DataFrame 2x2").is_some(), "the card's title");
    assert!(h.query_by_label("Python Object").is_some());
    h.check_accessibility();
    // An object that is no table keeps its repr, and has no table's actions.
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    card_on(&mut h, "D2", "dict");
    assert!(h.query_by_label("dict 1").is_some());
    assert!(h.query_by_label("{'a': 1}").is_some(), "its repr");
    assert!(h.query_by_label(COPY_CSV).is_none() && h.query_by_label(SPILL_HERE).is_none(), "a dict doesn't spill");
    // A flat list spills, and has no table.
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    card_on(&mut h, "F2", "flat");
    assert!(h.query_by_label("list 2").is_some());
    assert!(h.query_by_label(SPILL_HERE).is_some() && h.query_by_label(COPY_CSV).is_none());
    assert!(h.query_by_label(RUN_TO_SEE).is_none(), "it ran: it has no table to see");
}

#[test]
fn a_table_shows_its_columns_and_cells_not_its_repr() {
    let mut h = harness();
    card_on(&mut h, "B2", "small");
    for label in ["col0", "col1", "0", "1", "2", "3", COPY_CSV, COPY_MARKDOWN, SAVE_CSV, SPILL_HERE] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert!(h.query_by_label("the repr").is_none());
    assert!(h.query_by_label(NO_ROWS).is_none());
    // Numbers sit at the end of their cell, under their column.
    let (name, number) = (rect_of(&h, "col1"), rect_of(&h, "1"));
    assert!(number.right() > name.right() - 40.0 && number.top() > name.bottom() - 1.0, "{name:?} {number:?}");
    assert!(!h.state().session.active().unwrap().is_dirty() || h.state().message.is_none());
    h.check_accessibility();
}

#[test]
fn a_table_of_one_row_and_one_with_none() {
    let mut h = harness();
    card_on(&mut h, "B2", "one row");
    assert!(h.query_by_label("only").is_some() && h.query_by_label("1.5").is_some());
    assert!(h.query_by_label(NO_ROWS).is_none());
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();

    // Columns and no rows: the names, and it says so. Its CSV is the header.
    card_on(&mut h, "D2", "no rows");
    assert!(h.query_by_label("col2").is_some() && h.query_by_label(NO_ROWS).is_some());
    assert_eq!(click(&mut h, COPY_CSV).as_deref(), Some("col0,col1,col2\r\n"));
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();

    // Nothing at all: it says so, offers nothing to copy, and can still spill.
    card_on(&mut h, "F2", "no columns");
    assert_eq!(value(&h, "F2"), Value::text("DataFrame 0x0"));
    assert!(h.query_by_label(NO_COLUMNS).is_some());
    assert!(h.query_by_label(COPY_CSV).is_none() && h.query_by_label(SAVE_CSV).is_none());
    assert!(h.query_by_label(SPILL_HERE).is_some());
    h.check_accessibility();
}

#[test]
fn a_table_cut_at_200_rows_says_so_and_only_what_is_in_view_is_laid_out() {
    let mut h = harness();
    card_on(&mut h, "B2", "cut");
    assert_eq!(value(&h, "B2"), Value::text("DataFrame 1000x50"));
    let note = h
        .query_by_label_contains("Only the first 200 of 1000 rows are shown.")
        .expect("the card says how many rows it shows")
        .accesskit_node()
        .value()
        .unwrap_or_default();
    assert!(note.contains("Only the first 30 of 50 columns are shown."), "{note}");
    assert!(note.contains("Copying and saving take only what is shown."), "{note}");
    // Virtualised: a screenful of the 6,000 cells has nodes.
    let labels = h.query_all_by_role(egui::accesskit::Role::Label).count();
    assert!(labels < 400, "{labels} labels");
    // Copying gives the 200 rows, and says it is cut.
    let text = click(&mut h, COPY_CSV).expect("copied");
    assert_eq!(text.lines().count(), MAX_TABLE_ROWS + 1);
    assert_eq!(toast(&h).as_deref(), Some(COPIED_CUT));
}

#[test]
fn scrolling_reaches_the_last_row_with_the_header_in_view() {
    let mut h = harness();
    card_on(&mut h, "B2", "cut");
    let header = rect_of(&h, "col0");
    assert!(h.query_by_label("5970").is_none(), "the last row isn't laid out yet");
    // Scroll to the end over the grid.
    let at = header.center() + vec2(0.0, 60.0);
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    for _ in 0..40 {
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            source: egui::MouseWheelSource::Unknown,
            delta: vec2(0.0, -2000.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    }
    h.run_steps(4);
    assert!(h.query_by_label("5970").is_some(), "the last row's first cell");
    assert!(h.query_by_label("0").is_none(), "the first row has gone");
    let still = rect_of(&h, "col0");
    assert!((still.top() - header.top()).abs() < 1.0, "the header stays: {still:?} {header:?}");
}

#[test]
fn very_wide_cells_stay_in_their_columns() {
    let mut h = harness();
    card_on(&mut h, "B2", "wide");
    let card = card_rect(&h);
    assert!(card.width() <= 600.0, "{card:?}");
    // The text is cut for the column; its node says what shows.
    let starts = h.query_all_by_role(egui::accesskit::Role::Label).filter_map(|n| n.accesskit_node().value()).filter(|l| l.starts_with(LONG)).count();
    assert_eq!(starts, 2, "the name and the cell");
    for node in h.query_all_by_role(egui::accesskit::Role::Label) {
        if node.accesskit_node().value().is_some_and(|l| l.starts_with(LONG))
            && let Some(b) = node.accesskit_node().bounding_box()
        {
            assert!((b.x1 - b.x0) <= 225.0, "a column's width at most: {b:?}");
        }
    }
    // A line break shows as a space, on one line.
    assert!(h.query_by_label("two lines").is_some());
    assert!(h.query_by_label("#N/A").is_some());
    let note = h.query_by_label_contains("Some long text is cut.").expect("it says the text was cut");
    assert!(!note.accesskit_node().value().unwrap_or_default().contains("rows are shown"), "all its rows are here");
}

#[test]
fn a_table_without_column_names_has_no_header_and_row_labels_arent_columns() {
    let mut h = harness();
    card_on(&mut h, "B2", "unnamed");
    assert!(h.query_by_label("0").is_some() && h.query_by_label("1").is_some());
    assert_eq!(click(&mut h, COPY_CSV).as_deref(), Some("0,1\r\n"));
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    card_on(&mut h, "D2", "labels");
    assert!(h.query_by_label("key").is_some() && h.query_by_label("p").is_some());
    assert!(h.query_by_label_contains("columns are shown").is_none(), "one column is all it has");
}

#[test]
fn on_a_right_to_left_sheet_the_first_column_is_on_the_right() {
    let mut h = harness();
    h.state_mut().session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    card_on(&mut h, "C4", "small");
    let geo = h.state().grid.geo.as_ref().unwrap();
    let cell = geo.screen_cell_rect(h.state().session.active().unwrap().wb.active().unwrap(), c("C4"));
    let card = card_rect(&h);
    assert!((card.right() - cell.right()).abs() < 1.5, "the card ends at the cell's right edge: {card:?} {cell:?}");
    let (first, second) = (rect_of(&h, "col0"), rect_of(&h, "col1"));
    assert!(first.left() > second.right(), "{first:?} {second:?}");
    assert!(card.contains_rect(first.shrink(1.0)), "{first:?} in {card:?}");
    // Numbers are mirrored too: at the left of their cell, under their column.
    let number = rect_of(&h, "1");
    assert!(number.left() < second.left() + 40.0, "{number:?} {second:?}");
    // A wide one starts at the right, where its first column is.
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    card_on(&mut h, "G4", "cut");
    let card = card_rect(&h);
    let first = rect_of(&h, "col0");
    assert!(card.contains_rect(first.shrink(1.0)), "{first:?} in {card:?}");
    h.check_accessibility();
}

#[test]
fn right_to_left_text_shows_in_its_cell() {
    // Without a face for it there is no Arabic to draw, and a missing glyph fails a test.
    if !gridcraft_ui_egui::theme::craft_fonts_cover("مرحبا") {
        assert!(!gridcraft_ui_egui::theme::craft_fonts_required(), "the pinned fonts have no Arabic");
        eprintln!("no embedded face has Arabic; skipping the right-to-left text");
        return;
    }
    let mut h = harness_sized(app_with_rtl_text(), vec2(1300.0, 800.0));
    h.state_mut().session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    card_on(&mut h, "C4", "arabic");
    assert!(h.query_by_label("مرحبا").is_some());
    assert_eq!(click(&mut h, COPY_MARKDOWN).as_deref(), Some("|  |\n| --- |\n| مرحبا |\n"));
}

fn app_with_rtl_text() -> SheetApp {
    let t = FakeTransport::answering(|code, _| {
        Ok(if code == "arabic" {
            object("list", vec![1], Table { columns: Vec::new(), data: vec![Value::text("مرحبا")], ..frame(1, 1, 1, 1) })
        } else {
            answer(code)
        })
    });
    let mut app = app_with(Services::default());
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    app
}

#[test]
fn a_narrow_window_keeps_the_card_inside_it() {
    let mut h = harness_sized(app_with(Services::default()), vec2(420.0, 420.0));
    card_on(&mut h, "A1", "cut");
    let card = card_rect(&h);
    assert!(card.width() <= 420.0 && card.left() >= 0.0 && card.right() <= 420.0, "{card:?}");
    let saved = rect_of(&h, COPY_CSV);
    assert!(saved.right() <= 420.0, "{saved:?}");
}

#[test]
fn copy_as_csv_and_markdown_escape_and_guard() {
    let mut h = harness();
    card_on(&mut h, "B2", "guarded");
    assert_eq!(click(&mut h, COPY_CSV).as_deref(), Some("\"'=a\"\r\n\"'=1+1\"\r\n"));
    assert_eq!(toast(&h).as_deref(), Some("Copied to the clipboard."));
    assert_eq!(click(&mut h, COPY_MARKDOWN).as_deref(), Some("| =a |\n| --- |\n| =1+1 |\n"));
    assert!(h.state().grid.py_card.is_some(), "the card stays");
}

/// A picker that answers with this path, and remembers what it was offered.
fn services_picking(path: Option<PathBuf>, offered: Rc<RefCell<Vec<String>>>) -> Services {
    Services {
        pick_save: Some(Box::new(move |suggested: &str| {
            offered.borrow_mut().push(suggested.to_string());
            path.as_ref().map(|p| p.to_string_lossy().into_owned())
        })),
        ..Services::default()
    }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-card-save-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn save_as_csv_asks_for_a_place_and_writes_the_table() {
    let dir = temp("write");
    let offered = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness_with_services(services_picking(Some(dir.join("out")), offered.clone()));
    card_on(&mut h, "B2", "small");
    click(&mut h, SAVE_CSV);
    assert_eq!(offered.borrow().as_slice(), ["Book1-B2.csv"], "the picker is offered a name");
    assert_eq!(std::fs::read_to_string(dir.join("out.csv")).unwrap(), "col0,col1\r\n0,1\r\n2,3\r\n");
    assert_eq!(toast(&h).as_deref(), Some(SAVED));
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    // A table bigger than the preview: the person is told.
    card_on(&mut h, "D2", "cut");
    click(&mut h, SAVE_CSV);
    assert_eq!(toast(&h).as_deref(), Some(SAVED_CUT));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cancelling_the_picker_saves_nothing_and_a_failed_save_says_why() {
    let dir = temp("cancel");
    let offered = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness_with_services(services_picking(None, offered.clone()));
    card_on(&mut h, "B2", "small");
    click(&mut h, SAVE_CSV);
    assert_eq!(offered.borrow().len(), 1);
    assert!(toast(&h).is_none() && h.state().message.is_none());
    drop(h);
    let gone = dir.join("no-such-folder").join("x.csv");
    let mut h = harness_with_services(services_picking(Some(gone), offered));
    card_on(&mut h, "B2", "small");
    click(&mut h, SAVE_CSV);
    assert!(h.state().message.is_some(), "the person hears the save failed");
    assert!(toast(&h).is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn with_no_picker_the_button_is_off_and_on_the_web_the_file_downloads() {
    let mut h = harness();
    card_on(&mut h, "B2", "small");
    assert!(h.get_by_label(SAVE_CSV).accesskit_node().is_disabled());
    drop(h);
    let got = Rc::new(RefCell::new(Vec::new()));
    let sink = got.clone();
    let services = Services {
        download: Some(Box::new(move |name: &str, bytes: &[u8]| sink.borrow_mut().push((name.to_string(), bytes.to_vec())))),
        ..Services::default()
    };
    let mut h = harness_with_services(services);
    card_on(&mut h, "B2", "small");
    click(&mut h, SAVE_CSV);
    assert_eq!(got.borrow().as_slice(), [("Book1-B2.csv".to_string(), b"col0,col1\r\n0,1\r\n2,3\r\n".to_vec())]);
}

#[test]
fn the_command_with_a_path_opens_no_picker_and_from_a_menu_it_asks() {
    let dir = temp("command");
    let offered = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness_with_services(services_picking(Some(dir.join("menu.csv")), offered.clone()));
    card_on(&mut h, "B2", "small");
    let path = dir.join("programmatic.csv");
    h.state_mut().run("python.saveCsv", json!({"cell": "B2", "path": path})).unwrap();
    assert!(path.exists());
    assert!(offered.borrow().is_empty(), "a programmatic call opens no dialog");
    assert!(h.state_mut().run("python.saveCsv", json!({"cell": "B2"})).is_err(), "and without a path it is an error");
    assert!(offered.borrow().is_empty());
    h.state_mut().run_or_alert("python.saveCsv", json!({}));
    h.run_steps(2);
    assert_eq!(offered.borrow().len(), 1, "a menu asks where");
    assert!(dir.join("menu.csv").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn spill_here_turns_the_cell_to_an_excel_value() {
    let t = FakeTransport::new(|mut w| {
        w.ready();
        while let Some(req) = w.next_request() {
            let gridcraft_python::protocol::Request::Run { id, object: is_object, .. } = req else { continue };
            let list = gridcraft_engine::core::Array::new(2, 1, vec![1.0.into(), 2.0.into()]).map(Value::from).unwrap_or(Value::Empty);
            let out = if is_object {
                object("list", vec![2], Table { columns: Vec::new(), data: vec![1.0.into(), 2.0.into()], ..frame(2, 1, 2, 1) })
            } else {
                Output::Value(list)
            };
            w.done(id, Ok(out));
        }
    });
    let mut app = app_with(Services::default());
    app.python.transport = Some(Arc::new(move || Box::new(t.sharing())));
    let mut h = harness_sized(app, vec2(1300.0, 800.0));
    card_on(&mut h, "B2", "rows");
    click(&mut h, SPILL_HERE);
    assert!(h.state().grid.py_card.is_none(), "the card has done its work");
    settle(&mut h);
    let got: Json = h.state_mut().session.execute("cell.get", json!({"cell": "B2"})).unwrap();
    assert_eq!(got["python"]["output"], "value");
    assert_eq!((value(&h, "B2"), value(&h, "B3")), (Value::Number(1.0), Value::Number(2.0)));
    // One undo step takes it back to the object.
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(h.state_mut().session.execute("cell.get", json!({"cell": "B2"})).unwrap()["python"]["output"], "object");
}

#[test]
fn on_a_locked_cell_spill_here_and_the_lines_option_are_off() {
    let mut h = harness();
    card_on(&mut h, "B2", "small");
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    h.state_mut().run("python.set", json!({"cell": "D2", "code": "text"})).unwrap();
    settle(&mut h);
    h.state_mut().session.execute("review.protectSheet", json!({})).unwrap();
    h.run_steps(2);
    h.state_mut().session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.get_by_label(SPILL_HERE).accesskit_node().is_disabled());
    assert!(!h.get_by_label(COPY_CSV).accesskit_node().is_disabled(), "reading it is fine");
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"cell": "D2"})).unwrap();
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.get_by_label(SPILL_LINES).accesskit_node().is_disabled());
}

#[test]
fn the_lines_option_spills_a_text_and_takes_it_back() {
    let mut h = harness();
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "text"})).unwrap();
    settle(&mut h);
    assert_eq!(value(&h, "B2"), Value::text("one\ntwo\nthree"), "one cell by default");
    h.state_mut().session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    let d = h.state().session.active().unwrap();
    assert_eq!(python_card::card_text(&d.wb, d.wb.active().unwrap(), c("B2")).unwrap().spill_lines, Some(false));
    click(&mut h, SPILL_LINES);
    assert_eq!((value(&h, "B2"), value(&h, "B4")), (Value::text("one"), Value::text("three")));
    assert!(h.state().grid.py_card.is_some(), "the card stays, with the option on");
    click(&mut h, SPILL_LINES);
    assert_eq!((value(&h, "B2"), value(&h, "B3")), (Value::text("one\ntwo\nthree"), Value::Empty));
    // A spilled line shows its anchor's card, with the option.
    click(&mut h, SPILL_LINES);
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"cell": "B3"})).unwrap();
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label(SPILL_LINES).is_some());
    // A text of one line has nothing to spill.
    h.state_mut().run("ui.pythonCard", json!({"on": false})).unwrap();
    h.state_mut().run("python.set", json!({"cell": "D2", "code": "other"})).unwrap();
    settle(&mut h);
    h.state_mut().session.execute("selection.set", json!({"cell": "D2"})).unwrap();
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label(SPILL_LINES).is_none());
}

#[test]
fn after_reopening_the_card_says_to_run_the_cell_to_see_the_table() {
    let mut h = harness();
    card_on(&mut h, "B2", "cut");
    assert!(h.query_by_label(RUN_TO_SEE).is_none());
    let bytes = gridcraft_engine::io::save_bytes(&h.state().session.active().unwrap().wb, "o.xlsx").unwrap();
    let (back, _) = gridcraft_engine::io::open_bytes("o.xlsx", &bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().value(c("B2")), Value::text("DataFrame 1000x50"));

    // Reopened: the name and its size, and no table. Python is on and ready here, and the cell is
    // the same code, so nothing has run it yet.
    let mut s = Session::new();
    s.new_workbook();
    let doc = s.doc_mut().unwrap();
    doc.wb = Arc::new(back);
    let mut app = crate::common::prepare(SheetApp::new(s, Services::default()));
    app.ui.python.enabled = false;
    let mut h = harness_sized(app, vec2(1300.0, 800.0));
    h.state_mut().session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    h.state_mut().run("ui.pythonCard", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("DataFrame 1000x50").is_some(), "the file kept its name");
    assert!(h.query_by_label(RUN_TO_SEE).is_some(), "and says how to see the table");
    assert!(h.query_by_label(COPY_CSV).is_none(), "there is nothing to copy");
    assert!(h.query_by_label(SPILL_HERE).is_some(), "it can still go to the sheet");
    assert!(h.state_mut().run("python.table", json!({"cell": "B2"})).is_err());
    h.check_accessibility();
}

#[test]
fn a_python_object_with_no_table_from_a_file_that_names_none_says_nothing() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("python.set", json!({"cell": "A1", "code": "x", "output": "object"})).unwrap();
    {
        let d = s.doc_mut().unwrap();
        let wb = Arc::make_mut(&mut d.wb);
        wb.sheet_mut(0).unwrap().cells.get_mut(c("A1")).unwrap().value = Value::text("dict 3");
    }
    let d = s.active().unwrap();
    let text = python_card::card_text(&d.wb, d.wb.active().unwrap(), c("A1")).unwrap();
    assert!(!text.run_to_see && !text.spillable);
}
