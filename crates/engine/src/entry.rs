//! Interactive scalar entry. Commands and files use the invariant parser unless a front end
//! explicitly supplies a region for the one command being entered.
use gridcraft_core::{CellRef, Value};
use gridcraft_model::Workbook;
use gridcraft_numfmt::Locale;

/// Read an explicit command-entry snapshot; absent means invariant parsing. The small schema
/// is bounded and validated, independently of any saved document or front-end preferences.
pub(crate) fn locale_from_params(params: &serde_json::Value) -> crate::Result<Option<Locale>> {
    locale_from_params_named(params, "inputLocale")
}

/// Validate interactive entry and clipboard output with the same bounded schema.
pub(crate) fn locale_from_params_named(params: &serde_json::Value, name: &str) -> crate::Result<Option<Locale>> {
    params.get(name).map(|value| parse_locale_snapshot(value, name)).transpose()
}

/// Read numeric-region data from a command or a matching clipboard table.
pub(crate) fn parse_locale_snapshot(value: &serde_json::Value, name: &str) -> crate::Result<Locale> {
    let invalid = || {
        crate::EngineError::Other(format!(
            "{name} requires distinct decimal/group separators, a numeric shortDate (up to 255 characters), and an optional boolean percentPrefix."
        ))
    };
    let character = |key| {
        let mut chars = value.get(key).and_then(serde_json::Value::as_str)?.chars();
        let c = chars.next()?;
        (chars.next().is_none()
            && !c.is_alphanumeric()
            && !c.is_control()
            && !matches!(c, '\'' | '"' | '=' | '+' | '-' | '/' | ':' | ';' | '[' | ']' | '%' | '$' | '(' | ')'))
        .then_some(c)
    };
    let decimal = character("decimal").ok_or_else(invalid)?;
    // Apostrophes are valid Swiss digit grouping characters.
    let group = match value.get("group").and_then(serde_json::Value::as_str) {
        Some("'") => '\'',
        Some("’") => '’',
        _ => character("group").ok_or_else(invalid)?,
    };
    let date = value.get("shortDate").and_then(serde_json::Value::as_str).ok_or_else(invalid)?;
    if decimal == group
        || date.chars().take(256).count() > 255
        || date.is_empty()
        || !crate::display::number_format(date).has_numeric_calendar_order()
    {
        return Err(invalid());
    }
    let percent_prefix = match value.get("percentPrefix") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(invalid)?,
    };
    Ok(Locale { decimal, group, percent_prefix, short_date: std::borrow::Cow::Owned(date.to_string()) })
}

/// Capture numeric conventions independently of later host preference changes.
pub(crate) fn locale_snapshot(locale: &Locale) -> serde_json::Value {
    serde_json::json!({"decimal":locale.decimal.to_string(),"group":locale.group.to_string(),"shortDate":locale.short_date,"percentPrefix":locale.percent_prefix})
}

/// Canonical input for one destination, keeping Text cells and formulas untouched.
pub fn canonical_input(wb: &Workbook, sheet: usize, at: CellRef, text: &str, loc: &Locale) -> String {
    let Some(sh) = wb.sheet(sheet) else { return text.to_string() };
    let fmt = &wb.styles.get(sh.style_id(at)).num_fmt;
    if crate::display::number_format(fmt.as_str()).is_text() || crate::cmd::edit::is_formula_input(text) {
        return text.to_string();
    }
    let code = if !fmt.is_literal_short_date() && matches!(fmt.as_str(), "m/d/yyyy" | "m/d/yyyy h:mm") {
        loc.short_date.as_ref()
    } else if crate::display::number_format(fmt.as_str()).is_date() {
        fmt.as_str()
    } else {
        loc.short_date.as_ref()
    };
    loc.canonical_input(text, wb.date_system, code)
}

/// Editable regional text with full numeric precision. Use a formatted date only when the
/// regional entry path reads it back exactly; otherwise retain the existing safe ISO fallback.
pub fn editing_text(wb: &Workbook, sheet: usize, at: CellRef, loc: &Locale) -> String {
    let Some(sh) = wb.sheet(sheet) else { return String::new() };
    let raw = crate::display::edit_text(wb, sh, at);
    if loc == &Locale::EN_US {
        return raw;
    }
    let Some(cell) = sh.cell(at) else { return raw };
    if cell.formula.is_some() || cell.py.is_some() {
        return raw;
    }
    let fmt = &wb.styles.get(sh.style_id(at)).num_fmt;
    match &cell.value {
        Value::Text(text) if !crate::display::number_format(fmt.as_str()).is_text() => {
            let input = canonical_input(wb, sheet, at, &raw, loc);
            match gridcraft_core::parse::parse_input(&input, wb.date_system).value {
                Value::Text(ref t) if t.as_ref() == text.as_ref() => raw,
                _ => format!("'{raw}"),
            }
        }
        Value::Number(n) => {
            let reads_back = |text: &str| {
                let input = canonical_input(wb, sheet, at, text, loc);
                matches!(gridcraft_core::parse::parse_input(&input, wb.date_system).value, Value::Number(m) if m == *n)
            };
            if crate::display::number_format(fmt.as_str()).is_date() {
                let shown = crate::display::format_num_fmt_in(&cell.value, fmt, wb, loc).text;
                if reads_back(&shown) {
                    return shown;
                }
            }
            // ISO dates are already unambiguous. Ordinary numeric editor text has no grouping.
            let localized = if raw.parse::<f64>().is_ok() { raw.replace('.', &loc.decimal.to_string()) } else { raw.clone() };
            if reads_back(&localized) { localized } else { raw }
        }
        _ => raw,
    }
}
