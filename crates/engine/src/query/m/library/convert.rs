//! Conversions: `Text.From`, `Number.From`, `Date.From`, `DateTime.From`, `Logical.From`, and the
//! culture-aware value conversion Change Type (`Table.TransformColumnTypes`) uses.

use gridcraft_core::DateSystem;

use super::super::eval::Ev;
use super::super::value::{MError, MResult, MType, MValue, duration_text};
use super::number::round;
use super::{Args, cant};
use crate::query::types::{Culture, parse_datetime, parse_number, parse_time};

/// Days from 1899-12-30 (OLE Automation dates, which `Number.From(date)` gives) to 1970-01-01.
const OLE_1970: f64 = 25_569.0;

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "Text.From" => match a.at(0) {
            MValue::Null => MValue::Null,
            v => MValue::text(text_of(v, a.opt_text(1)?.as_deref()).ok_or_else(|| cant(v, "Text"))?),
        },
        "Number.From" | "Number.FromText" => {
            if name == "Number.FromText" && !matches!(a.at(0), MValue::Text(_) | MValue::Null) {
                return Err(cant(a.at(0), "Text"));
            }
            raise(convert(a.at(0), &MType::prim("number"), a.opt_text(1)?.as_deref(), ev.sys))?
        }
        "Date.From" => raise(convert(a.at(0), &MType::prim("date"), a.opt_text(1)?.as_deref(), ev.sys))?,
        "DateTime.From" => raise(convert(a.at(0), &MType::prim("datetime"), a.opt_text(1)?.as_deref(), ev.sys))?,
        "Logical.From" => raise(convert(a.at(0), &MType::prim("logical"), None, ev.sys))?,
        "Int64.From" | "Int32.From" | "Int16.From" | "Int8.From" | "Byte.From" | "Currency.From" | "Double.From" | "Single.From" | "Decimal.From"
        | "Percentage.From" => {
            let facet = match name {
                "Int64.From" => Some("Int64.Type"),
                "Int32.From" => Some("Int32.Type"),
                "Int16.From" => Some("Int16.Type"),
                "Int8.From" => Some("Int8.Type"),
                "Byte.From" => Some("Byte.Type"),
                "Currency.From" => Some("Currency.Type"),
                _ => None,
            };
            let ty = MType { kind: "number", nullable: false, facet, fields: vec![] };
            let culture = a.opt_text(1)?;
            let mode = if a.has(2) { Some(a.int(2, 4)?) } else { None };
            match (facet, mode) {
                // An explicit rounding mode (RoundingMode.*) for the whole-number types.
                (Some(f), Some(m)) if f != "Currency.Type" => match raise(convert(a.at(0), &MType::prim("number"), culture.as_deref(), ev.sys))? {
                    MValue::Number(x) => MValue::Number(round(x, 0, m)),
                    v => v,
                },
                _ => raise(convert(a.at(0), &ty, culture.as_deref(), ev.sys))?,
            }
        }
        "Logical.ToText" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Logical(b) => MValue::text(if *b { "true" } else { "false" }),
            other => return Err(cant(other, "Logical")),
        },
        "Logical.FromText" => match a.at(0) {
            MValue::Null => MValue::Null,
            MValue::Text(_) => raise(convert(a.at(0), &MType::prim("logical"), None, ev.sys))?,
            other => return Err(cant(other, "Text")),
        },
        "Value.Is" => match a.at(1) {
            MValue::Type(t) => MValue::Logical(super::super::eval::conforms(a.at(0), t)),
            other => return Err(cant(other, "Type")),
        },
        "Value.Type" => MValue::Type(std::rc::Rc::new(MType::prim(match a.at(0) {
            MValue::Error(_) | MValue::Lazy(_) => "any",
            v => v.kind(),
        }))),
        "Value.Equals" => MValue::Logical(super::super::value::equal(a.at(0), a.at(1))),
        _ => return Ok(None),
    };
    Ok(Some(r))
}

/// A held conversion error raised as the function's error.
pub fn raise(v: MValue) -> MResult<MValue> {
    match v {
        MValue::Error(e) => Err((*e).clone()),
        v => Ok(v),
    }
}

