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

/// Windows draws minimize, maximize and close in this bar and turns the native title bar off,
/// so the file name and the quick commands share that one row with the window buttons.
///
/// macOS already draws into the title bar (`with_fullsize_content_view`); AppKit keeps the
/// traffic lights, and this flag stays off so that inset is unchanged.
///
/// Other desktops keep the window manager's frame. A single button order would be wrong where
/// the desktop puts the buttons on the left, or only draws Close (GNOME's `button-layout`).
/// That is a window-manager special case this crate does not take on, so the native frame — and
/// the second row — stays.
pub const INTEGRATED_CAPTION: bool = cfg!(target_os = "windows");

/// Width of one caption button. Three of them sit at the trailing edge, full bar height, so the
/// top corner is the close button.
const CAPTION_BUTTON_W: f32 = 46.0;

/// How far a resize band reaches in from a window edge when the native frame is off.
const RESIZE_EDGE: f32 = 6.0;

/// How far a corner's grab runs along each of its two edges.
const RESIZE_CORNER: f32 = 16.0;

/// A window button drawn in the title bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptionKind {
    Minimize,
    Maximize,
    Close,
}

/// Draws the title bar: drag/maximize, AutoSave toggle, Home/Save/Undo/Redo/More, search, share, comments and theme controls, with a centered truncated title.
/// On Windows the caption buttons share this row (see [`INTEGRATED_CAPTION`]).
pub fn title_bar(app: &mut SheetApp, ui: &mut Ui) {
    title_bar_caption(app, ui, INTEGRATED_CAPTION);
}

fn title_bar_caption(app: &mut SheetApp, ui: &mut Ui, integrated: bool) {
    let t = Tokens::get(ui.ctx());
    let ch = TITLE_CONTROL_H;
    egui::Panel::top("title_bar").exact_size(TITLE_BAR_H).frame(egui::Frame::NONE.fill(t.window)).show(ui, |ui| {
        let rect = ui.max_rect();
        let caption = caption_button_rects(rect, integrated);
        // A press that started on a caption button is that button's, not a window drag or a
        // maximize. egui would otherwise also give the drag to this full-bar widget.
        let on_caption = ui.input(|i| i.pointer.press_origin()).is_some_and(|p| caption.iter().any(|b| b.contains(p)));
        // Dragging the empty title area moves the window.
        let bg = ui.interact(rect, ui.scope_id().with("drag_title"), Sense::click_and_drag());
        if bg.drag_started() && !on_caption {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() && !on_caption {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
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
                // The caption buttons are painted on this strip afterwards. Reserving it here
                // keeps search, share and the theme switch from sliding underneath them.
                ui.add_space(if integrated { caption_reserve(&caption) } else { 8.0 });
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
        if integrated {
            paint_caption_buttons(ui, &caption);
        }
    });
}

/// Minimize, maximize and close, left to right, flush with the trailing edge of `bar`.
/// Empty rects when `integrated` is false: macOS and other desktops draw no buttons here.
fn caption_button_rects(bar: egui::Rect, integrated: bool) -> [egui::Rect; 3] {
    if !integrated || bar.width() <= 0.0 || bar.height() <= 0.0 {
        return [egui::Rect::NOTHING; 3];
    }
    let w = CAPTION_BUTTON_W.min(bar.width() / 3.0);
    let (right, top, bottom) = (bar.right(), bar.top(), bar.bottom());
    let slot = |from_right: f32| egui::Rect::from_min_max(pos2(right - from_right * w, top), pos2(right - (from_right - 1.0) * w, bottom));
    [slot(3.0), slot(2.0), slot(1.0)]
}

/// Width of the caption strip, or 0 when there are no buttons.
fn caption_reserve(buttons: &[egui::Rect; 3]) -> f32 {
    if !buttons[0].is_positive() { 0.0 } else { buttons.iter().map(|r| r.width()).sum::<f32>() }
}

fn caption_label(kind: CaptionKind, maximized: bool) -> &'static str {
    match kind {
        CaptionKind::Minimize => "Minimize",
        CaptionKind::Maximize if maximized => "Restore",
        CaptionKind::Maximize => "Maximize",
        CaptionKind::Close => "Close",
    }
}

fn caption_command(kind: CaptionKind, maximized: bool) -> egui::ViewportCommand {
    match kind {
        CaptionKind::Minimize => egui::ViewportCommand::Minimized(true),
        CaptionKind::Maximize => egui::ViewportCommand::Maximized(!maximized),
        CaptionKind::Close => egui::ViewportCommand::Close,
    }
}

fn paint_caption_glyph(painter: &egui::Painter, rect: egui::Rect, kind: CaptionKind, maximized: bool, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.2, color);
    match kind {
        CaptionKind::Minimize => {
            painter.line_segment([pos2(c.x - 5.0, c.y + 3.0), pos2(c.x + 5.0, c.y + 3.0)], stroke);
        }
        CaptionKind::Maximize if !maximized => {
            painter.rect_stroke(egui::Rect::from_center_size(c, vec2(10.0, 10.0)), 0.0, stroke, StrokeKind::Inside);
        }
        CaptionKind::Maximize => {
            let back = egui::Rect::from_min_size(pos2(c.x - 5.0, c.y - 6.0), vec2(8.0, 8.0));
            let front = egui::Rect::from_min_size(pos2(c.x - 2.0, c.y - 3.0), vec2(8.0, 8.0));
            painter.rect_stroke(back, 0.0, stroke, StrokeKind::Inside);
            painter.rect_stroke(front, 0.0, stroke, StrokeKind::Inside);
        }
        CaptionKind::Close => {
            painter.line_segment([pos2(c.x - 5.0, c.y - 5.0), pos2(c.x + 5.0, c.y + 5.0)], stroke);
            painter.line_segment([pos2(c.x + 5.0, c.y - 5.0), pos2(c.x - 5.0, c.y + 5.0)], stroke);
        }
    }
}

