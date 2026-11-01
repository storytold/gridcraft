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

#[test]
fn a_header_only_tables_popup_has_no_value_choices() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "B3", "values": [["Company", "Job"]]})).unwrap();
    session.execute("insert.table", json!({"range": "B3:C3", "header": true})).unwrap();
    session.execute("selection.set", json!({"cell": "B3"})).unwrap();
    let mut h = common::os_harness(session, egui::os::OperatingSystem::Windows);
    click_filter(&mut h, "B3");
    assert_eq!(h.state().grid.filter_menu.map(|(col, row, _)| (col, row)), Some((1, 2)));
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sh.tables[0].range.height(), 1);
    assert_eq!(sh.autofilter.as_ref().unwrap().range.a1(), "B3:C3");
    assert_eq!(h.ctx.data(|d| d.get_temp::<Vec<(String, bool)>>(egui::Id::unique(("filter_sel", 1u32)))), Some(vec![]));
    assert!(h.query_by_label("(Blanks)").is_none());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().grid.filter_menu.is_none());
}

#[test]
fn switching_filter_columns_discards_old_edits_and_recomputes_eligible_choices() {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "range.setValues",
            json!({"range": "A1", "values": [
                ["Company", "Job"], ["East", "Alpha"], ["East", "Beta"], ["West", "Other"]
            ]}),
        )
        .unwrap();
    session.execute("data.filter", json!({"range": "A1:B4"})).unwrap();
    session.execute("selection.set", json!({"cell": "B1"})).unwrap();
    let mut h = common::os_harness(session, egui::os::OperatingSystem::Windows);
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    h.get_by_label("Other").click();
    h.run_steps(2);
    assert_eq!(h.ctx.data(|d| d.get_temp::<String>(egui::Id::unique(("filter_search", 1u32)))).as_deref(), Some(""));

    click_filter(&mut h, "A1");
    assert_eq!(h.state().grid.filter_menu.map(|(col, _, _)| col), Some(0));
    h.ctx.data(|d| {
        assert!(d.get_temp::<Vec<(String, bool)>>(egui::Id::unique(("filter_sel", 1u32))).is_none(), "old checkbox edits must be discarded");
        assert!(d.get_temp::<String>(egui::Id::unique(("filter_search", 1u32))).is_none(), "old search must be discarded");
        assert!(d.get_temp::<Vec<egui::Id>>(egui::Id::unique(("filter_buttons", 1u32))).is_none(), "old button focus IDs must be discarded");
    });
    assert!(h.query_by_label("East").is_some(), "the new column's choices must remain available");
    h.get_by_label("West").click();
    h.run_steps(2);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    click_filter(&mut h, "B1");
    assert!(h.query_by_label("Alpha").is_some());
    assert!(h.query_by_label("Beta").is_some(), "eligible alternatives must remain available");
    assert!(h.query_by_label("Other").is_none(), "the other column's new criterion must narrow the reopened list");
}

