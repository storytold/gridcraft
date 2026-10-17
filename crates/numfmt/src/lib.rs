//! GridCraft number formats: Excel format codes (`#,##0.00;[Red](#,##0.00)`, dates, fractions,
//! scientific…), the General display format and the `TEXT()` worksheet function. en-US only.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod builtin;
mod decimal;
mod parse;
mod render;

use std::sync::Arc;

use gridcraft_core::{CellError, DateSystem, Value};

pub use builtin::{builtin_format, builtin_id};
pub use decimal::format_general_fit;

use parse::{DatePart, SecKind, Section, Tok};
use render::Out;

/// Width (in characters) of General output in a cell.
const GENERAL_WIDTH: usize = 11;
/// What Excel paints when a date is out of range.
const OVERFLOW: &str = "########";

/// Colour from a `[Red]` or `[ColorN]` section tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormatColor {
    Black,
    Blue,
    Cyan,
    Green,
    Magenta,
    Red,
    White,
    Yellow,
    /// `[Color1]`..`[Color56]` (palette index).
    Indexed(u8),
}

/// Best guess at the ribbon's number-format category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormatKind {
    General,
    Number,
    Currency,
    Accounting,
    Date,
    Time,
    Percentage,
    Fraction,
    Scientific,
    Text,
    /// Zip codes, phone numbers, social security numbers.
    Special,
    Custom,
}

/// What the worksheet function `CELL` reports about a number format.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CellFormatInfo {
    /// `CELL("format")`: `G`, `F2`, `,0`, `C2-`, `P0`, `S2`, `D1`…`D9`, with `-` for a coloured
    /// negative section and `()` for parentheses.
    pub code: String,
    /// `CELL("color")`: the second section has a colour.
    pub color: bool,
    /// `CELL("parentheses")`: the first section has an unquoted, unescaped `(`.
    pub parentheses: bool,
}

/// A formatted value ready to paint.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Formatted {
    pub text: String,
    pub color: Option<FormatColor>,
    /// Repeat-fill char from `*x` and its byte position in `text`, for the renderer to expand to
    /// the cell width.
    pub fill: Option<(char, usize)>,
    /// True when the result should be right-aligned by default (numbers); General text is left
    /// aligned.
    pub numeric: bool,
}

#[derive(Debug)]
struct Inner {
    code: String,
    general: bool,
    sections: Vec<Section>,
    /// Sections that apply to numbers (a trailing text section is excluded).
    num_sections: usize,
    /// Index of the section that applies to text, if any.
    text_section: Option<usize>,
}

/// A parsed number format code. Cheap to clone.
#[derive(Clone, Debug)]
pub struct NumberFormat {
    inner: Arc<Inner>,
}

impl PartialEq for NumberFormat {
    fn eq(&self, other: &Self) -> bool {
        self.inner.code == other.inner.code
    }
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat::general()
    }
}

/// Longest number format code, in characters (not bytes), that is parsed: Excel's own limit.
/// A longer code is invalid: [`NumberFormat::parse`] returns a General format (the cell shows
/// its value unformatted; the stored code is kept as is) and `TEXT` returns `#VALUE!`. The cap
/// also bounds every allocation that scales with the number of placeholders in a code.
pub const MAX_FORMAT_CODE_CHARS: usize = 255;

/// True when `code` is longer than [`MAX_FORMAT_CODE_CHARS`] characters.
fn too_long(code: &str) -> bool {
    // A code of more than 255 bytes can still be short in characters; count only when needed.
    code.len() > MAX_FORMAT_CODE_CHARS && code.chars().count() > MAX_FORMAT_CODE_CHARS
}

impl NumberFormat {
    /// Parses a format code. Never fails: unknown or malformed parts degrade to literal text, and
    /// a code longer than [`MAX_FORMAT_CODE_CHARS`] characters degrades to General.
    pub fn parse(code: &str) -> NumberFormat {
        Self::parse_inner(code, false)
    }

    /// `parse` without the length cap, so tests can reach renderer paths a capped code cannot.
    #[cfg(test)]
    fn parse_uncapped(code: &str) -> NumberFormat {
        Self::parse_inner(code, true)
    }

