//! Text functions.
//!
//! This module implements the text functions that are not part of the core scalar switch in
//! `eval.rs`: the concatenation helpers (`COMBINEVALUES`, `CONCATENATEX`), number/date formatting
//! (`FIXED`, `FORMAT`), the character based `REPLACE`, table serialization (`TOCSV`, `TOJSON`) and
//! `UNICODE`.
//!
//! `FORMAT` follows the Visual Basic (OLE Automation) format strings that DAX documents
//! (<https://learn.microsoft.com/dax/format-function-dax>): the predefined names, one to three
//! numeric sections plus an optional text section, digit placeholders, grouping/scaling commas,
//! percent, scientific notation, quoted/escaped literals and the documented date/time tokens.
//! Formatting is culture invariant (en-US separators and English month/weekday names); the
//! optional locale argument is accepted but cannot change the output.
#![allow(unused_imports)]

use std::cmp::Ordering;

use super::support::{date_parts, date_serial, date_weekday, eval_all, eval_table, frame_for};
use super::*;

/// Upper bound for any string this module builds. DAX text has no practical limit, but a
/// serialized table or concatenation must not be able to exhaust memory from a validated call.
const MAX_TEXT_LEN: usize = 1_048_576;

/// Upper bound for a `FORMAT` pattern. Excel limits custom formats to 255 characters; a generous
/// multiple accepts real-world patterns while keeping every scan bounded.
const MAX_FORMAT_LEN: usize = 4096;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "COMBINEVALUES" => combine_values(ev, args, ctx),
        "CONCATENATEX" => concatenate_x(ev, args, ctx),
        "FIXED" => fixed(ev, args, ctx),
        "FORMAT" => format_value(ev, args, ctx),
        "REPLACE" => replace(ev, args, ctx),
        "TOCSV" => to_csv(ev, args, ctx),
        "TOJSON" => to_json(ev, args, ctx),
        "TRIM" => trim(ev, args, ctx),
        "UNICODE" => unicode(ev, args, ctx),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

/// Appends `text`, rejecting a result longer than [`MAX_TEXT_LEN`].
fn push_checked(out: &mut String, text: &str) -> Result<(), EvalError> {
    if out.len().checked_add(text.len()).is_none_or(|length| length > MAX_TEXT_LEN) {
        return Err(EvalError::new("the text result exceeds the maximum supported length"));
    }
    out.push_str(text);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// COMBINEVALUES / CONCATENATEX
// ---------------------------------------------------------------------------------------------

fn combine_values(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    if args.len() < 3 {
        return Err(EvalError::new("COMBINEVALUES needs a delimiter and at least two values"));
    }
    let delimiter = ev.eval(&args[0], ctx)?.to_text();
    let mut out = String::new();
    for (index, arg) in args.iter().enumerate().skip(1) {
        if index > 1 {
            push_checked(&mut out, &delimiter)?;
        }
        let value = ev.eval(arg, ctx)?;
        push_checked(&mut out, &value.to_text())?;
    }
    Ok(Scalar::String(out))
}

/// Orders two values for `CONCATENATEX`. BLANK sorts last in ascending order (and therefore first
/// when the order is reversed), matching DAX's ordering.
fn compare_order(left: Option<&Scalar>, right: Option<&Scalar>) -> Ordering {
    let left = left.cloned().unwrap_or(Scalar::Blank);
    let right = right.cloned().unwrap_or(Scalar::Blank);
    match (left.is_blank(), right.is_blank()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => left.compare(&right).unwrap_or_else(|| left.to_text().cmp(&right.to_text())),
    }
}

fn concatenate_x(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table_arg, expression) = match (args.first(), args.get(1)) {
        (Some(table), Some(expression)) => (table, expression),
        _ => return Err(EvalError::new("CONCATENATEX needs a table and an expression")),
    };
    let table = eval_table(ev, table_arg, ctx)?;
    let delimiter = match args.get(2) {
        Some(expr) => ev.eval(expr, ctx)?.to_text(),
        None => String::new(),
    };
    let order_by = args.get(3);
    // 0/FALSE/DESC is descending, 1/TRUE/ASC ascending. dax.guide documents ASC as the default
    // when an order-by expression has no explicit order.
    let descending = match args.get(4) {
        Some(expr) => {
            let value = ev.eval(expr, ctx)?;
            match &value {
                Scalar::String(text) if text.eq_ignore_ascii_case("ASC") => false,
                Scalar::String(text) if text.eq_ignore_ascii_case("DESC") => true,
                other => !other.to_bool().ok_or_else(|| EvalError::new("CONCATENATEX order must be ASC, DESC or a Boolean"))?,
            }
        }
        None => false,
    };
    let mut items: Vec<(Option<Scalar>, String)> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        ctx.rows.push(frame_for(ev, &table, row));
        let value = ev.eval(expression, ctx);
        let key = match (&value, order_by) {
            (Ok(_), Some(expr)) => Some(ev.eval(expr, ctx)),
            _ => None,
        };
        ctx.rows.pop();
        let value = value?;
        let key = match key {
            Some(key) => Some(key?),
            None => None,
        };
        items.push((key, value.to_text()));
    }
    if order_by.is_some() {
        items.sort_by(|left, right| {
            let ordering = compare_order(left.0.as_ref(), right.0.as_ref());
            if descending { ordering.reverse() } else { ordering }
        });
    }
    if items.is_empty() {
        return Ok(Scalar::Blank);
    }
    let mut out = String::new();
    for (index, (_, text)) in items.iter().enumerate() {
        if index > 0 {
            push_checked(&mut out, &delimiter)?;
        }
        push_checked(&mut out, text)?;
    }
    Ok(Scalar::String(out))
}

