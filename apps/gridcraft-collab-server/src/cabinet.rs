//! The cabinet commit callback (`INTEGRATION.md` §4.1/§5.2, mail-frontend M87).
//!
//! The room builds the final `.xlsx` from its authoritative workbook and posts it to the
//! cabinet, which owns the file version, the audit and the antivirus scan. The file never travels
//! through a browser, so a closed tab cannot lose a save. The body is the shared contract:
//!
//! ```json
//! { "fileId", "bytes": "<base64 .xlsx>", "sha256", "version", "expectedVersion"? }
//! ```
//!
//! `version` is the room's document version (the cabinet records it in error text only); the
//! cabinet names the file version in its answer. `expectedVersion` is the etag of the previous
//! accepted commit, so a file changed outside the session is refused with `409` instead of
//! overwritten. The cabinet answers `{ "version", "etag" }`, where `etag` is what the next commit
//! must send as `expectedVersion`.
//!
//! The client is deliberately plain: `http://` only, `Connection: close`, one request per attempt.
//! The server is deployed beside the cabinet behind the ingress (RFC §9), so TLS is the proxy's
//! job; an `https://` endpoint is refused with a message that says so rather than failing the
//! handshake. Neither the token nor the document ever reaches the logs.

use std::fmt;
use std::time::Duration;

use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Deadline for one request (connect, write and the whole answer).
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
/// Attempts for one commit: a lost answer or a busy cabinet is retried with the same body; a
/// `409`/`401`/`413` is not retried at all.
pub const DEFAULT_ATTEMPTS: u32 = 3;
/// A response head bigger than this is not the cabinet talking.
const MAX_HEAD: usize = 64 * 1024;
/// The answer is a small JSON object; anything larger is drained, not buffered.
const MAX_ANSWER: usize = 1 << 20;

/// Where and how the cabinet takes a committed file.
#[derive(Clone)]
pub struct CabinetConfig {
    /// The commit endpoint. `{sessionId}` and `{fileId}` are substituted.
    pub url: String,
    /// The service token the callback is authenticated with. Never logged.
    pub token: String,
    /// Deadline for one request.
    pub timeout: Duration,
    /// Attempts before a network failure is reported.
    pub attempts: u32,
}

impl fmt::Debug for CabinetConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CabinetConfig")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("timeout", &self.timeout)
            .field("attempts", &self.attempts)
            .finish()
    }
}

/// The cabinet's copy of an accepted commit.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Published {
    /// The cabinet's file version.
    pub version: u64,
    /// What the next commit must send as `expectedVersion`.
    pub etag: String,
}

/// Why the cabinet did not take the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// `409`: the cabinet holds another version (edited outside the session). The room must not
    /// overwrite it.
    Conflict(String),
    /// `401`/`403`: the service token is wrong. Retrying cannot help.
    Unauthorized(String),
    /// `413`: the cabinet refused the body size.
    TooLarge(String),
    /// The network or the cabinet failed and every attempt is spent: the file is built but is not
    /// a cabinet version yet. The next commit retries with the same `expectedVersion`.
    Unavailable(String),
    /// Any other answer; retrying would get it again.
    Refused(String),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PublishError::Conflict(message)
            | PublishError::Unauthorized(message)
            | PublishError::TooLarge(message)
            | PublishError::Unavailable(message)
            | PublishError::Refused(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PublishError {}

/// What one HTTP status means for the commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Accepted,
    Conflict,
    Unauthorized,
    TooLarge,
    /// Another attempt with the same body may succeed.
    Retryable,
    Refused,
}

/// Classify the cabinet's status. The named codes are the contract's (`200 {version, etag}`,
/// `409`, `401`, `413`); a proxy's `408`/`429` and the `5xx` family are transient, everything else
/// is a refusal.
pub fn classify(status: u16) -> Answer {
    match status {
        200..=299 => Answer::Accepted,
        401 | 403 => Answer::Unauthorized,
        409 => Answer::Conflict,
        413 => Answer::TooLarge,
        408 | 425 | 429 | 500..=599 => Answer::Retryable,
        _ => Answer::Refused,
    }
}

/// One file to publish.
pub struct Publication<'a> {
    /// Cabinet session the room belongs to (the callback path).
    pub session_id: &'a str,
    /// Cabinet file id: the session's `fileId`.
    pub file_id: &'a str,
    /// The built `.xlsx`.
    pub bytes: &'a [u8],
    /// Hex SHA-256 of `bytes`.
    pub sha256: &'a str,
    /// The room's document version.
    pub version: u64,
    /// The etag the cabinet returned for the previous commit, if any.
    pub expected: Option<&'a str>,
}

/// The cabinet callback client. One per server process; a slow cabinet blocks one commit, not the
/// room.
#[derive(Clone)]
pub struct Cabinet {
    endpoint: String,
    token: String,
    timeout: Duration,
    attempts: u32,
}