    fn parse_inner(code: &str, uncapped: bool) -> NumberFormat {
        #[cfg(test)]
        parse_cache::count_parse();
        let trimmed = code.trim();
        // `checkbox` marks a checkbox cell (Insert › Checkbox): TRUE/FALSE draw as a box, any other
        // value displays as General.
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("general") || trimmed == "checkbox" || (!uncapped && too_long(code)) {
            return NumberFormat {
                inner: Arc::new(Inner { code: code.to_string(), general: true, sections: Vec::new(), num_sections: 0, text_section: None }),
            };
        }
        let sections = parse::parse_sections(code);
        let len = sections.len();
        let last_is_text = sections.last().is_some_and(|s| s.kind == SecKind::Text);
        let text_section = if len >= 4 {
            Some(3)
        } else if last_is_text {
            Some(len - 1)
        } else {
            None
        };
        let num_sections = if len >= 4 {
            3
        } else if last_is_text {
            len - 1
        } else {
            len
        };
        NumberFormat { inner: Arc::new(Inner { code: code.to_string(), general: false, sections, num_sections, text_section }) }
    }

    pub fn general() -> NumberFormat {
        NumberFormat::parse("General")
    }

    pub fn code(&self) -> &str {
        &self.inner.code
    }

    pub fn is_general(&self) -> bool {
        self.inner.general
    }

    fn num_sections(&self) -> &[Section] {
        self.inner.sections.get(..self.inner.num_sections).unwrap_or(&[])
    }

    /// True when the code contains date or time tokens.
    pub fn is_date(&self) -> bool {
        self.num_sections().iter().any(|s| s.kind == SecKind::Date)
    }

    pub fn is_percent(&self) -> bool {
        self.num_sections().iter().any(|s| s.kind == SecKind::Number && s.percent > 0)
    }

    /// True for a pure text format such as `@`.
    pub fn is_text(&self) -> bool {
        self.inner.num_sections == 0 && self.inner.text_section.is_some()
    }

    /// Best guess at the format category for the ribbon's dropdown.
    pub fn kind(&self) -> FormatKind {
        if self.is_general() {
            return FormatKind::General;
        }
        if self.is_text() {
            return FormatKind::Text;
        }
        const SPECIAL: [&str; 4] = ["00000", "00000-0000", "[<=9999999]###-####;(###) ###-####", "000-00-0000"];
        if SPECIAL.contains(&self.code()) {
            return FormatKind::Special;
        }
        let Some(s) = self.inner.sections.first() else { return FormatKind::Custom };
        match s.kind {
            SecKind::General => return FormatKind::General,
            SecKind::Text => return FormatKind::Text,
            SecKind::Date => {
                let calendar = s.toks.iter().any(|t| matches!(t, Tok::Date(d) if d.is_calendar()));
                return if calendar { FormatKind::Date } else { FormatKind::Time };
            }
            SecKind::Number => {}
        }
        if s.fraction {
            FormatKind::Fraction
        } else if s.exp {
            FormatKind::Scientific
        } else if s.percent > 0 {
            FormatKind::Percentage
        } else if s.fill && s.has_digits {
            FormatKind::Accounting
        } else if s.currency && s.has_digits {
            FormatKind::Currency
        } else if s.has_digits {
            FormatKind::Number
        } else {
            FormatKind::Custom
        }
    }

    /// What `CELL("format")`, `CELL("color")` and `CELL("parentheses")` report for this code.
    pub fn cell_info(&self) -> CellFormatInfo {
        let first = self.inner.sections.first();
        let color = self.inner.sections.get(1).is_some_and(|s| s.color.is_some());
        let parentheses = first.is_some_and(|s| s.paren);
        let mut code = match first {
            Some(s) if !self.is_general() && !self.is_text() => section_cell_code(s),
            _ => "G".to_string(),
        };
        if color {
            code.push('-');
        }
        if parentheses {
            code.push_str("()");
        }
        CellFormatInfo { code, color, parentheses }
    }

    /// Picks the number section for `v`. Returns the section and whether the minus sign is
    /// dropped (the default negative section shows the absolute value). `None` = General.
    fn select(&self, v: f64) -> Option<(&Section, bool)> {
        let secs = self.num_sections();
        let k = secs.len();
        let s0 = secs.first()?;
        let c0 = s0.cond;
        let c1 = secs.get(1).and_then(|s| s.cond);
        if c0.is_some() || c1.is_some() {
            let idx = if c0.is_some_and(|c| c.test(v)) {
                0
            } else if c1.is_some_and(|c| c.test(v)) {
                1
            } else if c0.is_some() && c1.is_some() {
                2
            } else if c0.is_none() {
                0
            } else {
                1
            };
            // Excel never adds a minus sign in a multi-section conditional format.
            return secs.get(idx).map(|s| (s, k >= 2));
        }
        if v < 0.0 && k >= 2 {
            return secs.get(1).map(|s| (s, true));
        }
        if v == 0.0 && k >= 3 {
            return secs.get(2).map(|s| (s, false));
        }
        Some((s0, false))
    }

