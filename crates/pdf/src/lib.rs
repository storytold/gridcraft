//! A tiny, self-contained PDF 1.7 writer for GridCraft's print and PDF export.
//!
//! - The 14 standard fonts (no embedding) with WinAnsiEncoding, plus approximate width tables
//!   ([`text_width`]) so callers can measure, align and wrap text.
//! - Vector drawing: rectangles, lines (solid or dashed), polygons, circles (Bézier), clipping.
//! - RGBA images as RGB image XObjects with a soft mask for alpha (run-length encoded).
//! - Coordinates are points with a **top-left origin** (y grows down); text `y` is the baseline.
//!
//! Streams are written uncompressed (images use the trivial RunLengthDecode filter) so the crate
//! needs no dependencies.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod metrics;

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

/// RGB colour, 0–255 per channel.
pub type Rgb = [u8; 3];

/// The standard 14 fonts minus Symbol and ZapfDingbats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Font {
    #[default]
    Helvetica,
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
}

/// A font family for [`Font::styled`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    #[default]
    Sans,
    Serif,
    Mono,
}

impl Font {
    pub const ALL: [Font; 12] = [
        Font::Helvetica,
        Font::HelveticaBold,
        Font::HelveticaOblique,
        Font::HelveticaBoldOblique,
        Font::TimesRoman,
        Font::TimesBold,
        Font::TimesItalic,
        Font::TimesBoldItalic,
        Font::Courier,
        Font::CourierBold,
        Font::CourierOblique,
        Font::CourierBoldOblique,
    ];

    /// PostScript base font name.
    pub fn base_name(self) -> &'static str {
        match self {
            Font::Helvetica => "Helvetica",
            Font::HelveticaBold => "Helvetica-Bold",
            Font::HelveticaOblique => "Helvetica-Oblique",
            Font::HelveticaBoldOblique => "Helvetica-BoldOblique",
            Font::TimesRoman => "Times-Roman",
            Font::TimesBold => "Times-Bold",
            Font::TimesItalic => "Times-Italic",
            Font::TimesBoldItalic => "Times-BoldItalic",
            Font::Courier => "Courier",
            Font::CourierBold => "Courier-Bold",
            Font::CourierOblique => "Courier-Oblique",
            Font::CourierBoldOblique => "Courier-BoldOblique",
        }
    }

    fn index(self) -> usize {
        Font::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    /// The face of `family` with the given weight and slant.
    pub fn styled(family: Family, bold: bool, italic: bool) -> Font {
        match (family, bold, italic) {
            (Family::Sans, false, false) => Font::Helvetica,
            (Family::Sans, true, false) => Font::HelveticaBold,
            (Family::Sans, false, true) => Font::HelveticaOblique,
            (Family::Sans, true, true) => Font::HelveticaBoldOblique,
            (Family::Serif, false, false) => Font::TimesRoman,
            (Family::Serif, true, false) => Font::TimesBold,
            (Family::Serif, false, true) => Font::TimesItalic,
            (Family::Serif, true, true) => Font::TimesBoldItalic,
            (Family::Mono, false, false) => Font::Courier,
            (Family::Mono, true, false) => Font::CourierBold,
            (Family::Mono, false, true) => Font::CourierOblique,
            (Family::Mono, true, true) => Font::CourierBoldOblique,
        }
    }

    /// Picks a standard family for a font name: serif faces map to Times, monospaced ones to
    /// Courier, everything else to Helvetica.
    pub fn family_for(name: &str) -> Family {
        let n = name.to_ascii_lowercase();
        const SERIF: [&str; 10] = ["times", "cambria", "georgia", "garamond", "serif", "book", "palatino", "baskerville", "tinos", "caladea"];
        const MONO: [&str; 8] = ["courier", "consolas", "mono", "menlo", "lucida console", "cousine", "code", "typewriter"];
        if MONO.iter().any(|m| n.contains(m)) {
            Family::Mono
        } else if SERIF.iter().any(|s| n.contains(s)) && !n.contains("sans") {
            Family::Serif
        } else {
            Family::Sans
        }
    }

    /// Advance width of a character in 1/1000 em.
    pub fn char_width(self, c: char) -> u16 {
        let table: Option<&[u16; 95]> = match self {
            Font::Helvetica | Font::HelveticaOblique => Some(&metrics::HELVETICA),
            Font::HelveticaBold | Font::HelveticaBoldOblique => Some(&metrics::HELVETICA_BOLD),
            Font::TimesRoman | Font::TimesItalic => Some(&metrics::TIMES),
            Font::TimesBold | Font::TimesBoldItalic => Some(&metrics::TIMES_BOLD),
            _ => None,
        };
        let Some(table) = table else { return 600 };
        let ascii = |c: char| -> Option<u16> {
            let i = (c as u32).checked_sub(32)? as usize;
            table.get(i).copied()
        };
        if let Some(w) = ascii(c) {
            return w;
        }
        match c {
            '—' | '…' | '‰' | '™' => 1000,
            _ => metrics::fold(c).and_then(ascii).unwrap_or(match self {
                Font::TimesRoman | Font::TimesItalic | Font::TimesBold | Font::TimesBoldItalic => 500,
                _ => 556,
            }),
        }
    }
}