impl Cabinet {
    pub fn new(config: &CabinetConfig) -> Cabinet {
        Cabinet { endpoint: config.url.clone(), token: config.token.clone(), timeout: config.timeout, attempts: config.attempts.max(1) }
    }

    /// The endpoint for one room, with `{sessionId}`/`{fileId}` substituted.
    pub fn endpoint_for(&self, session_id: &str, file_id: &str) -> String {
        self.endpoint.replace("{sessionId}", session_id).replace("{fileId}", file_id)
    }

    /// The request body of the contract. Public so a test can compare the retried attempts byte
    /// for byte (the retry must be idempotent).
    pub fn body(publication: &Publication<'_>) -> String {
        let mut body = serde_json::Map::new();
        body.insert("fileId".into(), publication.file_id.into());
        body.insert("bytes".into(), base64::engine::general_purpose::STANDARD.encode(publication.bytes).into());
        body.insert("sha256".into(), publication.sha256.into());
        body.insert("version".into(), publication.version.into());
        if let Some(expected) = publication.expected {
            body.insert("expectedVersion".into(), expected.into());
        }
        serde_json::Value::Object(body).to_string()
    }

    /// Post one committed file, retrying a transient failure with the same body and the same
    /// `expectedVersion`.
    pub async fn publish(&self, publication: &Publication<'_>) -> Result<Published, PublishError> {
        let url = self.endpoint_for(publication.session_id, publication.file_id);
        let body = Cabinet::body(publication);
        let target = Target::parse(&url).map_err(PublishError::Refused)?;
        let mut last = String::new();
        for attempt in 0..self.attempts {
            if attempt > 0 {
                // 200 ms, then 800 ms: short enough for a commit request, long enough for a proxy
                // to come back.
                tokio::time::sleep(Duration::from_millis(200u64 << (attempt - 1))).await;
            }
            match self.post(&target, body.as_bytes()).await {
                Ok((status, answer)) => match classify(status) {
                    Answer::Accepted => return accepted(&answer),
                    Answer::Conflict => {
                        return Err(PublishError::Conflict("the cabinet holds another version of the file".into()));
                    }
                    Answer::Unauthorized => {
                        return Err(PublishError::Unauthorized("the cabinet refused the callback token".into()));
                    }
                    Answer::TooLarge => {
                        return Err(PublishError::TooLarge("the cabinet refused the committed file: too large".into()));
                    }
                    Answer::Retryable => last = format!("the cabinet answered HTTP {status}"),
                    Answer::Refused => return Err(PublishError::Refused(format!("the cabinet answered HTTP {status}"))),
                },
                Err(message) => last = message,
            }
        }
        Err(PublishError::Unavailable(last))
    }

    async fn post(&self, target: &Target, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let mut stream = tokio::time::timeout(self.timeout, TcpStream::connect(&target.authority))
            .await
            .map_err(|_| "the cabinet callback timed out connecting".to_string())?
            .map_err(|err| format!("the cabinet callback could not connect: {err}"))?;
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            target.path,
            target.host_header,
            self.token,
            body.len()
        );
        let sent = async {
            stream.write_all(head.as_bytes()).await?;
            stream.write_all(body).await?;
            stream.flush().await
        };
        tokio::time::timeout(self.timeout, sent)
            .await
            .map_err(|_| "the cabinet callback timed out sending".to_string())?
            .map_err(|err| format!("the cabinet callback could not be sent: {err}"))?;

        let mut response = Vec::new();
        let read = async {
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk).await {
                    Ok(0) => break,
                    Ok(n) => {
                        response.extend_from_slice(&chunk[..n]);
                        if response.len() > MAX_ANSWER {
                            // The answer is a small JSON object; keep draining and bound what is
                            // kept.
                            break;
                        }
                    }
                    Err(err) => {
                        // A peer that closes without `close_notify` is common for
                        // `Connection: close`; the bytes that arrived are still the answer.
                        if response.is_empty() {
                            return Err(err);
                        }
                        break;
                    }
                }
            }
            Ok::<(), std::io::Error>(())
        };
        tokio::time::timeout(self.timeout, read)
            .await
            .map_err(|_| "the cabinet callback timed out waiting for an answer".to_string())?
            .map_err(|err| format!("the cabinet callback got no answer: {err}"))?;
        split_response(&response)
    }
}

