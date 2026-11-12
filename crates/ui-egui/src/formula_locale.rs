//! UI formula syntax boundary. Engine commands, control/API callers and XLSX keep English.
use crate::i18n::Language;
use gridcraft_engine::formula::{self, FormulaLocale, ParseError};
use gridcraft_engine::model::Workbook;

/// Converts editor input to canonical engine syntax using the selected formula locale.
/// Preserves quoted text and explicit Text formatting, and resolves workbook names before aliases.
pub(crate) fn canonical(
    text: &str,
    locale: FormulaLocale,
    wb: &Workbook,
    sheet: usize,
    cell: gridcraft_engine::core::CellRef,
) -> Result<String, ParseError> {
    if locale == FormulaLocale::En || wb.sheet(sheet).is_some_and(|sh| wb.styles.get(sh.style_id(cell)).num_fmt.as_str() == "@") {
        return Ok(text.to_string());
    }
    // The engine's leading =/+/- convention, with localized identifiers (no trimming).
    let body = text;
    if is_formula_entry(body) {
        let known = |name: &str| wb.name(name, sheet).is_some() || wb.table(name).is_some();
        // Match the engine's existing convenience for missing closing parentheses.
        let expr = formula::parse_closing_parens(body, |text| formula::parse_input(text, locale, known));
        return Ok(format!("={}", formula::print(&expr?)));
    }
    if let Some(error) = locale.parse_error(text.trim()) {
        return Ok(error.as_str().to_string());
    }
    if let Some(value) = localized_boolean(text.trim(), locale) {
        return Ok(value.to_string());
    }
    // Decimal-comma number entry follows the same preference, without rewriting text or dates.
    let normalized = body.trim().replace(locale.decimal_separator(), ".");
    let number = normalized.strip_suffix('%').unwrap_or(&normalized);
    if number.parse::<f64>().is_ok_and(f64::is_finite) {
        return Ok(normalized);
    }
    Ok(text.to_string())
}

/// Recognizes explicit formulas and signed expression starts without treating ordinary signed numbers as formulas.
fn is_formula_entry(text: &str) -> bool {
    text.starts_with('=') || ((text.starts_with('+') || text.starts_with('-')) && text.chars().nth(1).is_some_and(|c| c.is_alphabetic() || c == '('))
}

/// The canonical `TRUE`/`FALSE` this locale spells `text` as, in any case; `None` for other text.
fn localized_boolean(text: &str, locale: FormulaLocale) -> Option<&'static str> {
    ["TRUE", "FALSE"].into_iter().find(|value| locale.fold_alias(text) == locale.fold_alias(locale.function_name(value)))
}

/// Localizes a stored canonical formula for display, preserving the original text on parse failure.
/// Workbook names keep precedence over aliases. Explicit UDF qualifiers preserve call identity during edits.
pub(crate) fn display(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    display_with_prefixes(text, locale, wb, sheet)
}

fn display_with_prefixes(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    if locale == FormulaLocale::En {
        return text.to_string();
    }
    let Some(body) = text.strip_prefix('=') else { return text.to_string() };
    formula::parse(body)
        .and_then(|expr| formula::print_input(&expr, locale, |name| wb.name(name, sheet).is_some() || wb.table(name).is_some()))
        .map(|body| format!("={body}"))
        .unwrap_or_else(|_| text.to_string())
}

/// Display spelling of Boolean/error constants, independent of the grid's layout and formatting.
pub(crate) fn localized_constant(value: &gridcraft_engine::core::Value, locale: FormulaLocale) -> Option<&'static str> {
    use gridcraft_engine::core::Value;
    match value {
        Value::Error(error) => Some(locale.error_name(*error)),
        Value::Bool(value) => Some(locale.function_name(if *value { "TRUE" } else { "FALSE" })),
        _ => None,
    }
}

/// Localizes a typed value without reinterpreting literal cell text on an unchanged edit.
pub(crate) fn edit_text(
    wb: &Workbook,
    sheet: &gridcraft_engine::model::Sheet,
    cell: gridcraft_engine::core::CellRef,
    locale: FormulaLocale,
) -> String {
    use gridcraft_engine::core::Value;
    let raw = gridcraft_engine::display::edit_text(wb, sheet, cell);
    if locale == FormulaLocale::En || wb.styles.get(sheet.style_id(cell)).num_fmt.as_str() == "@" {
        return raw;
    }
    match sheet.value(cell) {
        Value::Text(text)
            if !raw.starts_with('\'')
                && (is_formula_entry(&text) || localized_boolean(text.trim(), locale).is_some() || locale.parse_error(text.trim()).is_some()) =>
        {
            format!("'{raw}")
        }
        value => localized_constant(&value, locale).map_or(raw, str::to_string),
    }
}

