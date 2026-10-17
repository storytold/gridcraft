//! Page Layout view (View › Workbook Views › Page Layout): the sheet drawn as printed pages on a
//! desk, with rulers, margins, headers and footers.
//!
//! Pagination and the cell ↔ page mapping come from [`gridcraft_engine::paper::PageGrid`] (the
//! print pagination, so pages here are the printed pages). [`crate::grid::Geo`] carries the grid,
//! so painting, hit-testing, selection and editing work as in Normal view. Headers and footers are
//! edited in place: clicking a page's top or bottom margin edits that section, and the edit is
//! committed through `pageLayout.headerFooter` (one undo step) when the user clicks elsewhere.

use std::sync::Arc;

use egui::{Align2, Color32, FontId, Key, Painter, Pos2, Rect, Stroke, StrokeKind, TextEdit, pos2, vec2};
use gridcraft_engine::cf::CfCache;
use gridcraft_engine::cmd::print::{self, HfContext};
use gridcraft_engine::model::{HfPage, Sheet, SheetViewMode, Workbook};
use gridcraft_engine::paper::{self, HfZone, PageGrid};
use serde_json::json;

use crate::SheetApp;
use crate::grid::{self, DISPLAY_SCALE, Geo};
use crate::icons::Icon;
use crate::theme::{self, Tokens};

/// Ruler thickness in points.
pub const RULER: f32 = 18.0;
/// Pages whose cells are painted in one frame; further visible pages only get their paper.
const MAX_PAGES_PAINTED: usize = 96;
/// Page frames considered per frame (tiny zooms show many pages).
const MAX_PAGE_FRAMES: usize = 2_000;
/// The contextual ribbon tab shown while a header or footer is edited.
pub const TAB: &str = "Header & Footer";

/// Cached pagination: (doc uid, revision, sheet, sheet identity) → grid.
pub type PaperCache = ((u64, u64, usize, usize), Arc<PageGrid>);

/// A header or footer being edited in place.
#[derive(Clone, Debug)]
pub struct HfEdit {
    pub sheet: usize,
    /// Page (row band, column band) whose header/footer is shown in edit.
    pub page: (usize, usize),
    pub footer: bool,
    /// Which header/footer pair the page uses (first page, even pages, the rest).
    pub kind: HfPage,
    /// 0 left, 1 centre, 2 right.
    pub section: usize,
    /// The three sections as the user sees them (`&[Page]` field names).
    pub texts: [String; 3],
    original: [String; 3],
    request_focus: bool,
    /// Caret (characters) of the active section, kept while the ribbon has focus.
    caret: usize,
    set_caret: Option<usize>,
    prev_tab: String,
}

/// Rulers measure in inches for English, centimetres otherwise.
fn metric(app: &SheetApp) -> bool {
    !matches!(app.ui.language, crate::i18n::Language::En)
}

fn cache_key(app: &SheetApp, wb: &Workbook, si: usize) -> Option<(u64, u64, usize, usize)> {
    let d = app.session.active()?;
    Some((d.uid, d.revision, si, wb.sheets.get(si).map_or(0, |a| Arc::as_ptr(a) as usize)))
}

/// Paginates at least as far as `cell` (before scrolling to it).
pub fn ensure_covers(app: &mut SheetApp, wb: &Workbook, si: usize, cell: gridcraft_engine::core::CellRef) {
    let Some(key) = cache_key(app, wb, si) else { return };
    let (rows, cols) = match app.grid.paper.as_ref().filter(|(k, _)| *k == key) {
        Some((_, g)) if g.covers(cell.row, cell.col) => return,
        Some((_, g)) => (g.row_bands.last().map_or(0, |b| b.1), g.col_bands.last().map_or(0, |b| b.1)),
        None => (0, 0),
    };
    if let Some(g) = PageGrid::build(wb, si, rows.max(cell.row).saturating_add(64), cols.max(cell.col).saturating_add(8)) {
        app.grid.paper = Some((key, Arc::new(g)));
    }
}

/// The page grid for the active sheet, covering what the view shows (cached).
pub fn grid_for(app: &mut SheetApp, wb: &Workbook, si: usize, sh: &Sheet, view: egui::Vec2, scroll: egui::Vec2) -> Option<Arc<PageGrid>> {
    let key = cache_key(app, wb, si)?;
    let cached = app.grid.paper.as_ref().filter(|(k, _)| *k == key).map(|(_, g)| g.clone());
    let base = match cached {
        Some(g) => g,
        None => Arc::new(PageGrid::build(wb, si, 0, 0)?),
    };
    let z = ((sh.zoom as f32 / 100.0).clamp(0.1, 4.0) * DISPLAY_SCALE * base.scale.clamp(0.1, 4.0)).max(0.001);
    let (x1, y1) = (scroll.x as f64 + (view.x / z) as f64, scroll.y as f64 + (view.y / z) as f64);
    let (need_r, need_c) = paper::need_for(sh, x1, y1);
    let g = if base.covers(need_r, need_c) {
        base
    } else {
        // Grow in steps so scrolling doesn't re-paginate every frame.
        Arc::new(PageGrid::build(wb, si, need_r.saturating_add(511) / 512 * 512, need_c.saturating_add(31) / 32 * 32)?)
    };
    app.grid.paper = Some((key, g.clone()));
    Some(g)
}

/// Layout point → screen.
fn screen(geo: &Geo, x: f64, y: f64) -> Pos2 {
    pos2(geo.cells.left() + (x - geo.scroll.x as f64) as f32 * geo.z, geo.cells.top() + (y - geo.scroll.y as f64) as f32 * geo.z)
}

fn screen_rect(geo: &Geo, (x, y, w, h): (f64, f64, f64, f64)) -> Rect {
    Rect::from_min_max(screen(geo, x, y), screen(geo, x + w, y + h))
}

/// Visible pages (row band range, column band range).
fn visible_pages(geo: &Geo, pg: &PageGrid) -> ((usize, usize), (usize, usize)) {
    let (x0, y0) = (geo.scroll.x as f64, geo.scroll.y as f64);
    let (x1, y1) = (x0 + (geo.cells.width() / geo.z) as f64, y0 + (geo.cells.height() / geo.z) as f64);
    pg.pages_in(x0, y0, x1, y1)
}

