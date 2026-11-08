//! Number and date display conventions for invariant format codes.

use crate::date::{MONTHS, WEEKDAYS};

/// The conventions numbers and dates are written with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
/// Three-letter German month names (`Mrz`, `Okt`, `Dez`), the forms German Excel shows.
const DE_MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mrz", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"];
/// Full German weekday names, in the order of [`WEEKDAYS`] (index 0 = Sunday).
const DE_WEEKDAYS: [&str; 7] = ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"];
/// Two-letter German weekday names, the forms German Excel shows for `ddd` (`So`, `Mo`, `Di`):
/// not the first characters of [`DE_WEEKDAYS`], which would read `Son`, `Mon`, `Die`.
const DE_WEEKDAYS_SHORT: [&str; 7] = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];
/// Three-letter English weekday names, the first three characters of [`WEEKDAYS`].
const EN_WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

impl Locale {
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

    /// The month name for a month number 1–12 (`short3` = `Jan`/`Mrz`), or `""` for a number
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
        assert_eq!(Locale::De.month_name(3, true), "Mrz");
        assert_eq!(Locale::De.month_name(10, true), "Okt");
        assert_eq!(Locale::De.weekday_name(4, true), "Do");
        assert_eq!(Locale::En.month_name(10, true), "Oct");
        assert_eq!(Locale::En.weekday_name(4, true), WEEKDAYS[4].get(..3).unwrap_or_default());
        // Out of range names nothing.
        assert_eq!(Locale::De.month_name(0, false), "");
        assert_eq!(Locale::De.month_name(13, false), "");
        assert_eq!(Locale::De.weekday_name(7, false), "");
    }
}
