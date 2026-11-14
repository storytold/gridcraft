//! File tab Backstage view (#174): a full-window page that replaces the ribbon, formula bar, grid
//! and sheet tabs until Esc or the back arrow returns to the sheet.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Align2, Color32, Key, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n::{self, Language};
use crate::icons::{self, Icon};
use crate::rtl::UiRtlExt;
use crate::theme::{self, Tokens};

mod options;
mod print;

use options::{options, platform};
use print::print;

/// Width of the navigation column.
const NAV_W: f32 = 196.0;
/// Width of the content column (pages are left-aligned like a document, not stretched).
const CONTENT_W: f32 = 760.0;

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
    props: [String; 6],
    text_edit_focused: bool,
    pub print_all: bool,
    pub fit_to_page: bool,
    preview_page: usize,
    preview: Option<((u64, u64, bool, bool), Json)>,
    pub save_format: &'static str,
    recent: RecentFiles,
}

pub const SAVE_TYPES: [(&str, &str); 4] =
    [("xlsx", "Excel Workbook (*.xlsx)"), ("csv", "CSV UTF-8 (Comma delimited) (*.csv)"), ("html", "Web Page (*.html)"), ("pdf", "PDF (*.pdf)")];

impl Backstage {
    pub fn new(page: Page) -> Backstage {
        Backstage {
            page,
            props: Default::default(),
            text_edit_focused: false,
            print_all: false,
            fit_to_page: false,
            preview_page: 0,
            preview: None,
            save_format: "xlsx",
            recent: RecentFiles::default(),
        }
    }
}

#[derive(Default)]
struct Outcome {
    close: bool,
    page: Option<Page>,
}

/// Draws the Backstage view in place of the ribbon, formula bar, grid and sheet tabs.
pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let Some(mut bs) = app.backstage.take() else { return };
    let ctx = ui.ctx().clone();
    let typing = bs.text_edit_focused;
    let t = Tokens::get(&ctx);
    let lang = app.ui.language;
    let is_rtl = lang.is_rtl();
    let mut out = Outcome::default();

    let (nav_bg, nav_text, nav_sel) =
        if t.dark { (t.accent_soft, t.text, Color32::from_white_alpha(30)) } else { (t.accent, Color32::WHITE, Color32::from_black_alpha(46)) };

    let nav_panel = if is_rtl { egui::Panel::right("backstage_nav") } else { egui::Panel::left("backstage_nav") };

    nav_panel
        .exact_size(NAV_W)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(nav_bg).inner_margin(egui::Margin { left: 0, right: 0, top: 10, bottom: 10 }))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if nav_item(ui, is_rtl, Icon::Back, lang.tr("Back"), false, nav_text, nav_sel).clicked() {
                out.close = true;
            }
            ui.add_space(10.0);
            for page in [Page::Home, Page::New, Page::Open] {
                if nav_item(ui, is_rtl, page.icon(), lang.tr(page.label()), bs.page == page, nav_text, nav_sel).clicked() {
                    out.page = Some(page);
                }
            }
            nav_separator(ui, nav_text);
            if nav_item(ui, is_rtl, Page::Info.icon(), lang.tr("Info"), bs.page == Page::Info, nav_text, nav_sel).clicked() {
                out.page = Some(Page::Info);
            }
            if nav_item(ui, is_rtl, Icon::Save, lang.tr("Save"), false, nav_text, nav_sel).clicked() {
                save(app, &mut out);
            }
            for page in [Page::SaveAs, Page::Print, Page::Export] {
                if nav_item(ui, is_rtl, page.icon(), lang.tr(page.label()), bs.page == page, nav_text, nav_sel).clicked() {
                    out.page = Some(page);
                }
            }
            if nav_item(ui, is_rtl, Icon::Close, lang.tr("Close"), false, nav_text, nav_sel).clicked() {
                if app.commit_edit(0, 0, false, false) {
                    app.run_or_alert("file.close", json!({}));
                    out.close = true;
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                for page in [Page::Options, Page::Account] {
                    if nav_item(ui, is_rtl, page.icon(), lang.tr(page.label()), bs.page == page, nav_text, nav_sel).clicked() {
                        out.page = Some(page);
                    }
                }
            });
        });

    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.ribbon)).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(26.0);

            ui.rtl_container(is_rtl, CONTENT_W, 40.0, |ui| {
                let page_title = RichText::new(lang.tr(bs.page.label())).font(theme::ui_font(28.0)).color(t.text);
                if is_rtl {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(page_title);
                    });
                } else {
                    ui.label(page_title);
                }
                ui.add_space(18.0);

                match bs.page {
                    Page::Home => home(app, ui, &mut bs, &mut out, is_rtl),
                    Page::New => new_page(app, ui, &mut out, is_rtl),
                    Page::Open => open_page(app, ui, &mut bs, &mut out, is_rtl),
                    Page::Info => info(app, ui, &mut bs, is_rtl),
                    Page::SaveAs => save_as(app, ui, &mut bs, &mut out, is_rtl),
                    Page::Print => print(app, ui, &mut bs, &mut out),
                    Page::Export => export(app, ui, &mut out, is_rtl),
                    Page::Options => options(app, ui),
                    Page::Account => account(app, ui, is_rtl),
                }
            });

            ui.add_space(32.0);
        });
    });

    bs.text_edit_focused = ctx.text_edit_focused();
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
    if out.close {
        app.backstage = None;
    } else if app.backstage.is_none() {
        app.backstage = Some(bs);
    }
}

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

