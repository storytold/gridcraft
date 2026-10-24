//! UI formula syntax boundary. Engine commands, control/API callers and XLSX keep English.
use crate::i18n::Language;
use gridcraft_engine::formula::{self, FormulaLocale, ParseError};
use gridcraft_engine::model::Workbook;

pub(crate) fn canonical(
    text: &str,
    locale: FormulaLocale,
    wb: &Workbook,
    sheet: usize,
    cell: gridcraft_engine::core::CellRef,
) -> Result<String, ParseError> {
    // The editor hid the `_xludf.` prefixes of this cell's formula; confirming it unchanged keeps
    // the user-defined calls instead of binding them to the built-ins of the same name.
    if let Some(stored) = wb.sheet(sheet).and_then(|sh| sh.cell(cell)).filter(|c| c.formula.is_some()).map(|c| c.input_text())
        && has_udf_prefix(&stored)
        && text == display(&stored, locale, wb, sheet)
    {
        return Ok(stored);
    }
    if locale == FormulaLocale::En || wb.sheet(sheet).is_some_and(|sh| wb.styles.get(sh.style_id(cell)).num_fmt.as_str() == "@") {
        return Ok(text.to_string());
    }
    // Same formula detection as the engine's `input_to_cell` (no trimming: " =1" is text).
    let body = text;
    if body.starts_with('=')
        || ((body.starts_with('+') || body.starts_with('-')) && body.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic() || c == '('))
    {
        let known = |name: &str| wb.name(name, sheet).is_some() || wb.table(name).is_some();
        // Match the engine's existing convenience for missing closing parentheses.
        let expr = formula::parse_closing_parens(body, |text| formula::parse_input(text, locale, known));
        return Ok(format!("={}", formula::print(&expr?)));
    }
    // Decimal-comma number entry follows the same preference, without rewriting text or dates.
    let normalized = body.trim().replace(locale.decimal_separator(), ".");
    let number = normalized.strip_suffix('%').unwrap_or(&normalized);
    if number.parse::<f64>().is_ok_and(f64::is_finite) {
        return Ok(normalized);
    }
    Ok(text.to_string())
}

pub(crate) fn display(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    // English (and every language without a formula table) shows the stored text, `_xludf.`
    // prefixes aside, and skips a parse per frame.
    if locale == FormulaLocale::En {
        return hide_udf_prefix(text, locale);
    }
    let Some(body) = text.strip_prefix('=') else { return text.to_string() };
    let shown = formula::parse(body)
        .and_then(|expr| formula::print_input(&expr, locale, |name| wb.name(name, sheet).is_some() || wb.table(name).is_some()))
        .map(|body| format!("={body}"))
        .unwrap_or_else(|_| text.to_string());
    hide_udf_prefix(&shown, locale)
}

const UDF_PREFIX: &str = "_xludf.";

fn has_udf_prefix(text: &str) -> bool {
    text.to_ascii_lowercase().contains(UDF_PREFIX)
}

/// Excel shows a user-defined call such as `_xludf.EOMONTH()` as `=EOMONTH()`. Only function
/// names lose the prefix: text literals and names keep it.
fn hide_udf_prefix(text: &str, locale: FormulaLocale) -> String {
    let Some(body) = text.strip_prefix('=').filter(|b| has_udf_prefix(b)) else { return text.to_string() };
    let Ok(tokens) = formula::lexer::tokenize_locale(body, locale) else { return text.to_string() };
    let mut out = String::from("=");
    let mut from = 0;
    for t in tokens {
        if matches!(t.tok, formula::lexer::Tok::Func(_))
            && body.get(t.start..t.start + UDF_PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(UDF_PREFIX))
        {
            out.push_str(body.get(from..t.start).unwrap_or(""));
            from = t.start + UDF_PREFIX.len();
        }
    }
    out.push_str(body.get(from..).unwrap_or(""));
    out
}

/// A function's argument hint. The name and separators follow the formula syntax; the argument
/// names follow the interface language, all of them or none, so a hint never mixes two languages.
pub(crate) fn signature(name: &str, locale: FormulaLocale, language: Language) -> Option<String> {
    let english = crate::formula_bar::function_signature(locale.canonical_name(name))?;
    let Some((head, args)) = split_signature(&english) else { return Some(english) };
    let separator = format!("{} ", locale.list_separator());
    let args = match args.iter().map(|arg| translate_argument(arg, language)).collect::<Option<Vec<_>>>() {
        Some(translated) => translated.join(&separator),
        None => args.join(&separator),
    };
    Some(format!("{}({args})", locale.function_name(head)))
}

/// A function's description in the interface language (English while untranslated).
pub(crate) fn description(name: &str, locale: FormulaLocale, language: Language) -> Option<String> {
    let english = crate::formula_bar::function_description(locale.canonical_name(name))?;
    Some(language.function_description(&english).map_or(english, str::to_string))
}

