//! Dates, times and durations between GridCraft and the worker, which [`Value`] has no form for.
//!
//! The worker never sees a serial number: only GridCraft knows the workbook's date system, so it
//! alone turns serials into [`Stamp`]s and back, through `gridcraft_core::date`. On the wire a stamp
//! is text, at most [`MAX_DATE_BYTES`], or for a duration a number of days. Text over the cap, or of
//! the wrong JSON type, breaks the protocol like any other field. Text that keeps the shape and names
//! no date (`2023-02-30`) is only a bad value: it reads as `#VALUE!` and costs no worker.

use gridcraft_core::date::{DateSystem, datetime_from_serial, days_in_month, max_serial, serial_from_ymd};
use gridcraft_core::{CellError, Value};
use serde::ser::{Serialize, SerializeMap, Serializer};

/// Bytes in a date or time's text. A year that fills an `i64` still fits.
pub const MAX_DATE_BYTES: usize = 64;

const MILLIS_PER_DAY: f64 = 86_400_000.0;
const MICROS_PER_DAY: f64 = 86_400_000_000.0;
/// One millisecond, the precision of a serial read back, in days.
const LAST_MILLI: f64 = 1.0 / MILLIS_PER_DAY;

/// A calendar day. A real one, though its year may be one no serial holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

/// A time of day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub micro: u32,
}

/// A date, a time or a duration, in the workbook's terms only once it is given a date system.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stamp {
    Date(Day),
    DateTime(Day, Clock),
    Time(Clock),
    /// A length of time, in days.
    Duration(f64),
}

/// A value with stamps in it. `value` holds [`Value::Empty`] where a stamp goes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dated {
    /// A scalar, or an array of scalars.
    pub value: Value,
    /// The stamps, by their place in row order (0 for a scalar), in ascending order.
    pub stamps: Vec<(u32, Stamp)>,
}

impl Day {
    /// `YYYY-MM-DD`, with four or more digits of year and an optional minus sign.
    pub(super) fn parse(text: &str) -> Option<Day> {
        let (negative, rest) = text.strip_prefix('-').map_or((false, text), |rest| (true, rest));
        let (year, rest) = rest.split_once('-')?;
        let (month, day) = rest.split_once('-')?;
        if year.len() < 4 || !digits(year) {
            return None;
        }
        // A year past any serial's reach only has to stay past it.
        let year = year.parse::<i64>().unwrap_or(i64::MAX);
        let year = i32::try_from(if negative { -year } else { year }).unwrap_or(if negative { i32::MIN } else { i32::MAX });
        let (month, day) = (two_digits(month)?, two_digits(day)?);
        let last = days_in_month(year, u32::from(month));
        (last > 0 && day >= 1 && u32::from(day) <= last).then_some(Day { year, month, day })
    }

    fn serial(self, sys: DateSystem) -> Result<f64, CellError> {
        // serial_from_ymd reads a year below 1900 as DATE() does, as years after 1900.
        let first = if sys == DateSystem::D1904 { 1904 } else { 1900 };
        if !(first..=9999).contains(&self.year) {
            return Err(CellError::Num);
        }
        serial_from_ymd(sys, i64::from(self.year), i64::from(self.month), i64::from(self.day)).ok_or(CellError::Num)
    }

    fn text(self) -> String {
        let sign = if self.year < 0 { "-" } else { "" };
        format!("{sign}{:04}-{:02}-{:02}", self.year.unsigned_abs(), self.month, self.day)
    }
}

impl Clock {
    pub const MIDNIGHT: Clock = Clock { hour: 0, minute: 0, second: 0, micro: 0 };

    /// `HH:MM:SS`, with up to nine digits of fraction; what is past the microsecond is dropped.
    pub(super) fn parse(text: &str) -> Option<Clock> {
        let (whole, fraction) = text.split_once('.').map_or((text, None), |(w, f)| (w, Some(f)));
        let mut parts = whole.split(':');
        let hour = two_digits(parts.next()?)?;
        let minute = two_digits(parts.next()?)?;
        let second = two_digits(parts.next()?)?;
        if parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let micro = match fraction {
            None => 0,
            Some(f) if (1..=9).contains(&f.len()) && digits(f) => {
                f.bytes().chain(std::iter::repeat(b'0')).take(6).fold(0, |n, b| n * 10 + u32::from(b - b'0'))
            }
            Some(_) => return None,
        };
        Some(Clock { hour, minute, second, micro })
    }

    /// The fraction of a day. Whole microseconds keep the division exact to the last bit.
    fn fraction(self) -> f64 {
        let seconds = u64::from(self.hour) * 3600 + u64::from(self.minute) * 60 + u64::from(self.second);
        (seconds * 1_000_000 + u64::from(self.micro)) as f64 / MICROS_PER_DAY
    }

