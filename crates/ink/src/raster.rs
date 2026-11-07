//! From a stroke to a line image to a model input.
//!
//! Coordinates are sheet points, as `draw.stroke` stores them: x runs right, y runs down, one
//! unit is one point. The rasteriser keeps the stroke's aspect ratio and stretches the result to
//! the network's square input at the end, because a TrOCR encoder was trained on lines stretched
//! that way.

use crate::Error;

/// Width and height of the encoder's input, in pixels.
pub const IMAGE_SIZE: usize = 384;

/// Largest side of the rasterised line, in pixels. The model stretches it anyway; this bounds
/// the work and the memory a hostile stroke can ask for.
pub const MAX_SIDE: usize = 1024;

/// Most points one stroke may hold (the engine keeps the same cap for stored ink).
pub const MAX_POINTS: usize = 20_000;

/// Most segments rasterised from one stroke. A 1024-pixel line needs no more, and a stroke with
/// thousands of tiny segments (a slowed-down pen) costs the same as a decimated one.
const MAX_SEGMENTS: usize = 2_000;

/// Most segments one image may hold across all its strokes.
const MAX_SEGMENTS_TOTAL: usize = 8 * MAX_SEGMENTS;

/// Pixels the rasteriser visits before it stops adding segments. A stroke whose segments each
/// span the whole image (a hostile input) is cut short instead of taking seconds.
const MAX_WORK: usize = 40_000_000;

/// Share of the longer side left as paper around the ink, so a stroke never touches the edge.
const PADDING_SHARE: f32 = 0.10;
/// The padding's floor, in source points.
const PADDING_MIN: f32 = 6.0;

/// The box a stroke covers. `min`/`max` are the extremes of the points themselves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bbox {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Bbox {
    /// The box around `points`, ignoring non-finite ones. `None` when nothing finite is left.
    pub fn of(points: &[[f32; 2]]) -> Option<Bbox> {
        let mut b = Bbox { min_x: f32::INFINITY, min_y: f32::INFINITY, max_x: f32::NEG_INFINITY, max_y: f32::NEG_INFINITY };
        for p in points.iter().filter(|p| p[0].is_finite() && p[1].is_finite()).take(MAX_POINTS) {
            b.min_x = b.min_x.min(p[0]);
            b.min_y = b.min_y.min(p[1]);
            b.max_x = b.max_x.max(p[0]);
            b.max_y = b.max_y.max(p[1]);
        }
        (b.min_x <= b.max_x && b.min_y <= b.max_y).then_some(b)
    }

    pub fn width(&self) -> f32 {
        (self.max_x - self.min_x).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.max_y - self.min_y).max(0.0)
    }
}

/// A grey line image: `pixels[y * width + x]`, 0 is ink and 255 is paper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl LineImage {
    /// The pixel at `(x, y)`, or `None` outside the image.
    pub fn pixel(&self, x: usize, y: usize) -> Option<u8> {
        if x < self.width && y < self.height { self.pixels.get(y * self.width + x).copied() } else { None }
    }

    /// The darkest pixel: 255 is a blank image.
    pub fn darkest(&self) -> u8 {
        self.pixels.iter().copied().min().unwrap_or(255)
    }

    /// The four edges, all of which must be paper for the padding to be intact.
    pub fn border_is_paper(&self) -> bool {
        if self.width < 3 || self.height < 3 || self.pixels.len() != self.width * self.height {
            return false;
        }
        let last_row = (self.height - 1) * self.width;
        self.pixels[..self.width].iter().all(|&p| p >= 250)
            && self.pixels[last_row..last_row + self.width].iter().all(|&p| p >= 250)
            && (0..self.height).all(|y| self.pixels[y * self.width] >= 250 && self.pixels[y * self.width + self.width - 1] >= 250)
    }
}

/// Rasterises one stroke into a [`LineImage`]. See [`rasterize_strokes`].
pub fn rasterize(points: &[[f32; 2]], pen_width: f32) -> Result<LineImage, Error> {
    rasterize_strokes(&[points], pen_width)
}

