//! Help ▸ About ▸ Check for updates: tells whether a newer GridCraft exists; installing stays the
//! user's call.
//!
//! Two questions, asked of GitHub's public API, one request each and only when the user asks (or
//! once at start-up, if they turned that on; it is off by default, like everything that talks to
//! the network). First the latest release: newer than this build's version means "GridCraft X is
//! available", with the release page to go to. Otherwise, for a build that knows its commit (baked
//! in by `build.rs` from the checkout), how far `main` has moved on: "N commits ahead" is what a
//! source install updates with `cargo xtask update`.
//!
//! This module has no network code. The host passes a [`Fetch`] in `Services`: it runs the GET
//! off the UI thread and sends the body back on a channel, which [`Check::poll`] drains once a
//! frame. The web app and headless tests pass none, and the command says checks are unavailable.
//! Everything that arrives from the network is hostile: bodies are size-capped, JSON is read
//! field by field, and a version that does not parse is simply not newer.

use std::sync::mpsc::{Receiver, TryRecvError};

use serde::Serialize;
use serde_json::Value as Json;

/// The repository the app is built from, as `owner/name`.
pub const REPO: &str = "storytold/gridcraft";
/// The version compiled in (the workspace version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The commit this build was made from, when `build.rs` could ask git for it.
pub const COMMIT: Option<&str> = option_env!("GRIDCRAFT_BUILD_COMMIT");
/// Largest response body that is parsed; the real answers are a few kilobytes.
pub const MAX_BODY: usize = 256 * 1024;

/// A host's way to GET a URL off the UI thread. The body text (or an error) arrives on the
/// receiver; the host sets the `User-Agent` GitHub requires and a timeout.
pub type Fetch = Box<dyn Fn(&str) -> Receiver<Result<String, String>>>;

/// What the check found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Status {
    /// A newer release is published.
    Release {
        version: String,
        url: String,
    },
    /// The release is current, but `main` has moved on since this build's commit.
    Commits {
        behind: usize,
        url: String,
    },
    UpToDate,
}

impl Status {
    /// One sentence for the About window and the notice bar (English; translated by the caller).
    pub fn text(&self) -> String {
        match self {
            Status::Release { version, .. } => format!("GridCraft {version} is available."),
            Status::Commits { behind, .. } => {
                format!("This build is {behind} commit{} behind main.", if *behind == 1 { "" } else { "s" })
            }
            Status::UpToDate => "GridCraft is up to date.".into(),
        }
    }

    /// Where to go next, if anywhere.
    pub fn url(&self) -> Option<&str> {
        match self {
            Status::Release { url, .. } | Status::Commits { url, .. } => Some(url),
            Status::UpToDate => None,
        }
    }
}

/// The stage a running check is at.
enum Stage {
    Release,
    Compare,
}

/// A check in flight or finished. One per app; starting a new one replaces an unfinished one.
#[derive(Default)]
pub struct Check {
    pending: Option<(Stage, Receiver<Result<String, String>>)>,
    /// The last answer: the status found, or why it could not be found.
    pub result: Option<Result<Status, String>>,
    /// `now_ms` when `result` arrived.
    pub finished_at: Option<f64>,
    /// Whether the start-up check already ran this session (it runs at most once).
    pub startup_done: bool,
}

impl Check {
    pub fn is_checking(&self) -> bool {
        self.pending.is_some()
    }

    /// Ask for the latest release. Replaces any check still in flight.
    pub fn start(&mut self, fetch: &Fetch) {
        self.pending = Some((Stage::Release, fetch(&release_url())));
        self.result = None;
        self.finished_at = None;
    }

