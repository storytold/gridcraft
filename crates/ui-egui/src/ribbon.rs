//! Title bar (Quick Access Toolbar), ribbon tabs and ribbon groups, and keyboard shortcuts.

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{HAlign, Style, VAlign};
use serde_json::json;

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::keytips::keytip_table;
use crate::theme::{self, Tokens};
use crate::widgets::{big_button, color_palette, icon_button, small_button, split_button, toggle_button};

pub const TABS: &[&str] = &["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"];

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
const TITLE_BAR_H: f32 = if MAC { MAC_TITLE_BAR } else { 38.0 };
/// The tallest title-bar controls, smaller on macOS to keep a margin in the shorter bar.
const TITLE_CONTROL_H: f32 = if MAC { 22.0 } else { 26.0 };
const _: () = assert!(TITLE_CONTROL_H < TITLE_BAR_H, "title-bar controls keep a margin above and below");

pub fn title_bar(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ch = TITLE_CONTROL_H;
    egui::Panel::top("title_bar").exact_size(TITLE_BAR_H).frame(egui::Frame::NONE.fill(t.window)).show(ui, |ui| {
        let rect = ui.max_rect();
        // Dragging the empty title area moves the window.
        let bg = ui.interact(rect, ui.id().with("drag_title"), Sense::click_and_drag());
        if bg.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
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
            ui.painter().circle_filled(knob, 5.0, if on { Color32::WHITE } else { t.text_dim });
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
            undo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, can_undo, "Undo"));
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
            redo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, can_redo, "Redo"));
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
                theme.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, dark, lang.tr("Dark Mode")));
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

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let collapsed = app.ui.ribbon_collapsed;
    let h = if collapsed { 34.0 } else { 34.0 + 92.0 };
    // A contextual tab exists only while its object is selected (Excel hides Table Design once the
    // cell leaves the table); a remembered one that is gone would draw an empty ribbon.
    if CONTEXTUAL_TABS.contains(&app.ui.ribbon_tab.as_str()) && !contextual_tabs(app).contains(&app.ui.ribbon_tab.as_str()) {
        app.ui.ribbon_tab = "Home".to_string();
    }
    egui::Panel::top("ribbon")
        .exact_size(h)
        .frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 6 }))
        .show(ui, |ui| {
            // Tabs row.
            let tabs_rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 32.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.add_space(8.0);
                    if app.chrome.file_tab {
                        file_menu(app, ui);
                    }
                    let mut tabs: Vec<&str> = TABS.to_vec();
                    let ctx_tabs = contextual_tabs(app);
                    tabs.extend(ctx_tabs.iter());
                    for tab in tabs {
                        let active = app.ui.ribbon_tab == tab;
                        let contextual = ctx_tabs.contains(&tab);
                        let font = theme::ui_font(13.5);
                        let label = app.ui.language.tr(tab);
                        let tw = ui.painter().layout_no_wrap(label.to_string(), font.clone(), t.text).size().x;
                        let (r, resp) = ui.allocate_exact_size(vec2(tw + 22.0, 30.0), Sense::click());
                        if resp.hovered() && !active {
                            ui.painter().rect_filled(r.shrink2(vec2(2.0, 4.0)), 5.0, t.hover);
                        }
                        let color = if contextual {
                            t.accent
                        } else if active {
                            t.text
                        } else {
                            t.text_dim
                        };
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, label, if active { theme::ui_bold(13.5) } else { font }, color);
                        if active {
                            let ul = Rect::from_center_size(pos2(r.center().x, r.bottom() - 3.0), vec2(tw.clamp(20.0, 40.0), 3.0));
                            ui.painter().rect_filled(ul, 1.5, t.accent);
                        }
                        if resp.clicked() {
                            if active && !collapsed {
                                app.ui.ribbon_collapsed = true;
                            } else {
                                app.ui.ribbon_tab = tab.to_string();
                                app.ui.ribbon_collapsed = false;
                            }
                        }
                        if resp.double_clicked() {
                            app.ui.ribbon_collapsed = !collapsed;
                        }
                        if app.keytips.prefix() == Some("")
                            && let Some(sequence) = keytip_table::tab_sequence(tab)
                        {
                            keytip(app, ui, sequence, pos2(r.center().x, r.bottom() - 1.0));
                        }
                    }
                });
            });
            if collapsed {
                return;
            }
            // Ribbon card.
            let card = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 88.0));
            ui.painter().rect_filled(card, 10.0, t.ribbon);
            ui.painter().rect_stroke(card, 10.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            let inner = card.shrink2(vec2(10.0, 6.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                        match app.ui.ribbon_tab.as_str() {
                            "Home" => home(app, ui),
                            "Insert" => insert(app, ui),
                            "Draw" => draw(app, ui),
                            "Page Layout" => page_layout(app, ui),
                            "Formulas" => formulas(app, ui),
                            "Data" => data(app, ui),
                            "Review" => review(app, ui),
                            "View" => view(app, ui),
                            "Automate" => automate(app, ui),
                            "Table Design" => table_design(app, ui),
                            "Chart Design" => chart_design(app, ui),
                            crate::paper_view::TAB => crate::paper_view::ribbon_tab(app, ui),
                            _ => home(app, ui),
                        }
                    });
                });
            });
        });
}

/// The File tab opens the Backstage view (New, Open, Info, Save As, Print, Export, Options…), like
/// Excel's; the ribbon's tab and collapsed state are left as they were for when it closes.
fn file_menu(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let label = egui::RichText::new(app.ui.language.tr("File")).font(theme::ui_font(13.5)).color(t.accent);
    let file = ui.add(egui::Button::new(label).frame(false).min_size(vec2(46.0, 30.0)));
    if file.clicked() {
        app.open_backstage(crate::backstage::Page::Home);
    }
    if app.keytips.prefix() == Some("")
        && let Some(sequence) = keytip_table::tab_sequence("File")
    {
        keytip(app, ui, sequence, pos2(file.rect.center().x, file.rect.bottom() - 1.0));
    }
}

/// System / Light / Dark, as in the View tab's Display theme box (`view.theme`).
fn theme_choices(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let mode = app.ui.theme_mode();
    for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
        if ui.radio(mode == value, lang.tr(label)).clicked() {
            act(app, "view.theme", json!({"mode": value}));
            ui.close();
        }
    }
}

const CONTEXTUAL_TABS: [&str; 3] = ["Table Design", "Chart Design", crate::paper_view::TAB];

fn contextual_tabs(app: &SheetApp) -> Vec<&'static str> {
    let mut v = Vec::new();
    if let Some(d) = app.session.active()
        && let Some(sh) = d.wb.active()
    {
        if sh.table_at(d.selection.active).is_some() {
            v.push("Table Design");
        }
        if app.selected_chart.is_some_and(|id| sh.charts.iter().any(|c| c.id == id)) {
            v.push("Chart Design");
        }
        if app.grid.hf_edit.is_some() {
            v.push(crate::paper_view::TAB);
        }
    }
    v
}

fn sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 6.0), pos2(r.center().x, r.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

fn active_style(app: &SheetApp) -> Style {
    app.session
        .active()
        .and_then(|d| {
            let sh = d.wb.active()?;
            Some(d.wb.styles.get(sh.style_id(d.selection.active)).clone())
        })
        .unwrap_or_default()
}

fn theme_colors(app: &SheetApp) -> [u32; 12] {
    app.session.active().map(|d| d.wb.theme.colors).unwrap_or(gridcraft_engine::model::Theme::default().colors)
}

/// Runs a command from a ribbon click (menu-style: no params may open a dialog).
fn act(app: &mut SheetApp, id: &str, params: serde_json::Value) {
    app.run_or_alert(id, params);
}

/// Keytips are painted, not focusable widgets: the grid retains keyboard focus.
fn keytip(app: &SheetApp, ui: &Ui, sequence: &str, at: egui::Pos2) {
    let Some(rest) = app.keytips.prefix().and_then(|prefix| sequence.strip_prefix(prefix)).filter(|s| !s.is_empty()) else { return };
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("ribbon_keytips"))).with_clip_rect(ui.clip_rect());
    let text = painter.layout_no_wrap(rest.into(), theme::ui_bold(10.5), t.text);
    let rect = Rect::from_center_size(at, text.size() + vec2(7.0, 3.0));
    painter.rect_filled(rect, 2.0, t.input_bg);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
    painter.galley(rect.min + vec2(3.5, 1.5), text, t.text);
}

/// Paint the badge for a control that runs command `id`, when the armed prefix matches.
fn control_keytip(app: &SheetApp, ui: &Ui, response: &egui::Response, id: &str) {
    if let Some(sequence) = keytip_table::control_badge(app.keytips.prefix().unwrap_or_default(), id) {
        keytip(app, ui, sequence, response.rect.center_bottom());
    }
}

/// Paint the single badge for a menu anchor whose popup items run `targets`.
fn anchor_keytip(app: &SheetApp, ui: &Ui, response: &egui::Response, targets: &[&str]) {
    if let Some(sequence) = keytip_table::anchor_badge(app.keytips.prefix().unwrap_or_default(), targets) {
        keytip(app, ui, sequence, response.rect.center_bottom());
    }
}

/// The `on` value for a ribbon checkbox command. A click has already flipped the checkbox bool, so
/// it is sent as is; a key tip leaves the bool untouched, so the command gets the opposite.
fn checkbox_on(clicked: bool, shown: bool) -> bool {
    shown == clicked
}

/// Consume a one-shot keytip request for control `id` (set by `Action::Control`).
fn invoke(app: &mut SheetApp, id: &str) -> bool {
    if app.ui.keytip_invoke == Some(id) {
        app.ui.keytip_invoke = None;
        true
    } else {
        false
    }
}

/// Force-open command for a popup: either a keytip invoke (open it) or a click (toggle).
fn popup_command(forced: bool, clicked: bool) -> Option<egui::SetOpenCommand> {
    if forced { Some(egui::SetOpenCommand::Bool(true)) } else { clicked.then_some(egui::SetOpenCommand::Toggle) }
}

/// Per-control state of an [`editable_combo`]: the filter text and highlighted row.
#[derive(Clone, Default)]
struct ComboState {
    text: String,
    sel: i32,
    was_open: bool,
}

/// What the user did with an [`editable_combo`].
pub(crate) enum ComboResult {
    None,
    /// A row was applied: clicked or Enter on the highlighted row.
    Picked(String),
}

/// Excel-style dropdown: a filter field (type to narrow the list) plus arrow/Enter selection.
/// `items` are `(value, display)`; the value is what comes back. When `forced` (keytip invoke)
/// or clicked, the popup opens, focuses the filter field, and selects its text.
pub(crate) fn editable_combo(
    ui: &mut Ui,
    anchor: &egui::Response,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
    forced: bool,
) -> ComboResult {
    combo_popup(ui, anchor, salt, current_value, items, forced, true, None)
}

/// The dropdown of [`editable_combo`] without the filter field: the whole list, picked by click or
/// arrows and Enter. It drops directly below `under` and is as wide as it, like Excel's Font Size
/// list under its box.
fn list_combo(ui: &mut Ui, anchor: &egui::Response, under: Rect, salt: &'static str, current_value: &str, items: &[(&str, String)]) -> ComboResult {
    combo_popup(ui, anchor, salt, current_value, items, false, false, Some(under))
}

fn combo_popup(
    ui: &mut Ui,
    anchor: &egui::Response,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
    forced: bool,
    filter: bool,
    under: Option<Rect>,
) -> ComboResult {
    let popup_id = anchor.id.with("popup");
    let state_id = anchor.id.with(salt).with("__combo_state");
    let mut st = ui.data_mut(|d| d.get_temp::<ComboState>(state_id)).unwrap_or_default();
    let open_now = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if open_now && !st.was_open {
        st.text = String::new();
        st.sel = items.iter().position(|(v, _)| *v == current_value).map(|i| i as i32).unwrap_or(0);
        // The reset lands on the frame AFTER the popup opens, which usually has no input of
        // its own — without a repaint request the stale filtered list stays on screen.
        ui.ctx().request_repaint();
    }
    st.was_open = open_now;
    let mut result = ComboResult::None;
    let mut popup = egui::Popup::menu(anchor).open_memory(popup_command(forced, anchor.clicked()));
    if let Some(r) = under {
        popup = popup.anchor(r).align(egui::RectAlign::BOTTOM_START).width(r.width());
    }
    let row_w = under.map_or(220.0, |r| r.width() - 12.0);
    popup.show(|ui| {
        // Arrows and Enter navigate the list, not the filter field's cursor. Capture them and
        // strip them from the input so the TextEdit never sees them.
        let (mut down, mut up, mut enter) = (false, false, false);
        ui.input(|i| {
            down = i.key_pressed(egui::Key::ArrowDown);
            up = i.key_pressed(egui::Key::ArrowUp);
            enter = i.key_pressed(egui::Key::Enter);
        });
        if down || up || enter {
            ui.ctx().input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(e, egui::Event::Key { key: egui::Key::ArrowDown | egui::Key::ArrowUp | egui::Key::Enter, pressed: true, .. })
                });
            });
        }
        if filter {
            let text_id = anchor.id.with(salt).with("__combo_text");
            let te = ui.add(egui::TextEdit::singleline(&mut st.text).id(text_id).desired_width(220.0).hint_text("Type to filter…"));
            if open_now && !te.has_focus() {
                ui.ctx().memory_mut(|m| m.request_focus(text_id));
                // Select all once focus lands so typing replaces the filter.
                if let Some(mut s) = egui::text_edit::TextEditState::load(ui.ctx(), text_id) {
                    let len = st.text.chars().count();
                    s.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(len))));
                    s.store(ui.ctx(), text_id);
                }
            }
        }
        let lower = st.text.to_lowercase();
        let filtered: Vec<usize> = items.iter().enumerate().filter(|(_, (_, d))| d.to_lowercase().contains(&lower)).map(|(i, _)| i).collect();
        if !filtered.is_empty() {
            let n = filtered.len() as i32;
            st.sel = st.sel.clamp(0, n - 1);
            if down {
                st.sel = (st.sel + 1).rem_euclid(n);
            }
            if up {
                st.sel = (st.sel - 1).rem_euclid(n);
            }
        }
        if enter && !filtered.is_empty() {
            let idx = filtered[st.sel.clamp(0, filtered.len() as i32 - 1) as usize];
            result = ComboResult::Picked(items[idx].0.to_string());
        }
        if filter {
            ui.separator();
        }
        let button_h = ui.spacing().interact_size.y.max(20.0);
        let spacing = ui.spacing().item_spacing.y;
        let n = filtered.len() as f32;

        let content_h = n * button_h + (n - 1.0).max(0.0) * spacing;

        let max_rows = 9.0_f32;
        let max_h = max_rows * button_h + (max_rows - 1.0) * spacing;

        let desired_h = content_h.min(max_h);

        ui.set_min_height(desired_h);

        egui::ScrollArea::vertical().max_height(desired_h).show(ui, |ui| {
            for (pos, i) in filtered.iter().enumerate() {
                let (value, disp) = &items[*i];

                let row = ui.add(egui::Button::selectable(pos as i32 == st.sel, egui::RichText::new(disp)).min_size(vec2(row_w, 20.0)));

                if (down || up) && pos as i32 == st.sel && !ui.clip_rect().contains_rect(row.rect) {
                    row.scroll_to_me(Some(if down { egui::Align::Max } else { egui::Align::Min }));
                }

                if row.clicked() {
                    result = ComboResult::Picked(value.to_string());
                }

                if row.hovered() {
                    st.sel = pos as i32;
                }
            }
        });
        if !matches!(result, ComboResult::None) {
            ui.close();
        }
    });
    ui.data_mut(|d| d.insert_temp(state_id, st));
    result
}

