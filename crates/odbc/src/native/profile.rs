use crate::{Connection as Settings, OdbcError};
use sqlparser::{
    dialect::{Dialect, MsSqlDialect, MySqlDialect, OracleDialect, PostgreSqlDialect, SQLiteDialect},
    tokenizer::{Token, Tokenizer},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Driver {
    Postgres,
    SqlServer,
    MySql,
    MariaDb,
    Oracle,
    Sqlite,
}

impl Driver {
    pub fn from_name(name: &str) -> Result<Self, OdbcError> {
        let name = name.to_ascii_lowercase();
        if name.contains("postgresql") {
            Ok(Self::Postgres)
        } else if name.contains("sql server") {
            Ok(Self::SqlServer)
        } else if name.contains("mariadb") {
            Ok(Self::MariaDb)
        } else if name.contains("mysql") {
            Ok(Self::MySql)
        } else if name.contains("oracle") {
            Ok(Self::Oracle)
        } else if name.contains("sqlite3") || name == "sqlite" {
            Ok(Self::Sqlite)
        } else {
            Err(OdbcError::UnsupportedDatabase)
        }
    }

    pub fn accepts(self, dbms: &str) -> bool {
        let dbms = dbms.to_ascii_lowercase();
        match self {
            Self::Postgres => dbms.contains("postgresql"),
            Self::SqlServer => dbms.contains("sql server"),
            Self::MySql | Self::MariaDb => dbms.contains("mysql") || dbms.contains("mariadb"),
            Self::Oracle => dbms.contains("oracle"),
            Self::Sqlite => dbms.contains("sqlite"),
        }
    }

    fn options(self) -> &'static str {
        match self {
            Self::Postgres => {
                // psqlODBC's read-only flag blocks SET TRANSACTION too; the server enforces our read-only transaction.
                "BoolsAsChar=0;ReadOnly=0;CommLog=0;Debug=0;ConnSettings={};UseDeclareFetch=1;Fetch=100;BI=-5;NumericAS=2;LFConversion=0;IgnoreTimeout=0;"
            }
            Self::SqlServer => "ApplicationIntent=ReadOnly;MARS_Connection=No;",
            Self::MySql => {
                "OPTION=3145744;INITSTMT={};STMT={};NO_CACHE=1;FORWARD_CURSOR=1;MULTI_STATEMENTS=0;NO_TRANSACTIONS=0;AUTO_RECONNECT=0;LOG_QUERY=0;USE_MYCNF=0;CHARSET=utf8mb4;"
            }
            Self::MariaDb => "OPTION=3145744;OPTIONS=3145744;INITSTMT={};NO_CACHE=1;FORWARDONLY=1;AUTO_RECONNECT=0;USE_MYCNF=0;CHARSET=utf8mb4;",
            // DBA=R starts a second read-only transaction in the driver, conflicting with SET TRANSACTION.
            Self::Oracle => "DBA=W;QTO=T;FBS=65536;BNF=F;BTD=F;FEN=F;EXC=F;MTS=T;",
            Self::Sqlite => "NoCreat=1;StepAPI=1;NoTXN=0;NoWCHAR=0;LoadExt=;Timeout=15000;",
        }
    }

    pub fn test_sql(self) -> &'static str {
        if self == Self::Oracle { "SELECT 1 FROM DUAL" } else { "SELECT 1" }
    }

    pub fn begin_sql(self) -> Option<&'static str> {
        match self {
            Self::Postgres | Self::Oracle => Some("SET TRANSACTION READ ONLY"),
            Self::MySql | Self::MariaDb => Some("START TRANSACTION READ ONLY"),
            Self::Sqlite => Some("PRAGMA query_only=ON"),
            // SQL Server has no read-only transaction mode; server permissions must restrict writes.
            Self::SqlServer => None,
        }
    }

    pub fn query_sql(self, sql: &str) -> Result<String, OdbcError> {
        crate::validate_query(sql)?;
        let sql = sql.trim().strip_suffix(';').unwrap_or(sql.trim());
        if matches!(self, Self::MySql | Self::MariaDb) && (sql.contains("/*!") || sql.to_ascii_uppercase().contains("/*M!")) {
            return Err(OdbcError::InvalidQuery);
        }
        let dialect: &dyn Dialect = match self {
            Self::Postgres => &PostgreSqlDialect {},
            Self::SqlServer => &MsSqlDialect {},
            Self::MySql | Self::MariaDb => &MySqlDialect {},
            Self::Oracle => &OracleDialect {},
            Self::Sqlite => &SQLiteDialect {},
        };
        // A conservative token guard, not a SQL sandbox. Database permissions remain authoritative.
        for token in Tokenizer::new(dialect, sql).tokenize().map_err(|_| OdbcError::InvalidQuery)? {
            if let Token::Word(word) = token
                && word.quote_style.is_none()
                && matches!(
                    word.value.to_ascii_uppercase().as_str(),
                    "INSERT"
                        | "UPDATE"
                        | "DELETE"
                        | "MERGE"
                        | "INTO"
                        | "CREATE"
                        | "ALTER"
                        | "DROP"
                        | "TRUNCATE"
                        | "EXEC"
                        | "EXECUTE"
                        | "CALL"
                        | "GRANT"
                        | "REVOKE"
                        | "DENY"
                        | "SET"
                        | "RESET"
                        | "COMMIT"
                        | "ROLLBACK"
                        | "SAVEPOINT"
                        | "RELEASE"
                        | "BEGIN"
                        | "USE"
                        | "GO"
                        | "DBCC"
                        | "BACKUP"
                        | "RESTORE"
                        | "RECONFIGURE"
                        | "SHUTDOWN"
                        | "KILL"
                        | "WAITFOR"
                        | "ATTACH"
                        | "DETACH"
                        | "VACUUM"
                        | "PRAGMA"
                        | "COPY"
                        | "LOAD"
                        | "LOCK"
                        | "UNLOCK"
                )
            {
                return Err(OdbcError::InvalidQuery);
            }
        }
        // psqlODBC bypasses cursor fetching for bare TABLE/VALUES; other dialects retain their syntax.
        Ok(if self == Self::Postgres { format!("SELECT * FROM ({sql}\n) AS gridcraft_odbc_result") } else { sql.to_string() })
    }
}

