//! One native window per workbook window: each window keeps its own interface state, and closing
//! or quitting asks about unsaved workbooks only when a workbook would close.
//!
//! The windows are drawn through a test backend that, like eframe, runs each immediate viewport
//! as a nested pass with its own input, so a close button can be pressed on one window.
#![cfg(not(target_arch = "wasm32"))]

use std::cell::{Cell, RefCell};

use egui::{Context, RawInput, ViewportCommand, ViewportEvent, ViewportId, ViewportInfo};
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::doc_windows::DocWindows;
use gridcraft_ui_egui::{SaveAnswer, SheetApp};
use serde_json::json;

/// A window, its content line count, expansion flag, and remembered expanded height.
type BarState = (u64, usize, bool, usize);

thread_local! {
    /// All viewport paints use one deterministic, monotonically advancing clock.
    static PAINT_TIME: Cell<f64> = const { Cell::new(0.0) };
    /// A child viewport whose registered callback paints twice on the next driver frame.
    static EXTRA_PAINT: Cell<Option<ViewportId>> = const { Cell::new(None) };
    /// Each viewport's input for the next frame (focus, close button).
    static INFOS: RefCell<egui::ViewportIdMap<ViewportInfo>> = RefCell::new(egui::ViewportIdMap::default());
    /// The viewport commands the app sent this frame.
    static SENT: RefCell<Vec<(ViewportId, ViewportCommand)>> = const { RefCell::new(Vec::new()) };
    /// Input events for the root window's next frame (a key pressed in it).
    static EVENTS: RefCell<Vec<egui::Event>> = const { RefCell::new(Vec::new()) };
}