fn hint_function_name(name: &str, locale: FormulaLocale) -> &str {
    if name.get(..6).is_some_and(|prefix| prefix.eq_ignore_ascii_case("_xlfn.")) { &name[6..] } else { locale.canonical_name(name) }
}

/// A function's argument hint. The name and separators follow the formula syntax; the argument
/// names follow the interface language, all of them or none, so a hint never mixes two languages.
pub(crate) fn signature(name: &str, locale: FormulaLocale, language: Language) -> Option<String> {
    canonical_signature(hint_function_name(name, locale), locale, language)
}

/// Formats a hint for a function already identified by its engine name.
pub(crate) fn canonical_signature(name: &str, locale: FormulaLocale, language: Language) -> Option<String> {
    let english = crate::formula_bar::function_signature(name)?;
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
    canonical_description(hint_function_name(name, locale), language)
}

/// Translates the description of a function already identified by its engine name.
pub(crate) fn canonical_description(name: &str, language: Language) -> Option<String> {
    let english = crate::formula_bar::function_description(name)?;
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
pub(crate) fn completion_name<'a>(name: &'a str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> std::borrow::Cow<'a, str> {
    locale.unshadowed_function_name(name, |alias| wb.name(alias, sheet).is_some() || wb.table(alias).is_some())
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
        // The expanded Spanish help uses the same catalog and keeps optional/repeated arguments.
        let sumifs = sig("SUMIFS", Es, Language::Es);
        let sum_range = Language::Es.function_argument("sum_range").unwrap();
        let criteria_range = Language::Es.function_argument("criteria_range").unwrap();
        let criteria = Language::Es.function_argument("criteria").unwrap();
        assert!(sumifs.ends_with(&format!("({sum_range}; {criteria_range}1; {criteria}1; ...)")), "{sumifs}");
        let english = crate::formula_bar::function_description("SUMIFS").unwrap();
        assert_eq!(description("SUMIFS", Es, Language::Es).as_deref(), Language::Es.function_description(&english));
        assert_eq!(sig("IF", En, Language::En), "IF(logical_test, [value_if_true], [value_if_false])");
    }
}

#[cfg(test)]
mod russian_help_tests {
    use super::*;
    /// Reported tooltip gaps use catalog translations while preserving syntax and optional arguments.
    #[test]
    fn reported_tooltips_translate_descriptions_and_argument_lists() {
        for language in Language::ALL.into_iter().filter(|language| *language != Language::En) {
            for (name, russian_name) in [("XLOOKUP", "ПРОСМОТРX"), ("COLUMN", "СТОЛБЕЦ"), ("RADIANS", "РАДИАНЫ")] {
                let english = crate::formula_bar::function_description(name).unwrap();
                if matches!(language, Language::Da | Language::Sv | Language::Ka | Language::Lt) {
                    assert_eq!(description(name, FormulaLocale::Ru, language).as_deref(), Some(english.as_str()));
                    continue;
                }
                let expected = language.function_description(&english).unwrap();
                assert_ne!(expected, english);
                assert_eq!(description(name, FormulaLocale::Ru, language).as_deref(), Some(expected));
                let english_signature = crate::formula_bar::function_signature(name).unwrap();
                let translated = signature(name, FormulaLocale::Ru, language).unwrap();
                let arg = |key| language.function_argument(key).unwrap();
                // Independently spell the known signatures so swapped arguments or optionality fail.
                let expected = match name {
                    "XLOOKUP" => format!(
                        "{russian_name}({}; {}; {}; [{}]; [{}]; [{}])",
                        arg("lookup_value"),
                        arg("lookup_array"),
                        arg("return_array"),
                        arg("if_not_found"),
                        arg("match_mode"),
                        arg("search_mode")
                    ),
                    "COLUMN" => format!("{russian_name}([{}])", arg("reference")),
                    _ => format!("{russian_name}({})", arg("angle")), // The third case is RADIANS.
                };
                assert_eq!(translated, expected, "{language:?}: {name}");
                assert_eq!(signature(name, FormulaLocale::En, Language::En).as_deref(), Some(english_signature.as_str()));
            }
        }
    }