/// Width of `text` in points when set in `font` at `size` points.
pub fn text_width(font: Font, size: f32, text: &str) -> f32 {
    // u64: a u32 sum overflows for ~8 million characters.
    let units: u64 = text.chars().map(|c| u64::from(font.char_width(c))).sum();
    units as f32 * size / 1000.0
}

/// Unicode → WinAnsiEncoding (CP1252) byte; unmappable characters become `?`.
pub fn win_ansi(c: char) -> u8 {
    let u = c as u32;
    if (0x20..0x7F).contains(&u) || (0xA0..=0xFF).contains(&u) {
        return u as u8;
    }
    match c {
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8A,
        '‹' => 0x8B,
        'Œ' => 0x8C,
        'Ž' => 0x8E,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9A,
        '›' => 0x9B,
        'œ' => 0x9C,
        'ž' => 0x9E,
        'Ÿ' => 0x9F,
        '\t' => b' ',
        _ => b'?',
    }
}

/// A PDF literal string `( … )` for `text` in WinAnsiEncoding.
fn pdf_string(text: &str) -> String {
    let mut s = String::with_capacity(text.len() + 2);
    s.push('(');
    for c in text.chars() {
        let b = win_ansi(c);
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            0x20..=0x7E => s.push(b as char),
            _ => {
                let _ = write!(s, "\\{b:03o}");
            }
        }
    }
    s.push(')');
    s
}