/// Builds each viewport's input through the same backend path, including extra paints.
fn viewport_input(id: ViewportId, parent: Option<ViewportId>) -> RawInput {
    let time = PAINT_TIME.with(|t| {
        let next = t.get() + 1.0 / 60.0;
        t.set(next);
        next
    });
    let mut input = RawInput {
        viewport_id: id,
        time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
        ..Default::default()
    };
    input.viewports = INFOS.with(|i| i.borrow().clone());
    input.viewports.entry(id).or_default().parent = parent;
    input
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
    /// Creates one workbook and installs the viewport backend with a fresh deterministic clock
    /// and no extra paints, then initializes its native-window mapping.
    fn new(quit_with_last_window: bool) -> Desk {
        INFOS.with(|i| i.borrow_mut().clear());
        PAINT_TIME.with(|t| t.set(0.0));
        EXTRA_PAINT.with(|p| p.set(None));
        let ctx = Context::default();
        ctx.set_embed_viewports(false);
        Context::set_immediate_viewport_renderer(|ctx, viewport| {
            let id = viewport.ids.this;
            let paints = if EXTRA_PAINT.with(|p| p.get()) == Some(id) { 2 } else { 1 };
            let mut cb = viewport.viewport_ui_cb;
            for _ in 0..paints {
                record(ctx.run_ui(viewport_input(id, Some(viewport.ids.parent)), |ui| cb(ui)));
            }
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
        let mut input = viewport_input(ViewportId::ROOT, None);
        input.events = EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()));
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

    /// Paints through DocWindows, with an extra callback paint for the selected child. The root
    /// owns the driver, so its extra paint is a full driver frame; observe each such frame.
    fn frame_with_extra_paint(&mut self, window: u64, mut observe: impl FnMut(&mut Desk)) {
        let vp = self.viewport(window);
        EXTRA_PAINT.with(|p| p.set((vp != ViewportId::ROOT).then_some(vp)));
        for _ in 0..if vp == ViewportId::ROOT { 2 } else { 1 } {
            self.frames(1);
            observe(self);
        }
        EXTRA_PAINT.with(|p| p.set(None));
    }

    /// Reads a window's actual grid position and verifies that the layout cache and manual
    /// state belong to its selected content, including collapsed multiline cells.
    fn bar_position(&mut self, state: BarState) -> (f32, f32) {
        let (window, lines, expanded, remembered) = state;
        self.app.enter_window(window);
        assert_eq!(self.app.ui.formula_bar_expanded, expanded, "window {window}'s expansion");
        assert_eq!(self.app.ui.formula_bar_lines, remembered, "window {window}'s remembered height");
        let layout = gridcraft_ui_egui::formula_bar::BarLayout::get(&self.ctx, Some(window));
        assert_eq!(layout.lines, lines, "window {window}'s cached content lines");
        (layout.height, self.app.grid.rect.unwrap().top())
    }

    /// Expected height from the selected content in automatic mode, or this window's manual
    /// height when expanded. The test viewport has room for every manual height used here.
    fn bar_target(&self, state: BarState) -> f32 {
        use gridcraft_ui_egui::formula_bar::{MAX_LINES, one_line};
        let (_, lines, expanded, remembered) = state;
        let shown = if expanded { remembered } else { lines.clamp(1, MAX_LINES) };
        let row = self.ctx.fonts_mut(|f| f.row_height(&gridcraft_ui_egui::theme::ui_font(13.0)));
        one_line(row) + (shown - 1) as f32 * row
    }

    /// Verifies settled heights and returns grid positions for comparison across root adoption.
    fn assert_bars(&mut self, states: &[BarState]) -> Vec<(f32, f32)> {
        states
            .iter()
            .map(|&state| {
                let position = self.bar_position(state);
                let target = self.bar_target(state);
                assert!((position.0 - target).abs() < 0.5, "window {}: {position:?}, target {target}", state.0);
                position
            })
            .collect()
    }

    /// Checks every driver frame for backwards movement, overshoot, and a grid movement that
    /// disagrees with the bar. Unchanged neighbors must remain stationary throughout the ease.
    fn check_bar_motion(&mut self, state: BarState, start: (f32, f32), previous: (f32, f32)) -> ((f32, f32), bool) {
        let current = self.bar_position(state);
        let target = self.bar_target(state);
        let target_top = start.1 + target - start.0;
        assert!(
            current.1 >= start.1.min(target_top) - 0.5 && current.1 <= start.1.max(target_top) + 0.5,
            "window {}: grid overshot {start:?} -> {target_top}: {current:?}",
            state.0
        );
        assert!(
            (current.1 - target_top).abs() <= (previous.1 - target_top).abs() + 0.05,
            "window {}: grid moved away from {target_top}: {previous:?} -> {current:?}",
            state.0
        );
        assert!(
            ((current.1 - start.1) - (current.0 - start.0)).abs() < 0.5,
            "window {}: grid and bar moved differently: {start:?} -> {current:?}",
            state.0
        );
        let settled = (current.0 - target).abs() < 0.01
            && (current.1 - target_top).abs() < 0.5
            && (current.0 - previous.0).abs() < 0.01
            && (current.1 - previous.1).abs() < 0.01;
        (current, settled)
    }

    /// Polls for three consecutive settled frames, with a bounded timeout and rotating extra
    /// paints. It checks transitions as they happen instead of sleeping a fixed frame count.
    fn settle_bars(&mut self, states: &[BarState]) {
        let start: Vec<_> = states
            .iter()
            .map(|state| {
                self.app.enter_window(state.0);
                let layout = gridcraft_ui_egui::formula_bar::BarLayout::get(&self.ctx, Some(state.0));
                (layout.height, self.app.grid.rect.unwrap().top())
            })
            .collect();
        let mut previous = start.clone();
        let mut stable = 0;
        for frame in 0..90 {
            self.frame_with_extra_paint(states[frame % states.len()].0, |desk| {
                let mut settled = true;
                for (i, &state) in states.iter().enumerate() {
                    let (position, at_target) = desk.check_bar_motion(state, start[i], previous[i]);
                    previous[i] = position;
                    settled &= at_target;
                }
                stable = if settled { stable + 1 } else { 0 };
            });
            if stable >= 3 {
                return;
            }
        }
        panic!("formula bars did not settle: {states:?}, positions {previous:?}");
    }

    fn viewport(&self, window: u64) -> ViewportId {
        self.windows.viewport_of(window).unwrap_or_else(|| panic!("window {window} has no native window"))
    }

    /// Presses the close button and renders three frames so any root adoption has repainted.
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

    /// The user presses `key` in the root window.
    fn key(&mut self, key: egui::Key) -> Vec<(ViewportId, ViewportCommand)> {
        EVENTS.with(|e| {
            e.borrow_mut().push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE })
        });
        let mut sent = self.frame_with(|_, _| {});
        sent.extend(self.frames(2));
        sent
    }

    /// Opens the step editor on a new macro in the active window and edits it, so the draft has
    /// unsaved changes.
    fn edited_new_macro(&mut self) {
        self.app.open_dialog("macroEditor", json!({}));
        self.frames(3);
        if let Some(ed) = self.app.macros.editor.as_mut() {
            ed.draft.steps.push(gridcraft_engine::model::Step::command("cell.set", json!({"cell": "A1", "input": "7"})));
            ed.dirty = true;
        }
        self.frames(2);
    }
}