fn pages_iter(geo: &Geo, pg: &PageGrid) -> impl Iterator<Item = (usize, usize)> {
    let ((i0, i1), (j0, j1)) = visible_pages(geo, pg);
    (j0..=j1).flat_map(move |j| (i0..=i1).map(move |i| (i, j))).take(MAX_PAGE_FRAMES)
}

/// Paints the desk, the pages and their cells; returns the per-page quadrants for the headings.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_pages(
    app: &SheetApp,
    p: &Painter,
    geo: &Geo,
    wb: &Workbook,
    si: usize,
    sh: &Sheet,
    sel: &gridcraft_engine::Selection,
    outline: Rect,
    cf: &mut CfCache,
    t: &Tokens,
) -> Vec<((u32, u32), (u32, u32), Rect)> {
    let mut quads = Vec::new();
    let Some(pg) = geo.paper.clone() else { return quads };
    p.rect_filled(geo.cells, 0.0, t.desk);
    let cells = p.with_clip_rect(geo.cells);
    let mut painted = 0usize;
    for (i, j) in pages_iter(geo, &pg) {
        let page = screen_rect(geo, pg.page_rect(i, j));
        if !page.intersects(geo.cells) {
            continue;
        }
        cells.rect_filled(page.translate(vec2(1.5, 2.0)), 0.0, t.shadow);
        cells.rect_filled(page, 0.0, t.grid_bg);
        let (Some((r0, r1)), Some((c0, c1))) = (pg.row_band(i), pg.col_band(j)) else { continue };
        let clip = screen_rect(geo, pg.content_rect(i, j)).intersect(geo.cells);
        if !clip.is_positive() {
            continue;
        }
        let rows = (geo.row_at(sh, clip.top()).max(r0), geo.row_at(sh, clip.bottom() - 0.5).min(r1));
        let cols = (geo.col_at(sh, clip.left()).max(c0), geo.col_at(sh, clip.right() - 0.5).min(c1));
        if rows.0 > rows.1 || cols.0 > cols.1 {
            continue;
        }
        quads.push((rows, cols, clip));
        if painted >= MAX_PAGES_PAINTED {
            continue;
        }
        painted += 1;
        let pc = p.with_clip_rect(clip);
        grid::paint_quadrant(&pc, geo, wb, si, sh, sel.active, rows, cols, cf, t);
        if !pg.is_data_page(i, j) {
            // "Click to add data" pages: faded until something is typed there.
            pc.rect_filled(clip, 0.0, t.grid_bg.gamma_multiply(0.75));
            let size = (14.0 * geo.z / DISPLAY_SCALE).clamp(8.0, 22.0);
            pc.text(clip.center(), Align2::CENTER_CENTER, tr(app, "Click to add data"), theme::ui_font(size), Color32::from_gray(150));
        }
        // The selection outline may straddle the margin edge (as at A1 in Normal view).
        let sel_clip = clip.expand(3.0).intersect(page).intersect(geo.cells);
        grid::paint_selection(&p.with_clip_rect(sel_clip), geo, sh, sel, outline, t, app, rows, cols);
    }
    quads
}

fn tr<'a>(app: &SheetApp, s: &'a str) -> &'a str {
    app.ui.language.tr(s)
}

/// Section rectangles (left, centre, right) of a page's header or footer on screen.
fn section_rects(geo: &Geo, pg: &PageGrid, (i, j): (usize, usize), footer: bool) -> [Rect; 3] {
    let (px, py, w, h) = pg.page_rect(i, j);
    let [l, r, t, b, ..] = pg.margins;
    let (y0, y1) = if footer { (py + h - b, py + h) } else { (py, py + t) };
    let inner = (w - l - r).max(3.0);
    let third = inner / 3.0;
    let pad = 3.0 / geo.z as f64;
    let mut out = [Rect::NOTHING; 3];
    for (k, slot) in out.iter_mut().enumerate() {
        let x = px + l + third * k as f64;
        *slot = screen_rect(geo, (x, y0 + pad, third, (y1 - y0 - 2.0 * pad).max(1.0)));
    }
    out
}

fn hf_font(geo: &Geo, size_pt: Option<f32>, bold: bool) -> FontId {
    let px = size_pt.unwrap_or(10.0).clamp(1.0, 72.0) * 96.0 / 72.0 * geo.z;
    let px = px.clamp(4.0, 120.0);
    if bold { theme::ui_bold(px) } else { theme::ui_font(px) }
}

