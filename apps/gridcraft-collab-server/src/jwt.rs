//! Checking the signature of the cabinet's session token (ADR-0008), before any claim is trusted.
//!
//! The token travels in the first WebSocket message and nowhere else: not in the URL, not in a
//! header, not in the logs. The key comes from the environment and is never printed.

use gridcraft_collab::{AuthError, Claims};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, errors::ErrorKind};

use crate::config::ConfigError;

/// Why a token was refused. The code is what the connection close frame says; neither the code
/// nor the logs carry the token or the claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// The signature, shape or expiry is not acceptable.
    Token(&'static str),
    /// The signature is fine, but the claims do not fit this server or this session.
    Claims(AuthError),
}

impl VerifyError {
    pub fn code(self) -> &'static str {
        match self {
            VerifyError::Token(code) => code,
            VerifyError::Claims(error) => error.code(),
        }
    }
}

/// The verification key of the server: a shared secret for local runs, or a public key in PEM
/// for the cabinet (ES256 / EdDSA).
#[derive(Clone)]
pub struct JwtConfig {
    algorithm: Algorithm,
    key: DecodingKey,
}

impl std::fmt::Debug for JwtConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key itself is never printed.
        f.debug_struct("JwtConfig").field("algorithm", &self.algorithm).finish_non_exhaustive()
    }
}

impl JwtConfig {
    /// `GRIDCRAFT_COLLAB_JWT_SECRET` (HS256) or `GRIDCRAFT_COLLAB_JWT_PEM` with
    /// `GRIDCRAFT_COLLAB_JWT_ALG` (`ES256`, `ES384`, `EdDSA`; default `EdDSA`).
    pub fn from_env() -> Result<JwtConfig, ConfigError> {
        let secret = std::env::var("GRIDCRAFT_COLLAB_JWT_SECRET").ok().filter(|s| !s.is_empty());
        let pem = std::env::var("GRIDCRAFT_COLLAB_JWT_PEM").ok().filter(|s| !s.is_empty());
        match (secret, pem) {
            (Some(secret), None) => JwtConfig::hs256(secret.as_bytes()),
            (None, Some(pem)) => {
                let alg = match std::env::var("GRIDCRAFT_COLLAB_JWT_ALG").as_deref() {
                    Ok("ES256") => Algorithm::ES256,
                    Ok("ES384") => Algorithm::ES384,
                    Ok("EdDSA") | Ok("") | Err(_) => Algorithm::EdDSA,
                    Ok(_) => return Err(ConfigError("GRIDCRAFT_COLLAB_JWT_ALG is not ES256, ES384 or EdDSA".into())),
                };
                JwtConfig::pem(alg, pem.as_bytes())
            }
            (Some(_), Some(_)) => Err(ConfigError("set either GRIDCRAFT_COLLAB_JWT_SECRET or GRIDCRAFT_COLLAB_JWT_PEM, not both".into())),
            (None, None) => Err(ConfigError("GRIDCRAFT_COLLAB_JWT_SECRET or GRIDCRAFT_COLLAB_JWT_PEM is required".into())),
        }
    }

    /// A shared secret, for local runs and tests.
    pub fn hs256(secret: &[u8]) -> Result<JwtConfig, ConfigError> {
        if secret.is_empty() {
            return Err(ConfigError("the JWT secret is empty".into()));
        }
        Ok(JwtConfig { algorithm: Algorithm::HS256, key: DecodingKey::from_secret(secret) })
    }

    /// A public key in PEM, as the cabinet publishes it.
    pub fn pem(algorithm: Algorithm, pem: &[u8]) -> Result<JwtConfig, ConfigError> {
        let key = match algorithm {
            Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem(pem),
            Algorithm::EdDSA => DecodingKey::from_ed_pem(pem),
            _ => return Err(ConfigError("the PEM key must be ES256, ES384 or EdDSA".into())),
        }
        .map_err(|_| ConfigError("GRIDCRAFT_COLLAB_JWT_PEM is not a PEM public key".into()))?;
        Ok(JwtConfig { algorithm, key })
    }

    /// Verifies the signature and the standard claims (`exp`, `aud`). The application claims —
    /// file, app, session, role — are checked by [`Claims::validate`] against the session.
    pub fn verify(&self, token: &str, audience: &str) -> Result<Claims, VerifyError> {
        let mut validation = Validation::new(self.algorithm);
        validation.set_audience(&[audience]);
        validation.validate_nbf = true;
        let data = decode::<Claims>(token, &self.key, &validation).map_err(|e| match e.kind() {
            ErrorKind::ExpiredSignature => VerifyError::Token("token-expired"),
            ErrorKind::InvalidAudience => VerifyError::Token("token-audience"),
            ErrorKind::ImmatureSignature => VerifyError::Token("token-not-yet-valid"),
            ErrorKind::InvalidAlgorithm => VerifyError::Token("token-algorithm"),
            _ => VerifyError::Token("token-invalid"),
        })?;
        Ok(data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};

    fn claims(exp: u64) -> Claims {
        Claims {
            iss: "mail.lzmkm.ru".into(),
            sub: "ann".into(),
            aud: "office-sync".into(),
            file_id: 7,
            app: "gridcraft".into(),
            role: gridcraft_collab::Role::Editor,
            session_id: "s-1".into(),
            iat: jsonwebtoken::get_current_timestamp(),
            exp,
        }
    }

    fn token(secret: &[u8], claims: &Claims) -> String {
        encode(&Header::new(Algorithm::HS256), claims, &EncodingKey::from_secret(secret)).unwrap()
    }

    fn in_an_hour() -> u64 {
        jsonwebtoken::get_current_timestamp() + 3_600
    }

    #[test]
    fn a_signed_token_yields_its_claims() {
        let config = JwtConfig::hs256(b"local-secret").unwrap();
        let claims = config.verify(&token(b"local-secret", &claims(in_an_hour())), "office-sync").unwrap();
        assert_eq!(claims.sub, "ann");
    }

    #[test]
    fn another_key_or_audience_is_refused() {
        let config = JwtConfig::hs256(b"local-secret").unwrap();
        assert_eq!(config.verify(&token(b"other", &claims(in_an_hour())), "office-sync").unwrap_err().code(), "token-invalid");
        assert_eq!(config.verify(&token(b"local-secret", &claims(in_an_hour())), "elsewhere").unwrap_err().code(), "token-audience");
    }

    #[test]
    fn an_expired_token_is_refused() {
        let config = JwtConfig::hs256(b"local-secret").unwrap();
        assert_eq!(config.verify(&token(b"local-secret", &claims(1_001)), "office-sync").unwrap_err().code(), "token-expired");
    }
}