/// The cabinet's answer of a `2xx`. The contract names both fields; a body without them is not a
/// stored version the room can compare against.
fn accepted(answer: &[u8]) -> Result<Published, PublishError> {
    let value: serde_json::Value =
        serde_json::from_slice(answer).map_err(|_| PublishError::Refused("the cabinet answered 2xx without a JSON body".into()))?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| PublishError::Refused("the cabinet answered 2xx without a version".into()))?;
    let etag = value
        .get("etag")
        .and_then(serde_json::Value::as_str)
        .filter(|etag| !etag.is_empty())
        .ok_or_else(|| PublishError::Refused("the cabinet answered 2xx without an etag".into()))?
        .to_string();
    Ok(Published { version, etag })
}

/// Status line and body of one HTTP answer (`Content-Length` or chunked).
fn split_response(response: &[u8]) -> Result<(u16, Vec<u8>), String> {
    if response.len() > MAX_HEAD + MAX_ANSWER {
        return Err("the cabinet answer is too large".into());
    }
    let split = response.windows(4).position(|window| window == b"\r\n\r\n").map(|at| at + 4).ok_or("the cabinet sent no HTTP headers")?;
    let head = String::from_utf8_lossy(&response[..split]);
    let status = head.split_whitespace().nth(1).and_then(|status| status.parse::<u16>().ok()).ok_or("the cabinet sent no status line")?;
    let chunked = head.lines().any(|line| {
        let mut parts = line.splitn(2, ':');
        parts.next().is_some_and(|name| name.eq_ignore_ascii_case("transfer-encoding"))
            && parts.next().is_some_and(|value| value.to_ascii_lowercase().contains("chunked"))
    });
    let body = &response[split..];
    if chunked { Ok((status, decode_chunked(body)?)) } else { Ok((status, body.to_vec())) }
}

/// Minimal chunked transfer decoding for the callback's JSON answer.
fn decode_chunked(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = body.windows(2).position(|window| window == b"\r\n").ok_or("a chunk has no size line")?;
        let size = std::str::from_utf8(&body[..line_end])
            .ok()
            .and_then(|line| line.split(';').next())
            .and_then(|size| usize::from_str_radix(size.trim(), 16).ok())
            .ok_or("a chunk size is not hex")?;
        body = &body[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if body.len() < size {
            return Err("a chunk is shorter than its size".into());
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size..];
        if let Some(rest) = body.strip_prefix(b"\r\n") {
            body = rest;
        }
    }
}

#[derive(Clone, Debug)]
struct Target {
    /// `host:port` for the connection.
    authority: String,
    /// The `Host` header (the port is omitted for 80).
    host_header: String,
    path: String,
}

impl Target {
    /// Only `http://`: TLS is terminated by the ingress (`server.md`). Credentials in the URL are
    /// refused.
    fn parse(url: &str) -> Result<Target, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or("the cabinet callback URL must start with http:// (TLS is terminated by the ingress; an https:// endpoint is not supported)")?;
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (rest, "/".to_string()),
        };
        if authority.is_empty() {
            return Err("the cabinet callback URL names no host".into());
        }
        if authority.contains('@') {
            return Err("the cabinet callback URL takes no credentials; the service token goes in the Authorization header".into());
        }
        let (host, port) = split_host_port(authority)?;
        let port = port.unwrap_or(80);
        let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
        let host_header = if port == 80 { if host.contains(':') { format!("[{host}]") } else { host.clone() } } else { authority.clone() };
        Ok(Target { authority, host_header, path })
    }
}

