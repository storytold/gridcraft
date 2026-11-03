use super::clock::now_ms;
use super::services::{Chrome, Services};
use super::state::{NoticeKind, OpenWindow, SaveAnswer, SheetView};
use super::*;

mod clipboard;
mod commands;
mod editing;
mod files;
mod fonts;
mod input;
mod keys;
mod prefs;
mod print;
mod quit;
mod view;

impl SheetApp {
    /// Constructs a `SheetApp` with default UI, grid, printing, control and screenshot state and the caller's session and services.
    pub fn new(session: Session, services: Services) -> SheetApp {
        SheetApp {
            session,
            ui: UiState::default(),
            chrome: Chrome::default(),
            views: HashMap::new(),
            editor: None,
            text_box_editor: None,
            dialog: None,
            backstage: None,
            message: None,
            toast: None,
            notice: None,
            services,
            printing: Default::default(),
            print_feedback_seen: None,
            synthetic: Default::default(),
            synthetic_pending: false,
            synthetic_injected_at_pass: None,
            control_rx: None,
            grid: grid::GridState {
                last_border: "bottom".into(),
                last_fill: "#FFFF00".into(),
                last_font_color: "#C00000".into(),
                ..Default::default()
            },
            perf: Perf::default(),
            fonts_ready: false,
            fonts_set: false,
            prepared_fonts: false,
            fonts_bound: Vec::new(),
            fonts_seen: (0, 0),
            fonts_rebind: false,
            fonts_han: None,
            effective_dark: false,
            desktop_theme: desktop_theme::DesktopTheme::default(),
            desktop_dark: None,
            name_box: None,
            keytips: keytips::KeyTips::default(),
            shots: control::Shots::default(),
            capture_epoch: 0,
            capture_geometry: None,
            capture_geometry_from_raw: false,
            selected_chart: None,
            clipboard_text_at_copy: None,
            cut_text: None,
            tab_run: None,
            started: now_ms(),
            allow_close: false,
            open_switch_windows: false,
            ui_window: None,
            parked: HashMap::new(),
            native_windows: false,
            arrange_windows: None,
            #[cfg(not(target_arch = "wasm32"))]
            updates_rt: Default::default(),
        }
    }

    /// One-time context setup: start cataloging the machine's fonts, then install the fonts and
    /// visuals. Workbook-specific fonts are bound later, once the scan is done (see [`logic`]).
    ///
    /// [`logic`]: SheetApp::logic
    pub fn setup_context(ctx: &egui::Context, dark: bool) {
        crate::fonts::start_scan();
        Self::setup_context_for_language(ctx, dark, i18n::Language::En);
    }

    /// Sets up fonts and visuals for a specific persisted interface language.
    pub fn setup_context_for_language(ctx: &egui::Context, dark: bool, language: i18n::Language) {
        ctx.set_fonts(theme::font_definitions_for_language(language));
        theme::apply(ctx, dark);
    }