/// Draws headers and footers on the visible printed pages, and the "Add header" hints on hover.
pub(crate) fn paint_headers_footers(app: &SheetApp, ui: &egui::Ui, p: &Painter, geo: &Geo, wb: &Workbook, sh: &Sheet) {
    let Some(pg) = geo.paper.clone() else { return };
    let Some(d) = app.session.active() else { return };
    let p = p.with_clip_rect(geo.cells);
    let editing = app.grid.hf_edit.as_ref().map(|e| (e.page, e.footer));
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|q| geo.cells.contains(*q)).and_then(|q| {
        let (x, y) =
            (geo.scroll.x as f64 + ((q.x - geo.cells.left()) / geo.z) as f64, geo.scroll.y as f64 + ((q.y - geo.cells.top()) / geo.z) as f64);
        pg.hf_zone_at(x, y)
    });
    let needs_text = sh.print.has_header_footer();
    let (date, time) = if needs_text { print::hf_date_time(wb) } else { Default::default() };
    for (i, j) in pages_iter(geo, &pg) {
        let page = screen_rect(geo, pg.page_rect(i, j));
        if !page.intersects(geo.cells) {
            continue;
        }
        let kind = page_kind(sh, &pg, (i, j));
        for (spec, footer) in [(sh.print.hf(kind, false), false), (sh.print.hf(kind, true), true)] {
            if editing == Some(((i, j), footer)) {
                continue;
            }
            let rects = section_rects(geo, &pg, (i, j), footer);
            let hovered = hover.is_some_and(|(page, z)| page == (i, j) && z.footer == footer);
            if hovered {
                for r in &rects {
                    p.rect_stroke(*r, 0.0, Stroke::new(1.0, Color32::from_gray(185)), StrokeKind::Inside);
                }
            }
            let Some((n, total)) = pg.page_number(i, j).or_else(|| (spec.is_empty()).then_some((1, 1))) else { continue };
            if spec.is_empty() {
                if hovered {
                    let label = if footer { "Add footer" } else { "Add header" };
                    p.text(rects[1].center(), Align2::CENTER_CENTER, tr(app, label), hf_font(geo, Some(10.0), false), Color32::from_gray(120));
                }
                continue;
            }
            let cx = HfContext {
                page: n,
                pages: total,
                sheet: sh.name.clone(),
                file: d.display_title(),
                path: d.path.clone().unwrap_or_default(),
                date: date.clone(),
                time: time.clone(),
            };
            let secs = print::expand_header_footer(spec, &cx);
            let (_, py, _, h) = pg.page_rect(i, j);
            let [_, _, _, _, hm, fm] = pg.margins;
            for (k, sec) in secs.iter().enumerate() {
                if sec.text.trim().is_empty() {
                    continue;
                }
                let font = hf_font(geo, sec.size, sec.bold);
                let r = rects[k];
                let (anchor, x) = match k {
                    0 => (Align2::LEFT_TOP, r.left()),
                    1 => (Align2::CENTER_TOP, r.center().x),
                    _ => (Align2::RIGHT_TOP, r.right()),
                };
                let lines: Vec<&str> = sec.text.split('\n').take(20).collect();
                let line_h = font.size * 1.2;
                let block = line_h * lines.len() as f32;
                let top = if footer { screen(geo, 0.0, py + h - fm).y - block } else { screen(geo, 0.0, py + hm).y };
                for (li, line) in lines.iter().enumerate() {
                    p.text(pos2(x, top + li as f32 * line_h), anchor, *line, font.clone(), Color32::BLACK);
                }
            }
        }
    }
}

/// Clicks on headers and footers, and the in-place editor. Runs before the grid's own
/// interaction (which stands aside while a header or footer is being edited).
pub(crate) fn interact(app: &mut SheetApp, ui: &mut egui::Ui, resp: &egui::Response, geo: &Geo, sh: &Sheet, wb: &Workbook) {
    // Leaving the sheet or the view commits.
    if let Some(e) = &app.grid.hf_edit
        && (e.sheet != wb.active_sheet || sh.view != SheetViewMode::PageLayout || geo.paper.is_none())
    {
        commit(app);
    }
    let Some(pg) = geo.paper.clone() else { return };
    ruler_margins(app, ui, geo, &pg, sh, wb.active_sheet);
    if std::mem::take(&mut app.grid.hf_start) {
        begin(app, wb.active_sheet, sh, (pg.data_rows.0, pg.data_cols.0), HfZone { footer: false, section: 1 });
    }
    if resp.clicked_by(egui::PointerButton::Primary)
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
    {
        let (x, y) =
            (geo.scroll.x as f64 + ((p.x - geo.cells.left()) / geo.z) as f64, geo.scroll.y as f64 + ((p.y - geo.cells.top()) / geo.z) as f64);
        match pg.hf_zone_at(x, y) {
            Some((page, zone)) if geo.cells.contains(p) => begin(app, wb.active_sheet, sh, page, zone),
            _ => {
                // A click anywhere else ends the edit; the grid then handles the click.
                if app.grid.hf_edit.is_some() {
                    commit(app);
                }
            }
        }
    }
    editor(app, ui, geo, &pg);
}

/// The `pageLayout.headerFooter` parameter holding one kind of page's header or footer.
fn hf_key(kind: HfPage, footer: bool) -> &'static str {
    match (kind, footer) {
        (HfPage::Odd, false) => "header",
        (HfPage::Odd, true) => "footer",
        (HfPage::Even, false) => "evenHeader",
        (HfPage::Even, true) => "evenFooter",
        (HfPage::First, false) => "firstHeader",
        (HfPage::First, true) => "firstFooter",
    }
}

/// Insert › Header & Footer: switches to Page Layout view and edits the first page's centre header.
pub fn start_editing(app: &mut SheetApp) {
    if app.session.active().and_then(|d| d.wb.active()).is_some_and(|sh| sh.view != SheetViewMode::PageLayout) {
        app.run_or_alert("view.pageLayout", json!({}));
    }
    app.grid.hf_start = true;
}

/// Turns Different First Page / Different Odd & Even Pages on or off, keeping the edit on the
/// same page (which may now show another header).
fn set_option(app: &mut SheetApp, key: &str, on: bool) {
    let Some(e) = app.grid.hf_edit.as_ref() else { return };
    let (sheet, page, zone) = (e.sheet, e.page, HfZone { footer: e.footer, section: e.section });
    commit(app);
    app.run_or_alert("pageLayout.headerFooter", json!({"sheet": sheet, key: on}));
    let Some(sh) = app.session.active().and_then(|d| d.wb.sheet(sheet)).cloned() else { return };
    begin(app, sheet, &sh, page, zone);
}

/// Which header/footer pair a page shows: the first page, an even page, or the rest.
fn page_kind(sh: &Sheet, pg: &PageGrid, page: (usize, usize)) -> HfPage {
    let n = pg.page_number(page.0, page.1).map_or(1, |(n, _)| n);
    sh.print.hf_page(n == 1, n)
}

