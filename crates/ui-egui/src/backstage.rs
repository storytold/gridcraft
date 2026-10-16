//! File tab Backstage view (#174): a full-window page that replaces the ribbon, formula bar, grid
//! and sheet tabs until Esc or the back arrow returns to the sheet.
//!
//! A navigation column on the left picks a page (Home, New, Open, Info, Save As, Print, Export,
//! Options); Save and Close act at once. Every action runs an existing command through
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
}

impl Page {
    pub const ALL: [Page; 8] = [Page::Home, Page::New, Page::Open, Page::Info, Page::SaveAs, Page::Print, Page::Export, Page::Options];

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
    /// Print: orientation (`auto`, `portrait`, `landscape`).
    pub orientation: String,
    /// Print: the previewed page (0-based).
    preview_page: usize,
    /// Print: the preview for (document, revision, whole workbook, fit to page, orientation).
    preview: Option<((u64, u64, bool, bool, String), Json)>,
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
            orientation: "auto".into(),
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
    let (nav_bg, nav_text, nav_sel) =
        if t.dark { (t.accent_soft, t.text, Color32::from_white_alpha(30)) } else { (t.accent, Color32::WHITE, Color32::from_black_alpha(46)) };
    egui::Panel::left("backstage_nav")
        .exact_size(NAV_W)
        .resizable(false)
        .frame(egui::Frame::NONE.fill(nav_bg).inner_margin(egui::Margin { left: 0, right: 0, top: 10, bottom: 10 }))
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
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                if nav_item(ui, Page::Options.icon(), lang.tr("Options"), bs.page == Page::Options, nav_text, nav_sel).clicked() {
                    out.page = Some(Page::Options);
                }
            });
        });
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.ribbon)).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 44, right: 32, top: 26, bottom: 32 }).show(ui, |ui| {
                ui.set_max_width(CONTENT_W);
                ui.label(RichText::new(lang.tr(bs.page.label())).font(theme::ui_font(28.0)).color(t.text));
                ui.add_space(18.0);
                match bs.page {
                    Page::Home => home(app, ui, &mut bs, &mut out),
                    Page::New => new_page(app, ui, &mut out),
                    Page::Open => open_page(app, ui, &mut bs, &mut out),
                    Page::Info => info(app, ui, &mut bs),
                    Page::SaveAs => save_as(app, ui, &mut bs, &mut out),
                    Page::Print => print(app, ui, &mut bs, &mut out),
                    Page::Export => export(app, ui, &mut out),
                    Page::Options => options(app, ui),
                }
            });
        });
    });
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

fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool, text: Color32, sel: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(NAV_W, 38.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 0.0, sel);
        p.rect_filled(Rect::from_min_size(rect.min, vec2(4.0, rect.height())), 0.0, text);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, sel.gamma_multiply(0.55));
    }
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), vec2(17.0, 17.0)), icon, text);
    p.text(pos2(rect.left() + 50.0, rect.center().y), Align2::LEFT_CENTER, label, theme::ui_font(14.0), text);
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
    ui.label(RichText::new(lang.tr(title)).font(theme::ui_bold(15.0)).color(t.text));
    ui.add_space(8.0);
}

fn dim(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).font(theme::ui_font(12.5)).color(t.text_dim));
}

/// A new-workbook tile: a small sheet drawn in code over its name.
fn tile(ui: &mut Ui, label: &str, sample: Option<usize>) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(150.0, 140.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter();
    let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), 104.0));
    p.rect_filled(thumb, 4.0, t.grid_bg);
    let edge = if resp.hovered() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.ribbon_border) };
    let inner = thumb.shrink(10.0);
    let (rows, cols) = (7, 4);
    let (rh, cw) = (inner.height() / rows as f32, inner.width() / cols as f32);
    if let Some(i) = sample {
        // A header band and a few "values" so each sample reads differently from a blank book.
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
    p.galley(pos2(rect.left(), thumb.bottom() + 6.0), galley, t.text);
    resp
}

