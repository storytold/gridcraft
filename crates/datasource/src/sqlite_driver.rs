//! SQLite files (bundled SQLite), opened read-only.

use std::time::{Duration, Instant};

use gridcraft_engine::datasource::{DbRequest, DbTable, DbValue};
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    let path = req.connection.strip_prefix("sqlite://").or_else(|| req.connection.strip_prefix("sqlite:")).unwrap_or(&req.connection);
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Could not open the SQLite database: {e}"))?;
    let timeout = Duration::from_secs(req.timeout_secs);
    conn.busy_timeout(timeout).map_err(|e| format!("SQLite: {e}"))?;
    // Interrupt a query that runs past the timeout. This module is native only, so the clock is safe.
    #[allow(clippy::disallowed_methods)]
    let deadline = Instant::now() + timeout;
    #[allow(clippy::disallowed_methods)]
    let past_deadline = move || Instant::now() > deadline;
    conn.progress_handler(10_000, Some(past_deadline)).map_err(|e| format!("SQLite: {e}"))?;
    conn.authorizer(Some(authorize)).map_err(|e| format!("SQLite: {e}"))?;
    let mut stmt = conn.prepare(&req.query).map_err(|e| format!("SQLite: {e}"))?;
    let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let decl: Vec<String> = stmt.columns().iter().map(|c| c.decl_type().unwrap_or("").to_ascii_uppercase()).collect();
    let n = columns.len();
    let mut rows = stmt.query([]).map_err(|e| format!("SQLite: {e}"))?;
    let mut table = DbTable { columns, rows: Vec::new(), truncated: false };
    while let Some(row) = rows.next().map_err(|e| format!("SQLite: {e}"))? {
        if table.rows.len() >= req.max_rows {
            table.truncated = true;
            break;
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let v = row.get_ref(i).map_err(|e| format!("SQLite: {e}"))?;
            out.push(value(v, decl.get(i).map_or("", String::as_str)));
        }
        table.rows.push(out);
    }
    Ok(table)
}

fn value(v: ValueRef<'_>, decl: &str) -> DbValue {
    match v {
        ValueRef::Null => DbValue::Null,
        ValueRef::Integer(n) if decl.contains("BOOL") => DbValue::Bool(n != 0),
        ValueRef::Integer(n) => DbValue::Int(n),
        ValueRef::Real(f) => DbValue::Float(f),
        ValueRef::Text(t) => {
            let s = String::from_utf8_lossy(t).into_owned();
            // SQLite has no date type: columns declared DATE/TIME hold ISO text.
            if decl.contains("DATE") || decl.contains("TIME") { crate::parse_temporal(&s).unwrap_or(DbValue::Text(s)) } else { DbValue::Text(s) }
        }
        ValueRef::Blob(b) => DbValue::Text(format!("({} bytes)", b.len())),
    }
}

