//! `Date.*`, `DateTime.*`, `Duration.*` and the `#date`, `#datetime`, `#datetimezone`, `#time` and
//! `#duration` constructors.

use super::super::eval::Ev;
use super::super::value::{MError, MResult, MType, MValue, days, ymd};
use super::{Args, cant, show};

pub fn call(_ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Date.Year" | "Date.Month" | "Date.Day" => match day_of(a.at(0))? {
            None => MValue::Null,
            Some(d) => {
                let (y, m, dd) = ymd(d);
                MValue::Number(f64::from(match name {
                    "Date.Year" => y,
                    "Date.Month" => m as i32,
                    _ => dd as i32,
                }))
            }
        },
        "Date.AddDays" => {
            let n = a.num(1)?;
            if !n.is_finite() || n.fract() != 0.0 || n.abs() > 4_000_000.0 {
                return Err(cant(a.at(1), "Number"));
            }
            match a.at(0) {
                MValue::Null => MValue::Null,
                MValue::Date(d) => MValue::Date(checked_day(d.saturating_add(n as i64))?),
                MValue::DateTime(d) => MValue::DateTime(d + n),
                MValue::DateTimeZone(d, o) => MValue::DateTimeZone(d + n, *o),
                other => return Err(cant(other, "Date")),
            }
        }
        "DateTime.Date" => match day_of(a.at(0))? {
            None => MValue::Null,
            Some(d) => MValue::Date(d),
        },
        "DateTime.Time" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::DateTime(d) | MValue::DateTimeZone(d, _) => MValue::Time(d.rem_euclid(1.0)),
            MValue::Time(t) => MValue::Time(*t),
            other => return Err(cant(other, "DateTime")),
        },
        "DateTime.LocalNow" => MValue::DateTime(gridcraft_core::date::unix_now_ms().unwrap_or(0.0) / 86_400_000.0),
        "#date" => MValue::Date(days(whole(a, 0)?, whole(a, 1)?, whole(a, 2)?)?),
        "#datetime" => MValue::DateTime(days(whole(a, 0)?, whole(a, 1)?, whole(a, 2)?)? as f64 + time_of(a, 3)?),
        "#datetimezone" => {
            let (oh, om) = (whole(a, 6)?, whole(a, 7)?);
            if !(-14..=14).contains(&oh) || !(-59..=59).contains(&om) {
                return Err(MError::expr("The time zone offset is out of range."));
            }
            MValue::DateTimeZone(days(whole(a, 0)?, whole(a, 1)?, whole(a, 2)?)? as f64 + time_of(a, 3)?, (oh * 60 + om) as i32)
        }
        "#time" => MValue::Time(time_of(a, 0)?),
        "#duration" => MValue::Duration(a.num(0)? + a.num(1)? / 24.0 + a.num(2)? / 1440.0 + a.num(3)? / 86_400.0),
        "Date.AddMonths" | "Date.AddYears" | "Date.AddQuarters" | "Date.AddWeeks" => match parts(a.at(0))? {
            None => MValue::Null,
            Some(p) => {
                let n = a.int(1, 0)?;
                if n.abs() > 120_000 {
                    return Err(cant(a.at(1), "Number"));
                }
                let day = match name {
                    "Date.AddWeeks" => checked_day(p.day.saturating_add(n * 7))?,
                    "Date.AddMonths" => add_months(p.day, n)?,
                    "Date.AddQuarters" => add_months(p.day, n * 3)?,
                    _ => add_months(p.day, n * 12)?,
                };
                p.with(day, p.frac)
            }
        },
        "Date.StartOfDay"
        | "Date.EndOfDay"
        | "Date.StartOfWeek"
        | "Date.EndOfWeek"
        | "Date.StartOfMonth"
        | "Date.EndOfMonth"
        | "Date.StartOfQuarter"
        | "Date.EndOfQuarter"
        | "Date.StartOfYear"
        | "Date.EndOfYear" => match parts(a.at(0))? {
            None => MValue::Null,
            Some(p) => {
                let (y, m, _) = ymd(p.day);
                let (y, m) = (i64::from(y), i64::from(m));
                let first = first_day(a.at(1))?;
                let (start, end) = match name.trim_start_matches("Date.").trim_start_matches("StartOf").trim_start_matches("EndOf") {
                    "Day" => (p.day, p.day),
                    "Week" => {
                        let s = p.day - (weekday(p.day) - first).rem_euclid(7);
                        (s, s + 6)
                    }
                    "Month" => (days(y, m, 1)?, days(y, m, i64::from(gridcraft_core::date::days_in_month(y as i32, m as u32)))?),
                    "Quarter" => {
                        let q0 = (m - 1) / 3 * 3 + 1;
                        (days(y, q0, 1)?, days(y, q0 + 2, i64::from(gridcraft_core::date::days_in_month(y as i32, (q0 + 2) as u32)))?)
                    }
                    _ => (days(y, 1, 1)?, days(y, 12, 31)?),
                };
                if name.starts_with("Date.StartOf") { p.with(start, 0.0) } else { p.with(end, END_OF_DAY) }
            }
        },
        "Date.DayOfWeek" => match parts(a.at(0))? {
            None => MValue::Null,
            Some(p) => MValue::Number((weekday(p.day) - first_day(a.at(1))?).rem_euclid(7) as f64),
        },
        "Date.DayOfWeekName" | "Date.MonthName" => match parts(a.at(0))? {
            None => MValue::Null,
            Some(p) => {
                let culture = a.opt_text(1)?;
                let names = names(culture.as_deref());
                MValue::text(if name == "Date.MonthName" {
                    names.months.get((ymd(p.day).1 as usize).saturating_sub(1)).copied().unwrap_or("")
                } else {
                    names.days.get(weekday(p.day) as usize).copied().unwrap_or("")
                })
            }
        },
        "Date.QuarterOfYear" | "Date.DayOfYear" | "Date.DaysInMonth" | "Date.WeekOfYear" | "Date.WeekOfMonth" => match parts(a.at(0))? {
            None => MValue::Null,
            Some(p) => {
                let (y, m, d) = ymd(p.day);
                let first = first_day(a.at(1))?;
                let week_of = |from: i64| ((p.day - from) + (weekday(from) - first).rem_euclid(7)) / 7 + 1;
                MValue::Number(match name {
                    "Date.QuarterOfYear" => f64::from((m - 1) / 3 + 1),
                    "Date.DayOfYear" => (p.day - days(i64::from(y), 1, 1)? + 1) as f64,
                    "Date.DaysInMonth" => f64::from(gridcraft_core::date::days_in_month(y, m)),
                    "Date.WeekOfYear" => week_of(days(i64::from(y), 1, 1)?) as f64,
                    _ => week_of(p.day - i64::from(d) + 1) as f64,
                })
            }
        },
        "Date.IsLeapYear" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Number(n) if n.fract() == 0.0 && n.abs() < 1.0e6 => MValue::Logical(gridcraft_core::date::is_leap(*n as i32)),
            v => match parts(v)? {
                Some(p) => MValue::Logical(gridcraft_core::date::is_leap(ymd(p.day).0)),
                None => MValue::Null,
            },
        },
        "Date.ToText" | "DateTime.ToText" | "DateTimeZone.ToText" | "Time.ToText" => {
            let (format, culture) = format_options(a)?;
            match a.at(0) {
                MValue::Null => MValue::Null,
                MValue::Time(t) => {
                    MValue::text(format_date(*t, format.as_deref().unwrap_or(patterns(culture.as_deref()).long_time), culture.as_deref()))
                }
                v => match parts(v)? {
                    None => MValue::Null,
                    Some(p) => {
                        let pat = patterns(culture.as_deref());
                        let default = if name == "Date.ToText" || matches!(v, MValue::Date(_)) {
                            pat.short_date.to_string()
                        } else {
                            format!("{} {}", pat.short_date, pat.long_time)
                        };
                        MValue::text(format_date(p.day as f64 + p.frac, format.as_deref().unwrap_or(&default), culture.as_deref()))
                    }
                },
            }
        }
        "Date.FromText" | "DateTime.FromText" | "Time.FromText" => {
            let (_, culture) = format_options(a)?;
            let kind = match name {
                "Date.FromText" => "date",
                "DateTime.FromText" => "datetime",
                _ => "time",
            };
            match a.at(0) {
                MValue::Null => MValue::Null,
                MValue::Text(_) => {
                    match super::convert::convert(a.at(0), &MType::prim(kind), culture.as_deref(), gridcraft_core::DateSystem::D1900) {
                        MValue::Error(e) => return Err((*e).clone()),
                        v => v,
                    }
                }
                other => return Err(cant(other, "Text")),
            }
        }
        "Time.From" => match super::convert::convert(a.at(0), &MType::prim("time"), a.opt_text(1)?.as_deref(), gridcraft_core::DateSystem::D1900) {
            MValue::Error(e) => return Err((*e).clone()),
            v => v,
        },
        "Time.Hour" | "Time.Minute" | "Time.Second" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Time(t) | MValue::DateTime(t) | MValue::DateTimeZone(t, _) => {
                let (h, mi, s) = hms(t.rem_euclid(1.0));
                MValue::Number(match name {
                    "Time.Hour" => f64::from(h),
                    "Time.Minute" => f64::from(mi),
                    _ => s,
                })
            }
            other => return Err(cant(other, "Time")),
        },
        "DateTime.FixedLocalNow" => MValue::DateTime(now()),
        "DateTimeZone.UtcNow" | "DateTimeZone.FixedUtcNow" => MValue::DateTimeZone(now(), 0),
        "Duration.Days"
        | "Duration.Hours"
        | "Duration.Minutes"
        | "Duration.Seconds"
        | "Duration.TotalDays"
        | "Duration.TotalHours"
        | "Duration.TotalMinutes"
        | "Duration.TotalSeconds" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Duration(d) => {
                let whole = d.trunc();
                let (h, mi, s) = hms(d.abs().fract());
                let sign = if *d < 0.0 { -1.0 } else { 1.0 };
                MValue::Number(match name {
                    "Duration.Days" => whole,
                    "Duration.Hours" => sign * f64::from(h),
                    "Duration.Minutes" => sign * f64::from(mi),
                    "Duration.Seconds" => sign * s,
                    "Duration.TotalDays" => *d,
                    "Duration.TotalHours" => d * 24.0,
                    "Duration.TotalMinutes" => d * 1440.0,
                    _ => d * 86_400.0,
                })
            }
            other => return Err(cant(other, "Duration")),
        },
        "Duration.From" => match super::convert::convert(a.at(0), &MType::prim("duration"), None, gridcraft_core::DateSystem::D1900) {
            MValue::Error(e) => return Err((*e).clone()),
            v => v,
        },
        "Duration.ToText" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Duration(d) => MValue::text(super::super::value::duration_text(*d)),
            other => return Err(cant(other, "Duration")),
        },
        n if n.starts_with("Date.IsIn") => match parts(a.at(0))? {
            None => MValue::Null,
            Some(p) => MValue::Logical(is_in(n, p.day, if a.has(1) { a.int(1, 0)? } else { 0 })?),
        },
        _ => return Ok(None),
    };
    Ok(Some(r))
}

