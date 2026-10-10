//! GridCraft number formats: Excel format codes (`#,##0.00;[Red](#,##0.00)`, dates, fractions,
//! scientific…), the General display format and the `TEXT()` worksheet function. Codes are stored
//! in the invariant (en-US) spelling; rendering and the local spelling of codes follow a
//! [`Locale`] (separators, month and day names, AM/PM, booleans and errors).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod builtin;
mod decimal;
mod local;
mod parse;
mod render;

use std::sync::Arc;

use gridcraft_core::{CellError, DateSystem, Value};
use gridcraft_locale::{INVARIANT, Locale, Regional};

pub use builtin::{builtin_format, builtin_id};
pub use decimal::{format_general_fit, format_general_fit_in};
pub use local::{from_local_code, to_local_code};

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
    fn format_number(&self, n: f64, sys: DateSystem, reg: &Regional) -> (Formatted, bool) {
        let general_text = |n: f64| format_general_fit_in(n, GENERAL_WIDTH, reg).unwrap_or_else(|| OVERFLOW.into());
        let general = |n: f64| Formatted { text: general_text(n), color: None, fill: None, numeric: true };
        if self.is_general() {
            return (general(n), false);
        }
        let Some((sec, drop_sign)) = self.select(n) else { return (general(n), false) };
        let system = system_section(sec, reg);
        let sec = system.as_ref().unwrap_or(sec);
        let minus = n < 0.0 && !drop_sign;
        let out: Option<Out> = match sec.kind {
            SecKind::Text => Some(render::render_text(sec, &format_general_fit_in(n, GENERAL_WIDTH, reg).unwrap_or_default())),
            SecKind::General => {
                let width = if minus { GENERAL_WIDTH - 1 } else { GENERAL_WIDTH };
                Some(render::render_general(sec, n.abs(), width, reg.decimal))
            }
            SecKind::Date => {
                if minus {
                    None
                } else {
                    render::render_date(sec, n.abs(), sys, reg)
                }
            }
            SecKind::Number => Some(render::render_number(sec, n.abs(), reg)),
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

/// A format Excel takes from the system settings instead of from the code itself.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SystemFormat {
    ShortDate,
    ShortDateTime,
    LongDate,
    LongTime,
}

impl SystemFormat {
    /// The built-ins Excel stores as ids 14 and 22 are displayed with the region's short date
    /// and short date and time.
    fn of_code(code: &str) -> Option<SystemFormat> {
        let code = code.trim();
        if code.eq_ignore_ascii_case("m/d/yyyy") {
            Some(SystemFormat::ShortDate)
        } else if code.eq_ignore_ascii_case("m/d/yyyy h:mm") {
            Some(SystemFormat::ShortDateTime)
        } else {
            None
        }
    }

    /// The `[$-F800]` and `[$-F400]` tags Excel writes for the system long date and long time.
    fn of_lcid(lcid: u16) -> Option<SystemFormat> {
        match lcid {
            0xF800 => Some(SystemFormat::LongDate),
            0xF400 => Some(SystemFormat::LongTime),
            _ => None,
        }
    }

    fn code(self, reg: &Regional) -> String {
        match self {
            SystemFormat::ShortDate => reg.short_date.to_string(),
            SystemFormat::ShortDateTime => format!("{} {}", reg.short_date, reg.short_time),
            SystemFormat::LongDate => reg.long_date.to_string(),
            SystemFormat::LongTime => reg.long_time.to_string(),
        }
    }

    /// The region's own format for this system format (parsed once per region).
    fn format(self, reg: &Regional) -> NumberFormat {
        use std::cell::RefCell;
        use std::collections::HashMap;
        thread_local! {
            static CACHE: RefCell<HashMap<(&'static str, SystemFormat), NumberFormat>> = RefCell::new(HashMap::new());
        }
        CACHE.with(|cache| cache.borrow_mut().entry((reg.tag, self)).or_insert_with(|| NumberFormat::parse(&self.code(reg))).clone())
    }
}

/// The region's replacement for a built-in system date/time code; `None` when the code is not
/// one of them or the region is en-US (whose formats are the codes themselves).
fn system_alias(fmt: &NumberFormat, reg: &Regional) -> Option<NumberFormat> {
    if reg.tag == INVARIANT.regional.tag {
        return None;
    }
    SystemFormat::of_code(fmt.code()).map(|which| which.format(reg))
}

/// A section tagged `[$-F800]` or `[$-F400]` with the region's long date or long time pattern in
/// place of its own; colour and condition stay with the section. `None` for other sections and
/// for en-US.
fn system_section(sec: &Section, reg: &Regional) -> Option<Section> {
    if reg.tag == INVARIANT.regional.tag {
        return None;
    }
    let which = SystemFormat::of_lcid(sec.lcid?)?;
    let replacement = which.format(reg).inner.sections.first().cloned()?;
    Some(Section { color: sec.color, cond: sec.cond, ..replacement })
}

/// Formats a cell value for display in the invariant (en-US) locale.
pub fn format_value(v: &Value, fmt: &NumberFormat, sys: DateSystem) -> Formatted {
    format_value_in(v, fmt, sys, &INVARIANT)
}

/// Formats a cell value for display in a locale: the region's separators, month and day names and
/// AM/PM designators, and the formula language's boolean and error literals. The built-in short
/// date and time codes (`m/d/yyyy`, `m/d/yyyy h:mm`) and the system long date and time follow
/// the region's own formats, as in Excel.
pub fn format_value_in(v: &Value, fmt: &NumberFormat, sys: DateSystem, loc: &Locale) -> Formatted {
    match v {
        Value::Empty => Formatted::default(),
        Value::Number(n) if !n.is_finite() => Formatted { text: loc.formula.local_error(CellError::Num.as_str()).into(), ..Formatted::default() },
        Value::Number(n) => {
            let alias = system_alias(fmt, &loc.regional);
            alias.as_ref().unwrap_or(fmt).format_number(*n, sys, &loc.regional).0
        }
        Value::Text(t) => fmt.format_text(t),
        Value::Bool(b) => Formatted { text: loc.formula.bool_text(*b).into(), ..Formatted::default() },
        Value::Error(e) => Formatted { text: loc.formula.local_error(e.as_str()).into(), ..Formatted::default() },
        Value::Array(a) => match a.data.first() {
            Some(Value::Array(_)) | None => Formatted::default(),
            Some(first) => format_value_in(first, fmt, sys, loc),
        },
    }
}

/// The `TEXT()` worksheet function in the invariant locale: formats a value with a canonical
/// format code. Numeric text is converted to a number first; other text goes through the text
/// section (or is returned as is).
pub fn text_function(v: &Value, code: &str, sys: DateSystem) -> Result<String, CellError> {
    text_function_in(v, code, sys, &INVARIANT)
}

/// The `TEXT()` worksheet function in a locale. The code is spelled as a user of that locale
/// types it: the formula language's format letters (`dd/mm/aaaa`, `TT.MM.JJJJ`, `Geral`) and the
/// region's separators (`#.##0,00`). Numeric text is read with the region's separators.
pub fn text_function_in(v: &Value, local_code: &str, sys: DateSystem, loc: &Locale) -> Result<String, CellError> {
    if local_code.is_empty() {
        return Ok(String::new());
    }
    if local::too_long(local_code) {
        return Err(CellError::Value);
    }
    let fmt = NumberFormat::parse(&from_local_code(local_code, &loc.dialect()));
    let n = match v.scalar() {
        Value::Error(e) => return Err(e),
        Value::Empty => 0.0,
        Value::Number(n) => n,
        Value::Bool(b) => return Ok(loc.formula.bool_text(b).into()),
        Value::Text(t) => match gridcraft_core::parse::parse_number_text_in(&t, sys, &loc.regional) {
            Some(n) => n,
            None => return Ok(fmt.format_text(&t).text),
        },
        Value::Array(_) => return Err(CellError::Value),
    };
    if !n.is_finite() {
        return Err(CellError::Num);
    }
    let alias = system_alias(&fmt, &loc.regional);
    let (f, overflow) = alias.as_ref().unwrap_or(&fmt).format_number(n, sys, &loc.regional);
    if overflow { Err(CellError::Value) } else { Ok(f.text) }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_locale;
