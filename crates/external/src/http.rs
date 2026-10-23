//! HTTP(S) GET for From Web, From OData Feed and WEBSERVICE.

use std::time::Duration;

use gridcraft_engine::query::access::{check_url, url_host};
use gridcraft_engine::query::{HttpRequest, HttpResponse};

/// Most redirects followed.
const MAX_REDIRECTS: u32 = 10;

/// Performs the GET. Only a response's status, content type and body (cut off with an error
/// past `max_bytes`) come back; every failure is a short sentence naming the host.
pub fn get(req: &HttpRequest) -> Result<HttpResponse, String> {
    check_url(&req.url)?;
    let url = req.url.trim();
    let host = url_host(url).to_string();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls())
        .timeout_global(Some(Duration::from_millis(req.timeout_ms.max(1))))
        .max_redirects(MAX_REDIRECTS)
        .http_status_as_error(false)
        .user_agent(concat!("GridCraft/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut request = agent.get(url);
    if let Some(accept) = &req.accept {
        request = request.header("Accept", accept.as_str());
    }
    let mut resp = request.call().map_err(|e| describe(&e, &host))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty());
    let limit = u64::try_from(req.max_bytes).unwrap_or(u64::MAX);
    let body = resp.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| match e {
        ureq::Error::BodyExceedsLimit(_) => format!("The response from {host} is larger than {} MB.", req.max_bytes / (1024 * 1024)),
        e => describe(&e, &host),
    })?;
    Ok(HttpResponse { status, content_type, body })
}

/// The system TLS stack and its certificate store on Windows and macOS; rustls elsewhere.
fn tls() -> ureq::tls::TlsConfig {
    #[cfg(any(windows, target_os = "macos"))]
    {
        ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::Rustls).build()
    }
}

fn describe(e: &ureq::Error, host: &str) -> String {
    match e {
        ureq::Error::Timeout(_) => format!("{host} didn't answer in time."),
        ureq::Error::HostNotFound => format!("Couldn't find the server {host}."),
        ureq::Error::ConnectionFailed => format!("Couldn't connect to {host}."),
        ureq::Error::TooManyRedirects => format!("{host} redirected too many times."),
        ureq::Error::BodyExceedsLimit(_) => format!("The response from {host} is too large."),
        e => format!("Couldn't reach {host}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves `responses` (raw HTTP), one per connection, on a local port; returns the base URL.
    fn serve(responses: Vec<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for r in responses {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(&r);
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    fn response(status: &str, ctype: &str, body: &[u8]) -> Vec<u8> {
        let mut r = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn req(url: &str) -> HttpRequest {
        HttpRequest { url: url.into(), accept: Some("application/json".into()), timeout_ms: 5_000, max_bytes: 1024 }
    }

    #[test]
    fn gets_status_type_and_body_from_a_local_server() {
        let base = serve(vec![
            response("200 OK", "text/csv; charset=utf-8", b"a,b\n1,2\n"),
            response("404 Not Found", "text/html", b"missing"),
            response("200 OK", "text/plain", &[b'x'; 4096]),
        ]);
        let r = get(&req(&format!("{base}/data.csv"))).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.content_type.as_deref(), Some("text/csv"));
        assert_eq!(r.body, b"a,b\n1,2\n");
        let r = get(&req(&format!("{base}/gone"))).unwrap();
        assert_eq!(r.status, 404);
        let e = get(&req(&format!("{base}/big"))).unwrap_err();
        assert!(e.contains("larger") || e.contains("too large"), "{e}");
    }

    #[test]
    fn refuses_other_schemes_and_times_out() {
        assert!(get(&req("file:///etc/passwd")).is_err());
        assert!(get(&req("ftp://example.com/x")).is_err());
        // A server that accepts but never answers.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let _keep = std::thread::spawn(move || {
            let conn = listener.accept();
            std::thread::sleep(Duration::from_secs(3));
            drop(conn);
        });
        let mut r = req(&format!("http://{addr}/slow"));
        r.timeout_ms = 300;
        let e = get(&r).unwrap_err();
        assert!(e.contains("in time") || e.contains("Couldn't"), "{e}");
        // Nothing listens on a closed port.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        assert!(get(&req(&format!("http://{closed}/"))).is_err());
    }

    #[test]
    fn garbage_responses_are_errors_not_panics() {
        let base = serve(vec![b"NOT HTTP AT ALL\r\n\r\n".to_vec(), b"HTTP/1.1 200 OK\r\nContent-Length: 99999999999999999999\r\n\r\nx".to_vec()]);
        let _ = get(&req(&format!("{base}/a")));
        let _ = get(&req(&format!("{base}/b")));
    }
}
