//! Regional display conventions. Stored values, formulas and format codes stay invariant.

use std::borrow::Cow;

/// How a region shows numbers and the system short date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locale {
    pub decimal: char,
    pub group: char,
    /// Excel-format equivalent of the system's numeric short date.
    pub short_date: Cow<'static, str>,
}

impl Default for Locale {
    /// Use en-US display conventions when no region has been selected.
    fn default() -> Self {
        Self::EN_US
    }
}

impl Locale {
    pub const EN_US: Self = Self { decimal: '.', group: ',', short_date: Cow::Borrowed("m/d/yyyy") };

    /// Region overrides (`@rg=…` / `u-rg-…`) take precedence over the language's region.
    pub fn from_tag(tag: &str) -> Self {
        let base = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-");
        let mut parts = base.split('-');
        let lang = parts.next().unwrap_or("").to_ascii_lowercase();
        let region = parts
            .take_while(|p| p.len() != 1)
            .find(|p| p.len() == 2 || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit())))
            .unwrap_or("")
            .to_ascii_uppercase();
        let override_region = region_override(tag, &base);
        let region = override_region.as_deref().unwrap_or(&region);
        let (decimal, group, date) = match region {
            "CH" | "LI" => ('.', '\'', "dd.mm.yyyy"),
            "DE" | "AT" | "LU" | "RO" => (',', '.', "dd.mm.yyyy"),
            "FR" => (',', '\u{202F}', "dd/mm/yyyy"),
            "GB" | "IE" | "SG" => ('.', ',', "dd/mm/yyyy"),
            "IT" | "ES" | "BR" => (',', '.', "dd/mm/yyyy"),
            "NL" => (',', '.', "d-m-yyyy"),
            "JP" => ('.', ',', "yyyy/mm/dd"),
            "CN" => ('.', ',', "yyyy/m/d"),
            "" => {
                return match lang.as_str() {
                    "de" => Self::from_tag("de-DE"),
                    "ro" => Self::from_tag("ro-RO"),
                    "fr" => Self::from_tag("en-FR"),
                    "it" | "es" => Self::from_tag("en-IT"),
                    "nl" => Self::from_tag("en-NL"),
                    "ja" => Self::from_tag("en-JP"),
                    "zh" => Self::from_tag("en-CN"),
                    _ => Self::EN_US,
                };
            }
            _ => return Self::EN_US,
        };
        Self { decimal, group, short_date: Cow::Borrowed(date) }
    }

    /// Apply the system's ICU numeric short-date pattern. Unsupported patterns keep the region
    /// fallback rather than misinterpreting month names, eras or non-calendar years.
    pub fn with_icu_short_date(mut self, pattern: &str) -> Self {
        if let Some(code) = numeric_short_date(pattern) {
            self.short_date = Cow::Owned(code);
        }
        self
    }

    /// Return a changed regional display code for either canonical short-date family.
    pub(crate) fn display_code(&self, code: &str) -> Option<String> {
        match code {
            "m/d/yyyy" => Some(self.short_date.to_string()),
            "m/d/yyyy h:mm" => Some(format!("{} h:mm", self.short_date)),
            _ => None,
        }
        .filter(|c| c != code)
    }
}

/// Extract a valid POSIX or Unicode `rg` region override, ignoring private-use subtags.
/// POSIX overrides take precedence and the returned region is uppercase.
fn region_override(tag: &str, base: &str) -> Option<String> {
    let posix = tag
        .split_once('@')
        .and_then(|(_, ext)| ext.split(';').find_map(|p| p.split_once('=').filter(|(k, _)| k.eq_ignore_ascii_case("rg")).map(|(_, v)| v)));
    let parts: Vec<_> = base.split('-').collect();
    let unicode = parts.iter().take_while(|p| !p.eq_ignore_ascii_case("x")).position(|p| p.eq_ignore_ascii_case("u")).and_then(|start| {
        parts[start + 1..].windows(2).take_while(|pair| pair[0].len() != 1).find(|pair| pair[0].eq_ignore_ascii_case("rg")).map(|pair| pair[1])
    });
    let value = posix.or(unicode)?;
    let value = value.to_ascii_lowercase();
    let region = value.strip_suffix("zzzz")?;
    ((region.len() == 2 && region.chars().all(|c| c.is_ascii_alphabetic())) || (region.len() == 3 && region.chars().all(|c| c.is_ascii_digit())))
        .then(|| region.to_ascii_uppercase())
}

