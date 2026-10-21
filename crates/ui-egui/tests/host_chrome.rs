//! A host that embeds the sheet in its own window can turn off the app's title bar and File tab.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Chrome, Services, SheetApp, backstage::Page};

fn harness(chrome: Chrome) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.chrome = chrome;
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
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
    let h = harness(Chrome { title_bar: false, file_tab: false });
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
    let mut h = harness(Chrome { title_bar: true, file_tab: false });
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