/// Keeps the query inside the one local file the connection names: ATTACH (which VACUUM INTO
/// also goes through) could open a network share or write a file of the query's choosing.
fn authorize(ctx: AuthContext<'_>) -> Authorization {
    match ctx.action {
        AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
        _ => Authorization::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (std::path::PathBuf, Connection) {
        let dir = std::env::temp_dir().join(format!("gridcraft-ds-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE t (id INTEGER, name TEXT, price REAL, born DATE, seen DATETIME, ok BOOLEAN, pic BLOB);
             INSERT INTO t VALUES (1, 'a', 2.5, '2024-01-05', '2024-01-05 10:30:00', 1, x'0102');
             INSERT INTO t VALUES (2, NULL, NULL, 'not a date', NULL, 0, NULL);
             INSERT INTO t VALUES (3, 'c', 1, '2024-02-29', NULL, NULL, NULL);",
        )
        .unwrap();
        (path, c)
    }

    fn req(path: &std::path::Path, q: &str, max_rows: usize) -> DbRequest {
        DbRequest {
            driver: "sqlite".into(),
            connection: path.display().to_string(),
            password: None,
            user: None,
            query: q.into(),
            max_rows,
            timeout_secs: 5,
            op: gridcraft_engine::datasource::DbOp::Query,
        }
    }

    #[test]
    fn reads_typed_rows() {
        let (path, _c) = db();
        let t = run(&req(&path, "SELECT * FROM t ORDER BY id", 100)).unwrap();
        assert_eq!(t.columns, ["id", "name", "price", "born", "seen", "ok", "pic"]);
        assert_eq!(t.rows.len(), 3);
        assert!(!t.truncated);
        assert_eq!(
            t.rows[0],
            vec![
                DbValue::Int(1),
                DbValue::Text("a".into()),
                DbValue::Float(2.5),
                DbValue::Date(2024, 1, 5),
                DbValue::DateTime(2024, 1, 5, 10, 30, 0.0),
                DbValue::Bool(true),
                DbValue::Text("(2 bytes)".into()),
            ]
        );
        assert_eq!(t.rows[1][1], DbValue::Null);
        assert_eq!(t.rows[1][3], DbValue::Text("not a date".into()));
    }

    #[test]
    fn max_rows_truncates() {
        let (path, _c) = db();
        let t = run(&req(&path, "SELECT id FROM t", 2)).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert!(t.truncated);
    }

    #[test]
    fn read_only_and_errors() {
        let (path, _c) = db();
        assert!(run(&req(&path, "DELETE FROM t", 10)).is_err());
        assert!(run(&req(&path, "SELECT nope FROM t", 10)).is_err());
        assert!(run(&req(std::path::Path::new("/nonexistent/x.db"), "SELECT 1", 10)).is_err());
    }

    #[test]
    fn stored_sql_cannot_reach_other_files() {
        let (path, _c) = db();
        let other = path.with_file_name("other.db");
        let copy = path.with_file_name("copy.db");
        let _ = std::fs::remove_file(&copy);
        Connection::open(&other).unwrap().execute_batch("CREATE TABLE o (x); INSERT INTO o VALUES (1);").unwrap();
        for q in [
            format!("ATTACH DATABASE '{}' AS a", other.display()),
            format!("ATTACH '{}' AS a; SELECT * FROM a.o", other.display()),
            format!("VACUUM INTO '{}'", copy.display()),
            "DETACH DATABASE main".to_string(),
        ] {
            assert!(run(&req(&path, &q, 10)).is_err(), "{q} ran");
        }
        assert!(!copy.exists(), "VACUUM INTO wrote a file");
        // Ordinary reads, CTEs and the catalog's table-valued pragma still work.
        assert!(run(&req(&path, "WITH c(x) AS (SELECT 1) SELECT x FROM c", 10)).is_ok());
        assert!(run(&req(&path, "SELECT name FROM pragma_table_info('t')", 10)).is_ok());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn runaway_query_is_interrupted() {
        let (path, _c) = db();
        let mut r = req(&path, "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c", 10);
        r.timeout_secs = 1;
        let started = Instant::now();
        assert!(run(&r).is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn lists_tables_and_columns() {
        use gridcraft_engine::datasource::DbOp;
        let (path, c) = db();
        c.execute_batch("CREATE VIEW v AS SELECT id FROM t;").unwrap();
        drop(c);
        let tables = crate::run(&DbRequest { op: DbOp::Tables, ..req(&path, "", 100) }).unwrap();
        let names: Vec<(DbValue, DbValue)> = tables.rows.iter().map(|r| (r[1].clone(), r[2].clone())).collect();
        assert_eq!(names, [(DbValue::Text("t".into()), DbValue::Text("TABLE".into())), (DbValue::Text("v".into()), DbValue::Text("VIEW".into()))]);
        let cols = crate::run(&DbRequest { op: DbOp::Columns { schema: None, table: "t".into() }, ..req(&path, "", 100) }).unwrap();
        assert_eq!(cols.rows.len(), 7);
        assert_eq!(cols.rows[1], vec![DbValue::Text("name".into()), DbValue::Text("TEXT".into())]);
        let q = gridcraft_engine::datasource::select_sql("sqlite", None, "t", &["id".into(), "name".into()]);
        assert_eq!(crate::run(&req(&path, &q, 100)).unwrap().columns, ["id", "name"]);
    }
}
