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

/// #2546 (macOS): AppKit moved, and on a double-click zoomed, the window for presses on the title
/// bar's own buttons. The window may move only from the bar's empty area: not while the pointer
/// is on Undo, Redo, Home or the other controls. A double-click on a button is two clicks on it,
/// never a maximize; one on the empty area still does what the system's setting says.
#[test]
fn title_bar_buttons_keep_their_clicks_from_the_window() {
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        {
            let mut session = Session::new();
            session.new_workbook();
            crate::common::prepare(SheetApp::new(session, Services::default()))
        },
    );
    h.run_steps(4);
    for v in [1, 2, 3] {
        h.state_mut().session.execute("cell.set", serde_json::json!({"ref": "A1", "value": v})).ok();
    }
    h.state_mut().session.execute("edit.undo", serde_json::json!({})).ok();
    h.run_steps(2);
    let maximized = |h: &egui_kittest::Harness<'static, SheetApp>| {
        h.output().viewport_output.values().any(|v| {
            v.commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Maximized(_) | egui::ViewportCommand::Minimized(_) | egui::ViewportCommand::StartDrag))
        })
    };
    let double_click = |h: &mut egui_kittest::Harness<'static, SheetApp>, at: egui::Pos2| {
        let mut seen = false;
        for _ in 0..2 {
            for pressed in [true, false] {
                h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
                h.step();
                seen |= maximized(h);
            }
        }
        h.run_steps(2);
        seen || maximized(h)
    };
    let undo_steps = |h: &egui_kittest::Harness<'static, SheetApp>| h.state().session.active().map(|d| d.undo.len()).unwrap_or_default();
    // The empty bar (left of AutoSave) moves the window and maximizes it on a double-click.
    let empty = egui::pos2(3.0, h.get_by_label("Undo").rect().center().y);
    h.event(egui::Event::PointerMoved(empty));
    h.run_steps(2);
    assert!(craft_window::title_bar::window_movable(&h.ctx), "the empty bar moves the window");
    // It follows the system's setting (zoom by default, or minimize); a Mac set to do nothing
    // sends no command.
    let acts = double_click(&mut h, empty);
    if craft_window::title_bar::action() != craft_window::title_bar::DoubleClick::Nothing {
        assert!(acts, "a double-click on the empty bar maximizes (or minimizes, as the system is set)");
    }
    for label in ["Undo", "Redo", "Home", "More commands"] {
        let at = h.get_by_label(label).rect().center();
        h.event(egui::Event::PointerMoved(at));
        h.run_steps(2);
        assert!(!craft_window::title_bar::window_movable(&h.ctx), "{label}: the window must not move from a press on it");
        let before = undo_steps(&h);
        assert!(!double_click(&mut h, at), "{label}: a double-click on it maximized the window");
        if label == "Undo" {
            assert_eq!(undo_steps(&h), before.saturating_sub(2), "two clicks on Undo undo twice");
        }
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
    }
    // A drag that starts on a button doesn't move the window either.
    let at = h.get_by_label("Redo").rect().center();
    let mut moved = false;
    h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.step();
    for dx in [4.0, 12.0, 40.0, 80.0] {
        h.event(egui::Event::PointerMoved(at + egui::vec2(dx, 0.0)));
        h.step();
        moved |= maximized(&h);
    }
    h.event(egui::Event::PointerButton {
        pos: at + egui::vec2(80.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    assert!(!moved && !maximized(&h), "a drag from Redo moved the window");
}

/// #2546: a host that hides the title bar while the pointer is on Undo must not leave the window
/// unmovable. The macOS sync runs after the whole UI, not only when the title bar is drawn.
#[test]
fn hiding_the_title_bar_makes_the_window_movable_again() {
    let mut h = harness(Chrome::default());
    let at = h.get_by_label("Undo").rect().center();
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(2);
    assert_eq!(gridcraft_ui_egui::ribbon::wanted_movable(&h.ctx), Some(false), "the pointer on Undo: the window must not move");
    h.state_mut().chrome.title_bar = false;
    h.run_steps(3);
    assert!(h.query_by_label("AutoSave").is_none(), "the title bar is gone");
    assert_eq!(gridcraft_ui_egui::ribbon::wanted_movable(&h.ctx), Some(true), "without the bar the window moves again");
    h.state_mut().chrome.title_bar = true;
    h.run_steps(3);
    assert_eq!(gridcraft_ui_egui::ribbon::wanted_movable(&h.ctx), Some(false), "the bar back under the pointer: Undo keeps its clicks");
}
