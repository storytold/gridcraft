//! Bounded ODBC reads for spreadsheet imports.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::{fmt, mem::size_of, sync::atomic::AtomicBool};

use gridcraft_core::Value;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "windows")]
mod native;
#[cfg(test)]
mod tests;

pub const MAX_ROWS: usize = 100_000;
pub const MAX_COLUMNS: usize = 256;
pub const MAX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_CELL_BYTES: usize = 1024 * 1024;

#[derive(Clone, Default)]
pub struct Connection {
    pub dsn: Option<String>,
    pub connection_string: Option<String>,
    pub user: String,
    pub password: String,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("dsn", &self.dsn)
            .field("connection_string", &"[redacted]")
            .field("user", &self.user)
            .field("password", &"[redacted]")
            .finish()
    }
}

impl Connection {
    pub fn validate(&self) -> Result<(), OdbcError> {
        match (&self.dsn, &self.connection_string) {
            (Some(dsn), None)
                if !dsn.trim().is_empty()
                    && dsn.trim() == dsn
                    && dsn.encode_utf16().count() <= 32
                    && !dsn.chars().any(|c| c.is_control() || "[]{}(),;?*=!@\\".contains(c)) => {}
            (None, Some(cs))
                if !cs.trim().is_empty()
                    && cs.len() <= 16_384
                    && !cs.contains('\0')
                    && !cs.split(';').any(|part| part.split_once('=').is_some_and(|(key, _)| key.trim().eq_ignore_ascii_case("SAVEFILE"))) => {}
            _ => return Err(OdbcError::InvalidConnection),
        }
        if self.user.len() > 512 || self.password.len() > 16_384 || self.user.contains('\0') || self.password.contains('\0') {
            return Err(OdbcError::InvalidConnection);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub rows: usize,
    pub columns: usize,
    pub bytes: usize,
    pub cell_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { rows: MAX_ROWS, columns: MAX_COLUMNS, bytes: MAX_BYTES, cell_bytes: MAX_CELL_BYTES }
    }
}

impl Limits {
    pub fn validate(self) -> Result<Self, OdbcError> {
        if self.rows == 0
            || self.rows > MAX_ROWS
            || self.columns == 0
            || self.columns > MAX_COLUMNS
            || self.bytes == 0
            || self.bytes > MAX_BYTES
            || self.cell_bytes == 0
            || self.cell_bytes > MAX_CELL_BYTES
        {
            return Err(OdbcError::Limit("Requested limits are outside supported bounds."));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColumnKind {
    Text,
    Integer,
    Decimal,
    Float,
    Boolean,
    Temporal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub name: String,
    pub kind: ColumnKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSet {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
}

impl DataSet {
    /// Validates the result and returns its bounded storage estimate.
    pub fn validate(&self, limits: Limits) -> Result<usize, OdbcError> {
        let limits = limits.validate()?;
        if self.columns.is_empty()
            || self.columns.len() > limits.columns
            || self.rows.len() > limits.rows
            || self.columns.iter().any(|c| c.name.len() > 1024 || c.name.contains('\0'))
        {
            return Err(OdbcError::Limit("The result exceeds the supported shape."));
        }
        let mut bytes = self
            .columns
            .iter()
            .try_fold(0usize, |n, c| n.checked_add(c.name.len() + size_of::<Column>()))
            .ok_or(OdbcError::Limit("The result exceeds the supported size."))?;
        for row in &self.rows {
            if row.len() != self.columns.len() {
                return Err(OdbcError::InvalidData);
            }
            bytes = bytes.checked_add(size_of::<Vec<Value>>()).ok_or(OdbcError::Limit("The result exceeds the supported size."))?;
            for value in row {
                let size = match value {
                    Value::Empty | Value::Bool(_) => 0,
                    Value::Number(n) if n.is_finite() => 0,
                    Value::Text(s) if s.len() <= limits.cell_bytes => s.len() + 2 * size_of::<usize>(),
                    Value::Text(_) => return Err(OdbcError::Limit("A result cell exceeds the supported size.")),
                    _ => return Err(OdbcError::InvalidData),
                };
                bytes = bytes.checked_add(size + size_of::<Value>()).ok_or(OdbcError::Limit("The result exceeds the supported size."))?;
                if bytes > limits.bytes {
                    return Err(OdbcError::Limit("The result exceeds the supported size."));
                }
            }
        }
        if bytes > limits.bytes {
            return Err(OdbcError::Limit("The result exceeds the supported size."));
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    pub data_sources: Vec<String>,
    pub drivers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OdbcError {
    #[error("ODBC is available only in the Windows desktop build.")]
    Unavailable,
    #[error("The connection settings are invalid.")]
    InvalidConnection,
    #[error("The ODBC connection could not be completed.")]
    ConnectionFailed,
    #[error("The ODBC query failed. Check the SQL and database permissions.")]
    QueryFailed,
    #[error("ODBC operation failed (SQLSTATE {0}). Check the driver, connection and query.")]
    DriverState(String),
    #[error("Use a supported PostgreSQL, SQL Server, MySQL, MariaDB, Oracle or SQLite3 ODBC driver.")]
    UnsupportedDatabase,
    #[error("Use one SELECT, WITH, VALUES or TABLE query without internal semicolons, write/control keywords or executable comments.")]
    InvalidQuery,
    #[error("{0}")]
    Limit(&'static str),
    #[error("The driver returned unsupported or malformed data.")]
    InvalidData,
    #[error("The operation was cancelled.")]
    Cancelled,
}

pub trait Backend: Send + Sync + 'static {
    fn sources(&self) -> Result<Sources, OdbcError>;
    fn test(&self, connection: &Connection) -> Result<(), OdbcError>;
    fn query(&self, connection: &Connection, sql: &str, limits: Limits, preview: bool, cancelled: &AtomicBool) -> Result<DataSet, OdbcError>;
}

#[derive(Default)]
pub struct NativeBackend;

impl Backend for NativeBackend {
    fn sources(&self) -> Result<Sources, OdbcError> {
        native::sources()
    }
    fn test(&self, connection: &Connection) -> Result<(), OdbcError> {
        native::test(connection)
    }
    fn query(&self, connection: &Connection, sql: &str, limits: Limits, preview: bool, cancelled: &AtomicBool) -> Result<DataSet, OdbcError> {
        native::query(connection, sql, limits, preview, cancelled)
    }
}

pub const fn available() -> bool {
    cfg!(target_os = "windows")
}

pub fn validate_query(sql: &str) -> Result<(), OdbcError> {
    let body = sql.trim().strip_suffix(';').unwrap_or(sql.trim()).trim();
    // shortcut: semicolons inside literals are refused; use a SQL parser if multi-statement-aware editing is added.
    if body.is_empty()
        || body.len() > 1_000_000
        || body.contains(';')
        || body.chars().any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        || !body
            .split(|c: char| !c.is_ascii_alphabetic() && c != '_')
            .next()
            .is_some_and(|word| ["select", "with", "values", "table"].iter().any(|keyword| word.eq_ignore_ascii_case(keyword)))
    {
        return Err(OdbcError::InvalidQuery);
    }
    Ok(())
}

pub fn decode(kind: ColumnKind, value: &str) -> Result<Value, OdbcError> {
    let exact = || Value::text(value);
    Ok(match kind {
        ColumnKind::Text | ColumnKind::Temporal => exact(),
        ColumnKind::Integer => {
            let number = value.parse::<i128>().map_err(|_| OdbcError::InvalidData)?;
            if number.unsigned_abs() < 1_000_000_000_000_000 { Value::Number(number as f64) } else { exact() }
        }
        ColumnKind::Decimal => {
            let significant = value.trim_start_matches(['+', '-']).split(['e', 'E']).next().unwrap_or(value);
            let digits = significant.chars().filter(|c| c.is_ascii_digit()).skip_while(|c| *c == '0').collect::<String>().trim_end_matches('0').len();
            match value.parse::<f64>() {
                Ok(n) if n.is_finite() && digits <= 15 && (n != 0.0 || digits == 0) => Value::Number(n),
                Ok(_) => exact(),
                Err(_) => return Err(OdbcError::InvalidData),
            }
        }
        ColumnKind::Float => {
            let number = value.parse::<f64>().map_err(|_| OdbcError::InvalidData)?;
            if number.is_finite() { Value::Number(number) } else { exact() }
        }
        ColumnKind::Boolean => match value.to_ascii_lowercase().as_str() {
            "1" | "-1" | "true" | "t" | "yes" => Value::Bool(true),
            "0" | "false" | "f" | "no" => Value::Bool(false),
            _ => return Err(OdbcError::InvalidData),
        },
    })
}

#[cfg(not(target_os = "windows"))]
mod native {
    use super::*;
    pub fn sources() -> Result<Sources, OdbcError> {
        Err(OdbcError::Unavailable)
    }
    pub fn test(_: &Connection) -> Result<(), OdbcError> {
        Err(OdbcError::Unavailable)
    }
    pub fn query(_: &Connection, _: &str, _: Limits, _: bool, _: &AtomicBool) -> Result<DataSet, OdbcError> {
        Err(OdbcError::Unavailable)
    }
}
