use super::controls::{act, control_keytip};
use super::{Color32, Icon, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, icon_button, json, pos2, small_button, theme, theme_choices, vec2};

/// macOS draws the window into the title bar (`with_fullsize_content_view`) and AppKit paints the
/// traffic lights itself, centred in its standard 28 pt title-bar band. Moving them would take
/// AppKit calls (`unsafe`, forbidden here), so on macOS the title bar is that band: the lights sit
/// vertically centred in it, with the controls beside them (#170).
const MAC: bool = cfg!(target_os = "macos");

pub const MAC_TITLE_BAR: f32 = 28.0;

/// The traffic lights as AppKit lays them out: button diameter, centre-to-centre pitch and the
/// first button's left edge (8–10 pt depending on the macOS version; the larger value keeps the
/// gap after them from shrinking).
const MAC_LIGHT: f32 = 12.0;

const MAC_LIGHT_PITCH: f32 = 20.0;

const MAC_LIGHTS_LEFT: f32 = 10.0;

/// Where title-bar content starts on macOS: the end of the three lights plus the gap the lights
/// keep from the top of the window.
pub const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;

/// Leave room for the macOS traffic lights when the content extends into the title bar.
pub const TITLE_LEFT_PAD: f32 = if MAC { MAC_CONTENT_LEFT } else { 8.0 };

pub(super) const TITLE_BAR_H: f32 = if MAC { MAC_TITLE_BAR } else { 38.0 };

/// The tallest title-bar controls, smaller on macOS to keep a margin in the shorter bar.
pub(super) const TITLE_CONTROL_H: f32 = if MAC { 22.0 } else { 26.0 };

