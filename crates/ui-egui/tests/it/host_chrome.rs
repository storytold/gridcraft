//! A host that embeds the sheet in its own window can turn off the app's title bar and File tab.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Chrome, Services, SheetApp, backstage::Page};

fn harness(chrome: Chrome) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::prepare(SheetApp::new(session, Services::default()));
    app.chrome = chrome;
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

#[test]
fn the_title_bar_and_file_tab_are_drawn_by_default() {
    let h = harness(Chrome::default());
    assert!(h.query_by_label("AutoSave").is_some(), "the title bar is drawn");
    assert!(h.query_by_label("File").is_some(), "the File tab is drawn");
}

#[test]
fn a_host_can_leave_out_the_title_bar_and_the_file_tab() {
    let full = harness(Chrome::default());
    let h = harness(Chrome { title_bar: false, file_tab: false, ribbon: true });
    assert!(h.query_by_label("AutoSave").is_none(), "no title bar");
    assert!(h.query_by_label("File").is_none(), "no File tab");
    // The ribbon tabs are painted text, so measure the move with the first ribbon control.
    let (with_bar, without) = (full.get_by_label("Paste").rect(), h.get_by_label("Paste").rect());
    let moved = with_bar.min.y - without.min.y;
    assert!((25.0..=40.0).contains(&moved), "the ribbon moves up by the title bar's height (28 on macOS, 38 elsewhere), moved {moved}");
    assert_eq!(with_bar.min.x, without.min.x, "the ribbon's groups keep their place across");
}

#[test]
fn without_the_file_tab_the_backstage_view_never_opens() {
    let mut h = harness(Chrome { title_bar: true, file_tab: false, ribbon: true });
    h.state_mut().open_backstage(Page::Home);
    h.run_steps(3);
    assert!(h.state().backstage.is_none(), "opening Backstage directly is ignored");
    h.state_mut().open_backstage(Page::Print);
    h.run_steps(3);
    assert!(h.state().backstage.is_none(), "so is every page, Print and Save As included");
    assert!(h.query_by_label("AutoSave").is_some(), "the title bar can stay on its own");
}

#[test]
fn turning_the_file_tab_off_closes_an_open_backstage() {
    let mut h = harness(Chrome::default());
    h.state_mut().open_backstage(Page::Home);
    h.run_steps(3);
    assert!(h.state().backstage.is_some(), "Backstage opens with the File tab on");
    h.state_mut().chrome.file_tab = false;
    h.run_steps(3);
    assert!(h.state().backstage.is_none(), "and closes when the host turns the File tab off");
}

#[test]
fn a_host_can_leave_out_the_ribbon_and_keep_the_sheet() {
    let full = harness(Chrome::default());
    let h = harness(Chrome { ribbon: false, ..Chrome::default() });
    assert!(h.query_by_label("Paste").is_none(), "no ribbon groups");
    assert!(h.query_by_label("AutoSave").is_some(), "the title bar is a separate choice");
    // The grid moves up into the ribbon's place.
    let top = |h: &egui_kittest::Harness<'static, SheetApp>| h.state().grid.rect.map(|r| r.min.y).unwrap_or(f32::MAX);
    assert!(top(&h) + 60.0 < top(&full), "the grid starts higher without the ribbon: {} vs {}", top(&h), top(&full));
}

#[test]
fn without_the_ribbon_backstage_still_opens_for_print() {
    let mut h = harness(Chrome { ribbon: false, ..Chrome::default() });
    h.state_mut().open_backstage(Page::Print);
    h.run_steps(3);
    assert!(h.state().backstage.is_some(), "a host's Print button opens GridCraft's print page");
}

#[test]
fn the_control_channel_turns_the_ribbon_off_and_on() {
    let mut h = harness(Chrome::default());
    let ctx = h.ctx.clone();
    let reply = |h: &mut egui_kittest::Harness<'static, SheetApp>, on: bool| match gridcraft_ui_egui::control::handle(
        h.state_mut(),
        &ctx,
        "ui.set",
        &serde_json::json!({"ribbon": on}),
    ) {
        gridcraft_ui_egui::control::Outcome::Done(v) => v,
        gridcraft_ui_egui::control::Outcome::Screenshot { .. } => serde_json::Value::Null,
    };
    let off = reply(&mut h, false);
    h.run_steps(3);
    assert_eq!(off["result"]["ribbon"], serde_json::json!(false), "{off}");
    assert!(!h.state().chrome.ribbon && h.query_by_label("Paste").is_none());
    reply(&mut h, true);
    h.run_steps(3);
    assert!(h.query_by_label("Paste").is_some(), "and back");
}