fn nav_item(ui: &mut Ui, is_rtl: bool, icon: Icon, label: &str, selected: bool, text: Color32, sel: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(NAV_W, 38.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    let p = ui.painter();

    if selected {
        p.rect_filled(rect, 0.0, sel);
        if is_rtl {
            p.rect_filled(Rect::from_min_size(pos2(rect.right() - 4.0, rect.top()), vec2(4.0, rect.height())), 0.0, text);
        } else {
            p.rect_filled(Rect::from_min_size(rect.min, vec2(4.0, rect.height())), 0.0, text);
        }
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, sel.gamma_multiply(0.55));
    }

    if is_rtl {
        icons::paint(p, Rect::from_center_size(pos2(rect.right() - 30.0, rect.center().y), vec2(17.0, 17.0)), icon, text);
        p.text(pos2(rect.right() - 50.0, rect.center().y), Align2::RIGHT_CENTER, label, theme::ui_font(14.0), text);
    } else {
        icons::paint(p, Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), vec2(17.0, 17.0)), icon, text);
        p.text(pos2(rect.left() + 50.0, rect.center().y), Align2::LEFT_CENTER, label, theme::ui_font(14.0), text);
    }
    resp
}

fn nav_separator(ui: &mut Ui, text: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(NAV_W, 17.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment([pos2(rect.left() + 20.0, y), pos2(rect.right() - 20.0, y)], Stroke::new(1.0, text.gamma_multiply(0.35)));
}

// ---------------------------------------------------------------- shared pieces

fn section(ui: &mut Ui, lang: Language, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    let heading = RichText::new(lang.tr(title)).font(theme::ui_bold(15.0)).color(t.text);
    if lang.is_rtl() {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(heading);
        });
    } else {
        ui.label(heading);
    }
    ui.add_space(8.0);
}

fn dim(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).font(theme::ui_font(12.5)).color(t.text_dim));
}