/// `SUM(number1, [number2], ...)` -> `SUM` and its arguments as written.
fn split_signature(signature: &str) -> Option<(&str, Vec<&str>)> {
    let (head, rest) = signature.split_once('(')?;
    let inner = rest.strip_suffix(')')?;
    Some((head, if inner.is_empty() { Vec::new() } else { inner.split(", ").collect() }))
}

/// `[number2]` -> `number`. A bracket can open or close on its own (SWITCH's
/// `[default_or_value2, result2]`, LAMBDA's `[parameter1, ...]`).
fn argument_name(arg: &str) -> &str {
    let arg = arg.strip_prefix('[').unwrap_or(arg);
    let arg = arg.strip_suffix(']').unwrap_or(arg);
    arg.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// `[number2]` -> `[número2]`: brackets, index and `...` stay.
fn translate_argument(arg: &str, language: Language) -> Option<String> {
    let name = argument_name(arg);
    if name == "..." {
        return Some(arg.to_string());
    }
    // `arg` is `[`? + `name` + index + `]`?, so the first match is the name itself.
    Some(arg.replacen(name, language.function_argument(name)?, 1))
}

/// Avoid suggesting an alias that would call a workbook/table name instead of the function.
pub(crate) fn completion_name<'a>(name: &'a str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> &'a str {
    let alias = locale.function_name(name);
    if wb.name(alias, sheet).is_some() || wb.table(alias).is_some() { name } else { alias }
}

/// Engine-generated edit templates use canonical syntax. The only incomplete template is
/// the bare function opening emitted by Insert Function; typed user input never enters here.
pub(crate) fn display_template(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    if let Some(name) = text.strip_prefix('=').and_then(|s| s.strip_suffix('('))
        && !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
    {
        return format!("={}(", completion_name(name, locale, wb, sheet));
    }
    display(text, locale, wb, sheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hint for `name`, empty when there is none.
    fn sig(name: &str, locale: FormulaLocale, language: Language) -> String {
        signature(name, locale, language).unwrap_or_default()
    }

    /// Hints keep the formula syntax's name and separators and translate the argument names
    /// only when the interface language has them all.
    #[test]
    fn function_help_follows_the_interface_language_and_the_formula_syntax() {
        use FormulaLocale::{En, Es};
        // Spanish help over English formulas: the name and `,` stay as typed; brackets, indexes
        // and `...` survive.
        assert_eq!(sig("IF", En, Language::Es), "IF(prueba_lógica, [valor_si_verdadero], [valor_si_falso])");
        assert_eq!(sig("SUM", En, Language::Es), "SUM(número1, [número2], ...)");
        assert_eq!(description("IF", En, Language::Es).as_deref(), Some("Devuelve un valor según una condición."));
        // Spanish names and separators, from a localized name.
        assert_eq!(sig("suma", Es, Language::Es), "SUMA(número1; [número2]; ...)");
        assert_eq!(description("SUMA", Es, Language::Es).as_deref(), Some("Suma los números."));
        // An argument list Spanish covers only in part stays English instead of mixing (the name
        // is whatever the Spanish formula table calls SUMIFS).
        let sumifs = sig("SUMIFS", Es, Language::Es);
        assert!(sumifs.ends_with("(sum_range; criteria_range1; criteria1; ...)"), "{sumifs}");
        assert_eq!(description("SUMIFS", Es, Language::Es).as_deref(), Some("Adds the cells that meet every condition."));
        assert_eq!(sig("IF", En, Language::En), "IF(logical_test, [value_if_true], [value_if_false])");
    }

    /// Italian help over English and Italian formulas, as the Spanish one.
    #[test]
    fn italian_function_help_follows_the_interface_language_and_the_formula_syntax() {
        use FormulaLocale::{En, It};
        assert_eq!(sig("IF", En, Language::It), "IF(test_logico, [valore_se_vero], [valore_se_falso])");
        assert_eq!(sig("VLOOKUP", En, Language::It), "VLOOKUP(cerca_valore, matrice, indice_colonna, [ordinato])");
        assert_eq!(description("IF", En, Language::It).as_deref(), Some("Restituisce un valore in base a una condizione."));
        // Italian names and separators, from a localized name.
        assert_eq!(sig("se", It, Language::It), "SE(test_logico; [valore_se_vero]; [valore_se_falso])");
        assert_eq!(sig("somma", It, Language::It), "SOMMA(numero1; [numero2]; ...)");
        assert_eq!(description("SOMMA", It, Language::It).as_deref(), Some("Somma i numeri."));
        // The date keys sort with the space before the period: the lookup must still hit them.
        assert_eq!(description("TODAY", En, Language::It).as_deref(), Some("Restituisce la data odierna."));
        assert_eq!(description("NOW", En, Language::It).as_deref(), Some("Restituisce la data e l'ora correnti."));
        // An argument list Italian covers only in part stays English instead of mixing.
        let sumifs = sig("SOMMA.PIÙ.SE", It, Language::It);
        assert!(sumifs.ends_with("(sum_range; criteria_range1; criteria1; ...)"), "{sumifs}");
        assert_eq!(sig("IF", En, Language::En), "IF(logical_test, [value_if_true], [value_if_false])");
    }
}
