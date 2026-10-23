//! Cell styles (font, fill, borders, alignment, number format, protection), interned per
//! workbook.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StyleId(pub u32);

impl StyleId {
    pub const DEFAULT: StyleId = StyleId(0);
}

/// A colour: explicit RGB, a theme slot with tint, or automatic (black text / no fill).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Color {
    #[default]
    Auto,
    /// 0xRRGGBB
    Rgb(u32),
    /// Theme index (0 = Background 1 / lt1, 1 = Text 1 / dk1, 2 = lt2, 3 = dk2, 4–9 = Accent 1–6,
    /// 10 hyperlink, 11 followed) and tint in thousandths (-1000..=1000).
    Theme(u8, i16),
}

impl Color {
    pub fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::Rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }
    pub fn from_hex(s: &str) -> Option<Color> {
        let h = s.trim().trim_start_matches('#');
        let h = if h.len() == 8 { h.get(2..)? } else { h };
        if h.len() != 6 {
            return None;
        }
        u32::from_str_radix(h, 16).ok().map(Color::Rgb)
    }
    /// Resolved RGB, `None` for automatic.
    pub fn resolve(&self, theme: &Theme) -> Option<[u8; 3]> {
        match *self {
            Color::Auto => None,
            Color::Rgb(v) => Some([(v >> 16) as u8, (v >> 8) as u8, v as u8]),
            Color::Theme(i, tint) => {
                let base = theme.colors.get(i as usize).copied().unwrap_or(0);
                Some(apply_tint([(base >> 16) as u8, (base >> 8) as u8, base as u8], tint as f64 / 1000.0))
            }
        }
    }
    pub fn hex(&self, theme: &Theme) -> Option<String> {
        self.resolve(theme).map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}"))
    }
}

/// Tint as in ECMA-376: lighten towards white (positive) or darken (negative) in HSL luminance.
pub fn apply_tint(rgb: [u8; 3], tint: f64) -> [u8; 3] {
    if tint == 0.0 {
        return rgb;
    }
    let (h, s, l) = rgb_to_hsl(rgb);
    let l = if tint < 0.0 { l * (1.0 + tint) } else { l * (1.0 - tint) + tint };
    hsl_to_rgb(h, s, l.clamp(0.0, 1.0))
}

fn rgb_to_hsl([r, g, b]: [u8; 3]) -> (f64, f64, f64) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [u8; 3] {
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return [v, v, v];
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [(hue(h + 1.0 / 3.0) * 255.0).round() as u8, (hue(h) * 255.0).round() as u8, (hue(h - 1.0 / 3.0) * 255.0).round() as u8]
}

/// Workbook theme: 12 colours and the heading/body fonts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    /// lt1, dk1, lt2, dk2, accent1..6, hlink, folHlink (0xRRGGBB).
    pub colors: [u32; 12],
    pub major_font: String,
    pub minor_font: String,
    /// The heading font's East Asian typeface (`<a:ea>`); empty when the file did not set one.
    #[serde(default)]
    pub major_font_ea: String,
    /// The body font's East Asian typeface (`<a:ea>`); empty when the file did not set one.
    #[serde(default)]
    pub minor_font_ea: String,
    /// The heading font's complex-script typeface (`<a:cs>`); empty when the file did not set one.
    #[serde(default)]
    pub major_font_cs: String,
    /// The body font's complex-script typeface (`<a:cs>`); empty when the file did not set one.
    #[serde(default)]
    pub minor_font_cs: String,
    /// The heading font's per-script typefaces (`<a:font script="Arab" typeface="…"/>`), as
    /// (script, typeface) in file order.
    #[serde(default)]
    pub major_font_scripts: Vec<(String, String)>,
    /// The body font's per-script typefaces, as (script, typeface) in file order.
    #[serde(default)]
    pub minor_font_scripts: Vec<(String, String)>,
}