// ---------------------------------------------------------------------------------------------
// FIXED
// ---------------------------------------------------------------------------------------------

fn fixed(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = super::support::args("FIXED", &values);
    let number = a.number(0)?;
    if !number.is_finite() {
        return Err(EvalError::new("FIXED argument 1 must be a finite number"));
    }
    let decimals = a.optional_number(1, 2.0)?;
    if !decimals.is_finite() || !(-127.0..=127.0).contains(&decimals) {
        return Err(EvalError::new("FIXED decimals must be between -127 and 127"));
    }
    let decimals = decimals.trunc() as i32;
    let no_commas = a.optional_bool(2, false)?;

    let (integer, fraction) = decimal_parts(number.abs());
    let (integer, fraction) = round_decimal(&integer, &fraction, decimals);

    let mut out = String::new();
    if number < 0.0 {
        out.push('-');
    }
    if no_commas {
        out.push_str(&integer);
    } else {
        out.push_str(&group_thousands(&integer));
    }
    if decimals > 0 {
        out.push('.');
        out.push_str(&fraction);
    }
    if out.len() > MAX_TEXT_LEN {
        return Err(EvalError::new("FIXED result exceeds the maximum supported length"));
    }
    Ok(Scalar::String(out))
}

/// Decomposes a non-negative finite number into its first 15 significant decimal digits, which is
/// the precision Excel uses when it rounds and displays values (`2.675` follows the decimal
/// digits a user typed, not the exact binary expansion).
fn decimal_parts(magnitude: f64) -> (String, String) {
    if !magnitude.is_finite() || magnitude == 0.0 {
        return ("0".to_string(), "0".repeat(15));
    }
    let exponent = magnitude.log10().floor() as i32;
    let places = (14 - exponent).clamp(0, 20) as usize;
    let text = format!("{magnitude:.places$}");
    match text.split_once('.') {
        Some((integer, fraction)) => (integer.to_string(), fraction.to_string()),
        None => (text, "0".repeat(15)),
    }
}

/// Increments a decimal digit vector in place, growing it when every digit was a nine.
fn increment_digits(digits: &mut Vec<u8>) {
    for digit in digits.iter_mut().rev() {
        if *digit < 9 {
            *digit += 1;
            return;
        }
        *digit = 0;
    }
    digits.insert(0, 1);
}

fn digits_to_string(digits: &[u8]) -> String {
    digits.iter().map(|digit| char::from(b'0' + digit)).collect()
}

/// Drops leading zeros, keeping at least one digit.
fn normalize_integer(digits: &str) -> String {
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
}

/// Rounds a positive decimal expansion (integer and fraction digits without a point) to
/// `decimals` places, half away from zero. The results carry no sign.
fn round_decimal(integer: &str, fraction: &str, decimals: i32) -> (String, String) {
    if decimals >= 0 {
        let places = decimals as usize;
        if places >= fraction.len() {
            let mut result = fraction.to_string();
            result.push_str(&"0".repeat(places - fraction.len()));
            return (normalize_integer(integer), result);
        }
        let round_up = fraction.as_bytes().get(places).is_some_and(|digit| *digit >= b'5');
        let mut digits: Vec<u8> = integer.bytes().filter(u8::is_ascii_digit).map(|digit| digit - b'0').collect();
        digits.extend(fraction.bytes().take(places).map(|digit| digit - b'0'));
        if round_up {
            increment_digits(&mut digits);
        }
        let split = digits.len().saturating_sub(places);
        let rounded_integer = normalize_integer(&digits_to_string(&digits[..split]));
        let rounded_fraction = digits_to_string(&digits[split..]);
        (rounded_integer, rounded_fraction)
    } else {
        let places = decimals.unsigned_abs() as usize;
        if integer.len() <= places {
            return ("0".to_string(), String::new());
        }
        let split = integer.len() - places;
        let round_up = integer.as_bytes().get(split).is_some_and(|digit| *digit >= b'5');
        let mut digits: Vec<u8> = integer.bytes().take(split).filter(u8::is_ascii_digit).map(|digit| digit - b'0').collect();
        if round_up {
            increment_digits(&mut digits);
        }
        let mut result = normalize_integer(&digits_to_string(&digits));
        result.push_str(&"0".repeat(places));
        (result, String::new())
    }
}

