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
    /// Print: the previewed page (0-based).
    preview_page: usize,
    /// Print: the preview for (document, revision, whole workbook, fit to page).
    preview: Option<((u64, u64, bool, bool), Json)>,
    /// Print: margins and the table's place are drawn on the preview and can be dragged.
    pub show_margins: bool,
    /// Print: margins being dragged, shown live until the drag ends.
    margin_draft: Option<MarginDraft>,
    /// Print: margins in centimetres (else inches); decided on first show.
    metric: Option<bool>,
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
            show_margins: false,
            margin_draft: None,
            metric: None,
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

/// Normal margins in inches, in [`MARGIN_KEYS`] order.
const NORMAL_MARGINS: [f32; 6] = [0.7, 0.7, 0.75, 0.75, 0.3, 0.3];
/// Margin names as `pageLayout.pageSetup` takes them.
const MARGIN_KEYS: [&str; 6] = ["left", "right", "top", "bottom", "header", "footer"];
const MARGIN_LABELS: [&str; 6] = ["Left", "Right", "Top", "Bottom", "Header", "Footer"];
/// Printable width and height (inches) a dragged margin always leaves, as pagination does.
const MIN_PRINTABLE: f32 = 1.0;
/// Pointer travel (points) along an axis before dragging the table moves it on that axis.
const TABLE_SLOP: f32 = 3.0;
/// How close (screen points) the pointer must be to a margin line to grab it.
const GRAB: f32 = 5.0;
/// Room around the previewed page for the margin handles.
const HANDLE_ROOM: f32 = 12.0;

/// What a drag on the print preview moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragTarget {
    /// One margin, by its index in [`MARGIN_KEYS`].
    Margin(usize),
    /// The table: the left/right and top/bottom margins shift together.
    Table,
}

/// Margins being changed on the Print page (dragged on the preview or in a field), shown live and
/// committed through `pageLayout.pageSetup` as one undo step when the drag ends.
#[derive(Clone, Debug)]
pub struct MarginDraft {
    sheet: String,
    /// Set while the pointer drags on the preview (a field drag has none).
    target: Option<DragTarget>,
    /// Margins (inches) and centring when the drag started.
    start: [f32; 6],
    start_center: (bool, bool),
    /// Where the printed block sat when the drag started: [x, y, w, h] in points.
    block: [f32; 4],
    /// Pointer travel so far, in points.
    moved: egui::Vec2,
    margins: [f32; 6],
    center: (bool, bool),
}

/// Margins (inches) and centring after dragging `target` by `moved` points on paper `paper`
/// (points), the printed block having been at `block` ([x, y, w, h] points).
fn dragged_margins(
    target: DragTarget,
    start: [f32; 6],
    center: (bool, bool),
    paper: (f32, f32),
    block: [f32; 4],
    moved: egui::Vec2,
) -> ([f32; 6], (bool, bool)) {
    let (pw, ph) = (paper.0 / 72.0, paper.1 / 72.0);
    let (dx, dy) = (moved.x / 72.0, moved.y / 72.0);
    let mut m = start;
    let mut c = center;
    let room = |total: f32, other: f32| (total - other - MIN_PRINTABLE).max(0.0);
    match target {
        DragTarget::Margin(0) => m[0] = (start[0] + dx).clamp(0.0, room(pw, start[1])),
        DragTarget::Margin(1) => m[1] = (start[1] - dx).clamp(0.0, room(pw, start[0])),
        DragTarget::Margin(2) => m[2] = (start[2] + dy).clamp(0.0, room(ph, start[3])),
        DragTarget::Margin(3) => m[3] = (start[3] - dy).clamp(0.0, room(ph, start[2])),
        DragTarget::Margin(4) => m[4] = (start[4] + dy).clamp(0.0, ph / 2.0),
        DragTarget::Margin(_) => m[5] = (start[5] - dy).clamp(0.0, ph / 2.0),
        DragTarget::Table => {
            // The table moves by its near margin; the far one gives way, so the printable size
            // (and with it the page breaks) stays put as long as there is room.
            let axes = [(moved.x, 0usize, 1usize, block[0], block[2], pw), (moved.y, 2, 3, block[1], block[3], ph)];
            for (travel, near, far, at, size, total) in axes {
                if travel.abs() < TABLE_SLOP {
                    continue;
                }
                let (Some(&n0), Some(&f0)) = (start.get(near), start.get(far)) else { continue };
                let size = (size / 72.0).max(MIN_PRINTABLE);
                let pos = (at / 72.0 + travel / 72.0).clamp(0.0, (total - size).max(0.0));
                if let Some(slot) = m.get_mut(near) {
                    *slot = pos;
                }
                if let Some(slot) = m.get_mut(far) {
                    *slot = (n0 + f0 - pos).max(0.0);
                }
                if near == 0 {
                    c.0 = false;
                } else {
                    c.1 = false;
                }
            }
        }
    }
    (m, c)
}