/// The Accounting Number Format dropdown: label and the code Excel applies for that symbol.
const ACCOUNTING_SYMBOLS: &[(&str, &str)] = &[
    ("$ English (United States)", "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)"),
    ("£ English (United Kingdom)", "_-[$£-809]* #,##0.00_-;-[$£-809]* #,##0.00_-;_-[$£-809]* \"-\"??_-;_-@_-"),
    ("€ Euro", "_-[$€-x-euro2] * #,##0.00_-;-[$€-x-euro2] * #,##0.00_-;_-[$€-x-euro2] * \"-\"??_-;_-@_-"),
    ("¥ Japanese", "_-[$¥-411]* #,##0_-;-[$¥-411]* #,##0_-;_-[$¥-411]* \"-\"_-;_-@_-"),
    ("¥ Chinese (PRC)", "_-[$¥-804]* #,##0.00_-;-[$¥-804]* #,##0.00_-;_-[$¥-804]* \"-\"??_-;_-@_-"),
    ("₹ English (India)", "_-[$₹-4009]* #,##0.00_-;-[$₹-4009]* #,##0.00_-;_-[$₹-4009]* \"-\"??_-;_-@_-"),
    ("CHF German (Switzerland)", "_-[$CHF-807]* #,##0.00_-;-[$CHF-807]* #,##0.00_-;_-[$CHF-807]* \"-\"??_-;_-@_-"),
    ("Rp Indonesian", "_-[$Rp-421]* #,##0.00_-;-[$Rp-421]* #,##0.00_-;_-[$Rp-421]* \"-\"??_-;_-@_-"),
    ("₩ Korean", "_-[$₩-412]* #,##0_-;-[$₩-412]* #,##0_-;_-[$₩-412]* \"-\"_-;_-@_-"),
    ("R$ Portuguese (Brazil)", "_-[$R$-416]* #,##0.00_-;-[$R$-416]* #,##0.00_-;_-[$R$-416]* \"-\"??_-;_-@_-"),
    ("₽ Russian", "_-[$₽-419]* #,##0.00_-;-[$₽-419]* #,##0.00_-;_-[$₽-419]* \"-\"??_-;_-@_-"),
];

fn menu_items(app: &mut SheetApp, ui: &mut Ui, items: &[(&str, &str, serde_json::Value)]) {
    for (label, id, p) in items {
        if *label == "-" {
            ui.separator();
            continue;
        }
        let lang = app.ui.language;
        let response = ui.add(egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(220.0, 22.0)));
        if let Some(sequence) = keytip_table::item_letter(app.keytips.prefix().unwrap_or_default(), id) {
            keytip(app, ui, sequence, pos2(response.rect.right() - 10.0, response.rect.center().y));
        }
        if response.clicked() {
            if let Some(d) = id.strip_prefix("dialog:") {
                app.open_dialog(d, p.clone());
            } else if *id == "edit.paste" {
                app.paste_from_clipboard();
            } else {
                act(app, id, p.clone());
            }
        }
    }
}