pub(super) fn attributes(settings: &Settings) -> Result<Vec<(String, String)>, OdbcError> {
    settings.validate()?;
    if let Some(dsn) = &settings.dsn {
        return Ok(vec![("DSN".into(), dsn.clone())]);
    }
    let mut chars = settings.connection_string.as_deref().ok_or(OdbcError::InvalidConnection)?.chars().peekable();
    let mut result = Vec::new();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ';') {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut key = String::new();
        loop {
            match chars.next() {
                Some('=') => break,
                Some(c) if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' ') => key.push(c),
                _ => return Err(OdbcError::InvalidConnection),
            }
        }
        let key = key.trim().to_ascii_uppercase();
        if key.is_empty() || key.len() > 128 || matches!(key.as_str(), "SAVEFILE" | "FILEDSN") || result.len() == 128 {
            return Err(OdbcError::InvalidConnection);
        }
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let value = if chars.peek() == Some(&'{') {
            chars.next();
            let mut value = String::new();
            loop {
                match chars.next() {
                    Some('}') if chars.peek() == Some(&'}') => {
                        chars.next();
                        value.push('}');
                    }
                    Some('}') => break,
                    Some(c) => value.push(c),
                    None => return Err(OdbcError::InvalidConnection),
                }
            }
            while chars.peek().is_some_and(|c| c.is_whitespace()) {
                chars.next();
            }
            if chars.peek().is_some_and(|c| *c != ';') {
                return Err(OdbcError::InvalidConnection);
            }
            value
        } else {
            chars.by_ref().take_while(|c| *c != ';').collect::<String>().trim().to_string()
        };
        result.push((key, value));
    }
    Ok(result)
}

pub(super) fn selector(attrs: &[(String, String)]) -> Result<(&str, &str), OdbcError> {
    let mut selectors = attrs.iter().filter(|(key, _)| matches!(key.as_str(), "DSN" | "DRIVER"));
    let (key, value) = selectors.next().ok_or(OdbcError::InvalidConnection)?;
    if value.is_empty() || selectors.next().is_some() {
        return Err(OdbcError::InvalidConnection);
    }
    if key == "DSN" {
        Settings { dsn: Some(value.clone()), ..Default::default() }.validate()?;
    }
    Ok((key, value))
}

