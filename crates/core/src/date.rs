//! Date serial numbers.
//!
//! The 1900 system counts days from 1899-12-31 (serial 1 = 1900-01-01) and, like Lotus 1-2-3 and
//! Excel, treats 1900 as a leap year: serial 60 is the fictitious 1900-02-29. The 1904 system
//! counts from 1904-01-01 (serial 0). Times are fractions of a day.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DateSystem {
    #[default]
    D1900,
    D1904,
}

/// A calendar date and time of day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Milliseconds (rounded).
    pub milli: u32,
    /// 0 = Sunday.
    pub weekday: u32,
}

/// Days from 0000-03-01 style civil algorithm (Howard Hinnant), days since 1970-01-01.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Milliseconds since the Unix epoch (UTC) from the system clock; `None` when the clock is unavailable.
/// On wasm32 `std::time::SystemTime::now` traps, so the browser clock is read through `Date.now()`.
#[allow(clippy::disallowed_methods)] // the std clock is read only off wasm
pub fn unix_now_ms() -> Option<f64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(d.as_secs_f64() * 1000.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Some(js_sys::Date::now())
    }
}

/// Date serial (1900 system, with fraction) for a Unix time in milliseconds.
pub fn serial_from_unix_ms(ms: f64) -> f64 {
    25569.0 + ms / 86_400_000.0
}

pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

const UNIX_1899_12_30: i64 = -25569; // days_from_civil(1899, 12, 30)
const UNIX_1904_01_01: i64 = -24107;

pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days in a month of a date system's calendar: like [`days_in_month`], except that February
/// 1900 has 29 days in the 1900 system (serial 60 is the fictitious 1900-02-29).
pub fn days_in_month_in(sys: DateSystem, y: i32, m: u32) -> u32 {
    if sys == DateSystem::D1900 && y == 1900 && m == 2 { 29 } else { days_in_month(y, m) }
}

/// Serial for a date. Month and day overflow roll over like `DATE()` (month 13 = January next
/// year, day 0 = last day of the previous month), in the date system's own calendar: in the 1900
/// system February 1900 has 29 days, so `(1900, 2, 29)` and `(1900, 3, 0)` are serial 60.
/// `None` before the epoch or after 9999-12-31.
pub fn serial_from_ymd(sys: DateSystem, year: i64, month: i64, day: i64) -> Option<f64> {
    // DATE(): years 0–1899 are added to 1900.
    let y = if (0..1900).contains(&year) { year + 1900 } else { year };
    let m0 = month.checked_sub(1)?;
    let y = y.checked_add(m0.div_euclid(12))?;
    let m = m0.rem_euclid(12) + 1;
    if !(1900..=9999).contains(&y) && sys == DateSystem::D1900 || !(1904..=9999).contains(&y) && sys == DateSystem::D1904 {
        return None;
    }
    let first = days_from_civil(y as i32, m as u32, 1);
    // Serial of the first of the month; the day (and any overflow) counts on from there.
    let first = match sys {
        // Up to February 1900 the serial is one lower than the day count from 1899-12-30: the
        // fictitious 1900-02-29 (serial 60) comes before 1 March.
        DateSystem::D1900 => first - UNIX_1899_12_30 - i64::from(y == 1900 && m <= 2),
        DateSystem::D1904 => first - UNIX_1904_01_01,
    };
    let serial = first.checked_add(day)?.checked_sub(1)?;
    if !(0..=2_958_465).contains(&serial) {
        return None;
    }
    Some(serial as f64)
}

/// Date/time from a serial. `None` for negative or too-large serials.
pub fn datetime_from_serial(sys: DateSystem, serial: f64) -> Option<DateTime> {
    if !serial.is_finite() || !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let mut day = serial.floor() as i64;
    let mut frac_ms = ((serial - serial.floor()) * 86_400_000.0).round() as i64;
    if frac_ms >= 86_400_000 {
        frac_ms -= 86_400_000;
        day += 1;
    }
    let (y, m, d, weekday) = match sys {
        DateSystem::D1900 => {
            // Excel's weekdays continue the fictitious calendar: serial 1 is a Sunday.
            let wd = ((day + 6) % 7) as u32;
            if day == 60 {
                (1900, 2, 29, wd)
            } else if day == 0 {
                (1900, 1, 0, wd)
            } else {
                let real = if day < 60 { day + 1 } else { day };
                let (y, m, d) = civil_from_days(real + UNIX_1899_12_30);
                (y, m, d, wd)
            }
        }
        DateSystem::D1904 => {
            let unix = day + UNIX_1904_01_01;
            let (y, m, d) = civil_from_days(unix);
            (y, m, d, ((unix + 4).rem_euclid(7)) as u32)
        }
    };
    let secs = frac_ms / 1000;
    Some(DateTime {
        year: y,
        month: m,
        day: d,
        hour: (secs / 3600) as u32,
        minute: ((secs / 60) % 60) as u32,
        second: (secs % 60) as u32,
        milli: (frac_ms % 1000) as u32,
        weekday,
    })
}

/// Fraction of a day for a time.
pub fn time_fraction(h: f64, m: f64, s: f64) -> f64 {
    (h * 3600.0 + m * 60.0 + s) / 86400.0
}

