//! Printing: Page Setup, pagination and PDF output (File › Print, File › Export › PDF).
//!
//! Sheets are paginated like Excel's default "down, then over": the print area (or the used
//! range plus drawing objects) is split into bands of rows and columns that fit the printable
//! area at the page scale, honouring manual breaks, repeated title rows/columns, headings, Fit
//! To and centring. Each page is drawn into a PDF with `gridcraft-pdf`: fills, gridlines,
//! borders, formatted text (fonts mapped to the standard 14 PDF fonts), merged cells, wrapping,
//! overflow into empty neighbours, conditional formats, table styles, charts, images, shapes and
//! sparklines. Headers and footers support `&L`/`&C`/`&R` sections and the `&P &N &D &T &A &F &Z`
//! codes.

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::*;
use gridcraft_pdf::{Font, Page, PdfDoc, Rgb};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "file.exportPdf",
            "Export as PDF",
            ["File", "Export"],
            None,
            "{path?, sheets?: active|all, range?: \"A1:F40\" (active sheet), fitToPage?: bool} → {pages, bytes, path} or {pages, bytes, base64} when no path. Honours each sheet's Page Setup.",
            has_doc,
            export_pdf
        ),
        cmd!(
            noundo "file.print",
            "Print…",
            ["File"],
            Some("Cmd+P"),
            "{path?, sheets?: active|all, range?, fitToPage?: bool} → {pages, bytes, base64} — the PDF the UI hands to the system print dialog",
            has_doc,
            print
        ),
        cmd!(
            query "file.printPreview",
            "Print Preview",
            ["File"],
            None,
            "{page? (1-based), sheets?: active|all, range?, fitToPage?} → {pages, pageList: [{page, sheet, range, rows, columns, scale, paper, orientation, width, height}]}",
            has_doc,
            print_preview
        ),
        cmd!(
            "pageLayout.pageSetup",
            "Page Setup…",
            ["Page Layout", "Page Setup"],
            None,
            "{sheet?, orientation?: portrait|landscape, paper?: Letter|Legal|A4|A3|A5|Tabloid|Executive|Ledger|B5, margins?: {left,right,top,bottom,header,footer} (inches; also accepted as top-level keys), scale?: 10..400, fitWidth?, fitHeight? (pages; 0 = automatic), fitToPage?: bool, centerH?, centerV?, gridlines?, headings?, header?: \"&L&A&RPage &P of &N\", footer?, printArea?: \"A1:F50\" (\"\" clears), titleRows?: \"1:2\", titleCols?: \"A:A\", rowBreaks?: [46, 91], colBreaks?: [\"K\"]} → the sheet's print settings",
            has_doc,
            page_setup
        ),
    ]
}

/// Sheet geometry is in pixels at 96 dpi; PDF units are points.
const PX_TO_PT: f32 = 0.75;
/// Most pages one export produces.
pub const MAX_PAGES: usize = 2000;
/// Cells in a page block above which only stored cells are visited (not every grid position).
const DENSE_BLOCK: u64 = 60_000;

// ---------------------------------------------------------------- paper

/// Paper size in points (portrait) for a paper name; unknown names fall back to Letter.
pub fn paper_size(name: &str) -> (f32, f32) {
    known_paper(name).unwrap_or((612.0, 792.0))
}

fn known_paper(name: &str) -> Option<(f32, f32)> {
    let n = name.trim().to_ascii_lowercase().replace([' ', '-', '_'], "");
    Some(match n.as_str() {
        "letter" | "usletter" => (612.0, 792.0),
        "legal" | "uslegal" => (612.0, 1008.0),
        "a4" => (595.28, 841.89),
        "a3" => (841.89, 1190.55),
        "a5" => (419.53, 595.28),
        "tabloid" | "11x17" => (792.0, 1224.0),
        "ledger" => (1224.0, 792.0),
        "executive" => (522.0, 756.0),
        "b5" => (498.9, 708.66),
        "b4" => (708.66, 1000.63),
        _ => return None,
    })
}

// ---------------------------------------------------------------- layout

/// The pagination of one sheet.
#[derive(Clone, Debug)]
pub struct SheetLayout {
    pub sheet: usize,
    pub settings: PrintSettings,
    pub page_w: f32,
    pub page_h: f32,
    /// Left, right, top, bottom, header, footer margins in points.
    pub margins: [f32; 6],
    pub scale: f32,
    pub area: RangeRef,
    pub row_bands: Vec<(u32, u32)>,
    pub col_bands: Vec<(u32, u32)>,
    /// Pages in print order: (row band, column band).
    pub pages: Vec<(usize, usize)>,
    pub title_rows: Option<(u32, u32)>,
    pub title_cols: Option<(u32, u32)>,
    /// Heading sizes in sheet pixels (0 when headings are off).
    pub heading_w: f32,
    pub heading_h: f32,
}

impl SheetLayout {
    fn k(&self) -> f32 {
        PX_TO_PT * self.scale
    }
    fn avail(&self) -> (f32, f32) {
        let [l, r, t, b, ..] = self.margins;
        ((self.page_w - l - r).max(36.0), (self.page_h - t - b).max(36.0))
    }
}

/// Options shared by the print commands.
#[derive(Clone, Debug, Default)]
pub struct PrintOptions {
    pub all_sheets: bool,
    pub range: Option<RangeRef>,
    pub fit_to_page: Option<bool>,
}

impl PrintOptions {
    fn from_params(p: &Json, cmd: &str) -> Result<PrintOptions> {
        let all_sheets = match str_param(p, "sheets") {
            None | Some("active") => false,
            Some("all") | Some("workbook") => true,
            Some(other) => return Err(bad(cmd, format!("`sheets` must be active or all, not `{other}`"))),
        };
        let range = match str_param(p, "range").map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => Some(RangeRef::parse(split_sheet(r).1).ok_or_else(|| bad(cmd, format!("not a range: `{r}`")))?),
            None => None,
        };
        Ok(PrintOptions { all_sheets, range, fit_to_page: bool_param(p, "fitToPage") })
    }
}

/// The range a sheet prints: its print area, else the used range grown to cover drawing
/// objects. `None` when there is nothing to print.
pub fn print_range(sh: &Sheet) -> Option<RangeRef> {
    if let Some(a) = sh.print.print_area {
        return Some(trim_full(sh, a));
    }
    let mut r = sh.used_range();
    let mut grow = |a: &Anchor| {
        let x = sh.col_left(a.cell.col) + a.dx as f64;
        let y = sh.row_top(a.cell.row) + a.dy as f64;
        let end = CellRef::new(sh.row_at(y + a.height.max(0.0) as f64), sh.col_at(x + a.width.max(0.0) as f64));
        let ar = RangeRef::new(a.cell, end);
        r = Some(r.map_or(ar, |u| u.union(&ar)));
    };
    for c in &sh.charts {
        grow(&c.anchor);
    }
    for i in &sh.images {
        grow(&i.anchor);
    }
    for s in &sh.shapes {
        grow(&s.anchor);
    }
    // Printing starts at A1 like Excel when the data starts nearby? Excel prints from the first
    // used cell; keep the used range as is.
    r
}

/// Whole rows/columns are trimmed to the used range.
fn trim_full(sh: &Sheet, r: RangeRef) -> RangeRef {
    if !(r.is_full_cols() || r.is_full_rows()) {
        return r;
    }
    match sh.used_range() {
        Some(u) => r.intersection(&RangeRef::new(CellRef::new(0, 0), u.end)).unwrap_or(RangeRef::cell(r.start)),
        None => RangeRef::cell(r.start),
    }
}

/// Splits `start..=end` into bands whose sizes fit `avail(band_start)`; `breaks` force a new
/// band before a line. Stops after `cap` bands.
fn bands(start: u32, end: u32, size: impl Fn(u32) -> f32, avail: impl Fn(u32) -> f32, breaks: &[u32], cap: usize) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut cur = start;
    let mut used = 0.0f32;
    let mut room = avail(start);
    let mut line = start;
    loop {
        let sz = size(line);
        let manual = line > cur && breaks.binary_search(&line).is_ok();
        if line > cur && (manual || used + sz > room + 0.01) {
            out.push((cur, line - 1));
            if out.len() >= cap {
                return out;
            }
            cur = line;
            used = 0.0;
            room = avail(line);
        }
        used += sz;
        if line >= end {
            break;
        }
        line += 1;
    }
    out.push((cur, end));
    out
}

fn title_span(t: Option<(u32, u32)>, max: u32) -> Option<(u32, u32)> {
    let (a, b) = t?;
    let (a, b) = (a.min(b), a.max(b));
    if a >= max {
        return None;
    }
    // Repeated titles are capped (Excel needs them to fit on a page anyway).
    Some((a, b.min(max - 1).min(a.saturating_add(499))))
}