/// A number for a content stream: finite, short.
fn n(v: f32) -> String {
    let v = if v.is_finite() { v.clamp(-1.0e7, 1.0e7) } else { 0.0 };
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Appends `v / 255` with at most three decimals, exactly what `n(v as f32 / 255.0)` gives, but
/// without a float round trip or an allocation per channel.
fn push_unit(s: &mut String, v: u8) {
    let m = (u32::from(v) * 1000 + 127) / 255;
    match m {
        0 => s.push('0'),
        1000 => s.push('1'),
        _ => {
            let _ = write!(s, "0.{m:03}");
            while s.ends_with('0') {
                s.pop();
            }
        }
    }
}

/// Appends a colour operator line without the trailing newline: `r g b rg` (fill) or `r g b RG`.
fn push_rgb(s: &mut String, c: Rgb, stroke: bool) {
    for v in c {
        push_unit(s, v);
        s.push(' ');
    }
    s.push_str(if stroke { "RG" } else { "rg" });
}

/// True when every value is finite (NaN and infinities are rejected).
fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// A positive, finite extent (a width, height or radius).
fn extent(v: f32) -> bool {
    v > 0.0 && v.is_finite()
}

/// A stroke width: negative, NaN and infinite become 0 (the thinnest line).
fn stroke_width(w: f32) -> f32 {
    if w.is_finite() { w.max(0.0) } else { 0.0 }
}

/// The entries of a dash array (`[on, off, …]`, at most 8). A pattern the PDF specification
/// forbids (any non-finite entry, or no entry that survives rounding to a positive number) is
/// replaced by an empty array, i.e. a solid line.
fn dash_values(dash: &[f32]) -> String {
    let d: Vec<f32> = dash.iter().take(8).map(|v| if v.is_finite() { v.max(0.0) } else { f32::NAN }).collect();
    if !finite(&d) || !d.iter().any(|v| (v.min(1.0e7) * 1000.0).round() > 0.0) {
        return String::new();
    }
    d.iter().map(|v| n(*v)).collect::<Vec<_>>().join(" ")
}

struct ImageData {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    /// `None` when every pixel is fully opaque (no soft mask is written).
    alpha: Option<Vec<u8>>,
    /// Hash of all of the above, to find duplicates quickly; equality still compares the bytes.
    hash: u64,
}

impl ImageData {
    fn new(width: u32, height: u32, rgb: Vec<u8>, alpha: Option<Vec<u8>>) -> ImageData {
        let mut h = DefaultHasher::new();
        width.hash(&mut h);
        height.hash(&mut h);
        rgb.hash(&mut h);
        alpha.hash(&mut h);
        ImageData { width, height, rgb, alpha, hash: h.finish() }
    }

    /// Same pixels (a hash match alone is never trusted).
    fn same(&self, o: &ImageData) -> bool {
        self.hash == o.hash && self.width == o.width && self.height == o.height && self.rgb == o.rgb && self.alpha == o.alpha
    }
}

/// The part of the graphics state the writer tracks so that a style operator is only emitted
/// when it changes. `None` = not set yet on this page (or since the last restore that undid it).
#[derive(Clone, Debug, Default, PartialEq)]
struct Style {
    fill: Option<Rgb>,
    stroke: Option<Rgb>,
    /// Formatted line width.
    width: Option<String>,
    /// Formatted dash array contents.
    dash: Option<String>,
    /// Line join; PDF's initial value is 0 (miter).
    join: u8,
}

/// A page under construction. Obtain one with [`PdfDoc::add_page`].
pub struct Page {
    width: f32,
    height: f32,
    content: String,
    fonts: [bool; 12],
    images: Vec<ImageData>,
    /// Style operators already in effect at the end of `content`.
    style: Style,
    /// One entry per accepted [`Page::save`], restored by [`Page::restore`].
    saved: Vec<Style>,
    /// `save` calls refused at the nesting limit; the same number of `restore` calls are ignored.
    refused: u32,
}

/// Most bytes of content per page before further drawing is dropped (keeps hostile inputs from
/// exhausting memory).
const MAX_PAGE_CONTENT: usize = 64 * 1024 * 1024;
/// Largest image (pixels) embedded.
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
/// Deepest `q` nesting a PDF consumer must support (the PDF specification's implementation limits).
const MAX_SAVE_DEPTH: usize = 28;
/// Line join styles, as the `j` operator takes them.
const JOIN_MITER: u8 = 0;
const JOIN_ROUND: u8 = 1;

impl Page {
    pub fn width(&self) -> f32 {
        self.width
    }
    pub fn height(&self) -> f32 {
        self.height
    }
    fn y(&self, y: f32) -> f32 {
        self.height - y
    }
    fn full(&self) -> bool {
        self.content.len() > MAX_PAGE_CONTENT
    }
    fn op(&mut self, s: &str) {
        if !self.full() {
            self.content.push_str(s);
            self.content.push('\n');
        }
    }

    /// Appends a fill colour operator to `s` unless that colour is already in effect.
    fn set_fill(&mut self, s: &mut String, c: Rgb) {
        if self.style.fill != Some(c) {
            push_rgb(s, c, false);
            s.push('\n');
            self.style.fill = Some(c);
        }
    }

    /// Appends the stroke colour, width, dash and join operators to `s` that differ from the
    /// style already in effect.
    fn set_stroke(&mut self, s: &mut String, c: Rgb, width: f32, dash: &str, join: u8) {
        if self.style.stroke != Some(c) {
            push_rgb(s, c, true);
            s.push('\n');
            self.style.stroke = Some(c);
        }
        let w = n(stroke_width(width));
        if self.style.width.as_deref() != Some(w.as_str()) {
            let _ = writeln!(s, "{w} w");
            self.style.width = Some(w);
        }
        if self.style.dash.as_deref() != Some(dash) {
            let _ = writeln!(s, "[{dash}] 0 d");
            self.style.dash = Some(dash.to_string());
        }
        if self.style.join != join {
            let _ = writeln!(s, "{join} j");
            self.style.join = join;
        }
    }

    /// Saves the graphics state (pair with [`Page::restore`]). Nesting is limited to 28 levels, the
    /// depth every PDF consumer must support; a `save` beyond that is ignored, and so is the
    /// `restore` that would have matched it.
    pub fn save(&mut self) {
        if self.saved.len() >= MAX_SAVE_DEPTH {
            self.refused = self.refused.saturating_add(1);
            return;
        }
        self.saved.push(self.style.clone());
        self.content.push_str("q\n");
    }
    /// Restores the graphics state saved by [`Page::save`]; extra calls are ignored.
    pub fn restore(&mut self) {
        if self.refused > 0 {
            self.refused -= 1;
        } else if let Some(style) = self.saved.pop() {
            self.style = style;
            self.content.push_str("Q\n");
        }
    }
    /// Intersects the clip with a rectangle (until the next [`Page::restore`]).
    pub fn clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let s = format!("{} {} {} {} re W n", n(x), n(self.y(y + h)), n(w.max(0.0)), n(h.max(0.0)));
        self.op(&s);
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        if !(extent(w) && extent(h) && finite(&[x, y])) || self.full() {
            return;
        }
        let mut s = String::new();
        self.set_fill(&mut s, color);
        let _ = write!(s, "{} {} {} {} re f", n(x), n(self.y(y + h)), n(w), n(h));
        self.op(&s);
    }

    pub fn rect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, color: Rgb) {
        if !finite(&[x, y, w, h]) || self.full() {
            return;
        }
        let mut s = String::new();
        self.set_stroke(&mut s, color, width, "", JOIN_MITER);
        let _ = write!(s, "{} {} {} {} re S", n(x), n(self.y(y + h)), n(w), n(h));
        self.op(&s);
    }

    pub fn stroke_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgb) {
        self.stroke_line_dashed(x0, y0, x1, y1, width, color, &[]);
    }

    /// A line with a dash pattern (`[on, off, …]` in points; empty = solid).
    pub fn stroke_line_dashed(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgb, dash: &[f32]) {
        if !finite(&[x0, y0, x1, y1]) || self.full() {
            return;
        }
        let mut s = String::new();
        self.set_stroke(&mut s, color, width, &dash_values(dash), JOIN_MITER);
        let _ = write!(s, "{} {} m {} {} l S", n(x0), n(self.y(y0)), n(x1), n(self.y(y1)));
        self.op(&s);
    }

    /// An open polyline (round joins).
    pub fn polyline(&mut self, pts: &[[f32; 2]], width: f32, color: Rgb, dash: &[f32]) {
        if pts.len() < 2 || !pts.iter().all(|p| finite(p)) || self.full() {
            return;
        }
        let mut s = String::new();
        self.set_stroke(&mut s, color, width, &dash_values(dash), JOIN_ROUND);
        self.path(&mut s, pts, false);
        s.push_str(" S");
        self.op(&s);
    }

    fn path(&self, s: &mut String, pts: &[[f32; 2]], close: bool) {
        for (i, p) in pts.iter().enumerate() {
            let _ = write!(s, "{} {} {} ", n(p[0]), n(self.y(p[1])), if i == 0 { "m" } else { "l" });
        }
        if close {
            s.push('h');
        }
    }

    /// A closed polygon, filled and/or stroked (round joins).
    pub fn polygon(&mut self, pts: &[[f32; 2]], fill: Option<Rgb>, stroke: Option<(Rgb, f32)>) {
        if pts.len() < 2 || (fill.is_none() && stroke.is_none()) || !pts.iter().all(|p| finite(p)) || self.full() {
            return;
        }
        let mut s = String::new();
        if let Some(f) = fill {
            self.set_fill(&mut s, f);
        }
        if let Some((c, w)) = stroke {
            self.set_stroke(&mut s, c, w, "", JOIN_ROUND);
        }
        self.path(&mut s, pts, true);
        s.push_str(match (fill.is_some(), stroke.is_some()) {
            (true, true) => " B",
            (true, false) => " f",
            _ => " S",
        });
        self.op(&s);
    }

    /// A circle approximated by four cubic Béziers.
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: Option<Rgb>, stroke: Option<(Rgb, f32)>) {
        if !extent(r) || !finite(&[cx, cy]) || (fill.is_none() && stroke.is_none()) || self.full() {
            return;
        }
        const K: f32 = 0.552_284_8;
        let k = r * K;
        let y = self.y(cy);
        let mut s = String::new();
        if let Some(f) = fill {
            self.set_fill(&mut s, f);
        }
        if let Some((c, w)) = stroke {
            self.set_stroke(&mut s, c, w, "", JOIN_MITER);
        }
        let _ = write!(
            s,
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h",
            n(cx + r),
            n(y),
            n(cx + r),
            n(y + k),
            n(cx + k),
            n(y + r),
            n(cx),
            n(y + r),
            n(cx - k),
            n(y + r),
            n(cx - r),
            n(y + k),
            n(cx - r),
            n(y),
            n(cx - r),
            n(y - k),
            n(cx - k),
            n(y - r),
            n(cx),
            n(y - r),
            n(cx + k),
            n(y - r),
            n(cx + r),
            n(y - k),
            n(cx + r),
            n(y)
        );
        s.push_str(match (fill.is_some(), stroke.is_some()) {
            (true, true) => " B",
            (true, false) => " f",
            _ => " S",
        });
        self.op(&s);
    }

    /// Text with its baseline starting at `(x, y)`.
    pub fn text(&mut self, x: f32, y: f32, size: f32, font: Font, color: Rgb, text: &str) {
        self.text_rotated(x, y, size, font, color, text, 0.0);
    }

    /// Text rotated by `angle` radians (clockwise on the page, like the y-down coordinate
    /// system) about its baseline origin.
    pub fn text_rotated(&mut self, x: f32, y: f32, size: f32, font: Font, color: Rgb, text: &str, angle: f32) {
        if text.is_empty() || !extent(size) || !finite(&[x, y]) {
            return;
        }
        let fi = font.index();
        if let Some(f) = self.fonts.get_mut(fi) {
            *f = true;
        }
        if self.full() {
            return;
        }
        let (sn, cs) = if angle.is_finite() { angle.sin_cos() } else { (0.0, 1.0) };
        // The colour goes in front of the text object, so it is tracked like any other style.
        let mut colour = String::new();
        self.set_fill(&mut colour, color);
        // Clockwise in y-down = counter-clockwise negated in PDF's y-up space.
        let s = format!(
            "{colour}BT\n/F{} {} Tf\n{} {} {} {} {} {} Tm\n{} Tj\nET",
            fi + 1,
            n(size.min(10_000.0)),
            n(cs),
            n(-sn),
            n(sn),
            n(cs),
            n(x),
            n(self.y(y)),
            pdf_string(text)
        );
        self.op(&s);
    }

    /// Draws an RGBA image (`pw`×`ph` pixels, row-major, straight alpha) into the box
    /// `(x, y, w, h)`. Malformed or oversized buffers are ignored.
    pub fn image_rgba(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: &[u8], pw: u32, ph: u32) {
        let px = pw as u64 * ph as u64;
        if pw == 0 || ph == 0 || px > MAX_IMAGE_PIXELS || (rgba.len() as u64) < px * 4 || !(extent(w) && extent(h) && finite(&[x, y])) {
            return;
        }
        // The image is drawn inside its own q/Q, which counts towards the nesting limit.
        if self.saved.len() >= MAX_SAVE_DEPTH {
            return;
        }
        let px = px as usize;
        let mut rgb = Vec::with_capacity(px * 3);
        // The alpha plane is only allocated once a non-opaque pixel shows up.
        let mut alpha: Option<Vec<u8>> = None;
        for (i, p) in rgba.as_chunks::<4>().0.iter().take(px).enumerate() {
            rgb.extend_from_slice(&[p[0], p[1], p[2]]);
            match &mut alpha {
                Some(a) => a.push(p[3]),
                None if p[3] != 255 => {
                    let mut a = Vec::with_capacity(px);
                    a.resize(i, 255);
                    a.push(p[3]);
                    alpha = Some(a);
                }
                None => {}
            }
        }
        let img = ImageData::new(pw, ph, rgb, alpha);
        // The same picture drawn again on this page shares one XObject.
        let idx = match self.images.iter().position(|i| i.same(&img)) {
            Some(i) => i,
            None => {
                self.images.push(img);
                self.images.len() - 1
            }
        };
        let s = format!("q {} 0 0 {} {} {} cm /Im{} Do Q", n(w), n(h), n(x), n(self.y(y + h)), idx + 1);
        self.op(&s);
    }
}