/// Draws the title bar: drag/maximize, AutoSave toggle, Home/Save/Undo/Redo/More, search, share, comments and theme controls, with a centered truncated title.
pub fn title_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ch = TITLE_CONTROL_H;
    egui::Panel::top("title_bar").exact_size(TITLE_BAR_H).frame(egui::Frame::NONE.fill(t.window)).show(ui, |ui| {
        let rect = ui.max_rect();
        // The empty title area moves the window when dragged and follows the system's title-bar
        // double-click setting. Its controls keep their own clicks: on macOS AppKit moved and
        // zoomed the window for presses on them (a double-click on Undo or Redo zoomed it,
        // #2546), so the window is not movable while the pointer is on one (`sync_movable`).
        // In the browser there is no window to move or maximize.
        #[cfg(not(target_arch = "wasm32"))]
        craft_window::title_bar::empty_area(ui, rect);
        ui.horizontal_centered(|ui| {
            ui.add_space(TITLE_LEFT_PAD);
            // AutoSave toggle (saves after each change when the workbook has a file).
            ui.label(egui::RichText::new(app.ui.language.tr("AutoSave")).font(theme::ui_font(12.5)).color(t.text_dim));
            let on = app.ui.autosave;
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::click());
            ui.painter().rect_filled(r, 8.0, if on { t.accent } else { Color32::TRANSPARENT });
            ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, if on { t.accent } else { t.text_dim }), StrokeKind::Inside);
            let knob = if on { pos2(r.right() - 8.0, r.center().y) } else { pos2(r.left() + 8.0, r.center().y) };
            ui.painter().circle_filled(knob, 5.0, if on { t.on_accent } else { t.text_dim });
            if resp.on_hover_text(app.ui.language.tr("AutoSave: save after every change (workbooks saved to a file)")).clicked() {
                app.toggle_autosave();
            }
            ui.add_space(6.0);
            let __k = icon_button(ui, Icon::Home, t.text_dim, "Home", vec2(26.0, ch));
            control_keytip(app, ui, &__k, "start");
            if __k.clicked() {
                app.open_dialog("start", json!({}));
            }
            if icon_button(ui, Icon::Save, t.text_dim, "Save", vec2(26.0, ch)).clicked()
                // File operations must include the cell the user is still editing (as the File > Save menu item already does).
                && app.commit_edit(0, 0, false, false)
            {
                app.run_or_alert("file.save", json!({}));
            }
            // The commands' own availability: there must be a step, and no undo group may be open on
            // the document (an action that spans several calls is still running).
            let can_undo = gridcraft_engine::cmd::can_undo(&app.session).is_ok();
            let can_redo = gridcraft_engine::cmd::can_redo(&app.session).is_ok();
            // Named and marked enabled or not for assistive technology (and the UI tests).
            let undo = icon_button(ui, Icon::Undo, if can_undo { t.text_dim } else { t.text_disabled }, "Undo", vec2(26.0, ch));
            undo.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can_undo, "Undo"));
            if undo.clicked() && can_undo {
                app.run_or_alert("edit.undo", json!({}));
            }
            let undo_list = icon_button(ui, Icon::Chevron, t.text_dim, "Undo list", vec2(14.0, ch));
            egui::Popup::menu(&undo_list).show(|ui| {
                let labels: Vec<String> = if can_undo {
                    app.session.active().map(|d| d.undo.iter().rev().take(20).map(|e| e.label.clone()).collect()).unwrap_or_default()
                } else {
                    Vec::new()
                };
                if labels.is_empty() {
                    ui.label("Can't Undo");
                }
                for (i, l) in labels.iter().enumerate() {
                    if ui.button(format!("Undo {l}")).clicked() {
                        app.run_or_alert("edit.undo", json!({"steps": i + 1}));
                    }
                }
            });
            let redo = icon_button(ui, Icon::Redo, if can_redo { t.text_dim } else { t.text_disabled }, "Redo", vec2(26.0, ch));
            redo.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can_redo, "Redo"));
            if redo.clicked() && can_redo {
                app.run_or_alert("edit.redo", json!({}));
            }
            let more = icon_button(ui, Icon::More, t.text_dim, "More commands", vec2(26.0, ch));
            egui::Popup::menu(&more).show(|ui| {
                for (label, id) in [
                    ("New Workbook", "file.new"),
                    ("Open…", "file.open"),
                    ("Save As…", "file.saveAs"),
                    ("Sort A to Z", "data.sortAscending"),
                    ("Calculate Now", "formulas.calculateNow"),
                ] {
                    if ui.button(label).clicked() {
                        app.run_or_alert(id, json!({}));
                    }
                }
                if ui.button("Print…").clicked() {
                    app.print_now(json!({}));
                }
            });
            let left_end = ui.min_rect().right();
            let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let __k = icon_button(ui, Icon::Search, t.text_dim, "Search commands", vec2(28.0, ch));
                control_keytip(app, ui, &__k, "commandSearch");
                if __k.clicked() {
                    app.open_dialog("commandSearch", json!({}));
                }
                let share = small_button(ui, Icon::Share, "Share", "Share: export or save a copy", true);
                egui::Popup::menu(&share).show(|ui| {
                    if ui.button("Export as PDF…").clicked() {
                        app.run_or_alert("file.exportPdf", json!({}));
                    }
                    for (label, fmt) in
                        [("Save a Copy as Excel Workbook (.xlsx)…", "xlsx"), ("Export as CSV…", "csv"), ("Export as Web Page (.html)…", "html")]
                    {
                        if ui.button(label).clicked() {
                            app.open_dialog("saveCopy", json!({"format": fmt}));
                        }
                    }
                });
                let __k = icon_button(ui, Icon::Comment, t.text_dim, "Comments", vec2(28.0, ch));
                control_keytip(app, ui, &__k, "comments");
                if __k.clicked() {
                    app.open_dialog("comments", json!({}));
                }
                // Quick light/dark switch (#194); right-click offers System as well. The same
                // choice lives in File › Display theme and the View tab.
                let lang = app.ui.language;
                let theme = icon_button(ui, Icon::Appearance, t.text_dim, "Dark Mode", vec2(28.0, 26.0));
                let dark = ui.ctx().theme() == egui::Theme::Dark;
                theme.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, dark, lang.tr("Dark Mode")));
                if theme.clicked() {
                    act(app, "view.darkMode", json!({}));
                }
                theme.context_menu(|ui| theme_choices(app, ui));
                ui.min_rect().left()
            });
            // Centred title, cut with "…" so it never runs under the controls on either side.
            let title = app.session.active().map(|d| d.display_title()).unwrap_or_default();
            let dirty = app.session.active().is_some_and(|d| d.is_dirty());
            let max_w = title_max_width(rect.center().x, left_end, right.inner);
            paint_title(ui, rect.center(), &title, dirty, max_w, t.text);
        });
    });
}

