//! Native data access for GridCraft's Get & Transform Data and the WEBSERVICE function.
//!
//! The engine owns the [`DataAccess`] trait and never opens a connection itself; the desktop app
//! and the command line call [`install`] to hand it this implementation, as they hand the UI
//! their clipboard and printers. It adds what `std` can't do portably:
//! - HTTP(S) GET through `ureq` (the system TLS stack and certificate store on Windows and macOS,
//!   rustls elsewhere): http and https only, a timeout per request, a cap on the body size,
//!   redirects followed a bounded number of times;
//! - ODBC on Windows through the driver manager Windows ships, for From ODBC, From SQL Server
//!   Database and From Microsoft Access Database.
//!
//! Files are read by the engine's default (`std::fs`). The browser build never links this crate:
//! there, web and ODBC sources stay disabled with a reason.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::sync::Arc;

#[cfg(windows)]
use gridcraft_engine::query::DbTable;
use gridcraft_engine::query::{DataAccess, HttpRequest, HttpResponse};

#[cfg(not(target_arch = "wasm32"))]
mod http;
#[cfg(windows)]
mod odbc;

/// The native implementation of [`DataAccess`].
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeAccess;

impl DataAccess for NativeAccess {
    fn web_available(&self) -> Result<(), String> {
        if cfg!(target_arch = "wasm32") { Err(gridcraft_engine::query::access::WEB_UNAVAILABLE.into()) } else { Ok(()) }
    }

    fn http_get(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            http::get(request)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = request;
            Err(gridcraft_engine::query::access::WEB_UNAVAILABLE.into())
        }
    }

    #[cfg(windows)]
    fn odbc_available(&self) -> Result<(), String> {
        odbc::available()
    }

    #[cfg(windows)]
    fn odbc_tables(&self, connection: &str) -> Result<Vec<String>, String> {
        odbc::tables(connection)
    }

    #[cfg(windows)]
    fn odbc_table(&self, connection: &str, table: &str, max_rows: usize) -> Result<DbTable, String> {
        odbc::table(connection, table, max_rows)
    }

    #[cfg(windows)]
    fn odbc_query(&self, connection: &str, sql: &str, max_rows: usize) -> Result<DbTable, String> {
        odbc::query(connection, sql, max_rows)
    }
}

/// Gives a session native data access (web and, on Windows, ODBC).
pub fn install(session: &mut gridcraft_engine::Session) {
    session.set_data_access(Arc::new(NativeAccess));
}
