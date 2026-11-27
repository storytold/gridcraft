//! Searching in a table or AutoFilter column menu accumulates selections across searches and
//! across reopenings instead of overwriting active choices outside the current search (issue #4768).

use crate::common;

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::grid;
use serde_json::json;

fn key(h: &mut common::Harness, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers });
    }
    h.run_steps(3);
}

fn click_filter(h: &mut common::Harness, cell: &str) {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let at = grid::filter_button_rect(&geo, sh, CellRef::parse(cell).unwrap()).center();
    h.input_mut().events.push(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run_steps(3);
}

fn type_search(h: &mut common::Harness, text: &str) {
    h.state_mut().grid.filter_search.clear();
    h.run_steps(2);
    if !text.is_empty() {
        h.state_mut().grid.filter_search = text.to_string();
        h.run_steps(2);
    }
}

fn hidden(h: &common::Harness, rows: std::ops::RangeInclusive<u32>) -> Vec<bool> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    rows.map(|r| sh.is_row_hidden(r)).collect()
}

fn fruit_table_harness() -> common::Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Fruit"], ["Apple"], ["Banana"], ["Cherry"], ["Date"]]})).unwrap();
    session.execute("insert.table", json!({"range": "A1:A5", "header": true})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    common::os_harness(session, egui::os::OperatingSystem::Windows)
}

#[test]
fn reopening_table_filter_and_searching_appends_to_active_selection() {
    for use_enter in [false, true] {
        let mut h = fruit_table_harness();

        // First search on an unfiltered table narrows to Apple.
        click_filter(&mut h, "A1");
        assert!(h.state().grid.filter_menu.is_some());
        h.input_mut().events.push(Event::Text("App".into()));
        h.run_steps(2);
        if use_enter {
            key(&mut h, Key::Enter, Modifiers::NONE);
        } else {
            h.get_by_label("Apply").click();
            h.run_steps(3);
        }
        assert!(h.state().grid.filter_menu.is_none());
        assert_eq!(hidden(&h, 1..=4), [false, true, true, true], "initial search shows only Apple (enter={use_enter})");

        // Reopen, search for Banana, check it, and apply without clearing search: Apple must stay selected.
        click_filter(&mut h, "A1");
        assert!(h.state().grid.filter_menu.is_some());
        h.input_mut().events.push(Event::Text("Ban".into()));
        h.run_steps(2);
        h.get_by_label("Banana").click();
        h.run_steps(2);
        if use_enter {
            key(&mut h, Key::Enter, Modifiers::NONE);
        } else {
            h.get_by_label("Apply").click();
            h.run_steps(3);
        }
        assert!(h.state().grid.filter_menu.is_none());
        assert_eq!(hidden(&h, 1..=4), [false, false, true, true], "reopening and searching for Banana must append to Apple (enter={use_enter})");

        // Reopen again, search for Cherry, check via (Select All Search Results), and apply.
        click_filter(&mut h, "A1");
        h.input_mut().events.push(Event::Text("Che".into()));
        h.run_steps(2);
        h.get_by_label("(Select All Search Results)").click();
        h.run_steps(2);
        if use_enter {
            key(&mut h, Key::Enter, Modifiers::NONE);
        } else {
            h.get_by_label("Apply").click();
            h.run_steps(3);
        }
        assert_eq!(
            hidden(&h, 1..=4),
            [false, false, false, true],
            "selecting all search results appends Cherry to Apple and Banana (enter={use_enter})"
        );
    }
}

#[test]
fn multiple_searches_within_one_menu_session_accumulate_selections() {
    // 1) Starting unfiltered: uncheck (Select All), then pick Apple and Banana across two searches.
    let mut h = fruit_table_harness();
    click_filter(&mut h, "A1");
    h.get_by_label("(Select All)").click();
    h.run_steps(2);
    type_search(&mut h, "App");
    h.get_by_label("Apple").click();
    h.run_steps(2);
    type_search(&mut h, "Ban");
    h.get_by_label("Banana").click();
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(hidden(&h, 1..=4), [false, false, true, true], "selections from both searches must apply");

    // 2) Reopening with Apple + Banana active: add Cherry and Date across two searches in one session.
    click_filter(&mut h, "A1");
    type_search(&mut h, "Che");
    h.get_by_label("Cherry").click();
    h.run_steps(2);
    type_search(&mut h, "Dat");
    h.get_by_label("Date").click();
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(hidden(&h, 1..=4), [false, false, false, false], "selecting every remaining item clears the filter");

    // 3) Removing one item via search while another filter is active keeps the remaining items.
    h.state_mut().run("data.filterBy", json!({"column": "A", "values": ["Apple", "Banana", "Cherry"]})).unwrap();
    h.run_steps(2);
    click_filter(&mut h, "A1");
    type_search(&mut h, "Ban");
    h.get_by_label("Banana").click();
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().grid.filter_menu.is_none());
    assert_eq!(hidden(&h, 1..=4), [false, true, false, true], "unchecking Banana via search keeps Apple and Cherry");

    // 4) Starting unfiltered, unchecking two values across two searches accumulates both exclusions.
    h.state_mut().run("data.filterBy", json!({"column": "A", "clear": true})).unwrap();
    h.run_steps(2);
    click_filter(&mut h, "A1");
    type_search(&mut h, "Che");
    h.get_by_label("Cherry").click();
    h.run_steps(2);
    type_search(&mut h, "Dat");
    h.get_by_label("Date").click();
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(hidden(&h, 1..=4), [false, false, true, true], "exclusions across searches must accumulate");
}

#[test]
fn unfiltered_single_search_still_narrows_and_cascading_append_preserves_hidden_values() {
    // Single search on an unfiltered column with one match unchecked narrows to the remaining match.
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Fruit"], ["Apple"], ["Apricot"], ["Banana"], ["Cherry"]]})).unwrap();
    session.execute("insert.table", json!({"range": "A1:A5", "header": true})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    let mut h = common::os_harness(session, egui::os::OperatingSystem::Windows);
    click_filter(&mut h, "A1");
    h.input_mut().events.push(Event::Text("Ap".into()));
    h.run_steps(2);
    h.get_by_label("Apricot").click();
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert_eq!(hidden(&h, 1..=4), [false, true, true, true], "single unfiltered search narrows to checked matches");

    // Cascading table filter: appending via search on column B preserves values hidden by column A.
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "range.setValues",
            json!({"range": "A1", "values": [
                ["Region", "Fruit"],
                ["East", "Apple"],
                ["East", "Banana"],
                ["East", "Cherry"],
                ["West", "Other"]
            ]}),
        )
        .unwrap();
    session.execute("insert.table", json!({"range": "A1:B5", "header": true})).unwrap();
    session.execute("data.filterBy", json!({"column": "B", "values": ["Apple", "Other"]})).unwrap();
    session.execute("data.filterBy", json!({"column": "A", "values": ["East"]})).unwrap();
    session.execute("selection.set", json!({"cell": "B1"})).unwrap();
    let mut h = common::os_harness(session, egui::os::OperatingSystem::Windows);
    click_filter(&mut h, "B1");
    h.input_mut().events.push(Event::Text("Ban".into()));
    h.run_steps(2);
    h.get_by_label("Banana").click();
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    h.state_mut().run("data.filterBy", json!({"column": "A", "clear": true})).unwrap();
    assert_eq!(hidden(&h, 1..=4), [false, false, true, false], "appending Banana preserves Apple and column-A-hidden Other");
}
