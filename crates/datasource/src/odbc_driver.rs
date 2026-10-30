//! ODBC through the system driver manager (`odbc` feature): SQL Server, Oracle, DB2, Access…
//! whatever drivers the machine has.

use gridcraft_engine::datasource::{DbOp, DbRequest, DbTable, DbValue};
use odbc_api::buffers::TextRowSet;
use odbc_api::parameter::VarCharArray;
use odbc_api::{ConnectionOptions, Cursor, DataType, ResultSetMetadata};

/// Rows fetched per round trip.
const BATCH: usize = 1000;
/// Longest text read from one value; longer values are cut.
const MAX_STR: usize = 32_767;

pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    let env = odbc_api::environment().map_err(|e| format!("ODBC: {e}"))?;
    let mut conn_str = req.connection.trim().trim_end_matches(';').to_string();
    if let Some(u) = &req.user {
        conn_str.push_str(&format!(";UID={}", braced(u)));
    }
    if let Some(p) = &req.password {
        conn_str.push_str(&format!(";PWD={}", braced(p)));
    }
    let timeout = u32::try_from(req.timeout_secs).unwrap_or(u32::MAX);
    let conn = env
        .connect_with_connection_string(&conn_str, ConnectionOptions { login_timeout_sec: Some(timeout), ..Default::default() })
        .map_err(|e| format!("Could not connect through ODBC: {}", crate::redact(&e.to_string())))?;
    let text = |v: &VarCharArray<128>| v.as_str().ok().flatten().unwrap_or("").to_string();
    let text255 = |v: &VarCharArray<255>| v.as_str().ok().flatten().unwrap_or("").to_string();
    match &req.op {
        DbOp::Query => {}
        DbOp::Tables => {
            let rows = conn.tables("", "", "", "TABLE,VIEW").map_err(|e| format!("ODBC: {e}"))?;
            let mut table = DbTable { columns: vec!["schema".into(), "name".into(), "type".into()], rows: Vec::new(), truncated: false };
            for r in rows {
                let r = r.map_err(|e| format!("ODBC: {e}"))?;
                let t = |s: String| DbValue::Text(s);
                table.rows.push(vec![t(text(&r.schema)), t(text255(&r.table)), t(text255(&r.table_type))]);
            }
            return Ok(table);
        }
        DbOp::Columns { schema, table: name } => {
            let rows = conn.columns("", schema.as_deref().unwrap_or(""), name, "").map_err(|e| format!("ODBC: {e}"))?;
            let mut table = DbTable { columns: vec!["name".into(), "type".into()], rows: Vec::new(), truncated: false };
            for r in rows {
                let r = r.map_err(|e| format!("ODBC: {e}"))?;
                let ty = r.type_name.as_str().ok().flatten().unwrap_or("").to_string();
                table.rows.push(vec![DbValue::Text(text255(&r.column_name)), DbValue::Text(ty)]);
            }
            return Ok(table);
        }
    }
    let Some(mut cursor) = conn.execute(&req.query, (), Some(timeout as usize)).map_err(|e| format!("ODBC: {e}"))? else {
        return Ok(DbTable::default());
    };
    let n = cursor.num_result_cols().map_err(|e| format!("ODBC: {e}"))?;
    let n = u16::try_from(n.max(0)).unwrap_or(0);
    let mut columns = Vec::with_capacity(n.into());
    let mut types = Vec::with_capacity(n.into());
    for c in 1..=n {
        columns.push(cursor.col_name(c).map_err(|e| format!("ODBC: {e}"))?);
        types.push(cursor.col_data_type(c).unwrap_or(DataType::Unknown));
    }
    let buffer = TextRowSet::for_cursor(BATCH, &mut cursor, Some(MAX_STR)).map_err(|e| format!("ODBC: {e}"))?;
    let mut rows = cursor.bind_buffer(buffer).map_err(|e| format!("ODBC: {e}"))?;
    let mut table = DbTable { columns, rows: Vec::new(), truncated: false };
    'fetch: while let Some(batch) = rows.fetch().map_err(|e| format!("ODBC: {e}"))? {
        for r in 0..batch.num_rows() {
            if table.rows.len() >= req.max_rows {
                table.truncated = true;
                break 'fetch;
            }
            table.rows.push(
                types.iter().enumerate().map(|(c, ty)| batch.at(c, r).map_or(DbValue::Null, |b| value(&String::from_utf8_lossy(b), ty))).collect(),
            );
        }
    }
    Ok(table)
}

/// `{value}` with `}` doubled, so `;` and `=` in a user name or password can't break the string.
fn braced(s: &str) -> String {
    format!("{{{}}}", s.replace('}', "}}"))
}

fn value(s: &str, ty: &DataType) -> DbValue {
    let text = || DbValue::Text(s.to_string());
    match ty {
        DataType::Integer | DataType::SmallInt | DataType::BigInt | DataType::TinyInt => s.trim().parse().map_or_else(|_| text(), DbValue::Int),
        DataType::Numeric { .. } | DataType::Decimal { .. } | DataType::Float { .. } | DataType::Real | DataType::Double => {
            s.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map_or_else(text, DbValue::Float)
        }
        DataType::Bit => match s.trim() {
            "1" => DbValue::Bool(true),
            "0" => DbValue::Bool(false),
            _ => text(),
        },
        DataType::Date | DataType::Time { .. } | DataType::Timestamp { .. } => crate::parse_temporal(s).unwrap_or_else(text),
        _ => text(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braced_escapes() {
        assert_eq!(braced("a;b}c"), "{a;b}}c}");
    }

    #[test]
    fn values_by_type() {
        assert_eq!(value("42", &DataType::Integer), DbValue::Int(42));
        assert_eq!(value("1.50", &DataType::Decimal { precision: 10, scale: 2 }), DbValue::Float(1.5));
        assert_eq!(value("1", &DataType::Bit), DbValue::Bool(true));
        assert_eq!(value("2024-01-05 10:00:00.000", &DataType::Timestamp { precision: 3 }), DbValue::DateTime(2024, 1, 5, 10, 0, 0.0));
        assert_eq!(value("00123", &DataType::Varchar { length: None }), DbValue::Text("00123".into()));
    }

    #[test]
    fn missing_dsn_is_an_error() {
        let req = DbRequest {
            driver: "odbc".into(),
            connection: "DSN=gridcraft-does-not-exist".into(),
            password: Some("x".into()),
            user: None,
            query: "SELECT 1".into(),
            max_rows: 10,
            timeout_secs: 2,
            op: gridcraft_engine::datasource::DbOp::Query,
        };
        let e = run(&req).unwrap_err();
        assert!(!e.contains("{x}"), "{e}");
    }
}