pub fn whole(a: &Args<'_>, i: usize) -> MResult<i64> {
    let n = a.num(i)?;
    if n.fract() != 0.0 || !n.is_finite() || n.abs() > 1.0e9 {
        return Err(MError::expr(format!("{} isn't a whole number in range.", show(a.at(i)))));
    }
    Ok(n as i64)
}

pub fn time_of(a: &Args<'_>, i: usize) -> MResult<f64> {
    let (h, m, s) = (a.num(i)?, a.num(i + 1)?, a.num(i + 2)?);
    if !(0.0..24.0).contains(&h) || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&s) || h.fract() != 0.0 || m.fract() != 0.0 {
        return Err(MError::expr(format!("#time({h}, {m}, {s}) isn't a valid time.")));
    }
    Ok(h / 24.0 + m / 1440.0 + s / 86_400.0)
}

pub fn day_of(v: &MValue) -> MResult<Option<i64>> {
    match v {
        MValue::Null => Ok(None),
        MValue::Date(d) => Ok(Some(*d)),
        MValue::DateTime(d) | MValue::DateTimeZone(d, _) => Ok(Some(d.floor() as i64)),
        other => Err(cant(other, "Date")),
    }
}

pub fn checked_day(d: i64) -> MResult<i64> {
    let (y, _, _) = ymd(d);
    if (1..=9999).contains(&y) { Ok(d) } else { Err(MError::expr("The date is out of range.")) }
}

