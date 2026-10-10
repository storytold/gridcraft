//! UI interaction tests: real egui input through the whole app (no GPU needed).

use egui::{Event, Key, Modifiers, PointerButton};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

#[test]
fn vertical_scrollbar_pages_and_drags_without_changing_selection() {
    let mut h = harness(blank());
    let grid = h.state().grid.rect.unwrap();
    let cells = h.state().grid.cells_rect.unwrap();
    let p = egui::pos2(grid.right() + 7.0, cells.bottom() - 20.0);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(h.state().view().scroll.y > 0.0, "clicking the scrollbar track scrolls down");
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));

    h.state_mut().view_mut().unwrap().scroll.y = 0.0;
    h.run_steps(2);
    let start = egui::pos2(grid.right() + 7.0, cells.top() + 8.0);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.input_mut().events.push(Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let end = start + egui::vec2(0.0, 80.0);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.run_steps(2);
    let dragged = h.state().view().scroll.y;
    assert!(dragged > 0.0, "dragging the thumb scrolls down");
    h.run_steps(3);
    assert_eq!(h.state().view().scroll.y, dragged, "holding the thumb still must not keep scrolling");
    h.input_mut().events.push(Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));
}

fn key(h: &mut egui_kittest::Harness<'static, SheetApp>, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn text(h: &mut egui_kittest::Harness<'static, SheetApp>, t: &str) {
    h.input_mut().events.push(Event::Text(t.into()));
    h.run_steps(2);
}

fn value(h: &egui_kittest::Harness<'static, SheetApp>, a: &str) -> Value {
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap_or_default()))).unwrap_or_default()
}

fn click_cell(h: &mut egui_kittest::Harness<'static, SheetApp>, cell: &str) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

fn click_chart(h: &mut egui_kittest::Harness<'static, SheetApp>, id: u32) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let anchor = sheet.charts.iter().find(|chart| chart.id == id).unwrap().anchor;
    let pos = egui::pos2(geo.x(sheet, anchor.cell.col) + anchor.dx * geo.z, geo.y(sheet, anchor.cell.row) + anchor.dy * geo.z)
        + egui::vec2(anchor.width, anchor.height) * geo.z / 2.0;
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

#[test]
fn keyboard_navigation_does_not_replay_the_last_pointer_click() {
    let mut h = harness(blank());
    click_cell(&mut h, "B2");
    for (key_code, modifiers, expected) in [
        (Key::Enter, Modifiers::NONE, "B3"),
        (Key::Enter, Modifiers::NONE, "B4"),
        (Key::Enter, Modifiers::NONE, "B5"),
        (Key::Enter, Modifiers::SHIFT, "B4"),
        (Key::Tab, Modifiers::NONE, "C4"),
        (Key::Enter, Modifiers::NONE, "C5"),
    ] {
        key(&mut h, key_code, modifiers);
        let selection = &h.state().session.active().unwrap().selection;
        assert_eq!(selection.active.a1(), expected);
        assert_eq!(selection.anchor, selection.active);
        assert!(h.state().editor.is_none(), "navigation must not start editing at the old pointer location");
    }
    // A subsequent real pointer click must still select the clicked cell.
    click_cell(&mut h, "D6");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "D6");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "D7");
}

#[test]
fn space_toggles_the_active_checkbox_without_clicking_the_hovered_cell() {
    let mut session = blank();
    session.execute("insert.checkbox", json!({"range": "A1:B1"})).unwrap();
    let mut h = harness(session);
    click_cell(&mut h, "A1");
    assert_eq!(value(&h, "A1"), Value::Bool(true));
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "B1");
    assert_eq!(value(&h, "A1"), Value::Bool(true), "the old pointer location must remain unchanged");
    assert_eq!(value(&h, "B1"), Value::Bool(true));
    assert!(h.state().editor.is_none());
}

#[test]
fn typing_enters_values_and_moves_down() {
    let mut h = harness(blank());
    text(&mut h, "42");
    assert!(h.state().editor.is_some(), "typing starts editing");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Number(42.0));
    text(&mut h, "=A1*2");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(value(&h, "A2"), Value::Number(84.0));
    let active = h.state().session.active().map(|d| d.selection.active.a1());
    assert_eq!(active.as_deref(), Some("B2"));
}

#[test]
fn arrows_escape_and_undo() {
    let mut h = harness(blank());
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(h.state().session.active().map(|d| d.selection.active.a1()).as_deref(), Some("B2"));
    text(&mut h, "hello");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), Value::Empty);
    text(&mut h, "x");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), Value::from("x"));
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(value(&h, "B2"), Value::Empty);
}

#[test]
fn shortcuts_format_and_extend() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    let mut h = harness(s);
    key(&mut h, Key::ArrowRight, Modifiers::SHIFT);
    key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(h.state().session.active().map(|d| d.selection.a1()).as_deref(), Some("A1:B2"));
    key(&mut h, Key::B, Modifiers::COMMAND);
    let bold = h.state_mut().session.run("cell.get", json!({"cell": "B2"})).unwrap()["style"]["font"]["bold"].clone();
    assert_eq!(bold, json!(true));
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Empty);
}

#[test]
fn delete_key_removes_the_selected_chart() {
    let mut session = blank();
    session.execute("range.setValues", json!({"range": "A1:B2", "values": [[1, 2], [3, 4]]})).unwrap();
    let id = session.execute("insert.chart", json!({"range": "A1:B2", "at": "D2"})).unwrap()["chart"].as_u64().unwrap() as u32;
    let mut h = harness(session);

    click_cell(&mut h, "C10");
    click_chart(&mut h, id);
    assert_eq!(h.state().selected_chart, Some(id));

    key(&mut h, Key::Delete, Modifiers::NONE);

    assert!(h.state().session.active().unwrap().wb.active().unwrap().charts.is_empty());
    assert_eq!(h.state().selected_chart, None);
    assert_eq!(value(&h, "A1"), Value::Number(1.0));
}

