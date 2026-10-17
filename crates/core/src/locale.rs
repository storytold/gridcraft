//! Output locale: the number and date conventions a workbook paints with.
//!
//! Only *output* follows this: how a formatted number or date is written (decimal and thousands
//! separators, month and weekday names). Format codes are still read as en-US (`#,##0.00`,
//! `m/d/yyyy`) and function names, argument separators and typed input stay en-US — that is a
//! separate, larger piece of work (`docs/localization-parity.md`, PR #163).
//!
//! The locale is what View ▸ Interface language switches for German: picking German there makes
//! the grid, clipboard, printing, charts and `TEXT()` paint `1.234,56` and `15. Oktober 2026`.
//! It is a session setting (`Workbook::locale`), not part of the file: opening a workbook never
//! changes it.

use serde::{Deserialize, Serialize};

use crate::date::{MONTHS, WEEKDAYS};

/// The conventions numbers and dates are written with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Locale {
    /// en-US: `.` decimal, `,` thousands, English month and weekday names.
    #[default]
    En,
    /// de-DE: `,` decimal, `.` thousands, German month and weekday names.
    De,
}

/// Full month names by month number - 1 (English, the shared core table).
const EN_MONTHS: [&str; 12] = MONTHS;
/// Three-letter English month names, the first three characters of [`EN_MONTHS`].
const EN_MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/// Full German month names.
const DE_MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
/// Three-letter German month names (`Mär`, `Okt`, `Dez`), the forms German Excel shows.
const DE_MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"];
/// Full German weekday names, in the order of [`WEEKDAYS`] (index 0 = Sunday).
const DE_WEEKDAYS: [&str; 7] = ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"];
/// Three-letter German weekday names, the first three characters of [`DE_WEEKDAYS`].
const DE_WEEKDAYS_SHORT: [&str; 7] = ["Son", "Mon", "Die", "Mit", "Don", "Fre", "Sam"];
/// Three-letter English weekday names, the first three characters of [`WEEKDAYS`].
const EN_WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

impl Locale {
    /// The locale's tag: `"en"` or `"de"`.
    pub const fn tag(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::De => "de",
        }
    }

    /// The locale of a BCP 47-ish tag: `"de"`, `"de-DE"`, `"de_AT"`, `"en-GB"`… Anything whose
    /// language part is neither `de` nor `en`, and anything empty, is `None`.
    pub fn from_tag(tag: &str) -> Option<Self> {
        // `split` yields at least one (possibly empty) part, and takes any mixture of separators.
        let lang = tag.trim().split(['-', '_']).next().unwrap_or_default();
        if lang.eq_ignore_ascii_case("de") {
            Some(Locale::De)
        } else if lang.eq_ignore_ascii_case("en") {
            Some(Locale::En)
        } else {
            None
        }
    }

    /// The decimal separator (`.` in en-US, `,` in German).
    pub const fn decimal(self) -> char {
        match self {
            Locale::En => '.',
            Locale::De => ',',
        }
    }

    /// The thousands separator (`,` in en-US, `.` in German).
    pub const fn group(self) -> char {
        match self {
            Locale::En => ',',
            Locale::De => '.',
        }
    }

    /// The month name for a month number 1–12 (`short3` = `Jan`/`Mär`), or `""` for a number
    /// outside that range.
    pub fn month_name(self, month: u8, short3: bool) -> &'static str {
        let table: &[&'static str; 12] = match (self, short3) {
            (Locale::En, false) => &EN_MONTHS,
            (Locale::En, true) => &EN_MONTHS_SHORT,
            (Locale::De, false) => &DE_MONTHS,
            (Locale::De, true) => &DE_MONTHS_SHORT,
        };
        table.get(usize::from(month).wrapping_sub(1)).copied().unwrap_or("")
    }

    /// The weekday name for the weekday number of [`crate::date::DateTime::weekday`] (0 = Sunday,
    /// the order of [`WEEKDAYS`]), or `""` for a number outside 0–6.
    pub fn weekday_name(self, weekday: u8, short3: bool) -> &'static str {
        let table: &[&'static str; 7] = match (self, short3) {
            (Locale::En, false) => &WEEKDAYS,
            (Locale::En, true) => &EN_WEEKDAYS_SHORT,
            (Locale::De, false) => &DE_WEEKDAYS,
            (Locale::De, true) => &DE_WEEKDAYS_SHORT,
        };
        table.get(usize::from(weekday)).copied().unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_name_the_two_locales() {
        assert_eq!(Locale::default(), Locale::En);
        assert_eq!(Locale::En.tag(), "en");
        assert_eq!(Locale::De.tag(), "de");
        for tag in ["de", "DE", "de-DE", "de_AT", "de-CH"] {
            assert_eq!(Locale::from_tag(tag), Some(Locale::De), "{tag}");
        }
        for tag in ["en", "en-GB", "en_US"] {
            assert_eq!(Locale::from_tag(tag), Some(Locale::En), "{tag}");
        }
        for tag in ["", "fr", "deutsch", "es-ES"] {
            assert_eq!(Locale::from_tag(tag), None, "{tag}");
        }
    }

    #[test]
    fn separators_are_the_two_conventions() {
        assert_eq!((Locale::En.decimal(), Locale::En.group()), ('.', ','));
        assert_eq!((Locale::De.decimal(), Locale::De.group()), (',', '.'));
    }

    #[test]
    fn german_names_follow_the_core_date_tables() {
        // Same index order as the English tables the rest of the engine uses.
        assert_eq!(Locale::En.month_name(1, false), MONTHS[0]);
        assert_eq!(Locale::En.month_name(10, false), MONTHS[9]);
        assert_eq!(Locale::En.weekday_name(4, false), WEEKDAYS[4]);
        assert_eq!(Locale::De.month_name(1, false), "Januar");
        assert_eq!(Locale::De.month_name(3, false), "März");
        assert_eq!(Locale::De.month_name(10, false), "Oktober");
        assert_eq!(Locale::De.month_name(12, false), "Dezember");
        assert_eq!(Locale::De.weekday_name(0, false), "Sonntag");
        assert_eq!(Locale::De.weekday_name(4, false), "Donnerstag");
        assert_eq!(Locale::De.weekday_name(6, false), "Samstag");
        // Short names are the ones `ddd`/`mmm` show.
        assert_eq!(Locale::De.month_name(3, true), "Mär");
        assert_eq!(Locale::De.month_name(10, true), "Okt");
        assert_eq!(Locale::De.weekday_name(4, true), "Don");
        assert_eq!(Locale::En.month_name(10, true), "Oct");
        assert_eq!(Locale::En.weekday_name(4, true), WEEKDAYS[4].get(..3).unwrap_or_default());
        // Out of range names nothing.
        assert_eq!(Locale::De.month_name(0, false), "");
        assert_eq!(Locale::De.month_name(13, false), "");
        assert_eq!(Locale::De.weekday_name(7, false), "");
    }
}