/// Paginates one sheet.
pub fn layout_sheet(wb: &Workbook, si: usize, opts: &PrintOptions, range_override: Option<RangeRef>) -> Option<SheetLayout> {
    let sh = wb.sheet(si)?;
    let st = sh.print.clone();
    let (mut pw, mut ph) = paper_size(&st.paper);
    if st.orientation == Orientation::Landscape {
        std::mem::swap(&mut pw, &mut ph);
    }
    let inch = |i: usize| st.margins.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.5).clamp(0.0, 20.0) * 72.0;
    let mut margins = [inch(0), inch(1), inch(2), inch(3), inch(4), inch(5)];
    // Keep at least an inch of printable space.
    for (a, b, total) in [(0usize, 1usize, pw), (2, 3, ph)] {
        let sum = margins[a] + margins[b];
        if total - sum < 72.0 {
            let f = ((total - 72.0).max(0.0)) / sum.max(1.0);
            margins[a] *= f;
            margins[b] *= f;
        }
    }
    let area = match range_override {
        Some(r) => trim_full(sh, r),
        None => print_range(sh).unwrap_or(RangeRef::cell(CellRef::new(0, 0))),
    };
    let title_rows = title_span(st.title_rows, MAX_ROWS);
    let title_cols = title_span(st.title_cols, MAX_COLS);
    let digits = format!("{}", area.end.row + 1).len() as f32;
    let (heading_w, heading_h) = if st.headings { (10.0 + 7.0 * digits, sh.default_row_height.clamp(10.0, 60.0)) } else { (0.0, 0.0) };
    let fit = match opts.fit_to_page {
        Some(true) => Some((1u16, 1u16)),
        Some(false) => None,
        None => st.fit_to.filter(|(w, h)| *w > 0 || *h > 0),
    };
    let mut lay = SheetLayout {
        sheet: si,
        settings: st.clone(),
        page_w: pw,
        page_h: ph,
        margins,
        scale: (st.scale.clamp(10, 400) as f32) / 100.0,
        area,
        row_bands: vec![],
        col_bands: vec![],
        pages: vec![],
        title_rows,
        title_cols,
        heading_w,
        heading_h,
    };
    let paginate = |lay: &mut SheetLayout, manual: bool| {
        let k = lay.k();
        let (aw, ah) = lay.avail();
        let (aw, ah) = (aw / k, ah / k);
        let rb: Vec<u32> = if manual { st.row_breaks.clone() } else { vec![] };
        let cb: Vec<u32> = if manual { st.col_breaks.clone() } else { vec![] };
        let (mut rb, mut cb) = (rb, cb);
        rb.sort_unstable();
        cb.sort_unstable();
        let title_h = |start: u32| -> f32 {
            match title_rows {
                Some((a, b)) if a < start => (a..=b.min(start - 1)).map(|r| sh.row_height(r)).sum(),
                _ => 0.0,
            }
        };
        let title_w = |start: u32| -> f32 {
            match title_cols {
                Some((a, b)) if a < start => (a..=b.min(start - 1)).map(|c| sh.col_width(c)).sum(),
                _ => 0.0,
            }
        };
        lay.row_bands = bands(area.start.row, area.end.row, |r| sh.row_height(r), |start| (ah - heading_h - title_h(start)).max(1.0), &rb, MAX_PAGES);
        lay.col_bands = bands(area.start.col, area.end.col, |c| sh.col_width(c), |start| (aw - heading_w - title_w(start)).max(1.0), &cb, MAX_PAGES);
        lay.pages.clear();
        'outer: for ci in 0..lay.col_bands.len() {
            for ri in 0..lay.row_bands.len() {
                if lay.pages.len() >= MAX_PAGES {
                    break 'outer;
                }
                lay.pages.push((ri, ci));
            }
        }
    };
    match fit {
        Some((fw, fh)) => {
            let total_w: f32 = (area.start.col..=area.end.col).map(|c| sh.col_width(c)).sum::<f32>() + heading_w;
            let total_h: f32 = if area.height() > 200_000 {
                f32::INFINITY
            } else {
                (area.start.row..=area.end.row).map(|r| sh.row_height(r)).sum::<f32>() + heading_h
            };
            let (aw, ah) = lay.avail();
            let sw = if fw > 0 { aw * fw as f32 / (total_w * PX_TO_PT).max(1.0) } else { f32::INFINITY };
            let sh_ = if fh > 0 { ah * fh as f32 / (total_h * PX_TO_PT).max(1.0) } else { f32::INFINITY };
            lay.scale = sw.min(sh_).min(1.0).clamp(0.1, 1.0);
            for _ in 0..40 {
                paginate(&mut lay, false);
                let ok_w = fw == 0 || lay.col_bands.len() <= fw as usize;
                let ok_h = fh == 0 || lay.row_bands.len() <= fh as usize;
                if (ok_w && ok_h) || lay.scale <= 0.1 {
                    break;
                }
                lay.scale = (lay.scale * 0.97).max(0.1);
            }
        }
        None => paginate(&mut lay, true),
    }
    Some(lay)
}

/// Lays out every sheet a print command covers.
pub fn layouts(wb: &Workbook, opts: &PrintOptions) -> Vec<SheetLayout> {
    let mut out = Vec::new();
    let mut pages = 0usize;
    let sheets: Vec<usize> = if opts.all_sheets {
        (0..wb.sheets.len()).filter(|i| wb.sheet(*i).is_some_and(|s| s.visibility == Visibility::Visible)).collect()
    } else {
        vec![wb.active_sheet]
    };
    for si in sheets {
        if pages >= MAX_PAGES {
            break;
        }
        let ovr = if si == wb.active_sheet { opts.range } else { None };
        if opts.all_sheets && ovr.is_none() && wb.sheet(si).is_some_and(|s| print_range(s).is_none()) {
            continue;
        }
        if let Some(mut l) = layout_sheet(wb, si, opts, ovr) {
            l.pages.truncate(MAX_PAGES - pages);
            pages += l.pages.len();
            out.push(l);
        }
    }
    if out.is_empty() && opts.all_sheets {
        out.extend(layout_sheet(wb, wb.active_sheet, opts, None));
    }
    out
}

// ---------------------------------------------------------------- header / footer

/// Values for header/footer codes.
#[derive(Clone, Debug, Default)]
pub struct HfContext {
    pub page: usize,
    pub pages: usize,
    pub sheet: String,
    pub file: String,
    pub path: String,
    pub date: String,
    pub time: String,
}

/// One header/footer section: its text and whether `&B` made it bold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HfSection {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub size: Option<f32>,
}

/// Expands a header/footer string into its left, centre and right sections.
pub fn expand_header_footer(spec: &str, cx: &HfContext) -> [HfSection; 3] {
    let mut out: [HfSection; 3] = Default::default();
    let mut cur = 1usize;
    let chars: Vec<char> = spec.chars().take(10_000).collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '&' {
            if let Some(s) = out.get_mut(cur) {
                s.text.push(c);
            }
            i += 1;
            continue;
        }
        let Some(&code) = chars.get(i + 1) else { break };
        i += 2;
        let sec = &mut out[cur.min(2)];
        match code {
            'L' | 'l' => cur = 0,
            'C' | 'c' => cur = 1,
            'R' | 'r' => cur = 2,
            'P' | 'p' => {
                // &P, &P+n, &P-n
                let mut n = cx.page as i64;
                if let Some(&sign) = chars.get(i)
                    && (sign == '+' || sign == '-')
                {
                    let digits: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_digit()).take(6).collect();
                    if let Ok(d) = digits.parse::<i64>() {
                        n = if sign == '+' { n + d } else { n - d };
                        i += 1 + digits.len();
                    }
                }
                sec.text.push_str(&n.to_string());
            }
            'N' | 'n' => sec.text.push_str(&cx.pages.to_string()),
            'D' | 'd' => sec.text.push_str(&cx.date),
            'T' | 't' => sec.text.push_str(&cx.time),
            'A' | 'a' => sec.text.push_str(&cx.sheet),
            'F' | 'f' => sec.text.push_str(&cx.file),
            'Z' | 'z' => sec.text.push_str(&cx.path),
            '&' => sec.text.push('&'),
            'B' | 'b' => sec.bold = !sec.bold,
            'I' | 'i' => sec.italic = !sec.italic,
            '"' => {
                // &"Font,Style": skip the font name; honour Bold/Italic in the style.
                let rest: String = chars[i..].iter().take_while(|c| **c != '"').collect();
                let lower = rest.to_ascii_lowercase();
                if lower.contains("bold") {
                    sec.bold = true;
                }
                if lower.contains("italic") || lower.contains("oblique") {
                    sec.italic = true;
                }
                i += rest.chars().count() + 1;
            }
            d if d.is_ascii_digit() => {
                let digits: String = std::iter::once(d).chain(chars[i..].iter().copied().take_while(|c| c.is_ascii_digit())).take(3).collect();
                i += digits.len() - 1;
                if let Ok(v) = digits.parse::<f32>() {
                    sec.size = Some(v.clamp(1.0, 409.0));
                }
            }
            // Underline, strike, super/subscript, colour, picture: not rendered.
            'K' | 'k' => {
                i = (i + 6).min(chars.len());
            }
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------- drawing

fn rgb_of(c: Color, wb: &Workbook) -> Option<Rgb> {
    c.resolve(&wb.theme)
}

fn numfmt_rgb(c: gridcraft_numfmt::FormatColor) -> Rgb {
    use gridcraft_numfmt::FormatColor as F;
    match c {
        F::Black | F::Indexed(_) => [0, 0, 0],
        F::Blue => [0, 0, 255],
        F::Cyan => [0, 255, 255],
        F::Green => [0, 128, 0],
        F::Magenta => [255, 0, 255],
        F::Red => [255, 0, 0],
        F::White => [255, 255, 255],
        F::Yellow => [255, 255, 0],
    }
}

fn blend(fg: [u8; 4]) -> Rgb {
    let a = fg[3] as u16;
    let mix = |c: u8| ((c as u16 * a + 255 * (255 - a)) / 255) as u8;
    [mix(fg[0]), mix(fg[1]), mix(fg[2])]
}

/// The effective look of a cell: own style, table style, conditional formats.
struct Look {
    style: Style,
    fill: Option<Rgb>,
    cf: Option<crate::cf::CfLook>,
}

fn cell_look(wb: &Workbook, si: usize, sh: &Sheet, c: CellRef, cf: &mut Option<crate::cf::CfCache>) -> Look {
    let mut style = match sh.table_at(c) {
        Some(t) => crate::tables::table_cell_style(wb, si, t, c),
        None => wb.styles.get(sh.style_id(c)).clone(),
    };
    let cfl = cf.as_mut().and_then(|cache| cache.look(wb, si, c));
    if let Some(st) = cfl.as_ref().and_then(|l| l.style.as_ref()) {
        if st.fill.pattern != PatternType::None {
            style.fill = st.fill;
        }
        if st.font.color != Color::Auto {
            style.font.color = st.font.color;
        }
        style.font.bold |= st.font.bold;
        style.font.italic |= st.font.italic;
        style.font.strike |= st.font.strike;
        if st.font.underline != Underline::None {
            style.font.underline = st.font.underline;
        }
        for (dst, src) in [
            (&mut style.borders.left, st.borders.left),
            (&mut style.borders.right, st.borders.right),
            (&mut style.borders.top, st.borders.top),
            (&mut style.borders.bottom, st.borders.bottom),
        ] {
            if !src.is_none() {
                *dst = src;
            }
        }
    }
    let mut fill = match style.fill.pattern {
        PatternType::None => None,
        PatternType::Solid => rgb_of(style.fill.fg, wb),
        _ => {
            let fg = rgb_of(style.fill.fg, wb).unwrap_or([0, 0, 0]);
            let bg = rgb_of(style.fill.bg, wb).unwrap_or([255, 255, 255]);
            Some([0, 1, 2].map(|i| ((fg[i] as u16 + bg[i] as u16 * 3) / 4) as u8))
        }
    };
    if let Some(rgb) = cfl.as_ref().and_then(|l| l.scale_fill) {
        fill = Some(rgb);
    }
    Look { style, fill, cf: cfl }
}

/// PDF measurement for chart layout.
struct PdfMeasure;

impl gridcraft_chart::Measure for PdfMeasure {
    fn text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        gridcraft_pdf::text_width(if bold { Font::HelveticaBold } else { Font::Helvetica }, size, text)
    }
}

