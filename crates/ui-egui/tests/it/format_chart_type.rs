//! The Format Chart Area pane's Chart Type section changes the chart's type and marks the current one.

use egui::accesskit::{Role, Toggled};
use egui_kittest::kittest::{NodeT, Queryable};
use gridcraft_engine::Session;
use gridcraft_engine::model::ChartKind;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A two-series chart made with `insert.chart`'s `type` `t` and `subtype` `sub`, with Format Chart Area open on its Chart Type section.
fn open_chart_type(t: &str, sub: &str) -> (Harness, u32) {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Quarter", "North", "South"], ["Q1", 10, 30], ["Q2", 30, 10]]})).unwrap();
    session.execute("selection.set", json!({"range": "A1:C3"})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": t, "subtype": sub, "at": "E3"})).unwrap();
    let id = chart["chart"].as_u64().unwrap() as u32;
    let mut h = crate::support::harness(session, 4);
    h.state_mut().selected_chart = Some(id);
    h.state_mut().grid.pane = Some("formatChart".into());
    h.run_steps(2);
    h.get_by_role_and_label(Role::DisclosureTriangle, "Chart Type").click();
    h.run_steps(2);
    (h, id)
}

fn kind(h: &Harness, id: u32) -> ChartKind {
    h.state().session.active().unwrap().wb.active().unwrap().charts.iter().find(|c| c.id == id).unwrap().kind
}

/// Whether the pane's `label` button is drawn as the chart's current type. The pane sits on the
/// right, past the Chart Type header's left edge; the ribbon's buttons are elsewhere.
fn marked(h: &Harness, label: &str) -> bool {
    let left = h.get_by_role_and_label(Role::DisclosureTriangle, "Chart Type").rect().left();
    let button = h.get_all_by_role_and_label(Role::Button, label).find(|n| n.rect().left() >= left - 1.0).unwrap();
    button.accesskit_node().toggled() == Some(Toggled::True)
}

fn click(h: &mut Harness, label: &str) {
    let left = h.get_by_role_and_label(Role::DisclosureTriangle, "Chart Type").rect().left();
    h.get_all_by_role_and_label(Role::Button, label).find(|n| n.rect().left() >= left - 1.0).unwrap().click();
    h.run_steps(2);
}

#[test]
fn chart_type_offers_the_stacked_and_hundred_percent_stacked_types() {
    let (mut h, id) = open_chart_type("column", "");
    for (label, want) in [
        ("Stacked Column", ChartKind::ColumnStacked),
        ("100% Stacked Column", ChartKind::ColumnStacked100),
        ("Stacked Bar", ChartKind::BarStacked),
        ("100% Stacked Bar", ChartKind::BarStacked100),
        ("Stacked Line", ChartKind::LineStacked),
        ("Stacked Area", ChartKind::AreaStacked),
        ("100% Stacked Area", ChartKind::AreaStacked100),
        ("Column", ChartKind::ColumnClustered),
    ] {
        click(&mut h, label);
        assert_eq!(kind(&h, id), want, "{label} changes the chart's type");
    }
}

#[test]
fn chart_type_marks_the_chart_s_current_type() {
    for (t, sub, label) in
        [("bar", "stacked100", "100% Stacked Bar"), ("area", "stacked", "Stacked Area"), ("column", "", "Column"), ("line", "markers", "Line")]
    {
        let (h, _) = open_chart_type(t, sub);
        for other in [
            "Column",
            "Stacked Column",
            "100% Stacked Column",
            "Bar",
            "Stacked Bar",
            "100% Stacked Bar",
            "Line",
            "Stacked Line",
            "Area",
            "Stacked Area",
            "100% Stacked Area",
            "Pie",
            "Doughnut",
            "Scatter",
            "Radar",
        ] {
            assert_eq!(marked(&h, other), other == label, "{other} on a {t} {sub} chart");
        }
    }
}
