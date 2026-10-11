//! Language and regional settings.
//!
//! Stored text is canonical (en-US): `Formula::text`, XLSX, the journal and every parameter
//! without a `*Local` suffix. This module is the single path between that and what a person
//! types or reads: formulas ([`to_local_formula`], [`from_local_formula`]), number format codes
//! and the session's resolved [`Locale`].

use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_formula::{Expr, KnownNames, ParseError};
use gridcraft_locale::{DateOrder, Dialect, Locale, Regional};
use gridcraft_model::{Cell, Workbook};
use serde_json::{Value as Json, json};

use crate::{EngineError, Prefs, Result, Session, UiRequest};

// ---------------------------------------------------------------- formula conversion

/// Longest formula text accepted or converted, in characters (Excel's limit).
pub const MAX_FORMULA_CHARS: usize = 8192;

/// Whether `text` is longer than [`MAX_FORMULA_CHARS`] (stops counting at the limit).
fn too_long(text: &str) -> bool {
    text.chars().take(MAX_FORMULA_CHARS + 1).count() > MAX_FORMULA_CHARS
}

/// Rejects formula text over [`MAX_FORMULA_CHARS`] before it is lexed or copied.
pub fn check_formula_len(text: &str) -> Result<()> {
    if too_long(text) {
        return Err(EngineError::InvalidFormula(format!("the formula is longer than {MAX_FORMULA_CHARS} characters")));
    }
    Ok(())
}

/// Parses a formula body (no leading `=`) in `d`, with the workbook names `known`. Missing
/// closing parentheses are added, as Excel does when a formula is entered.
pub fn parse_closing(body: &str, d: &Dialect, known: KnownNames<'_>) -> std::result::Result<Expr, ParseError> {
    let mut text = body.to_string();
    let mut parsed = gridcraft_formula::parse_local_with(&text, d, known);
    for _ in 0..8 {
        if parsed.is_ok() {
            break;
        }
        text.push(')');
        parsed = gridcraft_formula::parse_local_with(&text, d, known);
    }
    parsed
}

/// A canonical formula body (`SUM(1.5,2)`) spelled in `d` (`SOMA(1,5;2)`), read back the same
/// with the workbook names `known`. A body that does not parse is returned unchanged.
pub fn to_local_body(canonical: &str, d: &Dialect, known: KnownNames<'_>) -> String {
    // Text over the limit can't have been entered; it is shown as stored instead of converted.
    if d.is_invariant() || too_long(canonical) {
        return canonical.to_string();
    }
    match gridcraft_formula::parse(canonical) {
        Ok(e) => gridcraft_formula::print_local_with(&e, d, known),
        Err(_) => canonical.to_string(),
    }
}

/// A formula body written in `d`, with the workbook names `known`, as canonical text.
pub fn from_local_body(local: &str, d: &Dialect, known: KnownNames<'_>) -> Result<String> {
    check_formula_len(local)?;
    let expr = parse_closing(local, d, known).map_err(|e| EngineError::InvalidFormula(e.to_string()))?;
    Ok(gridcraft_formula::print(&expr))
}

/// `=SUM(1.5,2)` → `=SOMA(1,5;2)`. Text that is not a formula is returned unchanged.
pub fn to_local_formula(canonical: &str, d: &Dialect, known: KnownNames<'_>) -> String {
    match canonical.strip_prefix('=') {
        Some(body) => format!("={}", to_local_body(body, d, known)),
        None => canonical.to_string(),
    }
}

/// The inverse of [`to_local_formula`]; a formula that does not parse is an
/// [`EngineError::InvalidFormula`]. Text that is not a formula is returned unchanged.
pub fn from_local_formula(local: &str, d: &Dialect, known: KnownNames<'_>) -> Result<String> {
    match local.strip_prefix('=') {
        Some(body) => Ok(format!("={}", from_local_body(body, d, known)?)),
        None => Ok(local.to_string()),
    }
}

