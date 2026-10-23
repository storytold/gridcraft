//! Ribbon controls that report data, ask for their input, or drive the window list (#1797):
//! each shows what it did instead of doing nothing or raising an error, and the View tab with
//! its Window and Macros groups still fits the window at 1200 px and 1440 px.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

fn harness_at(width: f32) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(width, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

/// Wide enough that every tab is fully on screen: these tests are about what a click does, not
/// about the layout (the layout has its own test below).
fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    harness_at(2400.0)
}

fn control(h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

fn run(h: &mut egui_kittest::Harness<'static, SheetApp>, id: &str, params: Json) {
    h.state_mut().run(id, params).unwrap_or_else(|e| panic!("{id}: {e}"));
    h.run_steps(2);
}

fn tab(h: &mut egui_kittest::Harness<'static, SheetApp>, name: &str) {
    h.state_mut().ui.ribbon_tab = name.into();
    h.run_steps(3);
}

fn click(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(4);
}

fn dialog_name(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name.clone())
}

#[test]
fn controls_whose_command_only_reports_data_show_the_result() {
    let mut h = harness();
    for (tab_name, label, dialog) in [
        ("Home", "Add-ins", "addIns"),
        ("Data", "Queries & Connections", "queriesConnections"),
        ("Review", "Thesaurus", "thesaurus"),
        ("View", "Custom Views", "customViews"),
        ("View", "Switch Windows", "switchWindows"),
    ] {
        tab(&mut h, tab_name);
        click(&mut h, label);
        assert_eq!(dialog_name(&h).as_deref(), Some(dialog), "{tab_name} › {label} opens {dialog}");
        assert!(h.state().message.is_none(), "{tab_name} › {label} must not raise an error: {:?}", h.state().message);
        let shown = h.state().dialog.as_ref().and_then(|d| d.result.clone()).unwrap_or(Json::Null);
        assert!(!shown.is_null(), "{dialog} shows what {label} returned");
        control(&mut h, "ui.dialog.cancel", json!({}));
        h.run_steps(2);
    }
}

#[test]
fn the_windows_dialog_lists_the_open_windows() {
    let mut h = harness();
    tab(&mut h, "View");
    click(&mut h, "New Window");
    click(&mut h, "Switch Windows");
    assert_eq!(dialog_name(&h).as_deref(), Some("switchWindows"));
    let windows = h.state().dialog.as_ref().and_then(|d| d.result.clone()).unwrap_or(Json::Null);
    assert_eq!(windows.as_array().map(Vec::len), Some(2), "both windows are listed: {windows}");
    // Clicking the window that is not in front switches to it.
    let other =
        windows.as_array().and_then(|a| a.iter().find(|w| w["active"] == false)).and_then(|w| w["title"].as_str()).unwrap_or_default().to_string();
    assert!(!other.is_empty());
    click(&mut h, &other);
    assert!(h.state().dialog.is_none(), "switching closes the dialog");
    let titles: Vec<String> = h.state().session.documents().iter().map(|d| d.display_title()).collect();
    assert_eq!(h.state().session.active().map(|d| d.display_title()), Some(other), "the clicked window is in front: {titles:?}");
}

#[test]
fn thesaurus_looks_up_the_active_cells_word() {
    let mut h = harness();
    run(&mut h, "range.setValues", json!({"range": "A1", "values": [["hello"]]}));
    run(&mut h, "selection.set", json!({"cell": "A1"}));
    tab(&mut h, "Review");
    click(&mut h, "Thesaurus");
    let d = h.state().dialog.as_ref().expect("Thesaurus opens its dialog");
    assert_eq!(d.values["word"], "hello", "the active cell's word is filled in");
    assert!(h.state().message.is_none());
    click(&mut h, "Look Up");
    assert!(h.state().message.is_none());
    let result = h.state().dialog.as_ref().and_then(|d| d.result.clone()).unwrap_or(Json::Null);
    assert!(result.is_array(), "Look Up lists related words (or none): {result}");
}