fn group_thousands(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let length = digits.len();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (length - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// FORMAT
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum PredefinedFormat {
    /// "General": numbers use General Number, date/time values use General Date.
    General,
    GeneralNumber,
    GeneralDate,
    Pattern(&'static str),
    Boolean(BooleanStyle),
}

#[derive(Clone, Copy)]
enum BooleanStyle {
    YesNo,
    TrueFalse,
    OnOff,
}

fn predefined_format(name: &str) -> Option<PredefinedFormat> {
    let key = name.trim().to_ascii_lowercase();
    Some(match key.as_str() {
        "general" => PredefinedFormat::General,
        "general number" => PredefinedFormat::GeneralNumber,
        "general date" => PredefinedFormat::GeneralDate,
        "currency" => PredefinedFormat::Pattern("$#,##0.00;($#,##0.00)"),
        "fixed" => PredefinedFormat::Pattern("0.00"),
        "standard" => PredefinedFormat::Pattern("#,##0.00"),
        "percent" => PredefinedFormat::Pattern("0.00%"),
        "scientific" => PredefinedFormat::Pattern("0.00E+00"),
        "yes/no" => PredefinedFormat::Boolean(BooleanStyle::YesNo),
        "true/false" => PredefinedFormat::Boolean(BooleanStyle::TrueFalse),
        "on/off" => PredefinedFormat::Boolean(BooleanStyle::OnOff),
        "long date" | "medium date" => PredefinedFormat::Pattern("dddd, mmmm d, yyyy"),
        "short date" => PredefinedFormat::Pattern("m/d/yyyy"),
        "long time" => PredefinedFormat::Pattern("h:nn:ss AM/PM"),
        "medium time" => PredefinedFormat::Pattern("h:nn AM/PM"),
        "short time" => PredefinedFormat::Pattern("h:nn"),
        _ => return None,
    })
}

fn boolean_style(style: BooleanStyle, value: &Scalar) -> String {
    let truthy = value.to_bool().unwrap_or_else(|| value.to_number().is_some_and(|number| number != 0.0));
    match style {
        BooleanStyle::YesNo => if truthy { "Yes" } else { "No" }.to_string(),
        BooleanStyle::TrueFalse => if truthy { "True" } else { "False" }.to_string(),
        BooleanStyle::OnOff => if truthy { "On" } else { "Off" }.to_string(),
    }
}

/// Splits a format string on `;`, ignoring separators inside quoted literals or after a
/// backslash escape.
fn split_sections(format: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut chars = format.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '"' => {
                current.push(ch);
                for next in chars.by_ref() {
                    current.push(next);
                    if next == '"' {
                        break;
                    }
                }
            }
            ';' => sections.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    sections.push(current);
    sections
}

/// Formats a text value with the optional fourth (text) section of a custom format.
fn apply_text_section(section: &str, text: &str) -> String {
    let chars: Vec<char> = section.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '"' => {
                index += 1;
                while index < chars.len() {
                    let ch = chars[index];
                    index += 1;
                    if ch == '"' {
                        break;
                    }
                    out.push(ch);
                }
            }
            '\\' => {
                index += 1;
                if let Some(ch) = chars.get(index) {
                    out.push(*ch);
                    index += 1;
                }
            }
            '@' => {
                out.push_str(text);
                index += 1;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    out
}

fn general_number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    format!("{value}")
}

/// True when the pattern contains date/time tokens and no digit placeholders, so a numeric value
/// should be interpreted as a date serial.
fn date_format_like(format: &str) -> bool {
    let mut has_placeholder = false;
    let mut has_token = false;
    let mut in_quote = false;
    let mut escaped = false;
    for ch in format.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_quote {
            if ch == '"' {
                in_quote = false;
            }
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => in_quote = true,
            '0' | '#' => has_placeholder = true,
            'd' | 'D' | 'm' | 'M' | 'y' | 'Y' | 'h' | 'H' | 'n' | 'N' | 's' | 'S' | 'q' | 'Q' | 'w' | 'W' | 'c' | 'C' | 't' | 'T' => has_token = true,
            _ => {}
        }
    }
    !has_placeholder && has_token
}

fn format_value(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = super::support::args("FORMAT", &values);
    let value = a.arg(0)?;
    let format = match a.optional(1) {
        Some(Scalar::Blank) | None => String::new(),
        Some(other) => other.to_text(),
    };
    // The optional LocaleName is accepted for signature compatibility; formatting is invariant.
    if value.is_blank() {
        return Ok(Scalar::String(String::new()));
    }
    if format.chars().count() > MAX_FORMAT_LEN {
        return Err(EvalError::new("FORMAT pattern is too long"));
    }
    if let Scalar::String(text) = value {
        let sections = split_sections(&format);
        if sections.len() >= 4 && !sections[3].is_empty() {
            let formatted = apply_text_section(&sections[3], text);
            if formatted.len() > MAX_TEXT_LEN {
                return Err(EvalError::new("FORMAT result exceeds the maximum supported length"));
            }
            return Ok(Scalar::String(formatted));
        }
        return Ok(Scalar::String(text.clone()));
    }

    let trimmed = format.trim();
    let named = predefined_format(trimmed);
    if trimmed.is_empty() {
        return Ok(Scalar::String(match value {
            Scalar::DateTime(serial) => format_date(*serial, "m/d/yyyy h:nn:ss AM/PM")?,
            Scalar::Boolean(flag) => if *flag { "TRUE" } else { "FALSE" }.to_string(),
            other => general_number(other.to_number().unwrap_or(0.0)),
        }));
    }
    let pattern = match named {
        Some(PredefinedFormat::Pattern(pattern)) => pattern.to_string(),
        Some(PredefinedFormat::GeneralDate) => "m/d/yyyy h:nn:ss AM/PM".to_string(),
        Some(PredefinedFormat::General) => {
            return Ok(Scalar::String(match value {
                Scalar::DateTime(serial) => format_date(*serial, "m/d/yyyy h:nn:ss AM/PM")?,
                Scalar::Boolean(flag) => if *flag { "TRUE" } else { "FALSE" }.to_string(),
                other => general_number(other.to_number().unwrap_or(0.0)),
            }));
        }
        Some(PredefinedFormat::GeneralNumber) => {
            return Ok(Scalar::String(general_number(value.to_number().unwrap_or(0.0))));
        }
        Some(PredefinedFormat::Boolean(style)) => return Ok(Scalar::String(boolean_style(style, value))),
        None => format.clone(),
    };

    if date_format_like(&pattern) {
        let serial = match value {
            Scalar::DateTime(serial) => *serial,
            other => other.to_number().ok_or_else(|| EvalError::new("FORMAT argument 1 must be a date or a number"))?,
        };
        return Ok(Scalar::String(format_date(serial, &pattern)?));
    }
    let number = value.to_number().ok_or_else(|| EvalError::new("FORMAT argument 1 must be a number, date, Boolean or text"))?;
    let formatted = format_numeric(number, &pattern)?;
    if formatted.len() > MAX_TEXT_LEN {
        return Err(EvalError::new("FORMAT result exceeds the maximum supported length"));
    }
    Ok(Scalar::String(formatted))
}

// -- numeric formats ---------------------------------------------------------------------------

enum NumericToken {
    Digit(char),
    Dot,
    Comma,
    Percent,
    Literal(String),
    Exponent { sign: char, places: usize },
}

fn scan_numeric_format(format: &str) -> Vec<NumericToken> {
    let chars: Vec<char> = format.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '"' => {
                index += 1;
                let mut literal = String::new();
                while index < chars.len() {
                    let ch = chars[index];
                    index += 1;
                    if ch == '"' {
                        break;
                    }
                    literal.push(ch);
                }
                tokens.push(NumericToken::Literal(literal));
            }
            '\\' => {
                index += 1;
                if let Some(ch) = chars.get(index) {
                    tokens.push(NumericToken::Literal(ch.to_string()));
                    index += 1;
                }
            }
            '0' | '#' => {
                tokens.push(NumericToken::Digit(chars[index]));
                index += 1;
            }
            '.' => {
                tokens.push(NumericToken::Dot);
                index += 1;
            }
            ',' => {
                tokens.push(NumericToken::Comma);
                index += 1;
            }
            '%' => {
                tokens.push(NumericToken::Percent);
                index += 1;
            }
            'e' | 'E' => {
                let mut cursor = index + 1;
                let sign = match chars.get(cursor) {
                    Some('+') => Some('+'),
                    Some('-') => Some('-'),
                    _ => None,
                };
                if sign.is_some() {
                    cursor += 1;
                    let start = cursor;
                    while cursor < chars.len() && chars[cursor] == '0' {
                        cursor += 1;
                    }
                    if cursor > start {
                        tokens.push(NumericToken::Exponent { sign: sign.unwrap_or('+'), places: cursor - start });
                        index = cursor;
                        continue;
                    }
                }
                tokens.push(NumericToken::Literal(chars[index].to_string()));
                index += 1;
            }
            other => {
                tokens.push(NumericToken::Literal(other.to_string()));
                index += 1;
            }
        }
    }
    tokens
}