/// Rasterises several strokes into one [`LineImage`], as one line of handwriting.
///
/// Each stroke is drawn on its own: no segment joins the end of one stroke to the start of the
/// next, so a letter written in two strokes (an "i", a crossed "t", a "4") stays that letter. The
/// box, the paper padding and the work limits cover all the strokes together.
pub fn rasterize_strokes(strokes: &[&[[f32; 2]]], pen_width: f32) -> Result<LineImage, Error> {
    let mut points: Vec<[f32; 2]> = Vec::new();
    for stroke in strokes {
        for p in stroke.iter().copied().filter(|p| p[0].is_finite() && p[1].is_finite()) {
            if points.len() >= MAX_POINTS {
                break;
            }
            points.push(p);
        }
    }
    let bbox = Bbox::of(&points).ok_or_else(|| Error::Ink("The stroke has no points to recognise.".to_string()))?;
    let pen = if pen_width.is_finite() { pen_width.clamp(0.5, 40.0) } else { 2.0 };
    let pad = (PADDING_SHARE * bbox.width().max(bbox.height())).max(PADDING_MIN);
    let span_x = bbox.width() + 2.0 * pad;
    let span_y = bbox.height() + 2.0 * pad;
    if !(span_x.is_finite() && span_y.is_finite()) || span_x <= 0.0 || span_y <= 0.0 {
        return Err(Error::Ink("The stroke is too large to recognise.".to_string()));
    }
    // A few pixels per point keeps a 2 pt pen solid; wider strokes cost more, so the bound holds
    // the longer side at MAX_SIDE.
    let scale = (MAX_SIDE as f32 / span_x.max(span_y)).min(4.0);
    let width = ((span_x * scale).ceil() as isize).clamp(8, MAX_SIDE as isize) as usize;
    let height = ((span_y * scale).ceil() as isize).clamp(8, MAX_SIDE as isize) as usize;
    // Anti-aliasing fades over one pixel, so never let the pen be thinner than that.
    let radius = (pen * scale * 0.5).max(0.5);

    let mut coverage = vec![0.0f32; width * height];
    let map = |p: [f32; 2]| [(p[0] - bbox.min_x + pad) * scale, (p[1] - bbox.min_y + pad) * scale];
    // Each stroke is decimated and mapped on its own, so no segment spans two strokes. A stroke
    // with one point is a click: one round dot.
    let mut segments: Vec<([f32; 2], [f32; 2])> = Vec::new();
    for stroke in strokes {
        let finite: Vec<[f32; 2]> = stroke.iter().copied().filter(|p| p[0].is_finite() && p[1].is_finite()).take(MAX_POINTS).collect();
        let step = finite.len().div_ceil(MAX_SEGMENTS).max(1);
        let kept: Vec<[f32; 2]> = finite.iter().step_by(step).copied().collect();
        if segments.len() >= MAX_SEGMENTS_TOTAL {
            break;
        }
        if kept.len() == 1 {
            let p = map(kept[0]);
            segments.push((p, p));
        } else {
            segments.extend(kept.windows(2).map(|w| (map(w[0]), map(w[1]))));
        }
    }
    let mut work = 0usize;
    for (a, b) in segments {
        let (lo_x, hi_x) = (a[0].min(b[0]) - radius - 1.0, a[0].max(b[0]) + radius + 1.0);
        let (lo_y, hi_y) = (a[1].min(b[1]) - radius - 1.0, a[1].max(b[1]) + radius + 1.0);
        let x0 = (lo_x.floor() as isize).clamp(0, width as isize - 1) as usize;
        let x1 = (hi_x.ceil() as isize).clamp(0, width as isize - 1) as usize;
        let y0 = (lo_y.floor() as isize).clamp(0, height as isize - 1) as usize;
        let y1 = (hi_y.ceil() as isize).clamp(0, height as isize - 1) as usize;
        work = work.saturating_add((x1 - x0 + 1).saturating_mul(y1 - y0 + 1));
        if work > MAX_WORK {
            break;
        }
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = if len2 > 0.0 { (((px - a[0]) * dx + (py - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let (cx, cy) = (a[0] + t * dx, a[1] + t * dy);
                let dist = ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt();
                let cover = (radius + 0.5 - dist).clamp(0.0, 1.0);
                if let Some(c) = coverage.get_mut(y * width + x) {
                    *c = c.max(cover);
                }
            }
        }
    }
    let pixels = coverage.iter().map(|c| (255.0 * (1.0 - c)).round().clamp(0.0, 255.0) as u8).collect();
    Ok(LineImage { width, height, pixels })
}

/// The encoder's input: 384 × 384, three channels (CHW), each value `(grey / 255 - 0.5) / 0.5`.
///
/// The image is stretched to the square input (not letterboxed), as the training pipeline does.
pub fn model_input(image: &LineImage) -> Vec<f32> {
    let resized = resize(image, IMAGE_SIZE, IMAGE_SIZE);
    let mut out = vec![0.0f32; 3 * IMAGE_SIZE * IMAGE_SIZE];
    let plane = IMAGE_SIZE * IMAGE_SIZE;
    for (i, &g) in resized.iter().enumerate() {
        let v = g as f32 / 127.5 - 1.0;
        for c in 0..3 {
            if let Some(slot) = out.get_mut(c * plane + i) {
                *slot = v;
            }
        }
    }
    out
}

/// Bilinear resample of a grey image to `width` × `height`.
fn resize(image: &LineImage, width: usize, height: usize) -> Vec<u8> {
    if image.width == 0 || image.height == 0 {
        return vec![255; width * height];
    }
    let mut out = vec![255u8; width * height];
    let sx = image.width as f32 / width as f32;
    let sy = image.height as f32 / height as f32;
    for y in 0..height {
        let fy = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(image.height - 1);
        let ty = fy - y0 as f32;
        for x in 0..width {
            let fx = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(image.width - 1);
            let tx = fx - x0 as f32;
            let at = |px: usize, py: usize| {
                image.pixels.get(py.min(image.height - 1) * image.width + px.min(image.width - 1)).copied().unwrap_or(255) as f32
            };
            let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
            let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
            let v = top * (1.0 - ty) + bottom * ty;
            if let Some(slot) = out.get_mut(y * width + x) {
                *slot = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rough "L" in sheet points.
    fn stroke() -> Vec<[f32; 2]> {
        vec![[10.0, 10.0], [10.0, 40.0], [40.0, 40.0]]
    }

    #[test]
    fn bbox_covers_the_points_and_ignores_non_finite_ones() {
        let b = Bbox::of(&[[1.0, 2.0], [5.0, 0.0], [f32::NAN, 3.0], [3.0, f32::INFINITY]]).expect("finite points");
        assert_eq!((b.min_x, b.min_y, b.max_x, b.max_y), (1.0, 0.0, 5.0, 2.0));
        assert!(Bbox::of(&[]).is_none());
        assert!(Bbox::of(&[[f32::NAN, f32::NAN]]).is_none());
    }

    #[test]
    fn a_stroke_is_rasterised_with_paper_padding_and_dark_ink() {
        let img = rasterize(&stroke(), 2.0).expect("rasterised");
        assert!(img.width >= 8 && img.height >= 8);
        assert!(img.border_is_paper(), "ink reaches the edge: {}x{}", img.width, img.height);
        assert!(img.darkest() < 40, "no solid ink: darkest {}", img.darkest());
        // The aspect ratio follows the stroke (30 x 30 points, equal padding): near square.
        let ratio = img.width as f32 / img.height as f32;
        assert!((0.9..=1.1).contains(&ratio), "ratio {ratio}");
    }

    /// Two strokes in one entry: both are drawn, and no segment joins them (a joined pair would
    /// put ink between them, where a person wrote nothing).
    #[test]
    fn separate_strokes_are_rasterised_together_without_being_joined() {
        let left = [[0.0f32, 0.0], [0.0, 40.0]];
        let right = [[100.0f32, 0.0], [100.0, 40.0]];
        let img = rasterize_strokes(&[&left, &right], 2.0).expect("rasterised");
        let column_dark = |x: usize| (0..img.height).filter(|y| img.pixel(x, *y).is_some_and(|p| p < 128)).count();
        // 0..40 under the left stroke, 440..480 under the right one (4 px per point, 10 pt pad).
        assert!(column_dark(img.width / 12) > 0, "the left stroke is missing");
        assert!(column_dark(img.width * 11 / 12) > 0, "the right stroke is missing");
        assert_eq!(column_dark(img.width / 2), 0, "the strokes were joined");
    }

    /// The documented shape: a 100-point horizontal stroke with a 2 pt pen is rasterised at 4 px
    /// per point into a stroke about 400 px long and 8 px thick, inside paper padding.
    #[test]
    fn a_stroke_keeps_its_length_and_pen_width_in_pixels() {
        let img = rasterize(&[[0.0, 0.0], [100.0, 0.0]], 2.0).expect("rasterised");
        let mid = img.height / 2;
        let dark_in_row = (0..img.width).filter(|x| img.pixel(*x, mid).is_some_and(|p| p < 128)).count();
        assert!((dark_in_row as i32 - 400).abs() <= 8, "{dark_in_row} px long, expected about 400");
        let column = img.width / 2;
        let dark_in_column = (0..img.height).filter(|y| img.pixel(column, *y).is_some_and(|p| p < 128)).count();
        assert!((dark_in_column as i32 - 8).abs() <= 2, "{dark_in_column} px thick, expected about 8");
        // The image is mostly paper: the ink is a thin line, not a filled block.
        let ink = img.pixels.iter().filter(|p| **p < 128).count();
        let share = ink as f32 / img.pixels.len() as f32;
        assert!((0.03..0.15).contains(&share), "ink covers {share:.3} of the image");
    }

    #[test]
    fn a_wide_stroke_stays_within_the_side_limit() {
        let img = rasterize(&[[0.0, 0.0], [100_000.0, 40.0]], 2.0).expect("rasterised");
        assert!(img.width <= MAX_SIDE && img.height <= MAX_SIDE);
        assert!(img.border_is_paper());
    }

    #[test]
    fn a_single_click_is_a_dot() {
        let img = rasterize(&[[5.0, 5.0]], 2.0).expect("dot");
        assert!(img.darkest() < 200);
        assert!(img.border_is_paper());
    }

    #[test]
    fn model_input_is_normalised_chw_and_keeps_ink_dark() {
        let img = rasterize(&stroke(), 2.0).expect("rasterised");
        let input = model_input(&img);
        assert_eq!(input.len(), 3 * IMAGE_SIZE * IMAGE_SIZE);
        assert!(input.iter().all(|v| (-1.0..=1.0).contains(v)));
        // Paper is +1; the darkest input pixel is negative.
        assert!(input[0] > 0.9);
        let darkest = input.iter().copied().fold(f32::INFINITY, f32::min);
        assert!(darkest < -0.5, "darkest {darkest}");
        // The three channels agree (grey input).
        let plane = IMAGE_SIZE * IMAGE_SIZE;
        assert_eq!(input[..plane], input[plane..2 * plane]);
        assert_eq!(input[..plane], input[2 * plane..]);
    }
}
