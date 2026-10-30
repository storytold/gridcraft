//! Help ▸ Report an Issue…: opens a new GitHub issue with the version and system filled in. Nothing
//! is created until the user submits it.

use gridcraft_engine::io::FileKind;
use serde_json::json;

use crate::SheetApp;

/// The new-issue address. `version` and `environment` (`os` too, for the other open form) are the bug form's field ids, and `body`
/// fills a blank issue where no form exists. The document is described by its file extension
/// alone, never a name, path or contents. Plain strings in, so it can move to a shared crate.
pub(crate) fn issue_url(version: &str, os: &str, document_ext: Option<&str>) -> String {
    let short = |s: &str| s.chars().filter(|c| !c.is_control()).take(16).collect::<String>();
    let ext: String = document_ext.unwrap_or("none").chars().filter(char::is_ascii_alphanumeric).take(8).collect();
    let (version, environment) = (short(version), format!("{}; document: {ext}", short(os)));
    let encode = |s: &str| {
        let keep = |b: &u8| b.is_ascii_alphanumeric() || b"-_.~".contains(b);
        s.bytes().map(|b| if keep(&b) { char::from(b).to_string() } else { format!("%{b:02X}") }).collect::<String>()
    };
    let body = format!("GridCraft {version}\n{environment}");
    let (v, e, b) = (encode(&version), encode(&environment), encode(&body));
    format!("https://github.com/storytold/gridcraft/issues/new?template=bug_report.yml&version={v}&environment={e}&os={e}&body={b}")
}

/// `help.reportIssue`: opens the issue form. Returns `{url, opened}` and never opens a dialog;
/// [`run_menu`] shows the address when no browser opened.
pub(crate) fn run(app: &mut SheetApp) -> serde_json::Value {
    // Only a format GridCraft opens is named: an odd extension could be private text.
    let ext = app
        .session
        .active()
        .and_then(|d| d.path.as_deref())
        .filter(|p| FileKind::from_path(p).is_some())
        .and_then(|p| std::path::Path::new(p).extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase));
    let url = issue_url(env!("CARGO_PKG_VERSION"), &crate::backstage::platform(), ext.as_deref());
    let opened = app.services.open_url.as_ref().is_some_and(|open| open(&url));
    json!({"url": url, "opened": opened})
}

/// The menu, ribbon and Account entries: like [`run`], plus the address to copy when no browser opened.
pub(crate) fn run_menu(app: &mut SheetApp) {
    let result = run(app);
    if result["opened"] != true {
        app.open_dialog("failedLink", json!({"url": result["url"]}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_carries_version_and_system_only_and_stays_short() {
        let url = issue_url("0.1.0", "Linux (x86_64)", Some("xlsx"));
        assert!(url.contains("&version=0.1.0&environment=Linux%20%28x86_64%29%3B%20document%3A%20xlsx&os=Linux%20%28x86_64%29%3B%20document%3A%20xlsx&body=GridCraft%200.1.0%0A"), "{url}");
        // Hostile text and an unusual extension can't add a parameter, leak, or grow the address.
        let url = issue_url(&"\u{1F600}".repeat(99), &"&#=".repeat(99), Some("x.y/Z"));
        assert_eq!(url.matches('?').count() + url.matches('#').count(), 1, "{url}");
        assert!(url.is_ascii() && url.contains("document%3A%20xyZ") && url.len() < 2048, "{} bytes: {url}", url.len());
        assert!(issue_url("1", "w", None).contains("document%3A%20none"));
    }
}