/// The last moment of a day as a fraction: as close to midnight as a date-time this far from 1970
/// holds (Excel's 23:59:59.9999999 is finer than a 64-bit float keeps there).
const END_OF_DAY: f64 = 1.0 - 1.0e-10;

/// A date, date-time or date-time-zone value taken apart, to be rebuilt as the same kind.
#[derive(Clone, Copy)]
struct Parts {
    day: i64,
    frac: f64,
    /// `None` for a date, `Some(None)` for a date-time, `Some(Some(offset))` with a zone.
    time: Option<Option<i32>>,
}

impl Parts {
    fn with(self, day: i64, frac: f64) -> MValue {
        match self.time {
            None => MValue::Date(day),
            Some(None) => MValue::DateTime(day as f64 + frac),
            Some(Some(o)) => MValue::DateTimeZone(day as f64 + frac, o),
        }
    }
}

fn parts(v: &MValue) -> MResult<Option<Parts>> {
    Ok(match v {
        MValue::Null => None,
        MValue::Date(d) => Some(Parts { day: *d, frac: 0.0, time: None }),
        MValue::DateTime(d) => Some(Parts { day: d.floor() as i64, frac: d - d.floor(), time: Some(None) }),
        MValue::DateTimeZone(d, o) => Some(Parts { day: d.floor() as i64, frac: d - d.floor(), time: Some(Some(*o)) }),
        other => return Err(cant(other, "Date")),
    })
}