/// Starts (or moves) editing a header or footer section.
pub fn begin(app: &mut SheetApp, sheet: usize, sh: &Sheet, page: (usize, usize), zone: HfZone) {
    if let Some(e) = app.grid.hf_edit.as_mut()
        && e.sheet == sheet
        && e.page == page
        && e.footer == zone.footer
    {
        if e.section != zone.section {
            e.section = zone.section;
            e.caret = e.texts.get(zone.section).map_or(0, |t| t.chars().count());
            e.set_caret = Some(e.caret);
        }
        e.request_focus = true;
        return;
    }
    if app.grid.hf_edit.is_some() {
        commit(app);
    }
    // A cell being edited is entered first, as when clicking another cell.
    if app.editor.is_some() && !app.commit_edit(0, 0, false, false) {
        return;
    }
    // Before the view has paginated this sheet, the top-left page is taken as the first.
    let kind = match app.grid.paper.as_ref().filter(|(key, _)| key.2 == sheet) {
        Some((_, pg)) => page_kind(sh, pg, page),
        None => sh.print.hf_page(page == (0, 0), 1),
    };
    let texts = print::split_header_footer(sh.print.hf(kind, zone.footer)).map(|s| print::codes_to_fields(&s));
    let caret = texts.get(zone.section).map_or(0, |t| t.chars().count());
    let prev_tab = if app.ui.ribbon_tab == TAB { "Home".to_string() } else { app.ui.ribbon_tab.clone() };
    app.grid.hf_edit = Some(HfEdit {
        sheet,
        page,
        footer: zone.footer,
        kind,
        section: zone.section.min(2),
        original: texts.clone(),
        texts,
        request_focus: true,
        caret,
        set_caret: Some(caret),
        prev_tab,
    });
    app.ui.ribbon_tab = TAB.to_string();
    app.ui.ribbon_collapsed = false;
}

/// Ends editing and writes the header or footer when it changed (one undo step).
pub fn commit(app: &mut SheetApp) {
    let Some(e) = app.grid.hf_edit.take() else { return };
    if app.ui.ribbon_tab == TAB {
        app.ui.ribbon_tab = e.prev_tab.clone();
    }
    if e.texts == e.original {
        return;
    }
    let spec = print::join_header_footer(&e.texts.clone().map(|s| print::fields_to_codes(&s)));
    let key = hf_key(e.kind, e.footer);
    app.run_or_alert("pageLayout.headerFooter", json!({"sheet": e.sheet, key: spec}));
}

/// Ends editing without writing.
pub fn cancel(app: &mut SheetApp) {
    if let Some(e) = app.grid.hf_edit.take()
        && app.ui.ribbon_tab == TAB
    {
        app.ui.ribbon_tab = e.prev_tab;
    }
}

/// Inserts a field (`&[Page]`…) at the caret of the section being edited.
pub fn insert_field(app: &mut SheetApp, field: &str) {
    let Some(e) = app.grid.hf_edit.as_mut() else { return };
    let Some(text) = e.texts.get_mut(e.section) else { return };
    let at = text.char_indices().nth(e.caret).map_or(text.len(), |(b, _)| b);
    text.insert_str(at, field);
    e.caret += field.chars().count();
    e.set_caret = Some(e.caret);
    e.request_focus = true;
}

/// Moves editing to the header or footer of the same page (Go to Header / Go to Footer).
pub fn go_to(app: &mut SheetApp, footer: bool) {
    let Some(e) = app.grid.hf_edit.as_ref() else { return };
    if e.footer == footer {
        return;
    }
    let (sheet, page, section) = (e.sheet, e.page, e.section);
    commit(app);
    let Some(sh) = app.session.active().and_then(|d| d.wb.sheet(sheet)).cloned() else { return };
    begin(app, sheet, &sh, page, HfZone { footer, section });
}

fn editor(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, pg: &PageGrid) {
    let Some(mut e) = app.grid.hf_edit.take() else { return };
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        app.grid.hf_edit = Some(e);
        cancel(app);
        return;
    }
    let rects = section_rects(geo, pg, e.page, e.footer);
    let p = ui.painter().with_clip_rect(geo.cells);
    let t = Tokens::get(ui.ctx());
    let font = hf_font(geo, Some(10.0), false);
    let mut switch_to = None;
    for (k, r) in rects.iter().enumerate() {
        if !r.intersects(geo.cells) {
            continue;
        }
        p.rect_filled(*r, 0.0, Color32::WHITE);
        let active = k == e.section;
        p.rect_stroke(
            *r,
            0.0,
            Stroke::new(if active { 1.5 } else { 1.0 }, if active { t.sel_border } else { Color32::from_gray(160) }),
            StrokeKind::Inside,
        );
        if active {
            continue;
        }
        let Some(text) = e.texts.get(k) else { continue };
        let anchor = [Align2::LEFT_TOP, Align2::CENTER_TOP, Align2::RIGHT_TOP][k];
        let x = [r.left() + 3.0, r.center().x, r.right() - 3.0][k];
        p.text(pos2(x, r.top() + 2.0), anchor, text, font.clone(), Color32::BLACK);
        if ui.interact(*r, ui.id().with(("hf_section", k)), egui::Sense::click()).clicked() {
            switch_to = Some(k);
        }
    }
    // Scrolled away: keep the edit, show it again when the page comes back.
    let Some(r) = rects.get(e.section).copied().filter(|r| r.intersects(geo.cells)) else {
        app.grid.hf_edit = Some(e);
        return;
    };
    let id = egui::Id::new("gridcraft.header_footer_editor");
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(3.0, 2.0)).intersect(geo.cells)));
    let align = [egui::Align::LEFT, egui::Align::Center, egui::Align::RIGHT][e.section.min(2)];
    if let Some(text) = e.texts.get_mut(e.section) {
        let mut output = TextEdit::multiline(text)
            .id(id)
            .frame(egui::Frame::NONE)
            .font(font)
            .text_color(Color32::BLACK)
            .background_color(Color32::WHITE)
            .horizontal_align(align)
            .desired_width(r.width() - 6.0)
            .desired_rows(1)
            .lock_focus(true)
            .show(&mut child);
        if e.request_focus {
            output.response.request_focus();
            e.request_focus = false;
        }
        if let Some(c) = e.set_caret.take() {
            output.state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(c))));
            output.state.store(ui.ctx(), id);
        } else if let Some(cr) = output.cursor_range {
            e.caret = cr.primary.index.into();
        }
    }
    if let Some(k) = switch_to {
        e.section = k;
        e.caret = e.texts.get(k).map_or(0, |t| t.chars().count());
        e.set_caret = Some(e.caret);
        e.request_focus = true;
    }
    app.grid.hf_edit = Some(e);
}