/// Document metadata.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub title: String,
    pub author: String,
    pub creator: String,
}

/// A PDF document under construction.
#[derive(Default)]
pub struct PdfDoc {
    pages: Vec<Page>,
    pub info: Info,
}

/// PackBits-style run-length encoding (PDF `RunLengthDecode`).
fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let mut run = 1;
        while i + run < data.len() && run < 128 && data[i + run] == b {
            run += 1;
        }
        if run >= 2 {
            out.push((257 - run) as u8);
            out.push(b);
            i += run;
            continue;
        }
        // Literal run until the next repeat of 3+ or 128 bytes.
        let start = i;
        let mut len = 0;
        while i < data.len() && len < 128 {
            if i + 2 < data.len() && data[i] == data[i + 1] && data[i] == data[i + 2] {
                break;
            }
            i += 1;
            len += 1;
        }
        if len == 0 {
            // Next is a repeat; loop will handle it.
            continue;
        }
        out.push((len - 1) as u8);
        out.extend_from_slice(&data[start..start + len]);
    }
    out.push(128);
    out
}

/// Longest Info string written, in characters; longer values are cut on a character boundary.
const MAX_INFO_CHARS: usize = 1024;

/// A PDF text string (document Info entries) for `text`: a literal `( … )` for printable ASCII,
/// otherwise UTF-16BE with a byte-order mark as a hex string (ISO 32000-1 7.9.2.2).
fn pdf_text_string(text: &str) -> String {
    let text: String = text.chars().take(MAX_INFO_CHARS).collect();
    if text.chars().all(|c| matches!(c, ' '..='~')) {
        return pdf_string(&text);
    }
    let mut s = String::with_capacity(text.len() * 4 + 6);
    s.push_str("<FEFF");
    for unit in text.encode_utf16() {
        let _ = write!(s, "{unit:04X}");
    }
    s.push('>');
    s
}