impl Default for Theme {
    /// GridCraft's own default theme ("Craft"): original palette. The East Asian and
    /// complex-script typefaces stay empty: the file did not set one.
    fn default() -> Self {
        Theme {
            name: "Craft".into(),
            colors: [0xFFFFFF, 0x000000, 0xE8E8E8, 0x0E2841, 0x156082, 0xE97132, 0x196B24, 0x0F9ED5, 0xA02B93, 0x4EA72E, 0x467886, 0x96607D],
            major_font: crate::DEFAULT_FONT.into(),
            minor_font: crate::DEFAULT_FONT.into(),
            major_font_ea: String::new(),
            minor_font_ea: String::new(),
            major_font_cs: String::new(),
            minor_font_cs: String::new(),
            major_font_scripts: Vec::new(),
            minor_font_scripts: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    SingleAccounting,
    DoubleAccounting,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VertAlign {
    #[default]
    Baseline,
    Superscript,
    Subscript,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Font {
    pub name: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: Underline,
    pub strike: bool,
    pub color: Color,
    pub vert: VertAlign,
    /// Windows character set the font was chosen for (178 = Arabic); kept so Excel substitutes a font for the same script.
    #[serde(default)]
    pub charset: Option<u8>,
    /// Font family class (1 roman, 2 swiss, 3 modern…); `None` is swiss, the default.
    #[serde(default)]
    pub family: Option<u8>,
}

impl Eq for Font {}
impl Hash for Font {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.name.hash(h);
        self.size.to_bits().hash(h);
        (self.bold, self.italic, self.underline, self.strike, self.color, self.vert, self.charset, self.family).hash(h);
    }
}

impl Default for Font {
    fn default() -> Self {
        Font {
            name: crate::DEFAULT_FONT.into(),
            size: crate::DEFAULT_FONT_SIZE,
            bold: false,
            italic: false,
            underline: Underline::None,
            strike: false,
            color: Color::Auto,
            vert: VertAlign::Baseline,
            charset: None,
            family: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PatternType {
    #[default]
    None,
    Solid,
    Gray50,
    Gray75,
    Gray25,
    Gray125,
    Gray0625,
    DarkHorizontal,
    DarkVertical,
    DarkDown,
    DarkUp,
    DarkGrid,
    DarkTrellis,
    LightHorizontal,
    LightVertical,
    LightDown,
    LightUp,
    LightGrid,
    LightTrellis,
}

/// A gradient stop: `position` is 0..=1 across the gradient.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    pub position: f64,
    pub color: Color,
}

impl Eq for GradientStop {}

impl Hash for GradientStop {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.position.to_bits().hash(h);
        self.color.hash(h);
    }
}

/// A linear or path gradient. Positions and convergence values are clamped to 0..=1;
/// linear degrees are normalised to 0..360. Degrees follow SpreadsheetML: 0 = left-to-right,
/// 90 = top-to-bottom, measured clockwise from the horizontal axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum GradientKind {
    Linear { degree: f64 },
    Path { left: f64, right: f64, top: f64, bottom: f64 },
}

impl Eq for GradientKind {}

impl Hash for GradientKind {
    fn hash<H: Hasher>(&self, h: &mut H) {
        match self {
            GradientKind::Linear { degree } => {
                0u8.hash(h);
                degree.to_bits().hash(h);
            }
            GradientKind::Path { left, right, top, bottom } => {
                1u8.hash(h);
                left.to_bits().hash(h);
                right.to_bits().hash(h);
                top.to_bits().hash(h);
                bottom.to_bits().hash(h);
            }
        }
    }
}

impl Default for GradientKind {
    fn default() -> Self {
        GradientKind::Linear { degree: 0.0 }
    }
}

impl GradientKind {
    fn normalized(self) -> GradientKind {
        match self {
            GradientKind::Linear { degree } => {
                // `+ 0.0` turns `-0.0` into `+0.0`: the two compare equal but hash differently.
                GradientKind::Linear { degree: if degree.is_finite() { degree.rem_euclid(360.0) + 0.0 } else { 0.0 } }
            }
            GradientKind::Path { mut left, mut right, mut top, mut bottom } => {
                let clamp = |v: f64| if v.is_finite() { v.clamp(0.0, 1.0) + 0.0 } else { 0.5 };
                left = clamp(left);
                right = clamp(right);
                top = clamp(top);
                bottom = clamp(bottom);
                if left > right {
                    std::mem::swap(&mut left, &mut right);
                }
                if top > bottom {
                    std::mem::swap(&mut top, &mut bottom);
                }
                GradientKind::Path { left, right, top, bottom }
            }
        }
    }
}

/// A cell gradient fill. Excel writes these as `<gradientFill>` and the UI exposes one or two
/// endpoint colours, but the model keeps every stop so imported files round-trip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientFill {
    pub kind: GradientKind,
    pub stops: Vec<GradientStop>,
}

impl Eq for GradientFill {}

impl Hash for GradientFill {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.kind.hash(h);
        self.stops.hash(h);
    }
}

impl GradientFill {
    pub fn new(kind: GradientKind, stops: Vec<GradientStop>) -> GradientFill {
        let mut stops: Vec<GradientStop> = stops
            .into_iter()
            .filter(|s| s.position.is_finite())
            // `+ 0.0` normalises `-0.0`: equality treats them as equal, so `Hash` must too.
            .map(|s| GradientStop { position: s.position.clamp(0.0, 1.0) + 0.0, color: s.color })
            .collect();
        stops.sort_by(|a, b| a.position.partial_cmp(&b.position).unwrap_or(std::cmp::Ordering::Equal));
        if stops.is_empty() {
            stops.push(GradientStop { position: 0.0, color: Color::Auto });
        }
        GradientFill { kind: kind.normalized(), stops }
    }