/// Maps chart-space coordinates to the page.
#[derive(Clone, Copy)]
struct Xf {
    ox: f32,
    oy: f32,
    k: f32,
}

impl Xf {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        (self.ox + x * self.k, self.oy + y * self.k)
    }
    fn pts(&self, pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
        pts.iter().map(|q| [self.ox + q[0] * self.k, self.oy + q[1] * self.k]).collect()
    }
}

/// Draws chart primitives onto a page.
fn draw_prims(pg: &mut Page, prims: &[gridcraft_chart::Prim], xf: Xf) {
    use gridcraft_chart::{HAlign as H, Prim, VAlign as V};
    let k = xf.k;
    for p in prims {
        match p {
            Prim::Rect { x, y, w, h, fill, stroke, .. } => {
                let (px, py) = xf.p(*x, *y);
                if let Some(f) = fill {
                    pg.fill_rect(px, py, w * k, h * k, blend(*f));
                }
                if let Some((c, lw)) = stroke {
                    pg.rect_stroke(px, py, w * k, h * k, (lw * k).max(0.1), blend(*c));
                }
            }
            Prim::Line { pts, color, width, dash } => {
                let d = if *dash { vec![3.0 * k, 2.0 * k] } else { vec![] };
                pg.polyline(&xf.pts(pts), (width * k).max(0.1), blend(*color), &d);
            }
            Prim::Polygon { pts, fill, stroke } => {
                pg.polygon(&xf.pts(pts), Some(blend(*fill)), stroke.map(|(c, w)| (blend(c), (w * k).max(0.1))));
            }
            Prim::Wedge { cx, cy, r_outer, r_inner, a0, a1, fill, stroke } => {
                let span = (a1 - a0).abs().min(std::f32::consts::TAU);
                let n = ((span / (std::f32::consts::PI / 36.0)).ceil() as usize).clamp(2, 400);
                let at = |a: f32, r: f32| [cx + r * a.sin(), cy - r * a.cos()];
                let mut pts: Vec<[f32; 2]> = (0..=n).map(|i| at(a0 + (a1 - a0) * i as f32 / n as f32, *r_outer)).collect();
                if *r_inner > 0.0 {
                    pts.extend((0..=n).rev().map(|i| at(a0 + (a1 - a0) * i as f32 / n as f32, *r_inner)));
                } else {
                    pts.push([*cx, *cy]);
                }
                pg.polygon(&xf.pts(&pts), Some(blend(*fill)), stroke.map(|(c, w)| (blend(c), (w * k).max(0.1))));
            }
            Prim::Circle { cx, cy, r, fill, stroke } => {
                let (px, py) = xf.p(*cx, *cy);
                pg.circle(px, py, r * k, fill.map(blend), stroke.map(|(c, w)| (blend(c), (w * k).max(0.1))));
            }
            Prim::Text { x, y, text, size, color, bold, align, valign, rotation } => {
                let font = if *bold { Font::HelveticaBold } else { Font::Helvetica };
                let size = size * k;
                let w = gridcraft_pdf::text_width(font, size, text);
                let dx = match align {
                    H::Left => 0.0,
                    H::Center => -w / 2.0,
                    H::Right => -w,
                };
                let dy = match valign {
                    V::Top => size * 0.78,
                    V::Middle => size * 0.35,
                    V::Bottom => -size * 0.21,
                };
                let (sn, cs) = if rotation.is_finite() { rotation.sin_cos() } else { (0.0, 1.0) };
                let (ax, ay) = xf.p(*x, *y);
                pg.text_rotated(ax + dx * cs - dy * sn, ay + dx * sn + dy * cs, size, font, blend(*color), text, *rotation);
            }
        }
    }
}

const PAD_PX: f32 = 2.0;

/// Word-wraps `text` to `width` points.
fn wrap_lines(text: &str, font: Font, size: f32, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || gridcraft_pdf::text_width(font, size, &cand) <= width {
                line = cand;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
            // Break a single over-long word by characters.
            while gridcraft_pdf::text_width(font, size, &line) > width && line.chars().count() > 1 {
                let mut cut = String::new();
                let mut rest = String::new();
                for ch in line.chars() {
                    if rest.is_empty() && gridcraft_pdf::text_width(font, size, &format!("{cut}{ch}")) <= width || cut.is_empty() {
                        cut.push(ch);
                    } else {
                        rest.push(ch);
                    }
                }
                if rest.is_empty() {
                    break;
                }
                out.push(cut);
                line = rest;
            }
            if out.len() > 2000 {
                return out;
            }
        }
        out.push(line);
    }
    out
}

/// Everything needed to draw one page block (a run of rows × a run of columns).
struct Block<'a> {
    wb: &'a Workbook,
    si: usize,
    sh: &'a Sheet,
    rows: (u32, u32),
    cols: (u32, u32),
    /// Page x of each column's left edge, plus the right edge of the last.
    xs: Vec<f32>,
    ys: Vec<f32>,
    k: f32,
    scale: f32,
}

impl Block<'_> {
    fn x(&self, col: u32) -> f32 {
        if col >= self.cols.0 && col <= self.cols.1 + 1 {
            return self.xs.get((col - self.cols.0) as usize).copied().unwrap_or(0.0);
        }
        let x0 = self.xs.first().copied().unwrap_or(0.0);
        x0 + ((self.sh.col_left(col) - self.sh.col_left(self.cols.0)) as f32) * self.k
    }
    fn y(&self, row: u32) -> f32 {
        if row >= self.rows.0 && row <= self.rows.1 + 1 {
            return self.ys.get((row - self.rows.0) as usize).copied().unwrap_or(0.0);
        }
        let y0 = self.ys.first().copied().unwrap_or(0.0);
        y0 + ((self.sh.row_top(row) - self.sh.row_top(self.rows.0)) as f32) * self.k
    }
    fn rect(&self, r: RangeRef) -> (f32, f32, f32, f32) {
        let (x0, y0) = (self.x(r.start.col), self.y(r.start.row));
        (x0, y0, self.x(r.end.col.saturating_add(1)) - x0, self.y(r.end.row.saturating_add(1)) - y0)
    }
    fn range(&self) -> RangeRef {
        RangeRef::new(CellRef::new(self.rows.0, self.cols.0), CellRef::new(self.rows.1, self.cols.1))
    }
}

fn border_dash(style: BorderStyle, k: f32) -> Vec<f32> {
    let u = k.max(0.2);
    match style {
        BorderStyle::Dashed | BorderStyle::MediumDashed => vec![4.0 * u, 2.0 * u],
        BorderStyle::Dotted => vec![1.0 * u, 1.0 * u],
        BorderStyle::Hair => vec![0.75 * u, 0.75 * u],
        BorderStyle::DashDot | BorderStyle::MediumDashDot | BorderStyle::SlantDashDot => vec![4.0 * u, 1.5 * u, 1.0 * u, 1.5 * u],
        BorderStyle::DashDotDot | BorderStyle::MediumDashDotDot => vec![4.0 * u, 1.5 * u, 1.0 * u, 1.5 * u, 1.0 * u, 1.5 * u],
        _ => vec![],
    }
}

fn draw_border(pg: &mut Page, b: &BorderLine, wb: &Workbook, x0: f32, y0: f32, x1: f32, y1: f32, k: f32) {
    if b.is_none() {
        return;
    }
    let color = rgb_of(b.color, wb).unwrap_or([0, 0, 0]);
    if b.style == BorderStyle::Double {
        let off = 1.0 * k;
        let lw = (0.75 * k).max(0.2);
        let (dx, dy) = if (y0 - y1).abs() < 0.001 { (0.0, off) } else { (off, 0.0) };
        pg.stroke_line(x0 - dx, y0 - dy, x1 - dx, y1 - dy, lw, color);
        pg.stroke_line(x0 + dx, y0 + dy, x1 + dx, y1 + dy, lw, color);
        return;
    }
    let lw = (b.style.width() * k).clamp(0.25, 6.0);
    pg.stroke_line_dashed(x0, y0, x1, y1, lw, color, &border_dash(b.style, k));
}

fn font_for(style: &Style) -> Font {
    Font::styled(Font::family_for(&style.font.name), style.font.bold, style.font.italic)
}

