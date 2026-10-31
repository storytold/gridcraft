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
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .max_redirects(0)
            .timeout_global(Some(std::time::Duration::from_secs(gridcraft_engine::webquery::DEFAULT_TIMEOUT_SECS)))
            .build(),
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
