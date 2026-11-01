//! The web table source for Data ▸ Get Data ▸ From Web.
//!
//! [`provider`] returns the [`WebSource`] a native host installs in
//! `Session::web_source`. It fetches one page, extracts its HTML tables and
//! reduces each to a [`WebTable`]. Native only: the engine and the web build
//! never link it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::sync::Arc;

use gridcraft_engine::webquery::{WebFetch, WebTable};

/// The web source with this build's fetcher.
pub fn provider() -> WebFetch {
    Arc::new(fetch)
}

/// Runs `req`: fetch the URL, extract the tables the page holds.
pub fn fetch(req: &gridcraft_engine::webquery::WebRequest) -> Result<Vec<WebTable>, String> {
    log::info!("data.fromWeb: {} (max {} bytes)", redact(&req.url), req.max_bytes);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = req;
        Err("Web page import is not available in this build.".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let html = http_get(req)?;
        Ok(gridcraft_engine::webquery::extract_tables(&html, req.max_rows, req.max_cols))
    }
}

/// The URL's origin (scheme and host), for logs and messages: userinfo and the path — which
/// can hold a secret segment — never reach the log.
pub fn redact(url: &str) -> String {
    let url = url.trim_start();
    let Some((scheme, rest)) = url.split_once("://") else {
        return "…".into();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    format!("{scheme}://{authority}")
}

/// One bounded GET: https only, no redirects, no cookies; size- and time-capped.
#[cfg(not(target_arch = "wasm32"))]
fn http_get(req: &gridcraft_engine::webquery::WebRequest) -> Result<String, String> {
    use std::io::Read;

    // The same gate every caller applies (https, no userinfo), kept here so the provider never
    // forwards an embedded credential even if a caller skipped its own validation.
    gridcraft_engine::webquery::validate_url(&req.url)?;
    // SSRF: the host must resolve off the user's own network. A spreadsheet command that
    // fetches a URL lets a shared workbook (or an agent) point the app at the machine it runs
    // on \u{2014} localhost, the link-local cloud-metadata endpoint, a private or loopback
    // range \u{2014} and import the response into a cell, a data-exfiltration channel. The
    // resolved addresses are checked, not the name: `https://attacker.example` pointing at
    // `192.168.0.5` is refused, not just a literal `192.168.0.5`. Every resolved address must
    // be a public one; a name with mixed records (one public, one private) is refused too.
    // The passing addresses are kept and handed to the agent as its resolver, so ureq
    // connects only to an address that passed this check and never resolves the name again
    // (a name that flipped to a private address between the two lookups would otherwise reach
    // the private one). Redirects are not followed (max_redirects(0) below), so this is the
    // only place a connection can be made from.
    let (host, port) = host_and_port(&req.url)?;
    let validated: Vec<std::net::SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
        .map_err(|e| format!("The address \u{2018}{host}\u{2019} could not be resolved: {e}"))?
        .filter(|a| {
            let ip = canonical(a.ip());
            !ip.is_loopback() && !ip.is_unspecified() && !is_private_or_shared(&ip)
        })
        .collect();
    if validated.is_empty() {
        return Err(format!("\u{2018}{host}\u{2019} is an address on this machine or its network, and can\u{2019}t be imported from."));
    }
    let agent = ureq::Agent::with_parts(
        ureq::config::Config::builder()
            .max_redirects(0)
            // No proxy, however the environment is set: a proxy resolves the URL's hostname
            // itself, and in that path the resolver (the validated addresses) is not used, so
            // the destination could be an address that never passed the check. The page import
            // always connects directly, or not at all.
            .proxy(None)
            .timeout_global(Some(std::time::Duration::from_secs(gridcraft_engine::webquery::DEFAULT_TIMEOUT_SECS)))
            .build(),
        ureq::unversioned::transport::DefaultConnector::default(),
        FixedResolver(validated),
    );
    let resp = agent
        .get(&req.url)
        .header("user-agent", concat!("GridCraft/", env!("CARGO_PKG_VERSION")))
        .header("accept", "text/html,application/xhtml+xml")
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(code) => format!("The page answered {code} and could not be imported."),
            other => other.to_string(),
        })?;
    // No redirects are followed, so a 3xx arrives as a normal response: name it as one before
    // the content-type gate, or a redirect body would come back as "No tables were found". The
    // target is redacted like every other URL that reaches a message.
    if resp.status().is_redirection() {
        let target = resp.headers().get("location").and_then(|v| v.to_str().ok()).map_or_else(|| "an unspecified address".into(), redact);
        return Err(format!("The page moved to {target}; redirects are not followed."));
    }
    let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    if !content_type.is_empty() && !content_type.contains("html") && !content_type.contains("xml") && !content_type.contains("text") {
        return Err(format!("The address serves {content_type}, not a web page."));
    }
    // Read up to the cap, then check for one more byte: a truncated page can look complete
    // (a whole table before the cutoff) and import wrong data in good conscience.
    let mut reader = resp.into_body().into_reader();
    let mut bytes: Vec<u8> = Vec::new();
    let _ = reader.by_ref().take(req.max_bytes as u64).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let mut extra = [0u8; 1];
    if reader.read(&mut extra).map_err(|e| e.to_string())? != 0 {
        return Err(format!("The page exceeds the {} byte limit.", req.max_bytes));
    }
    let encoding = encoding_from(&content_type, &bytes);
    Ok(encoding.decode(&bytes).0.into_owned())
}