fn paint_caption_buttons(ui: &mut Ui, buttons: &[egui::Rect; 3]) {
    let t = Tokens::get(ui.ctx());
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let kinds = [CaptionKind::Minimize, CaptionKind::Maximize, CaptionKind::Close];
    for (kind, rect) in kinds.into_iter().zip(buttons.iter().copied()) {
        if !rect.is_positive() {
            continue;
        }
        let resp = ui.interact(rect, ui.scope_id().with(("caption", caption_label(kind, false))), Sense::click());
        let close = kind == CaptionKind::Close;
        let hot = resp.hovered() || resp.is_pointer_button_down_on();
        if hot {
            ui.painter().rect_filled(rect, 0.0, if close { t.danger } else { t.hover });
        }
        let glyph = if close && hot { if t.dark { t.window } else { Color32::WHITE } } else { t.text };
        paint_caption_glyph(ui.painter(), rect, kind, maximized, glyph);
        let label = caption_label(kind, maximized);
        let resp = resp.on_hover_text(label);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if resp.clicked() {
            ui.ctx().send_viewport_cmd(caption_command(kind, maximized));
        }
    }
}

/// Whether a desktop still paints its own title bar above this one.
///
/// `macos_lights` is the macOS full-size title bar (traffic lights in this row, no second bar).
/// `caption_in_app_bar` is Windows. Any other desktop keeps its frame, so both rows are there.
///
/// The production flag is [`INTEGRATED_CAPTION`]. These two helpers exist so a test can state the
/// row count for every desktop without compiling three times.
#[cfg(test)]
fn native_title_row(macos_lights: bool, caption_in_app_bar: bool) -> bool {
    !macos_lights && !caption_in_app_bar
}

/// How many chrome rows sit above the ribbon. One when the native title bar is absent.
#[cfg(test)]
fn chrome_rows(native_title: bool) -> u8 {
    1 + u8::from(native_title)
}

