use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, DateSystem, Value};
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, gridcraft_ui_egui::SheetApp> {
    crate::support::harness(session, 4)
}

#[test]
fn date_picker_selects_a_date_as_one_undo_and_keeps_the_cell_format() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"=DATE(2024,2,1)"})).unwrap();
    session.execute("home.numberFormat", json!({"range":"A1", "code":"yyyy-mm-dd"})).unwrap();
    let mut h = harness(session);
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    let state = h.state_mut().grid.date_picker.as_mut().unwrap();
    state.year = 2024;
    state.month = 2;
    state.day = 29;
    h.input_mut().events.push(Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    assert_eq!(
        sheet.value_ref(CellRef::new(0, 0)),
        Some(&Value::Number(gridcraft_engine::core::date::serial_from_ymd(DateSystem::D1900, 2024, 2, 29).unwrap()))
    );
    assert_eq!(doc.wb.styles.get(sheet.style_id(CellRef::new(0, 0))).num_fmt.as_str(), "yyyy-mm-dd");
    assert!(h.state().grid.date_picker.is_none());
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active()).and_then(|s| s.value_ref(CellRef::new(0, 0))), Some(&Value::Number(45323.0)));
}

#[test]
fn date_picker_uses_the_1904_epoch() {
    let mut session = Session::new();
    session.new_workbook();
    std::sync::Arc::make_mut(&mut session.active_mut().unwrap().wb).date_system = DateSystem::D1904;
    let mut h = harness(session);
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    let state = h.state_mut().grid.date_picker.as_mut().unwrap();
    state.year = 1904;
    state.month = 1;
    state.day = 1;
    h.input_mut().events.push(Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active()).and_then(|s| s.value_ref(CellRef::new(0, 0))), Some(&Value::Number(0.0)));
}

#[test]
fn escape_cancels_without_changing_the_cell() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"42"})).unwrap();
    let mut h = harness(session);
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    h.input_mut().events.push(Event::Key {
        key: Key::Escape,
        physical_key: Some(Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    assert!(h.state().grid.date_picker.is_none());
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active()).and_then(|s| s.value_ref(CellRef::new(0, 0))), Some(&Value::Number(42.0)));
}

#[test]
fn opening_the_picker_moves_keyboard_focus_into_it() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness(session);
    let grid_focus = h.ctx.memory(|m| m.focused());
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    h.run_steps(1);
    assert!(h.state().grid.date_picker.is_some());
    assert!(h.ctx.memory(|m| m.focused()).is_some_and(|id| Some(id) != grid_focus), "opening the picker transfers keyboard focus from the grid");
}

#[test]
fn arrow_navigation_moves_focus_with_the_selected_day() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness(session);
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    h.run_steps(1);
    let initial_focus = h.ctx.memory(|m| m.focused());
    let initial = h.state().grid.date_picker.as_ref().unwrap();
    let expected =
        gridcraft_engine::core::date::serial_from_ymd(DateSystem::D1900, i64::from(initial.year), i64::from(initial.month), i64::from(initial.day))
            .unwrap()
            + 1.0;
    h.input_mut().events.push(Event::Key {
        key: Key::ArrowRight,
        physical_key: Some(Key::ArrowRight),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    assert_ne!(h.ctx.memory(|m| m.focused()), initial_focus, "focus follows the newly selected day");
    h.input_mut().events.push(Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active()).and_then(|s| s.value_ref(CellRef::new(0, 0))), Some(&Value::Number(expected)));
}

#[test]
fn changing_sheets_cancels_the_picker_instead_of_writing_to_the_new_sheet() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell":"A1", "input":"42"})).unwrap();
    let mut h = harness(session);
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    h.state_mut().run("home.insertSheet", json!({})).unwrap();
    h.input_mut().events.push(Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.wb.active_sheet(), 1);
    assert_eq!(doc.wb.active().and_then(|s| s.value_ref(CellRef::new(0, 0))), None);
    assert_eq!(doc.wb.sheets[0].value_ref(CellRef::new(0, 0)), Some(&Value::Number(42.0)));
    assert!(h.state().grid.date_picker.is_none());
}

#[test]
fn opening_the_picker_commits_an_active_cell_edit_before_capturing_the_value() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness(session);
    h.state_mut().begin_edit(Some("42".into()), false);
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    assert!(h.state().editor.is_none());
    assert_eq!(h.state().grid.date_picker.as_ref().map(|p| (p.year, p.month, p.day)), Some((1900, 2, 11)));
}

#[test]
fn a_refused_cell_edit_keeps_the_picker_closed() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness(session);
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    assert!(h.state().editor.is_some());
    assert!(h.state().grid.date_picker.is_none());
}

#[test]
fn right_to_left_picker_is_anchored_to_the_cells_right_edge() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    let mut h = harness(session);
    let (cell_rect, screen) = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        (geo.screen_cell_rect(sheet, CellRef::new(6, 0)), h.ctx.content_rect())
    };
    assert!(screen.contains_rect(cell_rect), "test cell should be on screen: {cell_rect:?} in {screen:?}");
    let cell_right = cell_rect.right();
    h.state_mut().run("ui.datePicker", json!({})).unwrap();
    h.run_steps(3);
    let picker = h.ctx.memory(|m| m.area_rect(egui::Id::unique("gridcraft.date_picker")));
    let picker_width = picker.map_or(0.0, |r| r.width());
    let picker_right = picker.map(|r| r.left() + picker_width);
    assert!(
        picker_width > 0.0 && picker.is_some_and(|r| screen.contains_rect(r)) && picker_right.is_some_and(|right| (right - cell_right).abs() < 1.0),
        "picker should end at the RTL cell's right edge: picker={picker:?}, cell={cell_rect:?}, screen={screen:?}"
    );
}

#[test]
fn opening_the_picker_by_shortcut_scrolls_the_active_cell_into_view() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell":"Z1000"})).unwrap();
    let mut h = harness(session);
    h.run_steps(1);
    h.input_mut().events.push(Event::Key {
        key: Key::P,
        physical_key: Some(Key::P),
        pressed: true,
        repeat: false,
        modifiers: Modifiers { ctrl: true, shift: true, ..Modifiers::NONE },
    });
    h.run_steps(2);
    assert!(h.state().grid.date_picker.is_some());
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let cell = geo.screen_cell_rect(sheet, CellRef::new(999, 25));
    assert!(geo.cells.intersects(cell), "active cell should be visible after the shortcut: {cell:?}");
}
