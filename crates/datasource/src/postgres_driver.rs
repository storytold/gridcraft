//! PostgreSQL through the synchronous `postgres` client (no TLS yet).

use std::str::FromStr;
use std::time::Duration;

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use gridcraft_engine::datasource::{DbRequest, DbTable, DbValue};
use postgres::fallible_iterator::FallibleIterator;
use postgres::types::{FromSql, Type};
use postgres::{Config, NoTls, Row};

pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    let mut cfg = Config::from_str(&req.connection).map_err(|e| format!("Invalid PostgreSQL connection: {e}"))?;
    if let Some(u) = &req.user {
        cfg.user(u);
    }
    if let Some(p) = &req.password {
        cfg.password(p);
    }
    cfg.connect_timeout(Duration::from_secs(req.timeout_secs));
    cfg.application_name("GridCraft");
    let mut client = cfg.connect(NoTls).map_err(|e| format!("Could not connect to PostgreSQL: {e}"))?;
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

fn get<'a, T: FromSql<'a>>(row: &'a Row, i: usize) -> Option<T> {
    row.try_get::<_, Option<T>>(i).ok().flatten()
}

fn value(row: &Row, i: usize) -> DbValue {
    let Some(col) = row.columns().get(i) else { return DbValue::Null };
    let ty = col.type_();
    let v = match *ty {
        Type::BOOL => get::<bool>(row, i).map(DbValue::Bool),
        Type::INT2 => get::<i16>(row, i).map(|n| DbValue::Int(n.into())),
        Type::INT4 => get::<i32>(row, i).map(|n| DbValue::Int(n.into())),
        Type::INT8 => get::<i64>(row, i).map(DbValue::Int),
        Type::OID => get::<u32>(row, i).map(|n| DbValue::Int(n.into())),
        Type::FLOAT4 => get::<f32>(row, i).map(|n| DbValue::Float(n.into())),
        Type::FLOAT8 => get::<f64>(row, i).map(DbValue::Float),
        Type::NUMERIC => get::<rust_decimal::Decimal>(row, i).map(|d| {
            use rust_decimal::prelude::ToPrimitive;
            d.to_f64().map_or_else(|| DbValue::Text(d.to_string()), DbValue::Float)
        }),
        Type::DATE => get::<NaiveDate>(row, i).map(date),
        Type::TIMESTAMP => get::<NaiveDateTime>(row, i).map(datetime),
        Type::TIMESTAMPTZ => get::<chrono::DateTime<chrono::Utc>>(row, i).map(|d| datetime(d.naive_utc())),
        Type::TIME => get::<NaiveTime>(row, i).map(time),
        Type::UUID => get::<uuid::Uuid>(row, i).map(|u| DbValue::Text(u.to_string())),
        Type::JSON | Type::JSONB => get::<serde_json::Value>(row, i).map(|j| DbValue::Text(j.to_string())),
        Type::BYTEA => get::<Vec<u8>>(row, i).map(|b| DbValue::Text(format!("({} bytes)", b.len()))),
        _ => match row.try_get::<_, Option<String>>(i) {
            Ok(s) => s.map(DbValue::Text),
            // A type with no text decoding (arrays, ranges, geometry…): name it instead of failing.
            Err(_) => Some(DbValue::Text(format!("({})", ty.name()))),
        },
    };
    v.unwrap_or(DbValue::Null)
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