/// Resize bands for a window whose native frame is off. They leave `buttons` alone, so a click
/// in the top corner closes the window instead of resizing it.
fn resize_bands(window: egui::Rect, buttons: &[egui::Rect]) -> Vec<(egui::ResizeDirection, egui::Rect)> {
    use egui::ResizeDirection as D;
    let (l, r, t, b, e, c) = (window.left(), window.right(), window.top(), window.bottom(), RESIZE_EDGE, RESIZE_CORNER);
    let band = |x0: f32, y0: f32, x1: f32, y1: f32| egui::Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
    let bands = [
        (D::NorthWest, band(l, t, l + c, t + e)),
        (D::NorthWest, band(l, t, l + e, t + c)),
        (D::NorthEast, band(r - c, t, r, t + e)),
        (D::NorthEast, band(r - e, t, r, t + c)),
        (D::SouthWest, band(l, b - e, l + c, b)),
        (D::SouthWest, band(l, b - c, l + e, b)),
        (D::SouthEast, band(r - c, b - e, r, b)),
        (D::SouthEast, band(r - e, b - c, r, b)),
        (D::North, band(l + c, t, r - c, t + e)),
        (D::South, band(l + c, b - e, r - c, b)),
        (D::West, band(l, t + c, l + e, b - c)),
        (D::East, band(r - e, t + c, r, b - c)),
    ];
    let mut out = Vec::new();
    for (direction, band) in bands {
        let mut pieces = vec![band];
        for button in buttons {
            if !button.is_positive() {
                continue;
            }
            let mut next = Vec::new();
            for piece in pieces {
                next.extend(outside(piece, *button));
            }
            pieces = next;
        }
        out.extend(pieces.into_iter().filter(|p| p.is_positive()).map(|p| (direction, p)));
    }
    out
}

/// The parts of `rect` that lie outside `hole`.
fn outside(rect: egui::Rect, hole: egui::Rect) -> Vec<egui::Rect> {
    let overlap = rect.intersect(hole);
    if !overlap.is_positive() {
        return vec![rect];
    }
    [
        egui::Rect::from_min_max(rect.min, pos2(rect.right(), overlap.top())),
        egui::Rect::from_min_max(pos2(rect.left(), overlap.bottom()), rect.max),
        egui::Rect::from_min_max(pos2(rect.left(), overlap.top()), overlap.left_bottom()),
        egui::Rect::from_min_max(overlap.right_top(), pos2(rect.right(), overlap.bottom())),
    ]
    .into_iter()
    .filter(|r| r.is_positive())
    .collect()
}

fn resize_cursor(direction: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    use egui::ResizeDirection as D;
    match direction {
        D::North => C::ResizeNorth,
        D::South => C::ResizeSouth,
        D::East => C::ResizeEast,
        D::West => C::ResizeWest,
        D::NorthEast => C::ResizeNorthEast,
        D::NorthWest => C::ResizeNorthWest,
        D::SouthEast => C::ResizeSouthEast,
        D::SouthWest => C::ResizeSouthWest,
    }
}

/// Which side of the window a scrollbar edge is measured against.
enum ResizeSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// How far `outer` reaches into the resize band on `side`. Zero when the edge is already clear,
/// or when `margin` is zero (no resize frame).
fn resize_clearance(outer: f32, window: egui::Rect, side: ResizeSide, margin: f32) -> f32 {
    if margin <= 0.0 {
        return 0.0;
    }
    let overlap = match side {
        ResizeSide::Right => outer - (window.right() - margin),
        ResizeSide::Left => (window.left() + margin) - outer,
        ResizeSide::Bottom => outer - (window.bottom() - margin),
        ResizeSide::Top => (window.top() + margin) - outer,
    };
    overlap.max(0.0)
}

/// The vertical scrollbar's x span. When its outer edge meets the window, the whole `bar` moves
/// inward by the resize margin instead of losing that strip of its width. A bar that stops short
/// of the frame (a task pane owns the edge) stays where it is.
pub(crate) fn vertical_scrollbar_x(available: egui::Rect, window: egui::Rect, rtl: bool, bar: f32, margin: f32) -> (f32, f32) {
    let bar = bar.max(0.0);
    if rtl {
        let left = (available.left() + resize_clearance(available.left(), window, ResizeSide::Left, margin)).min(available.right());
        let right = (left + bar).min(available.right());
        (left, right.max(left))
    } else {
        let right = (available.right() - resize_clearance(available.right(), window, ResizeSide::Right, margin)).max(available.left());
        let left = (right - bar).max(available.left());
        (left, right.max(left))
    }
}