/// The `movable` flag this window's pass gives the app's windows, or `None` when it leaves them
/// alone because the pointer is in another window (or in none).
///
/// craft-window keeps one movable value for all the windows, and `sync_movable` sets the flag on
/// every app window. Each workbook window draws its title bar in the same frame, and one the
/// pointer isn't over says "movable": were it to set the flag after the window whose Undo is
/// under the pointer, that window could move and zoom again (#2546). Only the window the pointer
/// is in decides; it reads the value its own title bar has just left.
#[cfg(not(target_arch = "wasm32"))]
pub fn movable_to_sync(ctx: &egui::Context) -> Option<bool> {
    ctx.pointer_hover_pos().is_some().then(|| craft_window::title_bar::window_movable(ctx))
}

#[cfg(not(target_arch = "wasm32"))]
fn wanted_id() -> egui::Id {
    egui::Id::unique("gridcraft_title_bar_wanted_movable")
}

/// The `movable` flag the last `sync_window_movable` asked for, on every desktop platform (only macOS
/// sets it on the window). `None` before the pointer has been in a window.
#[cfg(not(target_arch = "wasm32"))]
pub fn wanted_movable(ctx: &egui::Context) -> Option<bool> {
    ctx.data(|d| d.get_temp::<bool>(wanted_id()))
}

/// macOS: AppKit may move (and on a double-click, zoom) the window only from the title bar's empty
/// area. Call once per frame, after the window's UI has run, whether or not it drew a title bar: a
/// host that hides the bar while the pointer is on a control must not leave the window unmovable.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sync_window_movable(ctx: &egui::Context) {
    let Some(movable) = movable_to_sync(ctx) else {
        return;
    };
    ctx.data_mut(|d| d.insert_temp(wanted_id(), movable));
    #[cfg(target_os = "macos")]
    {
        let id = egui::Id::unique("gridcraft_title_bar_movable");
        let mut applied = ctx.data(|d| d.get_temp::<Option<bool>>(id)).flatten();
        craft_window::title_bar::sync_movable(ctx, &mut applied);
        ctx.data_mut(|d| d.insert_temp(id, applied));
    }
}

/// Lay out the title truncated to fit `max_w` together with the unsaved marker (when `dirty`).
fn title_galleys(
    painter: &egui::Painter,
    title: &str,
    dirty: bool,
    max_w: f32,
    color: Color32,
) -> (std::sync::Arc<egui::Galley>, Option<std::sync::Arc<egui::Galley>>) {
    let font = theme::ui_bold(13.0);
    let dot = dirty.then(|| painter.layout_no_wrap(" •".to_string(), font.clone(), color));
    let dot_w = dot.as_ref().map_or(0.0, |g| g.size().x);
    let mut job = egui::text::LayoutJob::simple_singleline(title.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_w - dot_w).max(0.0));
    (painter.layout_job(job), dot)
}

/// Gap kept between the title and the controls beside it.
const TITLE_GAP: f32 = 12.0;

/// The widest the centred title may be without reaching the controls that end at `left_end` or
/// start at `right_start`.
fn title_max_width(center_x: f32, left_end: f32, right_start: f32) -> f32 {
    let half = (center_x - left_end).min(right_start - center_x) - TITLE_GAP;
    (2.0 * half).max(0.0)
}

/// Paint the workbook title centred on `center`, truncated with "…" to `max_w`. The unsaved
/// marker stays visible however short the title gets.
fn paint_title(ui: &Ui, center: egui::Pos2, title: &str, dirty: bool, max_w: f32, color: Color32) {
    let (galley, dot) = title_galleys(ui.painter(), title, dirty, max_w, color);
    let dot_w = dot.as_ref().map_or(0.0, |g| g.size().x);
    let w = galley.size().x + dot_w;
    let left = center.x - w / 2.0;
    let top = center.y - galley.size().y / 2.0;
    let galley_w = galley.size().x;
    ui.painter().galley(pos2(left, top), galley, color);
    if let Some(dot) = dot {
        ui.painter().galley(pos2(left + galley_w, top), dot, color);
    }
}

