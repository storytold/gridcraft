//! Task pane labels follow the UI language while commands and workbook content stay stable.

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(language: Language, pane: &str) -> Harness {
    let mut h = crate::support::blank_harness(4);
    h.state_mut().ui.language = language;
    h.state_mut().grid.pane = Some(pane.into());
    h.run_steps(3);
    h
}

#[test]
fn comments_translate_empty_state_and_resolve_actions_without_translating_content() {
    for lang in [Language::Tr, Language::En] {
        let mut h = harness(lang, "comments");
        assert!(h.query_by_label(lang.tr("No comments on this sheet yet. Select a cell and choose New Comment.")).is_some());
        h.state_mut().run("review.newComment", json!({"cell": "A1", "text": "Keep this comment", "author": "Workbook Author"})).unwrap();
        h.run_steps(3);
        for label in ["Keep this comment", "Workbook Author", lang.tr("Send")] {
            assert!(h.query_all_by_label_contains(label).next().is_some(), "missing {label}");
        }
        // The hint is placeholder metadata, not an AccessKit label. Exercise the reply field.
        let field = h.get_all_by_role(Role::TextInput).find(|node| node.rect().left() > 850.0).unwrap();
        assert_eq!(field.accesskit_node().placeholder(), Some(lang.tr("Reply…")));
        field.click();
        h.run_steps(2);
        h.input_mut().events.push(egui::Event::Text("Keep this reply".into()));
        h.run_steps(2);
        h.get_by_role_and_label(Role::Button, lang.tr("Send")).click();
        h.run_steps(3);
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().comments.values().next().unwrap().replies[0].1, "Keep this reply");
        h.get_by_role_and_label(Role::Button, lang.tr("Resolve")).click();
        h.run_steps(3);
        assert!(h.query_by_label(lang.tr("Resolved")).is_some());
        assert!(h.state().session.active().unwrap().wb.active().unwrap().comments.values().next().unwrap().resolved);
        h.get_by_role_and_label(Role::Button, lang.tr("Reopen")).click();
        h.run_steps(3);
        assert!(!h.state().session.active().unwrap().wb.active().unwrap().comments.values().next().unwrap().resolved);
        assert!(h.query_by_label("Keep this comment").is_some());
    }
}

#[test]
fn watch_translates_headers_and_adds_the_original_cell_reference() {
    for lang in [Language::Tr, Language::En] {
        let mut h = harness(lang, "watch");
        h.state_mut().run("cell.set", json!({"cell": "A1", "input": "=1+2"})).unwrap();
        for key in ["Watch Window", "Cell", "Value", "Formula", "Add Watch (active cell)"] {
            assert!(h.query_all_by_label(lang.tr(key)).next().is_some(), "missing {key}");
        }
        h.get_by_role_and_label(Role::Button, lang.tr("Add Watch (active cell)")).click();
        h.run_steps(3);
        let list = h.state_mut().session.run("formulas.watchWindow", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert!(list[0]["cell"].as_str().unwrap().ends_with("!A1"));
        assert_eq!(list[0]["formula"], "=1+2");
    }
}

#[test]
fn selection_and_chart_empty_states_translate() {
    for lang in [Language::Tr, Language::En] {
        let mut h = harness(lang, "selection");
        assert!(h.query_by_label(lang.tr("Selection")).is_some());
        assert!(h.query_by_label(lang.tr("There are no shapes, pictures or charts on this sheet.")).is_some());
        h.state_mut().grid.pane = Some("formatChart".into());
        h.run_steps(3);
        assert!(h.query_by_label(lang.tr("Format Chart Area")).is_some());
        assert!(h.query_by_label(lang.tr("Select a chart to format it.")).is_some());
        h.state_mut().selected_chart = Some(u32::MAX);
        h.run_steps(3);
        assert!(h.query_by_label(lang.tr("The chart no longer exists.")).is_some());
    }
}

#[test]
fn chart_labels_translate_and_commands_keep_their_original_values() {
    for lang in [Language::Tr, Language::En] {
        let mut h = harness(lang, "formatChart");
        h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["Name", "Revenue"], ["Q1", 10], ["Q2", 20]]})).unwrap();
        let id =
            h.state_mut().run("insert.chart", json!({"range": "A1:B3", "type": "column", "at": "E3"})).unwrap()["chart"].as_u64().unwrap() as u32;
        h.state_mut().selected_chart = Some(id);
        h.run_steps(3);
        for key in ["Chart Title", "Chart Type", "Legend", "Elements", "Series", "Colors & Layout", "Size"] {
            assert!(h.query_by_role_and_label(Role::DisclosureTriangle, lang.tr(key)).is_some(), "missing {key}");
        }
        h.get_by_role_and_label(Role::DisclosureTriangle, lang.tr("Chart Type")).click();
        h.run_steps(3);
        h.get_by_role_and_label(Role::Button, lang.tr("Stacked Bar")).click();
        h.run_steps(3);
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().charts[0].kind, gridcraft_engine::model::ChartKind::BarStacked);
        assert!(
            h.state()
                .session
                .journal
                .iter()
                .any(|(command, params)| command == "chart.set" && params["type"] == "bar" && params["subtype"] == "stacked")
        );
        h.get_by_role_and_label(Role::DisclosureTriangle, lang.tr("Chart Type")).click();
        h.run_steps(3);
        h.get_by_role_and_label(Role::DisclosureTriangle, lang.tr("Colors & Layout")).click();
        h.run_steps(3);
        h.get_by_role_and_label(Role::Button, lang.tr("accent2")).click();
        h.run_steps(3);
        h.get_by_role_and_label(Role::Button, &lang.tr("Layout {n}").replace("{n}", "2")).click();
        h.run_steps(3);
        assert!(h.state().session.journal.iter().any(|(command, params)| command == "chart.changeColors" && params["palette"] == "accent2"));
        assert!(h.state().session.journal.iter().any(|(command, params)| command == "chart.quickLayout" && params["layout"] == 2));
        // Switching language must keep the same collapsing section open.
        h.state_mut().ui.language = if lang == Language::Tr { Language::En } else { Language::Tr };
        h.run_steps(3);
        assert!(h.query_by_role_and_label(Role::Button, h.state().ui.language.tr("accent2")).is_some());
    }
}

#[test]
fn chart_series_translates_generated_names_and_preserves_explicit_series1() {
    for lang in [Language::Tr, Language::En] {
        for (values, expected, unnamed) in
            [(json!([[10], [20]]), lang.tr("Series{n}").replace("{n}", "1"), true), (json!([["Series1"], [10], [20]]), "Series1".to_string(), false)]
        {
            let mut h = harness(lang, "formatChart");
            h.state_mut().run("range.setValues", json!({"range": "A1", "values": values})).unwrap();
            let range = if unnamed { "A1:A2" } else { "A1:A3" };
            let id =
                h.state_mut().run("insert.chart", json!({"range": range, "type": "column", "at": "E3"})).unwrap()["chart"].as_u64().unwrap() as u32;
            h.state_mut().selected_chart = Some(id);
            h.run_steps(3);
            let name = &h.state().session.active().unwrap().wb.active().unwrap().charts[0].series[0].name;
            assert_eq!(name.is_none(), unnamed);
            h.get_by_role_and_label(Role::DisclosureTriangle, lang.tr("Series")).click();
            h.run_steps(3);
            assert!(h.query_all_by_label(&expected).any(|node| node.rect().left() > 850.0), "missing series label {expected}");
        }
    }
}
