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
#[cfg(test)]
use gridcraft_model::{Alignment, Image, SheetViewMode};
use gridcraft_model::{
    Anchor, BorderLine, BorderStyle, Borders, Color, HAlign, Orientation, PatternType, PrintSettings, ShapeKind, Sheet, Style, StyleId, StyleTable,
    Underline, VAlign, Visibility, Workbook,
};
use gridcraft_pdf::{Font, Page, PdfDoc, Point, Rect, Rgb};
use serde_json::{Value as Json, json};

use super::{CommandSpec, Result, Session, bad, bool_param, cmd, edit, f64_param, has_doc, split_sheet, str_param, target_sheet, u32_param};

mod draw;
mod header_footer;
mod layout;
mod objects;
mod text;

use draw::{Block, PrintedBlock, draw_block, draw_block_links, draw_headings, make_block, printed_target};
use header_footer::draw_header_footer;
pub use header_footer::{
    HF_FIELDS, HfContext, HfSection, codes_to_fields, expand_header_footer, fields_to_codes, hf_date_time, join_header_footer, split_header_footer,
};
/// Decoded pictures (bytes, width, height) keyed by (sheet, picture); `None` = undecodable.
type PicCache = std::collections::HashMap<(usize, usize), Option<(Vec<u8>, u32, u32)>>;

#[cfg(test)]
use layout::bands;
pub(crate) use layout::split_bands;
pub use layout::{PrintOptions, SheetLayout, layout_sheet, layouts, paper_size, print_range};
use layout::{known_paper, seg_size, segments};
use objects::draw_objects;
#[cfg(test)]
use text::{center_across_width, wrap_lines};

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
            "{sheet?, orientation?: portrait|landscape, paper?: Letter|Legal|A4|A3|A5|A6|Tabloid|Executive|Ledger|B4|B5|B6|B4 (ISO)|B5 (ISO)|B6 (ISO)|Japanese Postcard|Japanese Double Postcard|Japanese Envelope #3|Japanese Envelope #4|Japanese Envelope Kaku #2|Japanese Envelope Kaku #3 (B4/B5/B6 are the JIS sizes), margins?: {left,right,top,bottom,header,footer} (inches; also accepted as top-level keys), scale?: 10..400, fitWidth?, fitHeight? (pages; 0 = automatic), fitToPage?: bool, centerH?, centerV?, gridlines?, headings?, header?: \"&L&A&RPage &P of &N\", footer?, printArea?: \"A1:F50\" (\"\" clears), titleRows?: \"1:2\", titleCols?: \"A:A\", rowBreaks?: [46, 91], colBreaks?: [\"K\"]} → the sheet's print settings",
            has_doc,
            page_setup
        ),
    ]
}

/// Sheet geometry is in pixels at 96 dpi; PDF units are points.
pub(crate) const PX_TO_PT: f32 = 0.75;
/// Most pages one export produces.
pub const MAX_PAGES: usize = 2000;

/// Renders laid-out sheets to a PDF.
pub fn render_pdf(wb: &Workbook, layouts: &[SheetLayout], file: &str, path: &str) -> (Vec<u8>, usize) {
    let mut doc = PdfDoc::new();
    doc.info.title = if wb.props.title.is_empty() { file.to_string() } else { wb.props.title.clone() };
    doc.info.author = wb.props.author.clone();
    let total: usize = layouts.iter().map(|l| l.pages.len()).sum();
    let (date, time) = hf_date_time(wb);
    let mut page_no = 0usize;
    let mut pending_links = Vec::new();
    let mut printed_blocks = Vec::new();
    // Decoded pictures are reused across the pages their anchor covers: a floating picture
    // spanning 40 pages was decoded (and pushed into the PDF) 40 times (#2389).
    let mut pics = PicCache::new();
    for lay in layouts {
        let Some(sh) = wb.sheet(lay.sheet) else { continue };
        let mut cf = if sh.cond_formats.is_empty() { None } else { Some(crate::cf::CfCache::for_export()) };
        let k = lay.k();
        let (aw, ah) = lay.avail();
        for (pi, (ri, ci)) in lay.pages.iter().enumerate() {
            page_no += 1;
            let (Some(rband), Some(cband)) = (lay.row_bands.get(*ri).copied(), lay.col_bands.get(*ci).copied()) else { continue };
            let rsegs = segments(lay.title_rows, rband);
            let csegs = segments(lay.title_cols, cband);
            let content_w = (lay.heading_w + csegs.iter().map(|s| seg_size(sh, *s, false)).sum::<f32>()) * k;
            let content_h = (lay.heading_h + rsegs.iter().map(|s| seg_size(sh, *s, true)).sum::<f32>()) * k;
            let ox = lay.margins[0] + if lay.settings.center_h { ((aw - content_w) / 2.0).max(0.0) } else { 0.0 };
            let oy = lay.margins[2] + if lay.settings.center_v { ((ah - content_h) / 2.0).max(0.0) } else { 0.0 };
            let page_index = doc.page_count();
            let pg = doc.add_page(lay.page_w, lay.page_h);
            let mut y = oy + lay.heading_h * k;
            let mut body: Option<Block> = None;
            for (rsi, rs) in rsegs.iter().enumerate() {
                let mut x = ox + lay.heading_w * k;
                for (csi, cs) in csegs.iter().enumerate() {
                    let b = make_block(wb, lay.sheet, sh, *rs, *cs, (x, y), lay);
                    draw_block(pg, &b, lay.settings.gridlines, &mut cf);
                    draw_block_links(pg, &b, page_index, &mut pending_links);
                    printed_blocks.push(PrintedBlock { page: page_index, sheet: lay.sheet, rows: *rs, cols: *cs, x, y, k });
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
                draw_objects(pg, wb, lay.sheet, sh, b, &mut pics);
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
            draw_header_footer(pg, lay, lay.settings.hf_page(pi == 0, page_no), &cx);
        }
    }
    for link in pending_links {
        if let Some((target_page, x, y)) = printed_target(wb, &printed_blocks, link.sheet, &link.target)
            && let Some(page) = doc.page_mut(link.page)
        {
            let (lx, ly, w, h) = link.rect;
            page.link_page(Rect::new(lx, ly, w, h), target_page, Point::new(x, y));
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

pub(crate) fn export_pdf(s: &mut Session, p: &Json) -> Result<Json> {
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
mod tests;
