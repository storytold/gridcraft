//! One native window per workbook window: each window keeps its own interface state, and closing
//! or quitting asks about unsaved workbooks only when a workbook would close.
//!
//! The windows are drawn through a test backend that, like eframe, runs each immediate viewport
//! as a nested pass with its own input, so a close button can be pressed on one window.
#![cfg(not(target_arch = "wasm32"))]

use std::cell::RefCell;

use egui::{Context, RawInput, ViewportCommand, ViewportEvent, ViewportId, ViewportInfo};
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::doc_windows::DocWindows;
use gridcraft_ui_egui::{SaveAnswer, SheetApp};
use serde_json::json;

thread_local! {
    /// Each viewport's input for the next frame (focus, close button).
    static INFOS: RefCell<egui::ViewportIdMap<ViewportInfo>> = RefCell::new(egui::ViewportIdMap::default());
    /// The viewport commands the app sent this frame.
    static SENT: RefCell<Vec<(ViewportId, ViewportCommand)>> = const { RefCell::new(Vec::new()) };
}

fn record(mut out: egui::FullOutput) {
    out.textures_delta.clear(); // not painted
    SENT.with(|s| {
        let mut s = s.borrow_mut();
        for (id, v) in &out.viewport_output {
            s.extend(v.commands.iter().map(|c| (*id, c.clone())));
        }
    });
}

struct Desk {
    ctx: Context,
    app: SheetApp,
    windows: DocWindows,
}

impl Desk {
    fn new(quit_with_last_window: bool) -> Desk {
        INFOS.with(|i| i.borrow_mut().clear());
        let ctx = Context::default();
        ctx.set_embed_viewports(false);
        Context::set_immediate_viewport_renderer(|ctx, viewport| {
            let id = viewport.ids.this;
            let mut input = RawInput { viewport_id: id, ..RawInput::default() };
            input.viewports = INFOS.with(|i| i.borrow().clone());
            input.viewports.entry(id).or_default().parent = Some(viewport.ids.parent);
            let mut cb = viewport.viewport_ui_cb;
            record(ctx.run_ui(input, |ui| cb(ui)));
        });
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        let windows = DocWindows::new(&mut app, egui::ViewportBuilder::default(), quit_with_last_window);
        let mut desk = Desk { ctx, app, windows };
        desk.frames(3);
        desk
    }

    /// One frame as eframe runs it; `act` runs first, inside the frame. Returns the viewport
    /// commands sent.
    fn frame_with(&mut self, act: impl FnOnce(&mut SheetApp, &Context)) -> Vec<(ViewportId, ViewportCommand)> {
        SENT.with(|s| s.borrow_mut().clear());
        let mut input = RawInput { viewport_id: ViewportId::ROOT, ..RawInput::default() };
        input.viewports = INFOS.with(|i| i.borrow().clone());
        input.viewports.entry(ViewportId::ROOT).or_default();
        let Desk { ctx, app, windows } = self;
        let mut act = Some(act);
        let out = ctx.clone().run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            if let Some(act) = act.take() {
                act(app, &ctx);
            }
            app.logic(&ctx);
            windows.logic(app, &ctx);
            windows.show(app, ui);
        });
        record(out);
        // Close buttons are one-shot events.
        INFOS.with(|i| i.borrow_mut().values_mut().for_each(|v| v.events.clear()));
        SENT.with(|s| s.borrow().clone())
    }

    fn frames(&mut self, n: usize) -> Vec<(ViewportId, ViewportCommand)> {
        (0..n).flat_map(|_| self.frame_with(|_, _| {})).collect()
    }

    fn viewport(&self, window: u64) -> ViewportId {
        self.windows.viewport_of(window).unwrap_or_else(|| panic!("window {window} has no native window"))
    }

    /// The user presses window `window`'s close button.
    fn close(&mut self, window: u64) -> Vec<(ViewportId, ViewportCommand)> {
        let vp = self.viewport(window);
        INFOS.with(|i| i.borrow_mut().entry(vp).or_default().events.push(ViewportEvent::Close));
        let mut sent = self.frame_with(|_, _| {});
        sent.extend(self.frames(2));
        sent
    }

    /// Answers the Save Changes question showing in the active window, as its button does.
    fn answer(&mut self, answer: SaveAnswer) -> Vec<(ViewportId, ViewportCommand)> {
        let mut sent = self.frame_with(|app, ctx| {
            let d = app.dialog.take().unwrap_or_else(|| panic!("no question is showing"));
            assert_eq!(d.name(), "saveChanges");
            let quitting = d.values.get("quit").and_then(serde_json::Value::as_bool) == Some(true);
            app.answer_save_changes(answer, quitting, ctx);
        });
        sent.extend(self.frames(2));
        sent
    }

    fn window_of(&self, doc: usize) -> u64 {
        let uid = self.app.session.documents()[doc].uid;
        self.app.session.doc_window(uid).unwrap_or_default()
    }
}

