//! File tab Backstage view (#174): a full-window page that replaces the ribbon, formula bar, grid
//! and sheet tabs until Esc or the back arrow returns to the sheet.
//!
//! A navigation column on the left picks a page (Home, New, Open, Info, Save As, Print, Export,
//! Options, Account); Save and Close act at once. Every action runs an existing command through
//! [`SheetApp::run`] or an existing dialog through [`SheetApp::open_dialog`]; the state here is
//! UI-only and never saved. Recent workbooks are listed by path, with when each was last changed:
//! that is asked of the file system on a thread of its own, never while drawing, and only the
//! files' metadata is read. No workbook is opened until the user picks one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Align2, Color32, Key, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n::{self, Language};
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

mod options;
mod print;

use options::{options, platform};
use print::print;

/// Width of the navigation column.
const NAV_W: f32 = 212.0;
/// Width of the content column (pages are left-aligned like a document, not stretched).
const CONTENT_W: f32 = 1160.0;

/// A Backstage page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    New,
    Open,
    Info,
    SaveAs,
    Print,
    Export,
    Options,
    Account,
}

impl Page {
    pub const ALL: [Page; 9] = [Page::Home, Page::New, Page::Open, Page::Info, Page::SaveAs, Page::Print, Page::Export, Page::Options, Page::Account];

    /// The id `ui.backstage` and `ui.dialog {"name": "backstage", "page": …}` take.
    pub fn id(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::New => "new",
            Page::Open => "open",
            Page::Info => "info",
            Page::SaveAs => "saveAs",
            Page::Print => "print",
            Page::Export => "export",
            Page::Options => "options",
            Page::Account => "account",
        }
    }

    pub fn parse(id: &str) -> Option<Page> {
        let id = id.trim();
        Page::ALL.into_iter().find(|p| p.id().eq_ignore_ascii_case(id))
    }

    /// English label (navigation entry and page heading); translated when drawn.
    pub fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::New => "New",
            Page::Open => "Open",
            Page::Info => "Info",
            Page::SaveAs => "Save As",
            Page::Print => "Print",
            Page::Export => "Export",
            Page::Options => "Options",
            Page::Account => "Account",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Page::Home => Icon::Home,
            Page::New => Icon::NewFile,
            Page::Open => Icon::OpenFolder,
            Page::Info => Icon::Info,
            Page::SaveAs => Icon::Save,
            Page::Print => Icon::Print,
            Page::Export => Icon::Share,
            Page::Options => Icon::Settings,
            Page::Account => Icon::Info,
        }
    }
}

/// Document properties shown on the Info page: (`file.properties` key, label).
const PROPERTIES: [(&str, &str); 6] = [
    ("title", "Title"),
    ("subject", "Subject"),
    ("author", "Author"),
    ("company", "Company"),
    ("keywords", "Keywords"),
    ("description", "Description"),
];

/// The open Backstage view.
#[derive(Clone, Debug)]
pub struct Backstage {
    pub page: Page,
    /// Info: property drafts while a field is being edited (committed when it loses focus).
    props: [String; 6],
    /// Print: the whole workbook rather than the active sheet.
    pub print_all: bool,
    /// Print: fit each sheet on one page.
    pub fit_to_page: bool,
    /// Print: the previewed page (0-based).
    preview_page: usize,
    /// Print: the preview for (document, revision, whole workbook, fit to page).
    preview: Option<((u64, u64, bool, bool), Json)>,
    /// Save As: the chosen file type (an extension from [`SAVE_TYPES`]).
    pub save_format: &'static str,
    /// Home and Open: when the recent workbooks were last changed.
    recent: RecentFiles,
}

/// Save As file types: (extension, label). The first is the workbook itself; the others are
/// exports that leave the workbook's own file and unsaved state alone.
pub const SAVE_TYPES: [(&str, &str); 4] =
    [("xlsx", "Excel Workbook (*.xlsx)"), ("csv", "CSV UTF-8 (Comma delimited) (*.csv)"), ("html", "Web Page (*.html)"), ("pdf", "PDF (*.pdf)")];

impl Backstage {
    pub fn new(page: Page) -> Backstage {
        Backstage {
            page,
            props: Default::default(),
            print_all: false,
            fit_to_page: false,
            preview_page: 0,
            preview: None,
            save_format: "xlsx",
            recent: RecentFiles::default(),
        }
    }
}

/// What a click asked for, applied after drawing.
#[derive(Default)]
struct Outcome {
    close: bool,
    page: Option<Page>,
}

