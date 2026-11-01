//! How Get & Transform and WEBSERVICE reach data outside the workbook.
//!
//! The engine owns the [`DataAccess`] trait and never opens a network connection or a database
//! itself: the native app layer injects an implementation (`Session::set_data_access`) the way it
//! injects printing and the clipboard. Without one, files are read with `std::fs` (as File › Open
//! already does; the browser has no file system) and the web and ODBC are unavailable with a
//! reason. Every caller goes through [`check_url`] and the size caps below, so an implementation
//! can stay simple and a hostile response can't exhaust memory.

use gridcraft_core::Value;

/// Largest file a source reads.
pub const MAX_FILE_BYTES: usize = 256 * 1024 * 1024;
/// Largest HTTP response body read.
pub const MAX_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;
/// Most files a folder listing returns.
pub const MAX_FOLDER_FILES: usize = 10_000;
/// Deepest subfolder a folder listing descends into.
pub const MAX_FOLDER_DEPTH: usize = 16;
/// How long a source request may take, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// How long one WEBSERVICE call may take, in milliseconds.
pub const WEBSERVICE_TIMEOUT_MS: u64 = 10_000;
/// Longest URL accepted (Excel's WEBSERVICE limit).
pub const MAX_URL_LEN: usize = 2048;
/// Most rows a database query returns.
pub const MAX_DB_ROWS: usize = 1_048_575;

/// Why the web isn't reachable without an injected implementation.
pub const WEB_UNAVAILABLE: &str = "Web sources need the GridCraft desktop app or command line (the browser version can't make the request).";
/// Why ODBC isn't reachable.
pub const ODBC_UNAVAILABLE: &str = "ODBC sources need the GridCraft desktop app or command line on Windows (it uses the ODBC drivers Windows ships).";
/// Why files aren't reachable in the browser.
pub const FILES_UNAVAILABLE: &str = "File sources need the GridCraft desktop app or command line (the browser has no file system to read).";

/// An HTTP GET.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    /// The `Accept` header, when the caller wants a format (OData asks for JSON).
    pub accept: Option<String>,
    pub timeout_ms: u64,
    /// The body is cut off (and the request fails) past this many bytes.
    pub max_bytes: usize,
}

/// An HTTP response. Only a 2xx status reaches callers as `Ok`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// The `Content-Type` header, lower case, parameters dropped (`text/html`).
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// One file of a folder listing (From Folder's columns).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileEntry {
    /// The file name (`sales.csv`).
    pub name: String,
    /// The folder holding it, with a trailing separator, as Power Query shows it.
    pub folder: String,
    /// `.csv`, lower case; empty for none.
    pub extension: String,
    pub size: u64,
    /// Date-time serials (1900 date system, UTC).
    pub accessed: Option<f64>,
    pub modified: Option<f64>,
    pub created: Option<f64>,
    pub hidden: bool,
    pub read_only: bool,
}

impl FileEntry {
    pub fn path(&self) -> String {
        format!("{}{}", self.folder, self.name)
    }
}

/// A database result: column names and rows of values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DbTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

/// External data access, injected by the host. Implementations must apply the request's timeout
/// and byte cap, follow only http/https redirects, and never panic on a hostile response.
pub trait DataAccess: Send + Sync {
    /// Reads a whole file, refusing files larger than `max` bytes.
    fn read_file(&self, path: &str, max: usize) -> Result<Vec<u8>, String> {
        std_read_file(path, max)
    }
    /// Lists a folder's files, subfolders included, at most `max` of them.
    fn list_folder(&self, path: &str, max: usize) -> Result<Vec<FileEntry>, String> {
        std_list_folder(path, max)
    }
    /// `Ok` when HTTP requests can be made here, else why not.
    fn web_available(&self) -> Result<(), String> {
        Err(WEB_UNAVAILABLE.into())
    }
    /// Performs a GET. Errors are short sentences for the user (`Couldn't reach example.com`).
    fn http_get(&self, _request: &HttpRequest) -> Result<HttpResponse, String> {
        Err(WEB_UNAVAILABLE.into())
    }
    /// `Ok` when ODBC connections can be made here, else why not.
    fn odbc_available(&self) -> Result<(), String> {
        Err(ODBC_UNAVAILABLE.into())
    }
    /// The tables (and views) a connection offers.
    fn odbc_tables(&self, _connection: &str) -> Result<Vec<String>, String> {
        Err(ODBC_UNAVAILABLE.into())
    }
    /// All rows of `table` (a name `odbc_tables` listed), at most `max_rows`.
    fn odbc_table(&self, _connection: &str, _table: &str, _max_rows: usize) -> Result<DbTable, String> {
        Err(ODBC_UNAVAILABLE.into())
    }
    /// The result of a SQL statement, at most `max_rows` rows.
    fn odbc_query(&self, _connection: &str, _sql: &str, _max_rows: usize) -> Result<DbTable, String> {
        Err(ODBC_UNAVAILABLE.into())
    }
}