fn home(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let st = active_style(app);
    // Clipboard
    let paste = big_button(ui, Icon::Paste, "Paste", "Paste", true);
    paste.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Paste"));
    for sequence in
        keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["edit.paste", "edit.pasteSpecial", "dialog:pasteSpecial"])
    {
        keytip(app, ui, sequence, paste.rect.center_bottom());
    }
    if paste.clicked() {
        app.paste_from_clipboard();
    }
    let paste_keys = matches!(app.keytips.prefix(), Some("HV" | "E"));
    // Keep a keyboard-opened menu anchored to the button when a pointer click
    // cancels keytips, so its items remain in place and can still be clicked.
    let anchor_id = paste.id.with("keytip_anchor");
    // Query response state before taking the egui context write lock.
    // Calling secondary_clicked() inside data_mut can deadlock.
    let paste_secondary_clicked = paste.secondary_clicked();
    let anchored = ui.ctx().data_mut(|data| {
        let anchored = if paste_keys {
            true
        } else if paste_secondary_clicked {
            false
        } else {
            data.get_temp::<bool>(anchor_id).unwrap_or(false)
        };
        data.insert_temp(anchor_id, anchored);
        anchored
    });
    let mut paste_popup = egui::Popup::context_menu(&paste);
    if anchored {
        paste_popup = paste_popup.anchor(&paste);
    }
    if paste_keys {
        paste_popup = paste_popup.open_memory(egui::SetOpenCommand::Bool(true));
    }
    paste_popup.show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Paste", "edit.paste", json!({})),
                ("Paste Values", "edit.pasteSpecial", json!({"what": "values"})),
                ("Paste Formulas", "edit.pasteSpecial", json!({"what": "formulas"})),
                ("Paste Formatting", "edit.pasteSpecial", json!({"what": "formats"})),
                ("Transpose", "edit.pasteSpecial", json!({"what": "all", "transpose": true})),
                ("Paste Link", "edit.pasteSpecial", json!({"what": "all", "link": true})),
                ("-", "", json!(null)),
                ("Paste Special…", "dialog:pasteSpecial", json!({})),
            ],
        );
    });
    ui.vertical(|ui| {
        let __k = icon_button(ui, Icon::Cut, t.text, "Cut", vec2(26.0, 23.0));
        control_keytip(app, ui, &__k, "edit.cut");
        if __k.clicked() {
            app.copy_to_clipboard(ui.ctx(), "edit.cut");
        }
        let __k = icon_button(ui, Icon::Copy, t.text, "Copy", vec2(26.0, 23.0));
        control_keytip(app, ui, &__k, "edit.copy");
        if __k.clicked() {
            app.copy_to_clipboard(ui.ctx(), "edit.copy");
        }
        let fp_on = app.session.format_painter.is_some();
        let fp = toggle_button(ui, Icon::Brush, fp_on, "Format Painter (double-click to keep it on)");
        fp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), fp_on, "Format Painter"));
        control_keytip(app, ui, &fp, "edit.formatPainter");
        if fp.double_clicked() {
            act(app, "edit.formatPainter", json!({"sticky": true}));
        } else if fp.clicked() {
            if fp_on {
                app.session.format_painter = None;
            } else {
                act(app, "edit.formatPainter", json!({}));
            }
        }
    });
    sep(ui);
    // Font
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            font_combo(app, ui, &st);
            size_combo(app, ui, &st);
            let __k = icon_button(ui, Icon::Plus, t.text, "Increase Font Size", vec2(22.0, 23.0));
            control_keytip(app, ui, &__k, "home.increaseFontSize");
            if __k.clicked() {
                act(app, "home.increaseFontSize", json!({}));
            }
            let __k = icon_button(ui, Icon::Minus, t.text, "Decrease Font Size", vec2(22.0, 23.0));
            control_keytip(app, ui, &__k, "home.decreaseFontSize");
            if __k.clicked() {
                act(app, "home.decreaseFontSize", json!({}));
            }
        });
        ui.horizontal(|ui| {
            let __k = toggle_button(ui, Icon::Bold, st.font.bold, "Bold");
            control_keytip(app, ui, &__k, "home.bold");
            if __k.clicked() {
                act(app, "home.bold", json!({}));
            }
            let __k = toggle_button(ui, Icon::Italic, st.font.italic, "Italic");
            control_keytip(app, ui, &__k, "home.italic");
            if __k.clicked() {
                act(app, "home.italic", json!({}));
            }
            let (u, ua) = split_button(ui, Icon::Underline, None, "Underline");
            control_keytip(app, ui, &ua, "home.underline");
            if u {
                act(app, "home.underline", json!({}));
            }
            egui::Popup::menu(&ua).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("Underline", "home.underline", json!({"style": "single"})),
                        ("Double Underline", "home.underline", json!({"style": "double"})),
                    ],
                );
            });
            let __k = toggle_button(ui, Icon::Strike, st.font.strike, "Strikethrough");
            control_keytip(app, ui, &__k, "home.strikethrough");
            if __k.clicked() {
                act(app, "home.strikethrough", json!({}));
            }
            let (b, ba) = split_button(ui, Icon::Borders, None, "Borders");
            control_keytip(app, ui, &ba, "home.borders");
            if b {
                act(app, "home.borders", json!({"preset": app.grid.last_border.clone()}));
            }
            ba.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Border presets"));
            egui::Popup::menu(&ba).show(|ui| {
                for (label, preset) in [
                    ("Bottom Border", "bottom"),
                    ("Top Border", "top"),
                    ("Left Border", "left"),
                    ("Right Border", "right"),
                    ("No Border", "none"),
                    ("All Borders", "all"),
                    ("Outside Borders", "outside"),
                    ("Thick Outside Borders", "thickOutside"),
                    ("Bottom Double Border", "doubleBottom"),
                    ("Thick Bottom Border", "thickBottom"),
                    ("Top and Bottom Border", "topBottom"),
                    ("Top and Thick Bottom Border", "topThickBottom"),
                    ("Top and Double Bottom Border", "topDoubleBottom"),
                    ("Inside Borders", "inside"),
                ] {
                    if crate::border_preview::preset_button(ui, label, preset).clicked() {
                        app.grid.last_border = preset.to_string();
                        act(app, "home.borders", json!({"preset": preset}));
                    }
                }
                ui.separator();
                if ui.button("More Borders…").clicked() {
                    app.open_dialog("formatCells", json!({"tab": "Border"}));
                }
            });
            let fill = app.grid.last_fill.clone();
            let fill_c = gridcraft_engine::model::Color::from_hex(&fill).and_then(|c| c.resolve(&Default::default())).map(theme::color32);
            let (f, fa) = split_button(ui, Icon::Fill, fill_c, "Fill Color");
            control_keytip(app, ui, &fa, "home.fillColor");
            if f {
                act(app, "home.fillColor", json!({"color": fill}));
            }
            let forced = invoke(app, "home.fillColor");
            egui::Popup::menu(&fa)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .open_memory(popup_command(forced, fa.clicked()))
                .show(|ui| {
                    if let Some(c) = color_palette(ui, &theme_colors(app), "No Fill") {
                        if c != "none" {
                            app.grid.last_fill = c.clone();
                        }
                        act(app, "home.fillColor", json!({"color": c}));
                        ui.close();
                    }
                });
            let fc = app.grid.last_font_color.clone();
            let fc_c = gridcraft_engine::model::Color::from_hex(&fc).and_then(|c| c.resolve(&Default::default())).map(theme::color32);
            let (c, ca) = split_button(ui, Icon::FontColor, fc_c, "Font Color");
            control_keytip(app, ui, &ca, "home.fontColor");
            if c {
                act(app, "home.fontColor", json!({"color": fc}));
            }
            let forced = invoke(app, "home.fontColor");
            egui::Popup::menu(&ca)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .open_memory(popup_command(forced, ca.clicked()))
                .show(|ui| {
                    if let Some(c) = color_palette(ui, &theme_colors(app), "Automatic") {
                        if c != "none" {
                            app.grid.last_font_color = c.clone();
                        }
                        act(app, "home.fontColor", json!({"color": if c == "none" { "auto".to_string() } else { c }}));
                        ui.close();
                    }
                });
        });
    });
    sep(ui);
    // Alignment
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, v, id, tip) in [
                (Icon::AlignTop, VAlign::Top, "home.alignTop", "Top Align"),
                (Icon::AlignMiddle, VAlign::Center, "home.alignMiddle", "Middle Align"),
                (Icon::AlignBottom, VAlign::Bottom, "home.alignBottom", "Bottom Align"),
            ] {
                let __k = toggle_button(ui, icon, st.align.v == v, tip);
                control_keytip(app, ui, &__k, id);
                if __k.clicked() {
                    act(app, id, json!({}));
                }
            }
            let o = small_button(ui, Icon::Orientation, "", "Orientation", true);
            control_keytip(app, ui, &o, "home.orientation");
            let forced_o = invoke(app, "home.orientation");
            egui::Popup::menu(&o).open_memory(popup_command(forced_o, o.clicked())).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("Angle Counterclockwise", "home.orientation", json!({"angle": "counterclockwise"})),
                        ("Angle Clockwise", "home.orientation", json!({"angle": "clockwise"})),
                        ("Vertical Text", "home.orientation", json!({"angle": "vertical"})),
                        ("Rotate Text Up", "home.orientation", json!({"angle": "up"})),
                        ("Rotate Text Down", "home.orientation", json!({"angle": "down"})),
                        ("Horizontal", "home.orientation", json!({"angle": 0})),
                        ("-", "", json!(null)),
                        ("Format Cell Alignment…", "dialog:formatCells", json!({"tab": "Alignment"})),
                    ],
                );
            });
            let wrap = toggle_button(ui, Icon::Wrap, st.align.wrap, "Wrap Text");
            control_keytip(app, ui, &wrap, "home.wrapText");
            if wrap.clicked() {
                act(app, "home.wrapText", json!({}));
            }
            ui.label(egui::RichText::new(app.ui.language.tr("Wrap Text")).font(theme::ui_font(12.5)));
        });
        ui.horizontal(|ui| {
            for (icon, h, id, tip) in [
                (Icon::AlignLeft, HAlign::Left, "home.alignLeft", "Align Left"),
                (Icon::AlignCenter, HAlign::Center, "home.alignCenter", "Center"),
                (Icon::AlignRight, HAlign::Right, "home.alignRight", "Align Right"),
            ] {
                let response = toggle_button(ui, icon, st.align.h == h, tip);
                for sequence in keytip_table::control_badges(app.keytips.prefix().unwrap_or_default(), id) {
                    keytip(app, ui, sequence, response.rect.center_bottom());
                }
                if response.clicked() {
                    act(app, id, json!({}));
                }
            }
            let __k = icon_button(ui, Icon::IndentDec, t.text, "Decrease Indent", vec2(24.0, 23.0));
            control_keytip(app, ui, &__k, "home.decreaseIndent");
            if __k.clicked() {
                act(app, "home.decreaseIndent", json!({}));
            }
            let __k = icon_button(ui, Icon::IndentInc, t.text, "Increase Indent", vec2(24.0, 23.0));
            control_keytip(app, ui, &__k, "home.increaseIndent");
            if __k.clicked() {
                act(app, "home.increaseIndent", json!({}));
            }
            let (m, ma) = split_button(ui, Icon::Merge, None, "Merge & Center");
            control_keytip(app, ui, &ma, "home.mergeCenter");
            if m {
                act(app, "home.mergeCenter", json!({}));
            }
            ui.label(egui::RichText::new(app.ui.language.tr("Merge & Center")).font(theme::ui_font(12.5)));
            egui::Popup::menu(&ma).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("Merge & Center", "home.mergeCenter", json!({})),
                        ("Merge Across", "home.mergeAcross", json!({})),
                        ("Merge Cells", "home.mergeCells", json!({})),
                        ("Unmerge Cells", "home.unmergeCells", json!({})),
                    ],
                );
            });
        });
    });
    sep(ui);
    // Number
    ui.vertical(|ui| {
        number_combo(app, ui, &st);
        ui.horizontal(|ui| {
            let (c, ca) = split_button(ui, Icon::Currency, None, "Accounting Number Format");
            control_keytip(app, ui, &ca, "home.accounting");
            if c {
                act(app, "home.accounting", json!({}));
            }
            egui::Popup::menu(&ca).show(|ui| {
                let mut items: Vec<_> = ACCOUNTING_SYMBOLS.iter().map(|(label, code)| (*label, "home.numberFormat", json!({"code": code}))).collect();
                items.push(("-", "", json!(null)));
                items.push(("More Accounting Formats…", "dialog:formatCells", json!({"tab": "Number"})));
                menu_items(app, ui, &items);
            });
            let __k = icon_button(ui, Icon::Percent, t.text, "Percent Style", vec2(24.0, 23.0));
            control_keytip(app, ui, &__k, "home.percent");
            if __k.clicked() {
                act(app, "home.percent", json!({}));
            }
            let __k = icon_button(ui, Icon::Comma, t.text, "Comma Style", vec2(24.0, 23.0));
            control_keytip(app, ui, &__k, "home.comma");
            if __k.clicked() {
                act(app, "home.comma", json!({}));
            }
            let __k = icon_button(ui, Icon::DecInc, t.text, "Increase Decimal", vec2(26.0, 23.0));
            control_keytip(app, ui, &__k, "home.increaseDecimal");
            if __k.clicked() {
                act(app, "home.increaseDecimal", json!({}));
            }
            let __k = icon_button(ui, Icon::DecDec, t.text, "Decrease Decimal", vec2(26.0, 23.0));
            control_keytip(app, ui, &__k, "home.decreaseDecimal");
            if __k.clicked() {
                act(app, "home.decreaseDecimal", json!({}));
            }
        });
    });
    sep(ui);
    // Styles
    let cf = big_button(ui, Icon::CondFormat, "Conditional\nFormatting", "Conditional Formatting", true);
    anchor_keytip(app, ui, &cf, &["home.conditionalFormat", "home.manageRules", "home.clearRules"]);
    egui::Popup::menu(&cf).show(|ui| cf_menu(app, ui));
    let ft = big_button(ui, Icon::FormatTable, "Format\nas Table", "Format as Table", true);
    anchor_keytip(app, ui, &ft, &["table.style"]);
    egui::Popup::menu(&ft).show(|ui| table_gallery(app, ui, "home.formatAsTable"));
    let cs = big_button(ui, Icon::CellStyles, "Cell\nStyles", "Cell Styles", true);
    anchor_keytip(app, ui, &cs, &["home.newCellStyle"]);
    egui::Popup::menu(&cs).show(|ui| cell_style_gallery(app, ui));
    sep(ui);
    // Cells
    ui.vertical(|ui| {
        let ins = small_button(ui, Icon::Insert, "Insert", "Insert", true);
        for sequence in keytip_table::group_extension(
            app.keytips.prefix().unwrap_or_default(),
            &["dialog:insertCells", "home.insertRows", "home.insertColumns", "home.insertSheet"],
        ) {
            keytip(app, ui, sequence, ins.rect.center_bottom());
        }
        egui::Popup::menu(&ins).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Insert Cells…", "dialog:insertCells", json!({})),
                    ("Insert Sheet Rows", "home.insertRows", json!({})),
                    ("Insert Sheet Columns", "home.insertColumns", json!({})),
                    ("Insert Sheet", "home.insertSheet", json!({})),
                ],
            );
        });
        let del = small_button(ui, Icon::Delete, "Delete", "Delete", true);
        for sequence in keytip_table::group_extension(
            app.keytips.prefix().unwrap_or_default(),
            &["dialog:deleteCells", "home.deleteRows", "home.deleteColumns", "home.deleteSheet"],
        ) {
            keytip(app, ui, sequence, del.rect.center_bottom());
        }
        egui::Popup::menu(&del).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Delete Cells…", "dialog:deleteCells", json!({})),
                    ("Delete Sheet Rows", "home.deleteRows", json!({})),
                    ("Delete Sheet Columns", "home.deleteColumns", json!({})),
                    ("Delete Sheet", "home.deleteSheet", json!({})),
                ],
            );
        });
        let fmt = small_button(ui, Icon::Format, "Format", "Format", true);
        for sequence in keytip_table::group_extension(
            app.keytips.prefix().unwrap_or_default(),
            &[
                "dialog:rowHeight",
                "home.autofitRowHeight",
                "dialog:columnWidth",
                "home.autofitColumnWidth",
                "dialog:defaultWidth",
                "home.hideRows",
                "home.hideColumns",
                "home.unhideRows",
                "home.unhideColumns",
                "sheet.hide",
                "sheet.unhide",
                "dialog:renameSheet",
                "dialog:moveSheet",
                "dialog:protectSheet",
                "home.lockCell",
                "dialog:formatCells",
            ],
        ) {
            keytip(app, ui, sequence, fmt.rect.center_bottom());
        }
        let format_keys = matches!(app.keytips.prefix(), Some("HO" | "OC"));
        let mut format_popup = egui::Popup::menu(&fmt);
        if format_keys {
            format_popup = format_popup.open_memory(egui::SetOpenCommand::Bool(true));
        }
        let protect_label = if app.active_sheet_protected() { "Unprotect Sheet…" } else { "Protect Sheet…" };
        format_popup.show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Row Height…", "dialog:rowHeight", json!({})),
                    ("AutoFit Row Height", "home.autofitRowHeight", json!({})),
                    ("Column Width…", "dialog:columnWidth", json!({})),
                    ("AutoFit Column Width", "home.autofitColumnWidth", json!({})),
                    ("Default Width…", "dialog:defaultWidth", json!({})),
                    ("-", "", json!(null)),
                    ("Hide Rows", "home.hideRows", json!({})),
                    ("Hide Columns", "home.hideColumns", json!({})),
                    ("Unhide Rows", "home.unhideRows", json!({})),
                    ("Unhide Columns", "home.unhideColumns", json!({})),
                    ("Hide Sheet", "sheet.hide", json!({})),
                    ("Unhide Sheet…", "sheet.unhide", json!({})),
                    ("-", "", json!(null)),
                    ("Rename Sheet", "dialog:renameSheet", json!({})),
                    ("Move or Copy Sheet…", "dialog:moveSheet", json!({})),
                    ("-", "", json!(null)),
                    (protect_label, "dialog:protectSheet", json!({})),
                    ("Lock Cell", "home.lockCell", json!({})),
                    ("Format Cells…", "dialog:formatCells", json!({})),
                ],
            );
        });
    });
    sep(ui);
    // Editing
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let (s, sa) = split_button(ui, Icon::Sum, None, "AutoSum");
            control_keytip(app, ui, &sa, "formulas.autoSum");
            if s {
                act(app, "formulas.autoSum", json!({}));
            }
            egui::Popup::menu(&sa).show(|ui| {
                for f in ["SUM", "AVERAGE", "COUNT", "MAX", "MIN"] {
                    if ui.button(f).clicked() {
                        act(app, "formulas.autoSum", json!({"function": f}));
                    }
                }
                ui.separator();
                if ui.button("More Functions…").clicked() {
                    app.open_dialog("insertFunction", json!({}));
                }
            });
        });
        let fill = small_button(ui, Icon::FillDown, "", "Fill", true);
        for sequence in keytip_table::group_extension(
            app.keytips.prefix().unwrap_or_default(),
            &["edit.fillDown", "edit.fillRight", "edit.fillUp", "edit.fillLeft", "dialog:series", "edit.flashFill"],
        ) {
            keytip(app, ui, sequence, fill.rect.center_bottom());
        }
        egui::Popup::menu(&fill).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Down", "edit.fillDown", json!({})),
                    ("Right", "edit.fillRight", json!({})),
                    ("Up", "edit.fillUp", json!({})),
                    ("Left", "edit.fillLeft", json!({})),
                    ("Series…", "dialog:series", json!({})),
                    ("Flash Fill", "edit.flashFill", json!({})),
                ],
            );
        });
        let clear = small_button(ui, Icon::Eraser, "", "Clear", true);
        for sequence in keytip_table::group_extension(
            app.keytips.prefix().unwrap_or_default(),
            &["edit.clearAll", "edit.clearFormats", "edit.clearContents", "edit.clearComments", "edit.clearHyperlinks"],
        ) {
            keytip(app, ui, sequence, clear.rect.center_bottom());
        }
        egui::Popup::menu(&clear).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Clear All", "edit.clearAll", json!({})),
                    ("Clear Formats", "edit.clearFormats", json!({})),
                    ("Clear Contents", "edit.clearContents", json!({})),
                    ("Clear Comments and Notes", "edit.clearComments", json!({})),
                    ("Clear Hyperlinks", "edit.clearHyperlinks", json!({})),
                ],
            );
        });
    });
    let sf = big_button(ui, Icon::SortFilter, "Sort &\nFilter", "Sort & Filter", true);
    for sequence in keytip_table::group_extension(
        app.keytips.prefix().unwrap_or_default(),
        &["data.sortAscending", "data.sortDescending", "dialog:sort", "data.filter", "data.clearFilter", "data.reapply"],
    ) {
        keytip(app, ui, sequence, sf.rect.center_bottom());
    }
    egui::Popup::menu(&sf).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Sort A to Z", "data.sortAscending", json!({})),
                ("Sort Z to A", "data.sortDescending", json!({})),
                ("Custom Sort…", "dialog:sort", json!({})),
                ("-", "", json!(null)),
                ("Filter", "data.filter", json!({})),
                ("Clear", "data.clearFilter", json!({})),
                ("Reapply", "data.reapply", json!({})),
            ],
        );
    });
    let fs = big_button(ui, Icon::Find, "Find &\nSelect", "Find & Select", true);
    for sequence in keytip_table::group_extension(
        app.keytips.prefix().unwrap_or_default(),
        &["dialog:find", "dialog:goTo", "dialog:goToSpecial", "edit.goToSpecial"],
    ) {
        keytip(app, ui, sequence, fs.rect.center_bottom());
    }
    egui::Popup::menu(&fs).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Find…", "dialog:find", json!({})),
                ("Replace…", "dialog:find", json!({"replace": true})),
                ("Go To…", "dialog:goTo", json!({})),
                ("Go To Special…", "dialog:goToSpecial", json!({})),
                ("-", "", json!(null)),
                ("Formulas", "edit.goToSpecial", json!({"kind": "formulas"})),
                ("Notes", "edit.goToSpecial", json!({"kind": "notes"})),
                ("Conditional Formatting", "edit.goToSpecial", json!({"kind": "conditionalFormats"})),
                ("Constants", "edit.goToSpecial", json!({"kind": "constants"})),
                ("Data Validation", "edit.goToSpecial", json!({"kind": "dataValidation"})),
            ],
        );
    });
}