/// Draws the Backstage view in place of the ribbon, formula bar, grid and sheet tabs.
pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let Some(mut bs) = app.backstage.take() else { return };
    let ctx = ui.ctx().clone();
    // Esc inside a text field leaves the field; it closes the view only from outside one.
    let typing = ctx.text_edit_focused();
    let t = Tokens::get(&ctx);
    let lang = app.ui.language;
    let mut out = Outcome::default();
    // A calm column on the window colour; the open page is a white pill with an accent bar.
    let (nav_bg, nav_text, nav_sel) = (t.window, t.text, t.ribbon);
    egui::Panel::left("backstage_nav")
        .show_separator_line(false)
        .exact_size(NAV_W)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE.fill(nav_bg).inner_margin(egui::Margin { left: 0, right: 0, top: 10, bottom: 12 }))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if nav_item(ui, Icon::Back, lang.tr("Back"), false, nav_text, nav_sel).clicked() {
                out.close = true;
            }
            ui.add_space(10.0);
            for page in [Page::Home, Page::New, Page::Open] {
                if nav_item(ui, page.icon(), lang.tr(page.label()), bs.page == page, nav_text, nav_sel).clicked() {
                    out.page = Some(page);
                }
            }
            nav_separator(ui, nav_text);
            if nav_item(ui, Page::Info.icon(), lang.tr("Info"), bs.page == Page::Info, nav_text, nav_sel).clicked() {
                out.page = Some(Page::Info);
            }
            if nav_item(ui, Icon::Save, lang.tr("Save"), false, nav_text, nav_sel).clicked() {
                save(app, &mut out);
            }
            for page in [Page::SaveAs, Page::Print, Page::Export] {
                if nav_item(ui, page.icon(), lang.tr(page.label()), bs.page == page, nav_text, nav_sel).clicked() {
                    out.page = Some(page);
                }
            }
            if nav_item(ui, Icon::Close, lang.tr("Close"), false, nav_text, nav_sel).clicked() {
                // A cell still being edited is part of the workbook; Close keeps the unsaved prompt.
                if app.commit_edit(0, 0, false, false) {
                    app.run_or_alert("file.close", json!({}));
                    out.close = true;
                }
            }
            // Bottom up: Account sits just above Options, as in Excel.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                for page in [Page::Options, Page::Account] {
                    if nav_item(ui, page.icon(), lang.tr(page.label()), bs.page == page, nav_text, nav_sel).clicked() {
                        out.page = Some(page);
                    }
                }
            });
        });
    // The page sits on a white card with window-coloured gutters, its content centred.
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 0, right: 8, top: 0, bottom: 8 })).show(
        ui,
        |ui| {
            let card = ui.max_rect();
            ui.painter().rect_filled(card, 12.0, t.ribbon);
            ui.painter().rect_stroke(card, 12.0, Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                // The left gutter is a layout offset, not a `Margin` (whose i8 would cap it at
                // 127 px and pull the column off centre in wide windows).
                let side = ((ui.available_width() - CONTENT_W) / 2.0).max(40.0);
                let mut column = ui.available_rect_before_wrap();
                column.min.x += side;
                ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                    egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 32, top: 40, bottom: 40 }).show(ui, |ui| {
                        ui.set_max_width(CONTENT_W);
                        let title = if bs.page == Page::Home { greeting(lang) } else { lang.tr(bs.page.label()).to_string() };
                        ui.label(RichText::new(title).font(theme::ui_bold(26.0)).color(t.text));
                        if bs.page == Page::Home {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(lang.tr("Start a new workbook or pick up where you left off."))
                                    .font(theme::ui_font(13.5))
                                    .color(t.text_dim),
                            );
                            ui.add_space(10.0);
                            tip_line(ui, lang);
                        }
                        ui.add_space(22.0);
                        match bs.page {
                            Page::Home => home(app, ui, &mut bs, &mut out),
                            Page::New => new_page(app, ui, &mut out),
                            Page::Open => open_page(app, ui, &mut bs, &mut out),
                            Page::Info => info(app, ui, &mut bs),
                            Page::SaveAs => save_as(app, ui, &mut bs, &mut out),
                            Page::Print => print(app, ui, &mut bs, &mut out),
                            Page::Export => export(app, ui, &mut out),
                            Page::Options => options(app, ui),
                            Page::Account => account(app, ui),
                        }
                    });
                });
            });
        },
    );
    if !typing
        && app.dialog.is_none()
        && app.message.is_none()
        && !egui::Popup::is_any_open(&ctx)
        && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        out.close = true;
    }
    if let Some(page) = out.page {
        bs.page = page;
    }
    // A command run from a page may itself have opened (or replaced) the view; keep that.
    if out.close {
        app.backstage = None;
    } else if app.backstage.is_none() {
        app.backstage = Some(bs);
    }
}

/// Save: saves a workbook that has a file and returns to the sheet; a new workbook goes to the
/// Save As page first, like Excel's.
fn save(app: &mut SheetApp, out: &mut Outcome) {
    if !app.commit_edit(0, 0, false, false) {
        return;
    }
    if app.session.active().is_some_and(|d| d.path.is_some()) {
        app.run_or_alert("file.save", json!({}));
        out.close = true;
    } else {
        out.page = Some(Page::SaveAs);
    }
}

// ---------------------------------------------------------------- navigation

/// Allocates and paints one Backstage navigation row: selected or hover fill, icon, label and button semantics. Returns the row's click response.
fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool, text: Color32, sel: Color32) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (slot, resp) = ui.allocate_exact_size(vec2(NAV_W, 36.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let rect = Rect::from_min_max(pos2(slot.left() + 10.0, slot.top() + 1.0), pos2(slot.right() - 10.0, slot.bottom() - 1.0));
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 8.0, sel);
        p.rect_stroke(rect, 8.0, Stroke::new(1.0, t.ribbon_border), egui::StrokeKind::Inside);
        p.rect_filled(Rect::from_min_size(pos2(slot.left() + 2.0, rect.top() + 8.0), vec2(3.0, rect.height() - 16.0)), 1.5, t.accent);
    } else if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.hover) {
        p.rect_filled(rect, 8.0, fill);
    }
    let icon_color = if selected { t.accent } else { text };
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0)), icon, icon_color);
    let font = if selected { theme::ui_bold(13.0) } else { theme::ui_font(13.0) };
    p.text(pos2(rect.left() + 36.0, rect.center().y), Align2::LEFT_CENTER, label, font, text);
    resp
}

fn nav_separator(ui: &mut Ui, _text: Color32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(NAV_W, 17.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment([pos2(rect.left() + 20.0, y), pos2(rect.right() - 20.0, y)], Stroke::new(1.0, t.ribbon_border));
}

/// "Good morning / afternoon / evening" for the Home page, by the local hour.
fn greeting(lang: Language) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    let hour = chrono::Timelike::hour(&chrono::Local::now());
    #[cfg(target_arch = "wasm32")]
    let hour = 12;
    lang.tr(match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    })
    .to_string()
}

// ---------------------------------------------------------------- shared pieces

fn section(ui: &mut Ui, lang: Language, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.label(RichText::new(lang.tr(title)).font(theme::ui_bold(15.0)).color(t.text));
    ui.add_space(8.0);
}

fn dim(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).font(theme::ui_font(12.5)).color(t.text_dim));
}

/// Seconds each sheet shows while a thumbnail flips through a workbook's sheets on hover, and the
/// cross-fade between them.
const FLIP_SECONDS: f64 = 1.1;
const FADE_SECONDS: f64 = 0.25;