/// A value spelled as it is typed in `loc`: the region's decimal separator and the formula
/// language's booleans and errors; text as it is. An array is spelled as an array constant
/// with the region's array separators (`{1,5.2,5;3.4}` in de-DE), its text elements quoted.
pub fn value_local(v: &Value, loc: &Locale) -> String {
    match v {
        Value::Text(t) => t.to_string(),
        Value::Error(e) => loc.formula.local_error(e.as_str()).to_string(),
        Value::Array(a) if a.data.len() == 1 => a.data.first().map(|e| value_local(e, loc)).unwrap_or_default(),
        Value::Array(a) if a.data.len() > 1 => {
            let r = &loc.regional;
            let mut out = String::from("{");
            for row in 0..a.rows {
                if row > 0 {
                    out.push(r.array_row);
                }
                for col in 0..a.cols {
                    if col > 0 {
                        out.push(r.array_col);
                    }
                    match a.data.get(row * a.cols + col) {
                        Some(Value::Text(t)) => {
                            out.push('"');
                            out.push_str(&t.replace('"', "\"\""));
                            out.push('"');
                        }
                        Some(e) => out.push_str(&value_local(e, loc)),
                        None => {}
                    }
                }
            }
            out.push('}');
            out
        }
        v => v.to_text_in(loc).unwrap_or_else(|_| v.display()),
    }
}

/// What the formula bar shows for a cell in `loc`: the local formula, or the value spelled by
/// [`value_local`].
pub fn input_local(cell: &Cell, loc: &Locale, known: KnownNames<'_>) -> String {
    match &cell.formula {
        Some(f) => to_local_formula(&format!("={}", f.text), &loc.dialect(), known),
        None => value_local(&cell.value, loc),
    }
}

/// [`input_local`] for the cell at `c` of `sh`. A cell picture has no input text (like
/// `Sheet::input_text`), so its `#VALUE!` fallback is never edited, searched or replaced.
pub fn sheet_input_local(sh: &gridcraft_model::Sheet, c: CellRef, loc: &Locale, known: KnownNames<'_>) -> String {
    if sh.cell_pictures.contains_key(&c) {
        return String::new();
    }
    sh.cell(c).map(|cell| input_local(cell, loc, known)).unwrap_or_default()
}

/// The text of a formula-valued parameter pair: `<key>Local` (spelled in `loc`, with the
/// workbook names `known`) wins over `<key>`. A local value is always converted (with or without
/// a leading `=`) and comes back with the `=`; a canonical value is returned as given.
pub(crate) fn canonical_formula_param(p: &Json, key: &str, loc: &Locale, known: KnownNames<'_>) -> Result<Option<String>> {
    if let Some(t) = p.get(format!("{key}Local")).and_then(Json::as_str) {
        let body = t.trim_start().strip_prefix('=').unwrap_or(t);
        return Ok(Some(format!("={}", from_local_body(body, &loc.dialect(), known)?)));
    }
    Ok(p.get(key).and_then(Json::as_str).map(str::to_string))
}

/// Text typed into a cell in `loc`, as the canonical input text a replay in any locale reads
/// back to the same cell: formulas in canonical syntax, numbers, dates and booleans spelled
/// the en-US way. Text stays as typed.
///
/// The second item is the number format typing the text applies automatically, when the
/// canonical text can't reproduce it (the region's currency: `R$ 3,5` is the number `3.5`
/// with the format `"R$" #,##0.00`, which en-US would read as text). The caller journals it
/// as a separate format step.
pub(crate) fn canonical_input(
    local: &str,
    loc: &Locale,
    sys: gridcraft_core::DateSystem,
    known: KnownNames<'_>,
) -> Result<(String, Option<&'static str>)> {
    let signed_formula = local.len() > 1
        && (local.starts_with('+') || local.starts_with('-'))
        && gridcraft_core::parse::parse_number_text_in(local, sys, &loc.regional).is_none()
        && local.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic() || c == '(');
    if local.starts_with('=') || signed_formula {
        let body = local.strip_prefix('=').unwrap_or(local);
        return Ok((format!("={}", from_local_body(body, &loc.dialect(), known)?), None));
    }
    let parsed = gridcraft_core::parse::parse_input_in(local, sys, &loc.regional, loc.formula);
    let inv = &gridcraft_locale::INVARIANT;
    let plain = |v: &Value| v.to_text_in(inv).unwrap_or_else(|_| v.display());
    Ok(match (&parsed.value, parsed.format) {
        (Value::Text(_) | Value::Empty, _) => (local.to_string(), None),
        (v, Some(code)) => {
            let text = gridcraft_numfmt::format_value_in(v, &crate::display::number_format(code), sys, inv).text;
            let again = gridcraft_core::parse::parse_input_in(&text, sys, &inv.regional, inv.formula);
            if again.value == *v && again.format == Some(code) { (text, None) } else { (plain(v), Some(code)) }
        }
        (v, None) => (plain(v), None),
    })
}