/// Blank workbook and the built-in samples.
fn new_tiles(app: &mut SheetApp, ui: &mut Ui, out: &mut Outcome) {
    let lang = app.ui.language;
    ui.horizontal_wrapped(|ui| {
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

/// File name and folder of a recent path, split as text.
fn split_path(path: &str) -> (String, String) {
    let p = std::path::Path::new(path);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string();
    let folder = p.parent().and_then(|f| f.to_str()).unwrap_or("").to_string();
    (name, folder)
}

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
    if modified_w > 0.0 {
        let (head, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
        ui.painter().text(
            pos2(head.right() - modified_w, head.center().y),
            Align2::LEFT_CENTER,
            lang.tr("Modified"),
            theme::ui_font(11.5),
            t.text_dim,
        );
    }
    let now = crate::now_ms() / 1000.0;
    for path in app.ui.recent.iter().take(limit).cloned().collect::<Vec<_>>() {
        let (name, folder) = split_path(&path);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, 4.0, t.hover);
        }
        icons::paint(p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(20.0, 20.0)), Icon::Book, t.text);
        // A long name or folder stops short of the column instead of running under it.
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
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
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
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let p = ui.painter();
    p.rect_filled(rect, 6.0, if resp.hovered() { t.hover } else { t.ribbon });
    p.rect_stroke(rect, 6.0, Stroke::new(1.0, if resp.hovered() { t.accent } else { t.ribbon_border }), StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(pos2(rect.center().x, rect.top() + 32.0), vec2(30.0, 30.0)), icon, t.text);
    let galley = p.layout(label.to_string(), theme::ui_font(12.0), t.text, rect.width() - 8.0);
    p.galley(pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 56.0), galley, t.text);
    resp
}

// ---------------------------------------------------------------- pages

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
            ui.label(RichText::new(lang.tr(label)).color(t.text_dim));
            let id = ui.make_persistent_id(("backstage_prop", *key));
            // Show the workbook's value unless the field is being edited (undo, another window).
            if !ui.ctx().memory(|m| m.has_focus(id)) {
                draft.clone_from(saved);
            }
            let resp = ui.add(egui::TextEdit::singleline(draft).id(id).desired_width(360.0));
            if resp.lost_focus() && draft != saved {
                let mut params = serde_json::Map::new();
                params.insert((*key).to_string(), json!(draft.clone()));
                app.run_or_alert("file.properties", Json::Object(params));
            }
            ui.end_row();
        }
    });
}

