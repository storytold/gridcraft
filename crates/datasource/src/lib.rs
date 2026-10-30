//! Database drivers for Data › Get Data › From Database.
//!
//! [`provider`] returns the [`DataSource`] a native host installs in
//! `Session::data_source`. Each driver runs one query, honours `max_rows` and `timeout_secs`,
//! and reduces values to [`DbValue`]s. Native only: the engine and the web build never link it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::sync::Arc;

pub use gridcraft_engine::datasource::redact;
use gridcraft_engine::datasource::{DataSource, DbOp, DbRequest, DbTable, DbValue, catalog_sql};

#[cfg(not(target_arch = "wasm32"))]
mod mysql_driver;
#[cfg(all(feature = "odbc", not(target_arch = "wasm32")))]
mod odbc_driver;
#[cfg(not(target_arch = "wasm32"))]
mod postgres_driver;
#[cfg(not(target_arch = "wasm32"))]
mod sqlite_driver;
#[cfg(not(target_arch = "wasm32"))]
mod tls;

/// The data source with every driver this build has.
pub fn provider() -> DataSource {
    Arc::new(run)
}

/// Runs `req` with its driver.
pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    log::info!("data.fromDatabase: {} {:?} on {}", req.driver, req.op, redact(&req.connection));
    // Tables and columns come from the catalog views, so those drivers only ever run SQL.
    if let Some(sql) = catalog_sql(&req.driver, &req.op) {
        return run(&DbRequest { query: sql, op: DbOp::Query, ..req.clone() });
    }
    match req.driver.as_str() {
        #[cfg(not(target_arch = "wasm32"))]
        "postgres" => postgres_driver::run(req),
        #[cfg(not(target_arch = "wasm32"))]
        "mysql" => mysql_driver::run(req),
        #[cfg(not(target_arch = "wasm32"))]
        "sqlite" => sqlite_driver::run(req),
        #[cfg(all(feature = "odbc", not(target_arch = "wasm32")))]
        "odbc" => odbc_driver::run(req),
        #[cfg(not(all(feature = "odbc", not(target_arch = "wasm32"))))]
        "odbc" => Err("ODBC support was not compiled into this build.".into()),
        other => Err(format!("Unknown database driver `{other}`.")),
    }
}

/// Text in ISO forms (`2024-01-05`, `2024-01-05 10:30:00.5`, `2024-01-05T10:30:00`, `10:30:00`) as a
/// date, date-time or time; anything else is `None`.
pub fn parse_temporal(s: &str) -> Option<DbValue> {
    let s = s.trim();
    let date = |d: &str| -> Option<(i32, u32, u32)> {
        let mut it = d.splitn(3, '-');
        let y: i32 = it.next()?.parse().ok()?;
        let m: u32 = it.next()?.parse().ok()?;
        let dd: u32 = it.next()?.parse().ok()?;
        (d.len() == 10 && (1..=12).contains(&m) && (1..=31).contains(&dd)).then_some((y, m, dd))
    };
    let time = |t: &str| -> Option<(u32, u32, f64)> {
        // Drop a trailing zone (`Z`, `+02`, `+02:00`).
        let t = t.trim_end_matches('Z');
        let t = match t.rfind(['+', '-']) {
            Some(i) if i >= 8 => &t[..i],
            _ => t,
        };
        let mut it = t.splitn(3, ':');
        let h: u32 = it.next()?.parse().ok()?;
        let m: u32 = it.next()?.parse().ok()?;
        let sec: f64 = it.next().map_or(Some(0.0), |x| x.parse().ok())?;
        (h < 24 && m < 60 && (0.0..61.0).contains(&sec)).then_some((h, m, sec))
    };
    if let Some((y, m, d)) = date(s) {
        return Some(DbValue::Date(y, m, d));
    }
    if let Some((d, t)) = s.get(..10).zip(s.get(11..))
        && matches!(s.as_bytes().get(10), Some(b' ' | b'T'))
    {
        let (y, mo, dd) = date(d)?;
        let (h, mi, sec) = time(t)?;
        return Some(DbValue::DateTime(y, mo, dd, h, mi, sec));
    }
    let (h, m, sec) = time(s)?;
    Some(DbValue::Time(h, m, sec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_passwords() {
        assert_eq!(redact("postgres://bob:hunter2@db:5432/x"), "postgres://bob:***@db:5432/x");
        assert_eq!(redact("mysql://bob@db/x"), "mysql://bob@db/x");
        assert_eq!(redact("DSN=Sales;UID=bob;PWD=hunter2"), "DSN=Sales;UID=bob;PWD=***");
        assert_eq!(redact("host=db password=x user=bob"), "host=db password=*** user=bob");
        assert_eq!(redact("Driver={X};Pwd=a;b"), "Driver={X};Pwd=***;b");
        assert_eq!(redact("postgres://u@h/db?password=x"), "postgres://u@h/db?password=***");
        assert_eq!(redact("postgres://u@h/db?sslmode=disable&password=x"), "postgres://u@h/db?sslmode=disable&password=***");
    }

    #[test]
    fn temporal_text() {
        assert_eq!(parse_temporal("2024-01-05"), Some(DbValue::Date(2024, 1, 5)));
        assert_eq!(parse_temporal("2024-01-05 10:30:15.5"), Some(DbValue::DateTime(2024, 1, 5, 10, 30, 15.5)));
        assert_eq!(parse_temporal("2024-01-05T10:30:00Z"), Some(DbValue::DateTime(2024, 1, 5, 10, 30, 0.0)));
        assert_eq!(parse_temporal("2024-01-05 10:30:00+02:00"), Some(DbValue::DateTime(2024, 1, 5, 10, 30, 0.0)));
        assert_eq!(parse_temporal("10:30"), Some(DbValue::Time(10, 30, 0.0)));
        assert_eq!(parse_temporal("hello"), None);
        assert_eq!(parse_temporal("2024-13-01"), None);
        assert_eq!(parse_temporal("12345"), None);
    }

    #[test]
    fn odbc_without_feature_and_unknown_driver_are_errors() {
        let req = DbRequest {
            driver: "nope".into(),
            connection: String::new(),
            password: None,
            user: None,
            query: "q".into(),
            max_rows: 10,
            timeout_secs: 1,
            op: DbOp::Query,
        };
        assert!(run(&req).is_err());
        #[cfg(not(feature = "odbc"))]
        assert!(run(&DbRequest { driver: "odbc".into(), ..req }).unwrap_err().contains("not compiled"));
    }
}