/// Convert a bounded numeric ICU pattern to Excel date tokens, retaining supported literals.
/// Require day, month and calendar year once each; reject unsupported tokens or broken quotes.
fn numeric_short_date(pattern: &str) -> Option<String> {
    if pattern.len() > 1024 {
        return None;
    }
    let mut chars = pattern.chars().peekable();
    let mut out = String::new();
    let mut seen = [false; 3];
    while let Some(c) = chars.next() {
        if c == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
                out.push_str("\"'\"");
                continue;
            }
            let mut literal = String::new();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        literal.push('\'');
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    literal.push(c);
                }
            }
            if !closed || literal.contains('"') {
                return None;
            }
            out.push('"');
            out.push_str(&literal);
            out.push('"');
        } else if c.is_ascii_alphabetic() {
            let mut n = 1;
            while chars.peek() == Some(&c) {
                chars.next();
                n += 1;
            }
            let (index, token) = match (c, n) {
                ('d', 1) => (0, "d"),
                ('d', 2) => (0, "dd"),
                ('M' | 'L', 1) => (1, "m"),
                ('M' | 'L', 2) => (1, "mm"),
                ('y', 2) => (2, "yy"),
                ('y', 1 | 3 | 4) => (2, "yyyy"),
                _ => return None,
            };
            if seen[index] {
                return None;
            }
            seen[index] = true;
            out.push_str(token);
        } else if matches!(c, '.' | '/' | '-' | ' ' | '\u{200E}' | '\u{200F}') {
            out.push(c);
        } else {
            return None;
        }
    }
    seen.into_iter().all(|v| v).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Romanian defaults and explicit region overrides win independently of interface language.
    #[test]
    fn romanian_region_and_overrides() {
        for tag in ["ro", "ro-RO", "ro_RO.UTF-8", "en-RO", "ro-Latn-RO", "en_US@rg=rozzzz", "en-US-u-rg-rozzzz"] {
            assert_eq!(Locale::from_tag(tag), Locale::from_tag("ro-RO"), "{tag}");
        }
        assert_eq!(Locale::from_tag("ro-US"), Locale::EN_US);
        assert_eq!(Locale::from_tag("en_US@rg=sgzzzz").short_date, "dd/mm/yyyy");
        assert_eq!(Locale::from_tag("en-US-x-rg-rozzzz"), Locale::EN_US);
        assert_eq!(Locale::from_tag("en-US-x-u-rg-rozzzz"), Locale::EN_US);
        assert_eq!(Locale::from_tag("en_US@RG=roZzZz"), Locale::from_tag("ro-RO"));
    }
    /// Numeric ICU conversion preserves supported year widths and quoted literals,
    /// and declines ambiguous or unsupported date patterns.
    #[test]
    fn system_numeric_patterns_preserve_year_width_and_literals() {
        for (input, expected) in [
            ("dd.MM.y", "dd.mm.yyyy"),
            ("M/d/yy", "m/d/yy"),
            ("y-MM-dd", "yyyy-mm-dd"),
            ("dd''MM''y", "dd\"'\"mm\"'\"yyyy"),
            ("dd 'de' MM 'de' y", "dd \"de\" mm \"de\" yyyy"),
        ] {
            assert_eq!(numeric_short_date(input).as_deref(), Some(expected));
        }
        for input in ["dd.MMM.y", "Y-MM-dd", "G y/M/d", "dd/MM", "dd/MM/y;0", "dd 'bad MM y"] {
            assert_eq!(numeric_short_date(input), None, "{input}");
        }
    }
}
