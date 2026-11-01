//! Which hyperlink targets may be opened. A cell's link comes from the document (often a file
//! somebody sent), so it is hostile input: only `http`, `https` and `mailto` ever leave the
//! workbook for the system; `file:`, `smb:`, UNC paths, custom URL handlers, `javascript:` and
//! the like are refused. Everything without a scheme stays an in-workbook reference.

use gridcraft_core::RangeRef;

/// What a link target is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// A cell, range, name or table of the workbook (`Sheet2!A1`, `Total`).
    Internal(String),
    /// An address to hand to the system, trimmed: `http`, `https` or `mailto` only.
    External(String),
    /// A document on this computer, as an absolute path: a PDF, picture, text or office file
    /// that the system opens with its default application. Programs, scripts and shortcuts are
    /// never local documents ([`LOCAL_DOCUMENTS`]).
    LocalFile(String),
    /// Refused, with the reason to show.
    Refused(String),
}

/// File types a link may open on this computer: documents only, never anything that runs.
pub const LOCAL_DOCUMENTS: [&str; 20] = [
    "pdf", "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp", "txt", "csv", "xlsx", "xls", "ods", "docx", "doc", "odt", "pptx", "odp", "rtf",
];

const ALLOWED: [&str; 3] = ["http", "https", "mailto"];

/// Classifies a link target. Never panics; the single place that decides what may be opened.
pub fn classify(target: &str) -> LinkKind {
    let t = target.trim();
    if t.chars().any(char::is_control) {
        return LinkKind::Refused("The link contains control characters.".into());
    }
    if t.starts_with("\\\\") || t.starts_with("//") {
        return LinkKind::Refused("Links to network locations aren't opened.".into());
    }
    if RangeRef::parse(t).is_some() {
        return LinkKind::Internal(t.to_string());
    }
    let Some((scheme, rest)) = t.split_once(':') else { return LinkKind::Internal(t.to_string()) };
    let scheme_like = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !scheme_like {
        return LinkKind::Internal(t.to_string());
    }
    let lower = scheme.to_ascii_lowercase();
    if ALLOWED.contains(&lower.as_str()) && !rest.trim().is_empty() {
        LinkKind::External(t.to_string())
    } else {
        LinkKind::Refused(format!("Links of type \"{lower}:\" aren't opened. Only http, https and mailto links are allowed."))
    }
}

/// Classifies a link target found in a workbook saved at `workbook_path` (if any): like
/// [`classify`], but a path to a document on this computer (`Receipts\\a.pdf`, `C:\\Docs\\a.pdf`,
/// `file:///C:/Docs/a.pdf`) becomes [`LinkKind::LocalFile`] with the absolute path, relative paths
/// resolved against the workbook's folder as Excel does. Network paths stay refused, and so does
/// every file type outside [`LOCAL_DOCUMENTS`]. Never panics.
pub fn classify_in(target: &str, workbook_path: Option<&str>) -> LinkKind {
    let t = target.trim();
    if t.chars().any(char::is_control) {
        return LinkKind::Refused("The link contains control characters.".into());
    }
    let Some(path) = local_path(t) else { return classify(t) };
    if path.chars().any(char::is_control) {
        return LinkKind::Refused("The link contains control characters.".into());
    }
    if is_network(&path) {
        return LinkKind::Refused("Links to network locations aren't opened.".into());
    }
    let ext = std::path::Path::new(&path).extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase).unwrap_or_default();
    if !LOCAL_DOCUMENTS.contains(&ext.as_str()) {
        return LinkKind::Refused(format!("Links to \".{ext}\" files aren't opened. Only documents (PDF, pictures, text and office files) are."));
    }
    if is_absolute(&path) {
        return LinkKind::LocalFile(path);
    }
    let Some(dir) = workbook_path.map(std::path::Path::new).and_then(std::path::Path::parent).filter(|d| !d.as_os_str().is_empty()) else {
        return LinkKind::Refused("Save the workbook first: links to files are found next to it.".into());
    };
    let rel: std::path::PathBuf = path.split(['/', '\\']).filter(|part| !part.is_empty() && *part != ".").collect();
    LinkKind::LocalFile(dir.join(rel).to_string_lossy().into_owned())
}

/// Whether `path` is an absolute path to a document [`classify_in`] would open: the last check
/// before a path is handed to the system.
pub fn is_local_document(path: &str) -> bool {
    !path.chars().any(char::is_control)
        && is_absolute(path)
        && !is_network(path)
        && std::path::Path::new(path)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|e| LOCAL_DOCUMENTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// A network location (`\\\\host\\share`, `//host/share`, or any mix of the two separators,
/// which Windows treats alike).
fn is_network(p: &str) -> bool {
    p.replace('\\', "/").starts_with("//")
}

/// `C:\\x`, `C:/x` or `/x`.
fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/') || (b.len() > 2 && b.first().is_some_and(u8::is_ascii_alphabetic) && b.get(1) == Some(&b':') && matches!(b.get(2), Some(b'\\' | b'/')))
}