fn tile(ui: &mut Ui, label: &str, sample: Option<usize>) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 140.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let p = ui.painter();
    let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), 104.0));
    p.rect_filled(thumb, 4.0, t.grid_bg);
    let edge = if resp.hovered() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.ribbon_border) };
    let inner = thumb.shrink(10.0);
    let (rows, cols) = (7, 4);
    let (rh, cw) = (inner.height() / rows as f32, inner.width() / cols as f32);
    if let Some(i) = sample {
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
        p.line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], Stroke::new(1.0, t.gridline));
    }
    for c in 0..=cols {
        let x = inner.left() + c as f32 * cw;
        p.line_segment([pos2(x, inner.top()), pos2(x, inner.bottom())], Stroke::new(1.0, t.gridline));
    }
    p.rect_stroke(thumb, 4.0, edge, StrokeKind::Inside);
    let galley = p.layout(label.to_string(), theme::ui_font(12.5), t.text, rect.width());
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, thumb.bottom() + 6.0), galley, t.text);
    resp
}

fn new_tiles(app: &mut SheetApp, ui: &mut Ui, out: &mut Outcome, is_rtl: bool) {
    let lang = app.ui.language;
    let layout = if is_rtl {
        egui::Layout::right_to_left(egui::Align::Min)
    } else {
        egui::Layout::left_to_right(egui::Align::Min)
    };

    ui.with_layout(layout, |ui| {
        ui.spacing_mut().item_spacing = vec2(18.0, 14.0);

        if tile(ui, lang.tr("Blank workbook"), None).clicked() {
            app.run_or_alert("file.new", json!({}));
            out.close = true;
        }

        for (i, (name, title)) in gridcraft_engine::sample::SAMPLES.iter().enumerate() {
            if tile(ui, title, Some(i)).clicked() {
                app.run_or_alert("file.new", json!({"sample": name}));
                out.close = true;
            }
        }
    });
}

fn split_path(path: &str) -> (String, String) {
    let p = std::path::Path::new(path);
    let name = p.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(path).to_string();
    let folder = p.parent().and_then(std::path::Path::to_str).unwrap_or("").to_string();
    (name, folder)
}

const MODIFIED_W: f32 = 150.0;

fn recent_list(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome, limit: usize, is_rtl: bool) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    if app.ui.recent.is_empty() {
        dim(ui, lang.tr("No recent workbooks yet."));
        return;
    }
    let width = ui.available_width().min(CONTENT_W);
    let known = bs.recent.known(ui.ctx(), &app.ui.recent);
    let modified_w = if cfg!(target_arch = "wasm32") || width < 420.0 { 0.0 } else { MODIFIED_W };

    if modified_w > 0.0 {
        let (head, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
        let p = ui.painter();
        if is_rtl {
            p.text(pos2(head.right() - 10.0, head.center().y), Align2::RIGHT_CENTER, lang.tr("Name"), theme::ui_font(12.0), t.text_dim);
            p.text(pos2(head.left() + 10.0, head.center().y), Align2::LEFT_CENTER, lang.tr("Modified"), theme::ui_font(12.0), t.text_dim);
        } else {
            p.text(pos2(head.left() + 42.0, head.center().y), Align2::LEFT_CENTER, lang.tr("Name"), theme::ui_font(12.0), t.text_dim);
            p.text(pos2(head.right() - modified_w, head.center().y), Align2::LEFT_CENTER, lang.tr("Modified"), theme::ui_font(12.0), t.text_dim);
        }
        p.line_segment([pos2(head.left(), head.bottom()), pos2(head.right(), head.bottom())], Stroke::new(1.0, t.ribbon_border));
        ui.add_space(6.0);
    }

    let now = crate::now_ms() / 1000.0;
    for path in app.ui.recent.iter().take(limit).cloned().collect::<Vec<_>>() {
        let (name, folder) = split_path(&path);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 46.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, 4.0, t.hover);
        }

        if is_rtl {
            icons::paint(p, Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(20.0, 20.0)), Icon::Book, t.text);
            let room_left = if modified_w > 0.0 { rect.left() + modified_w + 14.0 } else { rect.left() };
            let text = p.with_clip_rect(Rect::from_min_max(pos2(room_left, rect.top()), pos2(rect.right() - 36.0, rect.bottom())).intersect(p.clip_rect()));
            text.text(pos2(rect.right() - 40.0, rect.center().y - 8.0), Align2::RIGHT_CENTER, &name, theme::ui_font(13.5), t.text);
            text.text(pos2(rect.right() - 40.0, rect.center().y + 9.0), Align2::RIGHT_CENTER, &folder, theme::ui_font(11.5), t.text_dim);

            if modified_w > 0.0 {
                let (words, color) = match known.get(&path) {
                    Some(Modified::At(seconds)) => (age(lang, now - seconds), t.text_dim),
                    Some(Modified::Unknown) => (lang.tr("Not available").to_string(), t.text_disabled),
                    None => (String::new(), t.text_dim),
                };
                p.text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, words, theme::ui_font(12.0), color);
            }
        } else {
            icons::paint(p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(20.0, 20.0)), Icon::Book, t.text);
            let room = if modified_w > 0.0 { rect.right() - modified_w - 14.0 } else { rect.right() };
            let text = p.with_clip_rect(Rect::from_min_max(rect.min, pos2(room, rect.bottom())).intersect(p.clip_rect()));
            text.text(pos2(rect.left() + 42.0, rect.center().y - 8.0), Align2::LEFT_CENTER, &name, theme::ui_font(13.5), t.text);
            text.text(pos2(rect.left() + 42.0, rect.center().y + 9.0), Align2::LEFT_CENTER, &folder, theme::ui_font(11.5), t.text_dim);

            if modified_w > 0.0 {
                let (words, color) = match known.get(&path) {
                    Some(Modified::At(seconds)) => (age(lang, now - seconds), t.text_dim),
                    Some(Modified::Unknown) => (lang.tr("Not available").to_string(), t.text_disabled),
                    None => (String::new(), t.text_dim),
                };
                p.text(pos2(rect.right() - modified_w, rect.center().y), Align2::LEFT_CENTER, words, theme::ui_font(12.0), color);
            }
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Modified {
    At(f64),
    Unknown,
}

