//! #1906: the Sort dialog seeds "Sort by" from the active column instead of a
//! free-text box stuck on "A", and the column list offers the region's columns.

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A harness around the app with `session` loaded.
fn harness(session: Session) -> Harness {
    crate::support::harness(session, 4)
}

/// Header row + three rows; the user's cursor sits in column B (Revenue).
fn sales() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Month", "Revenue"], ["Mar", 30], ["Jan", 10], ["Feb", 20]]})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    s
}

/// The column of the first sort level in the open dialog.
fn col(h: &Harness) -> String {
    h.state().dialog.as_ref().unwrap().values["levels"][0]["column"].as_str().unwrap_or("<missing>").to_string()
}

/// Sort by starts on the column the cursor is in.
#[test]
fn sort_dialog_seeds_the_active_column() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    assert_eq!(col(&h), "B", "Sort by starts on the column the cursor is in, not A");
}

/// #3446: the closed "Sort by" box shows the column's name, as the open list does
/// ("Revenue", not just the letter "B").
#[test]
fn closed_column_box_shows_the_column_name() {
    use egui::accesskit::Role;
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    let shown: Vec<String> = h.get_all_by_role(Role::ComboBox).filter_map(|n| n.value()).collect();
    assert!(shown.iter().any(|v| v == "Revenue"), "the Sort by box shows the column name: {shown:?}");
}

