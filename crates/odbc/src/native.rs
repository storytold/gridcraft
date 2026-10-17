use std::{
    mem::size_of,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use gridcraft_core::Value;
use odbc_api::{
    ConnectionOptions, Cursor, DataType, Environment, ResultSetMetadata,
    buffers::Indicator,
    parameter::{VarCell, WideText},
};

use crate::{Column, ColumnKind, Connection as Settings, DataSet, Limits, OdbcError, Sources, decode};

mod profile;
use profile::Driver;

fn env() -> Result<&'static Environment, OdbcError> {
    static ENV: OnceLock<Result<Environment, OdbcError>> = OnceLock::new();
    ENV.get_or_init(|| Environment::new().map_err(|_| OdbcError::Unavailable)).as_ref().map_err(Clone::clone)
}

pub fn sources() -> Result<Sources, OdbcError> {
    let env = env()?;
    let data_sources = env.data_sources().map_err(|_| OdbcError::Unavailable)?.into_iter().map(|s| s.server_name).collect();
    let drivers = env.drivers().map_err(|_| OdbcError::Unavailable)?.into_iter().map(|d| d.description).collect();
    Ok(Sources { data_sources, drivers })
}

fn connect<'a>(env: &'a Environment, settings: &Settings) -> Result<(odbc_api::Connection<'a>, Driver), OdbcError> {
    let attrs = profile::attributes(settings)?;
    let (key, name) = profile::selector(&attrs)?;
    let description = if key == "DSN" {
        env.data_sources()
            .map_err(|_| OdbcError::Unavailable)?
            .into_iter()
            .find(|source| source.server_name.eq_ignore_ascii_case(name))
            .ok_or(OdbcError::ConnectionFailed)?
            .driver
    } else {
        env.drivers()
            .map_err(|_| OdbcError::Unavailable)?
            .into_iter()
            .find(|driver| driver.description.eq_ignore_ascii_case(name))
            .ok_or(OdbcError::ConnectionFailed)?
            .description
    };
    let driver = Driver::from_name(&description)?;
    let options = ConnectionOptions { login_timeout_sec: Some(15), ..Default::default() };
    let connection_string = profile::connection_string(settings, &attrs, driver)?;
    let connection = env.connect_with_connection_string(&connection_string, options).map_err(driver_error)?;
    if !driver.accepts(&connection.database_management_system_name().map_err(driver_error)?) {
        return Err(OdbcError::UnsupportedDatabase);
    }
    Ok((connection, driver))
}

fn driver_error(error: odbc_api::Error) -> OdbcError {
    if let odbc_api::Error::Diagnostics { record, .. } = error
        && record.state.0.iter().all(u8::is_ascii_alphanumeric)
        && let Ok(state) = std::str::from_utf8(&record.state.0)
    {
        return OdbcError::DriverState(state.into());
    }
    OdbcError::QueryFailed
}

fn begin(connection: &odbc_api::Connection<'_>, driver: Driver) -> Result<(), OdbcError> {
    connection.set_autocommit(false).map_err(driver_error)?;
    if let Some(sql) = driver.begin_sql() {
        connection.execute(sql, (), Some(60)).map_err(driver_error)?;
    }
    Ok(())
}

pub fn test(settings: &Settings) -> Result<(), OdbcError> {
    let (connection, driver) = connect(env()?, settings)?;
    begin(&connection, driver)?;
    drop(connection.execute(driver.test_sql(), (), Some(15)).map_err(driver_error)?);
    connection.rollback().map_err(driver_error)
}

fn column_kind(driver: Driver, data_type: DataType) -> Result<ColumnKind, OdbcError> {
    Ok(match data_type {
        DataType::Integer | DataType::SmallInt | DataType::BigInt | DataType::TinyInt => ColumnKind::Integer,
        DataType::Numeric { .. } | DataType::Decimal { .. } => ColumnKind::Decimal,
        DataType::Float { .. } | DataType::Real | DataType::Double => ColumnKind::Float,
        DataType::Bit => ColumnKind::Boolean,
        DataType::Date | DataType::Time { .. } | DataType::Timestamp { .. } => ColumnKind::Temporal,
        DataType::Char { .. }
        | DataType::WChar { .. }
        | DataType::Varchar { .. }
        | DataType::WVarchar { .. }
        | DataType::LongVarchar { .. }
        | DataType::WLongVarchar { .. } => ColumnKind::Text,
        DataType::Other { data_type, .. } if data_type.0 == -11 => ColumnKind::Text, // SQL_GUID
        DataType::Other { data_type, .. } if driver == Driver::SqlServer && matches!(data_type.0, -154 | -155) => ColumnKind::Temporal,
        DataType::Other { data_type, .. } if driver == Driver::SqlServer && data_type.0 == -152 => ColumnKind::Text, // SQL_SS_XML
        DataType::Unknown if driver == Driver::Sqlite => ColumnKind::Text,
        _ => return Err(OdbcError::InvalidData),
    })
}