/// Screen rectangles of the horizontal and vertical rulers. Right-to-left sheets mirror both:
/// the vertical ruler sits on the right, beside the row headings.
fn ruler_rects(outer: Rect, geo: &Geo) -> (Rect, Rect) {
    let hr = geo.mirror_rect(Rect::from_min_max(pos2(geo.cells.left(), outer.top()), pos2(geo.cells.right(), outer.top() + RULER)));
    let (x0, x1) = if geo.rtl { (outer.right() - RULER, outer.right()) } else { (outer.left(), outer.left() + RULER) };
    (hr, Rect::from_min_max(pos2(x0, geo.cells.top()), pos2(x1, geo.cells.bottom())))
}

/// Horizontal and vertical rulers (`outer` is the grid area including the rulers).
pub(crate) fn paint_rulers(app: &SheetApp, p: &Painter, outer: Rect, geo: &Geo, t: &Tokens) {
    let Some(pg) = geo.paper.clone() else { return };
    if geo.rect.top() - outer.top() < RULER - 0.5 {
        return;
    }
    let metric = metric(app);
    // Screen points per unit: an inch is 96 sheet pixels at the print scale.
    let inch = 96.0 / pg.scale.clamp(0.1, 4.0) * geo.z;
    let unit = if metric { inch / 2.54 } else { inch };
    let minor = if metric { 2.0 } else { 4.0 };
    let label_every = ((22.0 / unit).ceil() as usize).max(1);
    let font = theme::ui_font(9.5);
    let (hr, vr) = ruler_rects(outer, geo);
    // Right-to-left sheets: the horizontal ruler is mirrored with the pages (ticks run from the
    // right margin leftwards).
    let mx = |x: f32| geo.mirror_x(x);
    p.rect_filled(Rect::from_min_max(outer.min, pos2(outer.right(), outer.top() + RULER)), 0.0, t.window);
    p.rect_filled(Rect::from_min_max(pos2(vr.left(), outer.top()), pos2(vr.right(), outer.bottom())), 0.0, t.window);
    let ((i0, i1), (j0, j1)) = visible_pages(geo, &pg);
    let [ml, mr, mt, mb, mh, mf] = pg.margins;
    // One ruler span per page: margins shaded, the printable part light, ticks from the margin.
    let span = |p: &Painter, from: f32, to: f32, m0: f32, m1: f32, horizontal: bool, area: Rect| {
        let band = |a: f32, b: f32| {
            if horizontal {
                let (a, b) = (mx(a), mx(b));
                Rect::from_min_max(pos2(a.min(b), area.top() + 2.0), pos2(a.max(b), area.bottom() - 2.0))
            } else {
                Rect::from_min_max(pos2(area.left() + 2.0, a), pos2(area.right() - 2.0, b))
            }
        };
        let pc = p.with_clip_rect(area);
        pc.rect_filled(band(from, to), 0.0, t.header_sel_bg);
        pc.rect_filled(band(m0, m1), 0.0, t.header_bg);
        if unit < 2.0 {
            return;
        }
        let steps = (((m1 - m0) / unit * minor).ceil() as usize).min(4_000);
        for s in 0..=steps {
            let pos = m0 + s as f32 * unit / minor;
            if pos > m1 + 0.5 {
                break;
            }
            let pos = if horizontal { mx(pos) } else { pos };
            let major = s.is_multiple_of(minor as usize);
            let len = if major {
                6.0
            } else if s.is_multiple_of(2) && !metric {
                4.0
            } else {
                2.5
            };
            if !major && unit / minor < 3.5 {
                continue;
            }
            let n = s / minor as usize;
            if major && n > 0 && n.is_multiple_of(label_every) {
                let at = if horizontal { pos2(pos, area.center().y) } else { pos2(area.center().x, pos) };
                pc.text(at, Align2::CENTER_CENTER, n.to_string(), font.clone(), t.header_text);
                continue;
            }
            let (a, b) = if horizontal {
                (pos2(pos, area.bottom() - 2.0 - len), pos2(pos, area.bottom() - 2.0))
            } else if geo.rtl {
                (pos2(area.left() + 2.0, pos), pos2(area.left() + 2.0 + len, pos))
            } else {
                (pos2(area.right() - 2.0 - len, pos), pos2(area.right() - 2.0, pos))
            };
            pc.line_segment([a, b], Stroke::new(1.0, t.header_text.gamma_multiply(0.6)));
        }
    };
    for j in j0..=j1.min(j0 + 200) {
        let (x, _, w, _) = pg.page_rect(0, j);
        let a = screen(geo, x, 0.0).x;
        let b = screen(geo, x + w, 0.0).x;
        span(p, a, b, screen(geo, x + ml, 0.0).x, screen(geo, x + w - mr, 0.0).x, true, hr);
    }
    for i in i0..=i1.min(i0 + 200) {
        let (_, y, _, h) = pg.page_rect(i, 0);
        let a = screen(geo, 0.0, y).y;
        let b = screen(geo, 0.0, y + h).y;
        span(p, a, b, screen(geo, 0.0, y + mt).y, screen(geo, 0.0, y + h - mb).y, false, vr);
        // Header and footer margins: a small marker at each distance, grabbable like the margins.
        let pc = p.with_clip_rect(vr);
        for at in [screen(geo, 0.0, y + mh).y, screen(geo, 0.0, y + h - mf).y] {
            let (x, d) = if geo.rtl { (vr.right() - 2.0, -6.0) } else { (vr.left() + 2.0, 6.0) };
            pc.add(egui::Shape::convex_polygon(vec![pos2(x, at - 3.5), pos2(x, at + 3.5), pos2(x + d, at)], t.header_text, Stroke::NONE));
        }
    }
    p.line_segment([pos2(outer.left(), outer.top() + RULER - 0.5), pos2(outer.right(), outer.top() + RULER - 0.5)], Stroke::new(1.0, t.header_line));
    p.line_segment(
        if geo.rtl {
            [pos2(vr.left() + 0.5, outer.top()), pos2(vr.left() + 0.5, outer.bottom())]
        } else {
            [pos2(vr.right() - 0.5, outer.top()), pos2(vr.right() - 0.5, outer.bottom())]
        },
        Stroke::new(1.0, t.header_line),
    );
}