    /// Drain the in-flight request. `Some` once this call finished the check (the same answer is
    /// then in `result`); a release answer that needs the commit comparison starts it here.
    pub fn poll(&mut self, fetch: Option<&Fetch>, now_ms: f64) -> Option<Result<Status, String>> {
        let Some((stage, rx)) = &self.pending else { return None };
        let body = match rx.try_recv() {
            Ok(body) => body,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the update check was interrupted".to_string()),
        };
        let next = match (stage, body) {
            (_, Err(e)) => Err(e),
            (Stage::Release, Ok(body)) => match parse_release(&body) {
                Err(e) => Err(e),
                Ok(release) if is_newer(&release.version, VERSION) => Ok(Status::Release { version: release.version, url: release.url }),
                Ok(_) => match (COMMIT, fetch) {
                    (Some(commit), Some(fetch)) if !commit.is_empty() => {
                        self.pending = Some((Stage::Compare, fetch(&compare_url(commit))));
                        return None;
                    }
                    _ => Ok(Status::UpToDate),
                },
            },
            (Stage::Compare, Ok(body)) => {
                let commit = COMMIT.unwrap_or("");
                parse_compare(&body).map_err(|e| compare_error(e, commit)).map(|behind| match behind {
                    0 => Status::UpToDate,
                    behind => Status::Commits { behind, url: compare_page(commit) },
                })
            }
        };
        self.pending = None;
        self.result = Some(next.clone());
        self.finished_at = Some(now_ms);
        Some(next)
    }

    /// The state as the control channel reports it.
    pub fn json(&self) -> Json {
        let mut v = serde_json::json!({ "checking": self.is_checking(), "version": VERSION, "commit": COMMIT });
        match &self.result {
            Some(Ok(status)) => v["result"] = serde_json::to_value(status).unwrap_or(Json::Null),
            Some(Err(e)) => v["error"] = Json::String(e.clone()),
            None => {}
        }
        v
    }
}

pub fn release_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// `base...head` with this build's commit as base: `ahead_by` is then how far main has moved on.
pub fn compare_url(commit: &str) -> String {
    format!("https://api.github.com/repos/{REPO}/compare/{commit}...main")
}

/// The web page listing the same commits.
pub fn compare_page(commit: &str) -> String {
    format!("https://github.com/{REPO}/compare/{}...main", short(commit))
}

/// The first seven characters of a commit hash (what git shows).
pub fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

/// `GET /releases/latest`: the tag (a `v` prefix dropped) and the release page.
pub fn parse_release(body: &str) -> Result<Release, String> {
    let v = parse_body(body)?;
    let tag = v.get("tag_name").and_then(Json::as_str).ok_or("the release answer has no tag_name")?;
    let version = tag.trim().trim_start_matches(['v', 'V']).to_string();
    if parse_version(&version).is_none() {
        return Err(format!("the release tag `{tag}` is not a version"));
    }
    let url = v.get("html_url").and_then(Json::as_str).filter(|u| u.starts_with("https://github.com/")).map(str::to_string);
    Ok(Release { version, url: url.unwrap_or_else(|| format!("https://github.com/{REPO}/releases/latest")) })
}

/// `GET /compare/{commit}...main`: how many commits main is ahead of this build. A base commit
/// GitHub no longer knows (a rebased branch, a local-only commit) answers with an error.
pub fn parse_compare(body: &str) -> Result<usize, String> {
    let v = parse_body(body)?;
    let ahead = v.get("ahead_by").and_then(Json::as_u64).ok_or("the compare answer has no ahead_by")?;
    Ok(usize::try_from(ahead).unwrap_or(usize::MAX))
}

/// A compare that GitHub answers "Not Found" means it does not know this build's commit: it was
/// made from a commit that was never pushed (local work, a rebased branch). Say so, instead of a
/// bare "Not Found".
pub fn compare_error(e: String, commit: &str) -> String {
    if e.contains("Not Found") {
        format!("GitHub doesn't know this build's commit ({}); it was never pushed, so it can't be compared with main.", short(commit))
    } else {
        e
    }
}

fn parse_body(body: &str) -> Result<Json, String> {
    if body.len() > MAX_BODY {
        return Err(format!("the answer is too large ({} bytes)", body.len()));
    }
    let v: Json = serde_json::from_str(body).map_err(|e| format!("the answer is not JSON: {e}"))?;
    // GitHub reports problems (rate limit, unknown commit) as `{"message": "..."}`.
    if let Some(message) = v.get("message").and_then(Json::as_str)
        && v.get("tag_name").is_none()
        && v.get("ahead_by").is_none()
    {
        return Err(format!("GitHub said: {}", message.chars().take(200).collect::<String>()));
    }
    Ok(v)
}

