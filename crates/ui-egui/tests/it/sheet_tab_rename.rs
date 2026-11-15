//! Renaming a sheet tab: the name field must let go of the keyboard when the user is done (#145).

use egui::{Event, Key, Modifiers, PointerButton};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

fn start_rename(h: &mut egui_kittest::Harness<'static, SheetApp>, new_name: &str) {
    h.state_mut().grid.renaming_tab = Some(0);
    h.state_mut().grid.rename_text = new_name.to_string();
    h.run_steps(3);
}

fn sheet_name(h: &egui_kittest::Harness<'static, SheetApp>) -> String {
    h.state().session.active().unwrap().wb.sheets[0].name.clone()
}

fn rename_key(h: &mut egui_kittest::Harness<'static, SheetApp>, key: Key) {
    h.input_mut().events.extend([
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(3);
}

fn start_duplicate_name(h: &mut egui_kittest::Harness<'static, SheetApp>) {
    h.state_mut().run("home.insertSheet", json!({"name": "Taken"})).unwrap();
    h.state_mut().run("selection.set", json!({"sheet": 0, "cell": "A1"})).unwrap();
    h.run_steps(2);
    start_rename(h, "Taken");
}

#[test]
fn enter_saves_the_sheet_name_and_closes_the_field() {
    let mut h = harness();
    start_rename(&mut h, "Budget");
    h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(h.state().grid.renaming_tab, None, "Enter must close the rename field");
    assert_eq!(sheet_name(&h), "Budget");
}

#[test]
fn clicking_a_cell_saves_the_sheet_name_and_closes_the_field() {
    let mut h = harness();
    start_rename(&mut h, "Ledger");
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = geo.cell_rect(sheet, CellRef::parse("B2").unwrap()).center();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(3);
    assert_eq!(h.state().grid.renaming_tab, None, "clicking elsewhere must close the rename field");
    assert_eq!(sheet_name(&h), "Ledger");
}

#[test]
fn a_rejected_name_stays_open_and_can_be_corrected_after_the_message() {
    for dismiss in [Key::Enter, Key::Escape] {
        let mut h = harness();
        start_duplicate_name(&mut h);
        let edit_id = h.ctx.memory(|m| m.focused()).unwrap();
        rename_key(&mut h, Key::Enter);
        assert_eq!(sheet_name(&h), "Sheet1");
        assert_eq!(h.state().grid.renaming_tab, Some(0));
        assert_eq!(h.state().grid.rename_text, "Taken");
        assert!(h.state().message.is_some(), "the submitting Enter must not dismiss the error");
        for _ in 0..2 {
            h.run_steps(1);
            assert!(h.state().message.is_some());
            assert_ne!(h.ctx.memory(|m| m.focused()), Some(edit_id), "the rename field must not steal modal focus");
        }
        rename_key(&mut h, dismiss);
        assert!(h.state().message.is_none());
        assert_eq!(h.state().grid.renaming_tab, Some(0), "dismissing the error must leave the draft open");
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(edit_id), "the retained draft takes the keyboard back");
        h.input_mut().events.extend([
            Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
            Event::Text("Recovered".into()),
        ]);
        h.run_steps(2);
        rename_key(&mut h, Key::Enter);
        assert_eq!(sheet_name(&h), "Recovered");
        assert_eq!(h.state().grid.renaming_tab, None);
        assert!(h.state().message.is_none());
    }
}

#[test]
fn a_rejected_name_after_clicking_away_can_be_cancelled() {
    let mut h = harness();
    start_duplicate_name(&mut h);
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = geo.cell_rect(sheet, CellRef::parse("B2").unwrap()).center();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(3);
    assert!(h.state().message.is_some());
    assert_eq!(h.state().grid.renaming_tab, Some(0));
    assert_eq!(h.state().grid.rename_text, "Taken");
    rename_key(&mut h, Key::Escape);
    assert!(h.state().message.is_none());
    assert_eq!(h.state().grid.renaming_tab, Some(0));
    rename_key(&mut h, Key::Escape);
    assert_eq!(h.state().grid.renaming_tab, None);
    assert_eq!(sheet_name(&h), "Sheet1");
    assert!(h.state().message.is_none());
}