/// The access every session has without an injected one: files through `std::fs` (not in the
/// browser), no web, no ODBC.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdAccess;

impl DataAccess for StdAccess {}

/// Checks a URL before any request: http or https only (case-insensitive scheme), a host, no
/// spaces or control characters, at most [`MAX_URL_LEN`] characters.
pub fn check_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("Enter a URL.".into());
    }
    if url.chars().count() > MAX_URL_LEN {
        return Err("The URL is too long.".into());
    }
    if url.chars().any(|c| c.is_control() || c == ' ') {
        return Err("The URL contains spaces or control characters.".into());
    }
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).ok_or("Only http and https addresses can be used.")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host);
    if host.is_empty() || host.starts_with(':') {
        return Err("The URL has no host name.".into());
    }
    Ok(())
}

/// The host part of a URL, for messages.
pub fn url_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    host.rsplit('@').next().unwrap_or(host)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn std_read_file(path: &str, max: usize) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Couldn't open '{path}': {e}"))?;
    if meta.is_dir() {
        return Err(format!("'{path}' is a folder, not a file."));
    }
    if meta.len() > max as u64 {
        return Err(format!("'{path}' is larger than {} MB.", max / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read '{path}': {e}"))?;
    if bytes.len() > max {
        return Err(format!("'{path}' is larger than {} MB.", max / (1024 * 1024)));
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
pub fn std_read_file(_path: &str, _max: usize) -> Result<Vec<u8>, String> {
    Err(FILES_UNAVAILABLE.into())
}

#[cfg(not(target_arch = "wasm32"))]
fn serial(t: std::io::Result<std::time::SystemTime>) -> Option<f64> {
    let ms = t.ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
    Some(gridcraft_core::date::serial_from_unix_ms(ms as f64))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn std_list_folder(path: &str, max: usize) -> Result<Vec<FileEntry>, String> {
    let root = std::path::Path::new(path);
    if !root.is_dir() {
        return Err(format!("'{path}' isn't a folder that can be read."));
    }
    let mut out = Vec::new();
    // Breadth-first, sorted per folder, so the listing (and the query) is the same every time.
    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<std::fs::DirEntry> = read.flatten().take(max.saturating_mul(2).max(64)).collect();
        entries.sort_by_key(|e| e.file_name());
        let folder = {
            let mut f = dir.to_string_lossy().into_owned();
            if !f.ends_with(std::path::MAIN_SEPARATOR) && !f.ends_with('/') {
                f.push(std::path::MAIN_SEPARATOR);
            }
            f
        };
        for e in entries {
            // Symbolic links are not followed (no loops, nothing outside the folder).
            let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                if depth < MAX_FOLDER_DEPTH {
                    queue.push_back((e.path(), depth + 1));
                }
                continue;
            }
            if out.len() >= max {
                return Ok(out);
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let extension = std::path::Path::new(&name).extension().map(|x| format!(".{}", x.to_string_lossy().to_lowercase())).unwrap_or_default();
            out.push(FileEntry {
                hidden: name.starts_with('.'),
                read_only: meta.permissions().readonly(),
                accessed: serial(meta.accessed()),
                modified: serial(meta.modified()),
                created: serial(meta.created()),
                size: meta.len(),
                extension,
                folder: folder.clone(),
                name,
            });
        }
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
pub fn std_list_folder(_path: &str, _max: usize) -> Result<Vec<FileEntry>, String> {
    Err(FILES_UNAVAILABLE.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_and_https_urls_pass() {
        for ok in ["https://example.com", "http://example.com/a?b=1", "HTTPS://Example.com:8080/x", "http://127.0.0.1:9/feed"] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        for bad in ["", "file:///etc/passwd", "ftp://x", "javascript:alert(1)", "https://", "http:///x", "https://a b", "C:\\data.csv", "https://:80"]
        {
            assert!(check_url(bad).is_err(), "{bad}");
        }
        assert!(check_url(&format!("https://x.com/{}", "a".repeat(MAX_URL_LEN))).is_err());
        assert_eq!(url_host("https://user@example.com:8080/x?y"), "example.com:8080");
    }

    #[test]
    fn default_access_has_no_web_or_odbc() {
        let a = StdAccess;
        assert!(a.web_available().is_err());
        assert!(a.http_get(&HttpRequest { url: "https://x".into(), accept: None, timeout_ms: 1, max_bytes: 1 }).is_err());
        assert!(a.odbc_available().is_err());
        assert!(a.read_file("/definitely/not/here.csv", 10).is_err());
    }
}
