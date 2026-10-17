# Database imports through ODBC

GridCraft provides Windows ODBC adapters for PostgreSQL, SQL Server (including Azure SQL),
MySQL, MariaDB, Oracle and SQLite 3. They share preview, load and manual refresh commands.
Desktop, CLI and MCP use the same engine commands. Other platforms can edit the saved
cells and preserve query definitions; they cannot connect to a local ODBC driver yet.

## Setup and desktop workflow

1. Install a supported ODBC driver for the architecture of GridCraft (usually 64 bit).
   Database server installers do not necessarily install an ODBC driver.
2. In Windows **ODBC Data Sources (64-bit)**, create a User or System DSN
   with your server, database and SSL settings. Use a DSN name of at most 32 characters.
3. Open **Data → From ODBC**, discover/select the DSN and enter your username and password.
   Alternatively, choose connection string and enter a string such as
   `Driver={PostgreSQL Unicode(x64)};Servername=localhost;Port=5432;Database=example;SSLmode=require;`.
   The driver name must match a registered driver. Use separate credential fields.
4. Enter a query, then **Test**, **Preview**, or **Load**. Loading creates a table in a new sheet.
5. Use **Data → Connections** to preview or refresh a query table. **Refresh All** updates query
   tables before refreshing PivotTables. A failed query keeps its previous cells.

The driver is identified from the registered DSN or the `Driver` connection-string attribute,
then checked against the connected DBMS. MySQL and MariaDB drivers can connect to either server.
Ambiguous selectors and file DSNs are rejected. Driver-specific options are isolated by family.

