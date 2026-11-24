//! Pure URL query parsing and browser document-title formatting for `gridcraft-web`,
//! testable natively without a browser.

use gridcraft_ui_egui::Chrome;

/// Parsed startup flags from `window.location.search`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WebQuery {
    pub webgl: bool,
    pub sample: Option<String>,
    pub chrome: Chrome,
    pub author: Option<String>,
}

impl WebQuery {
    /// Parses `window.location.search` (with or without a leading `?`) using exact parameter
    /// names so unrelated values such as `?author=webgl_user` or `?author=sample` never toggle
    /// renderer or sample flags.
    pub fn parse(search: &str) -> Self {
        let mut out = Self::default();
        let mut seen_webgl = false;
        let mut seen_chrome = false;
        for (key, value) in query_pairs(search) {
            match key.as_str() {
                "webgl" if !seen_webgl => {
                    seen_webgl = true;
                    out.webgl = !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no");
                }
                "sample" if out.sample.is_none() => {
                    out.sample = Some(if value.is_empty() { "sales".to_string() } else { value });
                }
                "chrome" if !seen_chrome => {
                    seen_chrome = true;
                    out.chrome = match value.as_str() {
                        "none" => Chrome { title_bar: false, file_tab: false },
                        "notitlebar" => Chrome { title_bar: false, file_tab: true },
                        "nofiletab" => Chrome { title_bar: true, file_tab: false },
                        _ => Chrome::default(),
                    };
                }
                "author" if out.author.is_none() => {
                    out.author = Some(value);
                }
                _ => {}
            }
        }
        out
    }
}

/// Formats the browser tab `document.title` for the active workbook in standalone mode.
pub fn format_document_title(doc_title: Option<&str>, dirty: bool) -> String {
    match doc_title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(title) if dirty => format!("{title} — Edited — GridCraft"),
        Some(title) => format!("{title} — GridCraft"),
        None => "GridCraft".to_string(),
    }
}

fn query_pairs(search: &str) -> Vec<(String, String)> {
    let trimmed = search.strip_prefix('?').unwrap_or(search);
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((k, v)) => (decode_component(k), decode_component(v)),
            None => (decode_component(part), String::new()),
        })
        .collect()
}

fn decode_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_keys_prevent_substring_false_positives() {
        let q = WebQuery::parse("?author=webgl_sample_user&note=sample");
        assert!(!q.webgl, "webgl inside another parameter value must not force WebGL");
        assert_eq!(q.sample, None, "sample inside another parameter value must not open a sample");
        assert_eq!(q.author.as_deref(), Some("webgl_sample_user"));
    }

    #[test]
    fn webgl_flag_accepts_bare_key_and_respects_falsey_values() {
        assert!(WebQuery::parse("?webgl").webgl);
        assert!(WebQuery::parse("?sample=budget&webgl=1").webgl);
        assert!(WebQuery::parse("?webgl=true").webgl);
        assert!(!WebQuery::parse("?webgl=0").webgl);
        assert!(!WebQuery::parse("?webgl=false").webgl);
        assert!(!WebQuery::parse("?webgl=OFF").webgl);
    }

    #[test]
    fn sample_flag_supports_shorthand_and_named_samples() {
        assert_eq!(WebQuery::parse("?sample").sample.as_deref(), Some("sales"));
        assert_eq!(WebQuery::parse("?sample=").sample.as_deref(), Some("sales"));
        assert_eq!(WebQuery::parse("?sample=budget&webgl").sample.as_deref(), Some("budget"));
    }

    #[test]
    fn chrome_and_percent_encoded_author_parse_cleanly() {
        let none = WebQuery::parse("?chrome=none&author=Ada+Lovelace%20%26%20Co");
        assert!(!none.chrome.title_bar && !none.chrome.file_tab);
        assert_eq!(none.author.as_deref(), Some("Ada Lovelace & Co"));

        let no_title = WebQuery::parse("?chrome=notitlebar");
        assert!(!no_title.chrome.title_bar && no_title.chrome.file_tab);

        let no_file = WebQuery::parse("?chrome=nofiletab");
        assert!(no_file.chrome.title_bar && !no_file.chrome.file_tab);

        let unknown = WebQuery::parse("?chrome=invalid");
        assert!(unknown.chrome.title_bar && unknown.chrome.file_tab);
    }

    #[test]
    fn duplicate_query_keys_are_first_wins() {
        let q = WebQuery::parse("?sample=budget&sample=sales&author=Ada&author=Grace&chrome=none&chrome=notitlebar&webgl=0&webgl=1");
        assert_eq!(q.sample.as_deref(), Some("budget"));
        assert_eq!(q.author.as_deref(), Some("Ada"));
        assert!(!q.chrome.title_bar && !q.chrome.file_tab);
        assert!(!q.webgl);

        let q2 = WebQuery::parse("?sample=&sample=budget&chrome=invalid&chrome=none&webgl=1&webgl=0");
        assert_eq!(q2.sample.as_deref(), Some("sales"));
        assert!(q2.chrome.title_bar && q2.chrome.file_tab);
        assert!(q2.webgl);
    }

    #[test]
    fn format_document_title_reflects_workbook_and_dirty_state() {
        assert_eq!(format_document_title(None, false), "GridCraft");
        assert_eq!(format_document_title(Some("  "), true), "GridCraft");
        assert_eq!(format_document_title(Some("Budget.xlsx"), false), "Budget.xlsx — GridCraft");
        assert_eq!(format_document_title(Some("Budget.xlsx"), true), "Budget.xlsx — Edited — GridCraft");
    }

    #[test]
    fn edge_case_query_strings_and_percent_encoding_decode_safely() {
        let q = WebQuery::parse("sample=budget&&author=a%2Bb%3Dc%zz%4%");
        assert_eq!(q.sample.as_deref(), Some("budget"));
        assert_eq!(q.author.as_deref(), Some("a+b=c%zz%4%"));

        let eq = WebQuery::parse("?author=a=b=c&%63hrome=none");
        assert_eq!(eq.author.as_deref(), Some("a=b=c"));
        assert!(!eq.chrome.title_bar && !eq.chrome.file_tab);

        let bad_utf8 = WebQuery::parse("?author=hi%FFthere");
        assert_eq!(bad_utf8.author.as_deref(), Some("hi\u{FFFD}there"));
    }
}
