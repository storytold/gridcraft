//! The control channel's shared secret (#2174).
//!
//! A loopback TCP port is reachable by every process of every user on the machine (and by more
//! peers under WSL2 mirrored networking or a shared container network). So the desktop app
//! (`gridcraft --control PORT`) only serves a connection whose first line is
//! `{"auth": "<token>"}`. The token is `GRIDCRAFT_CONTROL_TOKEN` when the user set one, else
//! 256 random bits the app makes at start and writes to a file only the user can read
//! ([`token_path`]). [`Remote`](crate::Remote) looks in the same two places.
//!
//! This module holds what both ends need: the file's location, the handshake line and its check.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Environment variable with a token the user chose. The app uses it instead of making one (and
/// writes no file); clients use it instead of reading the file.
pub const TOKEN_ENV: &str = "GRIDCRAFT_CONTROL_TOKEN";
/// Shortest token the app accepts from [`TOKEN_ENV`] (a made one has 64 hex digits).
pub const MIN_TOKEN_LEN: usize = 16;
/// Longest token accepted anywhere: bounds the handshake line.
pub const MAX_TOKEN_LEN: usize = 1024;
/// Longest first line the app reads from a peer that has not authenticated yet.
pub const MAX_AUTH_LINE: usize = 4096;
/// The single reply every failed handshake gets, whatever was wrong, so it tells a stranger nothing.
pub const REFUSED: &str =
    "authentication required: the first line must be {\"auth\": \"<token>\"} (see docs/control-protocol.md); closing the connection";

/// The settings folder of the desktop app (the one holding `ui.json`).
fn config_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        Some(var("HOME")?.join("Library/Application Support/GridCraft"))
    } else if cfg!(windows) {
        Some(var("APPDATA")?.join("GridCraft"))
    } else {
        Some(var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))?.join("gridcraft"))
    }
}

/// Where the app writes the token it made for `port`, and where clients read it:
/// `control/<port>.token` in the settings folder. `None` when the settings folder is unknown.
pub fn token_path(port: u16) -> Option<PathBuf> {
    Some(config_dir()?.join("control").join(format!("{port}.token")))
}

/// A token as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len().saturating_mul(2));
    for b in bytes {
        for nibble in [b >> 4, b & 0x0f] {
            if let Some(&d) = DIGITS.get(usize::from(nibble)) {
                s.push(char::from(d));
            }
        }
    }
    s
}

/// Equality that looks at every byte, so the time taken says nothing about where two secrets
/// differ. Lengths may differ quickly: a token's length is not secret.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

/// Checks a token the user chose (trimmed): `Err` explains why it can't be used.
pub fn validate_chosen(token: &str) -> Result<String, String> {
    let t = token.trim();
    let n = t.chars().count();
    if n < MIN_TOKEN_LEN {
        return Err(format!("{TOKEN_ENV} must have at least {MIN_TOKEN_LEN} characters"));
    }
    if t.len() > MAX_TOKEN_LEN {
        return Err(format!("{TOKEN_ENV} must have at most {MAX_TOKEN_LEN} bytes"));
    }
    Ok(t.to_string())
}

/// The line a client sends first on every connection.
pub fn auth_line(token: &str) -> String {
    json!({"auth": token}).to_string()
}

/// Why a handshake line was refused. It is for the app's own logs and tests: the peer only ever
/// gets [`REFUSED`], whatever the reason, so a stranger learns nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// The app has no token (an empty token never authenticates).
    NoToken,
    /// The line is longer than [`MAX_AUTH_LINE`].
    TooLong,
    /// The line is not UTF-8 JSON.
    Malformed,
    /// The `auth` field is missing, not a string, or not the token.
    WrongToken,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoToken => "the control channel has no token",
            Self::TooLong => "the handshake line is too long",
            Self::Malformed => "the handshake line is not JSON",
            Self::WrongToken => "the handshake has no token or the wrong one",
        })
    }
}

impl std::error::Error for AuthError {}

/// Checks a connection's first line against `token`. `Ok` is the reply to send before serving
/// requests; `Err` means the caller replies [`REFUSED`] and closes the connection.
pub fn check_auth_line(line: &[u8], token: &str) -> Result<Value, AuthError> {
    if token.is_empty() {
        return Err(AuthError::NoToken);
    }
    if line.len() > MAX_AUTH_LINE {
        return Err(AuthError::TooLong);
    }
    let msg: Value = std::str::from_utf8(line).ok().and_then(|t| serde_json::from_str(t.trim()).ok()).ok_or(AuthError::Malformed)?;
    let given = msg.get("auth").and_then(Value::as_str).unwrap_or("");
    // Compare even when nothing was given, so a missing token costs what a wrong one does.
    if constant_time_eq(given.as_bytes(), token.as_bytes()) {
        let mut reply = json!({"ok": true, "result": {"authenticated": true}});
        if let (Some(id), Some(o)) = (msg.get("id"), reply.as_object_mut()) {
            o.insert("id".into(), id.clone());
        }
        Ok(reply)
    } else {
        Err(AuthError::WrongToken)
    }
}

