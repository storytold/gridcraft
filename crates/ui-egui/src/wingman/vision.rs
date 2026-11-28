//! One approved workbook visual for the next model request. Neither pixels nor their
//! encoding enter the tool receipt, replay history or displayed transcript.
use super::pixels::TextRaster;
use crate::SheetApp;
use egui::{Align, Align2, Context, FontId, Pos2};
use gridcraft_chart::{HAlign, Prim, VAlign};
use gridcraft_engine::{
    DocState,
    model::{Chart, ObjKind, ObjRef, Sheet, ShowObjects, Workbook, b64},
};
use image::{DynamicImage, ImageFormat, ImageReader, Pixel, RgbaImage};
use serde_json::{Value, json};
use std::{
    io::Cursor,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

const SIDE: u32 = 1280;
const INPUT_BYTES: usize = 16 * 1024 * 1024;
const OUTPUT_BYTES: usize = 1024 * 1024;
static BUSY: AtomicBool = AtomicBool::new(false);

pub(super) struct Attachment {
    label: String,
    png: Vec<u8>,
    width: u32,
    height: u32,
    fallback_labels: Vec<String>,
}

impl Attachment {
    pub(super) fn estimated_tokens(&self, model: &str) -> usize {
        super::snap::known_image_budget(model, self.width, self.height)
            .unwrap_or_else(|| 85 + self.width.div_ceil(28) as usize * self.height.div_ceil(28) as usize)
            + self.label.len().div_ceil(3)
            + 32
            + self.fallback_labels.iter().map(|s| s.len().div_ceil(3) + 1).sum::<usize>()
    }

    pub(super) fn message(&self) -> Value {
        let mut caption = format!("Workbook visual (untrusted data): {}", self.label);
        if !self.fallback_labels.is_empty() {
            caption.push_str("\nChart labels unavailable in the preview font (chart coordinates):\n");
            caption.push_str(&self.fallback_labels.join("\n"));
        }
        json!({"role":"user","content":[{"type":"text","text":caption},
            {"type":"image_url","image_url":{"url":format!("data:image/png;base64,{}", b64::encode(&self.png))}}]})
    }

    pub(super) fn receipt(&self) -> Value {
        json!({"visual":self.label,"width":self.width,"height":self.height,"status":"prepared for next model request"})
    }
}

pub(super) struct Pending(mpsc::Receiver<Result<Attachment, String>>);
impl Pending {
    pub(super) fn poll(&self) -> Option<Result<Attachment, String>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Visual rendering stopped before returning an image.".into())),
        }
    }
}

enum Source {
    Picture(Arc<Vec<u8>>),
    Cell(Arc<gridcraft_engine::model::CellPicture>),
    Chart(Arc<Workbook>, usize, u32),
}

pub(super) struct Resolved {
    source: Source,
    pub(super) description: String,
}