#[cfg(test)]
mod title_bar_tests {
    use super::*;

    #[test]
    fn mac_title_bar_centres_and_clears_the_traffic_lights() {
        // The lights are centred in the bar, and content keeps the same gap after them.
        let margin = (MAC_TITLE_BAR - MAC_LIGHT) / 2.0;
        let lights_end = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT;
        assert_eq!(MAC_CONTENT_LEFT - lights_end, margin);
    }

    #[test]
    fn title_fits_between_the_controls() {
        // Room limited by the nearer side, symmetric about the centre.
        assert_eq!(title_max_width(500.0, 300.0, 900.0), 2.0 * (200.0 - TITLE_GAP));
        assert_eq!(title_max_width(500.0, 100.0, 650.0), 2.0 * (150.0 - TITLE_GAP));
        // Controls overlapping the centre leave no room rather than a negative width.
        assert_eq!(title_max_width(500.0, 520.0, 900.0), 0.0);
    }

    /// Lay out a title in a real egui frame, returning (title text, title width, dot width).
    fn layout_title(title: &str, dirty: bool, max_w: f32) -> (String, f32, f32) {
        let ctx = egui::Context::default();
        ctx.set_fonts(theme::font_definitions());
        let mut out = (String::new(), 0.0, 0.0);
        let mut frame = ctx.run_ui(egui::RawInput::default(), |ui| {
            let (galley, dot) = title_galleys(ui.painter(), title, dirty, max_w, Color32::WHITE);
            // The glyphs actually drawn: `galley.text()` is the untruncated source.
            let shown = galley.rows.iter().flat_map(|r| r.glyphs.iter().map(|g| g.chr)).collect();
            out = (shown, galley.size().x, dot.map_or(0.0, |d| d.size().x));
        });
        frame.textures_delta.clear(); // nothing consumes the atlas in a headless test
        out
    }

    #[test]
    fn long_title_is_truncated_with_an_ellipsis() {
        let long = format!("upload-{}-cleaned (1).xlsx", "3FMyNTYg2gDaANiAAACq20AAAAQMBqug".repeat(4));
        let (text, w, _) = layout_title(&long, false, 300.0);
        assert!(text.ends_with('…'), "truncated title ends with an ellipsis: {text:?}");
        assert!(text.chars().count() < long.chars().count());
        assert!(w <= 300.0, "title stays within its room: {w}");
    }

    #[test]
    fn short_title_is_left_whole() {
        let (text, _, _) = layout_title("Book1.xlsx", false, 300.0);
        assert_eq!(text, "Book1.xlsx");
    }

    #[test]
    fn unsaved_dot_survives_truncation() {
        let long = "a very long workbook name ".repeat(20);
        let (text, w, dot_w) = layout_title(&long, true, 200.0);
        assert!(dot_w > 0.0, "the unsaved marker is laid out");
        assert!(text.ends_with('…'));
        assert!(w + dot_w <= 200.0, "title and marker fit together: {}", w + dot_w);
    }

