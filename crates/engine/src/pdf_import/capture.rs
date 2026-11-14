use super::{Control, ImportError, MAX_GLYPHS, MAX_SEGMENTS, MAX_TEXT, TOLERANCE};
use craft_pdf::interpret::{BlendMode, ClipPath, Device, GlyphDrawMode, Image, Paint, PathDrawMode, SoftMask, font::Glyph, hayro_cmap::BfString};
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug)]
pub(super) struct Segment {
    pub horizontal: bool,
    pub level: f64,
    pub start: f64,
    pub end: f64,
}
#[derive(Debug)]
pub(super) struct Letter {
    pub baseline: Point,
    pub center: Point,
    pub end: f64,
    pub height: f64,
    pub text: String,
}
pub(super) struct Capture {
    pub control: Control,
    pub failure: Option<ImportError>,
    pub segments: Vec<Segment>,
    pub letters: Vec<Letter>,
    pub images: Vec<Rect>,
    clip: Rect,
    clips: Vec<Rect>,
    groups: Vec<bool>,
    text_bytes: usize,
}
impl Capture {
    pub fn new(control: Control, clip: Rect) -> Self {
        Self {
            control,
            failure: None,
            segments: Vec::new(),
            letters: Vec::new(),
            images: Vec::new(),
            clip,
            clips: Vec::new(),
            groups: Vec::new(),
            text_bytes: 0,
        }
    }
    fn fail(&mut self, e: ImportError) {
        if self.failure.is_none() {
            self.failure = Some(e);
        }
        self.control.stop.store(true, Ordering::Relaxed);
    }
    fn ready(&mut self) -> bool {
        if self.failure.is_some() {
            return false;
        }
        if let Err(e) = self.control.spend(1) {
            self.fail(e);
            return false;
        }
        true
    }
    fn visible(&mut self, paint: &Paint<'_>) -> bool {
        if !self.ready() || self.groups.last() == Some(&false) {
            return false;
        }
        match paint {
            Paint::Color(c) => c.to_rgba().components()[3] > 0.0,
            Paint::Pattern(_) => {
                self.fail(ImportError::Unsupported("pattern-painted table content"));
                false
            }
        }
    }
    fn segment(&mut self, a: Point, b: Point) {
        if !self.ready() {
            return;
        }
        if ![a.x, a.y, b.x, b.y].iter().all(|n| n.is_finite()) {
            self.fail(ImportError::Invalid("non-finite drawing coordinates".into()));
            return;
        }
        let horizontal = (a.y - b.y).abs() < 0.01;
        let vertical = (a.x - b.x).abs() < 0.01;
        if !horizontal && !vertical {
            return;
        }
        let (level, start, end, lo, hi, perp_lo, perp_hi) = if horizontal {
            (a.y, a.x.min(b.x), a.x.max(b.x), self.clip.x0, self.clip.x1, self.clip.y0, self.clip.y1)
        } else {
            (a.x, a.y.min(b.y), a.y.max(b.y), self.clip.y0, self.clip.y1, self.clip.x0, self.clip.x1)
        };
        if level < perp_lo || level > perp_hi {
            return;
        }
        let (start, end) = (start.max(lo), end.min(hi));
        if end - start < 1.0 {
            return;
        }
        if self.segments.len() >= MAX_SEGMENTS {
            self.fail(ImportError::Limit("line segments"));
            return;
        }
        self.segments.push(Segment { horizontal, level, start, end });
    }
}
fn rectangle(path: &BezPath) -> Option<Rect> {
    if path.elements().len() > 6 {
        return None;
    }
    let points: Option<Vec<Point>> = path
        .elements()
        .iter()
        .filter(|e| !matches!(e, PathEl::ClosePath))
        .map(|e| match e {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => Some(*p),
            _ => None,
        })
        .collect();
    let mut points = points?;
    if points.len() == 5 && points.first() == points.last() {
        points.pop();
    }
    if points.len() != 4 {
        return None;
    }
    let bounds = path.bounding_box();
    let corners =
        [Point::new(bounds.x0, bounds.y0), Point::new(bounds.x1, bounds.y0), Point::new(bounds.x1, bounds.y1), Point::new(bounds.x0, bounds.y1)];
    if !corners.iter().all(|c| points.iter().any(|p| c.distance(*p) < 0.01)) {
        return None;
    }
    for i in 0..4 {
        let a = points[i];
        let b = points[(i + 1) % 4];
        if (a.x - b.x).abs() > 0.01 && (a.y - b.y).abs() > 0.01 {
            return None;
        }
    }
    Some(bounds)
}
impl<'a> Device<'a> for Capture {
    fn set_soft_mask(&mut self, mask: Option<SoftMask<'a>>) {
        if mask.is_some() {
            self.fail(ImportError::Unsupported("soft-masked content"));
        }
    }
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn push_clip_path(&mut self, path: &ClipPath) {
        if !self.ready() {
            return;
        }
        let Some(bounds) = rectangle(&path.path) else {
            self.fail(ImportError::Unsupported("a non-rectangular clipping path"));
            return;
        };
        if self.clips.len() >= 64 {
            self.fail(ImportError::Limit("clip depth"));
            return;
        }
        self.clips.push(self.clip);
        self.clip = self.clip.intersect(bounds);
    }
    fn pop_clip_path(&mut self) {
        if let Some(clip) = self.clips.pop() {
            self.clip = clip;
        }
    }
    fn push_transparency_group(&mut self, opacity: f32, mask: Option<SoftMask<'a>>, _: BlendMode) {
        if self.groups.len() >= 64 {
            self.fail(ImportError::Limit("transparency depth"));
            return;
        }
        self.groups.push(opacity > 0.0 && self.groups.last() != Some(&false));
        if mask.is_some() {
            self.fail(ImportError::Unsupported("masked transparency"));
        }
    }
    fn pop_transparency_group(&mut self) {
        self.groups.pop();
    }
    fn draw_image(&mut self, _: Image<'a, '_>, transform: Affine) {
        if !self.ready() || self.groups.last() == Some(&false) {
            return;
        }
        if self.images.len() >= MAX_SEGMENTS {
            self.fail(ImportError::Limit("images"));
            return;
        }
        self.images.push(transform.transform_rect_bbox(Rect::new(0., 0., 1., 1.)).intersect(self.clip));
    }
    fn actual_text(&mut self, _: &[u8]) {
        // The replacement may span several glyphs/cells. Refuse until those mappings are supported.
        self.fail(ImportError::Unsupported("marked-content replacement text"));
    }
    fn set_knockout(&mut self) {
        self.fail(ImportError::Unsupported("knockout transparency"));
    }
    fn draw_path(&mut self, path: &BezPath, t: Affine, paint: &Paint<'a>, mode: &PathDrawMode) {
        if !self.visible(paint) {
            return;
        }
        if matches!(mode, PathDrawMode::Fill(_)) {
            let transformed = t * path;
            if let Some(r) = rectangle(&transformed) {
                if r.width() <= 2.0 && r.height() > r.width() * 4.0 {
                    self.segment(Point::new((r.x0 + r.x1) * 0.5, r.y0), Point::new((r.x0 + r.x1) * 0.5, r.y1));
                } else if r.height() <= 2.0 && r.width() > r.height() * 4.0 {
                    self.segment(Point::new(r.x0, (r.y0 + r.y1) * 0.5), Point::new(r.x1, (r.y0 + r.y1) * 0.5));
                }
            }
            return;
        }
        let mut first = None;
        let mut last = None;
        for el in path.elements() {
            if !self.ready() {
                return;
            }
            match *el {
                PathEl::MoveTo(p) => {
                    first = Some(t * p);
                    last = first;
                }
                PathEl::LineTo(p) => {
                    let end = t * p;
                    if let Some(start) = last {
                        self.segment(start, end);
                    }
                    last = Some(end);
                }
                PathEl::ClosePath => {
                    if let (Some(a), Some(b)) = (last, first) {
                        self.segment(a, b);
                    }
                    last = first;
                }
                _ => last = None,
            }
        }
    }
    fn draw_glyph(&mut self, glyph: &Glyph<'a>, t: Affine, g: Affine, paint: &Paint<'a>, mode: &GlyphDrawMode) {
        if matches!(mode, GlyphDrawMode::Invisible | GlyphDrawMode::Clip) || !self.visible(paint) {
            return;
        }
        let transform = t * g;
        let baseline = transform * Point::ZERO;
        if !self.clip.contains(baseline) {
            return;
        }
        let Some(text) = glyph.as_unicode() else {
            self.fail(ImportError::Unsupported("text without a Unicode mapping"));
            return;
        };
        let text = match text {
            BfString::Char(c) => c.to_string(),
            BfString::String(s) => s,
        };
        let advance = match glyph {
            Glyph::Outline(g) => g.advance_width(),
            Glyph::Type3(g) => g.advance_width(),
        }
        .unwrap_or(0.0) as f64;
        let end = transform * Point::new(advance, 0.0);
        let height = (transform.as_coeffs()[3] * 1000.0).abs();
        if ![baseline.x, baseline.y, end.x, end.y, height].iter().all(|n| n.is_finite()) {
            self.fail(ImportError::Invalid("non-finite text coordinates".into()));
            return;
        }
        if (end.y - baseline.y).abs() > TOLERANCE || end.x < baseline.x {
            self.fail(ImportError::Unsupported("rotated or right-to-left text"));
            return;
        }
        if self.letters.len() >= MAX_GLYPHS || self.text_bytes.saturating_add(text.len()) > MAX_TEXT {
            self.fail(ImportError::Limit("page text"));
            return;
        }
        self.text_bytes += text.len();
        self.letters.push(Letter { baseline, center: Point::new((baseline.x + end.x) * 0.5, baseline.y - height * 0.25), end: end.x, height, text });
    }
}
