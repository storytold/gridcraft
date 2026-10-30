//! External databases (Data › Get Data › From Database).
//!
//! The engine stays network-free and wasm-clean: the host (desktop app, CLI) injects a
//! [`DataSource`] into [`crate::Session::data_source`] that runs a query and returns a
//! [`DbTable`]. Without one, `data.fromDatabase` returns an error.

use std::fmt::Write as _;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Rows imported by default (the sheet's row count minus the header row).
pub const DEFAULT_MAX_ROWS: usize = 1_048_575;
/// Columns imported at most (the sheet's column count).
pub const MAX_COLS: usize = 16_384;
/// Connect and query timeout, in seconds, when the caller gives none.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// One query against one database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbRequest {
    /// `postgres`, `mysql`, `sqlite` or `odbc`.
    pub driver: String,
    /// URL, file path or ODBC connection string, without the password.
    pub connection: String,
    /// Never stored in the workbook or the journal.
    pub password: Option<String>,
    pub user: Option<String>,
    pub query: String,
    pub max_rows: usize,
    pub timeout_secs: u64,
    /// What to run: `query`, or a look at the database's tables or a table's columns.
    pub op: DbOp,
}

/// What a [`DbRequest`] asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DbOp {
    /// Run `query`.
    #[default]
    Query,
    /// The tables and views: columns `schema`, `name`, `type`.
    Tables,
    /// A table's columns, in order: columns `name`, `type`.
    Columns { schema: Option<String>, table: String },
}

/// A database value, already reduced to what a cell can hold.
#[derive(Clone, Debug, PartialEq)]
pub enum DbValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// Calendar date (year, month, day).
    Date(i32, u32, u32),
    /// Date and time (year, month, day, hour, minute, second with fraction).
    DateTime(i32, u32, u32, u32, u32, f64),
    /// Time of day (hour, minute, second with fraction).
    Time(u32, u32, f64),
}

/// A query result: column names and rows (each row as long as `columns` or shorter).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DbTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<DbValue>>,
    /// More rows were available than `max_rows`.
    pub truncated: bool,
}

/// Runs a query. Implementations must honour `max_rows` and `timeout_secs` and never panic.
pub type DataSource = Arc<dyn Fn(&DbRequest) -> Result<DbTable, String> + Send + Sync>;

/// Drivers a host may support; the command rejects anything else before calling the host.
pub const DRIVERS: &[&str] = &["postgres", "mysql", "sqlite", "odbc"];

/// The driver a URL connection names by its scheme (`postgres://`, `mysql://`, …), if any.
pub fn driver_for_connection(connection: &str) -> Option<&'static str> {
    let (scheme, _) = connection.trim().split_once("://")?;
    match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => Some("postgres"),
        "mysql" | "mariadb" => Some("mysql"),
        "sqlite" => Some("sqlite"),
        _ => None,
    }
}

/// The SQL that answers a catalog [`DbOp`] on PostgreSQL, MySQL/MariaDB or SQLite. `None` for
/// [`DbOp::Query`] and for ODBC, whose drivers answer through the ODBC catalog functions.
pub fn catalog_sql(driver: &str, op: &DbOp) -> Option<String> {
    let lit = |v: &str| sql_literal(driver, v);
    match (driver, op) {
        (_, DbOp::Query) | ("odbc", _) => None,
        ("postgres", DbOp::Tables) => Some(
            "SELECT table_schema AS schema, table_name AS name, table_type AS type FROM information_schema.tables \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') ORDER BY 1, 2"
                .into(),
        ),
        ("mysql", DbOp::Tables) => Some(
            "SELECT table_schema AS `schema`, table_name AS name, table_type AS type FROM information_schema.tables \
             WHERE table_schema = DATABASE() OR (DATABASE() IS NULL AND table_schema NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys')) \
             ORDER BY 1, 2"
                .into(),
        ),
        ("sqlite", DbOp::Tables) => Some(
            "SELECT '' AS schema, name, upper(type) AS type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name"
                .into(),
        ),
        ("postgres" | "mysql", DbOp::Columns { schema, table }) => {
            let schema = match schema.as_deref().filter(|x| !x.is_empty()) {
                Some(x) => lit(x),
                None if driver == "mysql" => "DATABASE()".into(),
                None => "current_schema()".into(),
            };
            let q = if driver == "mysql" { "`" } else { "" };
            Some(format!(
                "SELECT column_name AS name, data_type AS {q}type{q} FROM information_schema.columns WHERE table_schema = {schema} AND table_name = {} ORDER BY ordinal_position",
                lit(table)
            ))
        }
        ("sqlite", DbOp::Columns { table, .. }) => Some(format!("SELECT name, type FROM pragma_table_info({})", lit(table))),
        _ => None,
    }
}