/// Starts object `id` at the current end of `out` and records its offset for the xref table.
fn begin_obj(out: &mut Vec<u8>, offsets: &mut Vec<usize>, id: u32) {
    let i = id as usize;
    if offsets.len() <= i {
        offsets.resize(i + 1, 0);
    }
    if let Some(slot) = offsets.get_mut(i) {
        *slot = out.len();
    }
    let _ = writeln!(Utf8(out), "{id} 0 obj");
}

/// Writes `id 0 obj <body> endobj`.
fn dict_obj(out: &mut Vec<u8>, offsets: &mut Vec<usize>, id: u32, body: &str) {
    begin_obj(out, offsets, id);
    out.extend_from_slice(body.as_bytes());
    out.extend_from_slice(b"\nendobj\n");
}

/// Writes a stream object: `dict` (the dictionary entries without `<<`, `>>` and `/Length`, may be
/// empty) and `data` as the stream body.
fn stream_obj(out: &mut Vec<u8>, offsets: &mut Vec<usize>, id: u32, dict: &str, data: &[u8]) {
    begin_obj(out, offsets, id);
    out.extend_from_slice(b"<<");
    if !dict.is_empty() {
        out.push(b' ');
        out.extend_from_slice(dict.as_bytes());
    }
    let _ = write!(Utf8(out), " /Length {} >>\nstream\n", data.len());
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream\nendobj\n");
}