/// The canonical text of a conditional-format operand (`value`/`value2`) of `rule`, as stored in
/// the rule: a number, a formula body, or a quoted string. `<key>Local` is read like typed
/// input (numbers by the region of `loc`, `=…` as a local formula, anything else as text) and
/// may be a JSON number; `<key>` is canonical.
pub(crate) fn cf_operand(rule: &Json, key: &str, loc: &Locale, sys: gridcraft_core::DateSystem, known: KnownNames<'_>) -> Result<Option<String>> {
    match rule.get(format!("{key}Local")) {
        Some(Json::Number(n)) => return Ok(n.as_f64().map(gridcraft_core::number_to_text)),
        Some(Json::String(t)) => {
            if let Some(n) = gridcraft_core::parse::parse_number_text_in(t, sys, &loc.regional) {
                return Ok(Some(gridcraft_core::number_to_text(n)));
            }
            return match t.strip_prefix('=') {
                Some(body) => from_local_body(body, &loc.dialect(), known).map(Some),
                None => Ok(Some(format!("\"{}\"", t.replace('"', "\"\"")))),
            };
        }
        _ => {}
    }
    let Some(v) = rule.get(key).map(crate::cmd::edit::json_to_input) else { return Ok(None) };
    Ok(Some(if gridcraft_core::parse::parse_number_text(&v).is_some() || v.starts_with('=') {
        v.trim_start_matches('=').to_string()
    } else {
        format!("\"{}\"", v.replace('"', "\"\""))
    }))
}

/// The canonical `value`/`value2` parameter that makes [`cf_operand`] return `stored` again.
fn cf_operand_param(stored: &str) -> Json {
    if gridcraft_core::parse::parse_number_text(stored).is_some() { json!(stored) } else { json!(format!("={stored}")) }
}

/// Whether the cell at `at` now has number format `code`.
fn format_applied(s: &Session, sheet: usize, at: gridcraft_core::CellRef, code: &str) -> bool {
    let Ok(d) = s.doc() else { return false };
    d.wb.sheet(sheet).and_then(|sh| sh.cell(at)).is_some_and(|c| d.wb.styles.get(c.style).num_fmt.as_str() == code)
}

/// The calls to journal for one command call: the command with canonical params, followed by
/// the number-format steps its typed input applied that canonical text can't carry. `names` is
/// the workbook as the command found it: its names decided how local formula text read, even
/// when the command then defined new ones.
pub(crate) fn canonical_journal(s: &Session, id: &str, params: &Json, names: Option<&Workbook>) -> Vec<(String, Json)> {
    let mut formats = Vec::new();
    let main = canonical_params(s, id, params, names, &mut formats);
    let mut out = vec![(id.to_string(), main)];
    out.extend(formats);
    out
}