/// Paints a workbook's sheet thumbnails into `rect`: the first sheet at rest; while `hovered`,
/// each sheet in turn with a cross-fade, its name in a small label at the bottom left.
fn paint_pages(ui: &Ui, id: egui::Id, rect: Rect, radius: f32, pages: &[crate::thumbnail::Page], hovered: bool, label: bool) {
    let Some(first) = pages.first() else { return };
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    let start_id = id.with("flip_start");
    let start: Option<f64> = ui.ctx().data(|d| d.get_temp(start_id));
    let (shown, next, fade) = if hovered && pages.len() > 1 {
        let start = start.unwrap_or_else(|| {
            ui.ctx().data_mut(|d| d.insert_temp(start_id, now));
            now
        });
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(40));
        // The first sheet stays for a moment before the flipping starts.
        let elapsed = (now - start - 0.35).max(0.0);
        let step = (elapsed / FLIP_SECONDS) as usize;
        let into = elapsed - step as f64 * FLIP_SECONDS;
        let fade = if crate::widgets::animations_on(ui.ctx()) {
            ((into - (FLIP_SECONDS - FADE_SECONDS)) / FADE_SECONDS).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        (step % pages.len(), (step + 1) % pages.len(), fade)
    } else {
        if start.is_some() {
            ui.ctx().data_mut(|d| d.remove::<f64>(start_id));
        }
        (0, 0, 0.0)
    };
    let draw = |page: &crate::thumbnail::Page, alpha: f32| {
        egui::Image::from_texture(egui::load::SizedTexture::new(page.texture, rect.size()))
            .corner_radius(radius)
            .tint(Color32::WHITE.gamma_multiply(alpha))
            .paint_at(ui, rect);
    };
    let current = pages.get(shown).unwrap_or(first);
    draw(current, 1.0);
    if fade > 0.0
        && let Some(n) = pages.get(next)
    {
        draw(n, fade);
    }
    if label && hovered && pages.len() > 1 {
        let name = if fade > 0.5 { pages.get(next).unwrap_or(current) } else { current };
        let text = format!("{}  {}/{}", name.name, if fade > 0.5 { next + 1 } else { shown + 1 }, pages.len());
        let galley = ui.painter().layout_no_wrap(text, theme::ui_font(11.0), t.text);
        let pill = Rect::from_min_size(pos2(rect.left() + 8.0, rect.bottom() - 8.0 - 20.0), vec2(galley.size().x + 16.0, 20.0));
        ui.painter().rect_filled(pill, 10.0, t.window);
        ui.painter().rect_stroke(pill, 10.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
        ui.painter().galley(pos2(pill.left() + 8.0, pill.center().y - galley.size().y / 2.0), galley, t.text);
    }
}

/// A new-workbook tile: the workbook's thumbnails (flipping through its sheets on hover), or for a
/// blank book or while they are being drawn, a small empty sheet, over its name.
fn tile(ui: &mut Ui, label: &str, sample: Option<usize>, pages: &[crate::thumbnail::Page]) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(208.0, 186.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let p = ui.painter();
    let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), 140.0));
    // The empty sheet follows the theme, as the drawn thumbnails do.
    p.rect_filled(thumb, 8.0, t.sheet_bg(true));
    let edge = if resp.hovered() { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.ribbon_border) };
    if !pages.is_empty() {
        paint_pages(ui, resp.id, thumb, 8.0, pages, resp.hovered(), true);
        let edge = if resp.hovered() { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.ribbon_border) };
        ui.painter().rect_stroke(thumb, 8.0, edge, StrokeKind::Inside);
        let galley = ui.painter().layout(label.to_string(), theme::ui_font(13.0), t.text, rect.width());
        ui.painter().galley(pos2(rect.left() + 2.0, thumb.bottom() + 8.0), galley, t.text);
        return resp;
    }
    let inner = thumb.shrink(12.0);
    let (rows, cols) = (7, 4);
    let (rh, cw) = (inner.height() / rows as f32, inner.width() / cols as f32);
    if let Some(i) = sample {
        // Until its thumbnail arrives: a header band and a few "values", so the sample reads
        // differently from a blank book.
        p.rect_filled(Rect::from_min_size(inner.min, vec2(inner.width(), rh)), 0.0, t.accent_soft);
        for r in 1..rows {
            let len = ((r * (i + 2) * 7 + i * 3) % 10) as f32 / 10.0;
            let bar = Rect::from_min_size(
                pos2(inner.left() + cw + 3.0, inner.top() + r as f32 * rh + 3.0),
                vec2((cw * 3.0 - 6.0) * (0.25 + 0.75 * len), rh - 6.0),
            );
            p.rect_filled(bar, 1.0, t.accent.gamma_multiply(0.75));
        }
    }
    for r in 0..=rows {
        let y = inner.top() + r as f32 * rh;
        p.line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], Stroke::new(1.0, if t.dark { t.separator } else { t.gridline }));
    }
    for c in 0..=cols {
        let x = inner.left() + c as f32 * cw;
        p.line_segment([pos2(x, inner.top()), pos2(x, inner.bottom())], Stroke::new(1.0, if t.dark { t.separator } else { t.gridline }));
    }
    p.rect_stroke(thumb, 8.0, edge, StrokeKind::Inside);
    let galley = p.layout(label.to_string(), theme::ui_font(13.0), t.text, rect.width());
    p.galley(pos2(rect.left() + 2.0, thumb.bottom() + 8.0), galley, t.text);
    resp
}

/// Blank workbook and the built-in samples.
fn new_tiles(app: &mut SheetApp, ui: &mut Ui, out: &mut Outcome) {
    let lang = app.ui.language;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(18.0, 18.0);
        if tile(ui, lang.tr("Blank workbook"), None, &[]).clicked() {
            app.run_or_alert("file.new", json!({}));
            out.close = true;
        }
        let dark = Tokens::get(ui.ctx()).dark && app.ui.dark_sheet;
        for (i, (name, title)) in gridcraft_engine::sample::SAMPLES.iter().enumerate() {
            let pages = app.thumbnails.pages(ui.ctx(), &crate::thumbnail::Source::Sample((*name).to_string()), dark);
            if tile(ui, title, Some(i), &pages).clicked() {
                app.run_or_alert("file.new", json!({"sample": name}));
                out.close = true;
            }
        }
    });
}

