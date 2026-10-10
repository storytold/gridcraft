//! GridCraft number formats: Excel format codes (`#,##0.00;[Red](#,##0.00)`, dates, fractions,
//! scientific…), the General display format and the `TEXT()` worksheet function.
//!
//! Format codes are always en-US, as in files. [`format_value_in`] shows a value in a display
//! locale (German: `1.234,56`, `10.10.2026`, `Okt`, `WAHR`, `#NV`); `TEXT()` and
//! [`format_value`] stay en-US, so computed results never depend on the interface language.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod builtin;
mod code;
mod decimal;
mod parse;
mod render;

use std::sync::Arc;

use gridcraft_core::{CellError, DateSystem, Locale, Value};

pub use builtin::{builtin_format, builtin_id};
pub use code::{code_from_local, code_to_local};
pub use decimal::format_general_fit;

use parse::{SecKind, Section, Tok};
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

impl NumberFormat {
    /// Parses a format code. Never fails: unknown or malformed parts degrade to literal text.
    pub fn parse(code: &str) -> NumberFormat {
        let trimmed = code.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("general") {
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
    fn format_number(&self, n: f64, sys: DateSystem, loc: Locale) -> (Formatted, bool) {
        let general = |n: f64| Formatted {
            text: format_general_fit(n, GENERAL_WIDTH).map(|s| loc.number_literal(&s)).unwrap_or_else(|| OVERFLOW.into()),
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
            SecKind::Text => Some(render::render_text(sec, &loc.number_literal(&format_general_fit(n, GENERAL_WIDTH).unwrap_or_default()))),
            SecKind::General => {
                let width = if minus { GENERAL_WIDTH - 1 } else { GENERAL_WIDTH };
                Some(render::render_general(sec, n.abs(), width, loc))
            }
            SecKind::Date => {
                if minus {
                    None
                } else {
                    render::render_date(sec, n.abs(), sys, loc)
                }
            }
            SecKind::Number => Some(render::render_number(sec, n.abs(), loc)),
        };
        let Some(mut out) = out else {
            return (Formatted { text: OVERFLOW.into(), color: sec.color, fill: None, numeric: true }, true);
        };
        if minus && sec.kind != SecKind::Text {
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

/// Formats a cell value for display (en-US).
pub fn format_value(v: &Value, fmt: &NumberFormat, sys: DateSystem) -> Formatted {
    format_value_in(v, fmt, sys, Locale::EnUs)
}

/// Formats a cell value for display in `loc`: its separators, month and day names, booleans
/// and error names. Excel's locale-dependent built-in date and time formats (short date, the
/// `[$-F800]` long date and `[$-F400]` time) take the locale's own pattern, as in Excel.
pub fn format_value_in(v: &Value, fmt: &NumberFormat, sys: DateSystem, loc: Locale) -> Formatted {
    match v {
        Value::Empty => Formatted::default(),
        Value::Number(n) if !n.is_finite() => Formatted { text: loc.error_name(CellError::Num).into(), ..Formatted::default() },
        Value::Number(n) => match locale_pattern(fmt, loc) {
            Some(local) => local.format_number(*n, sys, loc).0,
            None => fmt.format_number(*n, sys, loc).0,
        },
        Value::Text(t) => fmt.format_text(t),
        Value::Bool(b) => Formatted { text: loc.bool_name(*b).into(), ..Formatted::default() },
        Value::Error(e) => Formatted { text: loc.error_name(*e).into(), ..Formatted::default() },
        Value::Array(a) => match a.data.first() {
            Some(Value::Array(_)) | None => Formatted::default(),
            Some(first) => format_value_in(first, fmt, sys, loc),
        },
    }
}

/// German Excel's own patterns for the locale-dependent built-in formats.
const DE_PATTERNS: [&str; 7] = [
    "dd\".\"mm\".\"yyyy",
    "dd\". \"mmm\" \"yy",
    "dd\". \"mmm",
    "mmm\" \"yy",
    "dd\".\"mm\".\"yyyy\" \"hh:mm",
    "dddd\", \"d\". \"mmmm\" \"yyyy",
    "hh:mm:ss",
];

fn locale_pattern_index(code: &str) -> Option<usize> {
    Some(match code.trim() {
        "m/d/yyyy" => 0,
        "d-mmm-yy" => 1,
        "d-mmm" => 2,
        "mmm-yy" => 3,
        "m/d/yyyy h:mm" => 4,
        c if c.to_ascii_uppercase().starts_with("[$-F800]") => 5,
        c if c.to_ascii_uppercase().starts_with("[$-F400]") => 6,
        _ => return None,
    })
}

/// The code a locale-dependent built-in format shows in `loc` (`m/d/yyyy` is `dd.mm.yyyy` in
/// German Excel). `None` when the code is shown as written.
pub(crate) fn locale_pattern_code(code: &str, loc: Locale) -> Option<&'static str> {
    if loc != Locale::De {
        return None;
    }
    locale_pattern_index(code).and_then(|i| DE_PATTERNS.get(i).copied())
}

/// The pattern a locale-dependent built-in format takes in `loc` (`m/d/yyyy` is `TT.MM.JJJJ`
/// in German Excel). `None` when the code is shown as written.
fn locale_pattern(fmt: &NumberFormat, loc: Locale) -> Option<&'static NumberFormat> {
    if loc != Locale::De {
        return None;
    }
    static DE: std::sync::OnceLock<Vec<NumberFormat>> = std::sync::OnceLock::new();
    let de = DE.get_or_init(|| DE_PATTERNS.iter().map(|c| NumberFormat::parse(c)).collect());
    locale_pattern_index(fmt.code()).and_then(|i| de.get(i))
}

/// The `TEXT()` worksheet function: formats a value with a format code. Numeric text is
/// converted to a number first; other text goes through the text section (or is returned as is).
pub fn text_function(v: &Value, code: &str, sys: DateSystem) -> Result<String, CellError> {
    if code.is_empty() {
        return Ok(String::new());
    }
    let fmt = NumberFormat::parse(code);
    let n = match v.scalar() {
        Value::Error(e) => return Err(e),
        Value::Empty => 0.0,
        Value::Number(n) => n,
        Value::Bool(b) => return Ok(if b { "TRUE" } else { "FALSE" }.into()),
        Value::Text(t) => match gridcraft_core::parse::parse_number_text(&t) {
            Some(n) => n,
            None => return Ok(fmt.format_text(&t).text),
        },
        Value::Array(_) => return Err(CellError::Value),
    };
    if !n.is_finite() {
        return Err(CellError::Num);
    }
    let (f, overflow) = fmt.format_number(n, sys, Locale::EnUs);
    if overflow { Err(CellError::Value) } else { Ok(f.text) }
}

#[cfg(test)]
mod tests;