    /// Formats a number. The flag is true when the value cannot be shown (date out of range).
    fn format_number(&self, n: f64, sys: DateSystem) -> (Formatted, bool) {
        let general = |n: f64| Formatted {
            text: format_general_fit(n, GENERAL_WIDTH).unwrap_or_else(|| OVERFLOW.into()),
            color: None,
            fill: None,
            numeric: true,
        };
        if self.is_general() {
            return (general(n), false);
        }
        let Some((sec, drop_sign)) = self.select(n) else { return (general(n), false) };
        let minus = n < 0.0 && !drop_sign;
        let out: Option<Out> = match sec.kind {
            SecKind::Text => Some(render::render_text(sec, &format_general_fit(n, GENERAL_WIDTH).unwrap_or_default())),
            SecKind::General => {
                let width = if minus { GENERAL_WIDTH - 1 } else { GENERAL_WIDTH };
                Some(render::render_general(sec, n.abs(), width))
            }
            SecKind::Date => {
                if minus {
                    None
                } else {
                    render::render_date(sec, n.abs(), sys)
                }
            }
            SecKind::Number => Some(render::render_number(sec, n.abs())),
        };
        let Some(mut out) = out else {
            return (Formatted { text: OVERFLOW.into(), color: sec.color, fill: None, numeric: true }, true);
        };
        // Excel drops the minus sign when the first section shows a negative number as all zeros.
        if minus && sec.kind != SecKind::Text && !out.zero {
            out.prepend("-");
        }
        (Formatted { text: out.text, color: sec.color, fill: out.fill, numeric: true }, false)
    }

    fn format_text(&self, t: &str) -> Formatted {
        match self.inner.text_section.and_then(|i| self.inner.sections.get(i)) {
            Some(sec) => {
                let out = render::render_text(sec, t);
                Formatted { text: out.text, color: sec.color, fill: out.fill, numeric: false }
            }
            None => Formatted { text: t.to_string(), color: None, fill: None, numeric: false },
        }
    }
}

/// The `CELL("format")` letter and digit for a format's first section.
fn section_cell_code(s: &Section) -> String {
    match s.kind {
        SecKind::General | SecKind::Text => "G".into(),
        SecKind::Date => date_cell_code(s).into(),
        SecKind::Number if s.fraction => "G".into(),
        SecKind::Number => {
            let decimals = s
                .toks
                .iter()
                .skip_while(|t| !matches!(t, Tok::Point))
                .take_while(|t| !matches!(t, Tok::Exp { .. }))
                .filter(|t| matches!(t, Tok::Digit(_)))
                .count();
            let dollar = s.toks.iter().any(|t| matches!(t, Tok::Lit(l) if l.contains('$')));
            let letter = if s.percent > 0 {
                "P"
            } else if s.exp {
                "S"
            } else if dollar {
                "C"
            } else if s.thousands {
                ","
            } else {
                "F"
            };
            format!("{letter}{decimals}")
        }
    }
}

/// `D1`…`D9` (or `G`) for a date or time section: dates go by the order of their day, month and
/// year parts, times without date parts by their hours, minutes, seconds and AM/PM.
fn date_cell_code(s: &Section) -> &'static str {
    let parts = || {
        s.toks.iter().filter_map(|t| match t {
            Tok::Date(d) => Some(*d),
            _ => None,
        })
    };
    let mut order = parts().filter_map(|d| match d {
        DatePart::Day(_) => Some('d'),
        DatePart::Month(_) => Some('m'),
        DatePart::Year(_) => Some('y'),
        _ => None,
    });
    let (a, b, c) = (order.next(), order.next(), order.next());
    if a.is_some() {
        return match (a, b, c) {
            (Some('d'), Some('m'), Some('y')) => "D1",
            (Some('m'), Some('d'), Some('y')) => "D4",
            (Some('d'), Some('m'), _) => "D2",
            (Some('m'), Some('y'), _) => "D3",
            (Some('m'), Some('d'), _) => "D5",
            _ => "G",
        };
    }
    if parts().any(|d| matches!(d, DatePart::ElapsedH(_) | DatePart::ElapsedM(_) | DatePart::ElapsedS(_))) {
        return "G";
    }
    let hour = parts().any(|d| matches!(d, DatePart::Hour(_)));
    let minute = parts().any(|d| matches!(d, DatePart::Minute(_)));
    let second = parts().any(|d| matches!(d, DatePart::Second(_)));
    let ampm = parts().any(|d| matches!(d, DatePart::AmPm | DatePart::AP(..)));
    match (hour && minute, second, ampm) {
        (false, _, _) => "G",
        (true, true, true) => "D6",
        (true, false, true) => "D7",
        (true, true, false) => "D8",
        (true, false, false) => "D9",
    }
}

