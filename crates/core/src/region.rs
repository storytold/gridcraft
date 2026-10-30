//! Regional formats: how numbers and dates are written for people in a country. Files, formulas
//! and the engine API stay en-US; the interface shows and reads values in the user's region.

/// A set of regional conventions. `EnUs` is the file and API format and the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Region {
    #[default]
    EnUs,
    /// Spain: `1.234,56`, `31/12/2026`, euro after the amount.
    EsEs,
    /// Ukraine: `1 234,56` (non-breaking space), `31.12.2026`, hryvnia after the amount.
    UkUa,
}

impl Region {
    pub const ALL: [Region; 3] = [Region::EnUs, Region::EsEs, Region::UkUa];

    /// The decimal separator.
    pub fn decimal(self) -> char {
        match self {
            Region::EnUs => '.',
            Region::EsEs | Region::UkUa => ',',
        }
    }

    /// The thousands separator.
    pub fn group(self) -> char {
        match self {
            Region::EnUs => ',',
            Region::EsEs => '.',
            Region::UkUa => '\u{a0}',
        }
    }

    /// The format code the short date (built-in format 14, `m/d/yyyy`) shows as.
    pub fn short_date(self) -> &'static str {
        match self {
            Region::EnUs => "m/d/yyyy",
            Region::EsEs => "dd/mm/yyyy",
            Region::UkUa => "dd.mm.yyyy",
        }
    }

    /// The format code the short date and time (built-in format 22, `m/d/yyyy h:mm`) shows as.
    pub fn short_date_time(self) -> &'static str {
        match self {
            Region::EnUs => "m/d/yyyy h:mm",
            Region::EsEs => "dd/mm/yyyy h:mm",
            Region::UkUa => "dd.mm.yyyy h:mm",
        }
    }

    /// Whether dates are written day first (`31/12`), as everywhere but the United States.
    pub fn day_first(self) -> bool {
        self != Region::EnUs
    }

    /// The local currency symbol, written after the amount.
    pub fn currency(self) -> Option<&'static str> {
        match self {
            Region::EnUs => None,
            Region::EsEs => Some("€"),
            Region::UkUa => Some("₴"),
        }
    }

    /// The region's BCP 47 tag (`es-ES`).
    pub fn tag(self) -> &'static str {
        match self {
            Region::EnUs => "en-US",
            Region::EsEs => "es-ES",
            Region::UkUa => "uk-UA",
        }
    }

    /// The region for a BCP 47 tag or language code (`es`, `es-ES`, `uk`, `ca`…), if known.
    pub fn from_tag(tag: &str) -> Option<Region> {
        let lang = tag.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        match lang.as_str() {
            "en" => Some(Region::EnUs),
            // The languages of Spain write numbers and dates the Spanish way.
            "es" | "ca" | "gl" | "eu" => Some(Region::EsEs),
            "uk" => Some(Region::UkUa),
            _ => None,
        }
    }
}