#[test]
fn applying_eligible_choices_preserves_values_and_blanks_hidden_by_another_filter() {
    for table in [false, true] {
        for click in [false, true] {
            for edit in [false, true] {
                for (values, blanks, other_hidden, blank_hidden) in
                    [(vec!["Alpha", "Other"], false, false, true), (vec!["Alpha", "Beta"], false, true, true), (vec!["Alpha"], true, true, false)]
                {
                    let mut session = Session::new();
                    session.new_workbook();
                    session
                        .execute(
                            "range.setValues",
                            json!({"range": "A1", "values": [
                                ["Company", "Job"], ["East", "Alpha"], ["East", "Beta"], ["West", "Other"], ["West", null]
                            ]}),
                        )
                        .unwrap();
                    if table {
                        session.execute("insert.table", json!({"range": "A1:B5", "header": true})).unwrap();
                    } else {
                        session.execute("data.filter", json!({"range": "A1:B5"})).unwrap();
                    }
                    session.execute("data.filterBy", json!({"column": "B", "values": values, "blanks": blanks})).unwrap();
                    session.execute("data.filterBy", json!({"column": "A", "values": ["East"]})).unwrap();
                    session.execute("selection.set", json!({"cell": "B1"})).unwrap();
                    let mut h = common::os_harness(session, egui::os::OperatingSystem::Windows);
                    key(&mut h, Key::ArrowDown, Modifiers::ALT);
                    assert!(h.query_by_label("Other").is_none());
                    assert!(h.query_by_label("(Blanks)").is_none());
                    if edit {
                        h.get_by_label("Beta").click();
                        h.run_steps(2);
                    }
                    if click {
                        h.get_by_label("Apply").click();
                        h.run_steps(3);
                    } else {
                        key(&mut h, Key::Enter, Modifiers::NONE);
                    }
                    assert!(h.state().grid.filter_menu.is_none());
                    h.state_mut().run("data.filterBy", json!({"column": "A", "clear": true})).unwrap();
                    let sh = h.state().session.active().unwrap().wb.active().unwrap();
                    let beta_hidden = values.contains(&"Beta") == edit;
                    assert_eq!(
                        (1..=4).map(|row| sh.is_row_hidden(row)).collect::<Vec<_>>(),
                        [false, beta_hidden, other_hidden, blank_hidden],
                        "table={table}, click={click}, edit={edit}, values={values:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn search_apply_excludes_hidden_nonmatches_and_keeps_hidden_matches() {
    for click in [false, true] {
        for beta in [false, true] {
            let mut session = Session::new();
            session.new_workbook();
            let mut values = vec![json!(["Company", "Job"]), json!(["East", "Alpha"])];
            if beta {
                values.push(json!(["East", "Beta"]));
            }
            values.extend([json!(["West", "Alphonse"]), json!(["West", "Other"]), json!(["West", null])]);
            let last = values.len() - 1;
            session.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
            session.execute("data.filter", json!({"range": format!("A1:B{}", last + 1)})).unwrap();
            session.execute("data.filterBy", json!({"column": "A", "values": ["East"]})).unwrap();
            session.execute("selection.set", json!({"cell": "B1"})).unwrap();
            let mut h = common::os_harness(session, egui::os::OperatingSystem::Windows);
            key(&mut h, Key::ArrowDown, Modifiers::ALT);
            assert!(h.query_by_label("Other").is_none());
            assert!(h.query_by_label("Alphonse").is_none());
            h.input_mut().events.push(Event::Text("Alph".into()));
            h.run_steps(2);
            assert_eq!(h.ctx.data(|d| d.get_temp::<String>(egui::Id::unique(("filter_search", 1u32)))).as_deref(), Some("Alph"));
            assert!(h.query_by_label("Alpha").is_some());
            assert!(h.query_by_label("Beta").is_none());
            if click {
                h.get_by_label("Apply").click();
                h.run_steps(3);
            } else {
                key(&mut h, Key::Enter, Modifiers::NONE);
            }
            assert!(h.state().grid.filter_menu.is_none());
            h.state_mut().run("data.filterBy", json!({"column": "A", "clear": true})).unwrap();
            let sh = h.state().session.active().unwrap().wb.active().unwrap();
            assert!(!sh.is_row_hidden(1), "Alpha must remain selected");
            assert!(!sh.is_row_hidden((last - 2) as u32), "hidden Alphonse must still match the search");
            assert!(sh.is_row_hidden((last - 1) as u32), "hidden Other must be excluded: click={click}, beta={beta}");
            assert!(sh.is_row_hidden(last as u32), "hidden blanks do not match the search");
            if beta {
                assert!(sh.is_row_hidden(2), "eligible Beta does not match the search");
            }
        }
    }
}

#[test]
fn dropdown_can_change_and_clear_a_column_while_another_filter_stays_active() {
    for table in [false, true] {
        let mut session = Session::new();
        session.new_workbook();
        session
            .execute(
                "range.setValues",
                json!({"range": "A1", "values": [
                    ["Company", "Job"], ["East", "Alpha"], ["East", "Beta"], ["West", "Other"], ["East", null]
                ]}),
            )
            .unwrap();
        if table {
            session.execute("insert.table", json!({"range": "A1:B5", "header": true})).unwrap();
        }
        session.execute("data.filterBy", json!({"column": "A", "values": ["East"]})).unwrap();
        session.execute("data.filterBy", json!({"column": "B", "values": ["Alpha"]})).unwrap();
        session.execute("selection.set", json!({"cell": "B1"})).unwrap();
        let screenshot = std::env::var("GRIDCRAFT_CASCADING_SNAPSHOT").ok().filter(|_| !table);
        let mut h = if screenshot.is_some() {
            common::harness_with(
                common::builder().with_os(egui::os::OperatingSystem::Windows).with_size(egui::vec2(1200.0, 800.0)).wgpu(),
                common::app(session),
                4,
            )
        } else {
            common::os_harness(session, egui::os::OperatingSystem::Windows)
        };
        key(&mut h, Key::ArrowDown, Modifiers::ALT);
        assert!(h.query_by_label("Alpha").is_some());
        assert!(h.query_by_label("Beta").is_some(), "current-column filtering must not remove alternatives");
        assert!(h.query_by_label("(Blanks)").is_some());
        assert!(h.query_by_label("Other").is_none(), "the other company's job must not be listed");
        if let Some(path) = screenshot {
            h.render().unwrap().save(path).unwrap();
        }
        h.get_by_label("Alpha").click();
        h.run_steps(2);
        h.get_by_label("Beta").click();
        h.run_steps(2);
        h.get_by_label("Apply").click();
        h.run_steps(3);
        let hidden = |h: &common::Harness| {
            let sh = h.state().session.active().unwrap().wb.active().unwrap();
            (1..=4).map(|row| sh.is_row_hidden(row)).collect::<Vec<_>>()
        };
        assert_eq!(hidden(&h), [true, false, true, true]);
        key(&mut h, Key::ArrowDown, Modifiers::ALT);
        h.get_by_label("Clear Filter").click();
        h.run_steps(3);
        assert_eq!(hidden(&h), [false, false, true, false], "clearing Job keeps Company filtered");
    }
}