/// Pull `track` off every window edge that carries a resize band. Edges the track does not reach
/// stay put, so a scrollbar already inside the frame is unchanged. The bands themselves stay.
pub(crate) fn dodge_resize_bands(track: egui::Rect, window: egui::Rect, margin: f32) -> egui::Rect {
    if margin <= 0.0 {
        return track;
    }
    let left = track.left() + resize_clearance(track.left(), window, ResizeSide::Left, margin);
    let right = track.right() - resize_clearance(track.right(), window, ResizeSide::Right, margin);
    let top = track.top() + resize_clearance(track.top(), window, ResizeSide::Top, margin);
    let bottom = track.bottom() - resize_clearance(track.bottom(), window, ResizeSide::Bottom, margin);
    if left >= right || top >= bottom {
        return egui::Rect::from_min_max(pos2(left, top), pos2(left, top));
    }
    egui::Rect::from_min_max(pos2(left, top), pos2(right, bottom))
}

/// Whether this window is drawing resize bands. Same conditions as [`resize_frame`].
fn resize_frame_active(window: egui::Rect, maximized: bool, fullscreen: bool) -> bool {
    INTEGRATED_CAPTION && window.width() >= 1.0 && window.height() >= 1.0 && !maximized && !fullscreen
}

/// Width of the resize band a scrollbar must stay out of. Zero when the bands are not drawn
/// (another desktop, or a maximized or full-screen window).
pub(crate) fn frame_resize_margin(ui: &egui::Ui) -> f32 {
    let (window, maximized, fullscreen) =
        ui.input(|i| (i.viewport_rect(), i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if resize_frame_active(window, maximized, fullscreen) { RESIZE_EDGE } else { 0.0 }
}

/// Edge and corner drags of a window with no native frame. No-op except on Windows, and no-op
/// while the window is maximized or full screen (the frame is gone, and a band would steal
/// clicks from the caption buttons' top edge).
pub(crate) fn resize_frame(ui: &Ui, window: egui::Rect) {
    let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
    let fullscreen = ui.input(|i| i.viewport().fullscreen.unwrap_or(false));
    if !resize_frame_active(window, maximized, fullscreen) {
        return;
    }
    let bar = egui::Rect::from_min_size(window.min, vec2(window.width(), TITLE_BAR_H.min(window.height())));
    let buttons = caption_button_rects(bar, true);
    let bands = resize_bands(window, &buttons);
    if bands.is_empty() {
        return;
    }
    let id = egui::Id::unique("gridcraft-frame-resize");
    let layer = egui::LayerId::new(egui::Order::Foreground, id);
    let mut overlay = egui::Ui::new(ui.ctx().clone(), id, egui::UiBuilder::new().layer_id(layer).max_rect(window));
    overlay.set_clip_rect(window);
    for (i, (direction, band)) in bands.iter().copied().enumerate() {
        let response = overlay.interact(band, id.with(i), Sense::drag()).on_hover_cursor(resize_cursor(direction));
        if response.drag_started() {
            overlay.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
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
    fn vertical_scrollbar_stays_clear_of_the_resize_band() {
        let window = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 400.0));
        let (left, right) = vertical_scrollbar_x(window, window, false, 14.0, RESIZE_EDGE);
        assert!((right - left - 14.0).abs() < 0.01, "the bar keeps its width, got {}", right - left);
        assert!((window.right() - right - RESIZE_EDGE).abs() < 0.01, "the east band stays clear");
        let (left, right) = vertical_scrollbar_x(window, window, true, 14.0, RESIZE_EDGE);
        assert!((left - RESIZE_EDGE).abs() < 0.01, "the west band stays clear");
        assert!((right - left - 14.0).abs() < 0.01);
        // A bar that stops short of the frame (a task pane owns the edge) is not moved.
        let inner = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(150.0, 400.0));
        let (_, right) = vertical_scrollbar_x(inner, window, false, 14.0, RESIZE_EDGE);
        assert!((right - 150.0).abs() < 0.01);
        let (left, right) = vertical_scrollbar_x(window, window, false, 14.0, 0.0);
        assert!((right - 200.0).abs() < 0.01 && (left - 186.0).abs() < 0.01);
    }

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

    /// Lay out a title in a real egui frame, checking baselines and returning (text, width, dot width).
    fn layout_title(title: &str, dirty: bool, max_w: f32) -> (String, f32, f32) {
        let ctx = egui::Context::default();
        ctx.set_fonts(theme::font_definitions());
        let mut out = (String::new(), 0.0, 0.0);
        let mut frame = ctx.run_ui(egui::RawInput::default(), |ui| {
            let (galley, dot) = title_galleys(ui.painter(), title, dirty, max_w, Color32::WHITE);
            for row in &galley.rows {
                if let Some(first) = row.glyphs.first() {
                    assert!(row.glyphs.iter().all(|glyph| (glyph.pos.y - first.pos.y).abs() < 0.01), "title glyphs share a baseline: {title:?}");
                }
            }
            // The glyphs actually drawn: `galley.text()` is the untruncated source.
            let shown = galley.rows.iter().flat_map(|r| r.glyphs.iter().map(|g| g.chr)).collect();
            out = (shown, galley.size().x, dot.map_or(0.0, |d| d.size().x));
        });
        frame.textures_delta.clear(); // nothing consumes the atlas in a headless test
        out
    }

    #[test]
    fn long_title_is_truncated_with_an_ellipsis() {
        let long = format!("资产清单2026第4季度-{}-中文 (1).xlsx", "3FMyNTYg2gDaANiAAACq20AAAAQMBqug".repeat(4));
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

    #[test]
    fn no_room_draws_nothing_but_does_not_crash() {
        let (_, w, _) = layout_title("Book1.xlsx", true, 0.0);
        assert!(w <= 20.0, "a zero-width slot leaves at most the ellipsis: {w}");
    }

    #[test]
    fn chrome_above_the_ribbon_is_one_row_on_windows_and_mac() {
        // Windows puts the caption buttons in the app bar and drops the native title bar.
        // macOS already has the traffic lights in that bar. Other desktops keep their frame,
        // so the native title bar and this bar are two rows.
        assert_eq!(chrome_rows(native_title_row(false, true)), 1, "windows");
        assert_eq!(chrome_rows(native_title_row(true, false)), 1, "macos");
        assert_eq!(chrome_rows(native_title_row(false, false)), 2, "other desktops");
        assert_eq!(INTEGRATED_CAPTION, cfg!(target_os = "windows"));
        let native = native_title_row(cfg!(target_os = "macos"), INTEGRATED_CAPTION);
        assert_eq!(chrome_rows(native), if cfg!(target_os = "macos") || INTEGRATED_CAPTION { 1 } else { 2 });
        if INTEGRATED_CAPTION {
            assert_eq!(TITLE_BAR_H, 38.0, "the single Windows row is the app bar, not a native caption plus this bar");
        }
    }

    #[test]
    fn caption_buttons_fill_the_corner_and_stay_clear_of_the_commands() {
        let bar = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, TITLE_BAR_H));
        let [minimize, maximize, close] = caption_button_rects(bar, true);
        assert_eq!(close.right(), bar.right());
        assert_eq!(close.top(), bar.top());
        assert_eq!(close.bottom(), bar.bottom());
        assert!((close.width() - CAPTION_BUTTON_W).abs() < 0.01);
        assert_eq!(maximize.right(), close.left());
        assert_eq!(minimize.right(), maximize.left());
        assert!(!minimize.intersect(maximize).is_positive());
        assert!(!maximize.intersect(close).is_positive());
        // Everything to the left of the strip is the command row (AutoSave, undo, search, …).
        let commands = egui::Rect::from_min_max(bar.min, pos2(minimize.left(), bar.bottom()));
        for button in [minimize, maximize, close] {
            assert!(!commands.intersect(button).is_positive(), "{button:?} overlaps the commands");
            assert!(bar.contains_rect(button));
        }
        assert!(caption_button_rects(bar, false).iter().all(|r| !r.is_positive()), "macOS and Linux draw no caption buttons here");
        assert_eq!(caption_reserve(&caption_button_rects(bar, false)), 0.0);
        assert!(caption_reserve(&[minimize, maximize, close]) > 0.0);
    }

    #[test]
    fn resize_bands_leave_the_caption_buttons_and_the_top_corner() {
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 600.0));
        let bar = egui::Rect::from_min_size(window.min, vec2(window.width(), TITLE_BAR_H));
        let buttons = caption_button_rects(bar, true);
        let bands = resize_bands(window, &buttons);
        for (direction, band) in &bands {
            for button in buttons {
                assert!(!band.intersect(button).is_positive(), "{direction:?} covers a caption button");
            }
        }
        let corner = pos2(window.right() - 0.5, window.top() + 0.5);
        assert!(buttons[2].contains(corner), "the top corner is the close button");
        assert!(bands.iter().all(|(_, band)| !band.contains(corner)));
        assert!(bands.iter().any(|(d, _)| *d == egui::ResizeDirection::SouthEast), "the window can still be resized");
    }

    #[test]
    fn caption_clicks_map_to_viewport_commands() {
        use egui::ViewportCommand::{Close, Maximized, Minimized};
        assert_eq!(caption_command(CaptionKind::Minimize, false), Minimized(true));
        assert_eq!(caption_command(CaptionKind::Maximize, false), Maximized(true));
        assert_eq!(caption_command(CaptionKind::Maximize, true), Maximized(false));
        assert_eq!(caption_command(CaptionKind::Close, true), Close);
        assert_eq!(caption_label(CaptionKind::Maximize, true), "Restore");
        assert_eq!(caption_label(CaptionKind::Maximize, false), "Maximize");
    }

    /// One title-bar frame. The caption is forced on, so Linux and macOS CI exercise the same
    /// hit testing as Windows. This does not open a Win32 window.
    struct Bar {
        ctx: egui::Context,
        app: SheetApp,
        time: f64,
    }

    impl Bar {
        fn new() -> Self {
            let mut session = gridcraft_engine::Session::new();
            session.new_workbook();
            let ctx = egui::Context::default();
            ctx.set_fonts(theme::font_definitions());
            Self { ctx, app: SheetApp::new(session, crate::Services::default()), time: 0.0 }
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> Vec<egui::ViewportCommand> {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 900.0))),
                time: Some(self.time),
                events,
                ..egui::RawInput::default()
            };
            let ctx = self.ctx.clone();
            let mut out = ctx.run_ui(input, |ui| {
                title_bar_caption(&mut self.app, ui, true);
            });
            out.textures_delta.clear();
            out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
        }
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn close_is_hit_and_an_empty_drag_moves_the_window() {
        let bar = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, TITLE_BAR_H));
        let close = caption_button_rects(bar, true)[2].center();
        let mut ui = Bar::new();
        for _ in 0..2 {
            ui.frame(vec![]);
        }
        ui.frame(vec![egui::Event::PointerMoved(close)]);
        ui.frame(vec![pointer(close, true)]);
        let closed = ui.frame(vec![pointer(close, false)]);
        assert!(closed.iter().any(|c| matches!(c, egui::ViewportCommand::Close)), "the close button asks the window to close: {closed:?}");
        assert!(!closed.iter().any(|c| matches!(c, egui::ViewportCommand::StartDrag)), "clicking close does not drag the window: {closed:?}");

        // Between the quick commands and the centred title: empty bar, so the drag is the window's.
        let empty = pos2(480.0, TITLE_BAR_H / 2.0);
        ui.frame(vec![egui::Event::PointerMoved(empty)]);
        ui.frame(vec![pointer(empty, true)]);
        let mut dragged = ui.frame(vec![egui::Event::PointerMoved(empty + vec2(30.0, 0.0))]);
        dragged.extend(ui.frame(vec![pointer(empty + vec2(30.0, 0.0), false)]));
        assert!(dragged.iter().any(|c| matches!(c, egui::ViewportCommand::StartDrag)), "dragging the empty bar moves the window: {dragged:?}");
        assert!(!dragged.iter().any(|c| matches!(c, egui::ViewportCommand::Close)), "that drag is not a close: {dragged:?}");
    }
}