fn sent_to(sent: &[(ViewportId, ViewportCommand)], vp: ViewportId, cmd: &ViewportCommand) -> bool {
    sent.iter().any(|(id, c)| *id == vp && c == cmd)
}

#[test]
fn each_window_keeps_its_own_selection_and_draft() {
    let mut desk = Desk::new(true);
    desk.app.run("file.new", json!({})).unwrap();
    desk.frames(3);
    let (w1, w2) = (desk.window_of(0), desk.window_of(1));
    assert_eq!(desk.windows.shown_windows(), [w1, w2], "a native window per workbook");
    assert_ne!(desk.viewport(w1), desk.viewport(w2));

    desk.app.enter_window(w1);
    desk.app.run("selection.set", json!({"range": "B2"})).unwrap();
    desk.app.begin_edit(Some("one".into()), false);
    desk.frames(2);
    desk.app.enter_window(w2);
    desk.app.run("selection.set", json!({"range": "D4"})).unwrap();
    desk.app.begin_edit(Some("two".into()), false);
    desk.frames(3);

    for (w, cell, text) in [(w1, "B2", "one"), (w2, "D4", "two")] {
        desk.app.enter_window(w);
        assert_eq!(desk.app.editor.as_ref().map(|ed| ed.text.as_str()), Some(text), "window {w} keeps its draft");
        assert!(desk.app.grid.rect.is_some(), "window {w} drew its own grid");
        assert_eq!(desk.app.session.doc().unwrap().selection.active, CellRef::parse(cell).unwrap(), "window {w} keeps its selection");
    }
    assert!(desk.app.session.documents().iter().all(|d| !d.is_dirty()), "drawing the other window commits no draft");
}

#[test]
fn closing_one_of_two_windows_on_a_workbook_does_not_ask() {
    let mut desk = Desk::new(true);
    desk.app.run("cell.set", json!({"cell": "A1", "input": "42"})).unwrap();
    desk.app.run("view.newWindow", json!({})).unwrap();
    desk.frames(3);
    let windows = desk.windows.shown_windows();
    assert_eq!(windows.len(), 2, "New Window opens a native window");
    let child = windows.iter().copied().find(|w| Some(*w) != desk.windows.root_window()).unwrap();

    let sent = desk.close(child);
    assert!(desk.app.dialog.is_none(), "the workbook stays open in its other window: nothing to ask");
    assert_eq!(desk.app.session.documents().len(), 1);
    assert_eq!(desk.app.session.windows().len(), 1);
    assert!(desk.app.session.documents()[0].is_dirty(), "its changes are kept");
    assert_eq!(desk.windows.shown_windows().len(), 1);
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "{sent:?}");
}

#[test]
fn closing_the_last_window_of_an_unsaved_workbook_asks_and_the_app_quits_after() {
    let mut desk = Desk::new(true);
    desk.app.run("cell.set", json!({"cell": "A1", "input": "42"})).unwrap();
    desk.frames(2);
    let w = desk.window_of(0);
    let sent = desk.close(w);
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::CancelClose), "the window stays open to ask: {sent:?}");
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    assert_eq!(desk.app.session.documents().len(), 1);

    let sent = desk.answer(SaveAnswer::Cancel);
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "Cancel keeps it open");
    assert_eq!(desk.app.session.documents().len(), 1);

    desk.close(w);
    let sent = desk.answer(SaveAnswer::DontSave);
    assert!(desk.app.session.documents().is_empty());
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "the last window closed: the app quits: {sent:?}");
    assert!(desk.app.allow_close);
}