/// Where the printed block of size `size` (points) starts on the paper for these margins.
fn block_origin(m: [f32; 6], center: (bool, bool), paper: (f32, f32), size: (f32, f32)) -> egui::Vec2 {
    let [l, r, t, b, ..] = m.map(|v| v * 72.0);
    let x = l + if center.0 { ((paper.0 - l - r - size.0) / 2.0).max(0.0) } else { 0.0 };
    let y = t + if center.1 { ((paper.1 - t - b - size.1) / 2.0).max(0.0) } else { 0.0 };
    vec2(x, y)
}

/// Rounds inches to what the margin fields show: whole millimetres or hundredths of an inch.
fn round_margin(inches: f32, metric: bool) -> f32 {
    if metric { (inches * 25.4).round() / 25.4 } else { (inches * 100.0).round() / 100.0 }
}

/// The page's margins (inches) from the preview, hostile values replaced by Normal.
fn page_margins(page: &Json) -> [f32; 6] {
    let m = page.get("margins");
    let mut out = NORMAL_MARGINS;
    for (slot, key) in out.iter_mut().zip(MARGIN_KEYS) {
        if let Some(v) = m.and_then(|m| m.get(key)).and_then(Json::as_f64).filter(|v| v.is_finite() && *v >= 0.0) {
            *slot = v.min(20.0) as f32;
        }
    }
    out
}

fn num(v: Option<&Json>, key: &str) -> Option<f32> {
    v.and_then(|v| v.get(key)).and_then(Json::as_f64).filter(|v| v.is_finite()).map(|v| v as f32)
}

/// Saves margins and centring for `sheet` as one undo step.
fn commit_margins(app: &mut SheetApp, sheet: &str, margins: [f32; 6], center: (bool, bool), metric: bool) {
    let mut m = serde_json::Map::new();
    for (key, v) in MARGIN_KEYS.iter().zip(margins) {
        m.insert((*key).to_string(), json!(round_margin(v, metric)));
    }
    app.run_or_alert("pageLayout.pageSetup", json!({"sheet": sheet, "margins": m, "centerH": center.0, "centerV": center.1}));
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
    let key = (uid, revision, bs.print_all, bs.fit_to_page);
    if bs.preview.as_ref().is_none_or(|(k, _)| *k != key) {
        let preview = app.session.run("file.printPreview", params.clone()).unwrap_or(Json::Null);
        bs.preview = Some((key, preview));
    }
    let pages: Vec<Json> = bs.preview.as_ref().and_then(|(_, p)| p.get("pageList")).and_then(Json::as_array).cloned().unwrap_or_default();
    let last = pages.len().saturating_sub(1);
    bs.preview_page = bs.preview_page.min(last);
    let page = pages.get(bs.preview_page).cloned();
    let sheet = page.as_ref().and_then(|p| p.get("sheet")).and_then(Json::as_str).unwrap_or("").to_string();
    // A draft for another sheet (the user paged on) no longer applies.
    if bs.margin_draft.as_ref().is_some_and(|d| d.sheet != sheet) {
        bs.margin_draft = None;
    }
    let metric = *bs.metric.get_or_insert_with(|| {
        let paper = page.as_ref().and_then(|p| p.get("paper")).and_then(Json::as_str).unwrap_or("");
        !matches!(lang, Language::En) || paper.starts_with(['A', 'a', 'B', 'b'])
    });
    // Undo and redo a margin drag without leaving the page (a focused field keeps its own undo).
    if !ui.ctx().text_edit_focused() && bs.margin_draft.is_none() {
        let (undo, redo) = ui.input_mut(|i| {
            let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
            (i.consume_key(Modifiers::COMMAND, Key::Z), redo)
        });
        if undo || redo {
            app.run_or_alert(if undo { "edit.undo" } else { "edit.redo" }, json!({}));
        }
    }
    let saved_margins = page.as_ref().map_or(NORMAL_MARGINS, page_margins);
    let saved_center = page.as_ref().map_or((false, false), |p| {
        (p.get("centerH").and_then(Json::as_bool).unwrap_or(false), p.get("centerV").and_then(Json::as_bool).unwrap_or(false))
    });
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
                app.print_now(params.clone());
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
            if ui.link(lang.tr("Page Setup…")).clicked() {
                app.open_dialog("pageSetup", json!({}));
            }
            if page.is_some() {
                ui.add_space(16.0);
                margin_fields(app, ui, bs, &sheet, saved_margins, saved_center, metric);
            }
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            let Some(page) = &page else {
                dim(ui, lang.tr("Nothing to print."));
                return;
            };
            // Paper proportions from the preview (points); hostile sizes fall back to Letter.
            let w = page.get("width").and_then(Json::as_f64).filter(|w| w.is_finite() && *w > 1.0).unwrap_or(612.0) as f32;
            let h = page.get("height").and_then(Json::as_f64).filter(|h| h.is_finite() && *h > 1.0).unwrap_or(792.0) as f32;
            let scale = (560.0 / h).min((CONTENT_W - 250.0 - 24.0 - 2.0 * HANDLE_ROOM) / w);
            preview_page(app, ui, bs, page, &sheet, (w, h), scale, (saved_margins, saved_center), metric, &t);
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
                ui.add_space(16.0);
                ui.toggle_value(&mut bs.show_margins, lang.tr("Show Margins"))
                    .on_hover_text(lang.tr("Drag the margin lines, or the table itself, to place it on the page."));
            });
        });
    });
}

