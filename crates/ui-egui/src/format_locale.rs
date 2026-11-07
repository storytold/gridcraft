//! Number-format spelling at the UI boundary. Stored styles and API/XLSX codes stay canonical.

/// Locale data for date/time tokens, bracketed colours and the UI's date/time presets.
pub(crate) struct FormatLocale {
    pub tokens: &'static [(char, char)],
    pub colors: &'static [(&'static str, &'static str)],
    pub general: &'static str,
    pub short_date: Option<&'static str>,
    pub time: Option<&'static str>,
}

pub(crate) const EN: FormatLocale = FormatLocale { tokens: &[], colors: &[], general: "General", short_date: None, time: None };

impl FormatLocale {
    /// Spells a stored format for the editor without changing literals, currency tags or conditions.
    pub fn display(&self, code: &str) -> String {
        self.translate(code, true)
    }

    /// Converts user-facing tokens back to the engine spelling; English tokens remain accepted.
    pub fn canonical(&self, code: &str) -> String {
        self.translate(code, false)
    }

    /// Chooses a canonical format for a named UI category, leaving programmatic defaults alone.
    pub fn preset<'a>(&self, category: &'a str) -> &'a str {
        match category {
            "Short Date" if self.short_date.is_some() => self.short_date.unwrap_or("m/d/yyyy"),
            "Time" if self.time.is_some() => self.time.unwrap_or("h:mm:ss"),
            _ => gridcraft_engine::cmd::format::format_code_for(category),
        }
    }

    /// Uses locale-specific short-date suggestions while retaining every other pattern, including 12-hour time.
    pub fn suggestion<'a>(&self, code: &'a str) -> &'a str {
        match code {
            "m/d/yyyy" => self.short_date.unwrap_or(code),
            _ => code,
        }
    }

    /// Maps only grammar tokens. Quoted/escaped/padded text and non-time brackets are opaque.
    fn translate(&self, code: &str, to_local: bool) -> String {
        let source = if to_local { "General" } else { self.general };
        if code.to_lowercase() == source.to_lowercase() {
            return if to_local { self.general } else { "General" }.to_owned();
        }
        let mut out = String::with_capacity(code.len());
        let mut pos = 0;
        let mut quoted = false;
        while let Some(rest) = code.get(pos..).filter(|rest| !rest.is_empty()) {
            let Some(ch) = rest.chars().next() else { break };
            pos += ch.len_utf8();
            if ch == '\\' || (!quoted && matches!(ch, '_' | '*')) {
                out.push(ch);
                if let Some(next) = code.get(pos..).and_then(|tail| tail.chars().next()) {
                    out.push(next);
                    pos += next.len_utf8();
                }
            } else if ch == '"' {
                quoted = !quoted;
                out.push(ch);
            } else if quoted {
                out.push(ch);
            } else if ch == '[' {
                let Some(end) = code.get(pos..).and_then(|tail| tail.find(']')) else {
                    out.push_str(rest);
                    break;
                };
                let content = &code[pos..pos + end];
                out.push('[');
                out.push_str(&self.bracket(content, to_local));
                out.push(']');
                pos += end + 1;
            } else if let Some(marker) =
                ["AM/PM", "A/P"].iter().find(|marker| rest.get(..marker.len()).is_some_and(|s| s.eq_ignore_ascii_case(marker)))
            {
                out.push_str(&rest[..marker.len()]);
                pos += marker.len() - ch.len_utf8();
            } else {
                out.push(self.token(ch, to_local));
            }
        }
        out
    }

    /// Translates one date/time letter; case is insignificant for format tokens.
    fn token(&self, ch: char, to_local: bool) -> char {
        self.tokens
            .iter()
            .find_map(|(canonical, local)| {
                let (source, target) = if to_local { (*canonical, *local) } else { (*local, *canonical) };
                ch.to_lowercase().eq(source.to_lowercase()).then_some(target)
            })
            .unwrap_or(ch)
    }

    /// Handles elapsed-time tokens and colour names without touching locale tags or comparisons.
    fn bracket(&self, content: &str, to_local: bool) -> String {
        for (canonical, local) in self.colors {
            let (source, target) = if to_local { (*canonical, *local) } else { (*local, *canonical) };
            if content.to_lowercase() == source.to_lowercase() {
                return target.to_owned();
            }
        }
        let normalized: String = content.chars().map(|ch| if to_local { ch.to_ascii_lowercase() } else { self.token(ch, false) }).collect();
        if let Some(first) = normalized.chars().next()
            && matches!(first, 'h' | 'm' | 's')
            && normalized.chars().all(|ch| ch == first)
        {
            return content.chars().map(|ch| self.token(ch, to_local)).collect();
        }
        content.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use crate::i18n::Language;

    /// Date/time input and display agree, including elapsed hours and uppercase Russian input.
    #[test]
    fn russian_format_tokens_round_trip() {
        let locale = Language::Ru.format_locale();
        for (canonical, local) in [
            ("dd.mm.yyyy", "дд.мм.гггг"),
            ("hh:mm:ss", "чч:мм:сс"),
            ("[h]:mm:ss", "[ч]:мм:сс"),
            ("h:mm AM/PM", "ч:мм AM/PM"),
            ("[Red]dd-mm-yy", "[Красный]дд-мм-гг"),
        ] {
            assert_eq!(locale.display(canonical), local);
            assert_eq!(locale.canonical(local), canonical);
        }
        assert_eq!(locale.canonical("ДД.ММ.ГГГГ ЧЧ:ММ:СС"), "dd.mm.yyyy hh:mm:ss");
        assert_eq!(locale.canonical("dd.mm.yyyy"), "dd.mm.yyyy");
        assert_eq!(locale.display("General"), "Общий");
        for code in ["Общий", "общий", "ОБЩИЙ", "оБщИй"] {
            assert_eq!(locale.canonical(code), "General");
        }
        assert_eq!(locale.display("gEnErAl"), "Общий");
        assert_eq!(locale.canonical(r#""общий""#), r#""общий""#);
        assert_eq!(locale.preset("Time"), "hh:mm:ss");
        assert_eq!(locale.suggestion("h:mm AM/PM"), "h:mm AM/PM");
    }

    /// Format literals, escaped padding, currency tags and conditions must survive verbatim.
    #[test]
    fn number_format_literals_are_not_translated() {
        let locale = Language::Ru.format_locale();
        for code in [r#"dd "yyyy г. {sheet}" \\m"#, r#"[$USD-409][>=100]0.00;[Red]-0.00;_m*d@"#, "[unknown]dd.mm.yyyy", "[unterminated yyyy"] {
            assert_eq!(locale.canonical(&locale.display(code)), code);
        }
        assert_eq!(locale.canonical(r#"дд "дд yyyy" \м"#), r#"dd "дд yyyy" \м"#);
        assert_eq!(Language::En.format_locale().display("m/d/yyyy h:mm AM/PM"), "m/d/yyyy h:mm AM/PM");
    }
}