fn font_combo(app: &mut SheetApp, ui: &mut Ui, st: &Style) {
    let name = st.font.name.clone();
    let resp = ui.add_sized([150.0, 24.0], egui::Button::new(egui::RichText::new(&name).font(theme::ui_font(12.5))));
    control_keytip(app, ui, &resp, "home.fontName");
    let forced = invoke(app, "home.fontName");
    let items: Vec<(&str, String)> = FONTS.iter().map(|f| (*f, f.to_string())).collect();
    if let ComboResult::Picked(value) = editable_combo(ui, &resp, "font", &name, &items, forced) {
        act(app, "home.fontName", json!({"name": value}));
    }
}

pub const FONTS: &[&str] = &[
    "Aptos",
    "Aptos Narrow",
    "Arial",
    "Calibri",
    "Cambria",
    "Candara",
    "Consolas",
    "Courier New",
    "Garamond",
    "Georgia",
    "Helvetica",
    "Helvetica Neue",
    "Segoe UI",
    "Tahoma",
    "Times New Roman",
    "Trebuchet MS",
    "Verdana",
];

/// Excel's size box: type a size and press Enter to apply exactly that number (the command refuses
/// sizes outside 1 to 409), Escape or clicking away reverts; the arrow opens the standard sizes.
fn size_combo(app: &mut SheetApp, ui: &mut Ui, st: &Style) {
    let t = Tokens::get(ui.ctx());
    let size = st.font.size;
    let current = if size.fract() == 0.0 { format!("{}", size as i32) } else { format!("{size}") };
    let field = egui::Id::new("gridcraft.font_size_box");
    let draft = field.with("draft");
    let mut text = ui.data_mut(|d| d.get_temp::<String>(draft)).unwrap_or_else(|| current.clone());
    // One frame like the Font name box beside it (same fill, border, height and corners), with the
    // text field and the dropdown arrow inside it, as in Excel.
    let (frame, _) = ui.allocate_exact_size(vec2(52.0, 24.0), egui::Sense::hover());
    // Painted once the field's focus and hover are known, but beneath the field and the arrow.
    let bg = ui.painter().add(egui::Shape::Noop);
    let text_rect = Rect::from_min_max(frame.min, egui::pos2(frame.right() - 16.0, frame.bottom()));
    let arrow_rect = Rect::from_min_max(egui::pos2(frame.right() - 16.0, frame.top() + 1.0), egui::pos2(frame.right() - 1.0, frame.bottom() - 1.0));
    let mut field_ui =
        ui.new_child(egui::UiBuilder::new().max_rect(text_rect.shrink2(vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let out = egui::TextEdit::singleline(&mut text)
        .id(field)
        .font(theme::ui_font(12.5))
        .frame(egui::Frame::NONE)
        .desired_width(text_rect.width() - 12.0)
        .show(&mut field_ui);
    let resp = out.response;
    control_keytip(app, ui, &resp, "home.fontSize");
    if invoke(app, "home.fontSize") {
        ui.memory_mut(|m| m.request_focus(field));
    }
    if resp.gained_focus() {
        // Typing replaces the size, as in Excel, whether the box was clicked or reached by keytip.
        let mut state = out.state;
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()))));
        state.store(ui.ctx(), field);
    }
    if resp.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(draft));
        // Consume the Enter so the grid behind does not also move the active cell down.
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
            && let Some(v) = text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
        {
            act(app, "home.fontSize", json!({"size": v}));
        }
    } else if resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(draft));
    }
    let mut arrow_ui = ui.new_child(egui::UiBuilder::new().max_rect(arrow_rect));
    let arrow = icon_button(&mut arrow_ui, Icon::Chevron, t.text_dim, "Font Size", arrow_rect.size());
    let visuals = ui.style().visuals.clone();
    let looks = if resp.has_focus() || ui.rect_contains_pointer(frame) { &visuals.widgets.hovered } else { &visuals.widgets.inactive };
    let stroke = if resp.has_focus() { visuals.selection.stroke } else { looks.bg_stroke };
    ui.painter().set(bg, egui::epaint::RectShape::new(frame, looks.corner_radius, looks.weak_bg_fill, stroke, egui::StrokeKind::Inside));
    arrow.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Font Size"));
    let sizes = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0];
    let labels: Vec<String> = sizes.iter().map(|s| format!("{}", *s as i32)).collect();
    let items: Vec<(&str, String)> = labels.iter().map(|s| (s.as_str(), s.clone())).collect();
    if let ComboResult::Picked(value) = list_combo(ui, &arrow, frame, "size", &current, &items)
        && let Ok(v) = value.parse::<f32>()
    {
        act(app, "home.fontSize", json!({"size": v}));
    }
}

fn number_combo(app: &mut SheetApp, ui: &mut Ui, st: &Style) {
    let code = st.num_fmt.as_str().to_string();
    let kind = gridcraft_engine::display::number_format(&code).kind();
    let current = format!("{kind:?}");
    let lang = app.ui.language;
    let sample = app.session.active().and_then(|d| d.wb.active().map(|sh| sh.value(d.selection.active))).unwrap_or_default();
    let resp = ui.add_sized([150.0, 24.0], egui::Button::new(egui::RichText::new(lang.tr(&current)).font(theme::ui_font(12.5))));
    control_keytip(app, ui, &resp, "home.numberFormat");
    let forced = invoke(app, "home.numberFormat");
    let names = ["General", "Number", "Currency", "Accounting", "Short Date", "Long Date", "Time", "Percentage", "Fraction", "Scientific", "Text"];
    let items: Vec<(&str, String)> = names
        .iter()
        .map(|n| {
            let code = gridcraft_engine::cmd::format::format_code_for(n);
            let preview = app.session.active().map(|d| gridcraft_engine::display::format(&sample, code, &d.wb).text).unwrap_or_default();
            (*n, format!("{:<12}   {preview}", lang.tr(n)))
        })
        .chain(std::iter::once(("__more", format!("{:>12}", lang.tr("More Number Formats…")))))
        .collect();
    if let ComboResult::Picked(value) = editable_combo(ui, &resp, "number", &current, &items, forced) {
        match value.as_str() {
            "__more" => app.open_dialog("formatCells", json!({"tab": "Number"})),
            n => act(app, "home.numberFormat", json!({"format": n})),
        }
    }
}

fn cf_menu(app: &mut SheetApp, ui: &mut Ui) {
    ui.menu_button("Highlight Cells Rules", |ui| {
        for (label, op) in [("Greater Than…", "greater"), ("Less Than…", "less"), ("Between…", "between"), ("Equal To…", "equal")] {
            if ui.button(label).clicked() {
                app.open_dialog("cfQuick", json!({"type": "cellIs", "operator": op, "title": label}));
            }
        }
        if ui.button("Text that Contains…").clicked() {
            app.open_dialog("cfQuick", json!({"type": "containsText", "title": "Text that Contains"}));
        }
        if ui.button("A Date Occurring…").clicked() {
            app.open_dialog("cfQuick", json!({"type": "timePeriod", "title": "A Date Occurring"}));
        }
        if ui.button("Duplicate Values…").clicked() {
            act(app, "home.conditionalFormat", json!({"rule": {"type": "duplicate"}}));
        }
    });
    ui.menu_button("Top/Bottom Rules", |ui| {
        menu_items(
            app,
            ui,
            &[
                ("Top 10 Items", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10}})),
                ("Top 10%", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "percent": true}})),
                ("Bottom 10 Items", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "bottom": true}})),
                ("Bottom 10%", "home.conditionalFormat", json!({"rule": {"type": "top10", "rank": 10, "bottom": true, "percent": true}})),
                ("Above Average", "home.conditionalFormat", json!({"rule": {"type": "aboveAverage"}})),
                ("Below Average", "home.conditionalFormat", json!({"rule": {"type": "aboveAverage", "below": true}})),
            ],
        );
    });
    ui.menu_button("Data Bars", |ui| {
        for (label, c) in
            [("Blue", "#638EC6"), ("Green", "#63BE7B"), ("Red", "#F8696B"), ("Orange", "#FFB628"), ("Light Blue", "#008AEF"), ("Purple", "#D6007B")]
        {
            if ui.button(label).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "dataBar", "color": c}}));
            }
        }
    });
    ui.menu_button("Color Scales", |ui| {
        for (label, cols) in [
            ("Green - Yellow - Red", vec!["#63BE7B", "#FFEB84", "#F8696B"]),
            ("Red - Yellow - Green", vec!["#F8696B", "#FFEB84", "#63BE7B"]),
            ("Green - White - Red", vec!["#63BE7B", "#FCFCFF", "#F8696B"]),
            ("Blue - White - Red", vec!["#5A8AC6", "#FCFCFF", "#F8696B"]),
            ("White - Green", vec!["#FCFCFF", "#63BE7B"]),
            ("White - Red", vec!["#FCFCFF", "#F8696B"]),
        ] {
            if ui.button(label).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "colorScale", "colors": cols}}));
            }
        }
    });
    ui.menu_button("Icon Sets", |ui| {
        for (label, set) in [
            ("3 Arrows", "3Arrows"),
            ("3 Traffic Lights", "3TrafficLights1"),
            ("3 Symbols", "3Symbols"),
            ("4 Ratings", "4Rating"),
            ("5 Arrows", "5Arrows"),
            ("5 Quarters", "5Quarters"),
        ] {
            if ui.button(label).clicked() {
                act(app, "home.conditionalFormat", json!({"rule": {"type": "iconSet", "set": set}}));
            }
        }
    });
    ui.separator();
    if ui.button("New Rule…").clicked() {
        app.open_dialog("cfQuick", json!({"type": "expression", "title": "New Formatting Rule"}));
    }
    ui.menu_button("Clear Rules", |ui| {
        menu_items(
            app,
            ui,
            &[
                ("Clear Rules from Selected Cells", "home.clearRules", json!({})),
                ("Clear Rules from Entire Sheet", "home.clearRules", json!({"sheet": true})),
            ],
        );
    });
    if ui.button("Manage Rules…").clicked() {
        app.open_dialog("manageRules", json!({}));
    }
}

/// A gallery of table styles drawn as mini tables.
fn table_gallery(app: &mut SheetApp, ui: &mut Ui, cmd: &str) {
    ui.set_width(430.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    for (fam, n) in [("Light", 21u32), ("Medium", 28), ("Dark", 11)] {
        ui.label(egui::RichText::new(fam).strong());
        egui::Grid::new(("tg", fam)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for i in 1..=n {
                let name = format!("TableStyle{fam}{i}");
                let (r, resp) = ui.allocate_exact_size(vec2(54.0, 38.0), Sense::click());
                paint_table_swatch(ui, r, &wb, &name);
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.on_hover_text(&name).clicked() {
                    act(app, cmd, json!({"style": name}));
                    ui.close();
                }
                if i % 7 == 0 {
                    ui.end_row();
                }
            }
        });
    }
}

fn paint_table_swatch(ui: &Ui, r: Rect, wb: &gridcraft_engine::model::Workbook, style: &str) {
    let lk = gridcraft_engine::tables::look(style);
    let res = |c: Option<gridcraft_engine::model::Color>| c.and_then(|c| c.resolve(&wb.theme)).map(theme::color32);
    let p = ui.painter();
    p.rect_filled(r, 0.0, Color32::WHITE);
    let rows = 5;
    let rh = r.height() / rows as f32;
    for i in 0..rows {
        let rr = Rect::from_min_size(pos2(r.left(), r.top() + i as f32 * rh), vec2(r.width(), rh));
        let fill = if i == 0 {
            res(lk.header_fill)
        } else if i % 2 == 1 {
            res(lk.band_fill)
        } else {
            res(lk.body_fill)
        };
        if let Some(f) = fill {
            p.rect_filled(rr, 0.0, f);
        }
        if let Some(l) = res(lk.line) {
            p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, l));
        }
        for k in 0..3 {
            let x = rr.left() + 4.0 + k as f32 * (r.width() - 8.0) / 3.0;
            let col = if i == 0 { res(Some(lk.header_font)).unwrap_or(Color32::BLACK) } else { Color32::from_gray(120) };
            p.line_segment([pos2(x, rr.center().y), pos2(x + 10.0, rr.center().y)], Stroke::new(1.0, col));
        }
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(210)), StrokeKind::Inside);
}