pub fn number_text(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else {
        gridcraft_core::number_to_text(n)
    }
}

/// Text for a value as `Text.From` gives it (`None` for values that have no text form).
pub fn text_of(v: &MValue, culture: Option<&str>) -> Option<String> {
    let pat = super::date::patterns(culture);
    Some(match v {
        MValue::Text(t) => t.to_string(),
        MValue::Number(n) => {
            let s = number_text(*n);
            if Culture::of(culture).decimal == ',' { s.replace('.', ",") } else { s }
        }
        MValue::Logical(b) => if *b { "true" } else { "false" }.to_string(),
        MValue::Date(d) => super::date::format_date(*d as f64, pat.short_date, culture),
        MValue::DateTime(d) => super::date::format_date(*d, &format!("{} {}", pat.short_date, pat.long_time), culture),
        MValue::DateTimeZone(d, o) => format!(
            "{} {}{:02}:{:02}",
            super::date::format_date(*d, &format!("{} {}", pat.short_date, pat.long_time), culture),
            if *o < 0 { '-' } else { '+' },
            o.abs() / 60,
            o.abs() % 60
        ),
        MValue::Time(t) => super::date::format_date(*t, pat.long_time, culture),
        MValue::Duration(d) => duration_text(*d),
        _ => return None,
    })
}

pub fn bad(kind: &str) -> MValue {
    MValue::error(MError::format(format!("We couldn't convert to {kind}.")))
}

pub fn ole_days(serial_1900: f64) -> f64 {
    serial_1900 - OLE_1970
}

/// A value converted to `ty` (Change Type, `Number.From`…); a value that can't be converted
/// becomes a held `DataFormat.Error`.
pub fn convert(v: &MValue, ty: &MType, culture: Option<&str>, _sys: DateSystem) -> MValue {
    if v.is_null() || matches!(v, MValue::Error(_)) {
        return v.clone();
    }
    let cul = Culture::of(culture);
    match ty.kind {
        "any" | "anynonnull" => v.clone(),
        "text" => match text_of(v, culture) {
            Some(s) => MValue::text(s),
            None => bad("Text"),
        },
        "number" => {
            let n = match v {
                MValue::Number(n) => Some(*n),
                MValue::Logical(b) => Some(if *b { 1.0 } else { 0.0 }),
                MValue::Text(t) => {
                    let t = t.trim();
                    if t.is_empty() {
                        return MValue::Null;
                    }
                    parse_number(t, &cul).or_else(|| parse_datetime(t, &cul, DateSystem::D1900).map(|(n, _)| n))
                }
                MValue::Date(d) => Some(*d as f64 + OLE_1970),
                MValue::DateTime(d) | MValue::DateTimeZone(d, _) => Some(d + OLE_1970),
                MValue::Time(t) | MValue::Duration(t) => Some(*t),
                _ => None,
            };
            match n {
                Some(n) => MValue::Number(match ty.facet {
                    Some("Int64.Type" | "Int32.Type" | "Int16.Type" | "Int8.Type" | "Byte.Type") => round(n, 0, 4),
                    Some("Currency.Type") => round(n, 4, 4),
                    _ => n,
                }),
                None => bad("Number"),
            }
        }
        "date" | "datetime" | "datetimezone" => {
            let d: Option<f64> = match v {
                MValue::Date(d) => Some(*d as f64),
                MValue::DateTime(d) | MValue::DateTimeZone(d, _) => Some(*d),
                MValue::Number(n) if n.is_finite() => Some(ole_days(*n)),
                MValue::Text(t) => {
                    let t = t.trim();
                    if t.is_empty() {
                        return MValue::Null;
                    }
                    parse_datetime(t, &cul, DateSystem::D1900).map(|(n, _)| ole_days(n))
                }
                _ => None,
            };
            match d.filter(|d| d.is_finite() && (-719_162.0..=2_932_896.0).contains(d)) {
                Some(d) => match ty.kind {
                    "date" => MValue::Date(d.floor() as i64),
                    "datetimezone" => match v {
                        MValue::DateTimeZone(..) => v.clone(),
                        _ => MValue::DateTimeZone(d, 0),
                    },
                    _ => MValue::DateTime(d),
                },
                None => MValue::error(MError::format(format!(
                    "We couldn't parse the input provided as a {} value.",
                    if ty.kind == "date" { "Date" } else { "DateTime" }
                ))),
            }
        }
        "time" => {
            let t = match v {
                MValue::Time(t) => Some(*t),
                MValue::DateTime(d) | MValue::DateTimeZone(d, _) => Some(d.rem_euclid(1.0)),
                MValue::Number(n) if (0.0..1.0).contains(n) => Some(*n),
                MValue::Text(s) => {
                    let s = s.trim();
                    if s.is_empty() {
                        return MValue::Null;
                    }
                    parse_time(s).or_else(|| parse_datetime(s, &cul, DateSystem::D1900).map(|(n, _)| n.rem_euclid(1.0)))
                }
                _ => None,
            };
            match t {
                Some(t) => MValue::Time(t),
                None => MValue::error(MError::format("We couldn't parse the input provided as a Time value.")),
            }
        }
        "duration" => {
            let d = match v {
                MValue::Duration(d) => Some(*d),
                MValue::Number(n) => Some(*n),
                MValue::Time(t) => Some(*t),
                MValue::Text(s) => parse_duration(s.trim()),
                _ => None,
            };
            match d {
                Some(d) => MValue::Duration(d),
                None => MValue::error(MError::format("We couldn't parse the input provided as a Duration value.")),
            }
        }
        "logical" => match v {
            MValue::Logical(_) => v.clone(),
            MValue::Number(n) => MValue::Logical(*n != 0.0),
            MValue::Text(t) if t.trim().eq_ignore_ascii_case("true") => MValue::Logical(true),
            MValue::Text(t) if t.trim().eq_ignore_ascii_case("false") => MValue::Logical(false),
            _ => bad("Logical"),
        },
        kind if kind == v.kind() => v.clone(),
        _ => MValue::error(MError::expr(format!("We cannot convert a value of type {} to type {}.", v.kind(), ty.kind))),
    }
}