/// Assembles the digit blob: applies the `0`/`#` placeholders, the grouping comma and the
/// optional decimal point.
fn assemble_number(integer: &str, fraction: &str, integer_pattern: &str, fraction_pattern: &str, grouping: bool, has_dot: bool) -> String {
    let min_zeros = integer_pattern.chars().filter(|ch| *ch == '0').count();
    let mut integer_digits = normalize_integer(&integer.chars().filter(char::is_ascii_digit).collect::<String>());
    while integer_digits.len() > min_zeros && integer_digits.starts_with('0') {
        integer_digits.remove(0);
    }
    while integer_digits.len() < min_zeros {
        integer_digits.insert(0, '0');
    }
    let mut fraction_digits: Vec<char> = fraction.chars().collect();
    while let Some(last) = fraction_digits.last().copied() {
        let position = fraction_digits.len() - 1;
        if last == '0' && fraction_pattern.chars().nth(position) == Some('#') {
            fraction_digits.pop();
        } else {
            break;
        }
    }
    let mut out = String::new();
    if grouping && !integer_digits.is_empty() {
        out.push_str(&group_thousands(&integer_digits));
    } else {
        out.push_str(&integer_digits);
    }
    if has_dot {
        out.push('.');
        out.extend(fraction_digits);
    }
    out
}

fn exponent_text(sign: char, places: usize, exponent: i32) -> String {
    let magnitude = exponent.unsigned_abs();
    let width = places;
    match (sign, exponent < 0) {
        ('-', true) => format!("E-{magnitude:0width$}"),
        ('-', false) => format!("E{magnitude:0width$}"),
        (_, true) => format!("E-{magnitude:0width$}"),
        (_, false) => format!("E+{magnitude:0width$}"),
    }
}