| Database | Driver family | Read behavior / example query |
|---|---|---|
| PostgreSQL | [psqlODBC Unicode](https://odbc.postgresql.org/) | Read-only transaction, 100-row cursor cache; `SELECT 1 AS value` |
| SQL Server / Azure SQL | [Microsoft ODBC Driver for SQL Server](https://learn.microsoft.com/en-us/sql/connect/odbc/download-odbc-driver-for-sql-server) | Read intent and rollback; **SELECT-only account required**; `SELECT 1 AS value` |
| MySQL | [MySQL Connector/ODBC Unicode](https://dev.mysql.com/doc/connector-odbc/en/) | Read-only transaction, forward streaming; `SELECT 1 AS value` |
| MariaDB | [MariaDB Connector/ODBC 3.2+](https://mariadb.com/docs/connectors/mariadb-connector-odbc) | Read-only transaction, forward streaming; `SELECT 1 AS value` |
| Oracle | [Oracle ODBC Driver](https://docs.oracle.com/en/database/oracle/oracle-database/19/adfns/odbc-driver.html) | Server read-only transaction; `SELECT 1 AS value FROM DUAL` |
| SQLite 3 | [Christian Werner SQLite3 ODBC Driver](http://www.ch-werner.de/sqliteodbc/) | Existing database only, step fetching and `PRAGMA query_only=ON`; `SELECT 1 AS value` |

Use an account with only the required SELECT privileges for every server database. SQL Server's
read intent routes workloads; it does not deny writes. ODBC read-only attributes are also hints,
not an authorization boundary ([ODBC specification](https://learn.microsoft.com/en-us/sql/odbc/reference/syntax/sqlsetconnectattr-function)).
GridCraft rejects write/control keywords outside quoted values and identifiers, including CTE
writes and SELECT INTO. This guard does not sandbox database functions or replace server permissions.
Queries execute without autocommit and the connection rolls back after reading. PostgreSQL and
Oracle use `SET TRANSACTION READ ONLY`; MySQL/MariaDB use `START TRANSACTION READ ONLY`;
SQLite uses [query_only](https://sqlite.org/pragma.html#pragma_query_only).
PostgreSQL leaves the driver's `ReadOnly` flag off because psqlODBC otherwise rejects
`SET TRANSACTION READ ONLY` itself. Oracle uses `DBA=W` because the Oracle driver starts a
transaction for `DBA=R`, which conflicts with GridCraft's `SET TRANSACTION READ ONLY`; the
server transaction remains the read-only enforcement boundary for both drivers.

PostgreSQL queries are wrapped as a SELECT so TABLE/VALUES use cursor fetching. Other families
receive the original statement, preserving their CTE, ORDER BY and alias syntax. PostgreSQL
connection SQL and communication/debug logs, and MySQL/MariaDB initialization SQL are disabled.
MySQL query logging is disabled; ODBC manager and other vendor tracing must be disabled separately. MySQL and
MariaDB disable multiple statements, automatic reconnect and external option-file loading.
The byte limits below apply to GridCraft's materialized result; native drivers may allocate
additional memory for their current page and large fields. Drivers are installed separately;
GridCraft does not bundle vendor binaries or configure server accounts.

## Queries, values and limits

Queries must be one `SELECT`, `WITH`, `VALUES` or `TABLE` statement supported by the selected database. One trailing semicolon is
allowed; internal semicolons (including in literals/comments) are currently rejected. Executable
MySQL/MariaDB comments are rejected. Quote identifiers that coincide with blocked write keywords.
Binary and unsupported ODBC column types return an error. Cast them explicitly to text if needed.

| Value | Imported cell |
|---|---|
| SQL NULL | Empty |
| Boolean | Boolean |
| Integer/decimal within 15 significant digits | Number |
| Larger integers/decimals | Exact text, preserving precision |
| Floating point | Number; nonfinite values remain text |
| Text, dates and timestamps | Text supplied by the driver |
| GUID/UUID and SQL Server XML | Text |
| SQL Server time/datetimeoffset | Text supplied by the driver, including offset when provided |

Mapping follows ODBC metadata. A driver reporting SQL BOOLEAN as an integer produces numeric
0/1 cells; GridCraft does not infer boolean types from integer values.

Text beginning with `=` stays literal text. Date/time strings are not converted to spreadsheet
serial dates. ODBC timestamp types may omit timezone information; select an explicit textual
representation when preserving timezone/offset matters. Duplicate or empty column names are
normalized into unique table headers. A query with zero rows still creates headers.
SQLite expressions reported with unknown column metadata are preserved as text.

Preview is capped at 100 rows and 2 MiB. Loads allow 1–100,000 rows (default 100,000),
256 columns, 128 MiB of materialized storage including cell/row overhead, and 1 MiB per cell.
Preview may return `truncated: true`. Loads and refreshes exceeding a row or size limit fail
without applying partial data; restrict the SQL or increase `maxRows`. Login timeout is 15 seconds and statement timeout is 60 seconds,
subject to driver support. Cancellation prevents application immediately, but an in-flight
native call may finish before the worker releases the connection.

The final escaped connection string must fit ODBC's 32,767 UTF-16 code-unit limit, including
driver options and credentials; oversized connections return an error before calling the driver.

## Refresh and persistence

Load and refresh apply through one undoable engine edit. A result cannot apply after the workbook
changes or another document becomes active. Refresh keeps styles and filter criteria, resizes
the table and clears only its previous data area. It rejects changed column names/types,
formulas, merges, spilled arrays, pictures, protected sheets and occupied expansion areas.
If you edited cached values, refresh requires the explicit **Replace edited data** option.
This option does not override formula or adjacent-data protections.
Refresh also refuses to reapply a query filter when another active filter shares its rows.
Reload into a new table to change the stored SQL or row cap.

Query definitions contain the SQL, DSN (when used), row cap, column types and cached-data
fingerprint. Serialized model JSON and a GridCraft XLSX table extension preserve these definitions; Excel does
not refresh this extension. Opening a workbook never executes SQL. SQL and DSN names are
saved in the workbook, so do not embed secrets in SQL.

Passwords and arbitrary connection strings are kept only in memory, are masked in dialogs,
and are omitted from journals, dialog inspection and workbook files. After reopening, reenter
credentials; for a connection-string source, reenter that string too. Up to 64 live source
profiles and 32 terminal jobs are retained per session. Closing a document discards its profiles.

## Engine / MCP

Use MCP `execute_command` or control-channel `engine.execute` with these command IDs:

| Command | Parameters / result |
|---|---|
| `data.odbcSources` | `{}` → `{dataSources, drivers}` |
| `data.odbcTest` | `{dsn \| connectionString, user?, password?}` → `{jobId}` |
| `data.odbcPreview` | Connection fields plus `sql`, optional `maxRows` → `{jobId}` |
| `data.fromOdbc` | Connection fields plus `sql`, optional `maxRows` → `{jobId}` |
| `data.odbcStatus` | `{jobId}` → state, error/row count/column count/truncation |
| `data.odbcApply` | `{jobId, replace?}` → preview data or applied table summary |
| `data.odbcCancel` | `{jobId}` → cancellation request |
| `data.refreshQuery` | `{table, dsn? \| connectionString?, user?, password?, replace?}` → `{jobId}` |
| `data.refreshAll` | Optional connection overrides and `replace` → `{jobId}` when queries exist |

Poll jobs until `ready`, then call `data.odbcApply`. Connection tests finish as `completed`.
Other terminal states are `failed` and `cancelled`. Only one running or ready job is allowed;
apply or cancel a ready result before submitting another. Preview/test also accept `table` to
reuse a query definition and its live profile. Query jobs are not recorded as replay scripts.

Refresh All fetches queries sequentially within a shared 128 MiB budget, applies successful
queries in one edit, then refreshes pivots. The result includes `queriesRefreshed`,
`pivotsRefreshed`, `errors`, `truncated` and `truncatedQueries`. Failed queries keep their cache.
With no query tables, the existing synchronous PivotTable response is unchanged.

## CLI

```powershell
gridcraft-cli run --cmd 'data.fromOdbc={"dsn":"Reporting","sql":"SELECT id, name FROM public.customers","maxRows":1000}' --out imported.xlsx
gridcraft-cli run --in imported.xlsx --cmd 'data.refreshQuery={"table":"Query1"}' --out refreshed.xlsx
```

The CLI polls and applies these jobs before saving `--out`; previews return data directly.
Single jobs time out after five minutes; Refresh All allows time for its bounded query batch.
On timeout the CLI requests cancellation and returns an error. Avoid putting credentials in
shell history; prefer a DSN with suitable authentication or a private parameter source.

## Contributor checks

### Architecture and adding a database family

The native connector in `crates/odbc` owns driver discovery, connection profiles, SQL guards,
read-only transaction setup and ODBC value conversion. Database-specific selectors, options,
test queries and transaction statements belong in `crates/odbc/src/native/profile.rs`; shared
connection and result handling belongs in `crates/odbc/src/native.rs`. Keep vendor settings out
of the UI.

`crates/engine/src/odbc.rs` runs bounded jobs and coordinates cancellation. The commands in
`crates/engine/src/cmd/odbc.rs` apply results through the engine's undoable edits. Desktop UI,
CLI and MCP use those shared engine commands. Source definitions are persisted with workbook
data, while passwords and arbitrary connection strings stay in memory and must be requested
again after reopening.

To add a database family, add its driver profile and dialect-specific safety/read behavior,
cover driver detection, options and result types in the unit/profile tests, then add an
ignored synthetic-database case to `crates/odbc/tests/databases.rs`. Update the supported-driver
table and this guide when behavior or limitations change. Keep native driver dependencies out
of WASM and leave credentials out of saved workbooks and logs.

Portable unit tests cover conversion/limits; deterministic engine/UI/CLI tests cover load,
preview, refresh, cancellation, undo and credential redaction. Native profile tests cover driver
selection, connection-string escaping, isolated options and dialect-aware query guards.
The PostgreSQL integration test requires an installed driver and a synthetic database:

```powershell
$env:GRIDCRAFT_TEST_ODBC_DSN = 'GridCraftTest'
$env:GRIDCRAFT_TEST_ODBC_USER = 'readonly_test_user'
$env:GRIDCRAFT_TEST_ODBC_PASSWORD = '<test password>'
cargo test -p gridcraft-odbc --test postgres -- --ignored
```

`GRIDCRAFT_TEST_ODBC_CONNECTION_STRING` may replace the DSN. The integration test is ignored
by default and never connects to a business database automatically. Local integration results
are recorded below; they do not certify every server/driver combination.

The other adapters have separate ignored integration tests with the same synthetic checks:

```powershell
$env:GRIDCRAFT_TEST_ODBC_SQLSERVER_DSN = 'GridCraftSqlServerTest'
$env:GRIDCRAFT_TEST_ODBC_SQLSERVER_USER = 'readonly_test_user'
$env:GRIDCRAFT_TEST_ODBC_SQLSERVER_PASSWORD = '<test password>'
cargo test -p gridcraft-odbc --test databases sql_server -- --ignored
```

Replace `SQLSERVER` with `MYSQL`, `MARIADB`, `ORACLE` or `SQLITE` and the test filter with
`mysql`, `mariadb`, `oracle` or `sqlite`. Each prefix accepts `_CONNECTION_STRING` instead of
`_DSN`. SQLite requires an existing synthetic database file; the test never creates one.
Tests cover connection, Unicode, literal text, exact large integers, NULLs, empty results,
preview truncation, row overflow and cancellation. They create no server objects.

### Local Windows validation — 2026-10-16

These tests used installed 64-bit drivers and dedicated synthetic databases. PostgreSQL,
MySQL and MariaDB used separate reader accounts. SQL Server used Windows authentication
against a new LocalDB instance. Test servers were stopped and temporary LocalDB instances
and PostgreSQL DSNs were removed afterward.

| Database | Server / driver | Result |
|---|---|---|
| PostgreSQL | PostgreSQL 18.1 / psqlODBC Unicode 18.00 | Passed with both User DSN and connection string; server read-only transaction checked |
| SQL Server | SQL Server 2025 LocalDB 17.0.1000.7 / Microsoft ODBC 18.7.1.1 | Passed with connection string and Windows authentication |
| MySQL | MySQL Community 26.7.0 / Connector ODBC Unicode 26.7.1 | Passed with connection string and SELECT-only account |
| MariaDB | MariaDB 13.0.2 / Connector ODBC 3.2.10 | Passed with connection string and SELECT-only account |
| SQLite | SQLite3 ODBC driver 0.99991 / temporary database file | Passed with User DSN and connection string; DSN extension override checked |
| Oracle | Instant Client ODBC 23.26.3 / Oracle Free 26ai | Passed with connection string and SELECT-only account; server read-only transaction checked |

Real connections exposed Oracle transaction startup and two PostgreSQL issues. Ordinary
connection attributes must not be unconditionally braced, and the driver read-only flag
conflicts with starting a read-only transaction. Regression tests also preserve passwords
containing leading braces, separators
and edge spaces. SQLite requires `LoadExt=`, because `LoadExt={}` blocks connection opening
with driver 0.99991; a dedicated regression covers this wire format. Live PostgreSQL/MySQL/MariaDB tests used a synthetic password containing
braces, a semicolon and a plus sign. Local validation does not cover remote TLS certificate
validation, Azure SQL routing, Kerberos, production permissions or sustained large-result loads.
Oracle's `DBA=R` mode opened a driver transaction before the server read-only transaction;
using `DBA=W` avoids the conflict while retaining the server-side read-only guard. PostgreSQL
also passed separate live runs with `{pass}` and a password with edge spaces.
An end-to-end CLI check imported a synthetic SQLite table through ODBC and saved XLSX;
the saved XML preserved Unicode, literal `=1` text and the exact large integer without formulas.

Driver option references: [MySQL connection parameters](https://dev.mysql.com/doc/connector-odbc/en/connector-odbc-configuration-connection-parameters.html),
[MariaDB connection parameters](https://mariadb.com/docs/connectors/mariadb-connector-odbc/mariadb-connector-odbc-guide),
[Oracle connection parameters](https://docs.oracle.com/en/database/oracle/oracle-database/19/adfns/odbc-driver.html),
[SQL Server time mappings](https://learn.microsoft.com/en-us/sql/relational-databases/native-client-odbc-date-time/data-type-support-for-odbc-date-and-time-improvements).