/// 0 = Sunday … 6 = Saturday (1970-01-01 was a Thursday).
fn weekday(day: i64) -> i64 {
    (day + 4).rem_euclid(7)
}

/// The first day of the week argument (`Day.Sunday` … `Day.Saturday`); Sunday by default.
fn first_day(v: &MValue) -> MResult<i64> {
    match v {
        MValue::Null => Ok(0),
        MValue::Number(n) if (0.0..7.0).contains(n) && n.fract() == 0.0 => Ok(*n as i64),
        other => Err(cant(other, "Day")),
    }
}

/// `day` moved by `n` months, the day of the month kept or cut to the month's last day.
fn add_months(day: i64, n: i64) -> MResult<i64> {
    let (y, m, d) = ymd(day);
    let total = i64::from(y) * 12 + i64::from(m) - 1 + n;
    let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
    if !(1..=9999).contains(&ny) {
        return Err(MError::expr("The date is out of range."));
    }
    let dim = gridcraft_core::date::days_in_month(ny as i32, nm as u32);
    days(ny, nm, i64::from(d.min(dim)))
}

fn now() -> f64 {
    gridcraft_core::date::unix_now_ms().unwrap_or(0.0) / 86_400_000.0
}

/// Hours, minutes and seconds (with the fraction) of a fraction of a day.
fn hms(frac: f64) -> (u32, u32, f64) {
    let ticks = (frac * 864_000_000_000.0 + 0.5).floor().clamp(0.0, 863_999_999_999.0) as i64;
    let secs = ticks / 10_000_000;
    ((secs / 3600) as u32, (secs / 60 % 60) as u32, (secs % 60) as f64 + (ticks % 10_000_000) as f64 / 1.0e7)
}

/// `Date.IsIn…`: whether `day` falls in the current, previous or next day, week, month, quarter
/// or year (or the previous/next `n` of them), from today.
fn is_in(name: &str, day: i64, n: i64) -> MResult<bool> {
    let today = now().floor() as i64;
    if name == "Date.IsInYearToDate" {
        return Ok(ymd(day).0 == ymd(today).0 && day <= today);
    }
    let rest = name.trim_start_matches("Date.IsIn");
    let (which, unit) = ["Current", "PreviousN", "NextN", "Previous", "Next"]
        .iter()
        .find_map(|w| rest.strip_prefix(w).map(|u| (*w, u)))
        .ok_or_else(|| MError::expr(format!("The M function '{name}' isn't supported by GridCraft yet.")))?;
    let period = |d: i64| -> i64 {
        let (y, m, _) = ymd(d);
        let (y, m) = (i64::from(y), i64::from(m));
        match unit.trim_end_matches('s') {
            "Day" => d,
            "Week" => (d - weekday(d)).div_euclid(7),
            "Month" => y * 12 + m,
            "Quarter" => y * 4 + (m - 1) / 3,
            _ => y,
        }
    };
    let (p, c) = (period(day), period(today));
    Ok(match which {
        "Current" => p == c,
        "Previous" => p == c - 1,
        "Next" => p == c + 1,
        "PreviousN" => p >= c - n && p < c,
        _ => p > c && p <= c + n,
    })
}

