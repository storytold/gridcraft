//! MySQL and MariaDB through the pure-Rust `mysql` client.

use std::time::Duration;

use gridcraft_engine::datasource::{DbRequest, DbTable, DbValue};
use mysql::consts::ColumnType;
use mysql::prelude::Queryable;
use mysql::{Conn, Opts, OptsBuilder, Value};

pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    let opts = Opts::from_url(&req.connection).map_err(|e| format!("Invalid MySQL connection URL: {e}"))?;
    let timeout = Some(Duration::from_secs(req.timeout_secs));
    let mut b = OptsBuilder::from_opts(opts).tcp_connect_timeout(timeout).read_timeout(timeout).write_timeout(timeout);
    if let Some(u) = &req.user {
        b = b.user(Some(u.as_str()));
    }
    if let Some(p) = &req.password {
        b = b.pass(Some(p.as_str()));
    }
    let mut conn = Conn::new(b).map_err(|e| format!("Could not connect to MySQL: {e}"))?;
    // The binary protocol returns typed values; statements it can't prepare (SHOW, some CALLs)
    // fall back to the text protocol.
    match conn.exec_iter(req.query.as_str(), ()) {
        Ok(result) => return collect(result, req.max_rows),
        Err(mysql::Error::MySqlError(e)) if e.code == 1295 => {}
        Err(e) => return Err(format!("MySQL: {e}")),
    }
    let result = conn.query_iter(req.query.as_str()).map_err(|e| format!("MySQL: {e}"))?;
    collect(result, req.max_rows)
}

fn collect<P: mysql::prelude::Protocol>(mut result: mysql::QueryResult<'_, '_, '_, P>, max_rows: usize) -> Result<DbTable, String> {
    let Some(set) = result.iter() else { return Ok(DbTable::default()) };
    let cols = set.columns();
    let types: Vec<ColumnType> = cols.as_ref().iter().map(|c| c.column_type()).collect();
    let mut table = DbTable { columns: cols.as_ref().iter().map(|c| c.name_str().into_owned()).collect(), rows: Vec::new(), truncated: false };
    for row in set {
        if table.rows.len() >= max_rows {
            table.truncated = true;
            break;
        }
        let row = row.map_err(|e| format!("MySQL: {e}"))?;
        let values = row.unwrap();
        table.rows.push(values.into_iter().enumerate().map(|(i, v)| value(v, types.get(i).copied())).collect());
    }
    Ok(table)
}

fn value(v: Value, ty: Option<ColumnType>) -> DbValue {
    match v {
        Value::NULL => DbValue::Null,
        Value::Int(n) => DbValue::Int(n),
        Value::UInt(n) => i64::try_from(n).map_or_else(|_| DbValue::Text(n.to_string()), DbValue::Int),
        Value::Float(f) => DbValue::Float(f.into()),
        Value::Double(f) => DbValue::Float(f),
        Value::Date(y, m, d, h, mi, s, us) => {
            if ty == Some(ColumnType::MYSQL_TYPE_DATE) || (h, mi, s, us) == (0, 0, 0, 0) && ty != Some(ColumnType::MYSQL_TYPE_DATETIME) {
                DbValue::Date(y.into(), m.into(), d.into())
            } else {
                DbValue::DateTime(y.into(), m.into(), d.into(), h.into(), mi.into(), f64::from(s) + f64::from(us) / 1e6)
            }
        }
        Value::Time(neg, days, h, m, s, us) => {
            if neg || days > 0 {
                DbValue::Text(format!("{}{}:{m:02}:{s:02}", if neg { "-" } else { "" }, u64::from(days) * 24 + u64::from(h)))
            } else {
                DbValue::Time(h.into(), m.into(), f64::from(s) + f64::from(us) / 1e6)
            }
        }
        Value::Bytes(b) => bytes(b, ty),
    }
}

/// Text-protocol values and binary-protocol strings/decimals arrive as bytes.
fn bytes(b: Vec<u8>, ty: Option<ColumnType>) -> DbValue {
    use ColumnType::*;
    let Ok(s) = String::from_utf8(b) else { return DbValue::Text("(binary)".into()) };
    let num = || s.parse::<f64>().ok().filter(|f| f.is_finite());
    match ty {
        Some(MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_LONG | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONGLONG | MYSQL_TYPE_YEAR) => {
            s.parse::<i64>().map_or(DbValue::Text(s), DbValue::Int)
        }
        Some(MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL | MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE) => num().map_or(DbValue::Text(s), DbValue::Float),
        Some(MYSQL_TYPE_DATE | MYSQL_TYPE_DATETIME | MYSQL_TYPE_TIMESTAMP | MYSQL_TYPE_TIME) => crate::parse_temporal(&s).unwrap_or(DbValue::Text(s)),
        Some(MYSQL_TYPE_BIT) => DbValue::Text(s),
        _ => DbValue::Text(s),
    }
}
