//! Who a connection is: the claims of its JWT and what the server checks before it trusts them
//! (ADR-0008).
//!
//! The signature is verified by the transport, before a [`Claims`] value exists: this crate has
//! no crypto and no keys. What remains is the part the room cares about — the claims name the
//! file, the application, the session and the role, and every one of them has to match what the
//! server is serving.

use serde::{Deserialize, Serialize};

use crate::hub::Role;

/// The claims of the cabinet's session token (`INTEGRATION.md` §3.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    #[serde(rename = "fileId")]
    pub file_id: i64,
    pub app: String,
    pub role: Role,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub iat: u64,
    pub exp: u64,
}

/// What this server expects the claims to name.
#[derive(Clone, Copy, Debug)]
pub struct Expected<'a> {
    pub aud: &'a str,
    pub app: &'a str,
    pub file_id: i64,
    pub session_id: &'a str,
}

/// Why a token is not accepted. The transport closes the connection with the code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthError {
    Expired,
    WrongAudience,
    WrongApp,
    WrongFile,
    WrongSession,
    NotYetValid,
}

impl AuthError {
    pub fn code(self) -> &'static str {
        match self {
            AuthError::Expired => "token-expired",
            AuthError::WrongAudience => "token-audience",
            AuthError::WrongApp => "token-app",
            AuthError::WrongFile => "token-file",
            AuthError::WrongSession => "token-session",
            AuthError::NotYetValid => "token-not-yet-valid",
        }
    }
}

impl Claims {
    /// Checks the claims against `expected` at `now` (seconds since the epoch, as the token
    /// carries them) and returns the role the connection may use. The role is re-read from the
    /// token on every connection, never taken from a message.
    pub fn validate(&self, now: u64, expected: Expected<'_>) -> Result<Role, AuthError> {
        if self.exp <= now {
            return Err(AuthError::Expired);
        }
        if self.iat > now + 60 {
            return Err(AuthError::NotYetValid);
        }
        if self.aud != expected.aud {
            return Err(AuthError::WrongAudience);
        }
        if self.app != expected.app {
            return Err(AuthError::WrongApp);
        }
        if self.file_id != expected.file_id {
            return Err(AuthError::WrongFile);
        }
        if self.session_id != expected.session_id {
            return Err(AuthError::WrongSession);
        }
        Ok(self.role)
    }
}
