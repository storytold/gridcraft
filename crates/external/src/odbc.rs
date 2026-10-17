//! ODBC on Windows: tables of a data source, a whole table, or the result of a SQL statement,
//! read as text in batches and turned into cell values by the column's SQL type.

use gridcraft_engine::core::Value;
use gridcraft_engine::query::DbTable;
use odbc_api::buffers::TextRowSet;
use odbc_api::{ConnectionOptions, Cursor, DataType, ResultSetMetadata};

/// Rows fetched per batch.
const BATCH: usize = 500;
/// Longest text read from one field.
const MAX_FIELD: usize = 32_767;
/// Most columns read.
const MAX_COLUMNS: usize = 16_384;
/// Seconds a statement may run.
const QUERY_TIMEOUT: usize = 120;

fn env() -> Result<&'static odbc_api::Environment, String> {
    odbc_api::environment().map_err(|e| format!("ODBC isn't available: {e}"))
}

pub fn available() -> Result<(), String> {
    env().map(|_| ())
}

fn connect(connection: &str) -> Result<odbc_api::Connection<'static>, String> {
    let options = ConnectionOptions { login_timeout_sec: Some(30), ..Default::default() };
    env()?.connect_with_connection_string(connection, options).map_err(|e| format!("Couldn't connect to the data source: {e}"))
}

/// Tables and views of a connection: (schema, name), in the order the driver lists them.
fn list(conn: &odbc_api::Connection<'_>) -> Result<Vec<(Option<String>, String)>, String> {
    let mut out = Vec::new();
    let rows = conn.tables("", "", "", "TABLE,VIEW").map_err(|e| format!("Couldn't list the tables: {e}"))?;
    for row in rows {
        let row = row.map_err(|e| format!("Couldn't list the tables: {e}"))?;
        let Ok(Some(name)) = row.table.as_str() else { continue };
        if name.is_empty() {
            continue;
        }
        let schema = row.schema.as_str().ok().flatten().filter(|s| !s.is_empty()).map(str::to_string);
        // System catalogs aren't data.
        if schema.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("sys") || s.eq_ignore_ascii_case("INFORMATION_SCHEMA")) {
            continue;
        }
        out.push((schema, name.to_string()));
        if out.len() >= 100_000 {
            break;
        }
    }
    Ok(out)
}

fn display((schema, name): &(Option<String>, String)) -> String {
    match schema {
        Some(s) => format!("{s}.{name}"),
        None => name.clone(),
    }
}

/// Tables and views of the data source (`schema.table` where the source has schemas).
pub fn tables(connection: &str) -> Result<Vec<String>, String> {
    let conn = connect(connection)?;
    Ok(list(&conn)?.iter().map(display).collect())
}

/// Every row of `table` (a name [`tables`] listed).
pub fn table(connection: &str, table: &str, max_rows: usize) -> Result<DbTable, String> {
    let conn = connect(connection)?;
    let found = list(&conn)?
        .into_iter()
        .find(|t| display(t).eq_ignore_ascii_case(table))
        .ok_or_else(|| format!("The data source has no table '{table}'."))?;
    let open = conn.identifier_quote_char().ok().flatten().filter(|c| *c != ' ').unwrap_or('"');
    let close = if open == '[' { ']' } else { open };
    let quote = |part: &str| format!("{open}{}{close}", part.replace(close, &format!("{close}{close}")));
    let target = match &found.0 {
        Some(schema) => format!("{}.{}", quote(schema), quote(&found.1)),
        None => quote(&found.1),
    };
    run(&conn, &format!("SELECT * FROM {target}"), max_rows)
}

/// The result of a SQL statement.
pub fn query(connection: &str, sql: &str, max_rows: usize) -> Result<DbTable, String> {
    let conn = connect(connection)?;
    run(&conn, sql, max_rows)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Number,
    Logical,
    Text,
}