/// A page edge that can be dragged on a ruler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    /// Distance of the header from the top of the page.
    Header,
    /// Distance of the footer from the bottom of the page.
    Footer,
}

impl Edge {
    /// Index into `PrintSettings::margins` / `PageGrid::margins`.
    fn index(self) -> usize {
        match self {
            Edge::Left => 0,
            Edge::Right => 1,
            Edge::Top => 2,
            Edge::Bottom => 3,
            Edge::Header => 4,
            Edge::Footer => 5,
        }
    }
    /// The `pageLayout.pageSetup` margin key.
    fn key(self) -> &'static str {
        ["left", "right", "top", "bottom", "header", "footer"][self.index()]
    }
    fn label(self) -> &'static str {
        ["Left Margin", "Right Margin", "Top Margin", "Bottom Margin", "Header Margin", "Footer Margin"][self.index()]
    }
    fn horizontal(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

/// A margin being dragged on a ruler: the page it started on and the previewed value (inches).
#[derive(Clone, Debug)]
pub struct RulerDrag {
    pub sheet: usize,
    pub edge: Edge,
    /// Page column (horizontal ruler) or row (vertical ruler) the drag started on.
    pub page: usize,
    pub start_inches: f32,
    pub inches: f32,
    /// The sheet re-paginated with `inches` (page breaks and fit-to-page scale as a commit would
    /// give them), rebuilt when the value changes.
    preview: Option<(f32, Arc<PageGrid>)>,
}

/// Content that must stay between the margins, in points.
const MIN_CONTENT_PT: f64 = 36.0;
/// Screen distance within which a ruler margin edge is grabbed.
const EDGE_GRAB: f32 = 4.0;

/// Margin (inches, rounded to 0.01) for an edge dragged to `rel_pt` points from the page's
/// left/top edge. The margin stays on the page and leaves [`MIN_CONTENT_PT`] between the two
/// margins of its axis. `None` for NaN/infinite input or a page without a size.
pub fn margin_from_drag(edge: Edge, rel_pt: f64, page_pt: (f32, f32), margins_in: [f32; 6]) -> Option<f32> {
    let page = f64::from(if edge.horizontal() { page_pt.0 } else { page_pt.1 });
    if !rel_pt.is_finite() || !page.is_finite() || page <= 0.0 {
        return None;
    }
    let other = f64::from(margins_in.get(edge.index() ^ 1).copied().filter(|v| v.is_finite())?).max(0.0) * 72.0;
    let raw = match edge {
        Edge::Left | Edge::Top | Edge::Header => rel_pt,
        Edge::Right | Edge::Bottom | Edge::Footer => page - rel_pt,
    };
    let mut max = (page - other - MIN_CONTENT_PT).max(0.0);
    let mut min = 0.0_f64;
    let pt_of = |i: usize| f64::from(margins_in.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0)).max(0.0) * 72.0;
    match edge {
        // The header/footer sits within its page margin (conservative: never past the margin edge).
        Edge::Header => max = max.min(pt_of(2)),
        Edge::Footer => max = max.min(pt_of(3)),
        // ... so the top/bottom margin can't be dragged inside it.
        Edge::Top => min = pt_of(4),
        Edge::Bottom => min = pt_of(5),
        _ => {}
    }
    let pt = raw.clamp(min.min(max), max);
    Some(((pt / 72.0 * 100.0).round() / 100.0) as f32)
}

/// The grid while a margin is dragged: the sheet is re-paginated with the previewed margin, so
/// the content area, page breaks and the fit-to-page scale are those a commit would give.
pub(crate) fn with_dragged_margin(app: &mut SheetApp, wb: &Workbook, si: usize, pg: Arc<PageGrid>) -> Arc<PageGrid> {
    let Some(d) = app.grid.ruler_drag.as_mut().filter(|d| d.sheet == si) else { return pg };
    if let Some((v, g)) = &d.preview
        && (*v - d.inches).abs() < f32::EPSILON
    {
        return g.clone();
    }
    let mut w = wb.clone();
    let Some(sheet) = w.sheets.get_mut(si) else { return pg };
    let Some(slot) = Arc::make_mut(sheet).print.margins.get_mut(d.edge.index()) else { return pg };
    *slot = d.inches;
    let rows = pg.row_bands.last().map_or(0, |b| b.1);
    let cols = pg.col_bands.last().map_or(0, |b| b.1);
    let Some(g) = PageGrid::build(&w, si, rows, cols) else { return pg };
    let g = Arc::new(g);
    d.preview = Some((d.inches, g.clone()));
    g
}

/// Points per layout unit of the grid (paper size over the layout page size).
fn pt_per_unit(pg: &PageGrid) -> Option<f64> {
    let k = f64::from(pg.paper_pt.0) / pg.page_w;
    (k.is_finite() && k > 0.0).then_some(k)
}

/// The margin edge of a visible page under `pos` on the horizontal (`h`) or vertical ruler.
fn edge_at(geo: &Geo, pg: &PageGrid, pos: Pos2, h: bool) -> Option<(Edge, usize)> {
    let ((i0, i1), (j0, j1)) = visible_pages(geo, pg);
    let [ml, mr, mt, mb, mh, mf] = pg.margins;
    let mut best: Option<(f32, Edge, usize)> = None;
    let mut consider = |d: f32, e: Edge, page: usize| {
        if d <= EDGE_GRAB && best.is_none_or(|b| d < b.0) {
            best = Some((d, e, page));
        }
    };
    if h {
        for j in j0..=j1.min(j0.saturating_add(200)) {
            let (x, _, w, _) = pg.page_rect(0, j);
            consider((screen(geo, x + ml, 0.0).x - pos.x).abs(), Edge::Left, j);
            consider((screen(geo, x + w - mr, 0.0).x - pos.x).abs(), Edge::Right, j);
        }
    } else {
        for i in i0..=i1.min(i0.saturating_add(200)) {
            let (_, y, _, hh) = pg.page_rect(i, 0);
            consider((screen(geo, 0.0, y + mt).y - pos.y).abs(), Edge::Top, i);
            consider((screen(geo, 0.0, y + hh - mb).y - pos.y).abs(), Edge::Bottom, i);
            consider((screen(geo, 0.0, y + mh).y - pos.y).abs(), Edge::Header, i);
            consider((screen(geo, 0.0, y + hh - mf).y - pos.y).abs(), Edge::Footer, i);
        }
    }
    best.map(|(_, e, p)| (e, p))
}