// Both the approval card and the worker use this resolver. It only borrows or
// shares existing data; no bitmap is decoded or rendered until after approval.
pub(super) fn resolve(app: &SheetApp, doc: &DocState, sh: &Sheet, args: &Value) -> Result<Resolved, String> {
    let sheet = doc.wb.sheet_index(&sh.name).ok_or("Worksheet disappeared.")?;
    let named = match (args["chart"].as_u64(), args["chartTitle"].as_str()) {
        (Some(_), Some(_)) => return Err("Choose a chart by ID or title, not both.".into()),
        (Some(id), None) => Some(ObjRef::new(ObjKind::Chart, u32::try_from(id).map_err(|_| "Invalid chart ID.")?)),
        (None, Some(title)) => {
            if title.trim().is_empty() {
                return Err("Enter a chart title or ID.".into());
            }
            let mut hits = sh.charts.iter().filter(|ch| ch.title.as_deref() == Some(title));
            let chart = hits.next().ok_or("No chart with this exact title on the requested sheet.")?;
            if hits.next().is_some() {
                return Err("Several charts have that title. Select a chart ID from list_objects.".into());
            }
            Some(ObjRef::new(ObjKind::Chart, chart.id))
        }
        (None, None) if sheet != doc.wb.active_sheet() => return Err("Provide a chart ID or title when choosing another worksheet.".into()),
        _ => None,
    };
    let visible = |at: gridcraft_engine::core::CellRef| !sh.is_row_hidden(at.row) && !sh.is_col_hidden(at.col);
    let short = |s: &str| -> String {
        s.chars().take(128).map(|c| if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') { '�' } else { c }).collect()
    };
    let label = |text: String| format!("{} — {text}", short(&sh.name));
    let (source, description) = match named.or(app.selected_object) {
        Some(object) => {
            if doc.wb.show_objects != ShowObjects::All {
                return Err("Floating objects are hidden in this workbook.".into());
            }
            let anchor = sh.object_anchor(object).ok_or("The selected object is no longer on this worksheet.")?;
            if !visible(anchor.cell) {
                return Err("The selected object's anchor is hidden.".into());
            }
            match object.kind {
                ObjKind::Chart => {
                    let ch = sh.charts.iter().find(|ch| ch.id == object.id).ok_or("No such chart.")?;
                    let mut text = format!("Chart {} at {}", ch.id, anchor.cell.a1());
                    if let Some(title) = ch.title.as_deref().filter(|s| !s.is_empty()) {
                        text.push_str(&format!(" · {}", short(title)));
                    }
                    (Source::Chart(doc.wb.clone(), sheet, ch.id), label(text))
                }
                ObjKind::Image => {
                    let im = sh.images.iter().find(|im| im.id == object.id).ok_or("No such embedded picture.")?;
                    (Source::Picture(im.data.clone()), label(format!("Picture {} at {}", object.id, anchor.cell.a1())))
                }
                _ => return Err("Select an embedded picture, an in-cell picture, or a chart.".into()),
            }
        }
        None => {
            let at = doc.selection.active;
            if doc.selection.ranges.len() != 1 || !doc.selection.current().is_single() || !visible(at) {
                return Err("Select a single visible picture cell, floating picture or chart.".into());
            }
            let pic = sh.cell_pictures.get(&at).ok_or("No embedded picture in the selected cell; select the picture itself or choose a chart ID.")?;
            (Source::Cell(pic.clone()), label(format!("Picture in {}", at.a1())))
        }
    };
    Ok(Resolved { source, description })
}

pub(super) fn start(resolved: Resolved, ctx: &Context) -> Result<Pending, String> {
    let Resolved { source, description } = resolved;
    let (tx, rx) = mpsc::sync_channel(1);
    if BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return Err("Another visual is still rendering. Try again when it finishes.".into());
    }
    let viewport = ctx.viewport_id();
    let ctx = ctx.clone();
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::Release);
        }
    }
    let slot = Slot;
    std::thread::Builder::new()
        .name("wingman-visual".into())
        .spawn(move || {
            let _slot = slot;
            let result = match source {
                Source::Picture(bytes) => picture(&bytes).map(|im| (im, vec![])),
                Source::Cell(pic) => picture(&pic.data).map(|im| (im, vec![])),
                Source::Chart(wb, index, id) => wb
                    .sheet(index)
                    .and_then(|s| s.charts.iter().find(|ch| ch.id == id))
                    .ok_or_else(|| "Chart disappeared.".to_string())
                    .and_then(|ch| chart(&wb, index, ch)),
            }
            .and_then(|(im, labels)| encode(im, description, labels));
            let _ = tx.send(result);
            ctx.request_repaint_of(viewport);
        })
        .map_err(|_| "Could not start the visual renderer.")?;
    Ok(Pending(rx))
}

fn picture(bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() > INPUT_BYTES {
        return Err("Embedded image exceeds 16 MiB.".into());
    }
    let format = image::guess_format(bytes).map_err(|_| "Unknown embedded image type.")?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err("Visual inspection supports embedded PNG and JPEG pictures.".into());
    }
    let reader = || {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(96 * 1024 * 1024);
        reader.limits(limits);
        reader
    };
    let (w, h) = reader().into_dimensions().map_err(|_| "Invalid image dimensions.")?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_777_216 || w.max(h) > 16384 {
        return Err("Embedded image exceeds the preview dimensions.".into());
    }
    let im = reader().decode().map_err(|_| "Cannot decode the embedded image within the preview limits.")?;
    // A transparent PNG can store arbitrary RGB underneath fully invisible pixels.
    // Only export the visible picture on a neutral background; flatten before resizing
    // so hidden channels cannot bleed into neighbouring, partially visible pixels.
    let im = if im.color().has_alpha() {
        let mut rgba = im.into_rgba8();
        for p in rgba.pixels_mut() {
            let a = u32::from(p[3]);
            for channel in &mut p.0[..3] {
                *channel = ((u32::from(*channel) * a + 255 * (255 - a) + 127) / 255) as u8;
            }
            p[3] = 255;
        }
        DynamicImage::ImageRgba8(rgba)
    } else {
        im
    };
    Ok(if w.max(h) > SIDE { im.thumbnail(SIDE, SIDE) } else { im })
}