fn cell_style_gallery(app: &mut SheetApp, ui: &mut Ui) {
    ui.set_width(520.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let groups: &[(&str, &[&str])] = &[
        ("Good, Bad and Neutral", &["Normal", "Bad", "Good", "Neutral"]),
        ("Data and Model", &["Calculation", "Check Cell", "Explanatory Text", "Input", "Linked Cell", "Note", "Output", "Warning Text"]),
        ("Titles and Headings", &["Heading 1", "Heading 2", "Heading 3", "Heading 4", "Title", "Total"]),
        (
            "Themed Cell Styles",
            &[
                "20% - Accent1",
                "20% - Accent2",
                "20% - Accent3",
                "20% - Accent4",
                "20% - Accent5",
                "20% - Accent6",
                "40% - Accent1",
                "40% - Accent2",
                "40% - Accent3",
                "40% - Accent4",
                "40% - Accent5",
                "40% - Accent6",
                "60% - Accent1",
                "60% - Accent2",
                "60% - Accent3",
                "60% - Accent4",
                "60% - Accent5",
                "60% - Accent6",
                "Accent1",
                "Accent2",
                "Accent3",
                "Accent4",
                "Accent5",
                "Accent6",
            ],
        ),
        ("Number Format", &["Comma", "Comma [0]", "Currency", "Currency [0]", "Percent"]),
    ];
    for (g, names) in groups {
        ui.label(egui::RichText::new(*g).strong());
        egui::Grid::new(("csg", *g)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for (i, n) in names.iter().enumerate() {
                let st = gridcraft_engine::cmd::format::builtin_cell_style(n, &wb.theme).unwrap_or_default();
                let (r, resp) = ui.allocate_exact_size(vec2(82.0, 24.0), Sense::click());
                let fill = st
                    .fill
                    .fg
                    .resolve(&wb.theme)
                    .filter(|_| st.fill.pattern != gridcraft_engine::model::PatternType::None)
                    .map(theme::color32)
                    .unwrap_or(Color32::WHITE);
                ui.painter().rect_filled(r, 0.0, fill);
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(210)), StrokeKind::Inside);
                if !st.borders.bottom.is_none() {
                    let c = st.borders.bottom.color.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::BLACK);
                    ui.painter().line_segment(
                        [r.left_bottom() + vec2(0.0, -1.5), r.right_bottom() + vec2(0.0, -1.5)],
                        Stroke::new(st.borders.bottom.style.width(), c),
                    );
                }
                let col = st.font.color.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::BLACK);
                let fam = theme::cell_family(&st.font.name, st.font.bold, st.font.italic);
                ui.painter().text(
                    r.left_center() + vec2(4.0, 0.0),
                    Align2::LEFT_CENTER,
                    *n,
                    egui::FontId::new((st.font.size).clamp(10.0, 15.0), fam),
                    col,
                );
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.clicked() {
                    act(app, "home.cellStyle", json!({"name": n}));
                    ui.close();
                }
                if (i + 1) % 6 == 0 {
                    ui.end_row();
                }
            }
        });
    }
}

fn insert(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::PivotTable, "PivotTable", "PivotTable", false);
    control_keytip(app, ui, &__k, "insert.pivotTable");
    if __k.clicked() {
        act(app, "insert.pivotTable", json!({}));
    }
    let __k = big_button(ui, Icon::Table, "Table", "Table", false);
    control_keytip(app, ui, &__k, "insert.table");
    if __k.clicked() {
        act(app, "insert.table", json!({}));
    }
    sep(ui);
    let pic = big_button(ui, Icon::Picture, "Pictures", "Insert a picture", true);
    anchor_keytip(app, ui, &pic, &["insert.picture"]);
    pic.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Pictures"));
    egui::Popup::menu(&pic).show(|ui| {
        for (label, placement) in [("Place in Cell", "cell"), ("Place over Cells", "overCells")] {
            if ui.button(label).clicked() {
                app.open_dialog("insertPicture", json!({"placement": placement}));
                ui.close();
            }
        }
    });
    let shapes = big_button(ui, Icon::Shapes, "Shapes", "Shapes", true);
    anchor_keytip(app, ui, &shapes, &["insert.shape"]);
    egui::Popup::menu(&shapes).show(|ui| {
        for (label, kind) in [
            ("Rectangle", "rectangle"),
            ("Rounded Rectangle", "roundedRectangle"),
            ("Oval", "ellipse"),
            ("Triangle", "triangle"),
            ("Line", "line"),
            ("Arrow", "arrow"),
            ("Text Box", "textBox"),
        ] {
            if ui.button(label).clicked() {
                act(app, "insert.shape", json!({"kind": kind}));
            }
        }
    });
    let icn = big_button(ui, Icon::Theme, "Icons", "Insert an icon", true);
    anchor_keytip(app, ui, &icn, &["insert.icons"]);
    egui::Popup::menu(&icn).show(|ui| {
        ui.set_width(260.0);
        egui::Grid::new("icon_lib").show(ui, |ui| {
            for (i, (name, icon)) in icons::LIBRARY.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, Tokens::get(ui.ctx()).hover);
                }
                icons::paint(ui.painter(), r.shrink(6.0), *icon, Tokens::get(ui.ctx()).text);
                if resp.on_hover_text(*name).clicked() {
                    act(app, "insert.icons", json!({"name": name}));
                    ui.close();
                }
                if (i + 1) % 6 == 0 {
                    ui.end_row();
                }
            }
        });
    });
    sep(ui);
    let __k = big_button(ui, Icon::Chart, "Recommended\nCharts", "Recommended Charts", false);
    control_keytip(app, ui, &__k, "insert.recommendedCharts");
    if __k.clicked() {
        // Menu-style: with no chart type, run_or_alert opens the Insert Chart dialog.
        act(app, "insert.recommendedCharts", json!({}));
    }
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, tip, cid, subs) in [
                (
                    Icon::ChartBar,
                    "Column or Bar",
                    "insert.chart.column",
                    vec![
                        ("Clustered Column", "column", ""),
                        ("Stacked Column", "column", "stacked"),
                        ("100% Stacked Column", "column", "stacked100"),
                        ("Clustered Bar", "bar", ""),
                        ("Stacked Bar", "bar", "stacked"),
                    ],
                ),
                (
                    Icon::ChartLine,
                    "Line or Area",
                    "insert.chart.line",
                    vec![
                        ("Line", "line", ""),
                        ("Line with Markers", "line", "markers"),
                        ("Stacked Line", "line", "stacked"),
                        ("Area", "area", ""),
                        ("Stacked Area", "area", "stacked"),
                    ],
                ),
                (Icon::ChartPie, "Pie or Doughnut", "insert.chart.pie", vec![("Pie", "pie", ""), ("Doughnut", "doughnut", "")]),
            ] {
                let r = small_button(ui, icon, "", tip, true);
                control_keytip(app, ui, &r, cid);
                let forced_c = invoke(app, cid);
                egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            for (icon, tip, cid, subs) in [
                (
                    Icon::ChartScatter,
                    "Scatter or Bubble",
                    "insert.chart.scatter",
                    vec![("Scatter", "scatter", ""), ("Scatter with Lines", "scatter", "lines"), ("Bubble", "bubble", "")],
                ),
                (Icon::Chart, "Statistic", "insert.chart.statistic", vec![("Histogram", "histogram", ""), ("Box & Whisker", "boxWhisker", "")]),
                (
                    Icon::Theme,
                    "Hierarchy & more",
                    "insert.chart.hierarchy",
                    vec![
                        ("Treemap", "treemap", ""),
                        ("Sunburst", "sunburst", ""),
                        ("Waterfall", "waterfall", ""),
                        ("Funnel", "funnel", ""),
                        ("Radar", "radar", ""),
                        ("Stock", "stock", ""),
                        ("Combo", "combo", ""),
                    ],
                ),
            ] {
                let r = small_button(ui, icon, "", tip, true);
                control_keytip(app, ui, &r, cid);
                let forced_c = invoke(app, cid);
                egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
    });
    sep(ui);
    if big_button(ui, Icon::Filter, "Slicer", "Insert a slicer to filter a table", false).clicked() {
        act(app, "insert.slicer", json!({}));
    }
    sep(ui);
    let sp = big_button(ui, Icon::Sparkline, "Sparklines", "Sparklines", true);
    anchor_keytip(app, ui, &sp, &["dialog:sparkline"]);
    egui::Popup::menu(&sp).show(|ui| {
        for (label, kind) in [("Line", "line"), ("Column", "column"), ("Win/Loss", "winLoss")] {
            if ui.button(label).clicked() {
                app.open_dialog("sparkline", json!({"type": kind}));
            }
        }
    });
    sep(ui);
    // Cell Controls.
    if big_button(ui, Icon::Checkbox, "Checkbox", "Insert checkboxes in the selected cells", false).clicked() {
        act(app, "insert.checkbox", json!({}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::Link, "Link", "Insert Link", false);
    control_keytip(app, ui, &__k, "dialog:insertLink");
    if __k.clicked() {
        app.open_dialog("insertLink", json!({}));
    }
    let __k = big_button(ui, Icon::Comment, "Comment", "New Comment", false);
    control_keytip(app, ui, &__k, "dialog:comment");
    if __k.clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::TextBox, "Text\nBox", "Text Box", false);
    control_keytip(app, ui, &__k, "insert.textBox");
    if __k.clicked() {
        act(app, "insert.textBox", json!({}));
    }
    let __k = big_button(ui, Icon::PageLayout, "Header &\nFooter", "Header & Footer", false);
    control_keytip(app, ui, &__k, "dialog:headerFooter");
    if __k.clicked() {
        // As in Excel: Page Layout view, editing the header in place (the dialog is under Page Setup).
        crate::paper_view::start_editing(app);
    }
    let sym = big_button(ui, Icon::Symbol, "Symbol", "Symbol", true);
    control_keytip(app, ui, &sym, "insert.symbol");
    egui::Popup::menu(&sym).show(|ui| {
        ui.set_width(260.0);
        ui.horizontal_wrapped(|ui| {
            for ch in [
                "€", "£", "¥", "©", "®", "™", "±", "≠", "≤", "≥", "÷", "×", "∞", "µ", "α", "β", "π", "Σ", "Ω", "°", "•", "…", "→", "←", "↑", "↓",
                "✓", "★", "½", "¼",
            ] {
                if ui.button(ch).clicked() {
                    act(app, "insert.symbol", json!({"char": ch}));
                }
            }
        });
    });
}

fn draw(app: &mut SheetApp, ui: &mut Ui) {
    let tool = app.session.draw_tool.clone();
    let __k = big_button(ui, Icon::Shapes, "Select\nObjects", "Select and move ink and shapes", false);
    control_keytip(app, ui, &__k, "draw.lasso");
    if __k.clicked() {
        act(app, "draw.lasso", json!({}));
    }
    sep(ui);
    for (label, color) in [("Pen\nBlue", "#1F5FC9"), ("Pen\nBlack", "#000000"), ("Pen\nRed", "#D13B2F"), ("Highlighter", "#F2E33A")] {
        let on = tool == "pen" && app.session.draw_color.eq_ignore_ascii_case(color);
        let r = big_button(ui, Icon::Pen, label, "Draw with ink", false);
        if on {
            ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
        }
        if r.clicked() {
            let width = if label == "Highlighter" { 12.0 } else { 2.0 };
            act(app, "draw.pen", json!({"on": !on, "color": color, "width": width}));
        }
    }
    let er = big_button(ui, Icon::Eraser, "Eraser", "Erase ink strokes", false);
    if tool == "eraser" {
        ui.painter().rect_stroke(er.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if er.clicked() {
        act(app, "draw.eraser", json!({}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::Shapes, "Ink to\nShape", "Convert the last ink stroke to a shape", false);
    control_keytip(app, ui, &__k, "draw.inkToShape");
    if __k.clicked() {
        act(app, "draw.inkToShape", json!({}));
    }
}

fn page_layout(app: &mut SheetApp, ui: &mut Ui) {
    let th = big_button(ui, Icon::Theme, "Themes", "Themes", true);
    anchor_keytip(app, ui, &th, &["pageLayout.theme"]);
    egui::Popup::menu(&th).show(|ui| {
        for (name, colors) in gridcraft_engine::cmd::view::themes() {
            ui.horizontal(|ui| {
                for c in colors.iter().skip(4).take(6) {
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 1.0, Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8));
                }
                if ui.button(name).clicked() {
                    act(app, "pageLayout.theme", json!({"name": name}));
                }
            });
        }
    });
    sep(ui);
    let m = big_button(ui, Icon::Margins, "Margins", "Margins", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["pageLayout.margins", "dialog:pageSetup"]) {
        keytip(app, ui, sequence, m.rect.center_bottom());
    }
    egui::Popup::menu(&m).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Normal", "pageLayout.margins", json!({"preset": "normal"})),
                ("Wide", "pageLayout.margins", json!({"preset": "wide"})),
                ("Narrow", "pageLayout.margins", json!({"preset": "narrow"})),
                ("Custom Margins…", "dialog:pageSetup", json!({})),
            ],
        );
    });
    let o = big_button(ui, Icon::Orient, "Page Layout|Orientation", "Page Layout|Orientation", true);
    control_keytip(app, ui, &o, "pageLayout.orientation");
    egui::Popup::menu(&o).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Portrait", "pageLayout.orientation", json!({"orientation": "portrait"})),
                ("Landscape", "pageLayout.orientation", json!({"orientation": "landscape"})),
            ],
        );
    });
    let s = big_button(ui, Icon::Paper, "Size", "Paper Size", true);
    control_keytip(app, ui, &s, "pageLayout.size");
    egui::Popup::menu(&s).show(|ui| {
        // B4/B5/B6 are the JIS sizes Japanese Excel uses; the ISO ones stay selectable by name.
        for p in [
            "Letter",
            "Legal",
            "Tabloid",
            "Executive",
            "A3",
            "A4",
            "A5",
            "A6",
            "B4",
            "B5",
            "B6",
            "Japanese Postcard",
            "Japanese Double Postcard",
            "Japanese Envelope #3",
            "Japanese Envelope #4",
            "Japanese Envelope Kaku #2",
            "Japanese Envelope Kaku #3",
            "B4 (ISO)",
            "B5 (ISO)",
            "B6 (ISO)",
        ] {
            if ui.button(p).clicked() {
                act(app, "pageLayout.size", json!({"paper": p}));
            }
        }
    });
    let pa = big_button(ui, Icon::PrintArea, "Print\nArea", "Print Area", true);
    anchor_keytip(app, ui, &pa, &["pageLayout.printArea"]);
    egui::Popup::menu(&pa).show(|ui| {
        menu_items(
            app,
            ui,
            &[("Set Print Area", "pageLayout.printArea", json!({})), ("Clear Print Area", "pageLayout.printArea", json!({"clear": true}))],
        );
    });
    let br = big_button(ui, Icon::PageBreak, "Breaks", "Page Breaks", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["pageLayout.breaks"]) {
        keytip(app, ui, sequence, br.rect.center_bottom());
    }
    egui::Popup::menu(&br).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Insert Page Break", "pageLayout.breaks", json!({"insert": true})),
                ("Remove Page Break", "pageLayout.breaks", json!({"remove": true})),
                ("Reset All Page Breaks", "pageLayout.breaks", json!({"reset": true})),
            ],
        );
    });
    let __k = big_button(ui, Icon::Sheet, "Print\nTitles", "Print Titles", false);
    control_keytip(app, ui, &__k, "pageLayout.printTitles");
    if __k.clicked() {
        app.open_dialog("pageSetup", json!({"tab": "Sheet"}));
    }
    sep(ui);
    let sh = app
        .session
        .active()
        .and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings, s.print.gridlines, s.print.headings)))
        .unwrap_or((true, true, false, false));
    // These two checkboxes mean the screen view vs. the printed page. English shows "View" (as in
    // Excel), but the key is distinct from the "View" tab so languages can word them apart.
    let screen = app.ui.language.tr("Sheet Options|View");
    let print = app.ui.language.tr("Print");
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(app.ui.language.tr("Gridlines")).strong().small());
        let mut v = sh.0;
        let resp = ui.checkbox(&mut v, screen);
        control_keytip(app, ui, &resp, "view.gridlines");
        if resp.changed() || invoke(app, "view.gridlines") {
            act(app, "view.gridlines", json!({"on": checkbox_on(resp.changed(), v)}));
        }
        let mut p = sh.2;
        let resp = ui.checkbox(&mut p, print);
        control_keytip(app, ui, &resp, "pageLayout.printGridlines");
        if resp.changed() || invoke(app, "pageLayout.printGridlines") {
            act(app, "pageLayout.printGridlines", json!({"on": checkbox_on(resp.changed(), p)}));
        }
    });
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(app.ui.language.tr("Headings")).strong().small());
        let mut v = sh.1;
        let resp = ui.checkbox(&mut v, screen);
        control_keytip(app, ui, &resp, "view.headings");
        if resp.changed() || invoke(app, "view.headings") {
            act(app, "view.headings", json!({"on": checkbox_on(resp.changed(), v)}));
        }
        let mut p = sh.3;
        let resp = ui.checkbox(&mut p, print);
        control_keytip(app, ui, &resp, "pageLayout.printHeadings");
        if resp.changed() || invoke(app, "pageLayout.printHeadings") {
            act(app, "pageLayout.printHeadings", json!({"on": checkbox_on(resp.changed(), p)}));
        }
    });
    let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
    let r = big_button(ui, Icon::SheetRtl, "Sheet\nRight-to-Left", "Sheet Right-to-Left", false);
    if rtl {
        let t = Tokens::get(ui.ctx());
        ui.painter().rect_stroke(r.rect, 5.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    }
    if r.clicked() {
        act(app, "view.rightToLeft", json!({"on": !rtl}));
    }
}