/// Dragging the margin edges on the rulers sets the page margins: a resize cursor and a tooltip
/// on hover, a live preview while dragging, one `pageLayout.pageSetup` (one undo step) on
/// release, Esc cancels.
fn ruler_margins(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, pg: &PageGrid, sh: &Sheet, si: usize) {
    let Some(outer) = app.grid.rect else { return };
    if geo.rect.top() - outer.top() < RULER - 0.5 {
        app.grid.ruler_drag = None;
        return;
    }
    // Leaving the sheet or the view drops the drag.
    if app.grid.ruler_drag.as_ref().is_some_and(|d| d.sheet != si) {
        app.grid.ruler_drag = None;
    }
    let (hr, vr) = ruler_rects(outer, geo);
    // Right-to-left sheets mirror the pages and the horizontal ruler; hit-testing is logical.
    let hresp = ui.interact(hr, egui::Id::new("ruler_margin_h"), egui::Sense::drag());
    let vresp = ui.interact(vr, egui::Id::new("ruler_margin_v"), egui::Sense::drag());
    let margins_in = |sh: &Sheet| -> [f32; 6] {
        let m = |i: usize| sh.print.margins.get(i).copied().unwrap_or(0.5);
        [m(0), m(1), m(2), m(3), m(4), m(5)]
    };
    let current = margins_in(sh);
    let metric = metric(app);
    let tip = |ui: &egui::Ui, app: &SheetApp, edge: Edge, inches: f32| {
        let value = if metric { format!("{:.2} cm", inches * 2.54) } else { format!("{inches:.2} in") };
        let text = format!("{}: {value}", app.ui.language.tr(edge.label()));
        egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::new("ruler_margin_tip"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(text);
        });
    };
    let cursor = |ui: &egui::Ui, edge: Edge| {
        ui.ctx().set_cursor_icon(if edge.horizontal() { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
    };
    if let Some(d) = app.grid.ruler_drag.clone() {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            app.grid.ruler_drag = None;
            return;
        }
        cursor(ui, d.edge);
        let down = ui.input(|i| i.pointer.primary_down());
        let pos = ui.input(|i| i.pointer.latest_pos()).map(|p| geo.mirror_pos(p));
        let mut cur = d.clone();
        if let (Some(pos), Some(k)) = (pos, pt_per_unit(pg)) {
            let rel = if d.edge.horizontal() {
                let page_x = pg.page_x(d.page);
                f64::from(geo.scroll.x) + f64::from((pos.x - geo.cells.left()) / geo.z) - page_x
            } else {
                let page_y = pg.page_y(d.page);
                f64::from(geo.scroll.y) + f64::from((pos.y - geo.cells.top()) / geo.z) - page_y
            };
            if let Some(v) = margin_from_drag(d.edge, rel * k, pg.paper_pt, current) {
                cur.inches = v;
            }
        }
        if down {
            tip(ui, app, cur.edge, cur.inches);
            app.grid.ruler_drag = Some(cur);
            ui.ctx().request_repaint();
        } else {
            app.grid.ruler_drag = None;
            if (cur.inches - cur.start_inches).abs() > f32::EPSILON {
                app.run_or_alert("pageLayout.pageSetup", json!({"sheet": si, "margins": {cur.edge.key(): cur.inches}}));
            }
        }
        return;
    }
    for (resp, h) in [(&hresp, true), (&vresp, false)] {
        let Some(pos) = resp.hover_pos().or_else(|| resp.interact_pointer_pos()) else { continue };
        let origin = if resp.drag_started() { ui.input(|i| i.pointer.press_origin()).unwrap_or(pos) } else { pos };
        let origin = geo.mirror_pos(origin);
        let Some((edge, page)) = edge_at(geo, pg, origin, h) else { continue };
        let value = current.get(edge.index()).copied().unwrap_or(0.5);
        if resp.drag_started() {
            app.grid.ruler_drag = Some(RulerDrag { sheet: si, edge, page, start_inches: value, inches: value, preview: None });
            cursor(ui, edge);
        } else if resp.hovered() {
            cursor(ui, edge);
            tip(ui, app, edge, value);
        }
    }
}

