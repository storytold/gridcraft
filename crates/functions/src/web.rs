//! WEBSERVICE and FILTERXML.

use std::fmt::Debug;

use gridcraft_core::{Array, CellError, Value};

use crate::util::{R, text};
use crate::{Arg, Ctx, xml, xpath};

/// Longest URL WEBSERVICE accepts, in characters.
const MAX_URL: usize = 2048;
/// Longest response WEBSERVICE returns, in characters (the most a cell holds).
const MAX_RESPONSE: usize = 32_767;

/// Fetches what WEBSERVICE asks for. The host installs one; without it every WEBSERVICE is `#VALUE!`.
///
/// `get` runs while formulas calculate, so it should return promptly (apply a timeout) and may
/// refuse any URL: `None` means "no answer" and the cell shows `#VALUE!`. By the time it is called
/// the URL is a well-formed `http` or `https` URL of at most 2048 characters. A host that makes real
/// requests decides what a workbook may reach (local addresses, redirects, credentials, size).
pub trait WebFetcher: Debug + Send + Sync {
    /// The response body as text, or `None` when the request fails.
    fn get(&self, url: &str) -> Option<String>;
}

/// Whether `url` is an `http://` or `https://` URL with a host and no spaces or control characters.
fn is_http_url(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        url.get(8..)
    } else if lower.starts_with("http://") {
        url.get(7..)
    } else {
        None
    };
    rest.is_some_and(|r| !r.starts_with(['/', '?', '#']) && !r.is_empty()) && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// WEBSERVICE(url): the text a web service returns for `url`.
pub(crate) fn webservice(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let url = text(a, 0)?;
    if url.chars().take(MAX_URL + 1).count() > MAX_URL || !is_http_url(&url) {
        return Err(CellError::Value);
    }
    let body = c.web_get(&url).ok_or(CellError::Value)?;
    if body.chars().take(MAX_RESPONSE + 1).count() > MAX_RESPONSE {
        return Err(CellError::Value);
    }
    Ok(Value::from(body))
}