fn split_host_port(authority: &str) -> Result<(String, Option<u16>), String> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, rest) = rest.split_once(']').ok_or("the cabinet callback URL has an unclosed IPv6 address")?;
        let port = match rest.strip_prefix(':') {
            Some(port) => Some(port.parse::<u16>().map_err(|_| "the cabinet callback port is not a number".to_string())?),
            None if rest.is_empty() => None,
            None => return Err("the cabinet callback URL has trailing text after the IPv6 address".into()),
        };
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() => {
                (host, Some(port.parse::<u16>().map_err(|_| "the cabinet callback port is not a number".to_string())?))
            }
            _ => (authority, None),
        }
    };
    if host.is_empty() {
        return Err("the cabinet callback URL names no host".into());
    }
    Ok((host.to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_contract_statuses_are_classified() {
        assert_eq!(classify(200), Answer::Accepted);
        assert_eq!(classify(204), Answer::Accepted);
        assert_eq!(classify(409), Answer::Conflict);
        assert_eq!(classify(401), Answer::Unauthorized);
        assert_eq!(classify(403), Answer::Unauthorized);
        assert_eq!(classify(413), Answer::TooLarge);
        assert_eq!(classify(500), Answer::Retryable);
        assert_eq!(classify(503), Answer::Retryable);
        assert_eq!(classify(429), Answer::Retryable);
        assert_eq!(classify(400), Answer::Refused);
    }

    #[test]
    fn the_body_is_the_cabinet_contract() {
        let publication = Publication { session_id: "s1", file_id: "42", bytes: b"PK\x03\x04", sha256: "abc123", version: 7, expected: None };
        let body: serde_json::Value = serde_json::from_str(&Cabinet::body(&publication)).unwrap();
        assert_eq!(body["fileId"], "42");
        assert_eq!(body["sha256"], "abc123");
        assert_eq!(body["version"], 7);
        assert_eq!(body["bytes"], base64::engine::general_purpose::STANDARD.encode(b"PK\x03\x04"));
        assert!(body.get("expectedVersion").is_none(), "the first commit of a room expects nothing");
        let with_expected = Publication { expected: Some("\"e1\""), ..publication };
        let body: serde_json::Value = serde_json::from_str(&Cabinet::body(&with_expected)).unwrap();
        assert_eq!(body["expectedVersion"], "\"e1\"");
    }

    #[test]
    fn the_endpoint_substitutes_the_session_and_the_file() {
        let config = CabinetConfig {
            url: "http://cabinet:8080/api/v1/internal/office/sessions/{sessionId}/commit?file={fileId}".into(),
            token: "secret".into(),
            timeout: Duration::from_secs(1),
            attempts: 1,
        };
        let cabinet = Cabinet::new(&config);
        assert_eq!(cabinet.endpoint_for("s1", "42"), "http://cabinet:8080/api/v1/internal/office/sessions/s1/commit?file=42");
        assert!(!format!("{config:?}").contains("secret"), "the token never reaches a debug print");
    }

    #[test]
    fn the_target_parses_http_only() {
        let target = Target::parse("http://127.0.0.1:8787/v1/x").unwrap();
        assert_eq!(target.authority, "127.0.0.1:8787");
        assert_eq!(target.path, "/v1/x");
        assert!(Target::parse("https://cabinet/x").is_err(), "TLS is the ingress's job");
        assert!(Target::parse("http://user:pass@cabinet/x").is_err(), "no credentials in the URL");
    }

    #[test]
    fn an_answer_without_both_fields_is_not_stored() {
        assert!(accepted(br#"{"version":3}"#).is_err());
        assert!(accepted(br#"{"etag":"e"}"#).is_err());
        assert!(accepted(br#"{"version":3,"etag":""}"#).is_err());
        assert_eq!(accepted(br#"{"version":3,"etag":"e"}"#).unwrap(), Published { version: 3, etag: "e".into() });
    }

    #[test]
    fn a_chunked_answer_is_decoded() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n";
        let (status, body) = split_response(response).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"{\"a\":1}");
    }

    #[tokio::test]
    async fn a_409_is_a_conflict_and_is_not_retried() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let seen = attempts.clone();
        tokio::spawn(async move {
            for _ in 0..4 {
                let Ok((mut stream, _)) = listener.accept().await else { return };
                seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer).await;
                let _ = stream.write_all(b"HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\n\r\n{}").await;
            }
        });
        let cabinet = Cabinet::new(&CabinetConfig {
            url: format!("http://{address}/commit"),
            token: "secret".into(),
            timeout: Duration::from_secs(2),
            attempts: 3,
        });
        let publication = Publication { session_id: "s", file_id: "1", bytes: b"x", sha256: "h", version: 1, expected: None };
        assert!(matches!(cabinet.publish(&publication).await, Err(PublishError::Conflict(_))));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::Relaxed), 1, "409 is not retried");
    }

    #[tokio::test]
    async fn a_lost_answer_is_retried_with_the_same_body() {
        use std::sync::Mutex;
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = std::sync::Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        tokio::spawn(async move {
            for attempt in 0..2 {
                let Ok((mut stream, _)) = listener.accept().await else { return };
                let mut buffer = vec![0u8; 8192];
                let n = stream.read(&mut buffer).await.unwrap_or(0);
                if let Ok(mut requests) = seen.lock() {
                    requests.push(String::from_utf8_lossy(&buffer[..n]).to_string());
                }
                if attempt == 1 {
                    let body = br#"{"version":5,"etag":"e5"}"#;
                    let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await;
                    let _ = stream.write_all(body).await;
                }
            }
        });
        let cabinet = Cabinet::new(&CabinetConfig {
            url: format!("http://{address}/commit"),
            token: "secret".into(),
            timeout: Duration::from_secs(2),
            attempts: 2,
        });
        let publication = Publication { session_id: "s", file_id: "1", bytes: b"x", sha256: "h", version: 1, expected: Some("e0") };
        let published = cabinet.publish(&publication).await.unwrap();
        assert_eq!(published, Published { version: 5, etag: "e5".into() });
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let body_of = |request: &str| request.split_once("\r\n\r\n").map(|(_, body)| body.to_string()).unwrap_or_default();
        assert_eq!(body_of(&requests[0]), body_of(&requests[1]), "a retry is the same body, so it cannot become a second version");
    }
}