fn save_as(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let lang = app.ui.language;
    dim(ui, lang.tr("Save the workbook under another name or in another folder."));
    ui.add_space(14.0);
    let fmt = bs.save_format;
    let current = SAVE_TYPES.iter().find(|(ext, _)| *ext == fmt).map_or("Excel Workbook (*.xlsx)", |(_, label)| *label);
    ui.horizontal(|ui| {
        ui.label(lang.tr("Save as type"));
        egui::ComboBox::from_id_salt("backstage_save_type").width(280.0).selected_text(lang.tr(current)).show_ui(ui, |ui| {
            for (ext, label) in SAVE_TYPES {
                if ui.selectable_label(fmt == ext, lang.tr(label)).clicked() {
                    bs.save_format = ext;
                }
            }
        });
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

fn print(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let Some((uid, revision)) = app.session.active().map(|d| (d.uid, d.revision)) else {
        dim(ui, lang.tr("No workbook is open."));
        return;
    };
    let mut params = json!({"sheets": if bs.print_all { "all" } else { "active" }});
    if bs.fit_to_page {
        params["fitToPage"] = json!(true);
    }
    if bs.orientation != "auto" {
        params["orientation"] = json!(bs.orientation);
    }
    let key = (uid, revision, bs.print_all, bs.fit_to_page, bs.orientation.clone());
    if bs.preview.as_ref().is_none_or(|(k, _)| *k != key) {
        let preview = app.session.run("file.printPreview", params.clone()).unwrap_or(Json::Null);
        bs.preview = Some((key, preview));
    }
    let pages: Vec<Json> = bs.preview.as_ref().and_then(|(_, p)| p.get("pageList")).and_then(Json::as_array).cloned().unwrap_or_default();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            if let Some(discover) = &app.services.printers {
                crate::printing::show(&mut app.printing, ui, discover, |label| lang.tr(label).to_owned());
            }
            let can_print = app.services.print_native.is_none() || app.printing.ready();
            if ui.add_enabled_ui(can_print, |ui| square_button(ui, Icon::Print, lang.tr("Print"))).inner.clicked()
                && app.commit_edit(0, 0, false, false)
            {
                let mut submit_params = params.clone();
                submit_params["orientation"] = json!(bs.orientation);
                app.print_now(submit_params);
                out.close = app.services.print_native.is_none();
            }
            ui.add_space(16.0);
            let scope = if bs.print_all { "Print Entire Workbook" } else { "Print Active Sheets" };
            egui::ComboBox::from_id_salt("backstage_print_scope").width(230.0).selected_text(lang.tr(scope)).show_ui(ui, |ui| {
                for (all, label) in [(false, "Print Active Sheets"), (true, "Print Entire Workbook")] {
                    if ui.selectable_label(bs.print_all == all, lang.tr(label)).clicked() {
                        bs.print_all = all;
                        bs.preview_page = 0;
                    }
                }
            });
            ui.add_space(8.0);
            if ui.checkbox(&mut bs.fit_to_page, lang.tr("Fit Sheet on One Page")).changed() {
                bs.preview_page = 0;
            }
            ui.add_space(8.0);
            let orientation_label = match bs.orientation.as_str() {
                "portrait" => "Portrait",
                "landscape" => "Landscape",
                _ => "Auto orientation",
            };
            egui::ComboBox::from_id_salt("backstage_print_orientation").width(230.0).selected_text(lang.tr(orientation_label)).show_ui(ui, |ui| {
                for (val, label) in [("auto", "Auto orientation"), ("portrait", "Portrait"), ("landscape", "Landscape")] {
                    if ui.selectable_label(bs.orientation == val, lang.tr(label)).clicked() {
                        bs.orientation = val.into();
                        bs.preview_page = 0;
                    }
                }
            });
            ui.add_space(8.0);
            if ui.link(lang.tr("Page Setup…")).clicked() {
                app.open_dialog("pageSetup", json!({}));
            }
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            if pages.is_empty() {
                dim(ui, lang.tr("Nothing to print."));
                return;
            }
            let last = pages.len().saturating_sub(1);
            bs.preview_page = bs.preview_page.min(last);
            let Some(page) = pages.get(bs.preview_page) else { return };
            // Paper proportions from the preview (points); hostile sizes fall back to Letter.
            let w = page.get("width").and_then(Json::as_f64).filter(|w| w.is_finite() && *w > 1.0).unwrap_or(612.0) as f32;
            let h = page.get("height").and_then(Json::as_f64).filter(|h| h.is_finite() && *h > 1.0).unwrap_or(792.0) as f32;
            let scale = (520.0 / h).min(440.0 / w);
            let (rect, _) = ui.allocate_exact_size(vec2(w * scale, h * scale), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect.translate(vec2(2.0, 3.0)), 2.0, t.shadow);
            p.rect_filled(rect, 2.0, t.grid_bg);
            p.rect_stroke(rect, 2.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            if let Some(d) = app.session.active() {
                paint_page(p, rect, w, page, &d.wb, &t);
            }
            let sheet = page.get("sheet").and_then(Json::as_str).unwrap_or("");
            let range = page.get("range").and_then(Json::as_str).unwrap_or("");
            ui.add_space(4.0);
            dim(ui, &format!("{sheet} · {range}"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(bs.preview_page > 0, egui::Button::new("◀")).on_hover_text(lang.tr("Previous page")).clicked() {
                    bs.preview_page = bs.preview_page.saturating_sub(1);
                }
                let text = lang
                    .tr("Page {page} of {pages}")
                    .replace("{page}", &(bs.preview_page + 1).to_string())
                    .replace("{pages}", &pages.len().to_string());
                ui.label(text);
                if ui.add_enabled(bs.preview_page < last, egui::Button::new("▶")).on_hover_text(lang.tr("Next page")).clicked() {
                    bs.preview_page = (bs.preview_page + 1).min(last);
                }
            });
        });
    });
}