pub const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_from_unix_time() {
        assert_eq!(serial_from_unix_ms(0.0), 25569.0);
        // 2026-10-10T12:00:00Z
        assert_eq!(serial_from_unix_ms(1_791_633_600_000.0), 46305.5);
        assert!(unix_now_ms().is_some_and(|ms| serial_from_unix_ms(ms) > 46305.0));
    }

    #[test]
    fn known_serials() {
        let s = DateSystem::D1900;
        assert_eq!(serial_from_ymd(s, 1900, 1, 1), Some(1.0));
        assert_eq!(serial_from_ymd(s, 1900, 2, 28), Some(59.0));
        assert_eq!(serial_from_ymd(s, 1900, 3, 1), Some(61.0));
        assert_eq!(serial_from_ymd(s, 2000, 1, 1), Some(36526.0));
        assert_eq!(serial_from_ymd(s, 2026, 10, 7), Some(46302.0));
        assert_eq!(serial_from_ymd(s, 9999, 12, 31), Some(2958465.0));
        assert_eq!(serial_from_ymd(s, 2020, 13, 1), serial_from_ymd(s, 2021, 1, 1));
        assert_eq!(serial_from_ymd(s, 2020, 3, 0), serial_from_ymd(s, 2020, 2, 29));
        assert_eq!(serial_from_ymd(s, 120, 1, 1), serial_from_ymd(s, 2020, 1, 1));
        assert_eq!(serial_from_ymd(DateSystem::D1904, 1904, 1, 1), Some(0.0));
        assert_eq!(serial_from_ymd(DateSystem::D1904, 2000, 1, 1), Some(35064.0));
    }

    #[test]
    fn fictitious_1900_02_29() {
        // Excel's 1900 calendar has a 29 February 1900 (serial 60); day and month overflow count
        // through it.
        let s = DateSystem::D1900;
        assert_eq!(serial_from_ymd(s, 1900, 2, 29), Some(60.0));
        assert_eq!(serial_from_ymd(s, 1900, 3, 0), Some(60.0));
        assert_eq!(serial_from_ymd(s, 1900, 2, 30), Some(61.0));
        assert_eq!(serial_from_ymd(s, 1900, 1, 60), Some(60.0));
        assert_eq!(serial_from_ymd(s, 1900, 1, 61), Some(61.0));
        assert_eq!(serial_from_ymd(s, 1900, 1, 400), Some(400.0));
        assert_eq!(serial_from_ymd(s, 1900, 4, -30), Some(61.0));
        assert_eq!(serial_from_ymd(s, 1900, 4, -31), Some(60.0));
        assert_eq!(serial_from_ymd(s, 1900, 3, -1), Some(59.0));
        assert_eq!(serial_from_ymd(s, 1901, -10, 29), Some(60.0));
        assert_eq!(serial_from_ymd(s, 1900, 1, 0), Some(0.0));
        assert_eq!(serial_from_ymd(s, 1900, 1, -1), None);
        assert_eq!(serial_from_ymd(s, 1900, 0, 1), None);
        // Later years keep the real calendar.
        assert_eq!(serial_from_ymd(s, 1901, 2, 29), serial_from_ymd(s, 1901, 3, 1));
        assert_eq!(serial_from_ymd(s, 1904, 2, 29), Some(1521.0));
        // Every serial up to 1900-12-31 comes back from its own year, month and day.
        for serial in 0..=366 {
            let d = datetime_from_serial(s, serial as f64).unwrap();
            assert_eq!(serial_from_ymd(s, d.year as i64, d.month as i64, d.day as i64), Some(serial as f64), "{d:?}");
        }
        assert_eq!(days_in_month_in(s, 1900, 2), 29);
        assert_eq!(days_in_month_in(s, 1901, 2), 28);
        assert_eq!(days_in_month_in(DateSystem::D1904, 1900, 2), 28);
        // The 1904 system has no fictitious day: 1904 is a real leap year.
        let s = DateSystem::D1904;
        assert_eq!(serial_from_ymd(s, 1904, 2, 29), Some(59.0));
        assert_eq!(serial_from_ymd(s, 1904, 3, 0), Some(59.0));
        assert_eq!(serial_from_ymd(s, 1904, 3, 1), Some(60.0));
        assert_eq!(serial_from_ymd(s, 1904, 1, 0), None);
        assert_eq!(serial_from_ymd(s, 1900, 2, 29), None);
    }

    #[test]
    fn hostile_month_and_day() {
        let s = DateSystem::D1900;
        assert_eq!(serial_from_ymd(s, 2000, i64::MIN, 1), None);
        assert_eq!(serial_from_ymd(s, 2000, i64::MAX, 1), None);
        assert_eq!(serial_from_ymd(s, 2000, 1, i64::MAX), None);
        assert_eq!(serial_from_ymd(s, 2000, 1, i64::MIN), None);
        assert_eq!(serial_from_ymd(s, i64::MAX, 1, 1), None);
    }

    #[test]
    fn roundtrip() {
        let s = DateSystem::D1900;
        for serial in [1.0, 59.0, 60.0, 61.0, 36526.0, 46302.0, 2958465.0] {
            let d = datetime_from_serial(s, serial).unwrap();
            assert_eq!(serial_from_ymd(s, d.year as i64, d.month as i64, d.day as i64), Some(serial));
        }
        let d = datetime_from_serial(s, 60.0).unwrap();
        assert_eq!((d.year, d.month, d.day), (1900, 2, 29));
        let d = datetime_from_serial(s, 46302.75).unwrap();
        assert_eq!((d.hour, d.minute, d.weekday), (18, 0, 3)); // 2026-10-07 is a Wednesday
        let d = datetime_from_serial(s, 1.0).unwrap();
        assert_eq!(d.weekday, 0); // 1900-01-01 was a Sunday in Excel's calendar
    }
}