    pub fn linear(degree: f64, stops: Vec<GradientStop>) -> GradientFill {
        GradientFill::new(GradientKind::Linear { degree }, stops)
    }

    pub fn path(left: f64, right: f64, top: f64, bottom: f64, stops: Vec<GradientStop>) -> GradientFill {
        GradientFill::new(GradientKind::Path { left, right, top, bottom }, stops)
    }

    pub fn is_linear(&self) -> bool {
        matches!(self.kind, GradientKind::Linear { .. })
    }

    /// Resolved stops in position order, with automatic colours as black.
    pub fn resolved_stops(&self, theme: &Theme) -> Vec<(f64, [u8; 3])> {
        self.stops.iter().map(|s| (s.position, s.color.resolve(theme).unwrap_or([0, 0, 0]))).collect()
    }

    /// Samples the gradient at normalised cell coordinates (`u` right, `v` down).
    pub fn sample(&self, u: f64, v: f64, theme: &Theme) -> Option<[u8; 3]> {
        let stops = self.resolved_stops(theme);
        self.sample_resolved(&stops, u, v)
    }

    /// Samples against stops already resolved for the theme. Rendering paths call this with one
    /// `resolved_stops` per cell instead of resolving (and allocating) for every vertex.
    pub fn sample_resolved(&self, stops: &[(f64, [u8; 3])], u: f64, v: f64) -> Option<[u8; 3]> {
        let first = stops.first()?;
        let last = stops.last()?;
        let t = self.position(u, v);
        if t <= first.0 {
            return Some(first.1);
        }
        if t >= last.0 {
            return Some(last.1);
        }
        for pair in stops.windows(2) {
            let (a, ca) = pair[0];
            let (b, cb) = pair[1];
            if t >= a && t <= b {
                let f = if b > a { (t - a) / (b - a) } else { 0.0 };
                return Some([
                    (ca[0] as f64 + (cb[0] as f64 - ca[0] as f64) * f).round() as u8,
                    (ca[1] as f64 + (cb[1] as f64 - ca[1] as f64) * f).round() as u8,
                    (ca[2] as f64 + (cb[2] as f64 - ca[2] as f64) * f).round() as u8,
                ]);
            }
        }
        Some(last.1)
    }