/// The params to journal for a command call: every `*Local` parameter (and `local` string
/// values of `range.setValues`, and the `*Local` keys of a conditional-format rule) rewritten
/// to its canonical twin, with the `locale` tag dropped, so a recorded script replays the same
/// way in any locale. Params without them are returned as they are, and so are params that
/// fail to convert (the command has already reported those). Number formats that typed input
/// applied and its canonical text lost are appended to `formats` as `home.numberFormat` calls.
fn canonical_params(s: &Session, id: &str, params: &Json, names: Option<&Workbook>, formats: &mut Vec<(String, Json)>) -> Json {
    let Some(obj) = params.as_object() else { return params.clone() };
    let local_values = id == "range.setValues" && obj.get("local").and_then(Json::as_bool) == Some(true);
    let cf_rule = (id == "home.conditionalFormat")
        .then(|| obj.get("rule").and_then(Json::as_object))
        .flatten()
        .filter(|r| r.keys().any(|k| k.ends_with("Local")));
    if !local_values && cf_rule.is_none() && !obj.keys().any(|k| k.ends_with("Local")) {
        return params.clone();
    }
    let Ok(loc) = call_locale(s, params) else { return params.clone() };
    let sys = s.active().map(|d| d.wb.date_system).unwrap_or(gridcraft_core::DateSystem::D1900);
    let sheet = crate::cmd::target_sheet(s, params).ok();
    let known = |n: &str| names.zip(sheet).is_some_and(|(wb, si)| wb.knows_name(n, si));
    let mut out = obj.clone();
    out.remove("locale");
    // (cell, format) pairs typed input applied; checked against the workbook afterwards.
    let mut carried: Vec<(gridcraft_core::CellRef, &'static str)> = Vec::new();
    let mut carried_sheet = None;
    let converted = (|| -> Result<()> {
        for (key, value) in obj {
            let (Some(base), Some(text)) = (key.strip_suffix("Local"), value.as_str()) else { continue };
            out.remove(key);
            match base {
                "input" => {
                    let (canonical, format) = canonical_input(text, &loc, sys, &known)?;
                    out.insert("input".into(), json!(canonical));
                    if let (Some(code), "cell.set") = (format, id) {
                        let at = crate::cmd::cell_param(params, "cell").or_else(|| s.doc().ok().map(|d| d.selection.active));
                        carried_sheet = crate::cmd::target_sheet(s, params).ok();
                        carried.extend(at.map(|a| (a, code)));
                    }
                }
                "numberFormat" => {
                    let code = from_local_format(text, &loc.dialect());
                    match id {
                        "home.numberFormat" => out.insert("code".into(), json!(code)),
                        "home.formatCells" => {
                            let mut style = out.get("style").cloned().unwrap_or_else(|| json!({}));
                            if let Some(o) = style.as_object_mut() {
                                o.insert("numFmt".into(), json!(code));
                            }
                            out.insert("style".into(), style)
                        }
                        _ => out.insert("numberFormat".into(), json!(code)),
                    };
                }
                "formula1" | "formula2" if id == "data.validation" => {
                    let list = obj.get("type").and_then(Json::as_str) == Some("list");
                    if let Some(f) = crate::cmd::data::validation_formula(params, base, &loc, sys, list, &known)? {
                        out.insert(base.into(), json!(f));
                    }
                }
                _ => {
                    if let Some(f) = canonical_formula_param(params, base, &loc, &known)? {
                        out.insert(base.into(), json!(f));
                    }
                }
            }
        }
        if let Some(rule) = cf_rule {
            let mut canonical = rule.clone();
            for key in ["value", "value2"] {
                if canonical.remove(&format!("{key}Local")).is_some()
                    && let Some(stored) = cf_operand(&Json::Object(rule.clone()), key, &loc, sys, &known)?
                {
                    canonical.insert(key.into(), cf_operand_param(&stored));
                }
            }
            if canonical.remove("formulaLocal").is_some()
                && let Some(f) = canonical_formula_param(&Json::Object(rule.clone()), "formula", &loc, &known)?
            {
                canonical.insert("formula".into(), json!(f));
            }
            out.insert("rule".into(), Json::Object(canonical));
        }
        if local_values {
            out.remove("local");
            carried_sheet = crate::cmd::target_sheet(s, params).ok();
            let start = crate::cmd::target_range(s, params).ok().map(|r| r.start);
            if let Some(rows) = obj.get("values").and_then(Json::as_array) {
                let mut canonical = Vec::with_capacity(rows.len());
                for (ri, row) in rows.iter().enumerate() {
                    let mut cell = |ci: usize, v: &Json| -> Result<Json> {
                        let Some(t) = v.as_str() else { return Ok(v.clone()) };
                        let (text, format) = canonical_input(t, &loc, sys, &known)?;
                        if let (Some(code), Some(at)) = (format, start.and_then(|st| st.offset(ri as i64, ci as i64))) {
                            carried.push((at, code));
                        }
                        Ok(json!(text))
                    };
                    canonical.push(match row.as_array() {
                        Some(cells) => Json::Array(cells.iter().enumerate().map(|(ci, v)| cell(ci, v)).collect::<Result<Vec<_>>>()?),
                        None => cell(0, row)?,
                    });
                }
                out.insert("values".into(), Json::Array(canonical));
            }
        }
        Ok(())
    })();
    if converted.is_err() {
        return params.clone();
    }
    if let Some(sheet) = carried_sheet {
        for (at, code) in carried {
            if format_applied(s, sheet, at, code) {
                formats.push(("home.numberFormat".into(), json!({"sheet": sheet, "range": at.a1(), "code": code})));
            }
        }
    }
    Json::Object(out)
}

/// App-created content text in the UI language, with `{name}` placeholders replaced.
/// `{name:lower}` inserts the value lower-cased, for languages that build compounds
/// (de `Gesamt{label:lower}` → `Gesamtmittelwert`). Unknown placeholders and modifiers are
/// kept verbatim; values are never re-scanned.
pub fn content_fmt(loc: &Locale, key: &str, args: &[(&str, &str)]) -> String {
    let mut rest = loc.ui.content(key);
    let mut out = String::with_capacity(rest.len());
    while let Some((head, tail)) = rest.split_once('{') {
        out.push_str(head);
        match tail.split_once('}') {
            Some((placeholder, next)) => {
                let (name, modifier) = placeholder.split_once(':').unwrap_or((placeholder, ""));
                match (args.iter().find(|(n, _)| *n == name), modifier) {
                    (Some((_, value)), "") => out.push_str(value),
                    (Some((_, value)), "lower") => out.push_str(&value.to_lowercase()),
                    _ => {
                        out.push('{');
                        out.push_str(placeholder);
                        out.push('}');
                    }
                }
                rest = next;
            }
            None => {
                out.push('{');
                rest = tail;
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A canonical number format code spelled in `d` (`#,##0.00` → `#.##0,00`).
pub fn to_local_format(code: &str, d: &Dialect) -> String {
    gridcraft_numfmt::to_local_code(code, d)
}

/// The inverse of [`to_local_format`].
pub fn from_local_format(local: &str, d: &Dialect) -> String {
    gridcraft_numfmt::from_local_code(local, d)
}

/// The locale one call uses: the session's, or — with a `locale` tag parameter — that
/// language's formula names and that region's separators (the interface language is kept).
pub(crate) fn call_locale(s: &Session, p: &Json) -> Result<Locale> {
    let Some(tag) = p.get("locale").and_then(Json::as_str) else { return Ok(*s.locale) };
    let formula = gridcraft_locale::language(tag).ok_or_else(|| EngineError::InvalidLocale(format!("no language `{tag}`")))?;
    let regional = gridcraft_locale::region(tag).ok_or_else(|| EngineError::InvalidLocale(format!("no regional format `{tag}`")))?;
    Ok(Locale::new(s.locale.ui, formula, *regional))
}

// ---------------------------------------------------------------- session locale

/// Changes to the language settings; `None` keeps the current value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalePrefs {
    pub ui_language: Option<String>,
    pub formula_language: Option<String>,
    pub regional_format: Option<String>,
    pub use_system_separators: Option<bool>,
    pub decimal_separator: Option<char>,
    pub thousands_separator: Option<char>,
}

fn invalid(msg: impl Into<String>) -> EngineError {
    EngineError::InvalidLocale(msg.into())
}

/// Characters a custom decimal or thousands separator may be: the ones real regions use.
const SEPARATOR_CHARS: [char; 7] = ['.', ',', ' ', '\u{00A0}', '\u{202F}', '\'', '\u{2019}'];

fn separator(text: &str, what: &str) -> Result<char> {
    let mut it = text.chars();
    let c = match (it.next(), it.next()) {
        (Some(c), None) => c,
        _ => return Err(invalid(format!("the {what} separator must be exactly one character"))),
    };
    if !SEPARATOR_CHARS.contains(&c) {
        return Err(invalid(format!("the {what} separator must be one of `.` `,` space, a no-break space, `'` or `’`")));
    }
    Ok(c)
}

/// The custom decimal and thousands separators, validated as a pair.
fn separators(prefs: &Prefs) -> Result<(char, char)> {
    let decimal = separator(&prefs.decimal_separator, "decimal")?;
    let group = separator(&prefs.thousands_separator, "thousands")?;
    if decimal.is_whitespace() {
        return Err(invalid("the decimal separator can't be a space"));
    }
    if decimal == group {
        return Err(invalid("the decimal and thousands separators must differ"));
    }
    Ok((decimal, group))
}

/// Resets, field by field, the locale preferences that can't resolve (a language this build
/// lacks, bad separators: a hand-edited or older `prefs.json` / `localStorage`) to their defaults,
/// so one stale field neither leaves the session on en-US nor makes every later
/// [`Session::set_locale`] fail.
fn repair_locale_prefs(prefs: &mut Prefs) {
    let defaults = Prefs::default();
    if prefs.ui_language != "system" && gridcraft_locale::language(&prefs.ui_language).is_none() {
        prefs.ui_language = defaults.ui_language;
    }
    if prefs.formula_language != "followUi" && gridcraft_locale::language(&prefs.formula_language).is_none() {
        prefs.formula_language = defaults.formula_language;
    }
    if prefs.regional_format != "system" && gridcraft_locale::region(&prefs.regional_format).is_none() {
        prefs.regional_format = defaults.regional_format;
    }
    if separators(prefs).is_err() {
        prefs.use_system_separators = defaults.use_system_separators;
        prefs.decimal_separator = defaults.decimal_separator;
        prefs.thousands_separator = defaults.thousands_separator;
    }
}

/// Resolves preferences against the system locale tag; the preferences are validated whole.
fn resolve(prefs: &Prefs, system: &str) -> Result<Locale> {
    let ui = if prefs.ui_language == "system" {
        gridcraft_locale::negotiate_language(system)
    } else {
        gridcraft_locale::language(&prefs.ui_language).ok_or_else(|| invalid(format!("no interface language `{}`", prefs.ui_language)))?
    };
    let formula = if prefs.formula_language == "followUi" {
        ui
    } else {
        gridcraft_locale::language(&prefs.formula_language).ok_or_else(|| invalid(format!("no formula language `{}`", prefs.formula_language)))?
    };
    let base: Regional = if prefs.regional_format == "system" {
        *gridcraft_locale::negotiate_region(system)
    } else {
        *gridcraft_locale::region(&prefs.regional_format).ok_or_else(|| invalid(format!("no regional format `{}`", prefs.regional_format)))?
    };
    // The custom separators are validated even while the system ones are in use, so a bad value
    // can't be stored and surface later.
    let (decimal, group) = separators(prefs)?;
    let regional = if prefs.use_system_separators { base } else { base.with_separators(decimal, group) };
    Ok(Locale::new(ui, formula, regional))
}

impl Session {
    /// The resolved locale every document uses (interface, formula language, region).
    pub fn locale(&self) -> Arc<Locale> {
        self.locale.clone()
    }

    /// The OS or browser locale tag given to [`Session::set_system_locale`].
    pub fn system_locale(&self) -> &str {
        &self.system_locale
    }

    /// The OS or browser locale tag the apps detect (`pt_BR.UTF-8`, `de-AT`), given once the saved
    /// preferences are in `prefs`; the `"system"` preferences resolve through it. Repairs saved
    /// locale preferences that no longer resolve, then re-resolves and applies like
    /// [`Session::set_locale`] when that changes the result.
    pub fn set_system_locale(&mut self, tag: &str) {
        self.system_locale = tag.to_string();
        repair_locale_prefs(&mut self.prefs);
        if let Ok(loc) = resolve(&self.prefs, &self.system_locale)
            && loc != *self.locale
        {
            let prefs = self.prefs.clone();
            self.apply_locale(prefs, loc);
        }
    }

    /// Validates everything first; then stores the preferences, gives the locale to every open
    /// document (new, opened and restored ones get it too), recalculates in automatic mode
    /// (manual mode keeps its stale values until the next calculation) and pushes
    /// [`UiRequest::LocaleChanged`]. An invalid setting changes nothing.
    pub fn set_locale(&mut self, p: LocalePrefs) -> Result<()> {
        let mut prefs = self.prefs.clone();
        if let Some(v) = p.ui_language {
            prefs.ui_language = v;
        }
        if let Some(v) = p.formula_language {
            prefs.formula_language = v;
        }
        if let Some(v) = p.regional_format {
            prefs.regional_format = v;
        }
        if let Some(v) = p.use_system_separators {
            prefs.use_system_separators = v;
        }
        if let Some(v) = p.decimal_separator {
            prefs.decimal_separator = v.to_string();
        }
        if let Some(v) = p.thousands_separator {
            prefs.thousands_separator = v.to_string();
        }
        let loc = resolve(&prefs, &self.system_locale)?;
        let prefs_changed = (
            &prefs.ui_language,
            &prefs.formula_language,
            &prefs.regional_format,
            prefs.use_system_separators,
            &prefs.decimal_separator,
            &prefs.thousands_separator,
        ) != (
            &self.prefs.ui_language,
            &self.prefs.formula_language,
            &self.prefs.regional_format,
            self.prefs.use_system_separators,
            &self.prefs.decimal_separator,
            &self.prefs.thousands_separator,
        );
        if prefs_changed || loc != *self.locale {
            self.apply_locale(prefs, loc);
        }
        Ok(())
    }

    fn apply_locale(&mut self, prefs: Prefs, loc: Locale) {
        self.prefs = prefs;
        let loc = Arc::new(loc);
        for d in &mut self.docs {
            if *d.wb.locale != *loc {
                let recalc = d.wb.calc.mode != gridcraft_model::CalcMode::Manual;
                d.refresh_locale(&loc, recalc);
            }
        }
        self.locale = loc;
        self.ui_requests.push(UiRequest::LocaleChanged);
    }
}

fn date_order(o: DateOrder) -> &'static str {
    match o {
        DateOrder::Mdy => "mdy",
        DateOrder::Dmy => "dmy",
        DateOrder::Ymd => "ymd",
    }
}

/// The `app.getInternational` payload (Excel's `Application.International`).
pub fn international(s: &Session) -> Json {
    let l = &s.locale;
    let r = &l.regional;
    json!({
        "uiLanguage": l.ui.tag,
        "formulaLanguage": l.formula.tag,
        "regionalFormat": r.tag,
        "decimal": r.decimal.to_string(),
        "thousands": r.group.to_string(),
        "list": r.list.to_string(),
        "arrayColumn": r.array_col.to_string(),
        "arrayRow": r.array_row.to_string(),
        "dateOrder": date_order(r.date_order),
        "dateSeparator": r.date_sep.to_string(),
        "timeSeparator": r.time_sep.to_string(),
        "shortDate": gridcraft_numfmt::to_local_code(r.short_date, &l.dialect()),
        "longDate": gridcraft_numfmt::to_local_code(r.long_date, &l.dialect()),
        "r1c1": [l.formula.r1c1[0].to_string(), l.formula.r1c1[1].to_string()],
        "boolTrue": l.formula.bool_true,
        "boolFalse": l.formula.bool_false,
        "currency": r.currency,
        // What the "system" preferences resolve to: the tag the app detected and what it negotiates to.
        "systemLocale": s.system_locale,
        "systemLanguage": gridcraft_locale::negotiate_language(&s.system_locale).tag,
        "systemRegion": gridcraft_locale::negotiate_region(&s.system_locale).tag,
        "prefs": {
            "uiLanguage": s.prefs.ui_language,
            "formulaLanguage": s.prefs.formula_language,
            "regionalFormat": s.prefs.regional_format,
            "useSystemSeparators": s.prefs.use_system_separators,
            "decimalSeparator": s.prefs.decimal_separator,
            "thousandsSeparator": s.prefs.thousands_separator,
        },
    })
}