#[test]
fn without_quitting_on_the_last_window_the_app_hides_and_new_shows_it_again() {
    let mut desk = Desk::new(false);
    let w = desk.window_of(0);
    let sent = desk.close(w);
    assert!(desk.app.session.documents().is_empty(), "a clean workbook closes without asking");
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Visible(false)), "{sent:?}");
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "the app keeps running: {sent:?}");
    assert!(desk.app.session.documents().is_empty(), "and doesn't make a workbook of its own");

    let sent = desk.frame_with(|app, _| app.run_or_alert("file.new", json!({})));
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Visible(true)), "{sent:?}");
    assert_eq!(desk.windows.root_window(), Some(desk.window_of(0)));
}

#[test]
fn quit_asks_about_each_unsaved_workbook_in_its_window_and_cancel_stops() {
    let mut desk = Desk::new(true);
    desk.app.run("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    desk.app.run("file.new", json!({})).unwrap();
    desk.app.run("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    desk.frames(3);
    let (w1, w2) = (desk.window_of(0), desk.window_of(1));
    assert_eq!(desk.app.session.active_window(), Some(w2));

    let sent = desk.frame_with(|app, ctx| app.request_quit(ctx));
    assert_eq!(desk.app.session.active_window(), Some(w1), "Book1's window comes forward to ask");
    assert!(sent_to(&sent, desk.viewport(w1), &ViewportCommand::Focus), "{sent:?}");
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    let sent = desk.answer(SaveAnswer::Cancel);
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "Cancel stops the quit: {sent:?}");
    assert_eq!(desk.app.session.documents().len(), 2);
    assert!(desk.app.dialog.is_none());

    desk.frame_with(|app, ctx| app.request_quit(ctx));
    let sent = desk.answer(SaveAnswer::DontSave);
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "Book2 is still unsaved: {sent:?}");
    assert_eq!(desk.app.session.active_window(), Some(w2), "then Book2's window asks");
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    let sent = desk.answer(SaveAnswer::DontSave);
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "both answered: the app quits: {sent:?}");
}

#[test]
fn the_root_adopts_the_remaining_window_with_its_workbook_and_state() {
    let mut desk = Desk::new(true);
    desk.app.run("file.new", json!({})).unwrap();
    desk.frames(3);
    let (w1, w2) = (desk.window_of(0), desk.window_of(1));
    assert_eq!(desk.windows.root_window(), Some(w1));
    let book2 = desk.app.session.documents()[1].uid;
    desk.app.enter_window(w2);
    desk.app.run("selection.set", json!({"range": "C3"})).unwrap();
    desk.app.begin_edit(Some("kept".into()), false);
    desk.frames(2);

    let sent = desk.close(w1);
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::CancelClose), "closing the root's window doesn't end the app: {sent:?}");
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "{sent:?}");
    assert_eq!(desk.windows.root_window(), Some(w2), "Book2's window moves into the root");
    assert_eq!(desk.windows.shown_windows(), [w2]);
    assert_eq!(desk.app.session.documents().len(), 1);
    assert_eq!(desk.app.session.doc().unwrap().uid, book2);
    assert_eq!(desk.app.editor.as_ref().map(|ed| ed.text.as_str()), Some("kept"), "its draft is still open");
    assert_eq!(desk.app.session.doc().unwrap().selection.active, CellRef::parse("C3").unwrap());
    assert_eq!(desk.app.session.doc().unwrap().wb.active().unwrap().value(CellRef::parse("C3").unwrap()), Value::Empty);
}

#[test]
fn menu_commands_follow_the_focused_window_and_leaving_a_window_enters_its_draft() {
    let mut desk = Desk::new(true);
    desk.app.run("file.new", json!({})).unwrap();
    desk.frames(3);
    let (w1, w2) = (desk.window_of(0), desk.window_of(1));
    desk.app.begin_edit(Some("typed".into()), false);
    desk.frames(2);
    assert_eq!(desk.app.session.active_window(), Some(w2));

    // The user clicks into Book1's window.
    let (v1, v2) = (desk.viewport(w1), desk.viewport(w2));
    INFOS.with(|i| {
        let mut i = i.borrow_mut();
        i.entry(v1).or_default().focused = Some(true);
        i.entry(v2).or_default().focused = Some(false);
    });
    desk.frames(2);
    assert_eq!(desk.app.session.active_window(), Some(w1), "commands from the menu bar go to the window focused last");
    assert!(desk.app.editor.is_none(), "Book1's window has no draft");
    let book2 = &desk.app.session.documents()[1];
    assert_eq!(book2.wb.active().unwrap().value(CellRef::new(0, 0)), Value::text("typed"), "the draft was entered in its own window");
    desk.app.run("cell.set", json!({"cell": "B1", "input": "menu"})).unwrap();
    assert_eq!(desk.app.session.documents()[0].wb.active().unwrap().value(CellRef::new(0, 1)), Value::text("menu"));
}

