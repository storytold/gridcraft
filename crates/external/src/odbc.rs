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
/// Bytes one batch's buffer may take: wide text columns (`varchar(max)`, memo) each reserve the
/// longest field for every row of the batch.
const BATCH_BYTES: usize = 64 * 1024 * 1024;
/// Most columns read.
const MAX_COLUMNS: usize = 16_384;
/// Seconds a statement may run.
const QUERY_TIMEOUT: usize = 120;
/// Seconds a whole ODBC call (connect, run, fetch) may take before the caller gets an error.
/// Statement timeouts don't cover every driver (the text and Access drivers have none) or the
/// fetch, so this bounds a refresh whatever the driver does.
const CALL_DEADLINE: u64 = 180;

const DEADLINE: std::time::Duration = std::time::Duration::from_secs(CALL_DEADLINE);
/// Most ODBC calls running at once, counting calls past their deadline that haven't returned yet.
const MAX_RUNNING: usize = 4;

/// The ODBC calls running now, by what they do (`query`, connection, SQL…). A call keeps its
/// entry until its thread ends, past its deadline too.
static RUNNING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn running() -> std::sync::MutexGuard<'static, Vec<String>> {
    RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A reserved place in [`RUNNING`], given back when the worker thread ends (or never starts).
struct Slot(String);

impl Drop for Slot {
    fn drop(&mut self) {
        let mut r = running();
        if let Some(i) = r.iter().position(|k| *k == self.0) {
            r.swap_remove(i);
        }
    }
}

/// Runs `work` on its own thread and waits at most `deadline`. ODBC can't always be interrupted,
/// so a call past its deadline goes on in the background and its result is dropped; until it
/// ends, the same call (`key`) isn't started again (a statement that writes would run twice),
/// and at most `MAX_RUNNING` calls run at once.
fn bounded<T: Send + 'static>(
    deadline: std::time::Duration,
    key: String,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let slot = {
        let mut r = running();
        if r.contains(&key) {
            return Err("This statement is still running on the data source from an earlier attempt; try again once it has finished.".into());
        }
        if r.len() >= MAX_RUNNING {
            return Err(format!("{MAX_RUNNING} ODBC calls are still running on their data sources; try again once one has finished."));
        }
        r.push(key.clone());
        Slot(key)
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("gridcraft-odbc".into())
        .spawn(move || {
            let _slot = slot;
            let _ = tx.send(work());
        })
        .map_err(|e| format!("Couldn't start the ODBC call: {e}"))?;
    match rx.recv_timeout(deadline) {
        Ok(r) => r,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "The data source didn't answer within {} seconds. The statement may still be running there; GridCraft won't start it again until it has finished.",
            deadline.as_secs()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err("The ODBC call ended without a result.".into()),
    }
}

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
    let connection = connection.to_string();
    bounded(DEADLINE, format!("tables\u{0}{connection}"), move || {
        let conn = connect(&connection)?;
        Ok(list(&conn)?.iter().map(display).collect())
    })
}

/// Every row of `table` (a name [`tables`] listed).
pub fn table(connection: &str, table: &str, max_rows: usize) -> Result<DbTable, String> {
    let (connection, table) = (connection.to_string(), table.to_string());
    bounded(DEADLINE, format!("table\u{0}{connection}\u{0}{table}"), move || read_table(&connection, &table, max_rows))
}

fn read_table(connection: &str, table: &str, max_rows: usize) -> Result<DbTable, String> {
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
    let (connection, sql) = (connection.to_string(), sql.to_string());
    bounded(DEADLINE, format!("query\u{0}{connection}\u{0}{sql}"), move || {
        let conn = connect(&connection)?;
        run(&conn, &sql, max_rows)
    })
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
    // Drivers without statement timeouts (the text and Access drivers) answer HYC00: run without;
    // `bounded` still limits the whole call.
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
    // Fewer rows per batch, and shorter fields when there are very many columns, so one batch
    // stays within BATCH_BYTES however many wide columns the result has.
    let field = (BATCH_BYTES / n.max(1)).saturating_sub(1).clamp(255, MAX_FIELD);
    let rows_per_batch = (BATCH_BYTES / n.max(1).saturating_mul(field + 1)).clamp(1, BATCH);
    let buffer = TextRowSet::for_cursor(rows_per_batch, &mut cursor, Some(field)).map_err(fail)?;
    let mut block = cursor.bind_buffer(buffer).map_err(fail)?;
    // At MAX_FIELD a longer value is cut to what a cell holds anyway (Excel's 32,767-character
    // cell limit). A shorter field (a very wide result) must not cut values silently: a value
    // that doesn't fit fails the read instead.
    let shortened = field < MAX_FIELD;
    let mut rows: Vec<Vec<Value>> = Vec::new();
    while let Some(batch) = block.fetch_with_truncation_check(shortened).map_err(|e| match e {
        odbc_api::Error::TooLargeValueForBuffer { .. } => format!(
            "The result has {n} columns and a value longer than the {field} bytes GridCraft can read per field at that width; select fewer columns."
        ),
        e => fail(e),
    })? {
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
    #[allow(clippy::disallowed_methods)] // this crate is never built for wasm
    fn a_call_past_its_deadline_returns_an_error_and_holds_its_place_until_it_ends() {
        let short = std::time::Duration::from_millis(50);
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let start = std::time::Instant::now();
        let r: Result<(), String> = bounded(short, "slow".into(), move || {
            let _ = wait.recv_timeout(std::time::Duration::from_secs(10));
            Ok(())
        });
        assert!(r.unwrap_err().contains("may still be running"));
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        // The same call isn't started again while the first one runs; others are.
        assert!(bounded(DEADLINE, "slow".into(), || Ok(1)).unwrap_err().contains("still running"));
        assert_eq!(bounded(DEADLINE, "other".into(), || Ok(7)), Ok(7));
        assert_eq!(bounded::<()>(DEADLINE, "failing".into(), || Err("no".into())), Err("no".into()));
        // At most MAX_RUNNING calls at once, counting the one past its deadline.
        let mut holds = Vec::new();
        for i in 1..MAX_RUNNING {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let r: Result<(), String> = bounded(short, format!("held{i}"), move || {
                let _ = rx.recv_timeout(std::time::Duration::from_secs(10));
                Ok(())
            });
            assert!(r.is_err());
            holds.push(tx);
        }
        assert!(bounded(DEADLINE, "one more".into(), || Ok(1)).unwrap_err().contains("ODBC calls are still running"));
        // Once the calls end, their places are free again.
        let _ = release.send(());
        for tx in holds {
            let _ = tx.send(());
        }
        let mut freed = false;
        for _ in 0..200 {
            if running().is_empty() {
                freed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(freed);
        assert_eq!(bounded(DEADLINE, "slow".into(), || Ok(2)), Ok(2));
    }

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
