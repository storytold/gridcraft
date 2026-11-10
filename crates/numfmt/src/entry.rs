//! A front-end boundary for numeric entry; file and command parsers remain invariant.
use gridcraft_core::{DateSystem, Value, parse::parse_input};

use crate::{Locale, NumberFormat};

impl Locale {
    /// Translate numeric interactive input to the invariant parser's spelling. `date_code` is
    /// the destination's effective calendar format, or the region's short date for General.
    /// Unsupported or malformed regional numbers stay text rather than acquiring another value.
    pub fn canonical_input(&self, text: &str, sys: DateSystem, date_code: &str) -> String {
        if self == &Self::EN_US || text.starts_with(['\'', '=']) || text.is_empty() {
            return text.to_string();
        }
        if let Some(date) = numeric_date(text.trim(), date_code, sys) {
            return date;
        }
        let mut mapped: String = text
            .chars()
            .map(|c| {
                if c == self.decimal {
                    '.'
                } else if c == self.group || (self.group.is_whitespace() && matches!(c, '\u{a0}' | '\u{202f}')) || (self.group == '\'' && c == '’')
                {
                    ','
                } else {
                    c
                }
            })
            .collect();
        if self.percent_prefix
            && let Some(number) = mapped.trim().strip_prefix('%')
        {
            // Let the invariant parser validate the number and assign its percentage format.
            mapped = format!("{}%", number.trim_start());
        }
        if matches!(parse_input(&mapped, sys).value, Value::Number(_) | Value::Bool(_) | Value::Error(_) | Value::Empty) {
            return mapped;
        }
        let original = parse_input(text, sys);
        // Named dates/times keep the existing English parser's behavior. This boundary does not
        // add a month-name or currency parser.
        if text.chars().any(|c| c.is_ascii_alphabetic()) && original.format.is_some() {
            return text.to_string();
        }
        if matches!(original.value, Value::Number(_)) { format!("'{text}") } else { text.to_string() }
    }
}

/// Reorder three numeric calendar fields, leaving ISO input independent of the region. Date
/// validity and the 1900/1904 systems are checked by the existing core parser, not reimplemented.
fn numeric_date(text: &str, code: &str, sys: DateSystem) -> Option<String> {
    let (date, time) = text.split_once([' ', 'T']).map_or((text, ""), |(d, t)| (d, t.trim()));
    let fields: Vec<_> = date.split(['.', '/', '-']).take(4).collect();
    let [first, second, third] = fields.as_slice() else { return None };
    if fields.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let canonical = if first.len() == 4 {
        format!("{first}-{second}-{third}")
    } else {
        let order = NumberFormat::parse_cached(code).calendar_order()?;
        let mut day = "";
        let mut month = "";
        let mut year = "";
        for (part, value) in order.iter().zip(&fields) {
            match part {
                'd' => day = value,
                'm' => month = value,
                'y' => year = value,
                _ => return None,
            }
        }
        if day.len() > 2 || month.len() > 2 || !matches!(year.len(), 2 | 4) {
            return None;
        }
        format!("{month}/{day}/{year}")
    };
    let canonical = if time.is_empty() { canonical } else { format!("{canonical} {time}") };
    let parsed = parse_input(&canonical, sys);
    (matches!(parsed.value, Value::Number(_)) && parsed.format.is_some()).then_some(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_entry_uses_parsed_calendar_tokens_not_weekdays_or_literals() {
        let ro = Locale::from_tag("ro-RO");
        for sys in [DateSystem::D1900, DateSystem::D1904] {
            let expected = parse_input("2026-03-04", sys).value;
            for (input, code) in [
                ("04.03.2026", "dd.mm.yyyy"),
                ("3/4/2026", "dddd, m/d/yyyy"),
                ("3/4/2026", "[$-409]m/d/yyyy"),
                ("04.03.2026", "dd\".\"mm\".\"yyyy h:mm"),
                ("2026-03-04", "dd.mm.yyyy"),
            ] {
                let canonical = ro.canonical_input(input, sys, code);
                assert_eq!(parse_input(&canonical, sys).value, expected, "{input} / {code}");
            }
        }
    }

    #[test]
    fn regional_numeric_entry_preserves_formulas_quotes_and_invalid_groups() {
        let ro = Locale::from_tag("ro-RO");
        let sys = DateSystem::D1900;
        for text in ["=SUM(1.5,2.25)", "'1,234", "TRUE", "#DIV/0!"] {
            assert_eq!(ro.canonical_input(text, sys, "dd.mm.yyyy"), text);
        }
        for text in ["1.23", "11.13.2026"] {
            assert_eq!(parse_input(&ro.canonical_input(text, sys, "dd.mm.yyyy"), sys).value, Value::text(text));
        }
        assert_eq!(parse_input(&ro.canonical_input("1.002.024", sys, "dd.mm.yyyy"), sys).value, Value::Number(1002024.0));
        let ch = Locale::from_tag("de-CH");
        assert_eq!(parse_input(&ch.canonical_input("1’234.56", sys, "dd.mm.yyyy"), sys).value, Value::Number(1234.56));
        let fr = Locale::from_tag("fr-FR");
        assert_eq!(parse_input(&fr.canonical_input("1\u{a0}234,56", sys, "dd/mm/yyyy"), sys).value, Value::Number(1234.56));
    }

    #[test]
    fn leading_percent_is_regional_and_keeps_existing_entry_conventions() {
        let sys = DateSystem::D1900;
        for (tag, input, expected) in [
            ("tr-TR", "%15", Value::Number(0.15)),
            ("tr-TR", "%15,5", Value::Number(0.155)),
            ("tr-TR", "15%", Value::Number(0.15)),
            ("tr-TR", "1.5", Value::text("1.5")),
            ("tr-TR", "₺46,00", Value::text("₺46,00")),
            ("ro-RO", "%15", Value::text("%15")),
            ("ro-RO", "15,5%", Value::Number(0.155)),
            ("en-US", "%15", Value::text("%15")),
            ("en-US", "15.5%", Value::Number(0.155)),
        ] {
            let loc = Locale::from_tag(tag);
            let parsed = parse_input(&loc.canonical_input(input, sys, "General"), sys);
            assert_eq!(parsed.value, expected, "{tag}: {input}");
            if input.contains('%') && matches!(expected, Value::Number(_)) {
                assert!(NumberFormat::parse(parsed.format.unwrap()).is_percent());
            }
        }
    }
}