#[test]
fn feedback_from_opening_a_workbook_shows_in_its_new_window() {
    let mut desk = Desk::new(true);
    desk.app.run("cell.set", json!({"cell": "A1", "input": "kept"})).unwrap();
    desk.frames(2);
    let book1 = desk.window_of(0);
    let mut wb = gridcraft_engine::model::Workbook::new();
    wb.vba_project = Some(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1 fake OLE2 vbaProject".to_vec().into());
    let b64 = gridcraft_engine::io::base64_encode(&gridcraft_engine::io::save_bytes(&wb, "macros.xlsm").unwrap());
    desk.frame_with(|app, _| {
        // A script opening the file on the session, then the interface reporting on it.
        let r = app.session.run("file.open", json!({"name": "macros.xlsm", "base64": b64})).unwrap();
        app.show_open_warnings(&r);
        app.run_or_alert("no.such.command", json!({}));
    });
    desk.frames(2);
    let opened = desk.window_of(1);
    assert_eq!(desk.windows.shown_windows(), [book1, opened]);
    assert_eq!(desk.app.session.active_window(), Some(opened));
    assert_eq!(desk.app.notice.as_ref().map(|n| n.kind), Some(gridcraft_ui_egui::NoticeKind::Warning), "the macro notice shows in the opened window");
    assert!(desk.app.message.is_some(), "and so does the alert");
    desk.app.enter_window(book1);
    assert!(desk.app.notice.is_none() && desk.app.message.is_none(), "Book1's window shows neither");
}

/// Book1 with two sheets, saved, shown in two windows: window 1 on Sheet1 at B2, window 2 on
/// Sheet2 at D4. Returns the window ids.
fn two_windows_on_one_workbook(desk: &mut Desk) -> (u64, u64) {
    desk.app.run("home.insertSheet", json!({})).unwrap();
    let path = std::env::temp_dir().join(format!("gridcraft-two-windows-{}-{:?}.xlsx", std::process::id(), std::thread::current().id()));
    desk.app.run("file.save", json!({"path": path})).unwrap();
    let _ = std::fs::remove_file(&path);
    desk.app.run("sheet.activate", json!({"sheet": 0})).unwrap();
    desk.app.run("selection.set", json!({"range": "B2"})).unwrap();
    desk.frames(2);
    let w1 = desk.app.session.active_window().unwrap();
    desk.app.run("view.newWindow", json!({})).unwrap();
    desk.frames(3);
    let w2 = desk.app.session.active_window().unwrap();
    desk.app.run("sheet.activate", json!({"sheet": 1})).unwrap();
    desk.app.run("selection.set", json!({"range": "D4"})).unwrap();
    desk.frames(3);
    (w1, w2)
}

#[test]
fn two_windows_on_one_workbook_show_their_own_sheet_and_each_others_edits() {
    let mut desk = Desk::new(true);
    let (w1, w2) = two_windows_on_one_workbook(&mut desk);
    assert_eq!(desk.windows.shown_windows(), [w1, w2], "New Window opens a native window");
    assert_ne!(desk.viewport(w1), desk.viewport(w2));
    let titles: Vec<String> = [w1, w2].iter().map(|w| desk.app.session.window_title(*w).unwrap()).collect();
    assert!(titles[0].ends_with(".xlsx:1") && titles[1].ends_with(".xlsx:2"), "{titles:?}");

    // Drawing both windows every frame swaps their views without copying the workbook.
    let wb = desk.app.session.doc().unwrap().wb.clone();
    desk.frames(3);
    assert!(std::sync::Arc::ptr_eq(&wb, &desk.app.session.doc().unwrap().wb), "a window swap copied the workbook");
    assert!(!desk.app.session.doc().unwrap().is_dirty());
    for (w, sheet, cell) in [(w1, 0, "B2"), (w2, 1, "D4")] {
        desk.app.enter_window(w);
        let d = desk.app.session.doc().unwrap();
        assert_eq!((d.wb.active_sheet(), d.selection.active), (sheet, CellRef::parse(cell).unwrap()), "window {w}'s sheet and selection");
        assert!(desk.app.grid.rect.is_some(), "window {w} drew its grid");
    }

    // Window 1 refers to Sheet2!D4; window 2 types there; window 1 shows it on the next frame.
    desk.app.enter_window(w1);
    desk.app.run("cell.set", json!({"cell": "B2", "input": "=Sheet2!D4*2"})).unwrap();
    desk.frames(1);
    desk.app.enter_window(w2);
    desk.app.run("cell.set", json!({"input": "21"})).unwrap();
    desk.frames(1);
    desk.app.enter_window(w1);
    let d = desk.app.session.doc().unwrap();
    assert_eq!(d.wb.active_sheet(), 0);
    assert_eq!(d.wb.active().unwrap().value(CellRef::parse("B2").unwrap()), Value::Number(42.0));
}

#[test]
fn a_dialog_stays_with_its_window_across_a_switch() {
    let mut desk = Desk::new(true);
    let (w1, w2) = two_windows_on_one_workbook(&mut desk);
    desk.app.enter_window(w1);
    desk.app.open_dialog("zoom", json!({}));
    desk.frames(2);
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("zoom"));

    // From the menu bar.
    desk.app.run("view.switchWindows", json!({"window": w2})).unwrap();
    desk.frames(3);
    assert_eq!(desk.app.session.active_window(), Some(w2));
    assert!(desk.app.dialog.is_none(), "window 2 shows no dialog");

    // The user clicks into window 1, then back into window 2.
    for (w, other) in [(w1, w2), (w2, w1)] {
        let (v, o) = (desk.viewport(w), desk.viewport(other));
        INFOS.with(|i| {
            let mut i = i.borrow_mut();
            i.entry(v).or_default().focused = Some(true);
            i.entry(o).or_default().focused = Some(false);
        });
        desk.frames(2);
        assert_eq!(desk.app.session.active_window(), Some(w));
        let expected = if w == w1 { Some("zoom") } else { None };
        assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), expected, "window {w}'s dialog");
    }

    // A script switching windows while the dialog is open.
    desk.frame_with(|app, ctx| {
        let _ = gridcraft_ui_egui::control::handle(app, ctx, "engine.execute", &json!({"command": "window.activate", "params": {"window": w1}}));
    });
    desk.frames(2);
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("zoom"));
    desk.app.enter_window(w2);
    assert!(desk.app.dialog.is_none());
}