    fn text(self) -> String {
        let whole = format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second);
        if self.micro == 0 { whole } else { format!("{whole}.{:06}", self.micro) }
    }
}

impl Stamp {
    /// What a number in a cell stands for, given which parts its format's section shows, as
    /// `(date, time)` (`NumberFormat::date_shape`), to the millisecond:
    ///
    /// - A number with no place on the calendar stays a number (`None`): a negative one, one past
    ///   9999-12-31, and in the 1900 system the days that never were, serial 60 (29 February 1900)
    ///   and anything below 1 ("0 January 1900") under a format that shows a date.
    /// - A time format with a number below 1 gives a time, in either date system.
    /// - Anything else that holds more than its format shows gives a date and time, so that nothing
    ///   is lost: a date format with a time of day, and a time format with a whole number of days.
    pub fn from_serial(sys: DateSystem, shape: (bool, bool), n: f64) -> Option<Stamp> {
        let (date, time) = shape;
        if !date && !time {
            return None;
        }
        // The last moment a serial holds rounds up to the millisecond after it: it reads as the
        // last millisecond, rather than as a number or the next day.
        let top = max_serial(sys) as f64 + 1.0;
        let at = datetime_from_serial(sys, n).or_else(|| if (0.0..top).contains(&n) { datetime_from_serial(sys, top - LAST_MILLI) } else { None })?;
        let clock = Clock { hour: narrow(at.hour)?, minute: narrow(at.minute)?, second: narrow(at.second)?, micro: at.milli * 1000 };
        if !date && n < 1.0 {
            let last = (n * MILLIS_PER_DAY).round() >= MILLIS_PER_DAY;
            return Some(Stamp::Time(if last { Clock { hour: 23, minute: 59, second: 59, micro: 999_000 } } else { clock }));
        }
        if at.day == 0 || (at.year == 1900 && at.month == 2 && at.day == 29) {
            return None;
        }
        let day = Day { year: at.year, month: narrow(at.month)?, day: narrow(at.day)? };
        Some(if date && !time && clock == Clock::MIDNIGHT { Stamp::Date(day) } else { Stamp::DateTime(day, clock) })
    }

    /// The serial number in `sys`. `#NUM!` for what a serial can't hold: a date before the system's
    /// first year or after 9999, and a duration longer than the whole calendar.
    pub fn serial(self, sys: DateSystem) -> Result<f64, CellError> {
        match self {
            Stamp::Date(d) => d.serial(sys),
            Stamp::DateTime(d, c) => {
                // Late in the calendar a serial can't tell the last microsecond of a day from the
                // day after it. It stays in its own day, so the last moment of all stays a date.
                let day = d.serial(sys)?;
                Ok((day + c.fraction()).min((day + 1.0).next_down()))
            }
            Stamp::Time(c) => Ok(c.fraction()),
            Stamp::Duration(days) if days.abs() < max_serial(DateSystem::D1900) as f64 + 1.0 => Ok(days),
            Stamp::Duration(_) => Err(CellError::Num),
        }
    }

    /// Python's name for its type, for the card.
    pub fn type_name(self) -> &'static str {
        match self {
            Stamp::Date(_) => "date",
            Stamp::DateTime(..) => "datetime",
            Stamp::Time(_) => "time",
            Stamp::Duration(_) => "timedelta",
        }
    }

    /// The stamp in words, for the card.
    pub fn preview(self) -> String {
        match self {
            Stamp::Date(d) => d.text(),
            Stamp::DateTime(d, c) => format!("{} {}", d.text(), c.text()),
            Stamp::Time(c) => c.text(),
            Stamp::Duration(days) => format!("{days} days"),
        }
    }
}

impl Serialize for Stamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(1))?;
        match self {
            Stamp::Date(d) => m.serialize_entry("date", &d.text())?,
            Stamp::DateTime(d, c) => m.serialize_entry("dateTime", &format!("{}T{}", d.text(), c.text()))?,
            Stamp::Time(c) => m.serialize_entry("time", &c.text())?,
            Stamp::Duration(days) => m.serialize_entry("duration", days)?,
        }
        m.end()
    }
}

/// The day and clock of a `dateTime`'s text.
pub(super) fn parse_date_time(text: &str) -> Option<Stamp> {
    let (day, clock) = text.split_once('T')?;
    Some(Stamp::DateTime(Day::parse(day)?, Clock::parse(clock)?))
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn two_digits(s: &str) -> Option<u8> {
    if s.len() == 2 && digits(s) { s.parse().ok() } else { None }
}

fn narrow(n: u32) -> Option<u8> {
    u8::try_from(n).ok()
}