/// The port of a control address (`127.0.0.1:7979` → 7979).
fn port_of(addr: &str) -> Option<u16> {
    addr.rsplit_once(':')?.1.parse().ok()
}

/// The token a client uses: `env` (the value of [`TOKEN_ENV`]) when set, else the contents of
/// `file`. `Err` says where to find one.
pub fn pick_client_token(env: Option<String>, file: Option<&Path>) -> Result<String, String> {
    if let Some(t) = env.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
        return Ok(t);
    }
    let Some(path) = file else {
        return Err(format!("no control token: set {TOKEN_ENV} (the settings folder is unknown)"));
    };
    match std::fs::read_to_string(path) {
        Ok(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        _ => Err(format!(
            "no control token: set {TOKEN_ENV}, or start the app with `gridcraft --control PORT` as this user on this machine (it writes {})",
            path.display()
        )),
    }
}

/// The token a client uses for `addr`: [`TOKEN_ENV`], else the file the app wrote for its port.
pub fn client_token(addr: &str) -> Result<String, String> {
    let file = port_of(addr).and_then(token_path);
    pick_client_token(std::env::var(TOKEN_ENV).ok(), file.as_deref())
}

/// Writes `token` to `path` so only the user can read it: the folder is created (mode 0700 on
/// Unix), a leftover file is removed rather than reused (it may have looser permissions, or be a
/// link), and the new file is created with mode 0600. On Windows the file inherits the settings
/// folder's ACL, which grants the user (and administrators and SYSTEM) only.
pub fn write_token_file(path: &Path, token: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| std::io::Error::other("the token path has no folder"))?;
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(token.as_bytes())?;
    f.write_all(b"\n")?;
    f.flush()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    const TOKEN: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";

    #[test]
    fn only_the_exact_token_passes_the_handshake() {
        let ok = check_auth_line(auth_line(TOKEN).as_bytes(), TOKEN).unwrap();
        assert_eq!(ok["ok"], true);
        let with_id = check_auth_line(format!("{{\"id\":7,\"auth\":\"{TOKEN}\"}}\r\n").as_bytes(), TOKEN).unwrap();
        assert_eq!(with_id["id"], 7);
        let near = format!("{}1", &TOKEN[..TOKEN.len() - 1]);
        assert_ne!(near, TOKEN);
        for bad in [
            auth_line(&near),
            auth_line(""),
            auth_line(&format!("{TOKEN}x")),
            format!("{{\"token\":\"{TOKEN}\"}}"),
            format!("{{\"auth\":[\"{TOKEN}\"]}}"),
            "{\"id\":1,\"method\":\"app.save\",\"params\":{\"path\":\"x.xlsx\"}}".into(),
            "GET / HTTP/1.1".into(),
            String::new(),
        ] {
            assert!(check_auth_line(bad.as_bytes(), TOKEN).is_err(), "{bad}");
        }
        assert!(check_auth_line(b"\xff\xfe", TOKEN).is_err());
        assert!(check_auth_line(auth_line("").as_bytes(), "").is_err(), "an empty token never authenticates");
        let huge = auth_line(&"x".repeat(MAX_AUTH_LINE));
        assert!(check_auth_line(huge.as_bytes(), TOKEN).is_err());
    }

    #[test]
    fn helpers() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert_eq!(hex(&[0x00, 0x7f, 0xff]), "007fff");
        assert_eq!(port_of("127.0.0.1:7979"), Some(7979));
        assert_eq!(port_of("localhost"), None);
        assert!(validate_chosen("short").is_err());
        assert!(validate_chosen(&"x".repeat(MAX_TOKEN_LEN + 1)).is_err());
        assert_eq!(validate_chosen(&format!("  {TOKEN}\n")).unwrap(), TOKEN);
        assert!(token_path(7979).is_none_or(|p| p.ends_with("control/7979.token") || p.ends_with("control\\7979.token")));
    }

    #[test]
    fn the_token_file_is_private_and_replaced_and_read_back() {
        let dir = std::env::temp_dir().join(format!("gridcraft-control-auth-{}", std::process::id()));
        let path = dir.join("control").join("7979.token");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "stale").unwrap();
        write_token_file(&path, TOKEN).unwrap();
        assert_eq!(pick_client_token(None, Some(&path)).unwrap(), TOKEN);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        // The environment wins over the file; a missing file explains itself.
        assert_eq!(pick_client_token(Some(" from-env-0123456789 ".into()), Some(&path)).unwrap(), "from-env-0123456789");
        let _ = std::fs::remove_dir_all(&dir);
        let missing = pick_client_token(None, Some(&path)).unwrap_err();
        assert!(missing.contains(TOKEN_ENV) && missing.contains("7979.token"), "{missing}");
        assert!(pick_client_token(Some("  ".into()), None).is_err());
    }
}