/// Formats a cell value for display.
pub fn format_value(v: &Value, fmt: &NumberFormat, sys: DateSystem) -> Formatted {
    match v {
        Value::Empty => Formatted::default(),
        Value::Number(n) if !n.is_finite() => Formatted { text: CellError::Num.as_str().into(), ..Formatted::default() },
        Value::Number(n) => fmt.format_number(*n, sys).0,
        Value::Text(t) => fmt.format_text(t),
        Value::Bool(b) => Formatted { text: if *b { "TRUE" } else { "FALSE" }.into(), ..Formatted::default() },
        Value::Error(e) => Formatted { text: e.as_str().into(), ..Formatted::default() },
        Value::Array(a) => match a.data.first() {
            Some(Value::Array(_)) | None => Formatted::default(),
            Some(first) => format_value(first, fmt, sys),
        },
    }
}

/// A small per-thread cache of parsed format codes for callers that format many values with the
/// same code (`TEXT` over a range, chart labels). A parsed format depends on the code text only
/// (the date system and values are applied when formatting), so the code is the whole key.
mod parse_cache {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::NumberFormat;

    /// Most codes kept per thread. A full cache is emptied, so memory stays bounded.
    pub(super) const CAPACITY: usize = 64;
    /// Longer codes are parsed every time and never stored.
    pub(super) const MAX_CODE_LEN: usize = 256;

    thread_local! {
        static CACHE: RefCell<HashMap<String, NumberFormat>> = RefCell::new(HashMap::new());
    }

    pub(super) fn get(code: &str) -> NumberFormat {
        if code.len() > MAX_CODE_LEN {
            return NumberFormat::parse(code);
        }
        // Not usable (thread shutting down, re-entered): parse without the cache.
        let cached = CACHE
            .try_with(|c| {
                let mut c = c.try_borrow_mut().ok()?;
                if let Some(f) = c.get(code) {
                    return Some(f.clone());
                }
                let f = NumberFormat::parse(code);
                if c.len() >= CAPACITY {
                    c.clear();
                }
                c.insert(code.to_string(), f.clone());
                Some(f)
            })
            .ok()
            .flatten();
        cached.unwrap_or_else(|| NumberFormat::parse(code))
    }

    #[cfg(test)]
    thread_local! {
        static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[cfg(test)]
    pub(super) fn count_parse() {
        let _ = PARSES.try_with(|p| p.set(p.get() + 1));
    }

    /// Parses done on this thread so far (tests only).
    #[cfg(test)]
    pub(super) fn parses() -> usize {
        PARSES.try_with(std::cell::Cell::get).unwrap_or(0)
    }

    /// Empties this thread's cache (tests only).
    #[cfg(test)]
    pub(super) fn clear() {
        let _ = CACHE.try_with(|c| c.borrow_mut().clear());
    }
}

impl NumberFormat {
    /// Like [`NumberFormat::parse`], but remembers recently parsed codes (per thread, bounded) so
    /// formatting many values with one code parses it once. Same result as `parse`.
    pub fn parse_cached(code: &str) -> NumberFormat {
        parse_cache::get(code)
    }
}

/// The `TEXT()` worksheet function: formats a value with a format code. Numeric text is
/// converted to a number first; other text and booleans (as `TRUE`/`FALSE`) go through the text
/// section, or are returned as is.
pub fn text_function(v: &Value, code: &str, sys: DateSystem) -> Result<String, CellError> {
    if code.is_empty() {
        return Ok(String::new());
    }
    if too_long(code) {
        return Err(CellError::Value);
    }
    let fmt = NumberFormat::parse_cached(code);
    let n = match v.scalar() {
        Value::Error(e) => return Err(e),
        Value::Empty => 0.0,
        Value::Number(n) => n,
        Value::Bool(b) => return Ok(fmt.format_text(if b { "TRUE" } else { "FALSE" }).text),
        Value::Text(t) => match gridcraft_core::parse::parse_number_text(&t) {
            Some(n) => n,
            None => return Ok(fmt.format_text(&t).text),
        },
        Value::Array(_) => return Err(CellError::Value),
    };
    if !n.is_finite() {
        return Err(CellError::Num);
    }
    let (f, overflow) = fmt.format_number(n, sys);
    if overflow { Err(CellError::Value) } else { Ok(f.text) }
}

#[cfg(test)]
mod tests;