/// The Header & Footer contextual ribbon tab.
pub fn ribbon_tab(app: &mut SheetApp, ui: &mut egui::Ui) {
    use crate::widgets::big_button;
    let Some(e) = app.grid.hf_edit.as_ref() else { return };
    let (footer, kind) = (e.footer, e.kind);
    let (sheet_name, file) =
        app.session.active().map(|d| (d.wb.sheet(e.sheet).map(|s| s.name.clone()).unwrap_or_default(), d.display_title())).unwrap_or_default();
    // Header & Footer presets (shown with sample values, as the result will read).
    for (label, part) in [("Header", false), ("Footer", true)] {
        let r = big_button(ui, if part { Icon::Footer } else { Icon::Header }, label, label, true);
        egui::Popup::menu(&r).show(|ui| {
            let presets: [(String, &str); 7] = [
                (tr(app, "(none)").to_string(), ""),
                ("Page 1".into(), "&CPage &P"),
                ("Page 1 of ?".into(), "&CPage &P of &N"),
                (sheet_name.clone(), "&C&A"),
                (file.clone(), "&C&F"),
                (format!("{sheet_name}, Page 1"), "&C&A, Page &P"),
                (format!("{file}, Page 1"), "&C&F, Page &P"),
            ];
            for (text, spec) in presets {
                if ui.button(text).clicked() {
                    let sheet = app.grid.hf_edit.as_ref().map(|e| e.sheet).unwrap_or(0);
                    cancel(app);
                    app.run_or_alert("pageLayout.headerFooter", json!({"sheet": sheet, hf_key(kind, part): spec}));
                    ui.close();
                }
            }
        });
    }
    sep(ui);
    for (icon, label, tip, field) in [
        (Icon::PageNumber, "Page\nNumber", "Page Number", "&[Page]"),
        (Icon::PageCount, "Number\nof Pages", "Number of Pages", "&[Pages]"),
        (Icon::Calendar, "Current\nDate", "Current Date", "&[Date]"),
        (Icon::Clock, "Current\nTime", "Current Time", "&[Time]"),
        (Icon::Folder, "File\nPath", "File Path", "&[Path]&[File]"),
        (Icon::FileName, "File\nName", "File Name", "&[File]"),
        (Icon::Sheet, "Sheet\nName", "Sheet Name", "&[Tab]"),
    ] {
        if big_button(ui, icon, label, tip, false).clicked() {
            insert_field(app, field);
        }
    }
    sep(ui);
    if big_button(ui, Icon::Header, "Go to\nHeader", "Go to Header", false).clicked() && footer {
        go_to(app, false);
    }
    if big_button(ui, Icon::Footer, "Go to\nFooter", "Go to Footer", false).clicked() && !footer {
        go_to(app, true);
    }
    sep(ui);
    let (first, odd_even) = app
        .session
        .active()
        .and_then(|d| d.wb.sheet(app.grid.hf_edit.as_ref().map_or(0, |e| e.sheet)).map(|s| (s.print.different_first, s.print.different_odd_even)))
        .unwrap_or_default();
    ui.vertical(|ui| {
        let (mut f, mut o) = (first, odd_even);
        if ui.checkbox(&mut f, tr(app, "Different First Page")).changed() {
            set_option(app, "differentFirst", f);
        }
        if ui.checkbox(&mut o, tr(app, "Different Odd & Even Pages")).changed() {
            set_option(app, "differentOddEven", o);
        }
    });
}

fn sep(ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), egui::Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 6.0), pos2(r.center().x, r.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

#[cfg(test)]
mod tests {
    use super::*;

    const A4: (f32, f32) = (595.0, 842.0);

    #[test]
    fn drag_maps_to_margins_and_clamps_inside_the_page() {
        let m = [0.7, 0.7, 0.75, 0.75, 0.3, 0.3];
        // 72 pt from the left edge is one inch; from the right edge the margin is measured inward.
        assert_eq!(margin_from_drag(Edge::Left, 72.0, A4, m), Some(1.0));
        assert_eq!(margin_from_drag(Edge::Right, f64::from(A4.0) - 36.0, A4, m), Some(0.5));
        assert_eq!(margin_from_drag(Edge::Top, 108.0, A4, m), Some(1.5));
        // Dragged off the page: never negative (top/bottom keep the header/footer distance).
        assert_eq!(margin_from_drag(Edge::Left, -50.0, A4, m), Some(0.0));
        assert_eq!(margin_from_drag(Edge::Bottom, f64::from(A4.1) + 99.0, A4, m), Some(0.3));
        // Dragged across the opposite margin: stops a minimum content width short of it.
        let max = margin_from_drag(Edge::Left, 10_000.0, A4, m).unwrap();
        assert!((f64::from(max) * 72.0 - (f64::from(A4.0) - 0.7 * 72.0 - MIN_CONTENT_PT)).abs() < 1.0, "{max}");
        // Hostile input is ignored, not clamped to something arbitrary.
        assert_eq!(margin_from_drag(Edge::Left, f64::NAN, A4, m), None);
        assert_eq!(margin_from_drag(Edge::Left, f64::INFINITY, A4, m), None);
        assert_eq!(margin_from_drag(Edge::Left, 10.0, (0.0, 0.0), m), None);
        assert_eq!(margin_from_drag(Edge::Left, 10.0, A4, [0.7, f32::NAN, 0.7, 0.7, 0.3, 0.3]), None);
        // Oversized opposite margin leaves nothing to drag into.
        assert_eq!(margin_from_drag(Edge::Left, 300.0, A4, [0.0, 40.0, 0.0, 0.0, 0.3, 0.3]), Some(0.0));
        // Header/footer distances are measured from the top / bottom edge.
        assert_eq!(margin_from_drag(Edge::Header, 36.0, A4, m), Some(0.5));
        assert_eq!(margin_from_drag(Edge::Footer, f64::from(A4.1) - 18.0, A4, m), Some(0.25));
        assert_eq!(margin_from_drag(Edge::Footer, 1e9, A4, m), Some(0.0));
        // Header stays within the top margin (0.75 in), footer within the bottom margin.
        assert_eq!(margin_from_drag(Edge::Header, 500.0, A4, m), Some(0.75));
        assert_eq!(margin_from_drag(Edge::Footer, f64::from(A4.1) - 500.0, A4, m), Some(0.75));
        // The top/bottom margin can't be dragged inside the header/footer distance (0.3 in).
        assert_eq!(margin_from_drag(Edge::Top, 2.0, A4, m), Some(0.3));
        assert_eq!(margin_from_drag(Edge::Bottom, f64::from(A4.1) - 2.0, A4, m), Some(0.3));
        // Hostile header/footer values (NaN, negative, huge) never panic or escape the page.
        assert_eq!(margin_from_drag(Edge::Header, 10.0, A4, [0.7, 0.7, 0.75, 0.75, f32::NAN, 0.3]), Some(0.14));
        assert_eq!(margin_from_drag(Edge::Top, 300.0, A4, [0.7, 0.7, 0.75, 0.75, -5.0, f32::INFINITY]), Some(4.17));
        assert_eq!(margin_from_drag(Edge::Header, f64::NAN, A4, m), None);
        assert!(margin_from_drag(Edge::Footer, 10.0, A4, [0.0, 0.0, 1e9, 1e9, 0.3, 1e9]).is_some_and(|v| (0.0..=11.7).contains(&v)));
    }
}