/// Lets `write!` append text to a byte buffer.
struct Utf8<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for Utf8<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

impl PdfDoc {
    pub fn new() -> PdfDoc {
        PdfDoc::default()
    }

    /// Adds a page of `width_pt` × `height_pt` points and returns it for drawing.
    pub fn add_page(&mut self, width_pt: f32, height_pt: f32) -> &mut Page {
        let width = if width_pt.is_finite() { width_pt.clamp(3.0, 14_400.0) } else { 612.0 };
        let height = if height_pt.is_finite() { height_pt.clamp(3.0, 14_400.0) } else { 792.0 };
        self.pages.push(Page {
            width,
            height,
            content: String::new(),
            fonts: [false; 12],
            images: Vec::new(),
            style: Style::default(),
            saved: Vec::new(),
            refused: 0,
        });
        let i = self.pages.len() - 1;
        &mut self.pages[i]
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The page at `i`, to keep drawing on it.
    pub fn page_mut(&mut self, i: usize) -> Option<&mut Page> {
        self.pages.get_mut(i)
    }

    /// Serializes the document. A document without pages gets one blank Letter page.
    ///
    /// Every object is written straight into the output buffer (nothing is staged in an
    /// intermediate list of bodies); only the xref table needs the offset of each object id, and
    /// the ids are contiguous from 1. Objects are therefore in the file in the order they are
    /// numbered during writing (fonts, then per page its new images, content stream and page
    /// dictionary), with the catalog, page tree and info dictionary last.
    pub fn finish(mut self) -> Vec<u8> {
        if self.pages.is_empty() {
            self.add_page(612.0, 792.0);
        }
        for page in &mut self.pages {
            page.refused = 0;
            while !page.saved.is_empty() {
                page.restore();
            }
        }
        // Object numbering: 1 catalog, 2 page tree, 3 info, then fonts, then per page.
        let mut used = [false; 12];
        for p in &self.pages {
            for (u, f) in used.iter_mut().zip(p.fonts.iter()) {
                *u |= *f;
            }
        }
        let mut next = 4u32;
        let mut font_ids = [0u32; 12];
        for (i, u) in used.iter().enumerate() {
            if *u {
                font_ids[i] = next;
                next += 1;
            }
        }
        // Page contents are moved out so each one is freed as soon as it is in the output.
        let contents: Vec<String> = self.pages.iter_mut().map(|p| std::mem::take(&mut p.content)).collect();
        let reserve = contents.iter().fold(4096usize, |acc, c| acc.saturating_add(c.len()).saturating_add(1024));
        let mut out: Vec<u8> = Vec::with_capacity(reserve);
        out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        let mut offsets: Vec<usize> = vec![0; 4];
        let mut kids = Vec::new();
        let mut font_dict = String::new();
        for (i, id) in font_ids.iter().enumerate() {
            if *id != 0 {
                let _ = write!(font_dict, "/F{} {} 0 R ", i + 1, id);
                let name = Font::ALL.get(i).copied().unwrap_or_default().base_name();
                let enc = if name.starts_with("Symbol") { "" } else { " /Encoding /WinAnsiEncoding" };
                dict_obj(&mut out, &mut offsets, *id, &format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name}{enc} >>"));
            }
        }
        // Images already written, by content hash: (page, image, object id). A hash match is
        // confirmed by comparing the pixels before an object is shared.
        let mut written: HashMap<u64, Vec<(usize, usize, u32)>> = HashMap::new();
        for (pi, (page, content)) in self.pages.iter().zip(contents).enumerate() {
            let mut xobjects = String::new();
            for (k, img) in page.images.iter().enumerate() {
                let known = written.get(&img.hash).and_then(|c| {
                    c.iter().find_map(|(p, i, id)| {
                        let other = self.pages.get(*p)?.images.get(*i)?;
                        img.same(other).then_some(*id)
                    })
                });
                let img_id = match known {
                    Some(id) => id,
                    None => {
                        let img_id = next;
                        next += 1;
                        let mut dict = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /BitsPerComponent 8", img.width, img.height);
                        let smask_id = img.alpha.as_ref().map(|_| {
                            let id = next;
                            next += 1;
                            id
                        });
                        dict.push_str(" /ColorSpace /DeviceRGB");
                        if let Some(id) = smask_id {
                            let _ = write!(dict, " /SMask {id} 0 R");
                        }
                        dict.push_str(" /Filter /RunLengthDecode");
                        stream_obj(&mut out, &mut offsets, img_id, &dict, &run_length(&img.rgb));
                        if let (Some(id), Some(a)) = (smask_id, &img.alpha) {
                            let dict = format!(
                                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /RunLengthDecode",
                                img.width, img.height
                            );
                            stream_obj(&mut out, &mut offsets, id, &dict, &run_length(a));
                        }
                        written.entry(img.hash).or_default().push((pi, k, img_id));
                        img_id
                    }
                };
                let _ = write!(xobjects, "/Im{} {} 0 R ", k + 1, img_id);
            }
            let content_id = next;
            let page_id = next + 1;
            next += 2;
            stream_obj(&mut out, &mut offsets, content_id, "", content.as_bytes());
            drop(content);
            let xo = if xobjects.is_empty() { String::new() } else { format!(" /XObject << {xobjects}>>") };
            dict_obj(
                &mut out,
                &mut offsets,
                page_id,
                &format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {}>>{} >> /Contents {} 0 R >>",
                    n(page.width),
                    n(page.height),
                    font_dict,
                    xo,
                    content_id
                ),
            );
            kids.push(page_id);
        }
        dict_obj(&mut out, &mut offsets, 1, "<< /Type /Catalog /Pages 2 0 R >>");
        let kids_s: Vec<String> = kids.iter().map(|k| format!("{k} 0 R")).collect();
        dict_obj(&mut out, &mut offsets, 2, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids_s.join(" "), kids.len()));
        let creator = if self.info.creator.is_empty() { "GridCraft" } else { &self.info.creator };
        let mut info = format!("<< /Producer {} /Creator {}", pdf_text_string("GridCraft PDF"), pdf_text_string(creator));
        if !self.info.title.is_empty() {
            let _ = write!(info, " /Title {}", pdf_text_string(&self.info.title));
        }
        if !self.info.author.is_empty() {
            let _ = write!(info, " /Author {}", pdf_text_string(&self.info.author));
        }
        info.push_str(" >>");
        dict_obj(&mut out, &mut offsets, 3, &info);

        let xref = out.len();
        let _ = write!(Utf8(&mut out), "xref\n0 {}\n0000000000 65535 f \n", next);
        for off in offsets.iter().skip(1) {
            let _ = writeln!(Utf8(&mut out), "{off:010} 00000 n ");
        }
        let _ = write!(Utf8(&mut out), "trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{}\n%%EOF\n", next, xref);
        out
    }
}

#[cfg(test)]
mod tests;
