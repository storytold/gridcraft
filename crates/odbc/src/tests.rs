use super::*;

#[test]
fn odbc_values_keep_text_null_boolean_and_numeric_types() {
    assert_eq!(decode(ColumnKind::Text, "=SUM(A1:A2)").unwrap(), Value::text("=SUM(A1:A2)"));
    assert_eq!(decode(ColumnKind::Text, "0012").unwrap(), Value::text("0012"));
    assert_eq!(decode(ColumnKind::Integer, "42").unwrap(), Value::Number(42.0));
    assert_eq!(decode(ColumnKind::Boolean, "1").unwrap(), Value::Bool(true));
    assert_eq!(decode(ColumnKind::Boolean, "false").unwrap(), Value::Bool(false));
    assert!(decode(ColumnKind::Boolean, "not a boolean").is_err());
    for kind in [ColumnKind::Integer, ColumnKind::Decimal, ColumnKind::Float] {
        assert!(decode(kind, "not a number").is_err());
    }
    assert_eq!(decode(ColumnKind::Temporal, "2026-10-13 12:34:56.123456+03").unwrap(), Value::text("2026-10-13 12:34:56.123456+03"));
}

#[test]
fn odbc_precision_is_not_silently_lost() {
    for text in ["9007199254740993", "9223372036854775807"] {
        assert_eq!(decode(ColumnKind::Integer, text).unwrap(), Value::text(text));
    }
    assert_eq!(decode(ColumnKind::Decimal, "0.12345678901234567890").unwrap(), Value::text("0.12345678901234567890"));
    assert_eq!(decode(ColumnKind::Decimal, "12.34000").unwrap(), Value::Number(12.34));
    assert_eq!(decode(ColumnKind::Float, "NaN").unwrap(), Value::text("NaN"));
    assert_eq!(decode(ColumnKind::Decimal, "1e-400").unwrap(), Value::text("1e-400"));
}

#[test]
fn queries_are_single_read_operations_and_result_memory_is_bounded() {
    for sql in ["SELECT 1;", "WITH sample AS (SELECT 1) SELECT * FROM sample", "VALUES (1)"] {
        assert!(validate_query(sql).is_ok());
    }
    for sql in ["DELETE FROM sales", "SELECT 1; DELETE FROM sales", "COMMIT", "SELECT 1;;"] {
        assert!(validate_query(sql).is_err());
    }
    let dataset =
        DataSet { columns: vec![Column { name: "n".into(), kind: ColumnKind::Integer }], rows: vec![vec![Value::Number(1.0)]], truncated: false };
    assert!(dataset.validate(Limits { bytes: 1, ..Default::default() }).is_err());
}

#[test]
fn odbc_limits_reject_invalid_and_oversized_data() {
    assert!(Limits { rows: 0, ..Default::default() }.validate().is_err());
    assert!(Limits { rows: 100_001, ..Default::default() }.validate().is_err());
    let data =
        DataSet { columns: vec![Column { name: "text".into(), kind: ColumnKind::Text }], rows: vec![vec![Value::text("=1")]], truncated: false };
    assert!(data.validate(Limits::default()).is_ok());
    assert!(data.validate(Limits { cell_bytes: 1, ..Default::default() }).is_err());
    let ragged = DataSet { rows: vec![vec![]], ..data };
    assert!(ragged.validate(Limits::default()).is_err());
}

#[test]
fn odbc_connections_validate_without_printing_credentials() {
    let connection =
        Connection { dsn: Some("Postgres".into()), connection_string: Some("PWD=hidden".into()), user: "reader".into(), password: "hidden".into() };
    assert!(connection.validate().is_err());
    assert!(!format!("{connection:?}").contains("hidden"));
    assert!(Connection { dsn: Some("Postgres;PWD=hidden".into()), ..Default::default() }.validate().is_err());
    assert!(Connection { dsn: Some("a".repeat(33)), ..Default::default() }.validate().is_err());
    assert!(Connection { connection_string: Some("DSN=Postgres;SAVEFILE=credentials.dsn".into()), ..Default::default() }.validate().is_err());
}