    /// The gradient position (0..=1) at normalised cell coordinates.
    pub fn position(&self, u: f64, v: f64) -> f64 {
        let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        match self.kind {
            GradientKind::Linear { degree } => {
                let angle = degree.to_radians();
                // SpreadsheetML measures `degree` clockwise from the horizontal axis, with v
                // down: 0 = left-to-right, 90 = top-to-bottom.
                let (dx, dy) = (angle.cos(), angle.sin());
                let lo = dx.min(0.0) + dy.min(0.0);
                let hi = dx.max(0.0) + dy.max(0.0);
                if hi > lo {
                    let t = ((u * dx + v * dy - lo) / (hi - lo)).clamp(0.0, 1.0);
                    // sin/cos of right angles leave a tiny epsilon at the endpoints.
                    if t < 1e-12 {
                        0.0
                    } else if 1.0 - t < 1e-12 {
                        1.0
                    } else {
                        t
                    }
                } else {
                    0.0
                }
            }
            GradientKind::Path { left, right, top, bottom } => {
                let edge = |value: f64, lo: f64, hi: f64| {
                    if value < lo {
                        (lo - value) / lo.max(1e-9)
                    } else if value > hi {
                        (value - hi) / (1.0 - hi).max(1e-9)
                    } else {
                        0.0
                    }
                };
                edge(u, left, right).max(edge(v, top, bottom)).clamp(0.0, 1.0)
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fill {
    pub pattern: PatternType,
    /// Pattern foreground (the solid colour for `Solid`).
    pub fg: Color,
    pub bg: Color,
    /// When set, this is a gradient fill and `pattern` is `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient: Option<GradientFill>,
}

impl Fill {
    pub fn solid(c: Color) -> Fill {
        Fill { pattern: PatternType::Solid, fg: c, bg: Color::Auto, gradient: None }
    }

    pub fn gradient(kind: GradientKind, stops: Vec<GradientStop>) -> Fill {
        Fill { pattern: PatternType::None, fg: Color::Auto, bg: Color::Auto, gradient: Some(GradientFill::new(kind, stops)) }
    }

    pub fn is_set(&self) -> bool {
        self.gradient.is_some() || self.pattern != PatternType::None
    }

    pub fn is_gradient(&self) -> bool {
        self.gradient.is_some()
    }

    /// The colour a swatch or non-gradient fallback (HTML export) should show: the first stop for
    /// a gradient, the foreground for a pattern/solid, and `None` for no fill.
    pub fn preview_color(&self, theme: &Theme) -> Option<[u8; 3]> {
        if let Some(g) = &self.gradient {
            g.stops.first().and_then(|s| s.color.resolve(theme))
        } else if self.pattern != PatternType::None {
            self.fg.resolve(theme)
        } else {
            None
        }
    }

    /// Repairs values that arrived through JSON: clamps and orders gradient stops and keeps a
    /// gradient and a pattern from both being active.
    pub fn normalize(&mut self) {
        if let Some(g) = self.gradient.take() {
            self.gradient = Some(GradientFill::new(g.kind, g.stops));
            self.pattern = PatternType::None;
            self.fg = Color::Auto;
            self.bg = Color::Auto;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BorderStyle {
    #[default]
    None,
    Thin,
    Medium,
    Thick,
    Dashed,
    Dotted,
    Double,
    Hair,
    MediumDashed,
    DashDot,
    MediumDashDot,
    DashDotDot,
    MediumDashDotDot,
    SlantDashDot,
}

impl BorderStyle {
    pub fn width(&self) -> f32 {
        match self {
            BorderStyle::None => 0.0,
            BorderStyle::Hair | BorderStyle::Thin | BorderStyle::Dashed | BorderStyle::Dotted | BorderStyle::DashDot | BorderStyle::DashDotDot => 1.0,
            BorderStyle::Medium
            | BorderStyle::MediumDashed
            | BorderStyle::MediumDashDot
            | BorderStyle::MediumDashDotDot
            | BorderStyle::SlantDashDot => 2.0,
            BorderStyle::Thick | BorderStyle::Double => 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BorderLine {
    pub style: BorderStyle,
    pub color: Color,
}

impl BorderLine {
    pub fn thin() -> BorderLine {
        BorderLine { style: BorderStyle::Thin, color: Color::Auto }
    }
    pub fn is_none(&self) -> bool {
        self.style == BorderStyle::None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Borders {
    pub left: BorderLine,
    pub right: BorderLine,
    pub top: BorderLine,
    pub bottom: BorderLine,
    pub diag_down: BorderLine,
    pub diag_up: BorderLine,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HAlign {
    #[default]
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterAcross,
    Distributed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VAlign {
    Top,
    Center,
    #[default]
    Bottom,
    Justify,
    Distributed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Alignment {
    pub h: HAlign,
    pub v: VAlign,
    pub wrap: bool,
    pub shrink: bool,
    pub indent: u8,
    /// Degrees -90..=90, or 255 for vertical stacked text.
    pub rotation: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Protection {
    pub locked: bool,
    pub hidden: bool,
}

impl Default for Protection {
    fn default() -> Self {
        Protection { locked: true, hidden: false }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Style {
    pub font: Font,
    pub fill: Fill,
    pub borders: Borders,
    pub align: Alignment,
    /// Number format code; "General" by default.
    pub num_fmt: NumFmt,
    pub protection: Protection,
    /// Differential formats only.
    #[serde(default)]
    pub dxf_stated: DxfStated,
}

/// The parts a differential format states at a value its `Style` can't tell from unset: a dxf's
/// default parts are unset, so a bold turned off, a face, a size or a protection that matches the
/// defaults would otherwise read as nothing. The values are the style's own; this records that
/// the format states them, and only where they match the defaults, so a format built in the app
/// and the same format read back from a file stay equal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DxfStated {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub size: bool,
    pub name: bool,
    /// `<scheme val>`: the theme font the face follows (`major`, `minor` or `none`).
    pub scheme: Option<String>,
    pub protection: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NumFmt(pub Arc<str>);

impl Default for NumFmt {
    fn default() -> Self {
        NumFmt(Arc::from("General"))
    }
}

impl NumFmt {
    pub fn new(s: &str) -> NumFmt {
        NumFmt(Arc::from(s))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Interned styles. Index 0 is the default (Normal) style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleTable {
    styles: Vec<Style>,
    #[serde(skip)]
    index: HashMap<Style, StyleId>,
}

impl Default for StyleTable {
    fn default() -> Self {
        let d = Style::default();
        let mut index = HashMap::new();
        index.insert(d.clone(), StyleId::DEFAULT);
        StyleTable { styles: vec![d], index }
    }
}

impl StyleTable {
    /// A table whose default (Normal) style is `normal`, such as a file's own Normal font.
    pub fn with_default(normal: Style) -> Self {
        let mut index = HashMap::new();
        index.insert(normal.clone(), StyleId::DEFAULT);
        StyleTable { styles: vec![normal], index }
    }
    pub fn get(&self, id: StyleId) -> &Style {
        self.styles.get(id.0 as usize).or_else(|| self.styles.first()).unwrap_or(&DEFAULT_STYLE)
    }
    pub fn intern(&mut self, s: Style) -> StyleId {
        if self.index.is_empty() && !self.styles.is_empty() {
            self.rebuild_index();
        }
        if let Some(id) = self.index.get(&s) {
            return *id;
        }
        let id = StyleId(self.styles.len() as u32);
        self.index.insert(s.clone(), id);
        self.styles.push(s);
        id
    }
    /// The style `id` changed by `f`, interned.
    pub fn derive(&mut self, id: StyleId, f: impl FnOnce(&mut Style)) -> StyleId {
        let mut s = self.get(id).clone();
        f(&mut s);
        self.intern(s)
    }
    pub fn len(&self) -> usize {
        self.styles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (StyleId, &Style)> {
        self.styles.iter().enumerate().map(|(i, s)| (StyleId(i as u32), s))
    }
    pub fn rebuild_index(&mut self) {
        self.index = self.styles.iter().enumerate().map(|(i, s)| (s.clone(), StyleId(i as u32))).collect();
    }
}

static DEFAULT_STYLE: std::sync::LazyLock<Style> = std::sync::LazyLock::new(Style::default);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning() {
        let mut t = StyleTable::default();
        let bold = t.derive(StyleId::DEFAULT, |s| s.font.bold = true);
        assert_ne!(bold, StyleId::DEFAULT);
        assert_eq!(t.derive(StyleId::DEFAULT, |s| s.font.bold = true), bold);
        assert_eq!(t.derive(bold, |s| s.font.bold = false), StyleId::DEFAULT);
        assert_eq!(t.len(), 2);
        assert!(!t.get(StyleId(999)).font.bold);
    }

    #[test]
    fn a_theme_saved_before_per_script_fonts_still_opens() {
        let mut json = serde_json::to_value(Theme::default()).unwrap();
        let fields = json.as_object_mut().unwrap();
        assert!(fields.remove("major_font_scripts").is_some() && fields.remove("minor_font_scripts").is_some());
        assert_eq!(serde_json::from_value::<Theme>(json).unwrap(), Theme::default());
    }

    #[test]
    fn colors() {
        let th = Theme::default();
        assert_eq!(Color::from_hex("#FF0000").unwrap().resolve(&th), Some([255, 0, 0]));
        assert_eq!(Color::from_hex("FF00FF00").unwrap().resolve(&th), Some([0, 255, 0]));
        assert_eq!(Color::Theme(0, -500).resolve(&th), Some([128, 128, 128]));
        assert_eq!(Color::Theme(1, 500).resolve(&th), Some([128, 128, 128]));
        assert_eq!(Color::Auto.resolve(&th), None);
    }

    #[test]
    fn theme_json_keeps_ea_and_cs_and_reads_old_json() {
        // JSON written before the ea/cs fields existed still deserializes (`serde(default)`).
        let mut old = serde_json::to_value(Theme::default()).unwrap();
        let obj = old.as_object_mut().unwrap();
        for key in ["major_font_ea", "minor_font_ea", "major_font_cs", "minor_font_cs"] {
            assert!(obj.remove(key).is_some(), "the default theme serializes {key}");
        }
        assert_eq!(serde_json::from_value::<Theme>(old).unwrap(), Theme::default());

        // Set fields serialize and come back.
        let t = Theme {
            major_font_ea: "游ゴシック".into(),
            minor_font_ea: "ＭＳ Ｐゴシック".into(),
            major_font_cs: "Arial".into(),
            minor_font_cs: "Times New Roman".into(),
            ..Theme::default()
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("major_font_ea") && json.contains("游ゴシック"), "{json}");
        assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), t);
    }

    #[test]
    fn gradient_sampling_and_normalisation() {
        let th = Theme::default();
        let stops = vec![GradientStop { position: 0.0, color: Color::Rgb(0x000000) }, GradientStop { position: 1.0, color: Color::Rgb(0xFFFFFF) }];
        let horizontal = GradientFill::linear(0.0, stops.clone());
        assert_eq!(horizontal.position(0.0, 0.5), 0.0);
        assert_eq!(horizontal.position(1.0, 0.5), 1.0);
        assert_eq!(horizontal.sample(0.5, 0.5, &th), Some([128, 128, 128]));

        // SpreadsheetML: 0 = left-to-right, 90 = top-to-bottom, 180 = right-to-left,
        // 270 = bottom-to-top.
        let vertical = GradientFill::linear(90.0, stops.clone());
        assert_eq!(vertical.position(0.5, 0.0), 0.0);
        assert_eq!(vertical.position(0.5, 1.0), 1.0);
        let right_to_left = GradientFill::linear(180.0, stops.clone());
        assert_eq!(right_to_left.position(0.0, 0.5), 1.0);
        assert_eq!(right_to_left.position(1.0, 0.5), 0.0);
        let bottom_to_top = GradientFill::linear(270.0, stops.clone());
        assert_eq!(bottom_to_top.position(0.5, 0.0), 1.0);
        assert_eq!(bottom_to_top.position(0.5, 1.0), 0.0);
        let down_right = GradientFill::linear(45.0, stops.clone());
        assert_eq!(down_right.position(0.0, 0.0), 0.0);
        assert_eq!(down_right.position(1.0, 1.0), 1.0);
        let down_left = GradientFill::linear(135.0, stops.clone());
        assert_eq!(down_left.position(1.0, 0.0), 0.0);
        assert_eq!(down_left.position(0.0, 1.0), 1.0);

        // Resolving once per cell must give the same samples as resolving per point.
        let resolved = horizontal.resolved_stops(&th);
        for (u, v) in [(0.0, 0.0), (0.25, 0.75), (0.5, 0.5), (1.0, 1.0)] {
            assert_eq!(horizontal.sample_resolved(&resolved, u, v), horizontal.sample(u, v, &th));
        }

        // Swatches and HTML fall back to the first gradient stop, then the solid foreground.
        let preview = Fill::gradient(
            GradientKind::Linear { degree: 90.0 },
            vec![GradientStop { position: 0.0, color: Color::Rgb(0xFF0000) }, GradientStop { position: 1.0, color: Color::Rgb(0x0000FF) }],
        );
        assert_eq!(preview.preview_color(&th), Some([255, 0, 0]));
        assert_eq!(Fill::solid(Color::Rgb(0x00FF00)).preview_color(&th), Some([0, 255, 0]));
        assert_eq!(Fill::default().preview_color(&th), None);

        let center = GradientFill::path(0.5, 0.5, 0.5, 0.5, stops);
        assert_eq!(center.position(0.5, 0.5), 0.0);
        assert_eq!(center.position(0.0, 0.5), 1.0);

        let mut fill = Fill::gradient(
            GradientKind::Linear { degree: 0.0 },
            vec![GradientStop { position: 2.0, color: Color::Rgb(0xFFFFFF) }, GradientStop { position: -1.0, color: Color::Rgb(0x000000) }],
        );
        fill.pattern = PatternType::Solid;
        fill.normalize();
        let g = fill.gradient.as_ref().unwrap();
        assert_eq!(g.stops.iter().map(|s| s.position).collect::<Vec<_>>(), vec![0.0, 1.0]);
        assert_eq!(fill.pattern, PatternType::None);
        assert_eq!(Fill::solid(Color::Rgb(0xFF0000)).gradient, None);
    }

    #[test]
    fn gradient_normalisation_agrees_with_hash() {
        let stop = |p: f64| GradientStop { position: p, color: Color::Rgb(0xFF0000) };
        let a = GradientFill::new(GradientKind::Linear { degree: -0.0 }, vec![stop(-0.0), stop(1.0)]);
        let b = GradientFill::new(GradientKind::Linear { degree: 0.0 }, vec![stop(0.0), stop(1.0)]);
        assert_eq!(a, b);
        assert_eq!(a.kind, GradientKind::Linear { degree: 0.0 });
        assert_eq!(a.stops[0].position.to_bits(), 0.0f64.to_bits());

        let path = GradientFill::new(GradientKind::Path { left: -0.0, right: 0.0, top: -0.0, bottom: 0.0 }, vec![stop(0.5)]);
        if let GradientKind::Path { left, right, top, bottom } = path.kind {
            assert_eq!(left.to_bits(), 0.0f64.to_bits());
            assert_eq!(top.to_bits(), 0.0f64.to_bits());
            assert_eq!(right.to_bits(), 0.0f64.to_bits());
            assert_eq!(bottom.to_bits(), 0.0f64.to_bits());
        }

        let fill = |g: GradientFill| Fill { pattern: PatternType::None, fg: Color::Auto, bg: Color::Auto, gradient: Some(g) };
        let mut styles = StyleTable::default();
        let ia = styles.intern(Style { fill: fill(a), ..Style::default() });
        let ib = styles.intern(Style { fill: fill(b), ..Style::default() });
        assert_eq!(ia, ib, "equal fills must intern to one StyleId");
    }

    #[test]
    fn gradient_keeps_every_stop() {
        let stops = (0..100).map(|i| GradientStop { position: i as f64 / 99.0, color: Color::Rgb(0x010000 + i) }).collect();
        let g = GradientFill::new(GradientKind::Linear { degree: 0.0 }, stops);
        assert_eq!(g.stops.len(), 100);
    }
}