static ASKING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Default)]
pub struct RecentFiles {
    asked: Vec<String>,
    found: Arc<Mutex<HashMap<String, Modified>>>,
}

impl RecentFiles {
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

#[cfg(not(target_arch = "wasm32"))]
fn ask(ctx: egui::Context, paths: Vec<String>, found: Arc<Mutex<HashMap<String, Modified>>>) -> bool {
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

#[cfg(target_arch = "wasm32")]
fn ask(_ctx: egui::Context, _paths: Vec<String>, _found: Arc<Mutex<HashMap<String, Modified>>>) -> bool {
    false
}

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

fn action_row(ui: &mut Ui, icon: Icon, title: &str, detail: &str, is_rtl: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().min(CONTENT_W);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 56.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 4.0, t.hover);
    }
    if is_rtl {
        icons::paint(p, Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), vec2(24.0, 24.0)), icon, t.text);
        p.text(pos2(rect.right() - 52.0, rect.center().y - 9.0), Align2::RIGHT_CENTER, title, theme::ui_bold(13.5), t.text);
        p.text(pos2(rect.right() - 52.0, rect.center().y + 10.0), Align2::RIGHT_CENTER, detail, theme::ui_font(12.0), t.text_dim);
    } else {
        icons::paint(p, Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), vec2(24.0, 24.0)), icon, t.text);
        p.text(pos2(rect.left() + 52.0, rect.center().y - 9.0), Align2::LEFT_CENTER, title, theme::ui_bold(13.5), t.text);
        p.text(pos2(rect.left() + 52.0, rect.center().y + 10.0), Align2::LEFT_CENTER, detail, theme::ui_font(12.0), t.text_dim);
    }
    resp
}

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