/// The Formulas tab: Insert Function, AutoSum, the function-category menus (names in the formula
/// language, descriptions in the interface language), defined names, auditing and calculation.
fn formulas(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::Function, "Insert\nFunction", "Insert Function (⇧F3)", false);
    control_keytip(app, ui, &__k, "dialog:insertFunction");
    if __k.clicked() {
        app.open_dialog("insertFunction", json!({}));
    }
    let s = big_button(ui, Icon::Sum, "AutoSum", "AutoSum", true);
    control_keytip(app, ui, &s, "formulas.autoSum");
    if s.clicked() {
        act(app, "formulas.autoSum", json!({}));
    }
    for (label, cat, cid) in [
        ("Financial", "Financial", "formulas.financial"),
        ("Logical", "Logical", "formulas.logical"),
        ("Text", "Text", "formulas.text"),
        ("Date &\nTime", "DateTime", "formulas.dateTime"),
        ("Lookup &\nReference", "Lookup", "formulas.lookup"),
        ("Math &\nTrig", "MathTrig", "formulas.mathTrig"),
        ("More\nFunctions", "Statistical", "formulas.moreFunctions"),
    ] {
        let r = big_button(ui, Icon::Book, label, label, true);
        control_keytip(app, ui, &r, cid);
        let forced_c = invoke(app, cid);
        egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
            ui.set_max_height(420.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for f in gridcraft_engine::cmd::formulas::function_list() {
                    if f["category"].as_str() == Some(cat)
                        && let Some(n) = f["name"].as_str()
                    {
                        let locale = app.ui.language.formula_locale();
                        let name =
                            app.session.active().map(|d| crate::formula_locale::completion_name(n, locale, &d.wb, d.wb.active_sheet)).unwrap_or(n);
                        let desc = crate::formula_locale::description(n, locale, app.ui.language).unwrap_or_default();
                        if ui.button(name).on_hover_text(desc).clicked() {
                            app.run_or_alert("formulas.insertFunction", json!({"name": n}));
                            ui.close();
                        }
                    }
                }
            });
        });
    }
    sep(ui);
    let __k = big_button(ui, Icon::Name, "Name\nManager", "Name Manager", false);
    control_keytip(app, ui, &__k, "dialog:nameManager");
    if __k.clicked() {
        app.open_dialog("nameManager", json!({}));
    }
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Name, "Define Name", "Define Name", false);
        control_keytip(app, ui, &__k, "dialog:defineName");
        if __k.clicked() {
            app.open_dialog("defineName", json!({}));
        }
        let use_in = small_button(ui, Icon::Function, "Use in Formula", "Use in Formula", true);
        anchor_keytip(app, ui, &use_in, &["formulas.pasteName"]);
        egui::Popup::menu(&use_in).show(|ui| {
            let names: Vec<String> = app.session.active().map(|d| d.wb.names.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
            for n in names {
                if ui.button(&n).clicked() {
                    let cur = app.editor.as_ref().map(|e| e.text.clone()).unwrap_or_else(|| "=".into());
                    app.begin_edit(Some(format!("{cur}{n}")), false);
                }
            }
        });
        let __k = small_button(ui, Icon::Table, "Create from Selection", "Create from Selection", false);
        control_keytip(app, ui, &__k, "formulas.createFromSelection");
        if __k.clicked() {
            act(app, "formulas.createFromSelection", json!({}));
        }
    });
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::Trace, "Trace Precedents", "Trace Precedents", false).clicked()
            && let Ok(r) = app.run("formulas.tracePrecedents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Trace, "Trace Dependents", "Trace Dependents", false).clicked()
            && let Ok(r) = app.run("formulas.traceDependents", json!({}))
        {
            app.grid.trace = Some(r);
        }
        if small_button(ui, Icon::Close, "Remove Arrows", "Remove Arrows", false).clicked() {
            app.grid.trace = None;
        }
    });
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Fx, "Show Formulas", "Show Formulas (⌃`)", false);
        control_keytip(app, ui, &__k, "formulas.showFormulas");
        if __k.clicked() {
            act(app, "formulas.showFormulas", json!({}));
        }
        let __k = small_button(ui, Icon::Validation, "Error Checking", "Error Checking", false);
        control_keytip(app, ui, &__k, "dialog:errorChecking");
        if __k.clicked() {
            app.open_dialog("errorChecking", json!({}));
        }
        let __k = small_button(ui, Icon::Calc, "Evaluate Formula", "Evaluate Formula", false);
        control_keytip(app, ui, &__k, "dialog:evaluateFormula");
        if __k.clicked() {
            app.open_dialog("evaluateFormula", json!({}));
        }
    });
    if big_button(ui, Icon::Search, "Watch\nWindow", "Watch Window", false).clicked() {
        app.grid.pane = Some("watch".into());
    }
    sep(ui);
    let co = big_button(ui, Icon::Calc, "Calculation\nOptions", "Calculation Options", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["formulas.calculationOptions"]) {
        keytip(app, ui, sequence, co.rect.center_bottom());
    }
    egui::Popup::menu(&co).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Automatic", "formulas.calculationOptions", json!({"mode": "automatic"})),
                ("Automatic Except for Data Tables", "formulas.calculationOptions", json!({"mode": "automaticExceptTables"})),
                ("Manual", "formulas.calculationOptions", json!({"mode": "manual"})),
            ],
        );
    });
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Calc, "Calculate Now", "Calculate Now (F9)", false);
        control_keytip(app, ui, &__k, "formulas.calculateNow");
        if __k.clicked() {
            act(app, "formulas.calculateNow", json!({}));
        }
        let __k = small_button(ui, Icon::Sheet, "Calculate Sheet", "Calculate Sheet (⇧F9)", false);
        control_keytip(app, ui, &__k, "formulas.calculateSheet");
        if __k.clicked() {
            act(app, "formulas.calculateSheet", json!({}));
        }
    });
}

