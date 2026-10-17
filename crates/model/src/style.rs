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
}

impl Default for Theme {
    /// GridCraft's own default theme ("Craft"): original palette.
    fn default() -> Self {
        Theme {
            name: "Craft".into(),
            colors: [0xFFFFFF, 0x000000, 0xE8E8E8, 0x0E2841, 0x156082, 0xE97132, 0x196B24, 0x0F9ED5, 0xA02B93, 0x4EA72E, 0x467886, 0x96607D],
            major_font: crate::DEFAULT_FONT.into(),
            minor_font: crate::DEFAULT_FONT.into(),
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
}

impl Eq for Font {}
impl Hash for Font {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.name.hash(h);
        self.size.to_bits().hash(h);
        (self.bold, self.italic, self.underline, self.strike, self.color, self.vert).hash(h);
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fill {
    pub pattern: PatternType,
    /// Pattern foreground (the solid colour for `Solid`).
    pub fg: Color,
    pub bg: Color,
}

impl Fill {
    pub fn solid(c: Color) -> Fill {
        Fill { pattern: PatternType::Solid, fg: c, bg: Color::Auto }
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

/// Properties explicitly set by a conditional format. An absent property inherits the cell's
/// format; an explicit default (for example `Bold` with false) overrides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StyleProperty {
    FontName,
    FontSize,
    Bold,
    Italic,
    Underline,
    Strike,
    FontColor,
    FontVertical,
    Fill,
    LeftBorder,
    RightBorder,
    TopBorder,
    BottomBorder,
    DiagonalDown,
    DiagonalUp,
    NumberFormat,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Style {
    /// Differential formats only. Missing in older JSON workbooks: non-default parts are set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specified: Option<std::collections::BTreeSet<StyleProperty>>,
    pub font: Font,
    pub fill: Fill,
    pub borders: Borders,
    pub align: Alignment,
    /// Number format code; "General" by default.
    pub num_fmt: NumFmt,
    pub protection: Protection,
}

impl StyleProperty {
    pub const ALL: [Self; 16] = [
        Self::FontName,
        Self::FontSize,
        Self::Bold,
        Self::Italic,
        Self::Underline,
        Self::Strike,
        Self::FontColor,
        Self::FontVertical,
        Self::Fill,
        Self::LeftBorder,
        Self::RightBorder,
        Self::TopBorder,
        Self::BottomBorder,
        Self::DiagonalDown,
        Self::DiagonalUp,
        Self::NumberFormat,
    ];
}

impl Style {
    /// Backwards-compatible interpretation of the old default-as-unset differential styles.
    pub fn differential_properties(&self) -> std::collections::BTreeSet<StyleProperty> {
        if let Some(set) = &self.specified {
            return set.clone();
        }
        self.non_default_properties()
    }

    /// Checks presence without allocating in worksheet and print rendering.
    pub fn sets(&self, property: StyleProperty) -> bool {
        self.specified.as_ref().map_or_else(|| self.is_non_default(property), |set| set.contains(&property))
    }

    fn is_non_default(&self, property: StyleProperty) -> bool {
        match property {
            StyleProperty::FontName => self.font.name != crate::DEFAULT_FONT,
            StyleProperty::FontSize => self.font.size != crate::DEFAULT_FONT_SIZE,
            StyleProperty::Bold => self.font.bold,
            StyleProperty::Italic => self.font.italic,
            StyleProperty::Underline => self.font.underline != Underline::None,
            StyleProperty::Strike => self.font.strike,
            StyleProperty::FontColor => self.font.color != Color::Auto,
            StyleProperty::FontVertical => self.font.vert != VertAlign::Baseline,
            StyleProperty::Fill => self.fill != Fill::default(),
            StyleProperty::LeftBorder => self.borders.left != BorderLine::default(),
            StyleProperty::RightBorder => self.borders.right != BorderLine::default(),
            StyleProperty::TopBorder => self.borders.top != BorderLine::default(),
            StyleProperty::BottomBorder => self.borders.bottom != BorderLine::default(),
            StyleProperty::DiagonalDown => self.borders.diag_down != BorderLine::default(),
            StyleProperty::DiagonalUp => self.borders.diag_up != BorderLine::default(),
            StyleProperty::NumberFormat => self.num_fmt.as_str() != "General",
        }
    }

    fn non_default_properties(&self) -> std::collections::BTreeSet<StyleProperty> {
        StyleProperty::ALL.into_iter().filter(|&p| self.is_non_default(p)).collect()
    }

    /// Keep the old representation whenever it describes exactly the same overrides.
    pub fn normalize_differential(&mut self) {
        if let Some(fields) = self.specified.clone() {
            let mut canonical = Self { align: self.align, protection: self.protection, ..Self::default() };
            for property in &fields {
                canonical.copy_property(self, *property);
            }
            canonical.specified = Some(fields);
            *self = canonical;
        }
        // Unused fill/border colors have no XML representation when their pattern/style is none.
        if self.fill.pattern == PatternType::None {
            self.fill = Fill::default();
        } else if self.fill.pattern == PatternType::Solid {
            self.fill.bg = Color::Auto;
        }
        for line in [
            &mut self.borders.left,
            &mut self.borders.right,
            &mut self.borders.top,
            &mut self.borders.bottom,
            &mut self.borders.diag_down,
            &mut self.borders.diag_up,
        ] {
            if line.is_none() {
                *line = BorderLine::default();
            }
        }
        if self.specified.as_ref() == Some(&self.non_default_properties()) {
            self.specified = None;
        }
    }

    pub fn copy_property(&mut self, source: &Style, property: StyleProperty) {
        match property {
            StyleProperty::FontName => self.font.name = source.font.name.clone(),
            StyleProperty::FontSize => self.font.size = source.font.size,
            StyleProperty::Bold => self.font.bold = source.font.bold,
            StyleProperty::Italic => self.font.italic = source.font.italic,
            StyleProperty::Underline => self.font.underline = source.font.underline,
            StyleProperty::Strike => self.font.strike = source.font.strike,
            StyleProperty::FontColor => self.font.color = source.font.color,
            StyleProperty::FontVertical => self.font.vert = source.font.vert,
            StyleProperty::Fill => self.fill = source.fill,
            StyleProperty::LeftBorder => self.borders.left = source.borders.left,
            StyleProperty::RightBorder => self.borders.right = source.borders.right,
            StyleProperty::TopBorder => self.borders.top = source.borders.top,
            StyleProperty::BottomBorder => self.borders.bottom = source.borders.bottom,
            StyleProperty::DiagonalDown => self.borders.diag_down = source.borders.diag_down,
            StyleProperty::DiagonalUp => self.borders.diag_up = source.borders.diag_up,
            StyleProperty::NumberFormat => self.num_fmt = source.num_fmt.clone(),
        }
    }

    /// Applies only the properties a conditional format sets, including explicit defaults.
    pub fn apply_differential(&mut self, source: &Style) {
        for property in StyleProperty::ALL {
            if source.sets(property) {
                self.copy_property(source, property);
            }
        }
    }

    /// Fill gaps from a lower-priority rule without overwriting a higher-priority choice.
    pub fn differential_under(&mut self, lower: &Style) {
        let mut set = self.differential_properties();
        for property in lower.differential_properties() {
            if set.insert(property) {
                self.copy_property(lower, property);
            }
        }
        self.specified = Some(set);
        self.normalize_differential();
    }
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
    fn colors() {
        let th = Theme::default();
        assert_eq!(Color::from_hex("#FF0000").unwrap().resolve(&th), Some([255, 0, 0]));
        assert_eq!(Color::from_hex("FF00FF00").unwrap().resolve(&th), Some([0, 255, 0]));
        assert_eq!(Color::Theme(0, -500).resolve(&th), Some([128, 128, 128]));
        assert_eq!(Color::Theme(1, 500).resolve(&th), Some([128, 128, 128]));
        assert_eq!(Color::Auto.resolve(&th), None);
    }
}