/// File name and folder of a recent path, split as text.
fn split_path(path: &str) -> (String, String) {
    let p = std::path::Path::new(path);
    let name = p.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(path).to_string();
    let folder = p.parent().and_then(std::path::Path::to_str).unwrap_or("").to_string();
    (name, folder)
}

/// Where a recent workbook's name starts, after its thumbnail.
const THUMB_X: f32 = 88.0;

/// Width of the column that tells when a recent workbook was last changed.
const MODIFIED_W: f32 = 150.0;

/// Recent workbooks, newest first, each with when it was last changed. Opening one is the user's
/// explicit choice.
fn recent_list(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome, limit: usize) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    if app.ui.recent.is_empty() {
        dim(ui, lang.tr("No recent workbooks yet."));
        return;
    }
    let width = ui.available_width().min(CONTENT_W);
    // What the file system has said so far; the rest arrives in later frames (see `RecentFiles`).
    let known = bs.recent.known(ui.ctx(), &app.ui.recent);
    // The web build has no files to ask about, and a narrow window leaves the names their room.
    let modified_w = if cfg!(target_arch = "wasm32") || width < 420.0 { 0.0 } else { MODIFIED_W };
    // Column heads over a hairline: Name, Location, Modified.
    let location_x = (width * 0.52).max(260.0);
    let show_location = width >= 560.0;
    {
        let (head, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
        let p = ui.painter();
        p.text(pos2(head.left() + THUMB_X, head.center().y), Align2::LEFT_CENTER, lang.tr("Name"), theme::ui_font(12.0), t.text_dim);
        if show_location {
            p.text(pos2(head.left() + location_x, head.center().y), Align2::LEFT_CENTER, lang.tr("Location"), theme::ui_font(12.0), t.text_dim);
        }
        if modified_w > 0.0 {
            p.text(pos2(head.right() - modified_w, head.center().y), Align2::LEFT_CENTER, lang.tr("Modified"), theme::ui_font(12.0), t.text_dim);
        }
        p.line_segment([pos2(head.left(), head.bottom() - 0.5), pos2(head.right(), head.bottom() - 0.5)], Stroke::new(1.0, t.separator));
        ui.add_space(4.0);
    }
    let now = crate::now_ms() / 1000.0;
    let dark = t.dark && app.ui.dark_sheet;
    for path in app.ui.recent.iter().take(limit).cloned().collect::<Vec<_>>() {
        let (name, folder) = split_path(&path);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 58.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        let p = ui.painter();
        if let Some(fill) = crate::widgets::hover_fill(ui, resp.id, resp.hovered(), t.window) {
            p.rect_filled(rect, 8.0, fill);
        }
        // The workbook's thumbnail, once drawn (from the file as saved, so an edited file
        // gets a new one); the sheet icon until then, or when the file cannot be read.
        let thumb = Rect::from_min_size(pos2(rect.left() + 6.0, rect.center().y - 23.0), vec2(68.0, 46.0));
        let pages = app.thumbnails.pages(ui.ctx(), &crate::thumbnail::Source::File(path.clone()), dark);
        match pages.is_empty() {
            false => {
                paint_pages(ui, resp.id, thumb, 5.0, &pages, resp.hovered(), false);
                p.rect_stroke(thumb, 5.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            }
            true => {
                let csv = name.to_ascii_lowercase().ends_with(".csv");
                icons::paint(p, Rect::from_center_size(thumb.center(), vec2(18.0, 18.0)), Icon::Sheet, if csv { t.text_dim } else { t.accent });
            }
        }
        // A long name or folder stops short of the next column instead of running under it.
        let name_room = if show_location {
            rect.left() + location_x - 12.0
        } else if modified_w > 0.0 {
            rect.right() - modified_w - 14.0
        } else {
            rect.right()
        };
        let text = p.with_clip_rect(Rect::from_min_max(rect.min, pos2(name_room, rect.bottom())).intersect(p.clip_rect()));
        if show_location {
            text.text(pos2(rect.left() + THUMB_X, rect.center().y), Align2::LEFT_CENTER, &name, theme::ui_font(13.5), t.text);
            let folder = tilde_home(&folder, &std::env::var("HOME").unwrap_or_default());
            let room = if modified_w > 0.0 { rect.right() - modified_w - 14.0 } else { rect.right() };
            p.with_clip_rect(Rect::from_min_max(pos2(rect.left() + location_x, rect.top()), pos2(room, rect.bottom())).intersect(p.clip_rect()))
                .text(pos2(rect.left() + location_x, rect.center().y), Align2::LEFT_CENTER, folder, theme::ui_font(12.5), t.text_dim);
        } else {
            text.text(pos2(rect.left() + THUMB_X, rect.center().y - 8.0), Align2::LEFT_CENTER, &name, theme::ui_font(13.5), t.text);
            text.text(pos2(rect.left() + THUMB_X, rect.center().y + 9.0), Align2::LEFT_CENTER, &folder, theme::ui_font(11.5), t.text_dim);
        }
        if modified_w > 0.0 {
            let (words, color) = match known.get(&path) {
                Some(Modified::At(seconds)) => (age(lang, now - seconds), t.text_dim),
                Some(Modified::Unknown) => (lang.tr("Not available").to_string(), t.text_disabled),
                None => (String::new(), t.text_dim),
            };
            p.text(pos2(rect.right() - modified_w, rect.center().y), Align2::LEFT_CENTER, words, theme::ui_font(12.0), color);
        }
        let resp = resp.on_hover_text(&path);
        if resp.clicked() {
            app.open_path(&path);
            if app.session.active().and_then(|d| d.path.as_deref()) == Some(path.as_str()) {
                out.close = true;
            }
        }
    }
}

/// What the file system said about a recent workbook.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // only the native metadata thread answers
enum Modified {
    /// When it was last changed, in seconds since 1970.
    At(f64),
    /// It can't be reached: moved, deleted, or on a drive that is away.
    Unknown,
}

/// One thread at a time asks about recent workbooks, so a drive that never answers holds up one
/// thread, not one for every time the view is opened.
static ASKING: AtomicBool = AtomicBool::new(false);

/// When the listed workbooks were last changed, as far as the file system has answered.
///
/// Asking can take long (a network drive that is asleep, a share that is away), so it is never
/// done while drawing: a thread asks, and the list shows each answer as it comes back. Only the
/// files' metadata is read. A view opened anew asks anew.
#[derive(Clone, Debug, Default)]
pub struct RecentFiles {
    /// The list the answers are for.
    asked: Vec<String>,
    found: Arc<Mutex<HashMap<String, Modified>>>,
}

impl RecentFiles {
    /// What is known this frame about the files in `recent`; starts asking when the list is new.
    fn known(&mut self, ctx: &egui::Context, recent: &[String]) -> HashMap<String, Modified> {
        if self.asked != recent && !ASKING.swap(true, Ordering::SeqCst) {
            self.asked = recent.to_vec();
            if !ask(ctx.clone(), recent.to_vec(), self.found.clone()) {
                ASKING.store(false, Ordering::SeqCst);
            }
        }
        self.found.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Starts the thread that asks. Returns whether it started.
#[cfg(not(target_arch = "wasm32"))]
fn ask(ctx: egui::Context, paths: Vec<String>, found: Arc<Mutex<HashMap<String, Modified>>>) -> bool {
    /// Lets the next list ask, however the thread ends.
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            ASKING.store(false, Ordering::SeqCst);
        }
    }
    let asking = std::thread::Builder::new().name("gridcraft-recent-files".into()).spawn(move || {
        let _done = Done;
        for path in paths {
            let changed = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok());
            let answer = changed.map_or(Modified::Unknown, |since| Modified::At(since.as_secs_f64()));
            found.lock().unwrap_or_else(PoisonError::into_inner).insert(path, answer);
            ctx.request_repaint();
        }
    });
    asking.is_ok()
}