fn data(app: &mut SheetApp, ui: &mut Ui) {
    let gd = big_button(ui, Icon::Folder, "Get Data\n(Text/CSV)", "Import a text or CSV file", false);
    control_keytip(app, ui, &gd, "data.getData");
    if gd.clicked() {
        app.open_dialog("open", json!({}));
    }
    sep(ui);
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::SortAsc, "", "Sort A to Z", false);
        control_keytip(app, ui, &__k, "data.sortAscending");
        if __k.clicked() {
            act(app, "data.sortAscending", json!({}));
        }
        let __k = small_button(ui, Icon::SortDesc, "", "Sort Z to A", false);
        control_keytip(app, ui, &__k, "data.sortDescending");
        if __k.clicked() {
            act(app, "data.sortDescending", json!({}));
        }
    });
    let __k = big_button(ui, Icon::SortFilter, "Sort", "Custom Sort", false);
    control_keytip(app, ui, &__k, "dialog:sort");
    if __k.clicked() {
        app.open_dialog("sort", json!({}));
    }
    let __k = big_button(ui, Icon::Filter, "Filter", "Filter", false);
    control_keytip(app, ui, &__k, "data.filter");
    if __k.clicked() {
        act(app, "data.filter", json!({}));
    }
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Eraser, "Clear", "Clear Filter", false);
        control_keytip(app, ui, &__k, "data.clearFilter");
        if __k.clicked() {
            act(app, "data.clearFilter", json!({}));
        }
        let __k = small_button(ui, Icon::Redo, "Reapply", "Reapply", false);
        control_keytip(app, ui, &__k, "data.reapply");
        if __k.clicked() {
            act(app, "data.reapply", json!({}));
        }
        let __k = small_button(ui, Icon::Filter, "Advanced", "Advanced Filter", false);
        control_keytip(app, ui, &__k, "dialog:advancedFilter");
        if __k.clicked() {
            app.open_dialog("advancedFilter", json!({}));
        }
    });
    sep(ui);
    let __k = big_button(ui, Icon::TextColumns, "Text to\nColumns", "Text to Columns", false);
    control_keytip(app, ui, &__k, "dialog:textToColumns");
    if __k.clicked() {
        app.open_dialog("textToColumns", json!({}));
    }
    let __k = big_button(ui, Icon::FillDown, "Flash\nFill", "Flash Fill", false);
    control_keytip(app, ui, &__k, "edit.flashFill");
    if __k.clicked() {
        act(app, "edit.flashFill", json!({}));
    }
    let __k = big_button(ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", false);
    control_keytip(app, ui, &__k, "dialog:removeDuplicates");
    if __k.clicked() {
        app.open_dialog("removeDuplicates", json!({}));
    }
    let dv = big_button(ui, Icon::Validation, "Data\nValidation", "Data Validation", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["data.validation", "data.circleInvalid"]) {
        keytip(app, ui, sequence, dv.rect.center_bottom());
    }
    egui::Popup::menu(&dv).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Data Validation…", "dialog:dataValidation", json!({})),
                ("Circle Invalid Data", "data.circleInvalid", json!({})),
                ("Clear Validation", "data.validation", json!({"clear": true})),
            ],
        );
    });
    sep(ui);
    let wi = big_button(ui, Icon::Calc, "What-If\nAnalysis", "What-If Analysis", true);
    for sequence in
        keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["dialog:goalSeek", "data.scenarioManager", "data.dataTable"])
    {
        keytip(app, ui, sequence, wi.rect.center_bottom());
    }
    egui::Popup::menu(&wi).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Goal Seek…", "dialog:goalSeek", json!({})),
                ("Scenario Manager…", "data.scenarioManager", json!({})),
                ("Data Table…", "data.dataTable", json!({})),
            ],
        );
    });
    if big_button(ui, Icon::ChartLine, "Forecast\nSheet", "Forecast Sheet", false).clicked() {
        app.open_dialog("forecastSheet", json!({}));
    }
    sep(ui);
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Group, "Group", "Group", false);
        control_keytip(app, ui, &__k, "data.group");
        if __k.clicked() {
            act(app, "data.group", json!({}));
        }
        let __k = small_button(ui, Icon::Ungroup, "Ungroup", "Ungroup", false);
        control_keytip(app, ui, &__k, "data.ungroup");
        if __k.clicked() {
            act(app, "data.ungroup", json!({}));
        }
        let __k = small_button(ui, Icon::Subtotal, "Subtotal", "Subtotal", false);
        control_keytip(app, ui, &__k, "dialog:subtotal");
        if __k.clicked() {
            app.open_dialog("subtotal", json!({}));
        }
    });
    ui.vertical(|ui| {
        if small_button(ui, Icon::Plus, "Show Detail", "Show Detail", false).clicked() {
            act(app, "data.showDetail", json!({}));
        }
        if small_button(ui, Icon::Minus, "Hide Detail", "Hide Detail", false).clicked() {
            act(app, "data.hideDetail", json!({}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::ChartBar, "Data\nAnalysis", "Data Analysis", false).clicked() {
        app.open_dialog("analysisToolPak", json!({}));
    }
    sep(ui);
    if big_button(ui, Icon::Function, "Solver", "Solver", false).clicked() {
        app.open_dialog("solver", json!({}));
    }
}

fn review(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::Spell, "Spelling", "Spelling (F7)", false);
    control_keytip(app, ui, &__k, "dialog:spelling");
    if __k.clicked() {
        app.open_dialog("spelling", json!({}));
    }
    let __k = big_button(ui, Icon::Calc, "Workbook\nStatistics", "Workbook Statistics", false);
    control_keytip(app, ui, &__k, "statistics");
    if __k.clicked() {
        app.open_dialog("statistics", json!({}));
    }
    let __k = big_button(ui, Icon::Validation, "Check\nAccessibility", "Check Accessibility", false);
    control_keytip(app, ui, &__k, "dialog:accessibility");
    if __k.clicked() {
        app.open_dialog("accessibility", json!({}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::Comment, "New\nComment", "New Comment", false);
    control_keytip(app, ui, &__k, "dialog:comment");
    if __k.clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    let __k = big_button(ui, Icon::Delete, "Delete", "Delete Comment", false);
    control_keytip(app, ui, &__k, "review.deleteComment");
    if __k.clicked() {
        act(app, "review.deleteComment", json!({}));
    }
    ui.vertical(|ui| {
        for (icon, label, tip, id) in
            [(Icon::Left, "Previous", "Previous Comment", "review.previousComment"), (Icon::Right, "Next", "Next Comment", "review.nextComment")]
        {
            if small_button(ui, icon, label, tip, false).clicked() {
                act(app, id, json!({}));
                app.grid.ensure_visible = true;
            }
        }
    });
    let shown = app.session.comments_pane;
    let r = big_button(ui, Icon::Comment, "Show\nComments", "Comments pane", false);
    if shown {
        ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if r.clicked() {
        act(app, "review.showComments", json!({"on": !shown}));
        if app.session.comments_pane {
            app.grid.pane = None;
        }
    }
    let notes = big_button(ui, Icon::Note, "Notes", "Notes", true);
    anchor_keytip(app, ui, &notes, &["review.nextComment"]);
    egui::Popup::menu(&notes).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("New Note", "dialog:comment", json!({"threaded": false})),
                ("Show/Hide Note", "review.showNote", json!({})),
                ("Show All Notes", "review.showAllNotes", json!({})),
                ("-", "", json!({})),
                ("Convert to Comments", "review.convertToComments", json!({})),
            ],
        );
    });
    sep(ui);
    let protected = app.session.active().and_then(|d| d.wb.active().map(|s| s.is_protected())).unwrap_or(false);
    let __k = big_button(ui, Icon::Lock, if protected { "Unprotect\nSheet" } else { "Protect\nSheet" }, "Protect Sheet", false);
    control_keytip(app, ui, &__k, "dialog:protectSheet");
    if __k.clicked() {
        app.open_dialog("protectSheet", json!({}));
    }
    let __k = big_button(ui, Icon::Book, "Protect\nWorkbook", "Protect Workbook structure", false);
    control_keytip(app, ui, &__k, "review.protectWorkbook");
    if __k.clicked() {
        crate::backstage::protect_workbook(app);
    }
    if big_button(ui, Icon::Validation, "Allow Edit\nRanges", "Allow Edit Ranges", false).clicked() {
        app.open_dialog("allowEditRanges", json!({}));
    }
}

fn view(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let _ = t;
    for (icon, label, cmd) in [
        (Icon::Normal, "Normal", "view.normal"),
        (Icon::PageBreak, "Page Break\nPreview", "view.pageBreakPreview"),
        (Icon::PageLayout, "Page\nLayout", "view.pageLayout"),
    ] {
        let __k = big_button(ui, icon, label, label, false);
        control_keytip(app, ui, &__k, cmd);
        if __k.clicked() {
            act(app, cmd, json!({}));
        }
    }
    sep(ui);
    let s = app.session.active().and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings))).unwrap_or((true, true));
    let lang = app.ui.language;
    let page_layout =
        app.session.active().and_then(|d| d.wb.active().map(|s| s.view == gridcraft_engine::model::SheetViewMode::PageLayout)).unwrap_or(false);
    // Two columns of two, as in Excel's Show group: four stacked rows overflow the ribbon body.
    ui.vertical(|ui| {
        // Rulers exist only in Page Layout view.
        let mut ruler = !app.session.hide_ruler;
        let ruler_resp = ui.add_enabled(page_layout, egui::Checkbox::new(&mut ruler, lang.tr("Ruler")));
        control_keytip(app, ui, &ruler_resp, "view.ruler");
        if ruler_resp.changed() {
            act(app, "view.ruler", json!({"on": ruler}));
        }
        let mut g = s.0;
        let resp = ui.checkbox(&mut g, lang.tr("Gridlines"));
        control_keytip(app, ui, &resp, "view.gridlines");
        if resp.changed() {
            act(app, "view.gridlines", json!({"on": g}));
        }
    });
    ui.add_space(6.0);
    ui.vertical(|ui| {
        let mut fb = app.session.prefs.formula_bar;
        let resp = ui.checkbox(&mut fb, lang.tr("Formula Bar"));
        control_keytip(app, ui, &resp, "view.formulaBar");
        if resp.changed() || invoke(app, "view.formulaBar") {
            act(app, "view.formulaBar", json!({"on": checkbox_on(resp.changed(), fb)}));
        }
        let mut h = s.1;
        let resp = ui.checkbox(&mut h, lang.tr("Headings"));
        control_keytip(app, ui, &resp, "view.headings");
        if resp.changed() {
            act(app, "view.headings", json!({"on": h}));
        }
    });
    sep(ui);
    let __k = big_button(ui, Icon::Zoom, "Zoom", "Zoom…", false);
    control_keytip(app, ui, &__k, "dialog:zoom");
    if __k.clicked() {
        app.open_dialog("zoom", json!({}));
    }
    let __k = big_button(ui, Icon::Search, "100%", "Zoom to 100%", false);
    control_keytip(app, ui, &__k, "view.zoom");
    if __k.clicked() || invoke(app, "view.zoom") {
        act(app, "view.zoom", json!({"percent": 100}));
    }
    let __k = big_button(ui, Icon::Zoom, "Zoom to\nSelection", "Zoom to Selection", false);
    control_keytip(app, ui, &__k, "view.zoomToSelection");
    if __k.clicked() {
        let r = app.grid.cells_rect.unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0)));
        act(app, "view.zoomToSelection", json!({"viewWidth": r.width(), "viewHeight": r.height()}));
    }
    sep(ui);
    let fp = big_button(ui, Icon::Freeze, "Freeze\nPanes", "Freeze Panes", true);
    for sequence in
        keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["view.freezePanes", "view.freezeTopRow", "view.freezeFirstColumn"])
    {
        keytip(app, ui, sequence, fp.rect.center_bottom());
    }
    egui::Popup::menu(&fp).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Freeze Panes", "view.freezePanes", json!({})),
                ("Freeze Top Row", "view.freezeTopRow", json!({})),
                ("Freeze First Column", "view.freezeFirstColumn", json!({})),
                ("Unfreeze Panes", "view.unfreezePanes", json!({})),
            ],
        );
    });
    // Switch Windows lists the open workbooks (Excel's View > Window > Switch Windows).
    let sw = big_button(ui, Icon::Book, "Switch\nWindows", "Switch Windows", true);
    sw.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Switch Windows"));
    control_keytip(app, ui, &sw, "view.switchWindows");
    let forced = invoke(app, "view.switchWindows");
    egui::Popup::menu(&sw).open_memory(popup_command(forced, sw.clicked())).show(|ui| {
        // Many open workbooks scroll rather than run off the screen.
        ui.set_max_height(420.0);
        egui::ScrollArea::vertical().show(ui, |ui| switch_windows_menu(app, ui));
    });
    ui.vertical(|ui| {
        ui.label(lang.tr("Display theme"));
        let mode = app.ui.theme_mode();
        let label = match mode {
            "system" => "System",
            "dark" => "Dark",
            _ => "Light",
        };
        egui::ComboBox::from_id_salt("display_theme").selected_text(lang.tr(label)).width(88.0).show_ui(ui, |ui| {
            for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                if ui.selectable_label(mode == value, lang.tr(label)).clicked() {
                    act(app, "view.theme", json!({"mode": value}));
                }
            }
        });
        let mut animations = app.ui.animations;
        let hint = lang.tr("Glide the selection outline to the cell you move to instead of jumping there");
        if ui.checkbox(&mut animations, lang.tr("Animations")).on_hover_text(hint).changed() {
            act(app, "view.animations", json!({"on": animations}));
        }
    });
    sep(ui);
    // Interface size (issue #230): scales the whole window on top of the desktop's scale.
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(lang.tr("Interface size")).font(theme::ui_font(11.5)).color(t.text_dim));
        let current = app.ui.interface_scale;
        let text = |v: u16| if v == 100 { lang.tr("Auto").to_string() } else { format!("{v}%") };
        egui::ComboBox::from_id_salt("ui_interface_size")
            .width(120.0)
            .selected_text(egui::RichText::new(text(current)).font(theme::ui_font(12.5)))
            .show_ui(ui, |ui| {
                for v in [100, 75, 90, 110, 125, 150, 175, 200] {
                    if ui.selectable_label(v == current, text(v)).clicked() {
                        act(app, "view.interfaceScale", json!({"percent": v}));
                    }
                }
            });
    });
    // Interface language: settings live in the View tab, like Excel's Options. The label follows
    // the current language so it stays readable; the choices are always shown in their own script.
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(lang.tr("Interface language")).font(theme::ui_font(11.5)).color(t.text_dim));
        egui::ComboBox::from_id_salt("ui_language").width(120.0).selected_text(egui::RichText::new(lang.name()).font(theme::ui_font(12.5))).show_ui(
            ui,
            |ui| {
                for l in crate::i18n::Language::ALL {
                    if ui.selectable_label(l == lang, l.name()).clicked() {
                        act(app, "app.language.set", json!({"language": l.code()}));
                    }
                }
            },
        );
    });
}

/// The Switch Windows drop-down: the visible open workbooks in `window.list` order, numbered as
/// Excel numbers them, the active one checked and an unsaved one marked as the title bar marks it.
/// An entry activates its workbook by document id, so a closed window never shifts the target.
fn switch_windows_menu(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let windows = match app.session.run("window.list", json!({})) {
        Ok(list) => list.as_array().cloned().unwrap_or_default(),
        Err(e) => {
            ui.label(crate::clean_error(&e));
            return;
        }
    };
    let flag = |w: &serde_json::Value, key: &str| w.get(key).and_then(serde_json::Value::as_bool).unwrap_or(false);
    let mut chosen = None;
    for (n, w) in windows.iter().filter(|w| !flag(w, "hidden")).enumerate() {
        let title = w.get("title").and_then(serde_json::Value::as_str).unwrap_or("");
        let active = flag(w, "active");
        let label = format!("{} {}{}", n.saturating_add(1), title, if flag(w, "dirty") { " •" } else { "" });
        // The leading blanks keep a column clear for the check mark of the active book.
        let response = ui.add(egui::Button::new(format!("        {label}")).frame_when_inactive(false).min_size(vec2(220.0, 22.0)));
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label.clone()));
        if active {
            let at = pos2(response.rect.left() + 11.0, response.rect.center().y);
            icons::paint(ui.painter(), Rect::from_center_size(at, vec2(12.0, 12.0)), Icon::Check, t.text);
        }
        if response.clicked() {
            chosen = w.get("id").cloned();
        }
    }
    if let Some(id) = chosen {
        act(app, "window.activate", json!({"id": id}));
    }
}

fn automate(app: &mut SheetApp, ui: &mut Ui) {
    if big_button(ui, Icon::Script, "Macros", "Macros and scripts in this workbook (Alt+F8)", false).clicked() {
        app.open_dialog("macros", json!({}));
    }
    let __k = big_button(ui, Icon::Script, "Command\nPalette", "Search and run any command", false);
    control_keytip(app, ui, &__k, "commandSearch");
    if __k.clicked() {
        app.open_dialog("commandSearch", json!({}));
    }
    let __k = big_button(ui, Icon::Script, "Agent\nControl", "How agents drive GridCraft (MCP / control channel)", false);
    control_keytip(app, ui, &__k, "agents");
    if __k.clicked() {
        app.open_dialog("agents", json!({}));
    }
    let __k = big_button(ui, Icon::Script, "Action\nJournal", "Every command run in this session (replayable)", false);
    control_keytip(app, ui, &__k, "journal");
    if __k.clicked() {
        app.open_dialog("journal", json!({}));
    }
    let __k = big_button(ui, Icon::Script, "About\nGridCraft", "Version, contributors and the AI models that helped", false);
    control_keytip(app, ui, &__k, "about");
    if __k.clicked() {
        app.open_dialog("about", json!({}));
    }
}