#[test]
fn renders_every_ribbon_tab_without_panicking() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    let mut h = harness(s);
    for tab in ["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"] {
        h.state_mut().ui.ribbon_tab = tab.into();
        h.run_steps(2);
    }
    h.state_mut().ui.dark = true;
    h.run_steps(3);
}

#[test]
fn language_switch_translates_the_ribbon_and_persists() {
    use gridcraft_ui_egui::i18n::Language;
    let mut h = harness(blank());
    // English by default whatever the host's locale: only the desktop app reads the system language.
    assert_eq!(h.state().ui.language, Language::En);
    // The language commands are listed with the other UI commands.
    let ctx = h.ctx.clone();
    let gridcraft_ui_egui::control::Outcome::Done(listed) = gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, "engine.commands", &json!({}))
    else {
        panic!("engine.commands returns a response");
    };
    let ids: Vec<&str> = listed["result"].as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()).collect();
    for id in ["app.language.set", "app.language.english", "app.language.japanese"] {
        assert!(ids.contains(&id), "engine.commands lists {id}");
    }
    for (language, expected) in
        [(Language::Zh, "数据"), (Language::Ja, "データ"), (Language::Ko, "데이터"), (Language::Ru, "Данные"), (Language::PtBr, "Dados")]
    {
        h.state_mut().run("app.language.set", json!({"language": language.code()})).unwrap();
        h.run_steps(2); // every locale renders the ribbon without panicking
        assert_eq!(h.state().ui.language, language);
        assert_eq!(language.tr("Data"), expected);
        let restored: gridcraft_ui_egui::UiState = serde_json::from_str(&serde_json::to_string(&h.state().ui).unwrap()).unwrap();
        assert_eq!(restored.language, language);
    }
    h.state_mut().run("app.language.set", json!({"language": "en"})).unwrap();
    assert_eq!(h.state().ui.language, Language::En);
}

#[test]
fn dialogs_open_and_close() {
    let mut h = harness(blank());
    for name in [
        "formatCells",
        "insertFunction",
        "find",
        "sort",
        "nameManager",
        "dataValidation",
        "pasteSpecial",
        "commandSearch",
        "spelling",
        "goTo",
        "about",
    ] {
        h.state_mut().open_dialog(name, json!({}));
        h.run_steps(2);
        key(&mut h, Key::Escape, Modifiers::NONE);
        h.state_mut().dialog = None;
        h.state_mut().message = None;
    }
}

#[test]
fn about_renders_every_tab() {
    let mut h = harness(blank());
    for tab in ["About", "Contributors", "Models"] {
        h.state_mut().open_dialog("about", json!({"tab": tab}));
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.tab.clone()).as_deref(), Some(tab));
        h.state_mut().dialog = None;
    }
}

#[test]
fn column_autocomplete_completes_on_enter() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["North"], ["East"], ["Eastern"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A4"})).unwrap();
    let mut h = harness(s);
    text(&mut h, "No");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A4"), Value::from("North"));
    text(&mut h, "Ea"); // ambiguous: East / Eastern
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A5"), Value::from("Ea"));
}

#[test]
fn general_numbers_fit_by_the_cells_own_font() {
    // 9pt marks in narrow columns (a stored width of 4, i.e. 28px; common in mark sheets): "100" fits.
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[100, 123456789]]})).unwrap();
    s.execute("home.fontSize", json!({"range": "A1:B1", "size": 9})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "width": 28})).unwrap();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let h = harness(s);
    let texts: Vec<String> = h
        .output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t == "100"), "100 drawn in full: {texts:?}");
    // A number that really doesn't fit still shows #### (once: only B1).
    assert_eq!(texts.iter().filter(|t| t.starts_with('#')).count(), 1, "{texts:?}");
}

fn text_shapes(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(egui::epaint::TextShape, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.clone(), clip)),
        egui::Shape::Vec(v) => v.iter().for_each(|s| text_shapes(s, clip, out)),
        _ => {}
    }
}

#[test]
fn rotated_wrapped_text_is_drawn_rotated_inside_its_cell() {
    // Narrow columns with tall, upright headers (mark sheets): rotation and Wrap Text together.
    let header = "Term 1 : Drama : Continuous Assessment";
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[header, header]]})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1:B1", "on": true})).unwrap();
    s.execute("home.orientation", json!({"range": "A1", "angle": "up"})).unwrap();
    s.execute("home.orientation", json!({"range": "B1", "angle": "down"})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "chars": 6})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1:1", "height": 267})).unwrap();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let h = harness(s);
    let mut found = Vec::new();
    for c in &h.output().shapes {
        text_shapes(&c.shape, c.clip_rect, &mut found);
    }
    let rotated: Vec<_> = found.iter().filter(|(t, _)| t.galley.job.text == header).collect();
    assert_eq!(rotated.len(), 2, "both headers drawn as one text block each");
    for (t, clip) in rotated {
        assert!((t.angle.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "drawn at ±90°, got {}", t.angle);
        // Wrapped along the row height into a couple of long lines, not across the narrow column.
        assert!(t.galley.rows.len() <= 3, "{} lines", t.galley.rows.len());
        let bb = t.visual_bounding_rect();
        assert!(bb.height() > bb.width(), "upright, not flat: {bb:?}");
        assert!(clip.expand(1.0).contains_rect(bb), "inside its cell: {bb:?} not in {clip:?}");
        // Upright text never spills into the neighbouring columns: the clip is the cell itself.
        assert!(clip.width() < 70.0, "clip spans one 6-character column, got {clip:?}");
    }
}