/// The web build has no file system to ask.
#[cfg(target_arch = "wasm32")]
fn ask(_ctx: egui::Context, _paths: Vec<String>, _found: Arc<Mutex<HashMap<String, Modified>>>) -> bool {
    false
}

/// How long ago, in words. The clock's time zone isn't known here, so the list tells ages rather
/// than dates, which could be a day out. From two days on the counts are never one, so each
/// language needs one form of "days", "weeks", "months" and "years".
fn age(lang: Language, seconds: f64) -> String {
    const DAY: f64 = 86_400.0;
    let s = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    let counted = |pattern: &str, n: f64| lang.tr(pattern).replace("{n}", &(n.floor() as u64).to_string());
    if s < 60.0 {
        lang.tr("Just now").to_string()
    } else if s < 3600.0 {
        counted("{n} min ago", s / 60.0)
    } else if s < DAY {
        counted("{n} h ago", s / 3600.0)
    } else if s < 2.0 * DAY {
        lang.tr("Yesterday").to_string()
    } else if s < 14.0 * DAY {
        counted("{n} days ago", s / DAY)
    } else if s < 61.0 * DAY {
        counted("{n} weeks ago", s / (7.0 * DAY))
    } else if s < 731.0 * DAY {
        counted("{n} months ago", s / (30.44 * DAY))
    } else {
        counted("{n} years ago", s / (365.25 * DAY))
    }
}

/// A large clickable row (icon, title, one-line description).
fn action_row(ui: &mut Ui, icon: Icon, title: &str, detail: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().min(CONTENT_W);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 4.0, t.hover);
    }
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), vec2(24.0, 24.0)), icon, t.text);
    p.text(pos2(rect.left() + 52.0, rect.center().y - 9.0), Align2::LEFT_CENTER, title, theme::ui_bold(13.5), t.text);
    p.text(pos2(rect.left() + 52.0, rect.center().y + 10.0), Align2::LEFT_CENTER, detail, theme::ui_font(12.0), t.text_dim);
    resp
}

/// A bordered square command button (Browse…, Print, Protect Workbook).
fn square_button(ui: &mut Ui, icon: Icon, label: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(96.0, 88.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let p = ui.painter();
    p.rect_filled(rect, 6.0, if resp.hovered() { t.hover } else { t.ribbon });
    p.rect_stroke(rect, 6.0, Stroke::new(1.0, if resp.hovered() { t.accent } else { t.ribbon_border }), StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 32.0), vec2(30.0, 30.0)), icon, t.text);
    let galley = p.layout(label.to_string(), theme::ui_font(12.0), t.text, rect.width() - 8.0);
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 56.0), galley, t.text);
    resp
}

// ---------------------------------------------------------------- pages

/// Where a tip holds: everywhere, or only off macOS (or only on it) where the keys differ.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TipOn {
    All,
    NotMac,
    Mac,
}

/// Short tips about features GridCraft has, one shown a day on Home.
const TIPS: [(&str, TipOn); 19] = [
    ("Search commands with the magnifier in the title bar to run any command by name.", TipOn::All),
    ("Tap Alt to show key tips, then type the letters to reach any ribbon button.", TipOn::NotMac),
    ("Double-click Format Painter to keep it on while you paint several ranges.", TipOn::All),
    ("Press F2 to edit the active cell without retyping it.", TipOn::All),
    ("Ctrl+Shift+U expands or collapses the formula bar.", TipOn::All),
    ("While typing a reference, F4 cycles it through $A$1, A$1, $A1 and A1.", TipOn::All),
    ("Alt+= inserts an AutoSum for the selection.", TipOn::NotMac),
    ("Double-click a sheet tab to rename it in place.", TipOn::All),
    ("Hold Ctrl while dragging a sheet tab to drop a copy of the sheet.", TipOn::NotMac),
    ("Hold Cmd or Option while dragging a sheet tab to drop a copy of the sheet.", TipOn::Mac),
    ("Click a statistic in the status bar to copy it; right-click to choose which ones show.", TipOn::All),
    ("Click the zoom percentage in the status bar to set an exact zoom.", TipOn::All),
    ("Shift+F11 inserts a new sheet.", TipOn::All),
    ("Ctrl+; enters today's date in the active cell.", TipOn::All),
    ("Pick a display theme in View, Omarchy and Dark Green included.", TipOn::All),
    ("In a dark theme, View › Dark sheet turns the cells and charts dark too.", TipOn::All),
    ("Turn off View › Animations to make the selection jump instead of glide.", TipOn::All),
    ("Hover over a sample tile here to flip through its sheets.", TipOn::All),
    ("Click the view button in the status bar to cycle Normal, Page Layout and Page Break Preview.", TipOn::All),
];

