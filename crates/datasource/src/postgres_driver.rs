//! PostgreSQL through the synchronous `postgres` client.

use std::str::FromStr;
use std::time::Duration;

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use gridcraft_engine::datasource::{DbRequest, DbTable, DbValue};
use postgres::fallible_iterator::FallibleIterator;
use postgres::types::{FromSql, Type};
use postgres::{Config, Row};
use tokio_postgres_rustls::MakeRustlsConnect;

pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    let (connection, verify) = crate::tls::postgres_ssl_mode(&req.connection);
    let mut cfg = Config::from_str(&connection).map_err(|e| format!("Invalid PostgreSQL connection: {e}"))?;
    if let Some(u) = &req.user {
        cfg.user(u);
    }
    if let Some(p) = &req.password {
        cfg.password(p);
    }
    cfg.connect_timeout(Duration::from_secs(req.timeout_secs));
    cfg.application_name("GridCraft");
    // `sslmode` (default `prefer`) decides whether TLS is tried, required or skipped.
    let tls = MakeRustlsConnect::new(crate::tls::client_config(verify)?);
    let mut client = cfg.connect(tls).map_err(|e| format!("Could not connect to PostgreSQL: {}", with_causes(&e)))?;
    client.batch_execute(&format!("SET statement_timeout = {}", req.timeout_secs.saturating_mul(1000))).map_err(|e| format!("PostgreSQL: {e}"))?;
    let stmt = client.prepare(&req.query).map_err(|e| format!("PostgreSQL: {e}"))?;
    let columns: Vec<String> = stmt.columns().iter().map(|c| c.name().to_string()).collect();
    let mut it = client.query_raw(&stmt, std::iter::empty::<i32>()).map_err(|e| format!("PostgreSQL: {e}"))?;
    let mut table = DbTable { columns, rows: Vec::new(), truncated: false };
    while let Some(row) = it.next().map_err(|e| format!("PostgreSQL: {e}"))? {
        if table.rows.len() >= req.max_rows {
            table.truncated = true;
            break;
        }
        table.rows.push((0..row.len()).map(|i| value(&row, i)).collect());
    }
    Ok(table)
}

/// The value of column `i` through `f`: NULL as [`DbValue::Null`], and a value the client can't
/// decode as text that says so, rather than an empty cell.
fn get<'a, T: FromSql<'a>>(row: &'a Row, i: usize, f: impl FnOnce(T) -> DbValue) -> DbValue {
    match row.try_get::<_, Option<T>>(i) {
        Ok(Some(v)) => f(v),
        Ok(None) => DbValue::Null,
        Err(e) => DbValue::Text(format!("(unreadable: {e})")),
    }
}

fn value(row: &Row, i: usize) -> DbValue {
    let Some(col) = row.columns().get(i) else { return DbValue::Null };
    let ty = col.type_();
    match *ty {
        Type::BOOL => get(row, i, DbValue::Bool),
        Type::INT2 => get(row, i, |n: i16| DbValue::Int(n.into())),
        Type::INT4 => get(row, i, |n: i32| DbValue::Int(n.into())),
        Type::INT8 => get(row, i, DbValue::Int),
        Type::OID => get(row, i, |n: u32| DbValue::Int(n.into())),
        Type::FLOAT4 => get(row, i, |n: f32| DbValue::Float(n.into())),
        Type::FLOAT8 => get(row, i, DbValue::Float),
        Type::NUMERIC => get(row, i, |n: Numeric| n.0.parse::<f64>().ok().filter(|f| f.is_finite()).map_or(DbValue::Text(n.0), DbValue::Float)),
        Type::DATE => get(row, i, date),
        Type::TIMESTAMP => get(row, i, datetime),
        Type::TIMESTAMPTZ => get(row, i, |d: chrono::DateTime<chrono::Utc>| datetime(d.naive_utc())),
        Type::TIME => get(row, i, time),
        Type::UUID => get(row, i, |u: uuid::Uuid| DbValue::Text(u.to_string())),
        Type::JSON | Type::JSONB => get(row, i, |j: serde_json::Value| DbValue::Text(j.to_string())),
        Type::BYTEA => get(row, i, |b: Vec<u8>| DbValue::Text(format!("({} bytes)", b.len()))),
        _ => match row.try_get::<_, Option<String>>(i) {
            Ok(s) => s.map_or(DbValue::Null, DbValue::Text),
            // A type with no text decoding (arrays, ranges, geometry…): name it instead of failing.
            Err(_) => DbValue::Text(format!("({})", ty.name())),
        },
    }
}