    /// Per-frame logic (control channel, screenshots). Call before `ui`.
    pub fn logic(&mut self, ctx: &egui::Context) {
        self.ensure_text_width(ctx);
        // A keytip control request lives one frame: the ribbon consumes it during `ui`, and
        // anything that never reached a control (collapsed ribbon, tab switch) is dropped.
        self.ui.keytip_invoke = None;
        // Floating pictures' textures go with their workbook, even when no sheet is painted.
        let open: Vec<u64> = self.session.documents().iter().map(|d| d.uid).collect();
        floating_pictures::forget_closed(ctx, &open);
        self.apply_font_state(ctx);
        if let Some(answer) = self.desktop_theme.take_change() {
            self.desktop_dark = Some(answer);
        }
        self.sync_theme_preference(ctx);
        self.sync_input_options(ctx);
        // The geometry captures are bound to. Native frames recorded it from their fresh
        // `RawInput` in `raw_input_hook` (including hidden logic-only ticks); the epoch is
        // monotonic, so a resize back to the original values is still a new epoch and old pixels
        // are never saved with new geometry.
        let capture_geometry = control::current_capture_geometry(self, ctx);
        control::note_capture_geometry(self, capture_geometry);
        // Drain before admission: completed or timed-out captures release their slots and
        // reservation first, so a new request is never refused for budget that is about to free.
        // Screenshot results, deadlines and geometry validation belong to `logic`; `ui` only
        // issues the capture command (after every widget, so a command cannot be missed).
        control::drain_screenshots(self, ctx);
        control::poll(self, ctx);
        // Printing feedback is polled once per frame, here, so hidden windows and logic-only
        // frames still see worker completion. `ui` and the desktop app no longer poll it.
        self.poll_printing(ctx);
        self.guard_window_close(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::tick(self, ctx);
        // Files read asynchronously (web file picker, dropped files).
        self.poll_async_opens();
        self.autosave(ctx);
        self.update_title(ctx);
        self.request_hidden_paint(ctx);
        // One window always shows a workbook, unless each workbook has a native window of its own
        // (`doc_windows` decides what having none left means).
        if self.session.active().is_none() && !self.native_windows {
            self.session.new_workbook();
        }
        self.follow_active_window();
    }

    /// Window title, next to the close/autosave state: `ViewportCommand::Title` is valid outside a
    /// pass and a control-channel rename while hidden updates it on return.
    fn update_title(&mut self, ctx: &egui::Context) {
        // Native windows get theirs from `doc_windows`.
        if self.native_windows {
            return;
        }
        let Some(d) = self.session.active() else { return };
        let title = format!("{}{}", d.display_title(), if d.is_dirty() { " — Edited" } else { "" });
        if self.grid.last_title.as_deref() != Some(title.as_str()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.grid.last_title = Some(title);
        }
    }

    /// A queued/settling capture with a capturable size and an unapplied synthetic step both need
    /// a real `ui` pass. Once a capture is issued, its `Screenshot` command's own `wants_paint`
    /// paints the hidden window; after that a full hidden pass every frame only wastes CPU. A
    /// minimized request has no size, so no hidden pass can reach issuance: the restore pass issues
    /// it instead. Native eframe honours `RequestPaintWhileHidden`; the web runner does not (hidden
    /// tabs only run `run_logic`), so a hidden web tab queues the work until it is shown again.
    /// This is a shared eframe web follow-up, not a GridCraft-local workaround, and the control
    /// protocol documents the limitation.
    fn request_hidden_paint(&self, ctx: &egui::Context) {
        if !(self.shots.needs_hidden_paint() || self.synthetic_pending || !self.synthetic.is_empty()) {
            return;
        }
        // The web runner has no `RequestPaintWhileHidden`; sending it there only logs an
        // unhandled command. A visible tab still gets the repaint below; a hidden tab keeps the
        // work queued (see the shared eframe follow-up in docs/egui-037-local-followups.md).
        #[cfg(not(target_arch = "wasm32"))]
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        ctx.request_repaint();
    }

    /// Lays out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.fonts_ready || self.fonts_rebind {
            ctx.request_repaint();
            return;
        }
        let t0 = now_ms();
        // A script may have switched windows since the last frame.
        self.follow_active_window();
        text_box::before_ui(self, &ctx);
        // Another window came forward without going through a command (which settles drafts
        // first): the draft goes into its own window instead of floating over this one. If its
        // workbook refuses it, its window comes back to the front with the draft open.
        if self.draft_elsewhere() {
            self.commit_edit(0, 0, false, false);
        }
        // The language the widgets translate with this frame (see `i18n::current`).
        i18n::set_current(&ctx, self.ui.language);
        #[cfg(not(target_arch = "wasm32"))]
        updates_ui::show(self, &ctx);
        let t = theme::Tokens::get(&ctx);
        // A host can turn the File tab off while Backstage is already open: close it.
        if !self.chrome.file_tab {
            self.backstage = None;
        }
        // Keytips opened the File view (Alt, F): its letters keep working there.
        if self.backstage.is_none() || self.keytips.prefix().is_some() {
            self.ribbon_keys(&ctx);
        }
        if self.chrome.title_bar {
            ribbon::title_bar(self, ui);
        }
        if self.backstage.is_some() {
            // File tab: the Backstage view replaces everything below the title bar.
            backstage::show(self, ui);
        } else {
            ribbon::show(self, ui);
            widgets::notice_bar(self, ui);
            if self.session.prefs.formula_bar {
                formula_bar::show(self, ui);
            }
            if self.ui.status_bar {
                tabs::status_bar(self, ui);
            }
            tabs::sheet_tabs(self, ui);
            pivot_pane::show(self, ui);
            panes::show(self, ui);
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.grid_bg)).show(ui, |ui| {
                let g0 = now_ms();
                grid::show(self, ui);
                self.perf.grid_ms = now_ms() - g0;
            });
        }
        dialogs::show(self, &ctx);
        widgets::message_box(self, &ctx);
        widgets::toast(self, &ctx);
        // Requests are admitted in `logic`, for the root viewport, so only its pass issues them.
        if ctx.viewport_id() == egui::ViewportId::ROOT {
            control::issue_screenshots(self, &ctx);
        }
        if !ctx.input(|i| i.focused) {
            // Nothing else to do.
        }
        let now = now_ms();
        self.perf.frame_ms = now - t0;
        if self.perf.last > 0.0 {
            let dt = now - self.perf.last;
            if dt > 0.0 {
                self.perf.fps = self.perf.fps * 0.9 + 100.0 / dt;
            }
        }
        self.perf.last = now;
        // The step is applied only after a completed UI pass. A discarded or repeated pass of the
        // same frame must not offer it again (grid sizing calls `ui` more than once per frame);
        // provenance is the injected pass number, so only a pass that missed input handling is
        // re-offered by `raw_input_hook`.
        // Synthetic input reaches the root viewport only (`raw_input_hook`), so its pass consumes it.
        if self.synthetic_pending && ctx.viewport_id() == egui::ViewportId::ROOT {
            self.synthetic.pop_front();
            self.synthetic_pending = false;
            self.synthetic_injected_at_pass = None;
        }
    }

    /// Current selection's top-left cell text as A1 (for the Name Box).
    pub fn name_box_text(&self) -> String {
        let Some(d) = self.session.active() else { return String::new() };
        let sel = &d.selection;
        let cur = sel.current();
        // A named range that exactly matches shows its name.
        let sh = d.wb.active();
        if let Some(sh) = sh {
            for n in &d.wb.names {
                let target = n.formula.replace('$', "");
                let (sheet, body) = match target.rsplit_once('!') {
                    Some((s, b)) => (Some(gridcraft_engine::core::unquote_sheet(s)), b.to_string()),
                    None => (None, target.clone()),
                };
                if sheet.as_deref().is_none_or(|s| s.eq_ignore_ascii_case(&sh.name)) && RangeRef::parse(&body) == Some(cur) {
                    return n.name.clone();
                }
            }
        }
        if self.grid.drag_select && !cur.is_single() {
            return format!("{}R x {}C", cur.height(), cur.width());
        }
        sel.active.a1()
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    /// Builds a key-A press or release event without modifiers.
    fn key_event(pressed: bool) -> egui::Event {
        egui::Event::Key { key: egui::Key::A, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    /// Returns a test app with a new workbook and default services.
    fn app() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        SheetApp::new(session, Services::default())
    }

    /// One headless pass; a test has no renderer for the texture atlas delta (0.37 panics when it
    /// is dropped unapplied).
    fn ui_pass(ctx: &egui::Context, raw: egui::RawInput, f: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let mut out = ctx.run_ui(raw, f);
        out.textures_delta.clear();
        out
    }

    #[test]
    /// Only one synthetic step is offered, is not duplicated by a hidden logic-only tick, and press/release remain distinct batches.
    fn a_hidden_logic_tick_keeps_one_unapplied_synthetic_step() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.synthetic.push_back(vec![key_event(true)]);
        app.synthetic.push_back(vec![key_event(false)]);

        let mut first = egui::RawInput::default();
        app.raw_input_hook(&ctx, &mut first);
        assert_eq!(first.events.len(), 1, "only one step is offered");
        assert!(app.synthetic_pending);
        assert_eq!(app.synthetic.len(), 2, "a logic-only frame does not consume the step");

        // eframe carries the unconsumed `RawInput` into the next pass, so the hook offers the step
        // only once; a hidden logic-only tick does not duplicate it or offer the release.
        let mut second = egui::RawInput::default();
        app.raw_input_hook(&ctx, &mut second);
        assert!(second.events.is_empty(), "an unapplied step is not injected a second time");
        assert_eq!(app.synthetic.len(), 2, "press and release stay distinct batches");
    }

    /// Returns raw input with a screen rect at the given size.
    fn raw_with_size(size: egui::Vec2) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() }
    }

    #[test]
    /// A font-gated early-returning pass does not lose the queued step; the hook offers it again and a later real pass applies it.
    fn a_font_gated_pass_offers_the_synthetic_step_again() {
        let ctx = egui::Context::default();
        ctx.set_fonts(theme::font_definitions());
        let mut app = app();
        app.fonts_ready = true;
        app.fonts_set = true;
        // Binds `session.text_width`, as eframe's first frame does.
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        app.synthetic.push_back(vec![key_event(true)]);

        // The font gate returns before input handling; `begin_pass` has still consumed the events.
        app.fonts_rebind = true;
        let mut raw = raw_with_size(egui::vec2(900.0, 700.0));
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(raw.events.len(), 1);
        let _ = ui_pass(&ctx, raw, |ui| app.ui(ui));
        assert!(app.synthetic_pending, "the early-returning pass did not apply the step");
        assert_eq!(app.synthetic.len(), 1, "the step is still queued");

        // A completed pass that did not apply it made the hook offer it again for a real pass.
        app.fonts_rebind = false;
        let mut raw = raw_with_size(egui::vec2(900.0, 700.0));
        app.raw_input_hook(&ctx, &mut raw);
        assert_eq!(raw.events.len(), 1, "the step is not lost across the font gate");
        let _ = ui_pass(&ctx, raw, |ui| app.ui(ui));
        assert!(!app.synthetic_pending);
        assert!(app.synthetic.is_empty(), "the real UI pass applied the step");
    }

    #[test]
    /// A language-driven font rebuild waits while a synthetic step is pending and proceeds once it is clear.
    fn a_pending_step_delays_a_font_change() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.fonts_ready = true;
        app.fonts_han = Some(theme::HanOrder::Chinese);
        app.ui.language = i18n::Language::Ja;
        app.synthetic.push_back(vec![key_event(true)]);
        app.synthetic_pending = true;
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert!(app.fonts_ready, "the font rebuild waits for the pass that applies the step");
        app.synthetic.clear();
        app.synthetic_pending = false;
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert!(!app.fonts_ready, "without pending input the language rebuild proceeds");
    }

    #[test]
    /// Logic ticks see fresh viewport and DPI information; resizing back to the original size still advances the capture epoch.
    fn a_hidden_logic_tick_sees_fresh_viewport_info_and_keeps_the_epoch_monotonic() {
        /// Runs one logic tick with the requested width and native scale.
        fn tick(ctx: &egui::Context, app: &mut SheetApp, width: f32, native_ppp: f32) {
            let mut raw =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 400.0))), ..Default::default() };
            if let Some(viewport) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
                viewport.inner_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 400.0)));
                viewport.native_pixels_per_point = Some(native_ppp);
            }
            let _ = ctx.run_logic(&raw, |ctx| app.logic(ctx));
        }
        let ctx = egui::Context::default();
        let mut app = app();
        tick(&ctx, &mut app, 800.0, 2.0);
        let first = app.capture_geometry.expect("the first logic tick records the geometry");
        assert_eq!(first.viewport, Some(egui::vec2(800.0, 400.0)));
        assert_eq!(first.pixels_per_point, 2.0, "native scale times the interface zoom");
        tick(&ctx, &mut app, 900.0, 2.0);
        let resized = app.capture_geometry.expect("geometry");
        assert_eq!(resized.viewport, Some(egui::vec2(900.0, 400.0)), "run_logic's ViewportInfo is the fresh size");
        let epoch = app.capture_epoch;
        tick(&ctx, &mut app, 800.0, 2.0);
        assert_eq!(app.capture_geometry.map(|g| g.viewport), Some(Some(egui::vec2(800.0, 400.0))));
        assert!(app.capture_epoch > epoch, "resizing back is still a new epoch: old pixels must not be saved");
    }

    #[test]
    /// A resize and a resize back both advance the capture epoch.
    fn the_capture_epoch_changes_even_when_the_geometry_returns() {
        /// Runs one UI pass at the requested width.
        fn tick(ctx: &egui::Context, app: &mut SheetApp, width: f32) {
            let raw =
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 400.0))), ..Default::default() };
            let _ = ui_pass(ctx, raw, |ui| app.logic(ui.ctx()));
        }
        let ctx = egui::Context::default();
        let mut app = app();
        tick(&ctx, &mut app, 800.0);
        let base = app.capture_epoch;
        tick(&ctx, &mut app, 900.0);
        let changed = app.capture_epoch;
        assert!(changed > base, "a resize increments the capture epoch");
        tick(&ctx, &mut app, 800.0);
        assert!(app.capture_epoch > changed, "resizing back is still a new epoch: old pixels must not be saved");
    }

    /// Extracts title viewport commands from a logic output.
    fn titles(out: &egui::LogicOutput) -> Vec<String> {
        out.viewport_commands
            .values()
            .flatten()
            .filter_map(|c| match c {
                egui::ViewportCommand::Title(title) => Some(title.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    /// A hidden logic tick saves a dirty file-backed workbook once and clears its dirty state.
    fn a_logic_only_tick_autosaves_a_dirty_workbook_once() {
        let ctx = egui::Context::default();
        let mut app = app();
        let dir = std::env::temp_dir().join(format!("gridcraft-logic-autosave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("autosave.xlsx");
        app.run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
        app.ui.autosave = true;
        app.session.active_mut().unwrap().path = Some(path.to_string_lossy().into_owned());
        app.grid.last_autosave = monotonic_ms() - 3000.0;

        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert!(path.exists(), "a hidden logic-only tick saves a dirty workbook with a file");
        assert!(!app.session.active().unwrap().is_dirty(), "the save clears the dirty state");
        let saved_at = app.grid.last_autosave;
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(app.grid.last_autosave, saved_at, "the same change is not saved a second time");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    /// The title is sent on the first tick and after a rename, but not on every unchanged frame.
    fn a_logic_only_tick_updates_the_title_once_per_change() {
        let ctx = egui::Context::default();
        let mut app = app();
        let first = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        let title = app.session.active().unwrap().display_title().to_string();
        let expected = format!("{title}{}", if app.session.active().unwrap().is_dirty() { " — Edited" } else { "" });
        assert_eq!(titles(&first), [expected], "the first logic tick titles the window");
        let second = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert!(titles(&second).is_empty(), "an unchanged title is not sent every frame");

        app.session.active_mut().unwrap().title = "Renamed".into();
        let third = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        let expected = format!("Renamed{}", if app.session.active().unwrap().is_dirty() { " — Edited" } else { "" });
        assert_eq!(titles(&third), [expected], "a rename while hidden updates the title");
    }

    #[test]
    /// A hidden tick surfaces a failed print job in the app message.
    fn a_logic_only_tick_polls_printing_feedback() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.printing.printers.push(crate::printing::Printer { name: "Queue".into(), label: String::new(), default: true });
        app.run("ui.printSettings", serde_json::json!({"printer": "Queue"})).unwrap();
        app.services.print_native = Some(Box::new(|_, _| {
            let (sender, receiver) = std::sync::mpsc::channel();
            sender.send(Err("printer is offline".into())).unwrap();
            receiver
        }));
        app.run("ui.printSubmit", serde_json::json!({})).unwrap();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(app.printing.message, Some(crate::printing::Feedback::Failed("printer is offline".into())));
        assert!(app.message.as_ref().is_some_and(|(_, text)| text.contains("printer is offline")), "logic surfaces the failure");
    }

    #[test]
    /// Pending synthetic input makes logic request a real hidden paint.
    fn a_pending_synthetic_step_requests_a_hidden_paint() {
        let ctx = egui::Context::default();
        let mut app = app();
        app.synthetic.push_back(vec![key_event(true)]);
        app.synthetic_pending = true;
        let out = ui_pass(&ctx, egui::RawInput::default(), |ui| app.logic(ui.ctx()));
        let painted = out
            .viewport_output
            .get(&ctx.viewport_id())
            .is_some_and(|output| output.commands.iter().any(|c| matches!(c, egui::ViewportCommand::RequestPaintWhileHidden)));
        assert!(painted, "logic must ask for a real UI pass while a synthetic step is unapplied");
    }
}