fn sent_to(sent: &[(ViewportId, ViewportCommand)], vp: ViewportId, cmd: &ViewportCommand) -> bool {
    sent.iter().any(|(id, c)| *id == vp && c == cmd)
}

/// Checks every transition frame for independent content-driven heights under unequal repaint
/// schedules, then verifies that a repainted survivor keeps its geometry after root adoption.
#[test]
fn formula_bar_heights_settle_independently_in_multiple_windows() {
    use gridcraft_ui_egui::formula_bar::MAX_LINES;
    let mut desk = Desk::new(true);
    desk.ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    desk.app.use_prepared_fonts();
    for _ in 0..2 {
        desk.app.run("file.new", json!({})).unwrap();
        desk.frames(1); // Register the new viewport before observing its transitions.
    }
    let windows = [desk.window_of(0), desk.window_of(1), desk.window_of(2)];
    let mut states = windows.map(|w| (w, 1, false, MAX_LINES));
    desk.settle_bars(&states);
    for (window, text) in windows.into_iter().zip(["one\ntwo\nthree\nfour", "one", ""]) {
        desk.app.enter_window(window);
        desk.app.run("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    }
    states[0].1 = 4;
    desk.settle_bars(&states);
    for window in windows {
        desk.frame_with_extra_paint(window, |desk| {
            desk.assert_bars(&states);
        });
    }

    desk.app.enter_window(windows[1]);
    desk.app.run("cell.set", json!({"cell": "A1", "input": "one\ntwo"})).unwrap();
    states[1].1 = 2;
    desk.settle_bars(&states);
    let before = desk.assert_bars(&states[1..]);
    desk.app.enter_window(windows[0]);
    let d = desk.app.session.doc_mut().unwrap();
    d.saved = d.wb.clone();
    desk.close(windows[0]); // Includes three frames, not just a close event.
    assert_ne!(desk.windows.root_window(), Some(windows[0]));
    assert_eq!(desk.viewport(desk.windows.root_window().unwrap()), ViewportId::ROOT);
    assert_eq!(desk.assert_bars(&states[1..]), before, "root adoption changed a survivor's painted geometry");
    desk.settle_bars(&states[1..]);
}

/// Checks independent expansion and remembered heights, including automatic multiline height,
/// a second view of one workbook, root adoption, and fresh state after closing and reopening.
#[test]
fn formula_bar_expansion_and_remembered_height_belong_to_each_window() {
    use gridcraft_ui_egui::formula_bar::{BarLayout, MAX_LINES};
    let mut desk = Desk::new(true);
    desk.ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    desk.app.use_prepared_fonts();
    let first = desk.window_of(0);
    desk.app.run("view.expandFormulaBar", json!({"lines": 8})).unwrap();
    desk.app.run("file.new", json!({})).unwrap();
    let second = desk.window_of(1);
    assert!(!desk.app.ui.formula_bar_expanded, "a new workbook starts collapsed");
    assert_eq!(desk.app.ui.formula_bar_lines, MAX_LINES);
    desk.app.run("cell.set", json!({"cell":"A1", "input":"one\ntwo\nthree"})).unwrap();
    desk.app.run("view.expandFormulaBar", json!({"lines": 3})).unwrap();
    desk.app.run("view.expandFormulaBar", json!({"on": false})).unwrap();
    desk.frames(1); // Register the new viewport, then observe both windows' transitions.
    desk.settle_bars(&[(first, 1, true, 8), (second, 3, false, 3)]);

    desk.app.run("view.newWindow", json!({})).unwrap();
    let third = desk.app.session.active_window().unwrap();
    assert!(!desk.app.ui.formula_bar_expanded);
    assert_eq!(desk.app.ui.formula_bar_lines, MAX_LINES);
    desk.frames(1); // Register this view of the same multiline cell.
    desk.app.run("view.expandFormulaBar", json!({"lines": 6})).unwrap();
    desk.settle_bars(&[(first, 1, true, 8), (second, 3, false, 3), (third, 3, true, 6)]);

    desk.app.enter_window(second);
    assert_eq!(desk.app.run("view.expandFormulaBar", json!({})).unwrap(), json!({"on": true}));
    desk.app.enter_window(first);
    assert_eq!(desk.app.run("view.expandFormulaBar", json!({"lines": 1})).unwrap(), json!({"on": false, "lines": 8}));
    desk.app.enter_window(third);
    desk.app.run("view.expandFormulaBar", json!({"on": false})).unwrap();
    let survivors = [(second, 3, true, 3), (third, 3, false, 6)];
    desk.settle_bars(&[(first, 1, false, 8), survivors[0], survivors[1]]);
    let before = desk.assert_bars(&survivors);
    desk.close(first);
    assert_ne!(desk.windows.root_window(), Some(first));
    assert_eq!(desk.viewport(desk.windows.root_window().unwrap()), ViewportId::ROOT);
    assert_eq!(desk.assert_bars(&survivors), before, "root adoption changed a survivor's painted geometry");
    desk.settle_bars(&survivors);

    desk.close(second);
    desk.app.run("file.new", json!({})).unwrap();
    let fresh = desk.app.session.active_window().unwrap();
    assert!(![first, second, third].contains(&fresh));
    assert!(!desk.app.ui.formula_bar_expanded);
    assert_eq!(desk.app.ui.formula_bar_lines, MAX_LINES);
    assert_eq!(BarLayout::get(&desk.ctx, Some(fresh)), BarLayout::default(), "a new window inherited a closed window's layout");
    desk.frames(1);
    desk.settle_bars(&[(third, 3, false, 6), (fresh, 1, false, MAX_LINES)]);
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
fn a_quit_question_brings_its_window_to_the_front_from_the_background() {
    // A Dock quit reaches the app while it is behind other apps, hidden or minimized.
    let mut desk = Desk::new(true);
    desk.app.run("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    desk.frames(3);
    let w = desk.window_of(0);

    let sent = desk.frame_with(|app, ctx| app.request_quit(ctx));
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    for cmd in [ViewportCommand::Visible(true), ViewportCommand::Minimized(false), ViewportCommand::Focus] {
        assert!(sent_to(&sent, desk.viewport(w), &cmd), "{cmd:?} missing: {sent:?}");
    }
    let sent = desk.frames(2);
    assert!(!sent_to(&sent, desk.viewport(w), &ViewportCommand::Focus), "only once: {sent:?}");
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

#[test]
fn quit_with_the_macro_editor_open_goes_on_once_the_editor_has_closed() {
    let mut desk = Desk::new(false);
    desk.app.run("macro.save", json!({"macro": {"name": "Kept", "steps": []}})).unwrap();
    let d = desk.app.session.active_mut().unwrap();
    d.saved = d.wb.clone();
    desk.app.open_dialog("macroEditor", json!({"macro": "Kept"}));
    desk.frames(3);
    assert!(desk.app.macros.editor.is_some());
    // The editor has nothing unsaved, so it closes; then the quit goes on as the app's quit, not
    // as the editor window's close button (which would close only that workbook's window).
    let mut sent = desk.frame_with(|app, ctx| app.request_quit(ctx));
    sent.extend(desk.frames(2));
    assert!(desk.app.macros.editor.is_none());
    assert!(desk.app.allow_close, "the quit goes on: {sent:?}");
    assert!(sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close), "{sent:?}");
    assert_eq!(desk.app.session.documents().len(), 1, "the workbook was not closed on its own");
}

#[test]
fn quit_takes_over_a_workbook_close_waiting_on_the_macro_editor() {
    let mut desk = Desk::new(false);
    let w1 = desk.window_of(0);
    desk.app.open_dialog("macroEditor", json!({}));
    desk.frames(3);
    if let Some(ed) = desk.app.macros.editor.as_mut() {
        ed.draft.steps.push(gridcraft_engine::model::Step::command("cell.set", json!({"cell": "A1", "input": "7"})));
        ed.dirty = true;
    }
    desk.frames(2);
    // Book2 has unsaved changes; Book1, with the editor, is saved.
    desk.app.run("file.new", json!({})).unwrap();
    desk.app.run("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    desk.frames(3);
    let w2 = desk.window_of(1);
    desk.app.enter_window(w1);
    desk.frames(2);
    // File › Close on Book1 waits on the editor's question; then Quit takes over.
    let r = desk.app.run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], false);
    desk.frames(3);
    desk.frame_with(|app, ctx| app.request_quit(ctx));
    // Don't Save (the question's answer closes the editor without saving).
    if let Some(ed) = desk.app.macros.editor.as_mut() {
        ed.ask_close = false;
        ed.dirty = false;
        ed.close_requested = true;
    }
    desk.frames(3);
    assert!(desk.app.macros.editor.is_none());
    // The quit asks about Book2 in its window, as the quit's question, not as a File › Close.
    assert_eq!(desk.app.session.active_window(), Some(w2));
    let d = desk.app.dialog.as_ref().expect("Book2 is asked about");
    assert_eq!(d.name(), "saveChanges");
    assert_eq!(d.values.get("quit"), Some(&json!(true)), "{:?}", d.values);
    assert_eq!(desk.app.session.documents().len(), 2, "nothing closed before the answer");
}

#[test]
fn quit_with_a_draft_in_one_window_and_unsaved_cells_in_another_asks_in_order_and_cancel_stops_it() {
    let mut desk = Desk::new(false);
    // Book1 (the root window), saved, with an unsaved draft; Book2 with unsaved cells.
    let w1 = desk.window_of(0);
    desk.edited_new_macro();
    desk.app.run("file.new", json!({})).unwrap();
    desk.app.run("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    desk.frames(3);
    let w2 = desk.window_of(1);
    assert_eq!(desk.app.session.active_window(), Some(w2));
    // Quit from Book2's window: the editor's question comes first, in Book1's window.
    let mut sent = desk.frame_with(|app, ctx| app.request_quit(ctx));
    sent.extend(desk.frames(2));
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close) && !desk.app.allow_close, "{sent:?}");
    assert_eq!(desk.app.session.active_window(), Some(w1), "the editor's window comes forward");
    assert!(desk.app.macros.editor.as_ref().is_some_and(|ed| ed.ask_close && ed.quit), "the editor asks");
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("macroEditor"), "no workbook is asked about yet");
    // Don't Save (the question's answer closes the editor without saving).
    if let Some(ed) = desk.app.macros.editor.as_mut() {
        ed.ask_close = false;
        ed.dirty = false;
        ed.close_requested = true;
    }
    desk.frames(3);
    assert!(desk.app.macros.editor.is_none());
    // Book2 is asked about next, in its window, as the quit's question.
    assert_eq!(desk.app.session.active_window(), Some(w2), "the unsaved workbook's window comes forward");
    let d = desk.app.dialog.as_ref().expect("Book2 is asked about");
    assert_eq!(d.name(), "saveChanges");
    assert_eq!(d.values.get("title").and_then(serde_json::Value::as_str), Some("Book2"));
    assert_eq!(d.values.get("quit"), Some(&json!(true)), "{:?}", d.values);
    assert_eq!(desk.app.session.documents().len(), 2, "nothing closed before the answer");
    // Cancel: the quit stops with both windows and workbooks open; the discarded draft stays
    // discarded.
    let sent = desk.answer(SaveAnswer::Cancel);
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close) && !desk.app.allow_close, "{sent:?}");
    assert_eq!(desk.app.session.documents().len(), 2);
    assert_eq!(desk.windows.shown_windows(), [w1, w2]);
    assert!(desk.app.dialog.is_none() && desk.app.macros.editor.is_none());
    assert!(desk.app.session.documents()[1].is_dirty(), "Book2 keeps its changes");
    // A quit cancelled at the editor's question (Escape) stops before any workbook is asked about.
    desk.app.enter_window(w1);
    desk.edited_new_macro();
    let mut sent = desk.frame_with(|app, ctx| app.request_quit(ctx));
    sent.extend(desk.frames(2));
    assert!(desk.app.macros.editor.as_ref().is_some_and(|ed| ed.ask_close && ed.quit), "the editor asks");
    sent.extend(desk.key(egui::Key::Escape));
    assert!(!sent_to(&sent, ViewportId::ROOT, &ViewportCommand::Close) && !desk.app.allow_close, "{sent:?}");
    assert!(desk.app.macros.editor.as_ref().is_some_and(|ed| !ed.ask_close && !ed.quit && ed.draft.steps.len() == 1), "the draft is kept");
    assert_eq!(desk.app.dialog.as_ref().map(|d| d.name()), Some("macroEditor"), "no Save Changes question followed");
    assert_eq!(desk.app.session.documents().len(), 2);
    assert_eq!(desk.windows.shown_windows(), [w1, w2]);
}