/// Add Level picks a column no other level uses.
#[test]
fn add_level_picks_a_column_no_level_uses_yet() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    // Level 1 seeded B; clicking "Add Level" must not default level 2 to B too (#1906 review).
    h.run_steps(6);
    h.get_by_label("Add Level").click();
    h.run_steps(6);
    let cols: Vec<String> = h.state().dialog.as_ref().unwrap().values["levels"]
        .as_array()
        .map(|a| a.iter().filter_map(|l| l["column"].as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    assert_eq!(cols.len(), 2, "two levels after Add Level");
    assert_ne!(cols[0], cols[1], "the second level defaults to a different column: {cols:?}");
}

/// Add Level is not limited by the number of columns (Excel allows many levels): with every
/// column in use it adds a level on the selected level's column, and never one outside the range.
#[test]
fn add_level_keeps_adding_levels_when_every_column_is_used() {
    let mut s = sales();
    s.execute("selection.set", json!({"range": "A1:A4", "active": "A2"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    for _ in 0..2 {
        h.get_by_label("Add Level").click();
        h.run_steps(4);
    }
    let levels = h.state().dialog.as_ref().unwrap().values["levels"].as_array().unwrap();
    assert_eq!(levels.len(), 3, "Add Level stays available with one column");
    assert!(levels.iter().all(|l| l["column"] == "A"), "no level uses a column outside A: {levels:?}");
}

/// In the interface languages whose scripts the bundled fonts cover (CJK and Vietnamese need the
/// optional font repo), the toolbar (level buttons and the headers checkbox) stays on one row without
/// overlapping: the window widens for a longer translation.
#[test]
fn toolbar_fits_without_overlap_in_every_language() {
    use gridcraft_ui_egui::i18n::Language;
    for language in Language::ALL.into_iter().filter(|l| !matches!(l, Language::Ja | Language::Ko | Language::Zh | Language::Vi)) {
        let mut app = crate::common::app(sales());
        app.ui.language = language;
        let mut h = crate::support::app_harness(app, 4);
        h.state_mut().open_dialog("sort", json!({}));
        h.run_steps(6);
        let rects: Vec<egui::Rect> = ["Add Level", "Delete Level", "Copy Level", "Move Up", "Move Down", "My list has headers"]
            .iter()
            .map(|key| h.get_by_label(language.tr(key)).rect())
            .collect();
        let code = language.code();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.intersects(*b), "{code}: toolbar items overlap: {a:?} {b:?}");
            }
            assert!((a.center().y - rects[0].center().y).abs() < 12.0, "{code}: the toolbar wrapped: {rects:?}");
        }
    }
}

/// Escape restores every selected area and the active cell.
#[test]
fn sort_dialog_restores_all_selected_areas_and_the_active_cell() {
    let mut s = sales();
    s.execute("selection.set", json!({"range": "A1:A4,B1:B4", "active": "B2"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Key {
        key: Key::Escape,
        physical_key: Some(Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let selection = &h.state().session.active().unwrap().selection;
    assert_eq!(selection.a1(), "A1:A4,B1:B4");
    assert_eq!(selection.active.a1(), "B2");
}

/// Cancel after toggling headers restores the original selection.
#[test]
fn sort_dialog_restores_initial_selection_after_header_toggle() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.get_by_label("My list has headers").click();
    h.run_steps(2);
    h.get_by_label(" Cancel ").click();
    h.run_steps(2);
    let selection = &h.state().session.active().unwrap().selection;
    assert_eq!(selection.a1(), "B2");
    assert_eq!(selection.active.a1(), "B2");
}

/// OK after toggling headers restores the original selection.
#[test]
fn sort_dialog_restores_initial_selection_after_confirming_with_header_toggle() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.get_by_label("My list has headers").click();
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(2);
    let selection = &h.state().session.active().unwrap().selection;
    assert_eq!(selection.a1(), "B2");
    assert_eq!(selection.active.a1(), "B2");
}

/// OK sorts the rows by the seeded column.
#[test]
fn sorting_by_the_seeded_column_orders_the_rows() {
    let mut h = harness(sales());
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    // Accept the dialog: rows must come back ordered by Revenue (10, 20, 30).
    h.input_mut().events.push(egui::Event::Key {
        key: Key::Enter,
        physical_key: Some(Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    let nums: Vec<i64> = (1..4).map(|r| sh.value(gridcraft_engine::core::CellRef::new(r, 1))).filter_map(|v| v.as_f64().map(|n| n as i64)).collect();
    assert_eq!(nums, vec![10, 20, 30], "sort ran on the seeded column B");
}

/// Worksheet-changing commands are refused while Sort is open.
#[test]
fn sheet_tabs_cannot_switch_while_sort_is_open() {
    let mut s = sales();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    for id in ["sheet.activate", "sheet.next", "sheet.previous", "sheet.move", "sheet.hide", "sheet.unhide", "home.insertSheet", "home.deleteSheet"] {
        assert!(h.state_mut().run(id, json!({"sheet": 1})).is_err(), "{id} is refused while Sort is open");
    }
    assert_eq!(h.state().session.active().unwrap().wb.active_sheet(), 0);
    assert!(h.state().dialog.is_some());
}

/// OK on another worksheet than the one Sort opened on refuses to sort and keeps the dialog open.
#[test]
fn sort_refuses_to_run_on_another_worksheet_and_stays_open() {
    let mut s = sales();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(2);
    let state = h.state();
    assert_eq!(state.dialog.as_ref().map(|d| d.name.as_str()), Some("sort"), "the dialog stays open with its levels");
    assert!(state.message.is_some(), "the refusal is explained");
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    let doc = h.state().session.active().unwrap();
    let first = doc.wb.active().unwrap().value(gridcraft_engine::core::CellRef::new(1, 1)).as_f64();
    assert_eq!(first, Some(30.0), "the original worksheet was not sorted");
}

/// The dialog follows its worksheet by identity: moving the tab keeps Sort working on it.
#[test]
fn sort_still_runs_after_its_worksheet_moves_to_another_position() {
    let mut s = sales();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.state_mut().session.execute("sheet.move", json!({"sheet": 0, "to": 1})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().wb.active_sheet(), 1);
    h.get_by_label("   OK   ").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "the sort ran and the dialog closed");
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    let revenue: Vec<_> = (1..4).map(|r| sh.value(gridcraft_engine::core::CellRef::new(r, 1)).as_f64()).collect();
    assert_eq!(revenue, [Some(10.0), Some(20.0), Some(30.0)], "the moved worksheet was sorted");
}

/// A different worksheet that takes the opening worksheet's position is not the opening worksheet.
#[test]
fn sort_refuses_a_different_worksheet_at_the_same_position() {
    let mut s = sales();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.state_mut().session.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().wb.active_sheet(), 0);
    h.get_by_label("   OK   ").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("sort"));
    assert!(h.state().message.is_some(), "the refusal is explained");
    assert!(!h.state().session.journal.iter().any(|(id, _)| id == "data.sort"), "nothing was sorted");
}

/// Cancelling while another worksheet is active still restores the opening worksheet's selection.
#[test]
fn sort_cancel_restores_the_selection_of_the_opening_worksheet() {
    let mut s = sales();
    s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("sort", json!({}));
    h.run_steps(2);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(2);
    let other = h.state().session.active().unwrap().selection.a1();
    let done = gridcraft_ui_egui::control::handle(h.state_mut(), &egui::Context::default(), "ui.dialog.cancel", &json!({}));
    assert!(matches!(done, gridcraft_ui_egui::control::Outcome::Done(_)));
    assert_eq!(h.state().session.active().unwrap().selection.a1(), other, "the active worksheet's selection is left alone");
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    assert_eq!(h.state().session.active().unwrap().selection.a1(), "B2", "the opening worksheet got its selection back");
}

/// Control cancel and dialog replacement restore the selection.
#[test]
fn sort_selection_is_restored_when_control_cancels_or_replaces_the_dialog() {
    let mut cancelled = harness(sales());
    cancelled.state_mut().open_dialog("sort", json!({}));
    let done = gridcraft_ui_egui::control::handle(cancelled.state_mut(), &egui::Context::default(), "ui.dialog.cancel", &json!({}));
    assert!(matches!(done, gridcraft_ui_egui::control::Outcome::Done(_)));
    assert!(cancelled.state().dialog.is_none());
    assert_eq!(cancelled.state().session.active().unwrap().selection.a1(), "B2");

    let mut replaced = harness(sales());
    replaced.state_mut().open_dialog("sort", json!({}));
    replaced.state_mut().open_dialog("zoom", json!({}));
    assert_eq!(replaced.state().dialog.as_ref().unwrap().name, "zoom");
    assert_eq!(replaced.state().session.active().unwrap().selection.a1(), "B2");
}
