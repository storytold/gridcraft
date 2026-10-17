#![cfg(target_os = "windows")]
//! Opt in with a dedicated test connection; all queries use synthetic expressions.
use gridcraft_core::Value;
use gridcraft_odbc::{Backend, Connection, Limits, NativeBackend, OdbcError};
use std::sync::atomic::AtomicBool;

#[test]
#[ignore = "requires GRIDCRAFT_TEST_ODBC_DSN or GRIDCRAFT_TEST_ODBC_CONNECTION_STRING"]
fn postgres_psqlodbc_reads_types_unicode_precision_and_read_only_transactions() {
    let dsn = std::env::var("GRIDCRAFT_TEST_ODBC_DSN").ok();
    let connection = Connection {
        connection_string: if dsn.is_some() {
            None
        } else {
            Some(std::env::var("GRIDCRAFT_TEST_ODBC_CONNECTION_STRING").expect("test connection string"))
        },
        dsn,
        user: std::env::var("GRIDCRAFT_TEST_ODBC_USER").unwrap_or_default(),
        password: std::env::var("GRIDCRAFT_TEST_ODBC_PASSWORD").unwrap_or_default(),
    };
    let backend = NativeBackend;
    backend.test(&connection).unwrap();
    if let Ok(connection_string) = std::env::var("GRIDCRAFT_TEST_ODBC_CONNECTION_STRING") {
        let mut alternate = connection.clone();
        alternate.dsn = None;
        alternate.connection_string = Some(connection_string);
        backend.test(&alternate).unwrap();
    }
    let cancel = AtomicBool::new(false);
    let sql = "SELECT 9007199254740993::bigint AS identifier, 0.12345678901234567890::numeric(30,20) AS exact_amount, 12.34::numeric(10,2) AS amount, true AS active, '=1'::text AS literal, 'ação 🦊'::text AS unicode, timestamp '2026-10-14 12:34:56.123456' AS recorded, NULL::text AS optional_text, 42::integer AS count";
    let data = backend.query(&connection, sql, Limits::default(), false, &cancel).unwrap();
    let row = &data.rows[0];
    assert_eq!(row[0], Value::text("9007199254740993"));
    assert_eq!(row[1], Value::text("0.12345678901234567890"));
    assert_eq!(row[2], Value::Number(12.34));
    assert_eq!(row[3], Value::Bool(true));
    assert_eq!(row[4], Value::text("=1"));
    assert_eq!(row[5], Value::text("ação 🦊"));
    assert_eq!(row[6], Value::text("2026-10-14 12:34:56.123456"));
    assert_eq!(row[7], Value::Empty);
    assert_eq!(row[8], Value::Number(42.0));
    let read_only = backend.query(&connection, "SELECT current_setting('transaction_read_only') AS mode", Limits::default(), false, &cancel).unwrap();
    assert_eq!(read_only.rows[0][0], Value::text("on"));
    let preview = backend.query(&connection, "SELECT generate_series(1,101) AS value", Limits::default(), true, &cancel).unwrap();
    assert_eq!(preview.rows.len(), 100);
    assert!(preview.truncated);
    assert!(matches!(
        backend.query(&connection, "SELECT generate_series(1,2) AS value", Limits { rows: 1, ..Default::default() }, false, &cancel),
        Err(OdbcError::Limit(_))
    ));
    let empty = backend.query(&connection, "SELECT 1 AS value WHERE false", Limits::default(), false, &cancel).unwrap();
    assert_eq!(empty.columns.len(), 1);
    assert!(empty.rows.is_empty());
    assert!(backend.query(&connection, "SELECT decode('ff','hex') AS binary", Limits::default(), false, &cancel).is_err());
    assert!(matches!(backend.query(&connection, "SELECT 1; SELECT 2", Limits::default(), false, &cancel), Err(OdbcError::InvalidQuery)));
    let cancelled = AtomicBool::new(true);
    assert!(matches!(backend.query(&connection, "SELECT 1", Limits::default(), false, &cancelled), Err(OdbcError::Cancelled)));
}