#[test]
fn consolidate_opens_its_dialog_and_merges_the_named_sources() {
    let mut h = harness();
    for (name, numbers) in [("East", 10), ("West", 20)] {
        run(&mut h, "home.insertSheet", json!({}));
        run(&mut h, "sheet.rename", json!({"name": name}));
        run(&mut h, "range.setValues", json!({"range": "A1", "values": [[numbers, numbers + 1]]}));
    }
    run(&mut h, "sheet.activate", json!({"sheet": 0}));
    run(&mut h, "selection.set", json!({"cell": "A1"}));
    tab(&mut h, "Data");
    click(&mut h, "Consolidate");
    let d = h.state().dialog.as_ref().expect("Consolidate opens its dialog");
    assert_eq!(d.name, "consolidate");
    assert!(h.state().message.is_none(), "the button must not run the command without sources");
    control(&mut h, "ui.dialog.set", json!({"field": "sources", "value": "East!A1:B1;West!A1:B1"}));
    control(&mut h, "ui.dialog.confirm", json!({}));
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "OK closes the dialog");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    let sheet = h.state().session.active().and_then(|d| d.wb.active().cloned()).expect("active sheet");
    let at = |a: &str| sheet.value(CellRef::parse(a).unwrap()).display();
    assert_eq!((at("A1"), at("B1")), ("30".to_string(), "32".to_string()), "both sources are summed at the destination");
}

#[test]
fn pivot_chart_asks_for_the_report_instead_of_failing() {
    let mut h = harness();
    run(&mut h, "range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 10], ["West", 20]]}));
    tab(&mut h, "Insert");
    click(&mut h, "PivotChart");
    assert_eq!(dialog_name(&h).as_deref(), Some("pivotChart"), "a plain range asks which report to chart");
    assert!(h.state().message.is_none(), "no `pivot` error: {:?}", h.state().message);
    assert!(
        h.state().dialog.as_ref().is_some_and(|d| d.result.as_ref().and_then(Json::as_array).is_some()),
        "the dialog lists the workbook's PivotTables"
    );
    control(&mut h, "ui.dialog.cancel", json!({}));
    h.run_steps(2);

    // A PivotTable at the active cell is charted straight away, as before.
    run(&mut h, "insert.pivotTable", json!({"source": "Sheet1!A1:B3", "rows": ["Region"], "values": ["Sales"]}));
    let anchor = h.state().session.active().and_then(|d| d.wb.active().and_then(|sh| sh.pivots.first().map(|p| p.anchor)));
    let Some(anchor) = anchor else { panic!("the PivotTable was created") };
    run(&mut h, "selection.set", json!({"cell": anchor.a1()}));
    tab(&mut h, "Insert");
    click(&mut h, "PivotChart");
    assert!(h.state().dialog.is_none(), "inside a PivotTable the button charts it at once");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active().map(|sh| sh.charts.len())), Some(1));
}

/// Every control on the View tab ends inside the window with room to spare: the Window group's
/// seven controls and Macros used to push the last settings boxes off the right edge, and the
/// label widths follow the font (the Linux CI fonts measure ~7% wider than Windows'), so the
/// margin, not the exact metrics, is what this checks.
#[test]
fn the_view_tab_fits_at_1200_and_1440_pixels() {
    // 1160 stands in for a wider font: the tab must compact itself rather than scroll.
    for width in [1160.0, 1200.0, 1440.0] {
        let mut h = harness_at(width);
        tab(&mut h, "View");
        for label in [
            "Custom Views",
            "Ruler",
            "Gridlines",
            "Formula Bar",
            "Headings",
            "New Window",
            "Arrange All",
            "Freeze Panes",
            "Split",
            "Hide",
            "Unhide",
            "Switch Windows",
            "Display theme",
            "Animations",
            "Interface size",
            "Interface language",
            "Macros",
        ] {
            // The ribbon is the band above the sheet; a status-bar label of the same name is not it.
            let rect =
                h.query_all_by_label(label).map(|n| n.rect()).find(|r| r.min.y < 170.0).unwrap_or_else(|| panic!("{label} is on the View tab"));
            assert!(rect.max.x <= width - 24.0, "{label} ends at {:.0} px, leaving less than 24 px of a {width:.0} px window", rect.max.x);
            assert!(rect.max.y <= 164.0, "{label} hangs below the ribbon card, at {:.0} px", rect.max.y);
        }
    }
}

/// Insert ▸ Illustrations keeps one Checkbox button: the rebase once left a second one in the old
/// Cell Controls spot.
#[test]
fn the_insert_tab_has_one_checkbox_button() {
    let mut h = harness();
    tab(&mut h, "Insert");
    assert_eq!(h.query_all_by_label("Checkbox").count(), 1, "one Checkbox button, not two");
    // The button carries its own keytip badge (painted), so the table must resolve one for it.
    assert_eq!(
        gridcraft_ui_egui::keytips::keytip_table::control_badge("N", "insert.checkbox"),
        Some("NCB"),
        "the Checkbox button shows the badge for its direct command"
    );
    click(&mut h, "Checkbox");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}