#[test]
fn arrange_all_lays_out_the_native_windows_active_first() {
    let mut desk = Desk::new(true);
    let (w1, w2) = two_windows_on_one_workbook(&mut desk);
    INFOS.with(|i| i.borrow_mut().entry(ViewportId::ROOT).or_default().monitor_size = Some(egui::vec2(1000.0, 800.0)));
    desk.app.run("view.arrangeAll", json!({"arrange": "vertical"})).unwrap();
    let sent = desk.frames(1);
    let at = |x: f32| ViewportCommand::OuterPosition(egui::pos2(x, 0.0));
    assert!(sent_to(&sent, desk.viewport(w2), &at(0.0)), "the active window goes left: {sent:?}");
    assert!(sent_to(&sent, desk.viewport(w1), &at(500.0)));
    assert!(sent_to(&sent, desk.viewport(w1), &ViewportCommand::InnerSize(egui::vec2(500.0, 800.0))));

    // Without parameters, as a script calls it: tiled.
    let r = desk.app.run("view.arrangeAll", json!({})).unwrap();
    assert_eq!(r["arrange"], "tiled");
    let tiled = gridcraft_ui_egui::doc_windows::arrange_rects("tiled", 3, craft_window::Rect::new(0.0, 0.0, 1000.0, 800.0));
    let r = |x, y, w, h| craft_window::Rect::new(x, y, w, h);
    assert_eq!(tiled, [r(0.0, 0.0, 500.0, 400.0), r(500.0, 0.0, 500.0, 400.0), r(0.0, 400.0, 1000.0, 400.0)]);
}
