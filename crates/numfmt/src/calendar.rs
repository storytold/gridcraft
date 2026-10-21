//! Calendars a format section can show dates in: the `B1` (Gregorian) and `B2` (Hijri) prefixes.

use gridcraft_core::date::{DateTime, MONTHS, days_from_civil};

/// The calendar a section's year, month and day parts count in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Calendar {
    #[default]
    Gregorian,
    /// The tabular (arithmetic) Islamic calendar.
    Hijri,
}

/// Arabic names of the Hijri months, Muharram first.
const HIJRI_MONTHS: [&str; 12] =
    ["محرم", "صفر", "ربيع الأول", "ربيع الثاني", "جمادى الأولى", "جمادى الثانية", "رجب", "شعبان", "رمضان", "شوال", "ذو القعدة", "ذو الحجة"];

/// Year, month and day of a date in a section's calendar, with the month's names.
pub(crate) struct CalendarDate {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    /// `mmmm`.
    pub month_name: &'static str,
    /// `mmm`.
    pub month_short: &'static str,
}

impl Calendar {
    /// The date of a Gregorian day in this calendar. A Hijri date that cannot be computed (before
    /// 1 Muharram 1 AH) falls back to the Gregorian one.
    pub(crate) fn date(self, dt: &DateTime) -> CalendarDate {
        match self {
            Calendar::Hijri => hijri_date(dt).unwrap_or_else(|| gregorian_date(dt)),
            Calendar::Gregorian => gregorian_date(dt),
        }
    }
}

fn gregorian_date(dt: &DateTime) -> CalendarDate {
    let month_name = MONTHS.get((dt.month as usize).wrapping_sub(1)).copied().unwrap_or("");
    CalendarDate { year: dt.year.into(), month: dt.month, day: dt.day, month_name, month_short: month_name.get(..3).unwrap_or(month_name) }
}

/// Days from 1 Muharram 1 AH to the first of a Hijri month. Leap years (30 days in the last
/// month) are years 2, 5, 7, 10, 13, 16, 18, 21, 24, 26 and 29 of each 30-year cycle; odd months
/// have 30 days and even months 29.
fn days_before_month(year: i64, month: i64) -> i64 {
    (year - 1) * 354 + (3 + 11 * year).div_euclid(30) + 29 * (month - 1) + month / 2
}

/// 1 Muharram 1 AH is 18 July 622 (proleptic Gregorian), so that 1 January 2001 is 6 Shawwal 1421.
fn hijri_date(dt: &DateTime) -> Option<CalendarDate> {
    let days = days_from_civil(dt.year, dt.month, dt.day) - days_from_civil(622, 7, 18);
    if days < 0 {
        return None;
    }
    let year = (30 * days + 10646) / 10631;
    let month = ((11 * (days - days_before_month(year, 1)) + 330) / 325).clamp(1, 12);
    let day = days - days_before_month(year, month) + 1;
    let month_name = HIJRI_MONTHS.get(usize::try_from(month - 1).ok()?).copied()?;
    Some(CalendarDate { year, month: u32::try_from(month).ok()?, day: u32::try_from(day).ok()?, month_name, month_short: month_name })
}