fn home(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome, is_rtl: bool) {
    let lang = app.ui.language;
    section(ui, lang, "New");
    new_tiles(app, ui, out, is_rtl);
    ui.add_space(22.0);
    section(ui, lang, "Recent");
    recent_list(app, ui, bs, out, 8, is_rtl);
    if app.ui.recent.len() > 8 {
        ui.add_space(6.0);
        if ui.link(lang.tr("More workbooks")).clicked() {
            out.page = Some(Page::Open);
        }
    }
}

fn new_page(app: &mut SheetApp, ui: &mut Ui, out: &mut Outcome, is_rtl: bool) {
    new_tiles(app, ui, out, is_rtl);
}

fn open_page(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome, is_rtl: bool) {
    let lang = app.ui.language;
    let layout = if is_rtl {
        egui::Layout::right_to_left(egui::Align::Min)
    } else {
        egui::Layout::left_to_right(egui::Align::Min)
    };
    ui.with_layout(layout, |ui| {
        if square_button(ui, Icon::OpenFolder, lang.tr("Browse…")).clicked() {
            let before = app.session.active().map(|d| d.uid);
            app.open_dialog("open", json!({}));
            if app.session.active().map(|d| d.uid) != before {
                out.close = true;
            }
        }
    });
    ui.add_space(22.0);
    section(ui, lang, "Recent");
    recent_list(app, ui, bs, out, usize::MAX, is_rtl);
}

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