/// Draws the cells of a block: fills, gridlines, data bars, borders and text.
fn draw_block(pg: &mut Page, b: &Block, gridlines: bool, cf: &mut Option<crate::cf::CfCache>) {
    let (bx, by) = (b.xs.first().copied().unwrap_or(0.0), b.ys.first().copied().unwrap_or(0.0));
    let (bw, bh) = (b.xs.last().copied().unwrap_or(bx) - bx, b.ys.last().copied().unwrap_or(by) - by);
    if bw <= 0.0 || bh <= 0.0 {
        return;
    }
    let block = b.range();
    let sh = b.sh;
    let merges: Vec<RangeRef> = sh.merges.iter().filter(|m| m.intersects(&block)).copied().collect();
    let covered = |c: CellRef| merges.iter().any(|m| m.contains(c));
    // Cells worth visiting.
    let cells: Vec<CellRef> = if block.count() <= DENSE_BLOCK {
        block.iter().filter(|c| !covered(*c)).collect()
    } else {
        let mut set: Vec<CellRef> = sh.cells.iter_range(block).map(|(c, _)| c).collect();
        set.extend(sh.spill.range(block.start..=block.end).map(|(c, _)| *c).filter(|c| block.contains(*c)));
        set.sort_unstable();
        set.dedup();
        set.retain(|c| !covered(*c));
        set
    };
    pg.save();
    pg.clip_rect(bx, by, bw, bh);
    if gridlines {
        let grey = [0xB0, 0xB0, 0xB0];
        let lw = (0.35 * b.scale).clamp(0.1, 0.5);
        for x in &b.xs {
            pg.stroke_line(*x, by, *x, by + bh, lw, grey);
        }
        for y in &b.ys {
            pg.stroke_line(bx, *y, bx + bw, *y, lw, grey);
        }
    }
    // Cheap pre-filter: blank, unstyled cells outside tables and conditional formats draw nothing.
    let skip = |c: &CellRef| {
        sh.cond_formats.is_empty()
            && sh.value_ref(*c).is_none_or(|v| v.is_empty())
            && sh.style_id(*c) == StyleId::DEFAULT
            && (sh.tables.is_empty() || sh.table_at(*c).is_none())
    };
    let looks: Vec<(CellRef, Look)> = cells
        .iter()
        .filter(|c| !skip(c))
        .chain(merges.iter().map(|m| &m.start))
        .map(|c| (*c, cell_look(b.wb, b.si, sh, *c, cf)))
        .filter(|(c, l)| {
            l.fill.is_some()
                || l.cf.is_some()
                || merges.iter().any(|m| m.start == *c)
                || !sh.value(*c).is_empty()
                || l.style.borders != Borders::default()
        })
        .collect();
    let rect_of = |c: CellRef| -> (f32, f32, f32, f32, bool) {
        match merges.iter().find(|m| m.start == c) {
            Some(m) => {
                let (x, y, w, h) = b.rect(*m);
                (x, y, w, h, true)
            }
            None => {
                let (x, y, w, h) = b.rect(RangeRef::cell(c));
                (x, y, w, h, false)
            }
        }
    };
    // Fills (merged blocks hide the gridlines inside them).
    for (c, l) in &looks {
        let (x, y, w, h, merged) = rect_of(*c);
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        if let Some(f) = l.fill {
            pg.fill_rect(x, y, w, h, f);
        } else if merged && gridlines {
            pg.fill_rect(x, y, w, h, [255, 255, 255]);
        }
        if merged && gridlines {
            pg.rect_stroke(x, y, w, h, (0.35 * b.scale).clamp(0.1, 0.5), [0xB0, 0xB0, 0xB0]);
        }
        if let Some((frac, rgb, _)) = l.cf.as_ref().and_then(|cf| cf.bar) {
            let pad = 1.5 * b.k;
            let bwid = ((w - 2.0 * pad) * frac.clamp(0.0, 1.0)).max(0.0);
            pg.fill_rect(x + pad, y + pad, bwid, (h - 2.0 * pad).max(0.0), rgb);
        }
    }
    // Borders.
    for (c, l) in &looks {
        let (x, y, w, h, merged) = rect_of(*c);
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let bd = &l.style.borders;
        let (right, bottom) = match merges.iter().find(|m| m.start == *c).filter(|_| merged) {
            Some(m) => (
                b.wb.styles.get(sh.style_id(CellRef::new(m.start.row, m.end.col))).borders.right,
                b.wb.styles.get(sh.style_id(CellRef::new(m.end.row, m.start.col))).borders.bottom,
            ),
            None => (bd.right, bd.bottom),
        };
        draw_border(pg, &bd.top, b.wb, x, y, x + w, y, b.k);
        draw_border(pg, &bottom, b.wb, x, y + h, x + w, y + h, b.k);
        draw_border(pg, &bd.left, b.wb, x, y, x, y + h, b.k);
        draw_border(pg, &right, b.wb, x + w, y, x + w, y + h, b.k);
        draw_border(pg, &bd.diag_down, b.wb, x, y, x + w, y + h, b.k);
        draw_border(pg, &bd.diag_up, b.wb, x, y + h, x + w, y, b.k);
    }
    // Text.
    for (c, l) in &looks {
        let (x, y, w, h, merged) = rect_of(*c);
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        if let Some(picture) = sh.cell_pictures.get(c) {
            draw_cell_picture(pg, &picture.data, (x, y, w, h), b.k);
        } else {
            draw_text(pg, b, *c, l, (x, y, w, h), merged, &merges, (bx, bx + bw));
        }
    }
    pg.restore();
}