/// FILTERXML(xml, xpath): what an XPath expression selects from an XML document. Several matches
/// come back as a column.
pub(crate) fn filterxml(a: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let source = text(a, 0)?;
    let path = text(a, 1)?;
    let dom = xml::parse(&source).ok_or(CellError::Value)?;
    let expr = xpath::parse(&path).ok_or(CellError::Value)?;
    let mut found = xpath::evaluate(&dom, &expr).ok_or(CellError::Value)?;
    match found.len() {
        0 => Err(CellError::Value),
        1 => found.pop().ok_or(CellError::Value),
        n => Array::new(n, 1, found).map(|arr| Value::Array(arr.into())).ok_or(CellError::Value),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use gridcraft_core::{CellError, Value};

    use crate::util::testutil::{ev, t};
    use crate::{Arg, Ctx, call, lookup};

    #[test]
    fn accepts_only_plain_http_urls() {
        for ok in ["http://a", "https://a.test/x?y=1#z", "HTTP://A.TEST", "https://[::1]:8080/", "https://a.test/a%20b"] {
            assert!(super::is_http_url(ok), "{ok}");
        }
        for bad in [
            "",
            "a.test",
            "ftp://a.test",
            "file:///etc/hosts",
            "javascript:alert(1)",
            "http://",
            "https://",
            "http:///x",
            "https:// a.test",
            "http://a\u{0}b",
            "http://a b",
            "httpx://a",
            "//a.test",
            " http://a.test",
            "http://a.test\n",
        ] {
            assert!(!super::is_http_url(bad), "{bad:?}");
        }
        // Slicing near the scheme must not split a multibyte character.
        assert!(!super::is_http_url("httpé://a"));
        assert!(!super::is_http_url("日本語日本語日本語"));
    }

    /// Records the URLs it is asked for and answers with a fixed body.
    struct Recording {
        urls: Mutex<Vec<String>>,
        body: String,
    }

    impl Ctx for Recording {
        fn date_system(&self) -> gridcraft_core::DateSystem {
            gridcraft_core::DateSystem::D1900
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
        fn web_get(&mut self, url: &str) -> Option<String> {
            self.urls.lock().ok()?.push(url.to_string());
            Some(self.body.clone())
        }
    }

    fn fetch(body: &str, url: &str) -> (Value, Vec<String>) {
        let mut c = Recording { urls: Mutex::new(vec![]), body: body.to_string() };
        let spec = lookup("WEBSERVICE").unwrap();
        let v = call(spec, &[Arg::val(url)], &mut c);
        let urls = c.urls.into_inner().unwrap();
        (v, urls)
    }

    #[test]
    fn webservice_asks_the_host_only_for_valid_urls() {
        let (v, urls) = fetch("body", "https://a.test/x");
        assert_eq!(v, Value::from("body"));
        assert_eq!(urls, ["https://a.test/x"]);

        for bad in ["ftp://a.test", "file:///etc/hosts", "", "not a url"] {
            let (v, urls) = fetch("body", bad);
            assert_eq!(v, Value::Error(CellError::Value), "{bad:?}");
            assert!(urls.is_empty(), "{bad:?} must not reach the host");
        }
        let long = format!("https://a.test/{}", "a".repeat(2034));
        assert_eq!(long.chars().count(), 2049);
        assert!(fetch("body", &long).1.is_empty());
        let max = format!("https://a.test/{}", "a".repeat(2033));
        assert_eq!(fetch("body", &max).1.len(), 1);
    }

    #[test]
    fn webservice_caps_the_response() {
        assert_eq!(fetch(&"x".repeat(32_767), "http://a.test").0, Value::from("x".repeat(32_767)));
        assert_eq!(fetch(&"x".repeat(32_768), "http://a.test").0, Value::Error(CellError::Value));
        // Characters, not bytes.
        assert_eq!(fetch(&"é".repeat(32_767), "http://a.test").0, Value::from("é".repeat(32_767)));
        assert_eq!(fetch("", "http://a.test").0, Value::from(""));
    }

    #[test]
    fn webservice_has_no_answer_without_a_host_fetcher() {
        assert_eq!(ev("WEBSERVICE", vec![t("https://a.test")]), Value::Error(CellError::Value));
    }

    #[test]
    fn webservice_propagates_an_error_argument() {
        assert_eq!(ev("WEBSERVICE", vec![crate::util::testutil::e(CellError::NA)]), Value::Error(CellError::NA));
        assert_eq!(ev("WEBSERVICE", vec![crate::util::testutil::n(1.0)]), Value::Error(CellError::Value));
    }

    #[test]
    fn filterxml_returns_a_scalar_or_a_column() {
        assert_eq!(ev("FILTERXML", vec![t("<r><a>x</a></r>"), t("//a")]), Value::from("x"));
        let v = ev("FILTERXML", vec![t("<r><a>1</a><a>y</a></r>"), t("//a")]);
        let Value::Array(arr) = v else { panic!("expected an array, got {v:?}") };
        assert_eq!((arr.rows, arr.cols), (2, 1));
        assert_eq!(arr.data, vec![Value::from(1.0), Value::from("y")]);
    }

    #[test]
    fn filterxml_survives_very_long_operator_chains() {
        let sum = format!("1{}", "+1".repeat(15_000));
        let union = format!("//a{}", "|//a".repeat(15_000));
        for path in [sum, union] {
            assert_eq!(ev("FILTERXML", vec![t("<r><a>1</a></r>"), t(&path)]), Value::Error(CellError::Value));
        }
    }

    #[test]
    fn filterxml_errors() {
        for (xml, path) in [("<r/>", "//a"), ("<r>", "//r"), ("<r/>", "//"), ("", "//r"), ("<r/>", ""), ("<r/>", "1 div 0")] {
            assert_eq!(ev("FILTERXML", vec![t(xml), t(path)]), Value::Error(CellError::Value), "{xml} {path}");
        }
        assert_eq!(ev("FILTERXML", vec![crate::util::testutil::e(CellError::Div0), t("//a")]), Value::Error(CellError::Div0));
        assert_eq!(ev("FILTERXML", vec![t("<a/>"), crate::util::testutil::e(CellError::NA)]), Value::Error(CellError::NA));
    }
}