fn info(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, is_rtl: bool) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());

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


    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (icon_rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
            icons::paint(ui.painter(), icon_rect, Icon::Book, t.text);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.label(RichText::new(&title).font(theme::ui_bold(16.0)).color(t.text));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    match &path {
                        Some(p) => dim(ui, p),
                        None => dim(ui, lang.tr("Not saved yet")),
                    }
                });
            });
        });
    } else {
        ui.horizontal(|ui| {
            let (icon_rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
            icons::paint(ui.painter(), icon_rect, Icon::Book, t.text);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(&title).font(theme::ui_bold(16.0)).color(t.text));
                match &path {
                    Some(p) => dim(ui, p),
                    None => dim(ui, lang.tr("Not saved yet")),
                }
            });
        });
    }

    ui.add_space(18.0);

    let desc_key = if protected {
        "Sheet structure is locked: sheets can't be added, moved, renamed or deleted."
    } else {
        "Lock the sheet structure so sheets can't be added, moved, renamed or deleted."
    };

    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if square_button(ui, Icon::Lock, lang.tr("Protect Workbook")).clicked() {
                protect_workbook(app);
            }
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(vec2(360.0, 88.0), egui::Layout::top_down(egui::Align::Max), |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(lang.tr("Protect Workbook")).font(theme::ui_bold(13.5)).color(t.text));
                ui.add_space(4.0);
                dim(ui, lang.tr(desc_key));
            });
        });
    } else {
        ui.horizontal(|ui| {
            if square_button(ui, Icon::Lock, lang.tr("Protect Workbook")).clicked() {
                protect_workbook(app);
            }
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(340.0);
                ui.add_space(8.0);
                ui.label(RichText::new(lang.tr("Protect Workbook")).font(theme::ui_bold(13.5)).color(t.text));
                dim(ui, lang.tr(desc_key));
            });
        });
    }

    ui.add_space(22.0);
    section(ui, lang, "Properties");

    if is_rtl {
        let label_w = 110.0;
        let field_w = 340.0;
        let gap_w = 16.0;

        let draw_meta_row = |ui: &mut Ui, label_text: &str, val_text: &str| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label_rect, _) = ui.allocate_exact_size(vec2(label_w, 24.0), Sense::hover());
                ui.painter().text(pos2(label_rect.right(), label_rect.center().y), Align2::RIGHT_CENTER, label_text, theme::ui_font(12.5), t.text_dim);

                ui.add_space(gap_w);

                let (val_rect, _) = ui.allocate_exact_size(vec2(field_w, 24.0), Sense::hover());
                ui.painter().text(pos2(val_rect.right(), val_rect.center().y), Align2::RIGHT_CENTER, val_text, theme::ui_font(12.5), t.text);
            });
            ui.add_space(6.0);
        };

        draw_meta_row(ui, lang.tr("Location"), path.as_deref().unwrap_or(lang.tr("Not saved yet")));
        draw_meta_row(ui, lang.tr("Sheets"), &sheets.to_string());

        for (i, (key, label)) in PROPERTIES.iter().enumerate() {
            let (Some(draft), Some(saved)) = (bs.props.get_mut(i), current.get(i)) else { continue };
            let id = ui.make_persistent_id(("backstage_prop", *key));
            if !ui.ctx().memory(|m| m.has_focus(id)) {
                draft.clone_from(saved);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label_rect, _) = ui.allocate_exact_size(vec2(label_w, 24.0), Sense::hover());
                ui.painter().text(pos2(label_rect.right(), label_rect.center().y), Align2::RIGHT_CENTER, lang.tr(label), theme::ui_font(12.5), t.text_dim);

                ui.add_space(gap_w);

                let resp = ui.add_sized(
                    vec2(field_w, 22.0),
                    egui::TextEdit::singleline(draft)
                        .id(id)
                        .font(theme::ui_font(12.5))
                        .margin(egui::Margin::symmetric(6, 2)),
                );
                if resp.lost_focus() && draft != saved {
                    let mut params = serde_json::Map::new();
                    params.insert((*key).to_string(), json!(draft.clone()));
                    app.run_or_alert("file.properties", Json::Object(params));
                }
            });
            ui.add_space(6.0);
        }
    } else {
        egui::Grid::new("backstage_props").num_columns(2).spacing(vec2(18.0, 8.0)).show(ui, |ui| {
            ui.label(RichText::new(lang.tr("Location")).color(t.text_dim));
            ui.label(path.as_deref().unwrap_or(lang.tr("Not saved yet")));
            ui.end_row();
            ui.label(RichText::new(lang.tr("Sheets")).color(t.text_dim));
            ui.label(sheets.to_string());
            ui.end_row();
            for (i, (key, label)) in PROPERTIES.iter().enumerate() {
                let (Some(draft), Some(saved)) = (bs.props.get_mut(i), current.get(i)) else { continue };
                let label_widget = ui.label(RichText::new(lang.tr(label)).color(t.text_dim));
                let id = ui.make_persistent_id(("backstage_prop", *key));
                if !ui.ctx().memory(|m| m.has_focus(id)) {
                    draft.clone_from(saved);
                }
                let resp = ui.add(egui::TextEdit::singleline(draft).id(id).desired_width(360.0)).labelled_by(label_widget.id);
                if resp.lost_focus() && draft != saved {
                    let mut params = serde_json::Map::new();
                    params.insert((*key).to_string(), json!(draft.clone()));
                    app.run_or_alert("file.properties", Json::Object(params));
                }
                ui.end_row();
            }
        });
    }
}



fn save_as(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome, is_rtl: bool) {
    let lang = app.ui.language;
    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            dim(ui, lang.tr("Save the workbook under another name or in another folder."));
        });
    } else {
        dim(ui, lang.tr("Save the workbook under another name or in another folder."));
    }
    ui.add_space(14.0);

    let fmt = bs.save_format;
    let current = SAVE_TYPES.iter().find(|(ext, _)| *ext == fmt).map_or("Excel Workbook (*.xlsx)", |(_, label)| *label);

    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(lang.tr("Save as type"));
            ui.add_space(12.0);
            craft_ui::ComboBox::from_id_salt("backstage_save_type")
                .width(280.0)
                .selected_text(lang.tr(current))
                .show_ui(ui, |ui| {
                    for (ext, label) in SAVE_TYPES {
                        if ui.selectable_label(fmt == ext, lang.tr(label)).clicked() {
                            bs.save_format = ext;
                        }
                    }
                });
        });
    } else {
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
    }

    ui.add_space(14.0);
    let web = app.services.pick_save.is_none() && app.services.download.is_some();
    let label = if web { format!("{} (.{fmt})", lang.tr("Download")) } else { lang.tr("Browse…").to_string() };

    let on_browse = |app: &mut SheetApp, out: &mut Outcome| {
        if app.commit_edit(0, 0, false, false) {
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
    };

    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if square_button(ui, Icon::OpenFolder, &label).clicked() {
                on_browse(app, out);
            }
        });
    } else if square_button(ui, Icon::OpenFolder, &label).clicked() {
        on_browse(app, out);
    }

    ui.add_space(18.0);
    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            dim(ui, lang.tr("A CSV file, web page or PDF is a copy: the workbook keeps its own file."));
        });
    } else {
        dim(ui, lang.tr("A CSV file, web page or PDF is a copy: the workbook keeps its own file."));
    }
}

