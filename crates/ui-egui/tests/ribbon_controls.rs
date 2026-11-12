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

/// The Consolidate dialog defaults its destination to the active cell, not the selection's start:
/// a B2 active cell in A1:B3 consolidates at B2 instead of overwriting A1.
#[test]
fn consolidate_defaults_its_destination_to_the_active_cell() {
    let mut h = harness();
    run(&mut h, "range.setValues", json!({"range": "A1:B3", "values": [[1, 2], [3, 4], [5, 6]]}));
    run(&mut h, "selection.set", json!({"range": "A1:B3", "active": "B2"}));
    tab(&mut h, "Data");
    click(&mut h, "Consolidate");
    let d = h.state().dialog.as_ref().expect("Consolidate opens its dialog");
    assert_eq!(
        d.values.get("destination").and_then(Json::as_str),
        Some("B2"),
        "the destination is the active cell: {:?}",
        d.values.get("destination")
    );
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
    let chart = h.state().session.active().and_then(|d| d.wb.active().and_then(|sh| sh.charts.first().map(|c| c.id)));
    assert_eq!(h.state().selected_chart(), chart, "the inserted PivotChart is selected, so Chart Design applies to it");
}

/// A report whose name reads as a cell (`A1`) is submitted by id, so the dialog charts it instead
/// of the engine parsing the name as a cell reference and failing.
#[test]
fn pivot_chart_dialog_targets_a_report_named_like_a_cell() {
    let mut h = harness();
    run(&mut h, "range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 10], ["West", 20]]}));
    run(
        &mut h,
        "insert.pivotTable",
        json!({"source": "Sheet1!A1:B3", "rows": ["Region"], "values": ["Sales"], "destination": "Sheet1!D1", "name": "A1"}),
    );
    run(&mut h, "selection.set", json!({"cell": "H2"}));
    tab(&mut h, "Insert");
    click(&mut h, "PivotChart");
    assert_eq!(dialog_name(&h).as_deref(), Some("pivotChart"), "outside the report the dialog asks which one to chart");
    assert!(
        h.state().dialog.as_ref().and_then(|d| d.values.get("pivot")).and_then(Json::as_u64).is_some(),
        "the selection is the report id, not its name: {:?}",
        h.state().dialog.as_ref().and_then(|d| d.values.get("pivot"))
    );
    h.get_by_label_contains("OK").click();
    h.run_steps(4);
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active().map(|sh| sh.charts.len())), Some(1));
}