/// `[-][d.]hh:mm[:ss[.fff]]`.
pub fn parse_duration(s: &str) -> Option<f64> {
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let (days, rest) = match s.split_once('.') {
        Some((d, r)) if r.contains(':') && !d.contains(':') => (d.parse::<f64>().ok()?, r),
        _ => (0.0, s),
    };
    let mut parts = rest.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next().map(str::parse).transpose().ok()?.unwrap_or(0.0);
    if parts.next().is_some() {
        return None;
    }
    let d = days + h / 24.0 + m / 1440.0 + sec / 86_400.0;
    Some(if neg { -d } else { d })
}

#[cfg(test)]
mod tests {
    use super::super::super::value::days;
    use super::*;

    #[test]
    fn conversions_follow_m() {
        let n = MType::prim("number");
        assert!(matches!(convert(&MValue::text("1,5"), &n, Some("de-DE"), DateSystem::D1900), MValue::Number(x) if x == 1.5));
        assert!(matches!(convert(&MValue::text("abc"), &n, None, DateSystem::D1900), MValue::Error(_)));
        let int = MType::facet("number", "Int64.Type");
        assert!(matches!(convert(&MValue::Number(2.5), &int, None, DateSystem::D1900), MValue::Number(x) if x == 2.0));
        let d = convert(&MValue::text("1/31/2026"), &MType::prim("date"), None, DateSystem::D1900);
        assert!(matches!(d, MValue::Date(x) if x == days(2026, 1, 31).unwrap()), "{d:?}");
        assert!(
            matches!(convert(&MValue::Number(46_053.0), &MType::prim("date"), None, DateSystem::D1900), MValue::Date(x) if x == days(2026, 1, 31).unwrap())
        );
        assert_eq!(text_of(&MValue::Date(days(2026, 1, 31).unwrap()), None).unwrap(), "1/31/2026");
        assert_eq!(text_of(&MValue::Number(1.5), Some("de-DE")).unwrap(), "1,5");
        assert!(
            matches!(convert(&MValue::text("1.02:30:00"), &MType::prim("duration"), None, DateSystem::D1900), MValue::Duration(x) if (x - (1.0 + 2.5 / 24.0)).abs() < 1e-9)
        );
    }
}
