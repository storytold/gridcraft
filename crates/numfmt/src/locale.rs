//! Regional conventions for showing numbers: the decimal and digit-group separators a format's
//! `.` and `,` stand for, and the short date the built-in date format shows. Files, formulas and
//! format codes stay en-US; only the displayed text changes.

/// How a region shows numbers and the system short date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locale {
    /// Shown for a format's decimal point.
    pub decimal: char,
    /// Shown between digit groups for a format's thousands separator.
    pub group: char,
    /// The format code the built-in short date (`m/d/yyyy`, number format 14) shows as.
    pub short_date: &'static str,
}

impl Default for Locale {
    fn default() -> Locale {
        Locale::EN_US
    }
}

impl Locale {
    pub const EN_US: Locale = Locale { decimal: '.', group: ',', short_date: "m/d/yyyy" };

    /// The conventions of a language tag such as `de-CH` or `fr_FR.UTF-8`; regions not listed
    /// show as en-US.
    pub fn from_tag(tag: &str) -> Locale {
        let tag = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-");
        let mut parts = tag.split('-');
        let lang = parts.next().unwrap_or("").to_ascii_lowercase();
        // The region is the two-letter (or three-digit) subtag after an optional script.
        let region = parts.find(|p| p.len() == 2 || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))).unwrap_or("").to_ascii_uppercase();
        let (comma, narrow_space) = (',', '\u{202F}');
        let swiss = Locale { decimal: '.', group: '\'', short_date: "dd.mm.yyyy" };
        let german = Locale { decimal: comma, group: '.', short_date: "dd.mm.yyyy" };
        let french = Locale { decimal: comma, group: narrow_space, short_date: "dd/mm/yyyy" };
        let southern = Locale { decimal: comma, group: '.', short_date: "dd/mm/yyyy" };
        let british = Locale { short_date: "dd/mm/yyyy", ..Locale::EN_US };
        let dutch = Locale { decimal: comma, group: '.', short_date: "d-m-yyyy" };
        let japanese = Locale { short_date: "yyyy/mm/dd", ..Locale::EN_US };
        let chinese = Locale { short_date: "yyyy/m/d", ..Locale::EN_US };
        // The region decides (macOS reports `en-CH` for English with Swiss formats); a bare
        // language falls back to its main region.
        match region.as_str() {
            "CH" | "LI" => swiss,
            "DE" | "AT" | "LU" => german,
            "FR" => french,
            "GB" | "IE" => british,
            "IT" | "ES" | "BR" => southern,
            "NL" => dutch,
            "JP" => japanese,
            "CN" => chinese,
            "" => match lang.as_str() {
                "de" => german,
                "fr" => french,
                "it" | "es" => southern,
                "nl" => dutch,
                "ja" => japanese,
                "zh" => chinese,
                _ => Locale::EN_US,
            },
            _ => Locale::EN_US,
        }
    }

    /// The code a cell's format code shows with when it differs: the built-in short date (and
    /// short date with time) follow the region; every other code is used as written (`None`).
    pub(crate) fn display_code(&self, code: &str) -> Option<String> {
        if *self == Locale::EN_US {
            return None;
        }
        match code {
            "m/d/yyyy" => Some(self.short_date.to_string()),
            "m/d/yyyy h:mm" => Some(format!("{} h:mm", self.short_date)),
            _ => None,
        }
        .filter(|c| c != code)
    }
}