/// The margin fields, centring checkboxes and units under the Print page's settings.
fn margin_fields(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, sheet: &str, saved: [f32; 6], saved_center: (bool, bool), metric: bool) {
    let lang = app.ui.language;
    ui.label(RichText::new(lang.tr("Margins")).strong());
    ui.add_space(4.0);
    let (margins, center) = bs.margin_draft.as_ref().map_or((saved, saved_center), |d| (d.margins, d.center));
    let (factor, unit, step) = if metric { (2.54, " cm", 0.01) } else { (1.0, " in", 0.005) };
    let mut commit: Option<([f32; 6], (bool, bool))> = None;
    egui::Grid::new("backstage_print_margins").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
        for (pair, labels) in [[0usize, 1], [2, 3], [4, 5]].iter().zip([["Left", "Right"], ["Top", "Bottom"], ["Header", "Footer"]]) {
            for (&i, label) in pair.iter().zip(labels) {
                ui.label(lang.tr(label));
                let Some(&current) = margins.get(i) else { continue };
                let mut v = current * factor;
                let r = ui.add(
                    egui::DragValue::new(&mut v)
                        .speed(step * factor)
                        .range(0.0..=20.0 * factor)
                        .max_decimals(2)
                        .min_decimals(2)
                        .suffix(unit)
                        .update_while_editing(false),
                );
                if r.changed() {
                    let mut m = margins;
                    if let Some(slot) = m.get_mut(i) {
                        *slot = (v / factor).clamp(0.0, 20.0);
                    }
                    if r.dragged() {
                        let d = bs.margin_draft.get_or_insert_with(|| MarginDraft {
                            sheet: sheet.to_string(),
                            target: None,
                            start: margins,
                            start_center: center,
                            block: [0.0; 4],
                            moved: egui::Vec2::ZERO,
                            margins,
                            center,
                        });
                        d.margins = m;
                    } else {
                        commit = Some((m, center));
                    }
                }
                if r.drag_stopped()
                    && let Some(d) = &bs.margin_draft
                {
                    commit = Some((d.margins, d.center));
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.label(lang.tr("Center on page"));
    ui.horizontal(|ui| {
        let (mut ch, mut cv) = center;
        if ui.checkbox(&mut ch, lang.tr("Horizontally")).changed() {
            commit = Some((margins, (ch, cv)));
        }
        if ui.checkbox(&mut cv, lang.tr("Vertically")).changed() {
            commit = Some((margins, (ch, cv)));
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.link(lang.tr("Normal margins")).on_hover_text(lang.tr("Margins back to Normal and the table back to the top left.")).clicked() {
            commit = Some((NORMAL_MARGINS, (false, false)));
        }
        ui.add_space(12.0);
        let mut m = metric;
        ui.selectable_value(&mut m, true, "cm");
        ui.selectable_value(&mut m, false, "in");
        bs.metric = Some(m);
    });
    if let Some((m, c)) = commit {
        bs.margin_draft = None;
        commit_margins(app, sheet, m, c, metric);
    }
}

/// Draws the previewed page; with Show Margins on, its margin lines and its table can be dragged.
#[allow(clippy::too_many_arguments)]
fn preview_page(
    app: &mut SheetApp,
    ui: &mut Ui,
    bs: &mut Backstage,
    page: &Json,
    sheet: &str,
    paper: (f32, f32),
    scale: f32,
    saved: ([f32; 6], (bool, bool)),
    metric: bool,
    t: &Tokens,
) {
    let lang = app.ui.language;
    let content = page.get("content");
    let block = [num(content, "x"), num(content, "y"), num(content, "width"), num(content, "height")].map(|v| v.unwrap_or(0.0).max(0.0));
    let room = if bs.show_margins { HANDLE_ROOM } else { 0.0 };
    let (outer, resp) = ui.allocate_exact_size(vec2(paper.0 * scale + 2.0 * room, paper.1 * scale + 2.0 * room), Sense::click_and_drag());
    let rect = Rect::from_min_size(outer.min + vec2(room, room), vec2(paper.0 * scale, paper.1 * scale));
    let (margins, center) = bs.margin_draft.as_ref().map_or(saved, |d| (d.margins, d.center));
    // The table follows a live drag before the engine re-paginates.
    let origin = block_origin(margins, center, paper, (block[2], block[3]));
    let offset = if bs.margin_draft.is_some() { origin - vec2(block[0], block[1]) } else { egui::Vec2::ZERO };
    let p = ui.painter();
    p.rect_filled(rect.translate(vec2(2.0, 3.0)), 2.0, t.shadow);
    p.rect_filled(rect, 2.0, t.grid_bg);
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
    if let Some(d) = app.session.active() {
        paint_page(p, rect, scale, page, &d.wb, t, offset);
    }
    if !bs.show_margins {
        return;
    }
    // Margin lines in screen space: (index, vertical, coordinate).
    let [l, r, top, b, hd, ft] = margins.map(|m| m * 72.0 * scale);
    let lines = [
        (0usize, true, rect.left() + l),
        (1, true, rect.right() - r),
        (2, false, rect.top() + top),
        (3, false, rect.bottom() - b),
        (4, false, rect.top() + hd),
        (5, false, rect.bottom() - ft),
    ];
    let table = Rect::from_min_size(rect.min + (vec2(block[0], block[1]) + offset) * scale, vec2(block[2], block[3]) * scale);
    let hit = |pos: egui::Pos2| -> Option<DragTarget> {
        let near = lines
            .iter()
            .filter(|(_, vertical, at)| {
                if *vertical {
                    (pos.x - at).abs() <= GRAB && pos.y >= outer.top() && pos.y <= outer.bottom()
                } else {
                    (pos.y - at).abs() <= GRAB && pos.x >= outer.left() && pos.x <= outer.right()
                }
            })
            .min_by(|a, b| {
                let d = |(_, v, at): &(usize, bool, f32)| if *v { (pos.x - at).abs() } else { (pos.y - at).abs() };
                d(a).total_cmp(&d(b))
            });
        match near {
            Some((i, ..)) => Some(DragTarget::Margin(*i)),
            None if table.contains(pos) => Some(DragTarget::Table),
            None => None,
        }
    };
    let dragging = bs.margin_draft.as_ref().and_then(|d| d.target);
    let hovered = dragging.or_else(|| resp.hover_pos().and_then(hit));
    if resp.drag_started()
        && let Some(target) = ui.input(|i| i.pointer.press_origin()).and_then(hit)
    {
        bs.margin_draft = Some(MarginDraft {
            sheet: sheet.to_string(),
            target: Some(target),
            start: saved.0,
            start_center: saved.1,
            block,
            moved: egui::Vec2::ZERO,
            margins: saved.0,
            center: saved.1,
        });
    }
    if resp.dragged()
        && let Some(d) = bs.margin_draft.as_mut()
        && let Some(target) = d.target
    {
        d.moved += resp.drag_delta() / scale;
        (d.margins, d.center) = dragged_margins(target, d.start, d.start_center, paper, d.block, d.moved);
    }
    if resp.drag_stopped()
        && let Some(d) = bs.margin_draft.take_if(|d| d.target.is_some())
    {
        if d.margins != d.start || d.center != d.start_center {
            commit_margins(app, sheet, d.margins, d.center, metric);
        }
        return;
    }
    let cursor = match hovered {
        Some(DragTarget::Margin(i)) if i < 2 => egui::CursorIcon::ResizeHorizontal,
        Some(DragTarget::Margin(_)) => egui::CursorIcon::ResizeVertical,
        Some(DragTarget::Table) if dragging.is_some() => egui::CursorIcon::Grabbing,
        Some(DragTarget::Table) => egui::CursorIcon::Grab,
        None => egui::CursorIcon::Default,
    };
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(cursor);
    }
    let p = ui.painter();
    let table_on = hovered == Some(DragTarget::Table);
    let table_color = if table_on { t.accent } else { t.accent.gamma_multiply(0.45) };
    for (a, b) in [
        (table.left_top(), table.right_top()),
        (table.right_top(), table.right_bottom()),
        (table.right_bottom(), table.left_bottom()),
        (table.left_bottom(), table.left_top()),
    ] {
        p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(if table_on { 1.5 } else { 1.0 }, table_color), 4.0, 3.0));
    }
    for (i, vertical, at) in lines {
        let on = hovered == Some(DragTarget::Margin(i));
        let color = if on { t.accent } else { t.text_dim };
        let stroke = Stroke::new(if on { 1.5 } else { 1.0 }, color);
        let (a, b, handle) = if vertical {
            (pos2(at, rect.top()), pos2(at, rect.bottom()), Rect::from_center_size(pos2(at, rect.top() - room / 2.0), vec2(6.0, room - 2.0)))
        } else {
            (pos2(rect.left(), at), pos2(rect.right(), at), Rect::from_center_size(pos2(rect.right() + room / 2.0, at), vec2(room - 2.0, 6.0)))
        };
        if on {
            p.line_segment([a, b], stroke);
        } else {
            p.extend(egui::Shape::dashed_line(&[a, b], stroke, 3.0, 3.0));
        }
        // Header and footer handles are hollow, the page margins filled.
        if i >= 4 {
            p.rect_stroke(handle, 1.0, Stroke::new(1.0, color), StrokeKind::Inside);
        } else {
            p.rect_filled(handle, 1.0, color);
        }
    }
    // While dragging (or hovering), name what moves and by how much.
    if let Some(target) = hovered {
        let (factor, unit) = if metric { (2.54, "cm") } else { (1.0, "in") };
        let show = |i: usize| margins.get(i).map_or(0.0, |m| m * factor);
        let text = match target {
            DragTarget::Margin(i) => {
                let label = MARGIN_LABELS.get(i).map_or("", |l| lang.tr(l));
                format!("{label}: {:.2} {unit}", show(i))
            }
            DragTarget::Table => {
                format!("{}: {:.2} {unit} · {}: {:.2} {unit}", lang.tr("Left"), origin.x / 72.0 * factor, lang.tr("Top"), origin.y / 72.0 * factor)
            }
        };
        if let Some(pos) = resp.hover_pos().or_else(|| ui.input(|i| i.pointer.latest_pos())) {
            let galley = p.layout_no_wrap(text, theme::ui_font(11.5), t.text);
            let at = (pos + vec2(14.0, 14.0)).min(outer.right_bottom() - galley.size() - vec2(8.0, 4.0));
            let bg = Rect::from_min_size(at, galley.size() + vec2(8.0, 4.0));
            p.rect_filled(bg, 3.0, t.menu_bg);
            p.rect_stroke(bg, 3.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            p.galley(at + vec2(4.0, 2.0), galley, t.text);
        }
    }
}

/// Most rows and columns one preview page draws (a page holds far fewer at any readable scale).
const PREVIEW_ROWS: u32 = 200;
const PREVIEW_COLS: u32 = 60;

/// Draws one printed page in miniature: the cells' displayed text at the page's print scale,
/// where the print places them (`body`, moved by `offset` points during a drag). `per_pt` is
/// preview pixels per point.
fn paint_page(p: &egui::Painter, rect: Rect, per_pt: f32, page: &Json, wb: &gridcraft_engine::model::Workbook, t: &Tokens, offset: egui::Vec2) {
    let Some(range) = page.get("range").and_then(Json::as_str).and_then(gridcraft_engine::core::RangeRef::parse) else { return };
    let Some(sh) = page.get("sheet").and_then(Json::as_str).and_then(|n| wb.sheet_index(n)).and_then(|i| wb.sheets.get(i)) else { return };
    // Preview pixels per sheet pixel (sheet geometry is 96 dpi: 0.75 pt per pixel).
    let print_scale =
        page.get("scale").and_then(Json::as_f64).filter(|s| s.is_finite() && *s > 0.0).map_or(1.0, |s| (s / 100.0) as f32).clamp(0.1, 4.0);
    let per_px = per_pt * 0.75 * print_scale;
    let content = page.get("content");
    let body = page.get("body");
    let start = vec2(num(body, "x").unwrap_or(0.0), num(body, "y").unwrap_or(0.0)) + offset;
    let block = Rect::from_min_size(
        rect.min + (vec2(num(content, "x").unwrap_or(0.0), num(content, "y").unwrap_or(0.0)) + offset) * per_pt,
        vec2(num(content, "width").unwrap_or(0.0), num(content, "height").unwrap_or(0.0)) * per_pt,
    )
    .intersect(rect);
    if !block.is_positive() {
        return;
    }
    let font = theme::ui_font((11.0 * per_pt * print_scale).clamp(3.0, 14.0));
    let last_row = range.end.row.min(range.start.row.saturating_add(PREVIEW_ROWS));
    let last_col = range.end.col.min(range.start.col.saturating_add(PREVIEW_COLS));
    let mut y = rect.top() + start.y * per_pt;
    for row in range.start.row..=last_row {
        let h = sh.row_height(row) * per_px;
        if h <= 0.0 {
            continue;
        }
        if y >= block.bottom() {
            break;
        }
        let mut x = rect.left() + start.x * per_pt;
        for col in range.start.col..=last_col {
            let w = sh.col_width(col) * per_px;
            if w <= 0.0 {
                continue;
            }
            if x >= block.right() {
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
                    while next <= last_col && x + span < block.right() && sh.value(gridcraft_engine::core::CellRef::new(row, next)).is_empty() {
                        span += sh.col_width(next) * per_px;
                        next = next.saturating_add(1);
                    }
                }
                let clip = Rect::from_min_size(pos2(x, y), vec2(span, h)).intersect(block);
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
    fn dragging_margins_and_the_table() {
        let letter = (612.0, 792.0);
        let block = [50.4, 54.0, 144.0, 60.0];
        // Left margin dragged an inch right; never past an inch short of the right margin.
        let (m, _) = dragged_margins(DragTarget::Margin(0), NORMAL_MARGINS, (false, false), letter, block, vec2(72.0, 0.0));
        assert!((m[0] - 1.7).abs() < 1e-4 && m[1] == 0.7);
        let (m, _) = dragged_margins(DragTarget::Margin(0), NORMAL_MARGINS, (false, false), letter, block, vec2(9_000.0, 0.0));
        assert!((m[0] - (8.5 - 0.7 - MIN_PRINTABLE)).abs() < 1e-4);
        // Bottom margin dragged up grows; the footer can't go below the paper.
        let (m, _) = dragged_margins(DragTarget::Margin(3), NORMAL_MARGINS, (false, false), letter, block, vec2(0.0, -36.0));
        assert!((m[3] - 1.25).abs() < 1e-4);
        let (m, _) = dragged_margins(DragTarget::Margin(5), NORMAL_MARGINS, (false, false), letter, block, vec2(0.0, 500.0));
        assert_eq!(m[5], 0.0);
        // The table moves by its near margin and the far one gives way: same printable size.
        let (m, c) = dragged_margins(DragTarget::Table, NORMAL_MARGINS, (true, true), letter, block, vec2(36.0, 1.0));
        assert!((m[0] - 1.2).abs() < 1e-4 && (m[1] - 0.2).abs() < 1e-4);
        assert_eq!((m[2], m[3]), (0.75, 0.75), "a jitter under the slop leaves the other axis alone");
        assert_eq!(c, (false, true));
        // It stays on the paper.
        let (m, _) = dragged_margins(DragTarget::Table, NORMAL_MARGINS, (false, false), letter, block, vec2(-500.0, 9_000.0));
        assert_eq!(m[0], 0.0);
        // A block shorter than an inch still keeps an inch of printable height.
        assert!((m[2] - (11.0 - MIN_PRINTABLE)).abs() < 1e-4);
        assert_eq!(m[3], 0.0);
        // Centring puts the block in the middle of the printable area.
        let at = block_origin(NORMAL_MARGINS, (true, false), letter, (144.0, 60.0));
        assert!((at.x - (50.4 + (612.0 - 100.8 - 144.0) / 2.0)).abs() < 1e-3 && (at.y - 54.0).abs() < 1e-3);
        assert!((round_margin(0.7, true) * 25.4 - 18.0).abs() < 1e-3);
    }

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