    /// A window's UI: a 400×28 title bar with a button at x 10..60 on it, then the sync, as
    /// `SheetApp::ui` ends. Returns the `movable` flag the window's pass asked for.
    fn window_ui(ui: &mut Ui) -> Option<bool> {
        craft_window::title_bar::empty_area(ui, egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 28.0)));
        let _ = ui.put(egui::Rect::from_min_max(pos2(10.0, 4.0), pos2(60.0, 24.0)), egui::Button::new("Undo"));
        let wanted = movable_to_sync(ui.ctx());
        sync_window_movable(ui.ctx());
        wanted
    }

    fn window_input(viewport: egui::ViewportId, time: f64, events: Vec<egui::Event>) -> egui::RawInput {
        let mut input = egui::RawInput {
            viewport_id: viewport,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 300.0))),
            time: Some(time),
            events,
            ..egui::RawInput::default()
        };
        input.viewports.entry(viewport).or_default();
        input
    }

    /// Two workbook windows drawn as `doc_windows` draws them: the root window's UI, then the
    /// second window as an immediate viewport in the same frame, through a renderer standing in
    /// for eframe's.
    struct TwoWindows {
        ctx: egui::Context,
        time: std::rc::Rc<std::cell::Cell<f64>>,
        second_events: std::rc::Rc<std::cell::RefCell<Vec<egui::Event>>>,
        second_wanted: std::rc::Rc<std::cell::Cell<Option<bool>>>,
    }

    const SECOND: &str = "second workbook";

    impl TwoWindows {
        fn new() -> Self {
            let ctx = egui::Context::default();
            ctx.set_embed_viewports(false);
            let time = std::rc::Rc::new(std::cell::Cell::new(0.0));
            let second_events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let (t, ev) = (std::rc::Rc::clone(&time), std::rc::Rc::clone(&second_events));
            egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
                let input = window_input(viewport.ids.this, t.get(), ev.take());
                let mut out = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
                out.textures_delta.clear();
            });
            Self { ctx, time, second_events, second_wanted: std::rc::Rc::default() }
        }

        /// One frame with each window's events; returns what each window's pass asked for and
        /// the flag the frame ends with.
        fn frame(&mut self, first: Vec<egui::Event>, second: Vec<egui::Event>) -> (Option<bool>, Option<bool>, Option<bool>) {
            self.time.set(self.time.get() + 1.0 / 60.0);
            *self.second_events.borrow_mut() = second;
            let mut first_wanted = None;
            let second_wanted = std::rc::Rc::clone(&self.second_wanted);
            let mut out = self.ctx.run_ui(window_input(egui::ViewportId::ROOT, self.time.get(), first), |ui| {
                first_wanted = window_ui(ui);
                let id = egui::ViewportId::from_hash_of(SECOND);
                ui.ctx().show_viewport_immediate(id, egui::ViewportBuilder::default(), |ui, _| second_wanted.set(window_ui(ui)));
            });
            out.textures_delta.clear();
            (first_wanted, self.second_wanted.get(), wanted_movable(&self.ctx))
        }
    }

    /// #2546 with several workbook windows: they share one context and draw their title bars in
    /// the same frame, and one the pointer isn't over must not make the windows movable again
    /// after the window whose Undo is under the pointer made them unmovable.
    #[test]
    fn only_the_window_with_the_pointer_sets_movable() {
        let mut w = TwoWindows::new();
        let (undo, empty) = (pos2(30.0, 14.0), pos2(300.0, 14.0));
        for _ in 0..3 {
            w.frame(vec![egui::Event::PointerMoved(undo)], vec![]);
        }
        // The window under the pointer draws first, the other last: the order that went wrong.
        let (on_undo, other, wanted) = w.frame(vec![], vec![]);
        assert_eq!(on_undo, Some(false), "the pointer on Undo: its window must not move");
        assert_eq!(other, None, "the window without the pointer leaves the flag alone");
        assert_eq!(wanted, Some(false), "and the frame ends unmovable");
        for _ in 0..3 {
            w.frame(vec![egui::Event::PointerMoved(empty)], vec![]);
        }
        assert_eq!(w.frame(vec![], vec![]).2, Some(true), "the empty bar moves the window again");
        // The pointer leaves for the second window, onto its Undo: that window decides now.
        w.frame(vec![egui::Event::PointerGone], vec![egui::Event::PointerMoved(undo)]);
        for _ in 0..3 {
            w.frame(vec![], vec![]);
        }
        assert_eq!(w.frame(vec![], vec![]), (None, Some(false), Some(false)), "the second window's Undo keeps the windows unmovable");
        w.frame(vec![], vec![egui::Event::PointerMoved(empty)]);
        w.frame(vec![], vec![]);
        assert_eq!(w.frame(vec![], vec![]), (None, Some(true), Some(true)), "and its empty bar moves them");
    }

    #[test]
    fn a_window_the_pointer_never_entered_sets_nothing() {
        let mut w = TwoWindows::new();
        for _ in 0..3 {
            assert_eq!(w.frame(vec![], vec![]), (None, None, None));
        }
    }

    #[test]
    fn no_room_draws_nothing_but_does_not_crash() {
        let (_, w, _) = layout_title("Book1.xlsx", true, 0.0);
        assert!(w <= 20.0, "a zero-width slot leaves at most the ellipsis: {w}");
    }
}