fn export(app: &mut SheetApp, ui: &mut Ui, out: &mut Outcome, is_rtl: bool) {
    let lang = app.ui.language;
    if action_row(ui, Icon::Paper, lang.tr("Create PDF"), lang.tr("Save the sheet as a PDF document that keeps its page layout."), is_rtl).clicked()
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
        if action_row(ui, icon, lang.tr(title), lang.tr(detail), is_rtl).clicked() && export_as(app, fmt) {
            out.close = true;
        }
    }
}

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

fn account(app: &mut SheetApp, ui: &mut Ui, is_rtl: bool) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    section(ui, lang, "Product Information");

    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("GridCraft").font(theme::ui_bold(16.0)).color(t.text));
        });
    } else {
        ui.label(RichText::new("GridCraft").font(theme::ui_bold(16.0)).color(t.text));
    }
    ui.add_space(8.0);

    let rows = [
        ("Version", env!("CARGO_PKG_VERSION").to_string()),
        ("Platform", platform()),
        ("Licence", env!("CARGO_PKG_LICENSE").to_string()),
    ];

    if is_rtl {
        let label_w = 110.0;
        let val_w = 340.0;
        let gap_w = 16.0;

        for (label, value) in rows {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label_rect, _) = ui.allocate_exact_size(vec2(label_w, 24.0), Sense::hover());
                ui.painter().text(pos2(label_rect.right(), label_rect.center().y), Align2::RIGHT_CENTER, lang.tr(label), theme::ui_font(12.5), t.text_dim);

                ui.add_space(gap_w);

                let (val_rect, _) = ui.allocate_exact_size(vec2(val_w, 24.0), Sense::hover());
                ui.painter().text(pos2(val_rect.right(), val_rect.center().y), Align2::RIGHT_CENTER, &value, theme::ui_font(12.5), t.text);
            });
            ui.add_space(6.0);
        }
    } else {
        egui::Grid::new("backstage_account").num_columns(2).spacing(vec2(18.0, 8.0)).show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(lang.tr(label)).color(t.text_dim));
                ui.label(value);
                ui.end_row();
            }
        });
    }

    ui.add_space(18.0);

    if is_rtl {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if square_button(ui, Icon::Info, lang.tr("About GridCraft")).clicked() {
                app.open_dialog("about", json!({}));
            }
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(vec2(360.0, 88.0), egui::Layout::top_down(egui::Align::Max), |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(lang.tr("About GridCraft")).font(theme::ui_bold(13.5)).color(t.text));
                ui.add_space(4.0);
                dim(ui, lang.tr("Version, contributors and the AI models that helped"));
            });
        });
    } else {
        ui.horizontal(|ui| {
            if square_button(ui, Icon::Info, lang.tr("About GridCraft")).clicked() {
                app.open_dialog("about", json!({}));
            }
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(340.0);
                ui.add_space(8.0);
                ui.label(RichText::new(lang.tr("About GridCraft")).font(theme::ui_bold(13.5)).color(t.text));
                dim(ui, lang.tr("Version, contributors and the AI models that helped"));
            });
        });
    }
}