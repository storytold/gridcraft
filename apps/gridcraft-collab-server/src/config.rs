//! Process configuration (`docs/collab/server.md`).
//!
//! Everything the server needs to be trusted is read from the environment once, at startup, and
//! is never logged: the JWT key, the cabinet service token and the limits. Missing or malformed
//! values stop the process with a message that does not quote the value.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Why the environment does not describe a usable server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

/// Limits and addresses of one server process (RFC §9).
#[derive(Clone, Debug)]
pub struct Config {
    /// Where the listener binds.
    pub addr: SocketAddr,
    /// The WebSocket URL clients get from `POST /v1/sessions`; defaults to `ws://{addr}`.
    pub public_url: String,
    /// `aud` the cabinet signs into the session token.
    pub audience: String,
    /// The one application this adapter serves.
    pub app: String,
    /// Bearer token the cabinet uses for `POST /v1/sessions` and commit. `None` fails closed.
    pub service_token: Option<String>,
    /// How long a file lease lives without activity (ADR-0007 §1).
    pub lease_ttl: Duration,
    /// How long a connection may stay silent before `hello`.
    pub hello_timeout: Duration,
    /// Largest incoming frame: one operation, RFC §4.1.
    pub max_message: usize,
    /// Largest frame the socket layer buffers.
    pub max_frame: usize,
    /// Participants of one document (v1: 5).
    pub max_participants: usize,
    /// How often a snapshot would be written (reserved; storage lands in PR 4c).
    pub snapshot_ops: u64,
    /// How often a background snapshot runs even without enough operations (RFC §9).
    pub snapshot_interval: Duration,
}

impl Default for Config {
    fn default() -> Config {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 8787));
        Config {
            public_url: format!("ws://{addr}"),
            addr,
            audience: "office-sync".into(),
            app: "gridcraft".into(),
            service_token: None,
            lease_ttl: Duration::from_secs(30 * 60),
            hello_timeout: Duration::from_secs(10),
            max_message: 9 * 1024 * 1024,
            max_frame: 1024 * 1024,
            max_participants: 5,
            snapshot_ops: 500,
            snapshot_interval: Duration::from_secs(5 * 60),
        }
    }
}

impl Config {
    /// Reads the environment. `GRIDCRAFT_COLLAB_ADDR`, `..._PUBLIC_URL`, `..._AUDIENCE`,
    /// `..._APP`, `..._SERVICE_TOKEN`, `..._LEASE_SECS`, `..._HELLO_SECS`,
    /// `..._MAX_PARTICIPANTS`, `..._SNAPSHOT_OPS`, `..._SNAPSHOT_SECS`. The store path is read by
    /// the binary, which also restores the sessions from it.
    pub fn from_env() -> Result<Config, ConfigError> {
        let mut cfg = Config::default();
        if let Some(value) = var("GRIDCRAFT_COLLAB_ADDR")? {
            cfg.addr = value.parse().map_err(|_| ConfigError("GRIDCRAFT_COLLAB_ADDR is not an address".into()))?;
            cfg.public_url = format!("ws://{}", cfg.addr);
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_PUBLIC_URL")? {
            cfg.public_url = value;
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_AUDIENCE")? {
            cfg.audience = value;
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_APP")? {
            cfg.app = value;
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_SERVICE_TOKEN")? {
            cfg.service_token = Some(value);
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_LEASE_SECS")? {
            cfg.lease_ttl = Duration::from_secs(positive(&value, "GRIDCRAFT_COLLAB_LEASE_SECS")?);
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_HELLO_SECS")? {
            cfg.hello_timeout = Duration::from_secs(positive(&value, "GRIDCRAFT_COLLAB_HELLO_SECS")?);
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_SNAPSHOT_OPS")? {
            cfg.snapshot_ops = positive(&value, "GRIDCRAFT_COLLAB_SNAPSHOT_OPS")?;
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_SNAPSHOT_SECS")? {
            cfg.snapshot_interval = Duration::from_secs(positive(&value, "GRIDCRAFT_COLLAB_SNAPSHOT_SECS")?);
        }
        if let Some(value) = var("GRIDCRAFT_COLLAB_MAX_PARTICIPANTS")? {
            let n = positive(&value, "GRIDCRAFT_COLLAB_MAX_PARTICIPANTS")?;
            cfg.max_participants = usize::try_from(n).map_err(|_| ConfigError("GRIDCRAFT_COLLAB_MAX_PARTICIPANTS is too large".into()))?;
        }
        Ok(cfg)
    }
}

fn var(name: &str) -> Result<Option<String>, ConfigError> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => Err(ConfigError(format!("{name} is empty"))),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError(format!("{name} is not valid UTF-8"))),
    }
}

fn positive(value: &str, name: &str) -> Result<u64, ConfigError> {
    match value.parse::<u64>() {
        Ok(0) | Err(_) => Err(ConfigError(format!("{name} is not a positive number"))),
        Ok(n) => Ok(n),
    }
}

#[cfg(test)]
mod tests {
    use super::positive;

    #[test]
    fn limits_are_positive_numbers() {
        assert_eq!(positive("15", "X").unwrap(), 15);
        assert!(positive("0", "X").is_err());
        assert!(positive("-1", "X").is_err());
        assert!(positive("abc", "X").is_err());
    }
}