fn table_design(app: &mut SheetApp, ui: &mut Ui) {
    let Some((tname, header, totals, banded, banded_c, first, last, filter, style)) = app.session.active().and_then(|d| {
        let sh = d.wb.active()?;
        let t = sh.table_at(d.selection.active)?;
        Some((t.name.clone(), t.header_row, t.totals_row, t.banded_rows, t.banded_cols, t.first_col, t.last_col, t.filter_button, t.style.clone()))
    }) else {
        return;
    };
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Table Name:").small());
        let field = egui::Id::new(("gridcraft.table_name", &tname));
        if let Some(name) = crate::widgets::committed_text(ui, field, &tname, Some(120.0))
            && name != tname
        {
            act(app, "table.rename", json!({"table": tname, "name": name}));
        }
    });
    let __k = big_button(ui, Icon::Table, "Resize\nTable", "Resize Table", false);
    control_keytip(app, ui, &__k, "dialog:tableResize");
    if __k.clicked() {
        app.open_dialog("tableResize", json!({}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", false);
    control_keytip(app, ui, &__k, "data.removeDuplicates");
    if __k.clicked() {
        act(app, "data.removeDuplicates", json!({}));
    }
    let __k = big_button(ui, Icon::Table, "Convert\nto Range", "Convert to Range", false);
    control_keytip(app, ui, &__k, "table.convertToRange");
    if __k.clicked() {
        act(app, "table.convertToRange", json!({"table": tname}));
    }
    sep(ui);
    if big_button(ui, Icon::Filter, "Insert\nSlicer", "Insert Slicer", false).clicked() {
        act(app, "table.insertSlicer", json!({"table": tname}));
    }
    sep(ui);
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            ("Header Row", "table.headerRow", header && filter),
            ("Total Row", "table.totalRow", totals),
            ("Banded Rows", "table.bandedRows", banded),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, label).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            ("First Column", "table.firstColumn", first),
            ("Last Column", "table.lastColumn", last),
            ("Banded Columns", "table.bandedColumns", banded_c),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, label).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    sep(ui);
    table_style_strip(app, ui, &tname, &style);
}

/// The first page of Excel's Table Styles gallery, drawn in the ribbon. The More arrow opens the
/// full gallery (Light, Medium, Dark) as a popup.
const RIBBON_TABLE_STYLES: [&str; 7] =
    ["TableStyleLight1", "TableStyleLight2", "TableStyleLight3", "TableStyleLight4", "TableStyleLight5", "TableStyleLight6", "TableStyleLight7"];

fn table_style_strip(app: &mut SheetApp, ui: &mut Ui, tname: &str, current: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        // Borrow the workbook only while painting; the session must be free for `act` afterwards.
        let clicked = {
            let Some(d) = app.session.active() else { return };
            let wb = &d.wb;
            let mut clicked = None;
            for name in RIBBON_TABLE_STYLES {
                let (r, resp) = ui.allocate_exact_size(vec2(52.0, 40.0), Sense::click());
                paint_table_swatch(ui, r, wb, name);
                if name == current {
                    ui.painter().rect_stroke(r.expand(2.5), 2.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
                }
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.0), 2.0, Stroke::new(2.0, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.on_hover_text(name).clicked() {
                    clicked = Some(name);
                }
            }
            clicked
        };
        if let Some(name) = clicked {
            act(app, "table.style", json!({"table": tname, "style": name}));
        }
        let more = icon_button(ui, Icon::Chevron, t.text_dim, "More Table Styles", vec2(18.0, 40.0));
        anchor_keytip(app, ui, &more, &["table.style"]);
        egui::Popup::menu(&more).show(|ui| table_gallery(app, ui, "table.style"));
    });
}

fn chart_design(app: &mut SheetApp, ui: &mut Ui) {
    let Some(id) = app.selected_chart else { return };
    let ae = big_button(ui, Icon::Chart, "Add Chart\nElement", "Add Chart Element", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["chart.set"]) {
        keytip(app, ui, sequence, ae.rect.center_bottom());
    }
    egui::Popup::menu(&ae).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Legend: Bottom", "chart.set", json!({"chart": id, "legend": "bottom"})),
                ("Legend: Right", "chart.set", json!({"chart": id, "legend": "right"})),
                ("Legend: Top", "chart.set", json!({"chart": id, "legend": "top"})),
                ("Legend: None", "chart.set", json!({"chart": id, "legend": "none"})),
                ("-", "", json!(null)),
                ("Data Labels: Show", "chart.set", json!({"chart": id, "dataLabels": true})),
                ("Data Labels: None", "chart.set", json!({"chart": id, "dataLabels": false})),
                ("Gridlines: Show", "chart.set", json!({"chart": id, "gridlines": true})),
                ("Gridlines: None", "chart.set", json!({"chart": id, "gridlines": false})),
            ],
        );
    });
    let __k = big_button(ui, Icon::Redo, "Switch\nRow/Column", "Switch Row/Column", false);
    control_keytip(app, ui, &__k, "chart.switchRowColumn");
    if __k.clicked() {
        act(app, "chart.switchRowColumn", json!({"chart": id}));
    }
    if big_button(ui, Icon::Table, "Select\nData", "Select Data", false).clicked() {
        app.open_dialog("selectData", json!({"chart": id}));
    }
    let ct = big_button(ui, Icon::ChartBar, "Change\nChart Type", "Change Chart Type", true);
    for sequence in keytip_table::group_extension(
        app.keytips.prefix().unwrap_or_default(),
        &["column", "bar", "line", "area", "pie", "doughnut", "scatter", "radar", "waterfall", "funnel", "treemap"],
    ) {
        keytip(app, ui, sequence, ct.rect.center_bottom());
    }
    egui::Popup::menu(&ct).show(|ui| {
        for (label, k, s) in [
            ("Clustered Column", "column", ""),
            ("Stacked Column", "column", "stacked"),
            ("Bar", "bar", ""),
            ("Line", "line", "markers"),
            ("Area", "area", ""),
            ("Pie", "pie", ""),
            ("Doughnut", "doughnut", ""),
            ("Scatter", "scatter", ""),
            ("Radar", "radar", ""),
            ("Waterfall", "waterfall", ""),
            ("Funnel", "funnel", ""),
            ("Treemap", "treemap", ""),
        ] {
            if ui.button(label).clicked() {
                act(app, "chart.set", json!({"chart": id, "type": k, "subtype": s}));
            }
        }
    });
    if big_button(ui, Icon::Settings, "Format\nPane", "Format Chart pane", false).clicked() {
        app.grid.pane = Some("formatChart".into());
    }
    let __k = big_button(ui, Icon::TextBox, "Chart\nTitle", "Edit chart title", false);
    control_keytip(app, ui, &__k, "chartTitle");
    if __k.clicked() {
        app.open_dialog("chartTitle", json!({"chart": id}));
    }
    let __k = big_button(ui, Icon::Delete, "Delete\nChart", "Delete chart", false);
    control_keytip(app, ui, &__k, "chart.delete");
    if __k.clicked() {
        act(app, "chart.delete", json!({"chart": id}));
        app.selected_chart = None;
    }
}

/// Command-key shortcuts when the grid has focus.
pub fn shortcut(app: &mut SheetApp, key: Key, m: Modifiers) {
    let shift = m.shift;
    let id: Option<(&str, serde_json::Value)> = match key {
        Key::B => Some(("home.bold", json!({}))),
        Key::I => Some(("home.italic", json!({}))),
        Key::U => Some(("home.underline", json!({}))),
        Key::Z if shift => Some(("edit.redo", json!({}))),
        Key::Z => Some(("edit.undo", json!({}))),
        Key::Y => Some(("edit.redo", json!({}))),
        Key::A => Some(("edit.selectAll", json!({}))),
        Key::D => Some(("edit.fillDown", json!({}))),
        Key::R => Some(("edit.fillRight", json!({}))),
        Key::S if shift => Some(("file.saveAs", json!({}))),
        Key::S => Some(("file.save", json!({}))),
        Key::O => Some(("file.open", json!({}))),
        Key::N => Some(("file.new", json!({}))),
        Key::W => Some(("file.close", json!({}))),
        Key::P => {
            app.print_now(json!({}));
            None
        }
        Key::K => {
            app.open_dialog("insertLink", json!({}));
            None
        }
        Key::F if shift => Some(("data.filter", json!({}))),
        Key::F => {
            app.open_dialog("find", json!({}));
            None
        }
        Key::H => {
            app.open_dialog("find", json!({"replace": true}));
            None
        }
        Key::G => {
            app.open_dialog("goTo", json!({}));
            None
        }
        Key::T if shift => Some(("formulas.autoSum", json!({}))),
        Key::T => Some(("insert.table", json!({}))),
        Key::E => Some(("edit.flashFill", json!({}))),
        Key::Num1 => {
            app.open_dialog("formatCells", json!({}));
            None
        }
        Key::Num9 if shift => Some(("home.unhideRows", json!({}))),
        Key::Num9 => Some(("home.hideRows", json!({}))),
        Key::Num0 if shift => Some(("home.unhideColumns", json!({}))),
        Key::Num0 => Some(("home.hideColumns", json!({}))),
        // As in Excel: with whole rows or whole columns selected, Ctrl+Shift+= / Ctrl+- insert or
        // delete them at once; any other selection asks which way to shift.
        Key::Minus => match whole_lines(app) {
            Some(true) => Some(("home.deleteRows", json!({}))),
            Some(false) => Some(("home.deleteColumns", json!({}))),
            None => {
                app.open_dialog("deleteCells", json!({}));
                None
            }
        },
        // Insert works on one area at a time, so several areas still go through the dialog.
        Key::Plus | Key::Equals if shift => match whole_lines(app).filter(|_| single_area(app)) {
            Some(true) => Some(("home.insertRows", json!({}))),
            Some(false) => Some(("home.insertColumns", json!({}))),
            None => {
                app.open_dialog("insertCells", json!({}));
                None
            }
        },
        Key::Semicolon => {
            // Insert today's date.
            let text = app
                .session
                .active()
                .map(|d| {
                    let today = gridcraft_engine::core::date::serial_in_system(d.wb.date_system, gridcraft_engine::calc::now_serial()).floor();
                    gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(today), "m/d/yyyy", &d.wb).text
                })
                .unwrap_or_default();
            app.begin_edit(Some(text), false);
            None
        }
        // Windows Ctrl+`; on the Mac Cmd+` cycles windows (the native Window menu) and Show
        // Formulas is Control+` (`keymap`), as in Excel for Mac.
        Key::Backtick if !m.mac_cmd => Some(("formulas.showFormulas", json!({}))),
        _ => None,
    };
    if let Some((id, p)) = id {
        app.run_or_alert(id, p);
        app.grid.ensure_visible = true;
    }
}

#[allow(dead_code)]
fn unused(_: &Tokens, _: Icon) {
    let _ = icons::GREEN;
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

    #[test]
    fn no_room_draws_nothing_but_does_not_crash() {
        let (_, w, _) = layout_title("Book1.xlsx", true, 0.0);
        assert!(w <= 20.0, "a zero-width slot leaves at most the ellipsis: {w}");
    }
}

#[cfg(test)]
mod checkbox_on_tests {
    use super::*;

    #[test]
    fn click_sends_the_new_checkbox_value_and_key_tip_the_opposite() {
        // Click: the checkbox bool already holds the new choice.
        assert!(!checkbox_on(true, false));
        assert!(checkbox_on(true, true));
        // Key tip: the bool still holds the current setting, so toggle it.
        assert!(checkbox_on(false, false));
        assert!(!checkbox_on(false, true));
    }
}

/// Whether the selection is a single area.
fn single_area(app: &SheetApp) -> bool {
    app.session.active().is_some_and(|d| d.selection.ranges.len() == 1)
}

/// `Some(true)` when every selected area is whole rows, `Some(false)` when every one is whole
/// columns, `None` otherwise (including the whole sheet).
fn whole_lines(app: &SheetApp) -> Option<bool> {
    let sel = &app.session.active()?.selection;
    let rows = sel.ranges.iter().all(|r| r.is_full_rows() && !r.is_full_cols());
    let cols = sel.ranges.iter().all(|r| r.is_full_cols() && !r.is_full_rows());
    if rows {
        Some(true)
    } else if cols {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod accounting_symbol_tests {
    use super::*;
    use gridcraft_engine::core::{DateSystem, Value};
    use gridcraft_engine::numfmt::{NumberFormat, format_value};

    #[test]
    fn accounting_dropdown_offers_the_issue_currencies_with_excel_codes() {
        // #1063: label, Excel's [$symbol-LCID] tag, and 1234.5 as Excel shows it (fill and the
        // `_-` padding aside). The yen and won have no minor unit, so their codes show no decimals.
        for (label, tag, shown) in [
            ("Rp Indonesian", "[$Rp-421]", "Rp1,234.50"),
            ("₹ English (India)", "[$₹-4009]", "₹1,234.50"),
            ("₩ Korean", "[$₩-412]", "₩1,235"),
            ("¥ Chinese (PRC)", "[$¥-804]", "¥1,234.50"),
            ("¥ Japanese", "[$¥-411]", "¥1,235"),
        ] {
            let code = ACCOUNTING_SYMBOLS.iter().find(|(l, _)| *l == label).map(|(_, c)| *c);
            let Some(code) = code else { panic!("{label} is not in the Accounting dropdown") };
            assert!(code.split(';').take(3).all(|section| section.contains(tag)), "{label}: {code}");
            let out = format_value(&Value::Number(1234.5), &NumberFormat::parse(code), DateSystem::D1900);
            assert_eq!(out.text.trim(), shown, "{label}");
            let symbol = tag.trim_start_matches("[$").split('-').next().unwrap_or_default();
            assert_eq!(out.fill.map(|(c, at)| (c, out.text[..at].trim_start())), Some((' ', symbol)), "{label}");
        }
    }
}