/// The tip for day `day` (days since 1970), on macOS when `mac`.
fn tip_of_the_day(day: i64, mac: bool) -> &'static str {
    let tips: Vec<&str> = TIPS.iter().filter(|(_, on)| *on == TipOn::All || (*on == TipOn::Mac) == mac).map(|(tip, _)| *tip).collect();
    let n = tips.len().max(1) as i64;
    tips.get(day.rem_euclid(n) as usize).copied().unwrap_or("")
}

/// The one-line tip under Home's heading: an accent dot, a "Tip" label and the day's tip, dim.
fn tip_line(ui: &mut Ui, lang: Language) {
    let t = Tokens::get(ui.ctx());
    let day = (crate::now_ms() / 86_400_000.0).floor();
    let day = if day.is_finite() { day as i64 } else { 0 };
    let tip = lang.tr(tip_of_the_day(day, ui.ctx().os() == egui::os::OperatingSystem::Mac));
    let width = ui.available_width().min(CONTENT_W);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{}: {tip}", lang.tr("Tip"))));
    let p = ui.painter();
    let y = rect.center().y;
    p.circle_filled(pos2(rect.left() + 3.0, y), 3.0, t.accent);
    let label = p.text(pos2(rect.left() + 12.0, y), Align2::LEFT_CENTER, lang.tr("Tip"), theme::ui_bold(12.5), t.text);
    let text = p.with_clip_rect(rect.intersect(p.clip_rect()));
    text.text(pos2(label.right() + 8.0, y), Align2::LEFT_CENTER, tip, theme::ui_font(12.5), t.text_dim);
}

fn home(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let lang = app.ui.language;
    section(ui, lang, "New");
    new_tiles(app, ui, out);
    ui.add_space(22.0);
    section(ui, lang, "Recent");
    recent_list(app, ui, bs, out, 8);
    if app.ui.recent.len() > 8 {
        ui.add_space(6.0);
        if ui.link(lang.tr("More workbooks")).clicked() {
            out.page = Some(Page::Open);
        }
    }
}

fn new_page(app: &mut SheetApp, ui: &mut Ui, out: &mut Outcome) {
    new_tiles(app, ui, out);
}

fn open_page(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let lang = app.ui.language;
    if square_button(ui, Icon::OpenFolder, lang.tr("Browse…")).clicked() {
        let before = app.session.active().map(|d| d.uid);
        app.open_dialog("open", json!({}));
        // Desktop pickers return at once; the web picker delivers later (see `SheetApp::logic`).
        if app.session.active().map(|d| d.uid) != before {
            out.close = true;
        }
    }
    ui.add_space(22.0);
    section(ui, lang, "Recent");
    recent_list(app, ui, bs, out, usize::MAX);
}

/// Review › Protect Workbook and File › Info › Protect Workbook: the protect dialog (with an
/// optional password) when the structure is open; when it's locked, the password prompt if one
/// was set, else unprotect right away.
pub fn protect_workbook(app: &mut SheetApp) {
    let (on, password) = app.session.active().map(|d| (d.wb.protected_structure, d.wb.structure_password.is_some())).unwrap_or((false, false));
    if !on {
        app.open_dialog("protectWorkbook", json!({}));
    } else if password {
        app.open_dialog("unprotectWorkbook", json!({}));
    } else {
        app.run_or_alert("review.protectWorkbook", json!({"on": false}));
    }
}

/// Shows the workbook title, path, sheet count and structure protection, plus editable document properties. Committing a changed field after it loses focus runs `file.properties`.
fn info(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let Some(d) = app.session.active() else {
        dim(ui, lang.tr("No workbook is open."));
        return;
    };
    let title = d.display_title();
    let path = d.path.clone();
    let sheets = d.wb.sheets.len();
    let protected = d.wb.protected_structure;
    let current: Vec<String> = {
        let pr = &d.wb.props;
        vec![pr.title.clone(), pr.subject.clone(), pr.author.clone(), pr.company.clone(), pr.keywords.clone(), pr.description.clone()]
    };
    ui.horizontal(|ui| {
        icons::paint(ui.painter(), Rect::from_min_size(ui.cursor().min, vec2(28.0, 28.0)), Icon::Book, t.text);
        ui.add_space(38.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(&title).font(theme::ui_bold(16.0)).color(t.text));
            match &path {
                Some(p) => dim(ui, p),
                None => dim(ui, lang.tr("Not saved yet")),
            }
        });
    });
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        if square_button(ui, Icon::Lock, lang.tr("Protect Workbook")).clicked() {
            protect_workbook(app);
        }
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new(lang.tr("Protect Workbook")).font(theme::ui_bold(13.5)).color(t.text));
            let text = if protected {
                "Sheet structure is locked: sheets can't be added, moved, renamed or deleted."
            } else {
                "Lock the sheet structure so sheets can't be added, moved, renamed or deleted."
            };
            dim(ui, lang.tr(text));
        });
    });
    ui.add_space(22.0);
    section(ui, lang, "Properties");
    egui::Grid::new("backstage_props").num_columns(2).spacing(vec2(18.0, 8.0)).show(ui, |ui| {
        ui.label(RichText::new(lang.tr("Location")).color(t.text_dim));
        ui.label(path.as_deref().unwrap_or(lang.tr("Not saved yet")));
        ui.end_row();
        ui.label(RichText::new(lang.tr("Sheets")).color(t.text_dim));
        ui.label(sheets.to_string());
        ui.end_row();
        for (i, (key, label)) in PROPERTIES.iter().enumerate() {
            let (Some(draft), Some(saved)) = (bs.props.get_mut(i), current.get(i)) else { continue };
            let label = ui.label(RichText::new(lang.tr(label)).color(t.text_dim));
            let id = ui.make_persistent_id(("backstage_prop", *key));
            // Show the workbook's value unless the field is being edited (undo, another window).
            if !ui.ctx().memory(|m| m.has_focus(id)) {
                draft.clone_from(saved);
            }
            let resp = ui.add(egui::TextEdit::singleline(draft).id(id).desired_width(360.0)).labelled_by(label.id);
            if resp.lost_focus() && draft != saved {
                let mut params = serde_json::Map::new();
                params.insert((*key).to_string(), json!(draft.clone()));
                app.run_or_alert("file.properties", Json::Object(params));
            }
            ui.end_row();
        }
    });
}