pub fn query(settings: &Settings, sql: &str, limits: Limits, preview: bool, cancelled: &AtomicBool) -> Result<DataSet, OdbcError> {
    let limits = limits.validate()?;
    crate::validate_query(sql)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(OdbcError::Cancelled);
    }
    let (connection, driver) = connect(env()?, settings)?;
    let sql = driver.query_sql(sql)?;
    begin(&connection, driver)?;
    let mut cursor = connection.execute(&sql, (), Some(60)).map_err(driver_error)?.ok_or(OdbcError::InvalidData)?;
    let count = cursor.num_result_cols().map_err(|_| OdbcError::InvalidData)?;
    if count <= 0 || count as usize > limits.columns {
        return Err(OdbcError::Limit("The result has too many columns."));
    }
    let mut columns = Vec::with_capacity(count as usize);
    for i in 1..=count as u16 {
        let kind = column_kind(driver, cursor.col_data_type(i).map_err(|_| OdbcError::InvalidData)?)?;
        let name = cursor.col_name(i).map_err(|_| OdbcError::InvalidData)?;
        if name.len() > 1024 || name.contains('\0') {
            return Err(OdbcError::InvalidData);
        }
        columns.push(Column { name, kind });
    }
    let mut rows = Vec::new();
    let max_rows = if preview { limits.rows.min(100) } else { limits.rows };
    let mut bytes = columns.iter().map(|c| c.name.len() + size_of::<Column>()).sum::<usize>();
    let mut truncated = false;
    let mut buffer = vec![0u16; limits.cell_bytes.saturating_add(1)];
    while let Some(mut row) = cursor.next_row().map_err(|_| OdbcError::InvalidData)? {
        if cancelled.load(Ordering::Relaxed) {
            return Err(OdbcError::Cancelled);
        }
        if rows.len() == max_rows {
            if !preview {
                return Err(OdbcError::Limit("The result exceeds the row limit. Restrict the query or increase maxRows."));
            }
            truncated = true;
            break;
        }
        bytes = bytes
            .checked_add(size_of::<Vec<Value>>() + columns.len() * size_of::<Value>())
            .ok_or(OdbcError::Limit("The result exceeds the supported size."))?;
        if bytes > limits.bytes {
            return Err(OdbcError::Limit("The result exceeds the supported size."));
        }
        let mut values = Vec::with_capacity(columns.len());
        for (i, column) in columns.iter().enumerate() {
            let mut cell = VarCell::<&mut [u16], WideText>::from_buffer(buffer.as_mut_slice(), Indicator::Null);
            row.get_data((i + 1) as u16, &mut cell).map_err(|_| OdbcError::InvalidData)?;
            if !cell.is_complete() {
                return Err(OdbcError::Limit("A result cell exceeds the supported size."));
            }
            let value = match cell.as_slice() {
                None => Value::Empty,
                Some(raw) => {
                    let text = String::from_utf16(raw).map_err(|_| OdbcError::InvalidData)?;
                    if text.len() > limits.cell_bytes {
                        return Err(OdbcError::Limit("A result cell exceeds the supported size."));
                    }
                    decode(column.kind, &text)?
                }
            };
            if let Value::Text(text) = &value {
                bytes = bytes.checked_add(text.len() + 2 * size_of::<usize>()).ok_or(OdbcError::Limit("The result exceeds the supported size."))?;
            }
            if bytes > limits.bytes {
                return Err(OdbcError::Limit("The result exceeds the supported size."));
            }
            values.push(value);
        }
        rows.push(values);
    }
    let dataset = DataSet { columns, rows, truncated };
    drop(cursor);
    connection.rollback().map_err(|_| OdbcError::QueryFailed)?;
    dataset.validate(limits)?;
    Ok(dataset)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vendor_types_preserve_identifiers_offsets_and_binary_rejection() {
        let custom = |code| DataType::Other { data_type: odbc_api::sys::SqlDataType(code), column_size: None, decimal_digits: 0 };
        assert_eq!(column_kind(Driver::SqlServer, custom(-11)).unwrap(), ColumnKind::Text);
        for code in [-154, -155] {
            assert_eq!(column_kind(Driver::SqlServer, custom(code)).unwrap(), ColumnKind::Temporal);
        }
        assert_eq!(column_kind(Driver::SqlServer, custom(-152)).unwrap(), ColumnKind::Text);
        assert!(column_kind(Driver::Oracle, custom(-155)).is_err());
        assert_eq!(column_kind(Driver::Sqlite, DataType::Unknown).unwrap(), ColumnKind::Text);
        assert!(column_kind(Driver::SqlServer, DataType::Unknown).is_err());
        for driver in [Driver::Postgres, Driver::SqlServer, Driver::MySql, Driver::MariaDb, Driver::Oracle, Driver::Sqlite] {
            assert!(column_kind(driver, DataType::Binary { length: None }).is_err());
            assert_eq!(column_kind(driver, DataType::Numeric { precision: 38, scale: 20 }).unwrap(), ColumnKind::Decimal);
        }
    }

    #[test]
    fn credentials_escape_braces_and_separators_without_quoting_the_dsn() {
        let settings = Settings { dsn: Some("Postgres".into()), user: "reader".into(), password: "{pass};+".into(), connection_string: None };
        let value = profile::connection_string(&settings, &profile::attributes(&settings).unwrap(), Driver::Postgres).unwrap();
        assert!(value.contains("UID=reader;PWD={{pass}};+};DSN=Postgres;"));
        assert!(value.starts_with("BoolsAsChar=0;ReadOnly=0;CommLog=0;Debug=0;ConnSettings={};"));
        let unsafe_settings = Settings {
            connection_string: Some("Driver={PostgreSQL Unicode};CX=ff;A0=0;B2=1;B3=1;A6={DELETE FROM x};B6=0;BI=8;D6=8".into()),
            ..Default::default()
        };
        let value = profile::connection_string(&unsafe_settings, &profile::attributes(&unsafe_settings).unwrap(), Driver::Postgres).unwrap();
        assert!(value.contains("D6=8;BoolsAsChar=0;ReadOnly=0;CommLog=0;Debug=0;ConnSettings={};"));
        assert!(value.ends_with("UseDeclareFetch=1;Fetch=100;BI=-5;NumericAS=2;LFConversion=0;IgnoreTimeout=0;"));
    }
}