fn kind(t: DataType) -> Kind {
    match t {
        DataType::Numeric { .. }
        | DataType::Decimal { .. }
        | DataType::Integer
        | DataType::SmallInt
        | DataType::Float { .. }
        | DataType::Real
        | DataType::Double
        | DataType::BigInt
        | DataType::TinyInt => Kind::Number,
        DataType::Bit => Kind::Logical,
        _ => Kind::Text,
    }
}

fn run(conn: &odbc_api::Connection<'_>, sql: &str, max_rows: usize) -> Result<DbTable, String> {
    let fail = |e: odbc_api::Error| format!("The query failed: {e}");
    // Drivers without statement timeouts (the text and Access drivers) answer HYC00: run without.
    let executed = match conn.execute(sql, (), Some(QUERY_TIMEOUT)) {
        Err(e) if e.to_string().contains("HYC00") => conn.execute(sql, (), None),
        other => other,
    };
    let Some(mut cursor) = executed.map_err(fail)? else {
        return Ok(DbTable::default());
    };
    let n = usize::try_from(cursor.num_result_cols().map_err(fail)?).unwrap_or(0).min(MAX_COLUMNS);
    let mut columns = Vec::with_capacity(n);
    let mut kinds = Vec::with_capacity(n);
    for i in 1..=n {
        let col = u16::try_from(i).map_err(|_| "Too many columns.".to_string())?;
        columns.push(cursor.col_name(col).unwrap_or_else(|_| format!("Column{i}")));
        kinds.push(cursor.col_data_type(col).map(kind).unwrap_or(Kind::Text));
    }
    let buffer = TextRowSet::for_cursor(BATCH, &mut cursor, Some(MAX_FIELD)).map_err(fail)?;
    let mut block = cursor.bind_buffer(buffer).map_err(fail)?;
    let mut rows: Vec<Vec<Value>> = Vec::new();
    while let Some(batch) = block.fetch().map_err(fail)? {
        for r in 0..batch.num_rows() {
            if rows.len() >= max_rows {
                return Ok(DbTable { columns, rows });
            }
            let row = (0..n)
                .map(|c| match batch.at_as_str(c, r) {
                    Ok(Some(s)) => match kinds.get(c) {
                        Some(Kind::Number) => {
                            s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).map(Value::number).unwrap_or_else(|| Value::text(s))
                        }
                        Some(Kind::Logical) => Value::Bool(s.trim() == "1" || s.trim().eq_ignore_ascii_case("true")),
                        _ => Value::text(s),
                    },
                    Ok(None) => Value::Empty,
                    Err(_) => Value::Error(gridcraft_engine::core::CellError::Value),
                })
                .collect();
            rows.push(row);
        }
    }
    Ok(DbTable { columns, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_csv_through_the_access_text_driver_when_installed() {
        let dir = std::env::temp_dir().join(format!("gridcraft-odbc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("people.csv"),
            "Name,Age
Ann,30
Bob,41
",
        )
        .unwrap();
        let conn = format!("Driver={{Microsoft Access Text Driver (*.txt, *.csv)}};Dbq={};Extensions=csv;", dir.display());
        // Machines without the driver (it comes with Office or the Access Database Engine) skip.
        let Ok(names) = tables(&conn) else {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        };
        assert!(names.iter().any(|t| t.eq_ignore_ascii_case("people.csv")), "{names:?}");
        let t = table(&conn, "people.csv", 100).unwrap();
        assert_eq!(t.columns, vec!["Name", "Age"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1][0], Value::text("Bob"));
        assert_eq!(t.rows[1][1], Value::number(41.0));
        let q = query(&conn, "SELECT Name FROM [people.csv] WHERE Age > 35", 100).unwrap();
        assert_eq!(q.rows, vec![vec![Value::text("Bob")]]);
        assert!(table(&conn, "nope.csv", 10).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_connection_is_an_error_not_a_panic() {
        // Windows ships the ODBC driver manager; a data source that doesn't exist fails cleanly.
        if available().is_ok() {
            assert!(tables("DSN=GridCraftNoSuchDataSource").is_err());
            assert!(query("Driver={No Such Driver};", "SELECT 1", 10).is_err());
        }
    }
}