/// The page's encoding, in HTML5's order: a BOM beats everything, then the HTTP header's
/// charset, then the page's own `<meta charset>` (or `<meta http-equiv>` content type) in the
/// first 2 KB — most real pages declare their charset there, not in the header — else UTF-8.
fn encoding_from(content_type: &str, bytes: &[u8]) -> &'static encoding_rs::Encoding {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return encoding_rs::UTF_8;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE;
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE;
    }
    if let Some(c) = charset_from(content_type).and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes())) {
        return c;
    }
    meta_charset(bytes).and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes())).unwrap_or(encoding_rs::UTF_8)
}

/// The first `<meta>` tag's `charset` (or a `charset=` inside one, as `http-equiv` content types
/// hold it), from the head of the page: unquoted and quoted values both parse.
fn meta_charset(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]);
    let lower = head.to_ascii_lowercase();
    for (i, _) in lower.match_indices("<meta") {
        // `<meta` must end the tag name (`<metadata>` is no meta tag).
        match lower.as_bytes().get(i + 5) {
            Some(b' ') | Some(b'\n') | Some(b'\t') | Some(b'/') | Some(b'>') | None => {}
            _ => continue,
        }
        let end = lower[i..].find('>').map(|e| i + e).unwrap_or(lower.len());
        let tag = &lower[i..end];
        let Some(p) = tag.find("charset") else { continue };
        let Some(value) = tag[p + "charset".len()..].trim_start().strip_prefix('=') else { continue };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        let value = value.split(|c: char| c.is_whitespace()).next().unwrap_or("");
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// The URL's host and port, as `to_socket_addrs` takes the pair: the authority without
/// userinfo, an explicit port kept and https's 443 when the URL has none (the `&str` form
/// requires a port, so a bare hostname would fail to resolve). Bracketed IPv6 loses its
/// brackets here, as the pair form wants the bare address; ureq gets the original URL.
fn host_and_port(url: &str) -> Result<(String, u16), String> {
    let authority = url.trim_start().trim_start_matches("https://").split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    if let Some(rest) = host.strip_prefix('[') {
        // [v6]:port or [v6]
        let Some((v6, after)) = rest.split_once(']') else { return Err("The address is not a valid URL.".into()) };
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse::<u16>().map_err(|_| "The address's port is not valid.".to_string())?,
            None if after.is_empty() => 443,
            _ => return Err("The address is not a valid URL.".into()),
        };
        // Bare, without the brackets: `to_socket_addrs` takes the pair form's host as a bare
        // IPv6 address and does not parse a bracketed one. ureq still gets the original URL,
        // brackets and all; this is only for the resolution here.
        return Ok((v6.to_string(), port));
    }
    // host, or host:port. A colon that is not a port separator is a bad address, not a pair.
    match host.split_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            Ok((h.to_string(), p.parse::<u16>().map_err(|_| "The address's port is not valid.".to_string())?))
        }
        Some(_) => Err("The address is not a valid URL.".into()),
        None => Ok((host.to_string(), 443)),
    }
}

/// An address as the refusal rules see it: an IPv4-mapped IPv6 address (`::ffff:127.0.0.1`)
/// is the IPv4 address it stands for, so the IPv4 rules (loopback, link-local, private) cover
/// the mapped form too.
fn canonical(ip: std::net::IpAddr) -> std::net::IpAddr {
    match ip {
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => std::net::IpAddr::V4(v4),
            None => std::net::IpAddr::V6(v6),
        },
        v4 => v4,
    }
}

/// The agent's resolver: the addresses the SSRF gate validated, and nothing else. ureq never
/// resolves the hostname again, so the destination it connects to cannot differ from the
/// checked one.
#[derive(Debug)]
struct FixedResolver(Vec<std::net::SocketAddr>);

impl ureq::unversioned::resolver::Resolver for FixedResolver {
    fn resolve(&self, _uri: &ureq::http::Uri, _config: &ureq::config::Config, _timeout: ureq::unversioned::transport::NextTimeout) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let mut out = <Self as ureq::unversioned::resolver::Resolver>::empty(self);
        for a in &self.0 {
            if out.try_push(*a).is_err() {
                break;
            }
        }
        if out.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(out)
    }
}