/// Most rows and columns one preview page draws (a page holds far fewer at any readable scale).
const PREVIEW_ROWS: u32 = 200;
const PREVIEW_COLS: u32 = 60;

/// Draws one printed page in miniature: the cells' displayed text at the page's print scale,
/// inside the sheet's margins. `paper_w` is the page width in points.
fn paint_page(p: &egui::Painter, rect: Rect, paper_w: f32, page: &Json, wb: &gridcraft_engine::model::Workbook, t: &Tokens) {
    let Some(range) = page.get("range").and_then(Json::as_str).and_then(gridcraft_engine::core::RangeRef::parse) else { return };
    let Some(sh) = page.get("sheet").and_then(Json::as_str).and_then(|n| wb.sheet_index(n)).and_then(|i| wb.sheets.get(i)) else { return };
    // Preview pixels per point, and per sheet pixel (sheet geometry is 96 dpi: 0.75 pt per pixel).
    let per_pt = rect.width() / paper_w;
    let print_scale =
        page.get("scale").and_then(Json::as_f64).filter(|s| s.is_finite() && *s > 0.0).map_or(1.0, |s| (s / 100.0) as f32).clamp(0.1, 4.0);
    let per_px = per_pt * 0.75 * print_scale;
    let margin =
        |i: usize, default: f32| sh.print.margins.get(i).copied().filter(|m| m.is_finite()).unwrap_or(default).clamp(0.0, 3.0) * 72.0 * per_pt;
    let body = Rect::from_min_max(
        pos2(rect.left() + margin(0, 0.7), rect.top() + margin(2, 0.75)),
        pos2(rect.right() - margin(1, 0.7), rect.bottom() - margin(3, 0.75)),
    );
    if !body.is_positive() {
        return;
    }
    let font = theme::ui_font((11.0 * per_pt * print_scale).clamp(3.0, 14.0));
    let last_row = range.end.row.min(range.start.row.saturating_add(PREVIEW_ROWS));
    let last_col = range.end.col.min(range.start.col.saturating_add(PREVIEW_COLS));
    let mut y = body.top();
    for row in range.start.row..=last_row {
        let h = sh.row_height(row) * per_px;
        if h <= 0.0 {
            continue;
        }
        if y >= body.bottom() {
            break;
        }
        let mut x = body.left();
        for col in range.start.col..=last_col {
            let w = sh.col_width(col) * per_px;
            if w <= 0.0 {
                continue;
            }
            if x >= body.right() {
                break;
            }
            let cell = gridcraft_engine::core::CellRef::new(row, col);
            let text = gridcraft_engine::display::cell_text(wb, sh, cell);
            if !text.is_empty() {
                let numeric = sh.value(cell).as_f64().is_some();
                // Text runs on over empty neighbours, as on the sheet; numbers stay in their cell.
                let mut span = w;
                if !numeric {
                    let mut next = col.saturating_add(1);
                    while next <= last_col && x + span < body.right() && sh.value(gridcraft_engine::core::CellRef::new(row, next)).is_empty() {
                        span += sh.col_width(next) * per_px;
                        next = next.saturating_add(1);
                    }
                }
                let clip = Rect::from_min_size(pos2(x, y), vec2(span, h)).intersect(body);
                let (anchor, at) =
                    if numeric { (Align2::RIGHT_CENTER, pos2(x + w - 1.0, y + h / 2.0)) } else { (Align2::LEFT_CENTER, pos2(x + 1.0, y + h / 2.0)) };
                p.with_clip_rect(clip).text(at, anchor, text, font.clone(), t.cell_text);
            }
            x += w;
        }
        y += h;
    }
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
    let stem = std::path::Path::new(&title).file_stem().and_then(|s| s.to_str()).unwrap_or("Book1").to_string();
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

fn options(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    section(ui, lang, "Options|General");
    egui::Grid::new("backstage_options_general").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
        ui.label(lang.tr("Display theme"));
        let mode = app.ui.theme_mode();
        let current = match mode {
            "system" => "System",
            "dark" => "Dark",
            _ => "Light",
        };
        egui::ComboBox::from_id_salt("backstage_theme").width(160.0).selected_text(lang.tr(current)).show_ui(ui, |ui| {
            for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                if ui.selectable_label(mode == value, lang.tr(label)).clicked() {
                    app.run_or_alert("view.theme", json!({"mode": value}));
                }
            }
        });
        ui.end_row();
        ui.label(lang.tr("Interface language"));
        egui::ComboBox::from_id_salt("backstage_language").width(160.0).selected_text(lang.name()).show_ui(ui, |ui| {
            for l in i18n::Language::ALL {
                if ui.selectable_label(l == lang, l.name()).clicked() {
                    app.run_or_alert("app.language.set", json!({"language": l.code()}));
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(6.0);
    let mut formula_bar = app.session.prefs.formula_bar;
    if ui.checkbox(&mut formula_bar, lang.tr("Show formula bar")).changed() {
        app.run_or_alert("view.formulaBar", json!({"on": formula_bar}));
    }
    ui.add_space(18.0);
    section(ui, lang, "Options|Editing");
    let mut autocomplete = app.ui.autocomplete;
    if ui.checkbox(&mut autocomplete, lang.tr("Enable AutoComplete for cell values")).changed() {
        app.run_or_alert("options.autoComplete", json!({"on": autocomplete}));
    }
    ui.add_space(18.0);
    section(ui, lang, "Options|Save");
    let mut autosave = app.grid.autosave();
    if ui.checkbox(&mut autosave, lang.tr("Save .xlsx workbooks automatically after every change")).changed() {
        app.grid.toggle_autosave();
    }
    let Some(calc) = app.session.active().map(|d| d.wb.calc.clone()) else { return };
    ui.add_space(18.0);
    section(ui, lang, "Formulas");
    ui.label(lang.tr("Workbook calculation"));
    use gridcraft_engine::model::CalcMode;
    for (mode, id, label) in [
        (CalcMode::Automatic, "automatic", "Automatic"),
        (CalcMode::AutomaticExceptTables, "automaticExceptTables", "Automatic Except for Data Tables"),
        (CalcMode::Manual, "manual", "Manual"),
    ] {
        if ui.radio(calc.mode == mode, lang.tr(label)).clicked() && calc.mode != mode {
            app.run_or_alert("formulas.calculationOptions", json!({"mode": id}));
        }
    }
    ui.add_space(4.0);
    let mut iterative = calc.iterative;
    if ui.checkbox(&mut iterative, lang.tr("Enable iterative calculation")).changed() {
        app.run_or_alert("formulas.calculationOptions", json!({"iterative": iterative}));
    }
    ui.add_space(6.0);
    dim(ui, lang.tr("Calculation settings are saved with the workbook."));
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
        assert_eq!(age(Language::Ja, 3.0 * DAY), "3 日前");
        assert_eq!(age(Language::De, 3.0 * DAY), "vor 3 Tagen");
        assert_eq!(age(Language::Fr, 90.0), "il y a 1 min");
        for lang in Language::ALL {
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
    fn page_ids_round_trip_and_unknown_ids_are_refused() {
        for page in Page::ALL {
            assert_eq!(Page::parse(page.id()), Some(page));
        }
        assert_eq!(Page::parse(" SAVEAS "), Some(Page::SaveAs));
        assert_eq!(Page::parse("account"), None);
        assert_eq!(Page::parse(""), None);
    }

    #[test]
    fn recent_paths_split_without_touching_the_disk() {
        assert_eq!(split_path("/home/me/Budget.xlsx"), ("Budget.xlsx".into(), "/home/me".into()));
        assert_eq!(split_path("Budget.xlsx"), ("Budget.xlsx".into(), "".into()));
    }
}