    /// Checks Russian help combines current catalog signatures with the chosen formula syntax.
    #[test]
    fn russian_help_uses_catalog_signature_and_selected_syntax() {
        let translated_number = Language::Ru.function_argument("number").unwrap();
        let expected = format!("СУММ({translated_number}1; [{translated_number}2]; ...)");
        assert_eq!(signature("сумм", FormulaLocale::Ru, Language::Ru), Some(expected));
        assert_eq!(signature("SUM", FormulaLocale::En, Language::Ru), Some(format!("SUM({translated_number}1, [{translated_number}2], ...)")));
        let english = crate::formula_bar::function_description("IF").unwrap();
        let expected = Language::Ru.function_description(&english);
        assert!(expected.is_some());
        assert_eq!(description("ЕСЛИ", FormulaLocale::Ru, Language::Ru).as_deref(), expected);
    }

    /// SUM's hint follows each help catalog and falls back for interface-only languages.
    #[test]
    fn sum_hint_uses_each_language_catalog() {
        for language in Language::ALL {
            let locale = language.formula_locale();
            let hint = signature("SUM", locale, language).unwrap();
            let help = description("SUM", locale, language).unwrap();
            let fallback = matches!(language, Language::En | Language::Da | Language::Sv | Language::Ka | Language::Lt);
            let number = if fallback { "number" } else { language.function_argument("number").unwrap() };
            let expected = match locale {
                FormulaLocale::Ru => format!("СУММ({number}1; [{number}2]; ...)"),
                FormulaLocale::Es | FormulaLocale::Pl => format!("SUMA({number}1; [{number}2]; ...)"),
                FormulaLocale::Fr => format!("SOMME({number}1; [{number}2]; ...)"),
                FormulaLocale::It => format!("SOMMA({number}1; [{number}2]; ...)"),
                FormulaLocale::PtBr => format!("SOMA({number}1; [{number}2]; ...)"),
                FormulaLocale::Tr => format!("TOPLA({number}1; [{number}2]; ...)"),
                FormulaLocale::De => format!("SUMME({number}1; [{number}2]; ...)"),
                FormulaLocale::En => format!("SUM({number}1, [{number}2], ...)"),
            };
            assert_eq!(hint, expected, "{language:?}");
            if fallback {
                assert_eq!(help, "Adds all the numbers in the arguments.", "{language:?}");
            } else {
                assert_ne!(help, "Adds all the numbers in the arguments.", "{language:?}");
            }
        }
        use gridcraft_engine::core::{CellError, Value};
        assert_eq!(localized_constant(&Value::Error(CellError::Name), FormulaLocale::Ru), Some("#ИМЯ?"));
    }
}

#[cfg(test)]
mod help_structure_tests {
    use super::*;

    /// Every translated hint preserves the canonical optional/repeated argument structure and locale syntax.
    #[test]
    fn translated_hints_preserve_catalog_structure() {
        for function in gridcraft_engine::cmd::formulas::function_list() {
            let name = function["name"].as_str().unwrap();
            let english = function["signature"].as_str().unwrap();
            let (_, arguments) = split_signature(english).unwrap();
            for language in Language::ALL {
                let locale = language.formula_locale();
                let hint = canonical_signature(name, locale, language).unwrap();
                let separators = arguments.len().saturating_sub(1);
                assert_eq!(hint.matches(locale.list_separator()).count(), separators, "{language:?}: {name}: {hint}");
                for punctuation in ['[', ']'] {
                    assert_eq!(hint.matches(punctuation).count(), english.matches(punctuation).count(), "{language:?}: {name}: {hint}");
                }
                assert_eq!(hint.matches("...").count(), english.matches("...").count(), "{language:?}: {name}: {hint}");
                assert!(hint.starts_with(&format!("{}(", locale.function_name(name))), "{language:?}: {name}: {hint}");
            }
        }
    }
}

#[cfg(test)]
mod explicit_hint_tests {
    use super::*;
    #[test]
    fn explicit_french_trim_keeps_its_canonical_hint() {
        let mut editor = crate::editor::EditState::new(1, 0, gridcraft_engine::core::CellRef::new(0, 0), "=_xlfn.TRIM(".into(), true, false);
        editor.locale = FormulaLocale::Fr;
        let (name, _) = editor.current_function().unwrap();
        assert_eq!(name, "_xlfn.TRIM");
        assert_eq!(signature(&name, FormulaLocale::Fr, Language::En), canonical_signature("TRIM", FormulaLocale::Fr, Language::En));
        assert_eq!(description(&name, FormulaLocale::Fr, Language::En), canonical_description("TRIM", Language::En));
    }
}