/// The private and shared ranges the SSRF gate refuses: RFC 1918 (10/8, 172.16/12, 192.168/16),
/// the CGNAT and benchmarking ranges, the `0.0.0.0/8` "this host" block, and IPv6's `fc00::/7`
/// (unique local) and `fe80::/10` (link-local).
fn is_private_or_shared(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 0 || o[0] == 10 || o[0] == 127 || (o[0] == 169 && o[1] == 254) || (o[0] == 172 && (16..32).contains(&o[1])) || (o[0] == 192 && o[1] == 168) || (o[0] == 100 && (64..128).contains(&o[1])) || (o[0] == 198 && (18..20).contains(&o[1]))
        }
        std::net::IpAddr::V6(v6) => {
            let seg = v6.segments();
            (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80 || v6.is_loopback()
        }
    }
}

/// The `Content-Type` header's `charset` value, if it names one: parameter names match
/// case-insensitively and a quoted value loses its quotes, both of which ureq's own
/// `Body::charset()` leaves as-is (exact lowercase `charset=`, quotes kept), so a quoted or
/// capitalised label would silently fall back to UTF-8.
fn charset_from(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("charset"))
        .map(|(_, value)| value.trim().trim_matches('"').trim_matches('\'').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Parameter names match case-insensitively and quotes are stripped, where ureq's own
    /// lookup would miss both and fall back to UTF-8.
    fn the_charset_parameter_is_parsed_case_insensitively_and_unquoted() {
        assert_eq!(charset_from("text/html; charset=windows-1252").as_deref(), Some("windows-1252"));
        assert_eq!(charset_from("text/html; Charset=ISO-8859-1").as_deref(), Some("ISO-8859-1"));
        assert_eq!(charset_from("text/html; charset=\"windows-1252\"").as_deref(), Some("windows-1252"));
        assert_eq!(charset_from("text/html; charset='utf-8'").as_deref(), Some("utf-8"));
        assert_eq!(charset_from("text/html").as_deref(), None);
        assert_eq!(charset_from("text/html; boundary=none").as_deref(), None);
    }

    #[test]
    /// A quoted windows-1252 label resolves to that encoding, not the UTF-8 fallback.
    fn a_quoted_charset_resolves_to_its_encoding() {
        let c = charset_from("text/html; charset=\"windows-1252\"").unwrap();
        assert_eq!(encoding_rs::Encoding::for_label(c.as_bytes()), Some(encoding_rs::WINDOWS_1252));
    }

    #[test]
    /// A page declares its charset in a `<meta>` tag: quoted, unquoted and `http-equiv` forms
    /// all parse, and the declaration is found only within the page's head.
    fn a_meta_charset_tag_names_the_encoding() {
        assert_eq!(meta_charset(b"<html><head><meta charset=\"windows-1252\"></head>"), Some("windows-1252".into()));
        assert_eq!(meta_charset(b"<meta charset=utf-8>"), Some("utf-8".into()));
        assert_eq!(
            meta_charset(b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=iso-8859-1\">"),
            Some("iso-8859-1".into())
        );
        assert_eq!(meta_charset(b"<html><head><title>x</title></head>"), None);
        assert_eq!(meta_charset(b"<p>charset= is not a tag</p>"), None);
    }

    #[test]
    /// The SSRF gate refuses every address on the user's own machine or network: the loopback
    /// and "this host" blocks, the link-local cloud-metadata range, the RFC 1918 private
    /// ranges, and their IPv6 counterparts. A public address passes.
    fn the_ssrf_gate_refuses_local_and_private_addresses() {
        for ip in ["127.0.0.1", "127.8.8.8", "0.0.0.0", "10.1.2.3", "172.16.0.1", "172.31.255.255", "192.168.0.5", "169.254.169.254", "100.64.0.1", "198.18.0.1", "::1", "fc00::1", "fe80::1"] {
            let addr: std::net::IpAddr = ip.parse().expect("valid address");
            assert!(is_private_or_shared(&addr) || addr.is_loopback() || addr.is_unspecified(), "{ip} must be refused");
        }
        for ip in ["93.184.216.34", "172.32.0.1", "172.15.0.1", "192.169.0.1", "8.8.8.8", "2606:2800:220:1::1"] {
            let addr: std::net::IpAddr = ip.parse().expect("valid address");
            assert!(!is_private_or_shared(&addr) && !addr.is_loopback() && !addr.is_unspecified(), "{ip} must pass");
        }
    }

    #[test]
    /// The host and port a URL is resolved against: the authority without userinfo, an
    /// explicit port kept, https's 443 when the URL has none (a bare hostname must resolve,
    /// not fail for a missing port), and bracketed IPv6 with its port or the default.
    fn the_host_and_port_resolve_a_bare_hostname() {
        assert_eq!(host_and_port("https://example.com/table.html").as_ref().map(|(h, p)| (h.as_str(), *p)), Ok(("example.com", 443)));
        assert_eq!(host_and_port("https://example.com:8443/x").as_ref().map(|(h, p)| (h.as_str(), *p)), Ok(("example.com", 8443)));
        assert_eq!(host_and_port("https://user:pass@example.com/x").as_ref().map(|(h, p)| (h.as_str(), *p)), Ok(("example.com", 443)));
        assert_eq!(host_and_port("https://[2001:db8::1]/x").as_ref().map(|(h, p)| (h.as_str(), *p)), Ok(("2001:db8::1", 443)));
        assert_eq!(host_and_port("https://[2001:db8::1]:9443/x").as_ref().map(|(h, p)| (h.as_str(), *p)), Ok(("2001:db8::1", 9443)));
        assert!(host_and_port("https://2001:db8::1/x").is_err(), "a bare IPv6 with colons is not a host:port pair");
    }

    #[test]
    /// The pair form resolves a literal address with no DNS in play: a bare IPv6 from a
    /// bracketed URL and a hostname:port pair both parse into socket addresses. Hostname
    /// parsing itself is covered by `the_host_and_port_resolve_a_bare_hostname`, without
    /// a lookup; this test stays offline so CI without DNS still runs it.
    fn the_pair_form_resolves_without_dns() {
        let (v6, port) = host_and_port("https://[2001:db8::1]/x").expect("a bracketed URL parses");
        assert_eq!(v6, "2001:db8::1", "the brackets are gone for the resolution");
        let addrs: Vec<_> = std::net::ToSocketAddrs::to_socket_addrs(&(v6.as_str(), port)).expect("the bare pair resolves").collect();
        assert_eq!(addrs, vec!["[2001:db8::1]:443".parse().expect("a valid socket address")]);

        let (host, port) = host_and_port("https://192.0.2.1:8443/x").expect("an IPv4 URL parses");
        assert_eq!((host.as_str(), port), ("192.0.2.1", 8443));
        let addrs: Vec<_> = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port)).expect("the pair resolves").collect();
        assert_eq!(addrs, vec!["192.0.2.1:8443".parse().expect("a valid socket address")]);
    }

    #[test]
    /// An IPv4-mapped IPv6 address is the IPv4 address it stands for, so the IPv4 refusal
    /// rules cover the mapped forms: mapped loopback, private and link-local are refused.
    fn ipv4_mapped_addresses_are_refused_as_their_ipv4_form() {
        for text in ["::ffff:127.0.0.1", "::ffff:10.1.2.3", "::ffff:192.168.0.5", "::ffff:169.254.169.254", "::ffff:0.0.0.0"] {
            let ip: std::net::IpAddr = text.parse().expect("a mapped address parses");
            let c = canonical(ip);
            assert!(c.is_loopback() || c.is_unspecified() || is_private_or_shared(&c), "{text} must be refused");
        }
        // A mapped public address still passes, and a real (non-mapped) IPv6 is untouched.
        let pub_mapped = canonical("::ffff:93.184.216.34".parse::<std::net::IpAddr>().unwrap());
        assert!(!(pub_mapped.is_loopback() || pub_mapped.is_unspecified() || is_private_or_shared(&pub_mapped)), "a mapped public address passes");
        assert!(matches!(canonical("2606:2800:220:1::1".parse::<std::net::IpAddr>().unwrap()), std::net::IpAddr::V6(_)), "a real IPv6 stays IPv6");
    }

    #[test]
    /// The encoding order is the HTML5 one: a BOM beats the header, the header beats the meta
    /// tag, and a page with none of them decodes as UTF-8.
    fn bom_header_and_meta_declare_the_encoding_in_that_order() {
        let bom = [0xEF, 0xBB, 0xBF];
        assert_eq!(encoding_from("text/html; charset=utf-8", &bom), encoding_rs::UTF_8);
        assert_eq!(encoding_from("text/html; charset=utf-8", &[0xFF, 0xFE, 0x00]), encoding_rs::UTF_16LE);
        assert_eq!(
            encoding_from("text/html; charset=windows-1252", b"<meta charset=\"iso-8859-1\">"),
            encoding_rs::WINDOWS_1252
        );
        assert_eq!(encoding_from("text/html", b"<meta charset=\"windows-1252\">"), encoding_rs::WINDOWS_1252);
        assert_eq!(encoding_from("text/html", b"<html>plain</html>"), encoding_rs::UTF_8);
    }
}