/// Cell pictures fit the current cell (or merged-cell) rectangle without changing aspect ratio.
fn draw_cell_picture(pg: &mut Page, data: &[u8], rect: (f32, f32, f32, f32), k: f32) {
    let (x, y, w, h) = rect;
    let pad = 2.0 * k;
    let (w, h) = ((w - 2.0 * pad).max(0.0), (h - 2.0 * pad).max(0.0));
    if w <= 0.0 || h <= 0.0 || data.len() > 16 * 1024 * 1024 {
        return;
    }
    let Some((iw, ih)) = crate::io::image_size(data) else { return };
    if iw == 0 || ih == 0 || iw > 8192 || ih > 8192 || u64::from(iw) * u64::from(ih) > 16_000_000 {
        return;
    }
    let Ok(mut reader) = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format() else { return };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let Ok(image) = reader.decode() else { return };
    let image =
        image.thumbnail(((w / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32, ((h / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32).to_rgba8();
    let fit = (w / iw as f32).min(h / ih as f32);
    let (pw, ph) = (iw as f32 * fit, ih as f32 * fit);
    pg.image_rgba(x + pad + (w - pw) / 2.0, y + pad + (h - ph) / 2.0, pw, ph, image.as_raw(), image.width(), image.height());
}

/// Is a cell free for text to overflow into?
fn empty_for_overflow(sh: &Sheet, c: CellRef, merges: &[RangeRef]) -> bool {
    sh.value_ref(c).is_none_or(|v| v.is_empty()) && !merges.iter().any(|m| m.contains(c))
}

fn draw_text(pg: &mut Page, b: &Block, c: CellRef, l: &Look, rect: (f32, f32, f32, f32), merged: bool, merges: &[RangeRef], limits: (f32, f32)) {
    let (x, y, w, h) = rect;
    let sh = b.sh;
    let wb = b.wb;
    let v = sh.value(c);
    if v.is_empty() || l.cf.as_ref().is_some_and(|cf| cf.hide_value) {
        return;
    }
    if !sh.show_zeros && v.as_f64() == Some(0.0) {
        return;
    }
    let st = &l.style;
    let mut size = st.font.size.clamp(1.0, 409.0) * b.scale;
    let font = font_for(st);
    let pad = PAD_PX * b.k;
    let indent = st.align.indent as f32 * 9.0 * b.k;
    let avail = (w - 2.0 * pad - indent).max(0.0);
    let code = st.num_fmt.as_str();
    let formatted = crate::display::format(&v, code, wb);
    let mut color = formatted.color.map(numfmt_rgb).or_else(|| rgb_of(st.font.color, wb)).unwrap_or([0, 0, 0]);
    if l.cf.as_ref().and_then(|x| x.style.as_ref()).is_some_and(|s| s.font.color != Color::Auto) {
        color = rgb_of(st.font.color, wb).unwrap_or(color);
    }
    let is_num = matches!(v, Value::Number(_));
    let wrap = st.align.wrap;
    let mut text = if sh.show_formulas {
        crate::display::cell_text(wb, sh, c)
    } else if code == "General"
        && let Value::Number(n) = v
    {
        let digit = gridcraft_pdf::text_width(font, size, "0").max(0.1);
        let max_chars = (avail / digit).floor().max(1.0) as usize;
        gridcraft_numfmt::format_general_fit_in(n, max_chars.min(11), &wb.locale.regional).unwrap_or_else(|| "#".repeat(max_chars.min(255)))
    } else {
        crate::display::cell_text(wb, sh, c)
    };
    if !wrap {
        text = text.replace(['\n', '\r'], " ");
    }
    if text.is_empty() {
        return;
    }
    let mut tw = gridcraft_pdf::text_width(font, size, &text);
    if st.align.shrink && !wrap && tw > avail && tw > 0.0 {
        size = (size * avail / tw).max(1.0);
        tw = gridcraft_pdf::text_width(font, size, &text);
    }
    if is_num && !wrap && tw > avail + 0.01 && !sh.show_formulas {
        let hash = gridcraft_pdf::text_width(font, size, "#").max(0.1);
        text = "#".repeat(((avail / hash).floor().max(1.0) as usize).min(255));
        tw = gridcraft_pdf::text_width(font, size, &text);
    }
    let h_align = match st.align.h {
        HAlign::General => {
            if is_num {
                HAlign::Right
            } else if matches!(v, Value::Bool(_) | Value::Error(_)) {
                HAlign::Center
            } else {
                HAlign::Left
            }
        }
        HAlign::CenterAcross | HAlign::Distributed => HAlign::Center,
        HAlign::Justify | HAlign::Fill => HAlign::Left,
        other => other,
    };
    let rot = st.align.rotation;
    let rotated = rot != 0 && rot != 255;
    // Rotated text wraps along its own direction: upright (±90°) text to the row height.
    let lines: Vec<String> = match (wrap, rotated) {
        (true, false) => wrap_lines(&text, font, size, avail.max(size)),
        (true, true) if rot.abs() == 90 => wrap_lines(&text, font, size, (h - 2.0 * pad).max(size)),
        (true, true) => vec![text.lines().next().unwrap_or_default().to_string()],
        (false, _) => vec![text],
    };
    let line_h = size * 1.2;
    let n = lines.len().max(1) as f32;
    let first_baseline = match st.align.v {
        VAlign::Top => y + pad + size * 0.8,
        VAlign::Center | VAlign::Justify | VAlign::Distributed => y + (h - n * line_h) / 2.0 + line_h * 0.5 + size * 0.33,
        VAlign::Bottom => y + h - pad - size * 0.22 - (n - 1.0) * line_h,
    };
    // Overflow into empty neighbours (single-line text, not merged).
    let (mut clip_l, mut clip_r) = (x, x + w);
    if !wrap && !merged && !is_num && rot.abs() != 90 && tw + 2.0 * pad + indent > w {
        let need = tw + 2.0 * pad + indent - w;
        let grow = |dir: i64, mut need: f32| -> f32 {
            let mut ext = 0.0;
            let mut col = c.col as i64 + dir;
            while need > 0.0 && col >= 0 && col < MAX_COLS as i64 {
                let nc = CellRef::new(c.row, col as u32);
                if !empty_for_overflow(sh, nc, merges) {
                    break;
                }
                let cw = sh.col_width(nc.col) * b.k;
                ext += cw;
                need -= cw;
                col += dir;
                if ext > 20_000.0 {
                    break;
                }
            }
            ext
        };
        match h_align {
            HAlign::Right => clip_l -= grow(-1, need),
            HAlign::Center => {
                clip_l -= grow(-1, need / 2.0);
                clip_r += grow(1, need / 2.0);
            }
            _ => clip_r += grow(1, need),
        }
    }
    let clip_l = clip_l.max(limits.0);
    let clip_r = clip_r.min(limits.1);
    pg.save();
    pg.clip_rect(clip_l, y, (clip_r - clip_l).max(0.0), h);
    if rotated {
        // Rotated text: anchored at the bottom-left (upward) or top-left (downward) of the cell.
        // Wrapped lines stack rightward for upward text and leftward for downward text, so
        // downward text starts its first line far enough in for the last to end at the left pad.
        let deg = (rot as f32).clamp(-90.0, 90.0);
        let angle = -deg.to_radians();
        let last = lines.len().saturating_sub(1) as f32;
        for (i, t) in lines.iter().enumerate() {
            let i = i as f32;
            let (ox, oy) = if deg > 0.0 {
                (x + pad + size * 0.8 * deg.to_radians().sin() + i * line_h, y + h - pad)
            } else {
                (x + pad + (last - i) * line_h, y + pad + size * 0.8)
            };
            pg.text_rotated(ox, oy, size, font, color, t, angle);
        }
        pg.restore();
        return;
    }
    if rot == 255 {
        // Vertical stacked text.
        let chars: Vec<char> = lines.concat().chars().take(500).collect();
        let mut yy = y + pad + size * 0.8;
        for ch in chars {
            let s = ch.to_string();
            let cw = gridcraft_pdf::text_width(font, size, &s);
            pg.text(x + (w - cw) / 2.0, yy, size, font, color, &s);
            yy += line_h;
            if yy > y + h + line_h {
                break;
            }
        }
        pg.restore();
        return;
    }
    for (i, line) in lines.iter().enumerate() {
        let lw = gridcraft_pdf::text_width(font, size, line);
        let lx = match h_align {
            HAlign::Right => x + w - pad - lw - indent,
            HAlign::Center => x + (w - lw) / 2.0,
            _ => x + pad + indent,
        };
        let ly = first_baseline + i as f32 * line_h;
        if ly - size > y + h || ly < y - line_h {
            continue;
        }
        pg.text(lx, ly, size, font, color, line);
        if st.font.underline != Underline::None {
            let uw = (size * 0.06).max(0.2);
            pg.stroke_line(lx, ly + size * 0.12, lx + lw, ly + size * 0.12, uw, color);
            if matches!(st.font.underline, Underline::Double | Underline::DoubleAccounting) {
                pg.stroke_line(lx, ly + size * 0.24, lx + lw, ly + size * 0.24, uw, color);
            }
        }
        if st.font.strike {
            pg.stroke_line(lx, ly - size * 0.3, lx + lw, ly - size * 0.3, (size * 0.06).max(0.2), color);
        }
    }
    pg.restore();
}

/// Sheet pixel rectangle of an anchor.
fn anchor_rect(sh: &Sheet, a: &Anchor) -> (f64, f64, f64, f64) {
    (sh.col_left(a.cell.col) + a.dx as f64, sh.row_top(a.cell.row) + a.dy as f64, a.width.max(0.0) as f64, a.height.max(0.0) as f64)
}

/// Decodes an embedded picture to RGBA, downscaled to at most `max_w`×`max_h` pixels.
fn decode_image(data: &[u8], max_w: u32, max_h: u32) -> Option<(Vec<u8>, u32, u32)> {
    let (w, h) = crate::io::image_size(data)?;
    if w == 0 || h == 0 || w as u64 * h as u64 > 60_000_000 {
        return None;
    }
    let img = image::load_from_memory(data).ok()?;
    let img = if img.width() > max_w.max(1) || img.height() > max_h.max(1) { img.thumbnail(max_w.max(1), max_h.max(1)) } else { img };
    let rgba = img.to_rgba8();
    let (pw, ph) = (rgba.width(), rgba.height());
    Some((rgba.into_raw(), pw, ph))
}

/// Draws charts, pictures, shapes and sparklines overlapping the body block of a page.
fn draw_objects(pg: &mut Page, wb: &Workbook, si: usize, sh: &Sheet, b: &Block) {
    let block = b.range();
    let (sx0, sy0) = (sh.col_left(block.start.col), sh.row_top(block.start.row));
    let (sx1, sy1) = (sh.col_left(block.end.col.saturating_add(1)), sh.row_top(block.end.row.saturating_add(1)));
    let (bx, by) = (b.xs.first().copied().unwrap_or(0.0), b.ys.first().copied().unwrap_or(0.0));
    let (bw, bh) = (b.xs.last().copied().unwrap_or(bx) - bx, b.ys.last().copied().unwrap_or(by) - by);
    let visible = |r: (f64, f64, f64, f64)| r.2 > 0.0 && r.3 > 0.0 && r.0 < sx1 && r.0 + r.2 > sx0 && r.1 < sy1 && r.1 + r.3 > sy0;
    let place = |r: (f64, f64, f64, f64)| (bx + ((r.0 - sx0) as f32) * b.k, by + ((r.1 - sy0) as f32) * b.k, r.2 as f32 * b.k, r.3 as f32 * b.k);
    pg.save();
    pg.clip_rect(bx, by, bw, bh);
    for sp in &sh.sparklines {
        if !block.contains(sp.cell) {
            continue;
        }
        let vals: Vec<Option<f64>> = match gridcraft_calc::evaluate(wb, si, sp.cell, &sp.source) {
            Value::Array(a) => a.iter().take(10_000).map(Value::as_f64).collect(),
            v => vec![v.as_f64()],
        };
        let color = rgb_of(sp.color, wb).unwrap_or([0x15, 0x60, 0x82]);
        let (x, y, w, h) = b.rect(RangeRef::cell(sp.cell));
        let (wpx, hpx) = (w / b.k, h / b.k);
        let prims = gridcraft_chart::render_sparkline(sp.kind, &vals, [color[0], color[1], color[2], 255], sp.markers, wpx, hpx);
        draw_prims(pg, &prims, Xf { ox: x, oy: y, k: b.k });
    }
    for im in &sh.images {
        let r = anchor_rect(sh, &im.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, w, h) = place(r);
        // About 150 dpi is plenty for print.
        let (mw, mh) = (((w / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32, ((h / 72.0) * 150.0).ceil().clamp(1.0, 4000.0) as u32);
        if let Some((px, pw, ph)) = decode_image(&im.data, mw, mh) {
            pg.image_rgba(x, y, w, h, &px, pw, ph);
        }
    }
    for shp in &sh.shapes {
        let r = anchor_rect(sh, &shp.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, w, h) = place(r);
        let fill = rgb_of(shp.fill, wb);
        let line = rgb_of(shp.line, wb).map(|c| (c, (0.75 * b.k).max(0.25)));
        match shp.kind {
            ShapeKind::Ellipse => {
                let pts: Vec<[f32; 2]> = (0..72)
                    .map(|i| {
                        let a = i as f32 / 72.0 * std::f32::consts::TAU;
                        [x + w / 2.0 + w / 2.0 * a.cos(), y + h / 2.0 + h / 2.0 * a.sin()]
                    })
                    .collect();
                pg.polygon(&pts, fill, line);
            }
            ShapeKind::Triangle => pg.polygon(&[[x + w / 2.0, y], [x + w, y + h], [x, y + h]], fill, line),
            ShapeKind::Line | ShapeKind::Arrow => {
                let c = line.map(|l| l.0).or(fill).unwrap_or([0, 0, 0]);
                pg.stroke_line(x, y, x + w, y + h, (1.0 * b.k).max(0.3), c);
                if shp.kind == ShapeKind::Arrow {
                    let ang = h.atan2(w);
                    let al = 8.0 * b.k;
                    let p = |da: f32| [x + w - al * (ang + da).cos(), y + h - al * (ang + da).sin()];
                    pg.polygon(&[[x + w, y + h], p(0.4), p(-0.4)], Some(c), None);
                }
            }
            _ => {
                pg.polygon(&[[x, y], [x + w, y], [x + w, y + h], [x, y + h]], fill, line);
            }
        }
        if !shp.text.is_empty() {
            let size = 11.0 * b.scale;
            let lines = wrap_lines(&shp.text, Font::Helvetica, size, (w - 4.0 * b.k).max(size));
            let mut yy = y + 2.0 * b.k + size * 0.8;
            for l in lines.iter().take(500) {
                let lw = gridcraft_pdf::text_width(Font::Helvetica, size, l);
                let lx = if shp.kind == ShapeKind::TextBox { x + 2.0 * b.k } else { x + (w - lw) / 2.0 };
                pg.text(lx, yy, size, Font::Helvetica, [0, 0, 0], l);
                yy += size * 1.2;
            }
        }
    }
    for ch in &sh.charts {
        let r = anchor_rect(sh, &ch.anchor);
        if !visible(r) {
            continue;
        }
        let (x, y, _, _) = place(r);
        let data = gridcraft_chart::resolve(wb, si, ch);
        let prims = gridcraft_chart::render(ch, &data, r.2 as f32, r.3 as f32, &PdfMeasure);
        draw_prims(pg, &prims, Xf { ox: x, oy: y, k: b.k });
    }
    pg.restore();
}

fn draw_headings(pg: &mut Page, b: &Block, col_heads: bool, row_heads: bool, at: (f32, f32), size: (f32, f32)) {
    let fill = [0xF2, 0xF2, 0xF2];
    let line = [0xA0, 0xA0, 0xA0];
    let fsize = 9.0 * b.scale.max(0.1);
    let lw = (0.35 * b.scale).clamp(0.1, 0.5);
    if col_heads {
        let y = at.1;
        for col in b.cols.0..=b.cols.1 {
            let (x0, x1) = (b.x(col), b.x(col + 1));
            if x1 - x0 <= 0.0 {
                continue;
            }
            pg.fill_rect(x0, y, x1 - x0, size.1, fill);
            pg.rect_stroke(x0, y, x1 - x0, size.1, lw, line);
            let t = gridcraft_core::col_to_letters(col);
            let tw = gridcraft_pdf::text_width(Font::Helvetica, fsize, &t);
            pg.text(x0 + (x1 - x0 - tw) / 2.0, y + size.1 / 2.0 + fsize * 0.35, fsize, Font::Helvetica, [0x40, 0x40, 0x40], &t);
        }
    }
    if row_heads {
        let x = at.0;
        for row in b.rows.0..=b.rows.1 {
            let (y0, y1) = (b.y(row), b.y(row + 1));
            if y1 - y0 <= 0.0 {
                continue;
            }
            pg.fill_rect(x, y0, size.0, y1 - y0, fill);
            pg.rect_stroke(x, y0, size.0, y1 - y0, lw, line);
            let t = (row + 1).to_string();
            let tw = gridcraft_pdf::text_width(Font::Helvetica, fsize, &t);
            pg.text(x + (size.0 - tw) / 2.0, y0 + (y1 - y0) / 2.0 + fsize * 0.35, fsize, Font::Helvetica, [0x40, 0x40, 0x40], &t);
        }
    }
}

fn draw_header_footer(pg: &mut Page, lay: &SheetLayout, cx: &HfContext) {
    let [l, r, _, _, hm, fm] = lay.margins;
    let pw = lay.page_w;
    let ph = lay.page_h;
    for (spec, header) in [(&lay.settings.header, true), (&lay.settings.footer, false)] {
        if spec.is_empty() {
            continue;
        }
        let secs = expand_header_footer(spec, cx);
        for (i, sec) in secs.iter().enumerate() {
            if sec.text.trim().is_empty() {
                continue;
            }
            let size = sec.size.unwrap_or(10.0).clamp(1.0, 72.0);
            let font = Font::styled(gridcraft_pdf::Family::Sans, sec.bold, sec.italic);
            let lines: Vec<&str> = sec.text.split('\n').take(20).collect();
            let n = lines.len() as f32;
            for (j, line) in lines.iter().enumerate() {
                let lw = gridcraft_pdf::text_width(font, size, line);
                let x = match i {
                    0 => l,
                    1 => (pw - lw) / 2.0,
                    _ => pw - r - lw,
                };
                let y = if header { hm + size * 0.8 + j as f32 * size * 1.2 } else { ph - fm - size * 0.22 - (n - 1.0 - j as f32) * size * 1.2 };
                pg.text(x, y, size, font, [0, 0, 0], line);
            }
        }
    }
}

/// Builds a block for rows × cols positioned at (x, y) on the page.
fn make_block<'a>(wb: &'a Workbook, si: usize, sh: &'a Sheet, rows: (u32, u32), cols: (u32, u32), x: f32, y: f32, lay: &SheetLayout) -> Block<'a> {
    let k = lay.k();
    let mut xs = Vec::with_capacity((cols.1 - cols.0 + 2) as usize);
    let mut acc = x;
    xs.push(acc);
    for c in cols.0..=cols.1 {
        acc += sh.col_width(c) * k;
        xs.push(acc);
    }
    let mut ys = Vec::with_capacity((rows.1 - rows.0 + 2) as usize);
    let mut acc = y;
    ys.push(acc);
    for r in rows.0..=rows.1 {
        acc += sh.row_height(r) * k;
        ys.push(acc);
    }
    Block { wb, si, sh, rows, cols, xs, ys, k, scale: lay.scale }
}

/// Segments of a page along one axis: repeated titles (before the band) then the band.
fn segments(titles: Option<(u32, u32)>, band: (u32, u32)) -> Vec<(u32, u32)> {
    let mut v = Vec::new();
    if let Some((a, b)) = titles
        && a < band.0
    {
        v.push((a, b.min(band.0 - 1)));
    }
    v.push(band);
    v
}

fn seg_size(sh: &Sheet, seg: (u32, u32), rows: bool) -> f32 {
    if rows { (seg.0..=seg.1).map(|r| sh.row_height(r)).sum() } else { (seg.0..=seg.1).map(|c| sh.col_width(c)).sum() }
}

/// Renders laid-out sheets to a PDF.
pub fn render_pdf(wb: &Workbook, layouts: &[SheetLayout], file: &str, path: &str) -> (Vec<u8>, usize) {
    let mut doc = PdfDoc::new();
    doc.info.title = if wb.props.title.is_empty() { file.to_string() } else { wb.props.title.clone() };
    doc.info.author = wb.props.author.clone();
    let total: usize = layouts.iter().map(|l| l.pages.len()).sum();
    let now = gridcraft_calc::now_serial();
    let date = crate::display::format(&Value::Number(now.floor()), wb.locale.regional.short_date, wb).text;
    let time = crate::display::format(&Value::Number(now), wb.locale.regional.short_time, wb).text;
    let mut page_no = 0usize;
    for lay in layouts {
        let Some(sh) = wb.sheet(lay.sheet) else { continue };
        let mut cf = if sh.cond_formats.is_empty() { None } else { Some(crate::cf::CfCache::new()) };
        let k = lay.k();
        let (aw, ah) = lay.avail();
        for (ri, ci) in &lay.pages {
            page_no += 1;
            let (Some(rband), Some(cband)) = (lay.row_bands.get(*ri).copied(), lay.col_bands.get(*ci).copied()) else { continue };
            let rsegs = segments(lay.title_rows, rband);
            let csegs = segments(lay.title_cols, cband);
            let content_w = (lay.heading_w + csegs.iter().map(|s| seg_size(sh, *s, false)).sum::<f32>()) * k;
            let content_h = (lay.heading_h + rsegs.iter().map(|s| seg_size(sh, *s, true)).sum::<f32>()) * k;
            let ox = lay.margins[0] + if lay.settings.center_h { ((aw - content_w) / 2.0).max(0.0) } else { 0.0 };
            let oy = lay.margins[2] + if lay.settings.center_v { ((ah - content_h) / 2.0).max(0.0) } else { 0.0 };
            let pg = doc.add_page(lay.page_w, lay.page_h);
            let mut y = oy + lay.heading_h * k;
            let mut body: Option<Block> = None;
            for (rsi, rs) in rsegs.iter().enumerate() {
                let mut x = ox + lay.heading_w * k;
                for (csi, cs) in csegs.iter().enumerate() {
                    let b = make_block(wb, lay.sheet, sh, *rs, *cs, x, y, lay);
                    draw_block(pg, &b, lay.settings.gridlines, &mut cf);
                    if lay.settings.headings {
                        draw_headings(pg, &b, rsi == 0, csi == 0, (ox, oy), (lay.heading_w * k, lay.heading_h * k));
                    }
                    x += seg_size(sh, *cs, false) * k;
                    if rsi + 1 == rsegs.len() && csi + 1 == csegs.len() {
                        body = Some(b);
                    }
                }
                y += seg_size(sh, *rs, true) * k;
            }
            if let Some(b) = &body {
                draw_objects(pg, wb, lay.sheet, sh, b);
            }
            if lay.settings.headings {
                let fill = [0xF2, 0xF2, 0xF2];
                pg.fill_rect(ox, oy, lay.heading_w * k, lay.heading_h * k, fill);
                pg.rect_stroke(ox, oy, lay.heading_w * k, lay.heading_h * k, 0.3, [0xA0, 0xA0, 0xA0]);
            }
            let cx = HfContext {
                page: page_no,
                pages: total,
                sheet: sh.name.clone(),
                file: file.to_string(),
                path: path.to_string(),
                date: date.clone(),
                time: time.clone(),
            };
            draw_header_footer(pg, lay, &cx);
        }
    }
    let n = doc.page_count();
    (doc.finish(), n)
}

// ---------------------------------------------------------------- commands

fn build_pdf(s: &Session, p: &Json, cmd: &str) -> Result<(Vec<u8>, usize)> {
    let opts = PrintOptions::from_params(p, cmd)?;
    let d = s.doc()?;
    let lays = layouts(&d.wb, &opts);
    let path = d.path.clone().unwrap_or_default();
    Ok(render_pdf(&d.wb, &lays, &d.display_title(), &path))
}

fn export_pdf(s: &mut Session, p: &Json) -> Result<Json> {
    let (bytes, pages) = build_pdf(s, p, "file.exportPdf")?;
    match str_param(p, "path").filter(|x| !x.is_empty()) {
        Some(path) => {
            crate::io::write_file(path, &bytes)?;
            Ok(json!({"pages": pages, "bytes": bytes.len(), "path": path}))
        }
        None => Ok(json!({"pages": pages, "bytes": bytes.len(), "base64": crate::io::base64_encode(&bytes)})),
    }
}

fn print(s: &mut Session, p: &Json) -> Result<Json> {
    let (bytes, pages) = build_pdf(s, p, "file.print")?;
    let mut out = json!({"pages": pages, "bytes": bytes.len(), "base64": crate::io::base64_encode(&bytes)});
    if let Some(path) = str_param(p, "path").filter(|x| !x.is_empty()) {
        crate::io::write_file(path, &bytes)?;
        out["path"] = json!(path);
    }
    Ok(out)
}

fn print_preview(s: &mut Session, p: &Json) -> Result<Json> {
    let opts = PrintOptions::from_params(p, "file.printPreview")?;
    let d = s.doc()?;
    let lays = layouts(&d.wb, &opts);
    let mut list = Vec::new();
    let mut n = 0usize;
    for lay in &lays {
        let name = d.wb.sheet(lay.sheet).map(|s| s.name.clone()).unwrap_or_default();
        for (ri, ci) in &lay.pages {
            n += 1;
            let (Some(rb), Some(cb)) = (lay.row_bands.get(*ri), lay.col_bands.get(*ci)) else { continue };
            let r = RangeRef::new(CellRef::new(rb.0, cb.0), CellRef::new(rb.1, cb.1));
            list.push(json!({
                "page": n,
                "sheet": name,
                "range": r.a1(),
                "rows": format!("{}:{}", rb.0 + 1, rb.1 + 1),
                "columns": format!("{}:{}", gridcraft_core::col_to_letters(cb.0), gridcraft_core::col_to_letters(cb.1)),
                "scale": (lay.scale * 1000.0).round() / 10.0,
                "paper": lay.settings.paper,
                "orientation": if lay.settings.orientation == Orientation::Landscape { "landscape" } else { "portrait" },
                "width": lay.page_w,
                "height": lay.page_h,
            }));
        }
    }
    if let Some(page) = u32_param(p, "page") {
        let one =
            list.get((page as usize).saturating_sub(1)).cloned().ok_or_else(|| bad("file.printPreview", format!("there are only {n} pages")))?;
        return Ok(json!({"pages": n, "page": one}));
    }
    Ok(json!({"pages": n, "pageList": list}))
}

fn page_setup(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "pageLayout.pageSetup";
    let si = target_sheet(s, p)?;
    let orientation = match str_param(p, "orientation") {
        None => None,
        Some("portrait") => Some(Orientation::Portrait),
        Some("landscape") => Some(Orientation::Landscape),
        Some(o) => return Err(bad(CMD, format!("orientation must be portrait or landscape, not `{o}`"))),
    };
    let paper = match str_param(p, "paper") {
        None => None,
        Some(name) => {
            known_paper(name).ok_or_else(|| bad(CMD, format!("unknown paper size `{name}`")))?;
            Some(name.trim().to_string())
        }
    };
    let margin_src = p.get("margins").filter(|m| m.is_object()).unwrap_or(p);
    let mut margins: [Option<f32>; 6] = [None; 6];
    for (i, key) in ["left", "right", "top", "bottom", "header", "footer"].iter().enumerate() {
        if let Some(v) = f64_param(margin_src, key).or_else(|| f64_param(p, key)) {
            if !v.is_finite() || v < 0.0 {
                return Err(bad(CMD, format!("margin `{key}` must be a non-negative number of inches")));
            }
            margins[i] = Some(v.min(20.0) as f32);
        }
    }
    let parse_rows = |key: &str| -> Result<Option<Option<(u32, u32)>>> {
        match p.get(key) {
            None => Ok(None),
            Some(Json::Null) => Ok(Some(None)),
            Some(Json::String(t)) if t.trim().is_empty() => Ok(Some(None)),
            Some(Json::String(t)) => {
                let r = RangeRef::parse(split_sheet(t).1).ok_or_else(|| bad(CMD, format!("`{key}` is not a row range: `{t}`")))?;
                Ok(Some(Some((r.start.row, r.end.row))))
            }
            Some(_) => Err(bad(CMD, format!("`{key}` must be text like \"1:2\""))),
        }
    };
    let parse_cols = |key: &str| -> Result<Option<Option<(u32, u32)>>> {
        match p.get(key) {
            None => Ok(None),
            Some(Json::Null) => Ok(Some(None)),
            Some(Json::String(t)) if t.trim().is_empty() => Ok(Some(None)),
            Some(Json::String(t)) => {
                let r = RangeRef::parse(split_sheet(t).1).ok_or_else(|| bad(CMD, format!("`{key}` is not a column range: `{t}`")))?;
                Ok(Some(Some((r.start.col, r.end.col))))
            }
            Some(_) => Err(bad(CMD, format!("`{key}` must be text like \"A:B\""))),
        }
    };
    let title_rows = parse_rows("titleRows")?;
    let title_cols = parse_cols("titleCols")?;
    let print_area = match p.get("printArea") {
        None => None,
        Some(Json::Null) => Some(None),
        Some(Json::String(t)) if t.trim().is_empty() => Some(None),
        Some(Json::String(t)) => Some(Some(RangeRef::parse(split_sheet(t).1).ok_or_else(|| bad(CMD, format!("`printArea` is not a range: `{t}`")))?)),
        Some(_) => return Err(bad(CMD, "`printArea` must be a range like \"A1:F50\"")),
    };
    let row_breaks: Option<Vec<u32>> = p.get("rowBreaks").and_then(Json::as_array).map(|a| {
        let mut v: Vec<u32> = a
            .iter()
            .filter_map(|x| x.as_u64().or_else(|| x.as_str().and_then(|t| t.trim().parse().ok())))
            .filter(|r| *r >= 2 && *r <= MAX_ROWS as u64)
            .map(|r| r as u32 - 1)
            .take(1026)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    });
    let col_breaks: Option<Vec<u32>> = p.get("colBreaks").and_then(Json::as_array).map(|a| {
        let mut v: Vec<u32> = a
            .iter()
            .filter_map(|x| match x {
                Json::String(t) => gridcraft_core::letters_to_col(t.trim().trim_start_matches('$')),
                Json::Number(n) => n.as_u64().filter(|c| *c >= 2 && *c <= MAX_COLS as u64).map(|c| c as u32 - 1),
                _ => None,
            })
            .filter(|c| *c >= 1)
            .take(1026)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    });
    let scale = f64_param(p, "scale").filter(|v| v.is_finite()).map(|v| v.clamp(10.0, 400.0).round() as u16);
    let fit_w = p.get("fitWidth").map(|v| v.as_u64().unwrap_or(0).min(32767) as u16);
    let fit_h = p.get("fitHeight").map(|v| v.as_u64().unwrap_or(0).min(32767) as u16);
    let fit_page = bool_param(p, "fitToPage");
    let header = str_param(p, "header").map(|t| t.chars().take(255).collect::<String>());
    let footer = str_param(p, "footer").map(|t| t.chars().take(255).collect::<String>());
    let flags = [("centerH", 0usize), ("centerV", 1), ("gridlines", 2), ("headings", 3)].map(|(k, i)| (i, bool_param(p, k)));
    edit(s, |cx| {
        let pr = &mut cx.sheet_mut(si)?.print;
        if let Some(o) = orientation {
            pr.orientation = o;
        }
        if let Some(pp) = &paper {
            pr.paper = pp.clone();
        }
        for (i, m) in margins.iter().enumerate() {
            if let (Some(v), Some(slot)) = (m, pr.margins.get_mut(i)) {
                *slot = *v;
            }
        }
        if let Some(sc) = scale {
            pr.scale = sc;
            pr.fit_to = None;
        }
        if fit_w.is_some() || fit_h.is_some() {
            let (cw, ch) = pr.fit_to.unwrap_or((1, 1));
            pr.fit_to = Some((fit_w.unwrap_or(cw), fit_h.unwrap_or(ch)));
        }
        match fit_page {
            Some(true) => pr.fit_to = Some((1, 1)),
            Some(false) => pr.fit_to = None,
            None => {}
        }
        for (i, v) in flags {
            if let Some(v) = v {
                match i {
                    0 => pr.center_h = v,
                    1 => pr.center_v = v,
                    2 => pr.gridlines = v,
                    _ => pr.headings = v,
                }
            }
        }
        if let Some(h) = &header {
            pr.header = h.clone();
        }
        if let Some(f) = &footer {
            pr.footer = f.clone();
        }
        if let Some(a) = print_area {
            pr.print_area = a;
        }
        if let Some(t) = title_rows {
            pr.title_rows = t;
        }
        if let Some(t) = title_cols {
            pr.title_cols = t;
        }
        if let Some(b) = &row_breaks {
            pr.row_breaks = b.clone();
        }
        if let Some(b) = &col_breaks {
            pr.col_breaks = b.clone();
        }
        Ok(())
    })?;
    let pr = s.doc()?.wb.sheet(si).map(|sh| sh.print.clone()).unwrap_or_default();
    Ok(serde_json::to_value(&pr).unwrap_or(Json::Null))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn s() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }
    fn pdf_of(s: &mut Session, p: Json) -> (Vec<u8>, u64) {
        let r = s.execute("file.exportPdf", p).unwrap();
        let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
        (bytes, r["pages"].as_u64().unwrap())
    }
    fn contains(hay: &[u8], needle: &str) -> bool {
        hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[test]
    fn page_count_300_rows_letter_portrait() {
        let mut s = s();
        let rows: Vec<Vec<Json>> = (1..=300).map(|i| vec![json!(i)]).collect();
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        // 684pt of printable height / 15pt rows = 45 rows per page → 7 pages.
        let (pdf, pages) = pdf_of(&mut s, json!({}));
        assert_eq!(pages, 7);
        assert!(pdf.starts_with(b"%PDF-1.7"));
        assert!(contains(&pdf, "/Count 7"));
        let pv = s.execute("file.printPreview", json!({})).unwrap();
        assert_eq!(pv["pages"], 7);
        assert_eq!(pv["pageList"][0]["rows"], "1:45");
        assert_eq!(pv["pageList"][1]["rows"], "46:90");
        assert_eq!(s.execute("file.printPreview", json!({"page": 7})).unwrap()["page"]["rows"], "271:300");
        assert!(s.execute("file.printPreview", json!({"page": 8})).is_err());
        // Landscape: 504pt / 15 = 33 rows per page → 10 pages.
        s.execute("pageLayout.pageSetup", json!({"orientation": "landscape"})).unwrap();
        assert_eq!(pdf_of(&mut s, json!({})).1, 10);
        // Fit to one page.
        assert_eq!(pdf_of(&mut s, json!({"fitToPage": true})).1, 1);
        s.execute("pageLayout.pageSetup", json!({"fitWidth": 1, "fitHeight": 2, "orientation": "portrait"})).unwrap();
        assert_eq!(pdf_of(&mut s, json!({})).1, 2);
        // Manual break and repeated title rows.
        s.execute("pageLayout.pageSetup", json!({"scale": 100, "rowBreaks": [11], "titleRows": "1:1"})).unwrap();
        let pv = s.execute("file.printPreview", json!({})).unwrap();
        assert_eq!(pv["pageList"][0]["rows"], "1:10");
        // Title row takes one line on later pages: 44 body rows.
        assert_eq!(pv["pageList"][1]["rows"], "11:54");
    }

    #[test]
    fn wide_sheet_goes_down_then_over() {
        let mut s = s();
        let rows: Vec<Vec<Json>> = (0..60).map(|r| (0..15).map(|c| json!(r * 100 + c)).collect()).collect();
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        let pv = s.execute("file.printPreview", json!({})).unwrap();
        // 511.2pt / 48pt = 10 columns per page; 45 rows per page.
        assert_eq!(pv["pages"], 4);
        assert_eq!(pv["pageList"][0]["range"], "A1:J45");
        assert_eq!(pv["pageList"][1]["range"], "A46:J60");
        assert_eq!(pv["pageList"][2]["range"], "K1:O45");
    }

    #[test]
    fn pdf_contains_text_and_header_codes_expand() {
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Quarterly Report", null], ["Total", 1234.5]]})).unwrap();
        s.execute("home.bold", json!({"range": "A1"})).unwrap();
        s.execute(
            "pageLayout.pageSetup",
            json!({"header": "&LLeft &A&CMiddle&RPage &P of &N", "footer": "&C&F", "gridlines": true, "headings": true}),
        )
        .unwrap();
        let (pdf, pages) = pdf_of(&mut s, json!({}));
        assert_eq!(pages, 1);
        assert!(contains(&pdf, "(Quarterly Report) Tj"));
        assert!(contains(&pdf, "(1234.5) Tj"));
        assert!(contains(&pdf, "(Left Sheet1) Tj"));
        assert!(contains(&pdf, "(Page 1 of 1) Tj"));
        assert!(contains(&pdf, "(Book1) Tj"));
        assert!(contains(&pdf, "/Helvetica-Bold"));
        let cx = HfContext {
            page: 3,
            pages: 9,
            sheet: "Data".into(),
            file: "b.xlsx".into(),
            date: "1/2/2026".into(),
            time: "9:00 AM".into(),
            ..Default::default()
        };
        let [l, c, r] = expand_header_footer("&L&\"Arial,Bold\"&A &D&C&14&BTitle && more&RP&P+1/&N &T", &cx);
        assert_eq!(l.text, "Data 1/2/2026");
        assert!(l.bold);
        assert_eq!(c.text, "Title & more");
        assert!(c.bold);
        assert_eq!(c.size, Some(14.0));
        assert_eq!(r.text, "P4/9 9:00 AM");
        assert_eq!(expand_header_footer("plain", &cx)[1].text, "plain");
        assert_eq!(expand_header_footer("&", &cx)[1].text, "");
    }

    #[test]
    fn export_writes_file() {
        let mut s = s();
        s.execute("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
        let dir = std::env::temp_dir().join(format!("gridcraft-print-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.pdf");
        let r = s.execute("file.exportPdf", json!({"path": path.to_str().unwrap()})).unwrap();
        assert_eq!(r["pages"], 1);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(contains(&bytes, "(hello) Tj"));
        let _ = std::fs::remove_dir_all(&dir);
        let r = s.execute("file.print", json!({})).unwrap();
        assert!(r["base64"].as_str().unwrap().len() > 100);
    }

    #[test]
    fn rich_sheet_renders() {
        let mut s = Session::new();
        s.execute("file.new", json!({"sample": "sales"})).unwrap();
        s.execute("range.setValues", json!({"range": "H1", "values": [["A long piece of text that overflows into neighbours"]]})).unwrap();
        s.execute("cell.set", json!({"cell": "H3", "input": "Wrapped text across several lines in a narrow cell"})).unwrap();
        s.execute("home.wrapText", json!({"range": "H3"})).unwrap_or_default();
        s.execute("selection.set", json!({"range": "J5:K6"})).unwrap();
        s.execute("home.mergeCenter", json!({})).unwrap();
        s.execute("cell.set", json!({"cell": "J5", "input": "Merged"})).unwrap();
        s.execute("home.borders", json!({"range": "J5:K6", "preset": "outside", "style": "thick"})).unwrap_or_default();
        let (pdf, pages) = pdf_of(&mut s, json!({"sheets": "all"}));
        assert!(pages >= 1);
        assert!(contains(&pdf, "(Merged) Tj"));
        assert!(contains(&pdf, "(A long piece of text that overflows into neighbours) Tj"));
    }

    #[test]
    fn charts_and_images_are_drawn() {
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Fruit", "Qty"], ["Apples", 3], ["Pears", 5]]})).unwrap();
        s.execute("insert.chart", json!({"range": "A1:B3", "type": "pie"})).unwrap_or_default();
        s.execute("insert.chart", json!({"range": "A1:B3"})).unwrap_or_default();
        // A 2×2 PNG with transparency.
        let mut png = Vec::new();
        let img = image::RgbaImage::from_raw(2, 2, vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 0, 0, 0, 0]).unwrap();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let d = s.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let id = wb.next_object_id();
        wb.sheet_mut(0).unwrap().images.push(Image {
            id,
            anchor: Anchor { cell: CellRef::new(5, 5), dx: 0.0, dy: 0.0, width: 40.0, height: 40.0, mode: gridcraft_model::AnchorMode::MoveAndSize },
            data: png,
            mime: "image/png".into(),
            alt: String::new(),
        });
        let charts = s.doc().unwrap().wb.active().unwrap().charts.len();
        let (pdf, _) = pdf_of(&mut s, json!({}));
        assert!(contains(&pdf, "/Subtype /Image"));
        assert!(contains(&pdf, "/SMask"));
        if charts > 0 {
            assert!(contains(&pdf, "(Apples) Tj") || contains(&pdf, "(Qty) Tj"));
        }
    }

    #[test]
    fn page_setup_edits_settings() {
        let mut s = s();
        let r = s
            .execute(
                "pageLayout.pageSetup",
                json!({"paper": "A4", "orientation": "landscape", "margins": {"left": 1, "top": 0.5}, "right": 0.25, "centerH": true, "printArea": "A1:D20", "titleRows": "1:2", "titleCols": "A:A", "colBreaks": ["C"]}),
            )
            .unwrap();
        assert_eq!(r["paper"], "A4");
        assert_eq!(r["orientation"], "Landscape");
        assert_eq!(r["margins"][0], 1.0);
        assert_eq!(r["margins"][1], 0.25);
        assert_eq!(r["margins"][2], 0.5);
        assert_eq!(r["center_h"], true);
        assert_eq!(r["title_rows"], json!([0, 1]));
        assert_eq!(r["col_breaks"], json!([2]));
        let sh = s.doc().unwrap().wb.active().unwrap().print.clone();
        assert_eq!(sh.print_area, RangeRef::parse("A1:D20"));
        s.execute("pageLayout.pageSetup", json!({"printArea": ""})).unwrap();
        assert_eq!(s.doc().unwrap().wb.active().unwrap().print.print_area, None);
        assert!(s.execute("pageLayout.pageSetup", json!({"paper": "Napkin"})).is_err());
        assert!(s.execute("pageLayout.pageSetup", json!({"orientation": "sideways"})).is_err());
        // Undoable as one step.
        s.execute("edit.undo", json!({})).unwrap();
        assert!(s.doc().unwrap().wb.active().unwrap().print.print_area.is_some());
    }

    #[test]
    fn hostile_settings_never_panic_and_cap_pages() {
        let mut s = s();
        s.execute("cell.set", json!({"cell": "XFD1048576", "input": "far"})).unwrap();
        s.execute("cell.set", json!({"cell": "A1", "input": "near"})).unwrap();
        s.execute("pageLayout.pageSetup", json!({"scale": 400, "margins": {"left": 20, "right": 20, "top": 20, "bottom": 20}})).unwrap();
        let pv = s.execute("file.printPreview", json!({})).unwrap();
        assert!(pv["pages"].as_u64().unwrap() <= MAX_PAGES as u64);
        assert_eq!(pv["pages"].as_u64().unwrap(), MAX_PAGES as u64);
        let r = s.execute("file.exportPdf", json!({})).unwrap();
        assert_eq!(r["pages"], MAX_PAGES);
        s.execute("pageLayout.pageSetup", json!({"fitWidth": 1, "fitHeight": 1, "titleRows": "1:1048576", "titleCols": "A:XFD", "headings": true}))
            .unwrap();
        let pv = s.execute("file.printPreview", json!({})).unwrap();
        assert!(pv["pages"].as_u64().unwrap() <= MAX_PAGES as u64);
        assert!(s.execute("file.exportPdf", json!({"range": "A1:C3"})).is_ok());
        assert!(s.execute("file.exportPdf", json!({"range": "nope"})).is_err());
        assert!(s.execute("file.exportPdf", json!({"sheets": "some"})).is_err());
        // Empty workbook still prints one blank page.
        let mut e = Session::new();
        e.new_workbook();
        let r = e.execute("file.exportPdf", json!({})).unwrap();
        assert_eq!(r["pages"], 1);
        let r = e.execute("file.exportPdf", json!({"sheets": "all"})).unwrap();
        assert_eq!(r["pages"], 1);
        // Weird row sizes and huge fonts.
        s.execute("pageLayout.pageSetup", json!({"fitToPage": false, "scale": 10, "printArea": "A1:B3", "titleRows": "", "titleCols": ""})).unwrap();
        s.execute("home.fontSize", json!({"range": "A1", "size": 409})).unwrap_or_default();
        assert!(s.execute("file.exportPdf", json!({})).is_ok());
    }

    #[test]
    fn bands_unit() {
        let b = bands(0, 9, |_| 10.0, |_| 35.0, &[], 100);
        assert_eq!(b, vec![(0, 2), (3, 5), (6, 8), (9, 9)]);
        let b = bands(0, 9, |_| 10.0, |_| 35.0, &[5], 100);
        assert_eq!(b, vec![(0, 2), (3, 4), (5, 7), (8, 9)]);
        // A line larger than the page gets its own band.
        let b = bands(0, 2, |r| if r == 1 { 100.0 } else { 10.0 }, |_| 35.0, &[], 100);
        assert_eq!(b, vec![(0, 0), (1, 1), (2, 2)]);
        assert_eq!(bands(0, 1_000_000, |_| 10.0, |_| 5.0, &[], 3).len(), 3);
        let w = wrap_lines("the quick brown fox jumps", Font::Helvetica, 10.0, 50.0);
        assert!(w.len() >= 2);
        assert!(w.iter().all(|l| gridcraft_pdf::text_width(Font::Helvetica, 10.0, l) <= 50.0 || !l.contains(' ')));
        let w = wrap_lines("Supercalifragilistic", Font::Helvetica, 10.0, 20.0);
        assert!(w.len() > 2);
    }

    #[test]
    fn rotated_wrapped_text_prints_rotated() {
        // Upright headers over narrow columns (mark sheets) combine rotation with Wrap Text.
        let mut s = s();
        let header = "Term 1 : Drama : Continuous Assessment";
        s.execute("range.setValues", json!({"range": "A1", "values": [[header, header]]})).unwrap();
        s.execute("home.wrapText", json!({"range": "A1:B1", "on": true})).unwrap();
        s.execute("home.orientation", json!({"range": "A1", "angle": "up"})).unwrap();
        s.execute("home.orientation", json!({"range": "B1", "angle": "down"})).unwrap();
        s.execute("home.columnWidth", json!({"cols": "A:B", "chars": 6})).unwrap();
        s.execute("home.rowHeight", json!({"rows": "1:1", "height": 267})).unwrap();
        let (pdf, _) = pdf_of(&mut s, json!({}));
        let pdf = String::from_utf8_lossy(&pdf);
        // The page holds only these two cells, so every text block on it is a piece of a header.
        let blocks: Vec<&str> = pdf.split("BT\n").skip(1).filter(|b| b.contains(" Tj")).collect();
        let up = blocks.iter().filter(|b| b.contains("\n0 1 -1 0 ")).count();
        let down = blocks.iter().filter(|b| b.contains("\n0 -1 1 0 ")).count();
        // Every piece of both headers is rotated, each wrapped along the row height into a few long lines.
        assert_eq!(up + down, blocks.len(), "a header line printed flat: {blocks:?}");
        assert!((1..=3).contains(&up) && (1..=3).contains(&down), "up {up}, down {down}: {blocks:?}");
    }
}