/// A PostgreSQL `NUMERIC` as decimal text, at any size and with `NaN` and the infinities, so no
/// value is lost to a fixed-size decimal type.
struct Numeric(String);

impl<'a> FromSql<'a> for Numeric {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        numeric_text(raw).map(Numeric).ok_or_else(|| "malformed NUMERIC".into())
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

/// Decodes the binary `NUMERIC` form: digit count, weight of the first digit, sign, display
/// scale, then base-10000 digits.
fn numeric_text(raw: &[u8]) -> Option<String> {
    let word = |at: usize| raw.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let (ndigits, weight, sign, dscale) = (usize::from(word(0)?), i64::from(word(2)? as i16), word(4)?, usize::from(word(6)?));
    match sign {
        0xC000 => return Some("NaN".into()),
        0xD000 => return Some("Infinity".into()),
        0xF000 => return Some("-Infinity".into()),
        _ => {}
    }
    let digits: Vec<u16> = (0..ndigits).map(|k| word(8 + 2 * k)).collect::<Option<_>>()?;
    let digit = |pos: i64| usize::try_from(pos).ok().and_then(|p| digits.get(p)).copied().unwrap_or(0);
    let mut s = String::new();
    if sign == 0x4000 {
        s.push('-');
    }
    if weight < 0 {
        s.push('0');
    } else {
        s.push_str(&digit(0).to_string());
        for pos in 1..=weight {
            s.push_str(&format!("{:04}", digit(pos)));
        }
    }
    if dscale > 0 {
        let mut frac = String::new();
        let mut pos = weight + 1;
        while frac.len() < dscale {
            frac.push_str(&format!("{:04}", digit(pos)));
            pos += 1;
        }
        frac.truncate(dscale);
        s.push('.');
        s.push_str(&frac);
    }
    Some(s)
}

/// The error and its causes ("error performing TLS handshake: invalid peer certificate: …").
fn with_causes(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        let text = c.to_string();
        if !s.contains(&text) {
            s.push_str(": ");
            s.push_str(&text);
        }
        cause = c.source();
    }
    s
}

fn date(d: NaiveDate) -> DbValue {
    DbValue::Date(d.year(), d.month(), d.day())
}

fn datetime(d: NaiveDateTime) -> DbValue {
    let t = d.time();
    DbValue::DateTime(d.year(), d.month(), d.day(), t.hour(), t.minute(), seconds(t))
}

fn time(t: NaiveTime) -> DbValue {
    DbValue::Time(t.hour(), t.minute(), seconds(t))
}

fn seconds(t: NaiveTime) -> f64 {
    f64::from(t.second()) + f64::from(t.nanosecond().min(999_999_999)) / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The binary form of a NUMERIC with these base-10000 digits.
    fn raw(weight: i16, sign: u16, dscale: u16, digits: &[u16]) -> Vec<u8> {
        let mut v = vec![];
        for w in [digits.len() as u16, weight as u16, sign, dscale].iter().chain(digits) {
            v.extend(w.to_be_bytes());
        }
        v
    }

    #[test]
    fn numeric_decodes_any_size() {
        assert_eq!(numeric_text(&raw(0, 0, 2, &[12, 3400])).as_deref(), Some("12.34"));
        assert_eq!(numeric_text(&raw(-1, 0x4000, 3, &[50])).as_deref(), Some("-0.005"));
        assert_eq!(numeric_text(&raw(-2, 0, 8, &[12])).as_deref(), Some("0.00000012"));
        assert_eq!(numeric_text(&raw(1, 0, 0, &[1])).as_deref(), Some("10000"));
        assert_eq!(numeric_text(&raw(0, 0, 0, &[])).as_deref(), Some("0"));
        // 38 digits: beyond a 96-bit decimal.
        let big = raw(9, 0, 0, &[12, 3456, 7890, 1234, 5678, 9012, 3456, 7890, 1234, 5678]);
        assert_eq!(numeric_text(&big).as_deref(), Some("12345678901234567890123456789012345678"));
        assert_eq!(numeric_text(&raw(0, 0xC000, 0, &[])).as_deref(), Some("NaN"));
        assert_eq!(numeric_text(&[0, 1]), None);
    }
}