fn format_numeric(value: f64, pattern: &str) -> Result<String, EvalError> {
    if !value.is_finite() {
        return Err(EvalError::new("FORMAT cannot format a non-finite number"));
    }
    let sections = split_sections(pattern);
    if sections.is_empty() {
        return Ok(general_number(value));
    }
    let negative = value < 0.0;
    let zero = value == 0.0;
    let magnitude;
    let section;
    let add_sign;
    if negative {
        magnitude = -value;
        match sections.get(1).filter(|section| !section.is_empty()) {
            Some(negative_section) => {
                section = negative_section.as_str();
                add_sign = false;
            }
            None => {
                section = sections[0].as_str();
                add_sign = true;
            }
        }
    } else if zero {
        magnitude = 0.0;
        match sections.get(2).filter(|section| !section.is_empty()) {
            Some(zero_section) => section = zero_section.as_str(),
            None => section = sections[0].as_str(),
        }
        add_sign = false;
    } else {
        magnitude = value;
        section = sections[0].as_str();
        add_sign = false;
    }
    if section.trim().eq_ignore_ascii_case("general") {
        return Ok(general_number(magnitude));
    }

    let tokens = scan_numeric_format(section);
    let has_digits = tokens.iter().any(|token| matches!(token, NumericToken::Digit(_)));
    if !has_digits {
        // A section without digit placeholders is a literal (for example `"N/A"`).
        let mut out = String::new();
        for token in &tokens {
            match token {
                NumericToken::Literal(text) => push_checked(&mut out, text)?,
                NumericToken::Percent => out.push('%'),
                NumericToken::Dot => out.push('.'),
                _ => {}
            }
        }
        return Ok(out);
    }

    let first = tokens.iter().position(|token| matches!(token, NumericToken::Digit(_))).unwrap_or(0);
    let last = tokens.iter().rposition(|token| matches!(token, NumericToken::Digit(_))).unwrap_or(0);
    let dot_index = tokens.iter().position(|token| matches!(token, NumericToken::Dot));
    let exponent_index = tokens.iter().position(|token| matches!(token, NumericToken::Exponent { .. }));
    let core_limit = dot_index.or(exponent_index).unwrap_or(last + 1);
    let integer_last = tokens
        .iter()
        .enumerate()
        .filter(|(index, token)| *index >= first && *index < core_limit && matches!(token, NumericToken::Digit(_)))
        .map(|(index, _)| index)
        .next_back();

    let mut integer_pattern = String::new();
    let mut fraction_pattern = String::new();
    let mut seen_dot = false;
    for token in &tokens[first..=last] {
        match token {
            NumericToken::Digit(ch) => {
                if seen_dot {
                    fraction_pattern.push(*ch);
                } else {
                    integer_pattern.push(*ch);
                }
            }
            NumericToken::Dot => seen_dot = true,
            _ => {}
        }
    }
    // A comma between digit placeholders groups thousands; a comma after the last integer digit
    // (including one immediately before the decimal point or at the end of the section) scales
    // the value by 1000 per comma.
    let mut grouping = false;
    let mut scale = 0usize;
    let integer_region_end = dot_index.or(exponent_index).unwrap_or(tokens.len());
    for (index, token) in tokens.iter().enumerate().take(integer_region_end).skip(first) {
        if matches!(token, NumericToken::Comma) {
            match integer_last {
                Some(last_integer) if index > last_integer => scale += 1,
                _ => grouping = true,
            }
        }
    }

    let percent_count = tokens.iter().filter(|token| matches!(token, NumericToken::Percent)).count();
    let mut magnitude = magnitude;
    for _ in 0..percent_count {
        magnitude *= 100.0;
    }
    for _ in 0..scale {
        magnitude /= 1000.0;
    }
    if !magnitude.is_finite() {
        return Err(EvalError::new("FORMAT scaled the value out of range"));
    }

    let has_dot = dot_index.is_some();
    let fraction_len = fraction_pattern.chars().count();
    let (blob, exponent) = match tokens.iter().find_map(|token| match token {
        NumericToken::Exponent { sign, places } => Some((*sign, *places)),
        _ => None,
    }) {
        Some((sign, places)) => {
            if places > 32 {
                return Err(EvalError::new("FORMAT exponent width is too large"));
            }
            let (mantissa, exponent) = scientific_parts(magnitude, fraction_len);
            let (integer, fraction) = round_decimal(&mantissa.0, &mantissa.1, fraction_len as i32);
            (
                assemble_number(&integer, &fraction, &integer_pattern, &fraction_pattern, grouping, has_dot),
                Some(exponent_text(sign, places, exponent)),
            )
        }
        None => {
            let parts = decimal_parts(magnitude);
            let (integer, fraction) = round_decimal(&parts.0, &parts.1, fraction_len as i32);
            (assemble_number(&integer, &fraction, &integer_pattern, &fraction_pattern, grouping, has_dot), None)
        }
    };

    let mut out = String::new();
    if add_sign {
        out.push('-');
    }
    for token in &tokens[..first] {
        match token {
            NumericToken::Literal(text) => push_checked(&mut out, text)?,
            NumericToken::Percent => out.push('%'),
            _ => {}
        }
    }
    push_checked(&mut out, &blob)?;
    for token in &tokens[last + 1..] {
        match token {
            NumericToken::Literal(text) => push_checked(&mut out, text)?,
            NumericToken::Percent => out.push('%'),
            NumericToken::Exponent { .. } => push_checked(&mut out, exponent.as_deref().unwrap_or_default())?,
            _ => {}
        }
    }
    Ok(out)
}