/// The connection without anything that looks like a password, for logs and messages.
pub fn redact(connection: &str) -> String {
    // URL form: scheme://user:password@host
    if let Some((scheme, rest)) = connection.split_once("://")
        && let Some((auth, host)) = rest.split_once('@')
        && let Some((user, _)) = auth.split_once(':')
    {
        return format!("{scheme}://{user}:***@{host}");
    }
    // Key-value form (ODBC `PWD=…;`, libpq `password=… `): blank out each secret value.
    let lower = connection.to_ascii_lowercase();
    let mut out = String::with_capacity(connection.len());
    let mut i = 0;
    while i < connection.len() {
        let hit = ["pwd=", "password="].iter().find(|k| lower[i..].starts_with(**k) && (i == 0 || lower[..i].ends_with([';', ' ', '\t'])));
        match hit {
            Some(k) => {
                out.push_str(&connection[i..i + k.len()]);
                out.push_str("***");
                i += k.len();
                i += connection[i..].find([';', ' ', '\t']).unwrap_or(connection.len() - i);
            }
            None => {
                let ch = connection[i..].chars().next().unwrap_or(' ');
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// `v` as an SQL string literal (MySQL also treats `\` as an escape character).
fn sql_literal(driver: &str, v: &str) -> String {
    let mut v = v.replace('\'', "''");
    if driver == "mysql" {
        v = v.replace('\\', "\\\\");
    }
    format!("'{v}'")
}

/// `name` as a quoted identifier: backticks for MySQL, double quotes elsewhere.
pub fn quote_ident(driver: &str, name: &str) -> String {
    let q = if driver == "mysql" { '`' } else { '"' };
    format!("{q}{}{q}", name.replace(q, &format!("{q}{q}")))
}

/// `SELECT` of `columns` (all of them when empty) from `schema.table`.
pub fn select_sql(driver: &str, schema: Option<&str>, table: &str, columns: &[String]) -> String {
    let cols = if columns.is_empty() { "*".to_string() } else { columns.iter().map(|c| quote_ident(driver, c)).collect::<Vec<_>>().join(", ") };
    let mut from = String::new();
    if let Some(sc) = schema.filter(|x| !x.is_empty()) {
        let _ = write!(from, "{}.", quote_ident(driver, sc));
    }
    from.push_str(&quote_ident(driver, table));
    format!("SELECT {cols} FROM {from}")
}

/// A connection saved in the preferences (Data › From Database › Saved connection), or the
/// connection fields of the From Database dialog. Never holds a password.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConnectionProfile {
    pub name: String,
    /// One of [`DRIVERS`].
    pub driver: String,
    pub host: String,
    /// Blank = the driver's default port.
    pub port: String,
    pub database: String,
    /// SQLite: the database file.
    pub file: String,
    /// ODBC: a data source name, or a whole connection string without the password.
    pub dsn: String,
    pub user: String,
    /// A hand-written connection that replaces the composed one.
    pub custom: String,
}

impl ConnectionProfile {
    /// The connection the fields describe: `postgres://host:5432/db`, `mysql://host:3306/db`,
    /// the SQLite file, or `DSN=…`. The user name travels separately, the password never.
    pub fn connection(&self) -> String {
        if !self.custom.trim().is_empty() {
            return self.custom.trim().to_string();
        }
        let url = |scheme: &str, default_port: &str| {
            let host = self.host.trim();
            let host = if host.is_empty() { "localhost" } else { host };
            let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
            let port = self.port.trim();
            let port = if port.is_empty() { default_port } else { port };
            let db = self.database.trim();
            if db.is_empty() { format!("{scheme}://{host}:{port}") } else { format!("{scheme}://{host}:{port}/{}", url_escape(db)) }
        };
        match self.driver.as_str() {
            "postgres" => url("postgres", "5432"),
            "mysql" => url("mysql", "3306"),
            "sqlite" => self.file.trim().to_string(),
            "odbc" => {
                let d = self.dsn.trim();
                if d.is_empty() || d.contains('=') { d.to_string() } else { format!("DSN={d}") }
            }
            _ => String::new(),
        }
    }
}

/// Percent-encodes everything but unreserved URL characters.
fn url_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_connections() {
        let p = |driver: &str| ConnectionProfile { driver: driver.into(), host: "10.0.0.5".into(), database: "r lab".into(), ..Default::default() };
        assert_eq!(p("mysql").connection(), "mysql://10.0.0.5:3306/r%20lab");
        assert_eq!(ConnectionProfile { port: "6543".into(), database: "".into(), ..p("postgres") }.connection(), "postgres://10.0.0.5:6543");
        assert_eq!(ConnectionProfile { driver: "postgres".into(), host: "::1".into(), ..Default::default() }.connection(), "postgres://[::1]:5432");
        assert_eq!(ConnectionProfile { driver: "sqlite".into(), file: " /d/x.db ".into(), ..Default::default() }.connection(), "/d/x.db");
        assert_eq!(ConnectionProfile { driver: "odbc".into(), dsn: "Sales".into(), ..Default::default() }.connection(), "DSN=Sales");
        assert_eq!(
            ConnectionProfile { driver: "odbc".into(), dsn: "Driver={X};Server=s".into(), ..Default::default() }.connection(),
            "Driver={X};Server=s"
        );
        assert_eq!(ConnectionProfile { custom: "mysql://h/db".into(), ..p("mysql") }.connection(), "mysql://h/db");
    }

    #[test]
    fn select_and_catalog_sql() {
        assert_eq!(select_sql("mysql", Some("db"), "my`t", &["a".into(), "b c".into()]), "SELECT `a`, `b c` FROM `db`.`my``t`");
        assert_eq!(select_sql("postgres", None, "t\"x", &[]), "SELECT * FROM \"t\"\"x\"");
        assert!(catalog_sql("odbc", &DbOp::Tables).is_none());
        assert!(catalog_sql("mysql", &DbOp::Query).is_none());
        let c = catalog_sql("mysql", &DbOp::Columns { schema: None, table: "o'k\\".into() }).unwrap();
        assert!(c.contains("table_name = 'o''k\\\\'") && c.contains("DATABASE()"), "{c}");
        let c = catalog_sql("postgres", &DbOp::Columns { schema: Some("s".into()), table: "t".into() }).unwrap();
        assert!(c.contains("table_schema = 's'"), "{c}");
        assert!(catalog_sql("sqlite", &DbOp::Columns { schema: None, table: "t".into() }).unwrap().contains("pragma_table_info('t')"));
    }

    #[test]
    fn driver_from_url_scheme() {
        assert_eq!(driver_for_connection("mysql://u@h:3306/db"), Some("mysql"));
        assert_eq!(driver_for_connection(" MariaDB://u@h/db"), Some("mysql"));
        assert_eq!(driver_for_connection("postgresql://u@h/db"), Some("postgres"));
        assert_eq!(driver_for_connection("/data/x.db"), None);
        assert_eq!(driver_for_connection("DSN=Sales"), None);
    }
}