/// Every control on the View tab ends inside the window with room to spare: the Window group's
/// seven controls and Macros used to push the last settings boxes off the right edge. The label
/// widths follow the font (Linux CI fonts measure ~7% wider than Windows'), so the check is a
/// margin budget, not an exact width; at 1200 the tab's compact layout leaves the larger one.
#[test]
fn the_view_tab_fits_at_1200_and_1440_pixels() {
    for width in [1200.0, 1440.0] {
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
            // 1200 uses the compact layout, which must leave room for the wider CI font metrics.
            let margin = if width <= 1200.0 { 40.0 } else { 24.0 };
            assert!(rect.max.x <= width - margin, "{label} ends at {:.0} px, leaving less than {margin:.0} px of a {width:.0} px window", rect.max.x);
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

/// Page Layout ▸ Arrange ▸ Selection Pane runs a UI command, so the button and the control
/// channel open the same pane and neither has to write view state directly.
#[test]
fn the_selection_pane_opens_through_its_ui_command() {
    let mut h = harness();
    tab(&mut h, "Page Layout");
    click(&mut h, "Selection Pane");
    assert_eq!(h.state().grid.pane.as_deref(), Some("selection"), "the button opens the pane");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    // The control channel (and so the CLI/MCP bridge) reaches the same command and toggles it.
    let r = control(&mut h, "command", json!({"command": "view.selectionPane"}));
    assert_eq!((r["ok"].as_bool(), r["result"]["on"].as_bool()), (Some(true), Some(false)), "the second call closes it");
    assert_eq!(h.state().grid.pane, None);
    let r = control(&mut h, "command", json!({"command": "view.selectionPane", "params": {"on": true}}));
    assert_eq!(r["result"]["on"], true);
    assert_eq!(h.state().grid.pane.as_deref(), Some("selection"));
}

/// A `view.selectionPane` request naming another window acts on that window: `run_in` follows the
/// engine to the target before dispatch, so the target window's pane opens (not the origin's),
/// and the origin still shows its own pane afterwards.
#[test]
fn selection_pane_commands_reach_the_named_window() {
    let mut h = harness();
    let w1 = h.state().session.active_window().expect("the harness opens a window");
    run(&mut h, "view.newWindow", json!({}));
    let w2 = h.state().session.active_window().expect("New Window opens a second window");
    assert_ne!(w1, w2, "the two windows have distinct ids");
    h.state_mut().enter_window(w1);
    h.run_steps(2);
    assert_eq!(h.state().grid.pane, None, "window 1 starts with no pane");
    // From window 1, open the pane in window 2: the target's UI state is what changes.
    let r = control(&mut h, "command", json!({"command": "view.selectionPane", "params": {"on": true}, "window": w2}));
    assert_eq!(r["result"]["on"], true, "the targeted call reports the pane open: {r}");
    h.state_mut().enter_window(w2);
    h.run_steps(2);
    assert_eq!(h.state().grid.pane.as_deref(), Some("selection"), "the target window shows the pane");
    h.state_mut().enter_window(w1);
    h.run_steps(2);
    assert_eq!(h.state().grid.pane, None, "the originating window keeps its own pane state");
}

/// An unfilled field reaches `run_or_alert` as `""` or `[]`; blank means absent, so the dialogs
/// open instead of the command failing on an empty PivotTable name or source list.
#[test]
fn blank_pivot_and_consolidate_inputs_open_their_dialogs() {
    let mut h = harness();
    for (command, params, dialog) in [
        ("insert.pivotChart", json!({"pivot": ""}), "pivotChart"),
        ("data.consolidate", json!({"sources": ""}), "consolidate"),
        ("data.consolidate", json!({"sources": []}), "consolidate"),
    ] {
        h.state_mut().run_or_alert(command, params);
        h.run_steps(3);
        assert_eq!(dialog_name(&h).as_deref(), Some(dialog), "{command} with a blank field asks for it");
        assert!(h.state().message.is_none(), "{command} must not raise an error: {:?}", h.state().message);
        control(&mut h, "ui.dialog.cancel", json!({}));
        h.run_steps(2);
    }
}

/// Automate ▸ Scripting Tools keeps the script recorder next to the macro controls: Record
/// Actions runs `automate.recordActions`, and the Macros button still opens the macros dialog.
#[test]
fn the_automate_tab_offers_record_actions_and_macros() {
    let mut h = harness();
    tab(&mut h, "Automate");
    click(&mut h, "Record Actions");
    assert!(h.state().message.is_none(), "Record Actions starts the recorder: {:?}", h.state().message);
    control(&mut h, "ui.dialog.cancel", json!({}));
    h.run_steps(2);
    click(&mut h, "Macros");
    assert_eq!(dialog_name(&h).as_deref(), Some("macros"), "Macros opens the macros dialog");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}

/// Commands the app implements itself rather than the engine (`SheetApp::run_ui_command`). A
/// ribbon control may name one of these; every other id has to be a registered engine command.
const UI_COMMANDS: &[&str] = &["app.language.set", "view.animations", "view.darkMode", "view.interfaceScale", "view.selectionPane", "view.theme"];

/// The command ids a tab's source passes to `act(app, "…")` and `control_keytip(…, "…")`. Ids
/// without a dot are dialog or menu keytip targets, not commands, and are left out.
fn command_ids(src: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("act(app, \"") {
        let tail = &rest[i + "act(app, \"".len()..];
        match tail.find('"') {
            Some(j) => ids.push(tail[..j].to_string()),
            None => break,
        }
        rest = tail;
    }
    let mut rest = src;
    while let Some(i) = rest.find("control_keytip(") {
        let tail = &rest[i..];
        let Some(close) = tail.find(");") else { break };
        let call = &tail[..close];
        if let Some(close_quote) = call.rfind('"')
            && let Some(open_quote) = call[..close_quote].rfind('"')
        {
            ids.push(call[open_quote + 1..close_quote].to_string());
        }
        rest = &tail[close..];
    }
    ids
}

/// Every control the ribbon tabs draw names a command that exists: an engine command, or one of
/// the UI commands the app implements itself. A placeholder button that resolves to nothing is
/// exactly the defect #1797 reported, so this reads the tab sources rather than a hand-kept list.
#[test]
fn every_ribbon_control_points_at_a_registered_command() {
    let sources = [
        ("home", include_str!("../src/ribbon/home.rs")),
        ("insert", include_str!("../src/ribbon/insert.rs")),
        ("page_layout", include_str!("../src/ribbon/page_layout.rs")),
        ("formulas", include_str!("../src/ribbon/formulas.rs")),
        ("data", include_str!("../src/ribbon/data.rs")),
        ("review", include_str!("../src/ribbon/review.rs")),
        ("view", include_str!("../src/ribbon/view.rs")),
        ("automate", include_str!("../src/ribbon/automate.rs")),
        ("contextual", include_str!("../src/ribbon/contextual.rs")),
    ];
    let mut missing = Vec::new();
    let mut checked = 0;
    for (file, src) in sources {
        for id in command_ids(src) {
            if !id.contains('.') || id.starts_with("dialog:") {
                continue;
            }
            checked += 1;
            if gridcraft_engine::find_command(&id).is_none() && !UI_COMMANDS.contains(&id.as_str()) {
                missing.push(format!("{file}: {id}"));
            }
        }
    }
    assert!(checked > 100, "the scan found only {checked} control ids; the source layout changed");
    assert!(missing.is_empty(), "ribbon controls with no registered command: {missing:#?}");
}

/// Insert › PivotTable › Recommended PivotTables asks which suggestion to apply instead of
/// throwing the list away: the dialog lists them, and OK inserts the picked one.
#[test]
fn recommended_pivot_tables_list_their_suggestions_and_apply_the_pick() {
    let mut h = harness();
    run(&mut h, "range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", 10], ["West", 20]]}));
    run(&mut h, "selection.set", json!({"range": "A1:B3"}));
    tab(&mut h, "Insert");
    click(&mut h, "PivotTable options");
    click(&mut h, "Recommended PivotTables");
    assert_eq!(dialog_name(&h).as_deref(), Some("recommendedPivotTables"), "the menu item asks which suggestion to insert");
    assert!(h.state().message.is_none(), "no missing-`apply` error: {:?}", h.state().message);
    let d = h.state().dialog.as_ref().expect("Recommended PivotTables opens its dialog");
    assert_eq!(d.values.get("apply").and_then(Json::as_u64), Some(0), "the first suggestion is preselected");
    assert!(
        d.result.as_ref().and_then(|r| r.get("suggestions")).and_then(Json::as_array).is_some_and(|a| a.len() > 1),
        "the dialog lists the suggestions: {:?}",
        d.result
    );
    control(&mut h, "ui.dialog.set", json!({"field": "apply", "value": 1}));
    control(&mut h, "ui.dialog.confirm", json!({}));
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "OK closes the dialog");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active().map(|sh| sh.pivots.len())), Some(1));
}