fn scientific_parts(magnitude: f64, fraction_len: usize) -> ((String, String), i32) {
    if magnitude == 0.0 {
        return (decimal_parts(0.0), 0);
    }
    let mut exponent = magnitude.log10().floor() as i32;
    let mut mantissa = magnitude / 10f64.powi(exponent);
    if !mantissa.is_finite() || mantissa == 0.0 {
        return (decimal_parts(magnitude), 0);
    }
    let mut parts = decimal_parts(mantissa);
    let (mut integer, mut fraction) = round_decimal(&parts.0, &parts.1, fraction_len as i32);
    if integer.parse::<f64>().is_ok_and(|value| value >= 10.0) {
        mantissa /= 10.0;
        exponent += 1;
        parts = decimal_parts(mantissa);
        let rounded = round_decimal(&parts.0, &parts.1, fraction_len as i32);
        integer = rounded.0;
        fraction = rounded.1;
    }
    ((integer, fraction), exponent)
}

// -- date/time formats ------------------------------------------------------------------------

const MONTH_NAMES: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAY_NAMES: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn starts_with_ignore_case(chars: &[char], text: &str) -> bool {
    for (index, expected) in text.chars().enumerate() {
        match chars.get(index) {
            Some(actual) if actual.eq_ignore_ascii_case(&expected) => {}
            _ => return false,
        }
    }
    true
}

/// The next format token letter at or after `from`, skipping separators, quoted literals and
/// escaped characters. It lets `mm` in `hh:mm:ss` and `mm:ss` count minutes even though a
/// separator sits between the tokens.
fn next_date_letter(chars: &[char], from: usize) -> Option<char> {
    let mut index = from;
    while index < chars.len() {
        match chars[index] {
            '"' => {
                index += 1;
                while index < chars.len() {
                    let ch = chars[index];
                    index += 1;
                    if ch == '"' {
                        break;
                    }
                }
            }
            '\\' => index += 2,
            ch if ch.is_ascii_alphabetic() => return Some(ch.to_ascii_lowercase()),
            _ => index += 1,
        }
    }
    None
}

fn contains_ampm(format: &str) -> bool {
    let upper = format.to_ascii_uppercase();
    upper.contains("AM/PM") || upper.contains("A/P") || upper.contains("AMPM")
}

fn cased_name(name: &str, upper: bool) -> String {
    if upper { name.to_uppercase() } else { name.to_string() }
}

#[allow(clippy::cognitive_complexity)]
// One pass over Excel's date tokens (y/m/d/h/s and the elapsed-time forms) keeps the section
// handling and the token cases together.
fn format_date(serial: f64, format: &str) -> Result<String, EvalError> {
    let section = split_sections(format).into_iter().next().unwrap_or_default();
    let (year, month, day) = date_parts(serial);
    let weekday = date_weekday(serial);
    let fraction = serial - serial.floor();
    let total_seconds = (fraction * 86_400.0).round() as i64;
    let hour24 = (total_seconds / 3600).rem_euclid(24);
    let minute = (total_seconds / 60).rem_euclid(60);
    let second = total_seconds.rem_euclid(60);
    let use_ampm = contains_ampm(&section);
    let hour = if use_ampm {
        let hour = hour24 % 12;
        if hour == 0 { 12 } else { hour }
    } else {
        hour24
    };
    let before_noon = hour24 < 12;
    let day_of_year = (serial.floor() - date_serial(year, 1, 1, 0, 0, 0)).round() as i64 + 1;
    let january_first_weekday = date_weekday(date_serial(year, 1, 1, 0, 0, 0));
    let week_of_year = (day_of_year + january_first_weekday - 2).div_euclid(7) + 1;

    let chars: Vec<char> = section.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    let mut last_letter: Option<char> = None;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            index += 1;
            while index < chars.len() {
                let literal = chars[index];
                index += 1;
                if literal == '"' {
                    break;
                }
                out.push(literal);
            }
            continue;
        }
        if ch == '\\' {
            index += 1;
            if let Some(literal) = chars.get(index) {
                out.push(*literal);
                index += 1;
            }
            continue;
        }
        if starts_with_ignore_case(&chars[index..], "AM/PM") {
            let label = if before_noon { "AM" } else { "PM" };
            out.push_str(&if ch.is_lowercase() { label.to_ascii_lowercase() } else { label.to_string() });
            index += 5;
            continue;
        }
        if starts_with_ignore_case(&chars[index..], "A/P") {
            let label = if before_noon { 'A' } else { 'P' };
            out.push(if ch.is_lowercase() { label.to_ascii_lowercase() } else { label });
            index += 3;
            continue;
        }
        if starts_with_ignore_case(&chars[index..], "AMPM") {
            let label = if before_noon { "AM" } else { "PM" };
            out.push_str(&if ch.is_lowercase() { label.to_ascii_lowercase() } else { label.to_string() });
            index += 4;
            continue;
        }

        let lower = ch.to_ascii_lowercase();
        if !lower.is_ascii_alphabetic() {
            out.push(ch);
            index += 1;
            continue;
        }
        let mut run = 1;
        while index + run < chars.len() && chars[index + run].to_ascii_lowercase() == lower {
            run += 1;
        }
        let upper = ch.is_ascii_uppercase();
        let minutes = last_letter == Some('h') || next_date_letter(&chars, index + run) == Some('s');
        match lower {
            'd' => match run {
                1 => out.push_str(&day.to_string()),
                2 => out.push_str(&format!("{day:02}")),
                3 => {
                    let name = WEEKDAY_NAMES[(weekday.clamp(1, 7) - 1) as usize];
                    out.push_str(&cased_name(&name[..3], upper));
                }
                4 => out.push_str(&cased_name(WEEKDAY_NAMES[(weekday.clamp(1, 7) - 1) as usize], upper)),
                5 => out.push_str(&format_date(serial, "m/d/yyyy")?),
                6 => out.push_str(&format_date(serial, "dddd, mmmm d, yyyy")?),
                _ => out.push_str(&cased_name(WEEKDAY_NAMES[(weekday.clamp(1, 7) - 1) as usize], upper)),
            },
            'm' if minutes => match run {
                1 => out.push_str(&minute.to_string()),
                _ => out.push_str(&format!("{minute:02}")),
            },
            'm' => match run {
                1 => out.push_str(&month.to_string()),
                2 => out.push_str(&format!("{month:02}")),
                3 => out.push_str(&cased_name(&MONTH_NAMES[(month.clamp(1, 12) - 1) as usize][..3], upper)),
                _ => out.push_str(&cased_name(MONTH_NAMES[(month.clamp(1, 12) - 1) as usize], upper)),
            },
            'n' => match run {
                1 => out.push_str(&minute.to_string()),
                _ => out.push_str(&format!("{minute:02}")),
            },
            's' => match run {
                1 => out.push_str(&second.to_string()),
                _ => out.push_str(&format!("{second:02}")),
            },
            'h' => match run {
                1 => out.push_str(&hour.to_string()),
                _ => out.push_str(&format!("{hour:02}")),
            },
            'y' => match run {
                1 => out.push_str(&day_of_year.to_string()),
                2 => out.push_str(&format!("{:02}", year.rem_euclid(100))),
                _ => out.push_str(&format!("{year:04}")),
            },
            'q' => out.push_str(&((month.clamp(1, 12) + 2) / 3).to_string()),
            'w' if run >= 2 => out.push_str(&week_of_year.to_string()),
            'w' => out.push_str(&weekday.clamp(1, 7).to_string()),
            'c' => {
                let has_date = serial.floor() != 0.0;
                let has_time = fraction != 0.0;
                if has_date && has_time {
                    out.push_str(&format_date(serial, "m/d/yyyy h:nn:ss AM/PM")?);
                } else if has_date {
                    out.push_str(&format_date(serial, "m/d/yyyy")?);
                } else {
                    out.push_str(&format_date(serial, "h:nn:ss AM/PM")?);
                }
            }
            't' if run >= 4 => out.push_str(&format!("{hour24}:{minute:02}:{second:02}")),
            _ => {
                for literal in &chars[index..index + run] {
                    out.push(*literal);
                }
            }
        }
        last_letter = Some(lower);
        index += run;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// REPLACE / UNICODE
