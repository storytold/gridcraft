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
    let values = super::unfill(source, message)?;
    for (name, value) in &values {
        let slot = format!("{{{name}}}");
        if value.contains('`') && (source.contains(&format!("`{slot}")) || source.contains(&format!("{slot}`"))) {
            return None;
        }
    }
    let locale = language.formula_locale();
    let mut rendered = Vec::with_capacity(values.len());
    for (name, value) in &values {
        let value = match *name {
            "token" => language.translation(value).unwrap_or(value).to_string(),
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
    Some(super::fill(translated, &args))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks captured parameters remain literal and partial or unknown messages are not translated.
    #[test]
    fn parameters_are_literal_and_messages_are_anchored() {
        assert_eq!(format_message("bad number `abc`", super::super::Language::En), None);
        assert_eq!(
            fill("bad number `abc`", "bad number `{number}`", "Invalid number: {number}", super::super::Language::En),
            Some("Invalid number: abc".into())
        );
        assert_eq!(fill("prefix: bad number `abc`", "bad number `{number}`", "Invalid number: {number}", super::super::Language::En), None);
        assert_eq!(fill("bad number `abc` suffix", "bad number `{number}`", "Invalid number: {number}", super::super::Language::En), None);
        assert_eq!(fill("bad number `a`b`", "bad number `{number}`", "Invalid number: {number}", super::super::Language::En), None);
        assert_eq!(format_message("Printer Office: permission denied", super::super::Language::En), None);
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
}