/// Page Layout › Background asks for its picture through the picture dialog, but confirms through
/// `pageLayout.background`: the picture is stored as the sheet's background (marked, at A1), not
/// as a floating picture that Background › Clear could not remove.
#[test]
fn background_confirms_through_the_background_command() {
    let mut h = harness();
    tab(&mut h, "Page Layout");
    click(&mut h, "Background");
    let d = h.state().dialog.as_ref().expect("Background asks for the picture");
    assert_eq!(d.name, "insertPicture");
    assert_eq!(d.command, Some("pageLayout.background"), "the dialog must not insert an unmarked floating picture");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    control(&mut h, "ui.dialog.set", json!({"field": "base64", "value": "bm90IGEgcGljdHVyZQ=="}));
    control(&mut h, "ui.dialog.confirm", json!({}));
    h.run_steps(3);
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    let sh = h.state().session.active().and_then(|d| d.wb.active().cloned()).expect("active sheet");
    assert_eq!(sh.images.len(), 1);
    assert_eq!(sh.images[0].alt, "Sheet background");
    assert_eq!(sh.images[0].anchor.cell, CellRef::parse("A1").unwrap());
}

/// Page Layout › Background also offers Clear Background: the command removes a saved background
/// only with `{"clear": true}`.
#[test]
fn background_menu_clears_the_saved_background() {
    let mut h = harness();
    run(&mut h, "pageLayout.background", json!({"base64": "bm90IGEgcGljdHVyZQ=="}));
    assert_eq!(h.state().session.active().and_then(|d| d.wb.active().map(|sh| sh.images.len())), Some(1));
    tab(&mut h, "Page Layout");
    click(&mut h, "Background options");
    click(&mut h, "Clear Background");
    assert_eq!(
        h.state().session.active().and_then(|d| d.wb.active().map(|sh| sh.images.len())),
        Some(0),
        "the menu action clears the saved background"
    );
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}