pub(super) fn connection_string(settings: &Settings, attrs: &[(String, String)], driver: Driver) -> Result<String, OdbcError> {
    let escaped = |value: &str| {
        if value.starts_with('{') || value.trim() != value {
            format!("{{{}}}", value.replace('}', "}}"))
        } else {
            odbc_api::escape_attribute_value(value).into_owned()
        }
    };
    let safety = driver.options();
    let mut credentials = String::new();
    if !settings.user.is_empty() {
        credentials.push_str(&format!("UID={};", escaped(&settings.user)));
    }
    if !settings.password.is_empty() {
        credentials.push_str(&format!("PWD={};", escaped(&settings.password)));
    }
    // Cover both first- and last-occurrence resolution, including options read from a DSN.
    let mut value = format!("{safety}{credentials}");
    for (key, entry) in attrs {
        let overridden = safety.split(';').filter_map(|s| s.split_once('=')).any(|(option, _)| option.eq_ignore_ascii_case(key));
        if overridden
            || (!settings.user.is_empty() && matches!(key.as_str(), "UID" | "USER"))
            || (!settings.password.is_empty() && matches!(key.as_str(), "PWD" | "PASSWORD"))
        {
            continue;
        }
        if key == "DSN" {
            value.push_str(&format!("DSN={entry};"));
        } else if key == "DRIVER" {
            value.push_str(&format!("DRIVER={{{}}};", entry.replace('}', "}}")));
        } else {
            value.push_str(&format!("{key}={};", escaped(entry)));
        }
    }
    value.push_str(&credentials);
    value.push_str(safety);
    // SQLDriverConnectW takes a signed 16-bit length; odbc-api panics if it cannot convert it.
    if value.encode_utf16().count() > i16::MAX as usize {
        return Err(OdbcError::InvalidConnection);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_connections_fit_the_odbc_smallint_length() {
        for password in ["x".repeat(16_384), "};".repeat(8192)] {
            let settings = Settings { dsn: Some("Test".into()), password, ..Default::default() };
            assert!(connection_string(&settings, &attributes(&settings).unwrap(), Driver::Postgres).is_err());
        }
        let settings = Settings { dsn: Some("Test".into()), password: "🦊".repeat(4096), ..Default::default() };
        let value = connection_string(&settings, &attributes(&settings).unwrap(), Driver::Postgres).unwrap();
        assert!(value.len() > i16::MAX as usize);
        assert!(value.encode_utf16().count() <= i16::MAX as usize);
    }

    #[test]
    fn credentials_keep_leading_braces_and_edge_spaces() {
        for password in ["{pass}", " pass ", "{odbc};+test"] {
            let settings = Settings { dsn: Some("Test".into()), password: password.into(), ..Default::default() };
            let raw = connection_string(&settings, &attributes(&settings).unwrap(), Driver::Postgres).unwrap();
            let parsed = attributes(&Settings { connection_string: Some(raw), ..Default::default() }).unwrap();
            let passwords: Vec<_> = parsed.iter().filter(|(key, _)| key == "PWD").map(|(_, value)| value.as_str()).collect();
            assert_eq!(passwords, [password, password]);
        }
    }

    #[test]
    fn ordinary_attributes_are_not_changed_into_braced_literals() {
        let settings = Settings {
            connection_string: Some("Driver={PostgreSQL Unicode(x64)};Servername=127.0.0.1;Port=55435;Database=postgres;SSLmode=disable".into()),
            user: "gridcraft_reader".into(),
            password: "test".into(),
            ..Default::default()
        };
        let value = connection_string(&settings, &attributes(&settings).unwrap(), Driver::Postgres).unwrap();
        for expected in ["UID=gridcraft_reader;", "PWD=test;", "SERVERNAME=127.0.0.1;", "PORT=55435;", "DATABASE=postgres;", "SSLMODE=disable;"] {
            assert!(value.contains(expected), "missing {expected}");
        }
        assert!(value.contains("DRIVER={PostgreSQL Unicode(x64)};"));
    }

    #[test]
    fn sqlite_disables_extensions_with_an_unquoted_empty_value() {
        assert!(Driver::Sqlite.options().contains("LoadExt=;"));
    }

    #[test]
    fn oracle_starts_read_only_transactions_without_a_conflicting_driver_mode() {
        let settings = Settings { connection_string: Some("Driver={Oracle in instantclient_23_26};DBA=R".into()), ..Default::default() };
        let value = connection_string(&settings, &attributes(&settings).unwrap(), Driver::Oracle).unwrap();
        assert!(!value.contains("DBA=R;"));
        assert!(value.starts_with("DBA=W;") && value.ends_with(Driver::Oracle.options()));
        assert_eq!(Driver::Oracle.begin_sql(), Some("SET TRANSACTION READ ONLY"));
    }

    #[test]
    fn supported_drivers_have_database_specific_read_settings() {
        for (name, driver, test_sql, begin_sql) in [
            ("PostgreSQL Unicode(x64)", Driver::Postgres, "SELECT 1", Some("SET TRANSACTION READ ONLY")),
            ("ODBC Driver 18 for SQL Server", Driver::SqlServer, "SELECT 1", None),
            ("MySQL ODBC 9.0 Unicode Driver", Driver::MySql, "SELECT 1", Some("START TRANSACTION READ ONLY")),
            ("MariaDB ODBC 3.2 Driver", Driver::MariaDb, "SELECT 1", Some("START TRANSACTION READ ONLY")),
            ("Oracle in instantclient_23_9", Driver::Oracle, "SELECT 1 FROM DUAL", Some("SET TRANSACTION READ ONLY")),
            ("SQLite3 ODBC Driver", Driver::Sqlite, "SELECT 1", Some("PRAGMA query_only=ON")),
        ] {
            assert_eq!(Driver::from_name(name).unwrap(), driver);
            assert_eq!(driver.test_sql(), test_sql);
            assert_eq!(driver.begin_sql(), begin_sql);
        }
        assert!(Driver::from_name("Unknown ODBC Driver").is_err());
        assert!(Driver::MySql.accepts("MariaDB"));
        assert!(Driver::MariaDb.accepts("MySQL"));
        assert!(!Driver::Oracle.accepts("PostgreSQL"));
    }

    #[test]
    fn driver_options_are_isolated_and_credentials_override_aliases() {
        let connection = Settings {
            connection_string: Some(
                "Driver={MySQL ODBC 9.0 Unicode Driver};PASSWORD=old;USER=old;INITSTMT={DELETE FROM t};OPTION=67108864;SSLCA={C:\\certs\\a;b.pem}"
                    .into(),
            ),
            user: "reader".into(),
            password: "{pass};+".into(),
            ..Default::default()
        };
        let attrs = attributes(&connection).unwrap();
        assert_eq!(selector(&attrs).unwrap(), ("DRIVER", "MySQL ODBC 9.0 Unicode Driver"));
        let value = connection_string(&connection, &attrs, Driver::MySql).unwrap();
        assert!(!value.contains("DELETE FROM t"));
        assert!(!value.contains("PASSWORD={old}"));
        assert!(value.contains("SSLCA={C:\\certs\\a;b.pem};"));
        assert!(value.contains("PWD={{pass}};+};"));
        assert!(value.contains("NO_CACHE=1;"));
        assert!(!value.contains("BoolsAsChar"));
        for driver in [Driver::SqlServer, Driver::Oracle, Driver::Sqlite, Driver::MariaDb] {
            assert!(!connection_string(&connection, &attrs, driver).unwrap().contains("BoolsAsChar"));
        }
    }

    #[test]
    fn ambiguous_or_malformed_connection_selectors_are_rejected() {
        for raw in [
            "Driver={SQLite3 ODBC Driver};DSN=Other",
            "DSN=One;dsn=Two",
            "FILEDSN=hidden.dsn",
            "Driver={unclosed",
            "Driver={x}junk",
            "Driver=x;SAVEFILE={hidden.dsn}",
        ] {
            let settings = Settings { connection_string: Some(raw.into()), ..Default::default() };
            assert!(attributes(&settings).and_then(|attrs| selector(&attrs).map(|_| ())).is_err(), "{raw}");
        }
        let attrs = attributes(&Settings { dsn: Some("Reporting".into()), ..Default::default() }).unwrap();
        assert_eq!(selector(&attrs).unwrap(), ("DSN", "Reporting"));
    }

    #[test]
    fn query_guards_preserve_dialects_and_reject_writes() {
        for (driver, sql) in [
            (Driver::SqlServer, "WITH t AS (SELECT 1 AS [value]) SELECT [value] FROM t ORDER BY [value]"),
            (Driver::SqlServer, "SELECT CASE WHEN 1=1 THEN 'DELETE' ELSE 'INTO' END AS [update] /* DELETE */"),
            (Driver::MySql, "SELECT `value`, 'delete' FROM example LIMIT 2"),
            (Driver::Oracle, "SELECT q'[delete]' AS value FROM DUAL"),
            (Driver::Sqlite, "WITH t AS (SELECT 1 AS value) SELECT value FROM t"),
            (Driver::Postgres, "VALUES (1)"),
        ] {
            let prepared = driver.query_sql(sql).unwrap();
            if driver != Driver::Postgres {
                assert_eq!(prepared, sql);
            }
        }
        for (driver, sql) in [
            (Driver::SqlServer, "WITH t AS (SELECT 1 AS n) DELETE FROM example"),
            (Driver::SqlServer, "SELECT 1 INTO copied"),
            (Driver::SqlServer, "SELECT 1 EXEC('DELETE FROM example')"),
            (Driver::MySql, "SELECT 1 INTO OUTFILE '/tmp/result'"),
            (Driver::MariaDb, "SELECT 1 /*!50000 INTO OUTFILE '/tmp/result' */"),
            (Driver::MySql, "SELECT 1 /*M! INTO OUTFILE '/tmp/result' */"),
            (Driver::Sqlite, "WITH t AS (SELECT 1) UPDATE example SET n=1"),
        ] {
            assert!(driver.query_sql(sql).is_err(), "{sql}");
        }
    }
}