/// The file path a target names, or `None` when it is no file path (a URL, a reference, a name).
/// `file:` URLs and `%20` escapes are decoded.
fn local_path(t: &str) -> Option<String> {
    let lower = t.to_ascii_lowercase();
    let raw = if let Some(rest) = lower.strip_prefix("file:") {
        let rest = t.get(t.len() - rest.len()..)?;
        // file://host/share is a network location; file:///… and file:/… are this computer.
        if rest.starts_with("//") && !rest.starts_with("///") {
            return Some(format!("//{}", rest.trim_start_matches('/')));
        }
        let rest = rest.trim_start_matches('/');
        // file:///C:/x -> C:/x ; file:///home/x -> /home/x
        if rest.as_bytes().get(1) == Some(&b':') { rest.to_string() } else { format!("/{rest}") }
    } else if is_absolute(t) || t.starts_with("\\\\") || t.starts_with("//") {
        t.to_string()
    } else {
        if t.starts_with('#') || t.contains('!') || RangeRef::parse(t).is_some() {
            return None;
        }
        if let Some((scheme, _)) = t.split_once(':')
            && !scheme.is_empty()
        {
            return None;
        }
        let ext = std::path::Path::new(t).extension().and_then(std::ffi::OsStr::to_str)?;
        if ext.is_empty() || ext.len() > 5 {
            return None;
        }
        t.to_string()
    };
    Some(percent_decode(&raw))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%'
            && let Some(h) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(h);
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_https_and_mailto_leave_the_workbook() {
        for ok in ["http://a.example/x", "HTTPS://A.example", "  https://a.example  ", "MailTo:a@b.example"] {
            assert!(matches!(classify(ok), LinkKind::External(_)), "{ok}");
        }
        assert_eq!(classify(" https://a.example "), LinkKind::External("https://a.example".into()));
        for bad in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "file:///etc/passwd",
            "smb://host/share",
            "ms-excel:ofe|u|http://x",
            "\\\\host\\share\\x.exe",
            "//host/share",
            "C:\\Windows\\calc.exe",
            "\tjavascript:1",
            "java\nscript:1",
            "\u{1}http://a.example",
            "http:",
            "mailto:",
            "x-custom://a",
        ] {
            assert!(matches!(classify(bad), LinkKind::Refused(_)), "{bad:?} -> {:?}", classify(bad));
        }
    }

    #[test]
    fn documents_next_to_the_workbook_open() {
        let base = Some("/home/me/Finance/book.xlsx");
        assert_eq!(classify_in("Receipts/a b.pdf", base), LinkKind::LocalFile("/home/me/Finance/Receipts/a b.pdf".into()));
        assert_eq!(classify_in("Receipts%20x/a%20b.pdf", base), LinkKind::LocalFile("/home/me/Finance/Receipts x/a b.pdf".into()));
        assert_eq!(classify_in("C:\\Docs\\a.PDF", None), LinkKind::LocalFile("C:\\Docs\\a.PDF".into()));
        assert_eq!(classify_in("file:///C:/Docs/a.pdf", None), LinkKind::LocalFile("C:/Docs/a.pdf".into()));
        assert!(matches!(classify_in("a.pdf", None), LinkKind::Refused(_)));
        for bad in [
            "calc.exe",
            "C:\\Windows\\calc.exe",
            "x.bat",
            "x.lnk",
            "\\\\host\\share\\a.pdf",
            "//host/a.pdf",
            "file://host/share/a.pdf",
            "run.ps1",
            "a.pdf\u{1}",
            "Receipts/a%0Ab.pdf",
            "/\\host\\share\\a.pdf",
            "file:///\\\\host\\share\\a.pdf",
            "%5C%5Chost%5Cshare%5Ca.pdf",
        ] {
            assert!(matches!(classify_in(bad, base), LinkKind::Refused(_)), "{bad:?} -> {:?}", classify_in(bad, base));
        }
        assert!(matches!(classify_in("https://a.example/x.pdf", base), LinkKind::External(_)));
        assert!(matches!(classify_in("Sheet2!A1", base), LinkKind::Internal(_)));
        assert!(matches!(classify_in("Total", base), LinkKind::Internal(_)));
        assert!(is_local_document("C:\\x\\a.pdf") && !is_local_document("C:\\x\\a.exe") && !is_local_document("a.pdf"));
        assert!(!is_local_document("/\\host\\share\\a.pdf") && !is_local_document("\\\\host\\share\\a.pdf"));
    }

    #[test]
    fn references_stay_internal() {
        for r in ["Sheet2!A1", "'My Sheet'!B2:C3", "A1:B2", "#Sheet1!A1", "Total", "", "   "] {
            assert!(matches!(classify(r), LinkKind::Internal(_)), "{r:?} -> {:?}", classify(r));
        }
    }
}
