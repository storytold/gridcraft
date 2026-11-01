//! Shared dynamic UI error messages, translated by any language catalog. Captured workbook data is not translated or reinterpreted as a template.

const TEMPLATES: &[&str] = &[
    "{function} has too few arguments. Use {signature}",
    "{function} has too many arguments. Use {signature}",
    "no sheet named `{sheet}`",
    "not a range: `{range}`",
    "missing `{parameter}`",
    "`{cell}` for `{key}` names a sheet other than the one this command works on",
    "unknown command `{command}`",
    "formula is longer than {limit} characters",
    "bad number `{number}`",
    "number too large `{number}`",
    "unexpected character `{character}`",
    "invalid cell reference `{cell}` for `{key}`",
    "expected `,` or `)`, found {token}",
    "expected a reference after the sheet name, found {token}",
    "expected a number in array, found {token}",
    "array constants may only contain constants, found {token}",
    "expected {expected}, found {actual}",
    "unexpected {token}",
];

/// Matches an entire known message. Unknown messages remain untouched by the caller.
pub(super) fn format_message(message: &str, language: super::Language) -> Option<String> {
    if let Some(detail) = message.strip_prefix("There's a problem with this formula: ") {
        let detail = language.translation(detail).map(str::to_owned).or_else(|| translate_template(detail, language))?;
        return Some(format!("{} {detail}", language.translation("There's a problem with this formula:")?));
    }
    translate_template(message, language)
}

/// Tries translated templates in order and returns the first whole-message match.
fn translate_template(message: &str, language: super::Language) -> Option<String> {
    TEMPLATES.iter().find_map(|source| fill(message, source, language.translation(source)?, language))
}

/// Resolves named slots once so braces inside a filename or sheet name stay literal.
fn fill(message: &str, source: &str, translated: &str, language: super::Language) -> Option<String> {
    let mut rest = message;
    let mut pattern = source;
    let mut values = Vec::new();
    while let Some((prefix, slot)) = pattern.split_once('{') {
        rest = rest.strip_prefix(prefix)?;
        let (name, tail) = slot.split_once('}')?;
        let delimiter = tail.split('{').next()?;
        let end = if delimiter.is_empty() { rest.len() } else { rest.find(delimiter)? };
        values.push((name, rest.get(..end)?));
        rest = rest.get(end..)?;
        pattern = tail;
    }
    if rest != pattern {
        return None;
    }
    let locale = language.formula_locale();
    let mut rendered = Vec::with_capacity(values.len());
    for (name, value) in &values {
        let value = match *name {
            "function" => locale.function_name(value).to_string(),
            "signature" => {
                let function = values.iter().find(|(key, _)| *key == "function")?.1;
                crate::formula_locale::signature(function, locale, language).unwrap_or_else(|| value.to_string())
            }
            _ => value.to_string(),
        };
        rendered.push((*name, value));
    }
    let args: Vec<(&str, &str)> = rendered.iter().map(|(key, value)| (*key, value.as_str())).collect();
    Some(craft_i18n::fmt(translated, &args))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks captured parameters remain literal and partial or unknown messages are not translated.
    #[test]
    fn parameters_are_literal_and_messages_are_anchored() {
        assert_eq!(format_message("bad number `abc`", super::super::Language::En), None);
        assert_eq!(
            fill("bad number `abc`", "bad number `{number}`", "Nombre incorrect : {number}", super::super::Language::Fr),
            Some("Nombre incorrect : abc".into())
        );
        assert_eq!(
            format_message("invalid cell reference `Лист{key}` for `cell`", super::super::Language::Ru),
            Some("Неверная ссылка на ячейку `Лист{key}` в параметре `cell`".into())
        );
        assert_eq!(format_message("prefix: bad number `abc`", super::super::Language::Ru), None);
        assert_eq!(format_message("bad number `abc` suffix", super::super::Language::Ru), None);
        assert_eq!(format_message("Printer Office: permission denied", super::super::Language::Ru), None);
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    /// Checks every available template translation preserves the source set of named placeholders.
    #[test]
    fn translated_templates_preserve_slots_for_every_catalog() {
        let names = |text: &str| {
            let mut names: Vec<&str> = text.split('{').skip(1).filter_map(|s| s.split_once('}').map(|(name, _)| name)).collect();
            names.sort();
            names.into_iter().map(str::to_owned).collect::<Vec<_>>()
        };
        for language in crate::i18n::Language::ALL {
            for source in TEMPLATES {
                if let Some(translated) = language.translation(source) {
                    assert_eq!(names(source), names(translated), "{language:?}: {source}");
                }
            }
        }
    }

    /// Checks real canonical parser errors are translated only at the UI boundary.
    #[test]
    fn actual_parser_diagnostics_are_translated_without_changing_the_parser() {
        for input in ["=1+", "=#UNKNOWN!", "=1e999", "=SUM(1;2)", "=SUM({1;})"] {
            let error = gridcraft_engine::formula::parse(input).unwrap_err().to_string();
            let translated = crate::i18n::Language::Ru.tr_error(&error);
            assert_ne!(translated, error, "{input}: {error}");
        }
    }
}