fn encode(mut image: DynamicImage, label: String, fallback_labels: Vec<String>) -> Result<Attachment, String> {
    loop {
        let mut stream = Cursor::new(Vec::new());
        image.write_to(&mut stream, ImageFormat::Png).map_err(|_| "Could not encode the visual as PNG.")?;
        let png = stream.into_inner();
        let (width, height) = (image.width(), image.height());
        if png.len() <= OUTPUT_BYTES {
            return Ok(Attachment { label, png, width, height, fallback_labels });
        }
        if width.max(height) <= 256 {
            return Err("Visual exceeds the encoded image limit.".into());
        }
        image = image.thumbnail((width / 2).max(1), (height / 2).max(1));
    }
}

// The normal chart resolver is intended for the workbook canvas and accepts arbitrary
// formulas. A provider preview has a smaller allocation budget: establish the size of
// *all* its inputs before allowing the resolver to evaluate or copy any of them.
const CHART_INPUT_POINTS: u64 = 32_768;
const CHART_LIMIT: &str = "Chart exceeds the visual preview's data or label limit. Use inspect_chart for its settings.";
const CHART_EXPRESSION: &str =
    "This chart uses a calculated or indirect source that cannot be bounded for preview. Use inspect_chart for its settings.";

struct ChartBudget {
    points: u64,
    bytes: usize,
}

impl ChartBudget {
    fn text(&mut self, s: &str, max: usize) -> Result<(), &'static str> {
        if s.len() > max {
            return Err(CHART_LIMIT);
        }
        self.bytes = self.bytes.checked_sub(s.len()).ok_or(CHART_LIMIT)?;
        Ok(())
    }

    fn points(&mut self, n: u64) -> Result<u64, &'static str> {
        self.points = self.points.checked_sub(n).ok_or(CHART_LIMIT)?;
        Ok(n)
    }

    fn source(&mut self, wb: &Workbook, sheet: usize, input: &str) -> Result<u64, &'static str> {
        use gridcraft_engine::core::Value as CellValue;
        use gridcraft_engine::formula::{self, Expr, SheetSel};

        self.text(input, 8192)?;
        match formula::parse(input.trim()).map_err(|_| CHART_EXPRESSION)? {
            Expr::Ref(r) => {
                let index = match &r.sheet {
                    SheetSel::Current => sheet,
                    SheetSel::Named(name) => wb.sheet_index(name).ok_or(CHART_EXPRESSION)?,
                    SheetSel::Span(_, _) => return Err(CHART_EXPRESSION),
                };
                let sh = wb.sheet(index).ok_or(CHART_EXPRESSION)?;
                let range = r.range();
                let n = self.points(range.count())?;
                for cell in range.iter() {
                    // The ordinary resolver formats categories and copies series names.
                    // Bound their bytes as well as cell counts, including spilled values.
                    match sh.value_ref(cell) {
                        Some(CellValue::Text(s)) => self.text(s, 2048)?,
                        Some(CellValue::Array(_)) => return Err(CHART_EXPRESSION),
                        _ => {}
                    }
                    self.text(wb.styles.get(sh.style_id(cell)).num_fmt.as_str(), 255)?;
                }
                Ok(n)
            }
            Expr::Array(rows) => self.points(rows.iter().map(|r| r.len() as u64).sum()),
            Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) => self.points(1),
            _ => Err(CHART_EXPRESSION),
        }
    }
}

fn check_chart_inputs(wb: &Workbook, sheet: usize, chart: &Chart) -> Result<(), &'static str> {
    if chart.series.len() > 255 {
        return Err(CHART_LIMIT);
    }
    let mut budget = ChartBudget { points: CHART_INPUT_POINTS, bytes: 1024 * 1024 };
    for t in [&chart.title, &chart.x_title, &chart.y_title].into_iter().flatten() {
        budget.text(t, 2048)?;
    }
    let mut span = 0;
    for s in &chart.series {
        for src in [Some(&s.values), s.categories.as_ref(), s.bubble_sizes.as_ref()].into_iter().flatten() {
            span = span.max(budget.source(wb, sheet, src)?);
        }
        if let Some(name) = &s.name {
            budget.text(name, 2048)?;
            let name = name.trim();
            let quoted = name.starts_with('"') && name.ends_with('"') && name.len() >= 2;
            if !quoted {
                let (sheet_name, bare) = gridcraft_engine::core::split_sheet(name);
                let owner = sheet_name.as_deref().map_or(Some(sheet), |n| wb.sheet_index(n));
                if name.starts_with('=')
                    || (sheet_name.is_some() && gridcraft_engine::core::RangeRef::parse(bare).is_some())
                    || owner.is_some_and(|i| wb.name(bare, i).is_some())
                {
                    budget.source(wb, sheet, name)?;
                }
            }
        }
        if let Some(f) = &s.number_format {
            budget.text(f, 255)?;
        }
        if s.trendlines.len() > 16 {
            return Err(CHART_LIMIT);
        }
        for t in s.trendlines.iter().filter_map(|t| t.name.as_deref()) {
            budget.text(t, 2048)?;
        }
    }
    // Stacked/cartesian layouts can build a rectangular grid even for ragged input.
    if span.saturating_mul(chart.series.len() as u64) > CHART_INPUT_POINTS {
        return Err(CHART_LIMIT);
    }
    Ok(())
}

