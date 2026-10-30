//! MySQL and MariaDB through the pure-Rust `mysql` client.

use std::time::Duration;

use gridcraft_engine::datasource::{DbRequest, DbTable, DbValue};
use mysql::consts::ColumnType;
use mysql::prelude::Queryable;
use mysql::{Conn, DriverError, Opts, OptsBuilder, SslOpts, Value};

/// The MySQL client's `ssl-mode`, given as a URL parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SslMode {
    Disabled,
    /// Encrypt when the server can, without checking its certificate (the default).
    Preferred,
    /// Encrypt or fail, without checking the certificate.
    Required,
    /// Encrypt with a certificate from a trusted root.
    VerifyCa,
    /// As `VerifyCa`, and issued for the host.
    VerifyIdentity,
}

/// The URL without its `ssl-mode` parameter (the `mysql` client doesn't take it), and the mode.
fn split_ssl_mode(url: &str) -> Result<(String, Option<SslMode>), String> {
    let Some((base, query)) = url.split_once('?') else { return Ok((url.to_string(), None)) };
    let mut mode = None;
    let mut kept = vec![];
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if matches!(k.to_ascii_lowercase().as_str(), "ssl-mode" | "ssl_mode" | "sslmode") {
            mode = Some(match v.to_ascii_lowercase().replace('-', "_").as_str() {
                "disabled" | "disable" => SslMode::Disabled,
                "preferred" | "prefer" => SslMode::Preferred,
                "required" | "require" => SslMode::Required,
                "verify_ca" => SslMode::VerifyCa,
                "verify_identity" | "verify_full" => SslMode::VerifyIdentity,
                _ => return Err(format!("Unknown ssl-mode `{v}` (DISABLED, PREFERRED, REQUIRED, VERIFY_CA, VERIFY_IDENTITY).")),
            });
        } else {
            kept.push(pair);
        }
    }
    Ok((if kept.is_empty() { base.to_string() } else { format!("{base}?{}", kept.join("&")) }, mode))
}

pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    let (url, mode) = split_ssl_mode(&req.connection)?;
    let opts = Opts::from_url(&url).map_err(|e| format!("Invalid MySQL connection URL: {e}"))?;
    // TLS options written into the URL (`root_cert_path`, `danger_…`) require TLS, as before.
    let mode = mode.unwrap_or(if opts.get_ssl_opts().is_some() { SslMode::Required } else { SslMode::Preferred });
    let given = opts.get_ssl_opts().cloned().unwrap_or_default();
    let timeout = Some(Duration::from_secs(req.timeout_secs));
    let mut b = OptsBuilder::from_opts(opts).tcp_connect_timeout(timeout).read_timeout(timeout).write_timeout(timeout);
    if let Some(u) = &req.user {
        b = b.user(Some(u.as_str()));
    }
    if let Some(p) = &req.password {
        b = b.pass(Some(p.as_str()));
    }
    let tls = match mode {
        SslMode::Disabled => None,
        SslMode::Preferred | SslMode::Required => Some(given.with_danger_accept_invalid_certs(true)),
        SslMode::VerifyCa => Some(given.with_danger_skip_domain_validation(true)),
        SslMode::VerifyIdentity => Some(given),
    };
    let mut conn = match Conn::new(b.clone().ssl_opts(tls)) {
        // PREFERRED falls back to an unencrypted session only when the server has no TLS at all.
        Err(mysql::Error::DriverError(DriverError::TlsNotSupported)) if mode == SslMode::Preferred => Conn::new(b.ssl_opts(None::<SslOpts>)),
        r => r,
    }
    .map_err(|e| format!("Could not connect to MySQL: {e}"))?;
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
            // A DATETIME or TIMESTAMP at midnight stays a date-time: the column's format comes from
            // its first value, so turning it into a date would hide the time of every later row.
            let has_time = matches!(
                ty,
                Some(
                    ColumnType::MYSQL_TYPE_DATETIME
                        | ColumnType::MYSQL_TYPE_DATETIME2
                        | ColumnType::MYSQL_TYPE_TIMESTAMP
                        | ColumnType::MYSQL_TYPE_TIMESTAMP2
                )
            );
            if !has_time && (ty == Some(ColumnType::MYSQL_TYPE_DATE) || (h, mi, s, us) == (0, 0, 0, 0)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssl_mode_is_read_and_removed() {
        assert_eq!(split_ssl_mode("mysql://u@h/db").unwrap(), ("mysql://u@h/db".into(), None));
        assert_eq!(split_ssl_mode("mysql://u@h/db?ssl-mode=VERIFY_IDENTITY").unwrap(), ("mysql://u@h/db".into(), Some(SslMode::VerifyIdentity)));
        assert_eq!(
            split_ssl_mode("mysql://u@h/db?prefer_socket=false&ssl_mode=required").unwrap(),
            ("mysql://u@h/db?prefer_socket=false".into(), Some(SslMode::Required))
        );
        assert!(split_ssl_mode("mysql://u@h/db?ssl-mode=maybe").is_err());
    }

    #[test]
    fn midnight_keeps_the_time_of_date_time_columns() {
        let midnight = || Value::Date(2024, 1, 5, 0, 0, 0, 0);
        assert_eq!(value(midnight(), Some(ColumnType::MYSQL_TYPE_DATE)), DbValue::Date(2024, 1, 5));
        for ty in [ColumnType::MYSQL_TYPE_DATETIME, ColumnType::MYSQL_TYPE_TIMESTAMP, ColumnType::MYSQL_TYPE_TIMESTAMP2] {
            assert_eq!(value(midnight(), Some(ty)), DbValue::DateTime(2024, 1, 5, 0, 0, 0.0), "{ty:?}");
        }
        assert_eq!(value(Value::Date(2024, 1, 5, 10, 30, 0, 0), Some(ColumnType::MYSQL_TYPE_TIMESTAMP)), DbValue::DateTime(2024, 1, 5, 10, 30, 0.0));
    }
}