/// `MAJOR.MINOR.PATCH` with an optional `-pre.release`; `None` for anything else. Build metadata
/// (`+…`) is ignored.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64, Option<&str>)> {
    let v = v.split('+').next()?;
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) if !p.is_empty() => (c, Some(p)),
        Some(_) => return None,
        None => (v, None),
    };
    let mut nums = core.split('.').map(|n| if n.is_empty() || n.len() > 9 { None } else { n.parse::<u64>().ok() });
    let (a, b, c) = (nums.next()??, nums.next()??, nums.next()??);
    if nums.next().is_some() {
        return None;
    }
    Some((a, b, c, pre))
}

/// Whether `latest` is a newer version than `current`. A pre-release is older than its release
/// (`1.0.0-rc.1 < 1.0.0`) and pre-releases order by their text; unparseable input is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (Some(l), Some(c)) = (parse_version(latest), parse_version(current)) else { return false };
    let key = |(a, b, c, pre): (u64, u64, u64, Option<&str>)| (a, b, c, pre.is_none(), pre.map(str::to_string));
    key(l) > key(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(parse_version("0.7.0"), Some((0, 7, 0, None)));
        assert_eq!(parse_version("1.2.3-rc.1"), Some((1, 2, 3, Some("rc.1"))));
        assert_eq!(parse_version("1.2.3+build.5"), Some((1, 2, 3, None)));
        for bad in ["", "1.2", "1.2.3.4", "a.b.c", "1.2.3-", "1234567890.0.0"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(is_newer("0.8.0", "0.7.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(is_newer("0.7.1", "0.7.0"));
        assert!(is_newer("0.7.0", "0.7.0-rc.1"), "a release is newer than its pre-release");
        assert!(is_newer("0.7.0-rc.2", "0.7.0-rc.1"));
        assert!(!is_newer("0.7.0", "0.7.0"));
        assert!(!is_newer("0.6.9", "0.7.0"));
        assert!(!is_newer("0.7.0-rc.1", "0.7.0"));
        assert!(!is_newer("garbage", "0.7.0"), "unparseable input is never an update");
        assert!(!is_newer("0.8.0", "garbage"));
    }

    #[test]
    fn reads_the_release_answer() {
        let body = r#"{"tag_name":"v0.8.0","html_url":"https://github.com/storytold/gridcraft/releases/tag/v0.8.0","name":"GridCraft 0.8.0"}"#;
        assert_eq!(
            parse_release(body).unwrap(),
            Release { version: "0.8.0".into(), url: "https://github.com/storytold/gridcraft/releases/tag/v0.8.0".into() }
        );
        // No usable page: the generic latest-release page.
        let r = parse_release(r#"{"tag_name":"0.8.0","html_url":"javascript:alert(1)"}"#).unwrap();
        assert_eq!(r.url, "https://github.com/storytold/gridcraft/releases/latest");
        assert!(parse_release(r#"{"tag_name":"nightly"}"#).is_err());
        assert!(parse_release(r#"{"html_url":"https://github.com/x"}"#).is_err());
        assert!(parse_release("not json").is_err());
        assert!(parse_release(r#"{"message":"API rate limit exceeded"}"#).unwrap_err().contains("rate limit"));
        let huge = format!("{{\"tag_name\":\"v0.8.0\",\"pad\":\"{}\"}}", "x".repeat(MAX_BODY));
        assert!(parse_release(&huge).unwrap_err().contains("too large"));
    }

    #[test]
    fn reads_the_compare_answer() {
        assert_eq!(parse_compare(r#"{"status":"ahead","ahead_by":12,"behind_by":0}"#).unwrap(), 12);
        assert_eq!(parse_compare(r#"{"status":"identical","ahead_by":0,"behind_by":0}"#).unwrap(), 0);
        assert!(parse_compare(r#"{"message":"Not Found"}"#).unwrap_err().contains("Not Found"));
        assert!(parse_compare(r#"{"ahead_by":-1}"#).is_err());
        assert!(parse_compare("[]").is_err());
        // An unpushed commit: GitHub answers Not Found, and the message says what that means.
        let e = compare_error(parse_compare(r#"{"message":"Not Found"}"#).unwrap_err(), "5376244abcdef");
        assert_eq!(e, "GitHub doesn't know this build's commit (5376244); it was never pushed, so it can't be compared with main.");
        assert_eq!(compare_error("rate limit".into(), "abc"), "rate limit", "other errors pass through");
    }

    #[test]
    fn urls_name_the_repository() {
        assert_eq!(release_url(), "https://api.github.com/repos/storytold/gridcraft/releases/latest");
        assert_eq!(compare_url("abc1234def"), "https://api.github.com/repos/storytold/gridcraft/compare/abc1234def...main");
        assert_eq!(compare_page("abc1234def"), "https://github.com/storytold/gridcraft/compare/abc1234...main");
        assert_eq!(short("abc"), "abc");
    }

    #[test]
    fn status_texts() {
        assert_eq!(Status::Release { version: "0.8.0".into(), url: String::new() }.text(), "GridCraft 0.8.0 is available.");
        assert_eq!(Status::Commits { behind: 1, url: String::new() }.text(), "This build is 1 commit behind main.");
        assert_eq!(Status::Commits { behind: 3, url: String::new() }.text(), "This build is 3 commits behind main.");
        assert_eq!(Status::UpToDate.text(), "GridCraft is up to date.");
        assert_eq!(Status::UpToDate.url(), None);
    }

    /// A fetch that answers every request from a queue, in order.
    fn scripted(answers: Vec<Result<String, String>>) -> Fetch {
        let queue = std::cell::RefCell::new(answers.into_iter());
        Box::new(move |_url| {
            let (tx, rx) = channel();
            if let Some(a) = queue.borrow_mut().next() {
                let _ = tx.send(a);
            }
            rx
        })
    }

    #[test]
    fn a_newer_release_finishes_the_check_in_one_request() {
        let fetch = scripted(vec![Ok(r#"{"tag_name":"v99.0.0","html_url":"https://github.com/storytold/gridcraft/releases/tag/v99.0.0"}"#.into())]);
        let mut check = Check::default();
        assert_eq!(check.poll(Some(&fetch), 0.0), None, "nothing in flight");
        check.start(&fetch);
        assert!(check.is_checking());
        let got = check.poll(Some(&fetch), 5.0).unwrap().unwrap();
        assert_eq!(got, Status::Release { version: "99.0.0".into(), url: "https://github.com/storytold/gridcraft/releases/tag/v99.0.0".into() });
        assert!(!check.is_checking());
        assert_eq!(check.finished_at, Some(5.0));
        assert_eq!(check.json()["result"]["status"], "release");
        assert_eq!(check.json()["result"]["version"], "99.0.0");
    }

    #[test]
    fn the_current_release_goes_on_to_the_commit_comparison_when_the_build_knows_its_commit() {
        let current = format!(r#"{{"tag_name":"v{VERSION}"}}"#);
        let fetch = scripted(vec![Ok(current), Ok(r#"{"ahead_by":4}"#.into())]);
        let mut check = Check::default();
        check.start(&fetch);
        let first = check.poll(Some(&fetch), 1.0);
        match COMMIT {
            Some(commit) if !commit.is_empty() => {
                assert_eq!(first, None, "the comparison is now in flight");
                assert!(check.is_checking());
                let got = check.poll(Some(&fetch), 2.0).unwrap().unwrap();
                assert_eq!(got, Status::Commits { behind: 4, url: compare_page(commit) });
            }
            _ => assert_eq!(first, Some(Ok(Status::UpToDate)), "without a commit there is nothing more to ask"),
        }
    }

    #[test]
    fn without_a_fetch_for_the_second_request_the_release_answer_is_final() {
        let current = format!(r#"{{"tag_name":"{VERSION}"}}"#);
        let fetch = scripted(vec![Ok(current)]);
        let mut check = Check::default();
        check.start(&fetch);
        assert_eq!(check.poll(None, 1.0), Some(Ok(Status::UpToDate)));
    }

    #[test]
    fn errors_are_reported_not_raised() {
        let fetch = scripted(vec![Err("no network".into())]);
        let mut check = Check::default();
        check.start(&fetch);
        assert_eq!(check.poll(Some(&fetch), 1.0), Some(Err("no network".into())));
        assert_eq!(check.json()["error"], "no network");
        // A host that drops the sender without answering.
        let fetch: Fetch = Box::new(|_| channel().1);
        check.start(&fetch);
        assert_eq!(check.result, None, "starting clears the old answer");
        assert!(check.poll(Some(&fetch), 2.0).unwrap().unwrap_err().contains("interrupted"));
    }
}
