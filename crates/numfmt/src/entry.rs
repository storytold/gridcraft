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
            return match date {
                NumericDate::Canonical(date) => date,
                // Do not let the invariant fallback silently pick another date order.
                NumericDate::Unresolved => format!("'{text}"),
            };
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

enum NumericDate {
    Canonical(String),
    Unresolved,
}

/// Reorder numeric fields using the section that would display each candidate. ISO input is
/// independent of the region. Date validity and calendar epochs remain the core parser's job.
fn numeric_date(text: &str, code: &str, sys: DateSystem) -> Option<NumericDate> {
    let (date, time) = text.split_once([' ', 'T']).map_or((text, ""), |(d, t)| (d, t.trim()));
    let fields: Vec<_> = date.split(['.', '/', '-']).take(4).collect();
    let [first, _, _] = fields.as_slice() else { return None };
    if fields.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let iso = first.len() == 4;
    let format = NumberFormat::parse_cached(code);
    let orders: Vec<_> = if iso { vec![['y', 'm', 'd']] } else { format.calendar_orders().collect() };
    let mut accepted: Option<(String, f64)> = None;
    let mut parsed_date = false;
    for order in orders {
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
        if !iso && (day.len() > 2 || month.len() > 2 || !matches!(year.len(), 2 | 4)) {
            continue;
        }
        let canonical = if iso { format!("{year}-{month}-{day}") } else { format!("{month}/{day}/{year}") };
        let canonical = if time.is_empty() { canonical } else { format!("{canonical} {time}") };
        let parsed = parse_input(&canonical, sys);
        let Value::Number(value) = parsed.value else { continue };
        if parsed.format.is_none() {
            continue;
        }
        parsed_date = true;
        if !iso && format.calendar_order_for(value) != Some(order) {
            continue;
        }
        if accepted.as_ref().is_some_and(|(_, previous)| *previous != value) {
            return Some(NumericDate::Unresolved);
        }
        accepted = Some((canonical, value));
    }
    accepted.map(|(date, _)| NumericDate::Canonical(date)).or_else(|| parsed_date.then_some(NumericDate::Unresolved))
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
    #[test]
    fn conditional_date_entry_uses_the_selected_section_and_rejects_ambiguity() {
        let ro = Locale::from_tag("ro-RO");
        for sys in [DateSystem::D1900, DateSystem::D1904] {
            let Value::Number(january) = parse_input("2026-01-01", sys).value else { panic!("date fixture") };
            let Value::Number(march) = parse_input("2026-03-20", sys).value else { panic!("date fixture") };
            let selected = format!("[<{january}]mm/dd/yyyy;dd/mm/yyyy");
            for (input, expected) in [("04/03/2026", "2026-03-04"), ("04/03/2026 15:00", "2026-03-04 15:00")] {
                assert_eq!(parse_input(&ro.canonical_input(input, sys, &selected), sys).value, parse_input(expected, sys).value);
            }
            for code in [format!("[<{march}]dd/mm/yyyy;mm/dd/yyyy"), format!("[<{march}]mm/dd/yyyy;dd/mm/yyyy")] {
                for input in ["04/03/2026", "04/03/2026 15:00", " 04/03/2026 "] {
                    assert_eq!(parse_input(&ro.canonical_input(input, sys, &code), sys).value, Value::text(input));
                }
                assert_eq!(parse_input(&ro.canonical_input("2026-03-04", sys, &code), sys).value, parse_input("2026-03-04", sys).value);
            }
            let same_order = format!("[<{march}]dd/mm/yyyy;dd/mm/yyyy");
            assert_eq!(parse_input(&ro.canonical_input("04/03/2026", sys, &same_order), sys).value, parse_input("2026-03-04", sys).value);
        }
    }
}