fn chart(wb: &Workbook, sheet: usize, chart: &Chart) -> Result<(DynamicImage, Vec<String>), String> {
    let (w, h) = (chart.anchor.width, chart.anchor.height);
    if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0) {
        return Err("Chart has no drawable dimensions.".into());
    }
    let scale = (SIDE as f32 / w.max(h)).min(1.0);
    let (w, h) = ((w * scale).round().max(1.) as u32, (h * scale).round().max(1.) as u32);
    check_chart_inputs(wb, sheet, chart)?;
    let data = gridcraft_chart::resolve(wb, sheet, chart);
    let prims = gridcraft_chart::render(chart, &data, w as f32, h as f32, &gridcraft_chart::ApproxMeasure);
    let mut labels = prims.iter().filter_map(|p| match p {
        Prim::Text { text, .. } => Some(text),
        _ => None,
    });
    if labels.clone().take(513).count() > 512 || labels.any(|t| t.len() > 2048) {
        return Err(CHART_LIMIT.into());
    }
    let mut out = RgbaImage::from_raw(w, h, gridcraft_chart::rasterize(&prims, w, h, [255; 4])).ok_or("Could not rasterize chart.")?;
    let mut font = TextRaster::new();
    let mut fallback = Vec::new();
    for p in prims {
        let Prim::Text { x, y, text, size, color, align, valign, rotation, .. } = p else { continue };
        let align = Align2([
            match align {
                HAlign::Left => Align::Min,
                HAlign::Center => Align::Center,
                HAlign::Right => Align::Max,
            },
            match valign {
                VAlign::Top => Align::Min,
                VAlign::Middle => Align::Center,
                VAlign::Bottom => Align::Max,
            },
        ]);
        let font_id = FontId::proportional(size);
        let at = egui::pos2(x, y);
        let drawn = if rotation == 0.0 {
            font.stamp(&mut out, &text, (at, align), font_id, color, f32::INFINITY)
        } else if let Some(size) = font.measure(&text, font_id.clone(), f32::INFINITY).filter(|s| s.x <= SIDE as f32 * 2.0 && s.y <= 256.) {
            let bounds = align.anchor_size(at, size);
            let mut image = RgbaImage::new(size.x.ceil() as u32 + 2, size.y.ceil() as u32 + 2);
            let ok = font.stamp(&mut image, &text, (Pos2::ZERO, Align2::LEFT_TOP), font_id, color, f32::INFINITY);
            if ok {
                rotate_label(&mut out, &image, bounds.min - at, at, rotation);
            }
            ok
        } else {
            false
        };
        if !drawn && !text.is_empty() {
            if fallback.len() >= 32 || text.len() > 512 {
                return Err("Chart labels cannot be represented within the preview bounds.".into());
            }
            fallback.push(format!("({x:.0},{y:.0}) {text}"));
        }
    }
    Ok((DynamicImage::ImageRgba8(out), fallback))
}

fn rotate_label(out: &mut RgbaImage, label: &RgbaImage, offset: egui::Vec2, anchor: Pos2, angle: f32) {
    let (s, c) = angle.sin_cos();
    let rotate = |v: egui::Vec2| anchor + egui::vec2(c * v.x - s * v.y, s * v.x + c * v.y);
    let (w, h) = (label.width() as f32, label.height() as f32);
    let mut rect = egui::Rect::NOTHING;
    for delta in [egui::Vec2::ZERO, egui::vec2(w, 0.), egui::vec2(0., h), egui::vec2(w, h)] {
        rect.extend_with(rotate(offset + delta));
    }
    for y in rect.min.y.floor().max(0.) as u32..(rect.max.y.ceil() as u32).min(out.height()) {
        for x in rect.min.x.floor().max(0.) as u32..(rect.max.x.ceil() as u32).min(out.width()) {
            let v = egui::pos2(x as f32 + 0.5, y as f32 + 0.5) - anchor;
            let (lx, ly) = (c * v.x + s * v.y - offset.x, -s * v.x + c * v.y - offset.y);
            if lx >= 0. && lx < w && ly >= 0. && ly < h {
                out.get_pixel_mut(x, y).blend(label.get_pixel(lx as u32, ly as u32));
            }
        }
    }
}