// ---------------------------------------------------------------------------------------------

fn replace(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = super::support::args("REPLACE", &values);
    let text = a.text(0)?;
    let start = a.number(1)?;
    let count = a.number(2)?;
    let new_text = a.text(3)?;
    if !start.is_finite() || start < 1.0 {
        return Err(EvalError::new("REPLACE StartPosition must be a number greater than or equal to 1"));
    }
    if !count.is_finite() || count < 0.0 {
        return Err(EvalError::new("REPLACE NumberOfCharacters must not be negative"));
    }
    let characters: Vec<char> = text.chars().collect();
    // Positions and lengths are character based (REPLACE is documented as one character per
    // character; a surrogate pair is still one character).
    let start_index = (start as usize).saturating_sub(1);
    let prefix_length = start_index.min(characters.len());
    let remove = (count as usize).min(characters.len() - prefix_length);
    let mut out = String::with_capacity(text.len() + new_text.len());
    let prefix: String = characters[..prefix_length].iter().collect();
    push_checked(&mut out, &prefix)?;
    push_checked(&mut out, &new_text)?;
    let suffix: String = characters[prefix_length + remove..].iter().collect();
    push_checked(&mut out, &suffix)?;
    Ok(Scalar::String(out))
}

/// `TRIM ( <Text> )`: removes leading and trailing ASCII spaces and collapses every run of ASCII
/// spaces between words to one. Only character 32 is affected, so tabs and the non-breaking space
/// survive, as the catalog documents.
fn trim(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = super::support::args("TRIM", &values);
    let text = a.text(0)?;
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character == ' ' {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(' ');
        }
        pending_space = false;
        out.push(character);
    }
    Ok(Scalar::String(out))
}

fn unicode(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, args, ctx)?;
    let a = super::support::args("UNICODE", &values);
    let text = a.text(0)?;
    match text.chars().next() {
        Some(character) => Ok(Scalar::Integer(i64::from(character as u32))),
        None => Err(EvalError::new("UNICODE requires a text value with at least one character")),
    }
}

// ---------------------------------------------------------------------------------------------
// TOCSV / TOJSON
// ---------------------------------------------------------------------------------------------

fn row_limit(max_rows: i64, total: usize) -> usize {
    if max_rows < 0 { total } else { (max_rows as u64).min(total as u64) as usize }
}

/// Reads the optional `MaxRows` argument: BLANK or omitted means the documented default of 10.
fn max_rows_argument(ev: &Evaluator<'_>, args: &[BoundExpr], index: usize, ctx: &mut Context) -> Result<i64, EvalError> {
    match args.get(index) {
        Some(expr) => {
            let value = ev.eval(expr, ctx)?;
            if value.is_blank() {
                return Ok(10);
            }
            let number = value.to_number().ok_or_else(|| EvalError::new("the MaxRows argument must be numeric"))?;
            if !number.is_finite() {
                return Err(EvalError::new("the MaxRows argument must be a finite number"));
            }
            Ok(number.trunc() as i64)
        }
        None => Ok(10),
    }
}