/// Month and day names of a culture.
pub struct Names {
    pub months: [&'static str; 12],
    pub days: [&'static str; 7],
}

pub fn names(culture: Option<&str>) -> Names {
    let lang = culture.unwrap_or("en").split(['-', '_']).next().unwrap_or("en").to_ascii_lowercase();
    match lang.as_str() {
        "de" => Names {
            months: ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
            days: ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
        },
        "nl" => Names {
            months: ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"],
            days: ["zondag", "maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag"],
        },
        "fr" => Names {
            months: ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
            days: ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"],
        },
        "es" => Names {
            months: ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
            days: ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"],
        },
        "ru" => Names {
            months: ["январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август", "сентябрь", "октябрь", "ноябрь", "декабрь"],
            days: ["воскресенье", "понедельник", "вторник", "среда", "четверг", "пятница", "суббота"],
        },
        _ => Names {
            months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
            days: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
        },
    }
}

/// A culture's standard date and time patterns.
pub struct Patterns {
    pub short_date: &'static str,
    pub long_date: &'static str,
    pub short_time: &'static str,
    pub long_time: &'static str,
}

pub fn patterns(culture: Option<&str>) -> Patterns {
    let tag = culture.unwrap_or("en-US").to_ascii_lowercase().replace('_', "-");
    let lang = tag.split('-').next().unwrap_or("en").to_string();
    let p = |short_date, long_date, short_time, long_time| Patterns { short_date, long_date, short_time, long_time };
    match (lang.as_str(), tag.as_str()) {
        (_, "en-us" | "en-ph" | "en") | ("", _) => p("M/d/yyyy", "dddd, MMMM d, yyyy", "h:mm tt", "h:mm:ss tt"),
        ("en", _) => p("dd/MM/yyyy", "dd MMMM yyyy", "HH:mm", "HH:mm:ss"),
        ("de", _) => p("dd.MM.yyyy", "dddd, d. MMMM yyyy", "HH:mm", "HH:mm:ss"),
        ("nl", _) => p("d-M-yyyy", "dddd d MMMM yyyy", "HH:mm", "HH:mm:ss"),
        ("fr", _) => p("dd/MM/yyyy", "dddd d MMMM yyyy", "HH:mm", "HH:mm:ss"),
        ("es", _) => p("dd/MM/yyyy", "dddd, d 'de' MMMM 'de' yyyy", "H:mm", "H:mm:ss"),
        ("ru", _) => p("dd.MM.yyyy", "d MMMM yyyy 'г.'", "H:mm", "H:mm:ss"),
        ("ja" | "zh" | "ko", _) => p("yyyy/MM/dd", "yyyy'年'M'月'd'日'", "H:mm", "H:mm:ss"),
        _ => match crate::query::types::Culture::of(Some(&tag)).order {
            crate::query::types::DateOrder::Dmy => p("dd.MM.yyyy", "d MMMM yyyy", "HH:mm", "HH:mm:ss"),
            crate::query::types::DateOrder::Ymd => p("yyyy-MM-dd", "yyyy MMMM d", "HH:mm", "HH:mm:ss"),
            crate::query::types::DateOrder::Mdy => p("M/d/yyyy", "dddd, MMMM d, yyyy", "h:mm tt", "h:mm:ss tt"),
        },
    }
}

/// `Date.ToText`'s options: a format text, or a record with `Format` and `Culture`; and the
/// culture argument.
fn format_options(a: &Args<'_>) -> MResult<(Option<String>, Option<String>)> {
    let text = |v: Option<&MValue>| match v {
        Some(MValue::Text(t)) => Some(t.to_string()),
        _ => None,
    };
    Ok(match a.at(1) {
        MValue::Null => (None, a.opt_text(2)?.map(|c| c.to_string())),
        MValue::Text(t) => (Some(t.to_string()), a.opt_text(2)?.map(|c| c.to_string())),
        MValue::Record(r) => (text(r.get("Format")), text(r.get("Culture")).or(a.opt_text(2)?.map(|c| c.to_string()))),
        other => return Err(cant(other, "Text")),
    })
}

/// A date-time (days since 1970-01-01 with the time as the fraction) in a .NET-style format:
/// standard (`d`, `D`, `t`, `T`, `g`, `G`, `s`, `o`, `M`, `Y`) or custom (`yyyy-MM-dd HH:mm`,
/// `dddd`, `MMM`, `tt`, `'literal'`).
pub fn format_date(value: f64, fmt: &str, culture: Option<&str>) -> String {
    let pat = patterns(culture);
    let expanded: String = match fmt {
        "d" => pat.short_date.to_string(),
        "D" => pat.long_date.to_string(),
        "t" => pat.short_time.to_string(),
        "T" => pat.long_time.to_string(),
        "g" => format!("{} {}", pat.short_date, pat.short_time),
        "G" => format!("{} {}", pat.short_date, pat.long_time),
        "s" => "yyyy'-'MM'-'dd'T'HH':'mm':'ss".to_string(),
        "o" | "O" => "yyyy'-'MM'-'dd'T'HH':'mm':'ss'.'fffffff".to_string(),
        "M" | "m" => "MMMM d".to_string(),
        "Y" | "y" => "MMMM yyyy".to_string(),
        other => other.to_string(),
    };
    let day = value.floor() as i64;
    let (y, mo, d) = ymd(day);
    let (h, mi, s) = hms(value - value.floor());
    let names = names(culture);
    let english = culture.is_none_or(|c| c.to_ascii_lowercase().starts_with("en"));
    let chars: Vec<char> = expanded.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        if c == '\'' || c == '"' {
            let mut j = i + 1;
            while let Some(&x) = chars.get(j) {
                if x == c {
                    break;
                }
                out.push(x);
                j += 1;
            }
            i = j + 1;
            continue;
        }
        if c == '\\' {
            if let Some(&x) = chars.get(i + 1) {
                out.push(x);
            }
            i += 2;
            continue;
        }
        let mut n = 1usize;
        while chars.get(i + n) == Some(&c) {
            n += 1;
        }
        let pad = |v: i64, w: usize| format!("{v:0w$}");
        match c {
            'y' => out.push_str(&match n {
                1 => (i64::from(y) % 100).to_string(),
                2 => pad(i64::from(y) % 100, 2),
                _ => pad(i64::from(y), n),
            }),
            'M' => out.push_str(&match n {
                1 => mo.to_string(),
                2 => pad(i64::from(mo), 2),
                3 => names.months.get(mo as usize - 1).map(|m| m.chars().take(3).collect()).unwrap_or_default(),
                _ => names.months.get(mo as usize - 1).map(|m| m.to_string()).unwrap_or_default(),
            }),
            'd' => out.push_str(&match n {
                1 => d.to_string(),
                2 => pad(i64::from(d), 2),
                3 => names.days.get(weekday(day) as usize).map(|m| m.chars().take(3).collect()).unwrap_or_default(),
                _ => names.days.get(weekday(day) as usize).map(|m| m.to_string()).unwrap_or_default(),
            }),
            'H' => out.push_str(&pad(i64::from(h), n.min(2))),
            'h' => out.push_str(&pad(i64::from(if h % 12 == 0 { 12 } else { h % 12 }), n.min(2))),
            'm' => out.push_str(&pad(i64::from(mi), n.min(2))),
            's' => out.push_str(&pad(s.floor() as i64, n.min(2))),
            'f' | 'F' => {
                let digits = n.min(7);
                let frac = ((s - s.floor()) * 10f64.powi(digits as i32)).floor() as i64;
                let text = pad(frac, digits);
                out.push_str(if c == 'F' { text.trim_end_matches('0') } else { &text });
            }
            't' if english => out.push_str((if h < 12 { "AM" } else { "PM" }).get(..n.min(2)).unwrap_or("")),
            't' | 'z' | 'K' | 'g' => {}
            other => {
                for _ in 0..n {
                    out.push(other);
                }
            }
        }
        i += n;
    }
    out
}
