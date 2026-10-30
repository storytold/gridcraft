//! TLS for the PostgreSQL driver, with libpq's `sslmode` meanings: `prefer` (the default) and
//! `require` encrypt without checking the server's certificate, `verify-ca` checks it against
//! the trusted roots, and `verify-full` also checks that it names the host.

use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};

/// How much of the server's certificate is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verify {
    /// Encrypt only (libpq `prefer`, `require`).
    None,
    /// Signed by a trusted root (libpq `verify-ca`).
    Ca,
    /// Signed by a trusted root and issued for the host (libpq `verify-full`).
    Full,
}

/// The connection with a `sslmode` that the PostgreSQL client can't parse (`verify-ca`,
/// `verify-full`) turned into `require`, and how much to verify.
pub fn postgres_ssl_mode(connection: &str) -> (String, Verify) {
    let lower = connection.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("sslmode=").map(|i| at + i) {
        at = i + "sslmode=".len();
        if i > 0 && !lower[..i].ends_with([' ', '\t', '?', '&']) {
            continue;
        }
        let end = lower[at..].find([' ', '\t', '&']).map_or(lower.len(), |e| at + e);
        let verify = match &lower[at..end] {
            "verify-full" => Verify::Full,
            "verify-ca" => Verify::Ca,
            _ => return (connection.to_string(), Verify::None),
        };
        return (format!("{}require{}", &connection[..at], &connection[end..]), verify);
    }
    (connection.to_string(), Verify::None)
}

/// A client configuration that verifies as `verify` says, with the system's roots (which hold a
/// company's own CA) and Mozilla's.
pub fn client_config(verify: Verify) -> Result<ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().map_err(|e| format!("TLS: {e}"))?;
    let verifier: Arc<dyn ServerCertVerifier> = match verify {
        Verify::None => Arc::new(EncryptOnly(provider)),
        Verify::Ca | Verify::Full => {
            let webpki = WebPkiServerVerifier::builder_with_provider(Arc::new(roots()), provider).build().map_err(|e| format!("TLS: {e}"))?;
            if verify == Verify::Full { webpki } else { Arc::new(AnyHost(webpki)) }
        }
    };
    Ok(builder.dangerous().with_custom_certificate_verifier(verifier).with_no_client_auth())
}

fn roots() -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    roots
}

/// Accepts any certificate but still checks the handshake signatures, as libpq does for
/// `prefer` and `require`.
#[derive(Debug)]
struct EncryptOnly(Arc<CryptoProvider>);

impl ServerCertVerifier for EncryptOnly {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// `verify-ca`: the chain must lead to a trusted root; the host name is not checked.
#[derive(Debug)]
struct AnyHost(Arc<WebPkiServerVerifier>);

impl ServerCertVerifier for AnyHost {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.0.verify_server_cert(end_entity, intermediates, server_name, ocsp, now) {
            Err(Error::InvalidCertificate(CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. })) => {
                Ok(ServerCertVerified::assertion())
            }
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.0.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.0.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_modes_become_require() {
        assert_eq!(postgres_ssl_mode("postgres://u@h/db"), ("postgres://u@h/db".into(), Verify::None));
        assert_eq!(postgres_ssl_mode("postgres://u@h/db?sslmode=verify-full"), ("postgres://u@h/db?sslmode=require".into(), Verify::Full));
        assert_eq!(postgres_ssl_mode("host=h sslmode=verify-ca user=u"), ("host=h sslmode=require user=u".into(), Verify::Ca));
        assert_eq!(postgres_ssl_mode("postgres://h/db?a=1&sslmode=require"), ("postgres://h/db?a=1&sslmode=require".into(), Verify::None));
        assert_eq!(postgres_ssl_mode("postgres://h/xsslmode=verify-full"), ("postgres://h/xsslmode=verify-full".into(), Verify::None));
    }

    #[test]
    fn configs_build() {
        for v in [Verify::None, Verify::Ca, Verify::Full] {
            assert!(client_config(v).is_ok(), "{v:?}");
        }
    }
}