/// Draws the Save As type picker and action: xlsx opens the `saveAs` dialog, other types export, and the page closes when the save/export succeeds (or a web download handles it).
fn save_as(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let lang = app.ui.language;
    dim(ui, lang.tr("Save the workbook under another name or in another folder."));
    ui.add_space(14.0);
    let fmt = bs.save_format;
    let current = SAVE_TYPES.iter().find(|(ext, _)| *ext == fmt).map_or("Excel Workbook (*.xlsx)", |(_, label)| *label);
    ui.horizontal(|ui| {
        let label = ui.label(lang.tr("Save as type"));
        craft_ui::ComboBox::from_id_salt("backstage_save_type")
            .width(280.0)
            .selected_text(lang.tr(current))
            .show_ui(ui, |ui| {
                for (ext, label) in SAVE_TYPES {
                    if ui.selectable_label(fmt == ext, lang.tr(label)).clicked() {
                        bs.save_format = ext;
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
    ui.add_space(14.0);
    // Desktop: the system Save dialog. Web: the file downloads.
    let web = app.services.pick_save.is_none() && app.services.download.is_some();
    let label = if web { format!("{} (.{fmt})", lang.tr("Download")) } else { lang.tr("Browse…").to_string() };
    if square_button(ui, Icon::OpenFolder, &label).clicked() && app.commit_edit(0, 0, false, false) {
        if fmt == "xlsx" {
            let before = app.session.active().map(|d| (d.path.clone(), d.is_dirty()));
            app.open_dialog("saveAs", json!({}));
            let after = app.session.active().map(|d| (d.path.clone(), d.is_dirty()));
            if after != before || app.services.pick_save.is_none() {
                out.close = true;
            }
        } else if export_as(app, fmt) {
            out.close = true;
        }
    }
    ui.add_space(18.0);
    dim(ui, lang.tr("A CSV file, web page or PDF is a copy: the workbook keeps its own file."));
}

fn export(app: &mut SheetApp, ui: &mut Ui, out: &mut Outcome) {
    let lang = app.ui.language;
    if action_row(ui, Icon::Paper, lang.tr("Create PDF"), lang.tr("Save the sheet as a PDF document that keeps its page layout.")).clicked()
        && export_as(app, "pdf")
    {
        out.close = true;
    }
    ui.add_space(14.0);
    section(ui, lang, "Change File Type");
    for (fmt, icon, title, detail) in [
        ("csv", Icon::TextColumns, "CSV (Comma delimited)", "Save the active sheet as comma-separated text."),
        ("html", Icon::Link, "Web Page (.html)", "Save the workbook as a web page."),
    ] {
        if action_row(ui, icon, lang.tr(title), lang.tr(detail)).clicked() && export_as(app, fmt) {
            out.close = true;
        }
    }
}

/// Exports the active workbook as `pdf`, `csv` or `html`: through the save picker on the desktop,
/// as a browser download on the web. Returns whether a file was written or handed over.
pub fn export_as(app: &mut SheetApp, fmt: &str) -> bool {
    if !app.commit_edit(0, 0, false, false) {
        return false;
    }
    let id = match fmt {
        "pdf" => "file.exportPdf",
        "csv" => "file.exportCsv",
        "html" => "file.exportHtml",
        _ => return false,
    };
    let title = app.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
    let stem = std::path::Path::new(&title).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book1").to_string();
    if let Some(pick) = &app.services.pick_save {
        let Some(path) = pick(&format!("{stem}.{fmt}")) else { return false };
        // The picker may suggest a workbook extension; the export writes the chosen format.
        let path = std::path::Path::new(&path).with_extension(fmt).to_string_lossy().into_owned();
        return match app.run(id, json!({"path": path})) {
            Ok(_) => true,
            Err(e) => {
                app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                false
            }
        };
    }
    if app.services.download.is_none() {
        app.message = Some(("GridCraft".into(), "Exporting needs a file picker, which this window doesn't have.".into()));
        return false;
    }
    if fmt == "pdf" {
        // Without a path the PDF comes back as bytes, which `SheetApp::run` hands to the browser.
        return match app.run(id, json!({})) {
            Ok(_) => true,
            Err(e) => {
                app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                false
            }
        };
    }
    app.store_view_state();
    match app.session.run("file.saveBytes", json!({"format": fmt})) {
        Ok(r) => {
            let bytes = r.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode);
            let name = r.get("name").and_then(Json::as_str).map_or_else(|| format!("{stem}.{fmt}"), str::to_string);
            match (bytes, &app.services.download) {
                (Some(b), Some(dl)) => {
                    dl(&name, &b);
                    true
                }
                _ => false,
            }
        }
        Err(e) => {
            app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
            false
        }
    }
}

/// Excel's File › Account: product information and the About window.
fn account(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    section(ui, lang, "Product Information");
    ui.label(RichText::new("GridCraft").font(theme::ui_bold(16.0)).color(t.text));
    ui.add_space(8.0);
    egui::Grid::new("backstage_account").num_columns(2).spacing(vec2(18.0, 8.0)).show(ui, |ui| {
        for (label, value) in
            [("Version", env!("CARGO_PKG_VERSION").to_string()), ("Platform", platform()), ("Licence", env!("CARGO_PKG_LICENSE").to_string())]
        {
            ui.label(RichText::new(lang.tr(label)).color(t.text_dim));
            ui.label(value);
            ui.end_row();
        }
    });
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        if square_button(ui, Icon::Info, lang.tr("About GridCraft")).clicked() {
            app.open_dialog("about", json!({}));
        }
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new(lang.tr("About GridCraft")).font(theme::ui_bold(13.5)).color(t.text));
            dim(ui, lang.tr("Version, contributors and the AI models that helped"));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_are_told_in_words() {
        const DAY: f64 = 86_400.0;
        for (seconds, words) in [
            (0.0, "Just now"),
            (59.9, "Just now"),
            (60.0, "1 min ago"),
            (150.0, "2 min ago"),
            (3600.0, "1 h ago"),
            (23.9 * 3600.0, "23 h ago"),
            (DAY, "Yesterday"),
            (1.99 * DAY, "Yesterday"),
            // From here on a count is never one.
            (2.0 * DAY, "2 days ago"),
            (13.9 * DAY, "13 days ago"),
            (14.0 * DAY, "2 weeks ago"),
            (60.9 * DAY, "8 weeks ago"),
            (61.0 * DAY, "2 months ago"),
            (730.9 * DAY, "24 months ago"),
            (731.0 * DAY, "2 years ago"),
            (4000.0 * DAY, "10 years ago"),
            // A clock that is behind the file's, or a number that isn't one.
            (-5.0, "Just now"),
            (f64::NAN, "Just now"),
            (f64::NEG_INFINITY, "Just now"),
            (f64::INFINITY, "Just now"),
        ] {
            assert_eq!(age(Language::En, seconds), words, "{seconds}");
        }
        assert!(age(Language::En, f64::MAX).ends_with(" years ago"));
        // Other languages put the count where their own words want it.
        for lang in Language::ALL {
            assert!(age(lang, 3.0 * DAY).contains('3'), "{lang:?}: {}", age(lang, 3.0 * DAY));
            for seconds in [10.0, 600.0, 7200.0, 1.5 * DAY, 5.0 * DAY, 30.0 * DAY, 200.0 * DAY, 2000.0 * DAY] {
                let words = age(lang, seconds);
                assert!(!words.is_empty() && !words.contains('{'), "{lang:?} {seconds}: {words}");
            }
        }
    }

    #[test]
    fn the_recent_list_learns_when_its_files_were_last_changed() {
        let dir = std::env::temp_dir().join(format!("gridcraft-recent-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let there = dir.join("There.xlsx").to_string_lossy().into_owned();
        let gone = dir.join("Gone.xlsx").to_string_lossy().into_owned();
        std::fs::write(&there, b"a file, if not a workbook").unwrap();
        let ctx = egui::Context::default();
        let recent = vec![there.clone(), gone.clone()];
        let mut files = RecentFiles::default();
        // Nothing is read while drawing: the answers arrive from a thread, in later frames.
        let mut known = HashMap::new();
        for _ in 0..2_000 {
            known = files.known(&ctx, &recent);
            if known.len() == recent.len() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(known.get(&there), Some(Modified::At(seconds)) if *seconds > 1.0e9), "{known:?}");
        assert_eq!(known.get(&gone), Some(&Modified::Unknown));
        // The same list is not asked about twice; an empty one is never asked about.
        assert_eq!(files.known(&ctx, &recent), known);
        assert!(RecentFiles::default().known(&ctx, &[]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tip_changes_daily_and_fits_the_platform() {
        for mac in [false, true] {
            let tips: Vec<&str> = (0..30).map(|day| tip_of_the_day(day, mac)).collect();
            assert!(tips.iter().all(|tip| !tip.is_empty() && tip.len() < 100), "{tips:?}");
            assert!(tips.windows(2).all(|w| w[0] != w[1]), "consecutive days show different tips");
            assert_eq!(tips.iter().any(|tip| tip.contains("Alt")), !mac, "Alt tips are for Windows and Linux");
            assert_eq!(tips.iter().any(|tip| tip.contains("Cmd")), mac);
        }
        assert!(!tip_of_the_day(-3, false).is_empty() && !tip_of_the_day(i64::MAX, true).is_empty());
    }

    #[test]
    fn page_ids_round_trip_and_unknown_ids_are_refused() {
        for page in Page::ALL {
            assert_eq!(Page::parse(page.id()), Some(page));
        }
        assert_eq!(Page::parse(" SAVEAS "), Some(Page::SaveAs));
        assert_eq!(Page::parse("feedback"), None);
        assert_eq!(Page::parse(""), None);
    }

    #[test]
    fn recent_paths_split_without_touching_the_disk() {
        assert_eq!(split_path("/home/me/Budget.xlsx"), ("Budget.xlsx".into(), "/home/me".into()));
        assert_eq!(split_path("Budget.xlsx"), ("Budget.xlsx".into(), "".into()));
    }
}

/// `folder` with the home folder `home` shown as `~`, only as a whole leading path component;
/// an empty `home` (no HOME on Windows or the web) leaves the path as it is.
fn tilde_home(folder: &str, home: &str) -> String {
    folder
        .strip_prefix(home)
        .filter(|rest| !home.is_empty() && (rest.is_empty() || rest.starts_with(['/', '\\'])))
        .map_or_else(|| folder.to_string(), |rest| format!("~{rest}"))
}

#[cfg(test)]
mod tilde_tests {
    use super::tilde_home;

    #[test]
    fn only_a_leading_home_folder_becomes_a_tilde() {
        assert_eq!(tilde_home("/home/me/Docs", "/home/me"), "~/Docs");
        assert_eq!(tilde_home("/home/me", "/home/me"), "~");
        assert_eq!(tilde_home("/home/meg/Docs", "/home/me"), "/home/meg/Docs");
        assert_eq!(tilde_home("/srv/home/me/Docs", "/home/me"), "/srv/home/me/Docs");
        assert_eq!(tilde_home("C:\\Users\\me\\Docs", ""), "C:\\Users\\me\\Docs");
    }
}