fn number_text(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 { format!("{value:.0}") } else { format!("{value}") }
}

fn iso_datetime(serial: f64) -> String {
    if !serial.is_finite() {
        return String::new();
    }
    let (year, month, day) = date_parts(serial);
    let fraction = serial - serial.floor();
    let seconds = (fraction * 86_400.0).round() as i64;
    let hour = (seconds / 3600).rem_euclid(24);
    let minute = (seconds / 60).rem_euclid(60);
    let second = seconds.rem_euclid(60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

fn csv_value(value: &Scalar) -> String {
    match value {
        Scalar::Blank => String::new(),
        Scalar::String(text) => text.clone(),
        Scalar::Boolean(true) => "true".to_string(),
        Scalar::Boolean(false) => "false".to_string(),
        Scalar::Integer(number) => number.to_string(),
        Scalar::Currency(scaled) => number_text(*scaled as f64 / 10_000.0),
        Scalar::Decimal(number) => number_text(*number),
        Scalar::DateTime(serial) => iso_datetime(*serial),
    }
}

fn csv_field(text: &str, delimiter: &str) -> String {
    let needs_quotes = text.contains('"') || text.contains('\n') || text.contains('\r') || text.contains(delimiter);
    if !needs_quotes {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn to_csv(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let table_arg = args.first().ok_or_else(|| EvalError::new("TOCSV needs a table"))?;
    let table = eval_table(ev, table_arg, ctx)?;
    let max_rows = max_rows_argument(ev, args, 1, ctx)?;
    let delimiter = match args.get(2) {
        Some(expr) => ev.eval(expr, ctx)?.to_text(),
        None => ",".to_string(),
    };
    if delimiter.is_empty() {
        return Err(EvalError::new("TOCSV Delimiter must not be empty"));
    }
    let include_headers = match args.get(3) {
        Some(expr) => ev.eval(expr, ctx)?.to_bool().ok_or_else(|| EvalError::new("TOCSV IncludeHeaders must be Boolean"))?,
        None => true,
    };
    let limit = row_limit(max_rows, table.rows.len());

    let mut out = String::new();
    let mut first_line = true;
    if include_headers {
        let mut line = String::new();
        for (index, column) in table.columns.iter().enumerate() {
            if index > 0 {
                line.push_str(&delimiter);
            }
            line.push_str(&csv_field(&column.name, &delimiter));
        }
        if line.len() > MAX_TEXT_LEN {
            return Err(EvalError::new("TOCSV result exceeds the maximum supported length"));
        }
        push_checked(&mut out, &line)?;
        first_line = false;
    }
    for row in table.rows.iter().take(limit) {
        let mut line = String::new();
        for index in 0..table.columns.len() {
            if index > 0 {
                line.push_str(&delimiter);
            }
            let text = row.get(index).map(csv_value).unwrap_or_default();
            line.push_str(&csv_field(&text, &delimiter));
        }
        if line.len() > MAX_TEXT_LEN {
            return Err(EvalError::new("TOCSV result exceeds the maximum supported length"));
        }
        if !first_line {
            push_checked(&mut out, "\r\n")?;
        }
        push_checked(&mut out, &line)?;
        first_line = false;
    }
    Ok(Scalar::String(out))
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            control if (control as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", control as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn json_value(value: &Scalar) -> String {
    match value {
        Scalar::Blank => "null".to_string(),
        Scalar::String(text) => json_string(text),
        Scalar::Boolean(true) => "true".to_string(),
        Scalar::Boolean(false) => "false".to_string(),
        Scalar::Integer(number) => number.to_string(),
        Scalar::Currency(scaled) => json_number(*scaled as f64 / 10_000.0),
        Scalar::Decimal(number) => json_number(*number),
        Scalar::DateTime(serial) => json_string(&iso_datetime(*serial)),
    }
}

fn json_number(value: f64) -> String {
    if value.is_finite() { number_text(value) } else { "null".to_string() }
}

fn to_json(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let table_arg = args.first().ok_or_else(|| EvalError::new("TOJSON needs a table"))?;
    let table = eval_table(ev, table_arg, ctx)?;
    let max_rows = max_rows_argument(ev, args, 1, ctx)?;
    // The engine returns no result at all for an empty table, unlike TOCSV, which still prints
    // the header row.
    if table.rows.is_empty() {
        return Ok(Scalar::Blank);
    }
    let limit = row_limit(max_rows, table.rows.len());

    let mut out = String::from("{\"header\":[");
    for (index, column) in table.columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_checked(&mut out, &json_string(&column.name))?;
    }
    push_checked(&mut out, "],\"rowCount\":")?;
    push_checked(&mut out, &table.rows.len().to_string())?;
    push_checked(&mut out, ",\"data\":[")?;
    for (row_index, row) in table.rows.iter().take(limit).enumerate() {
        if row_index > 0 {
            out.push(',');
        }
        out.push('[');
        for index in 0..table.columns.len() {
            if index > 0 {
                out.push(',');
            }
            let value = row.get(index).map(json_value).unwrap_or_else(|| "null".to_string());
            push_checked(&mut out, &value)?;
        }
        out.push(']');
    }
    push_checked(&mut out, "]}")?;
    Ok(Scalar::String(out))
}
