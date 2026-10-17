//! Help ▸ About ▸ Check for updates: the commands, the start-up preference, and the notice that
//! announces something new. No network: the host's fetch is a scripted channel.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::channel;

use gridcraft_engine::Session;
use gridcraft_ui_egui::updates::{Fetch, VERSION};
use gridcraft_ui_egui::{Services, SheetApp, UiState};
use serde_json::{Value, json};

fn app(fetch: Option<Fetch>) -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    SheetApp::new(session, Services { fetch, ..Default::default() })
}

/// A fetch answering every request with `body`, counting the requests.
fn answering(body: &'static str) -> (Fetch, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let fetch: Fetch = Box::new(move |_url| {
        seen.set(seen.get() + 1);
        let (tx, rx) = channel();
        let _ = tx.send(Ok(body.to_string()));
        rx
    });
    (fetch, calls)
}

/// Drive the per-frame poll until the check has an answer (two frames at most: release, compare).
fn settle(app: &mut SheetApp) -> Value {
    let ctx = egui::Context::default();
    for _ in 0..4 {
        app.poll_updates(&ctx);
    }
    app.run("ui.updates", json!({})).unwrap()
}

#[test]
fn the_startup_preference_is_off_by_default_and_toggles_without_a_dialog() {
    let mut a = app(None);
    assert!(!a.ui.check_updates);
    assert_eq!(a.run("options.checkForUpdatesOnStartup", json!({})), Ok(json!({"on": true})));
    assert_eq!(a.run("options.checkForUpdatesOnStartup", json!({"on": true})), Ok(json!({"on": true})));
    assert_eq!(a.run("options.checkForUpdatesOnStartup", json!({})), Ok(json!({"on": false})));
    assert!(a.dialog.is_none());
    // Saved with the other UI preferences; a ui.json from before this setting reads as off.
    let saved = serde_json::to_value(&a.ui).unwrap();
    assert_eq!(saved["checkUpdates"], json!(false));
    let old: UiState = serde_json::from_value(json!({"dark": true})).unwrap();
    assert!(!old.check_updates);
}

#[test]
fn without_a_fetch_service_the_check_is_unavailable_but_nothing_breaks() {
    let mut a = app(None);
    let err = a.run("help.checkForUpdates", json!({})).unwrap_err();
    assert!(err.contains("unavailable"), "{err}");
    let state = a.run("ui.updates", json!({})).unwrap();
    assert_eq!(state["checking"], json!(false));
    assert_eq!(state["version"], json!(VERSION));
    // Turned on with no fetch: the start-up check is skipped once and never retried.
    a.ui.check_updates = true;
    settle(&mut a);
    assert!(a.notice.is_none());
}

#[test]
fn a_newer_release_is_reported_and_announced_in_the_notice_bar() {
    let (fetch, calls) = answering(r#"{"tag_name":"v99.0.0","html_url":"https://github.com/storytold/gridcraft/releases/tag/v99.0.0"}"#);
    let mut a = app(Some(fetch));
    let started = a.run("help.checkForUpdates", json!({})).unwrap();
    assert_eq!(started["checking"], json!(true));
    assert_eq!(calls.get(), 1);
    assert!(a.dialog.is_none(), "a programmatic check opens no dialog");

    let state = settle(&mut a);
    assert_eq!(state["checking"], json!(false));
    assert_eq!(state["result"]["status"], json!("release"));
    assert_eq!(state["result"]["version"], json!("99.0.0"));
    assert_eq!(state["result"]["url"], json!("https://github.com/storytold/gridcraft/releases/tag/v99.0.0"));
    assert_eq!(calls.get(), 1, "a newer release needs no second request");

    let notice = a.run("ui.notice", json!({})).unwrap();
    assert_eq!(notice["text"], json!("GridCraft 99.0.0 is available."));
    assert_eq!(notice["kind"], json!("info"));
    assert_eq!(notice["action"][1], json!("about"), "the notice's button opens About, where the link is");
}

#[test]
fn the_current_release_is_up_to_date_or_compared_with_main() {
    // The same answer serves both requests: a current release, then a compare with nothing ahead.
    let body: &'static str = Box::leak(format!(r#"{{"tag_name":"v{VERSION}","ahead_by":0}}"#).into_boxed_str());
    let (fetch, calls) = answering(body);
    let mut a = app(Some(fetch));
    a.run("help.checkForUpdates", json!({})).unwrap();
    let state = settle(&mut a);
    assert_eq!(state["result"]["status"], json!("upToDate"));
    let knows_commit = state["commit"].as_str().is_some_and(|c| !c.is_empty());
    assert_eq!(calls.get(), if knows_commit { 2 } else { 1 }, "the commit comparison runs only for a build that knows its commit");
    assert!(a.notice.is_none(), "up to date is not announced");
    assert!(a.run("ui.updates", json!({})).unwrap().get("error").is_none());
}

#[test]
fn errors_from_the_network_are_shown_not_raised() {
    let fetch: Fetch = Box::new(|_| {
        let (tx, rx) = channel();
        let _ = tx.send(Err("no route to host".to_string()));
        rx
    });
    let mut a = app(Some(fetch));
    a.run("help.checkForUpdates", json!({})).unwrap();
    let state = settle(&mut a);
    assert_eq!(state["error"], json!("no route to host"));
    assert!(state.get("result").is_none());
    assert!(a.notice.is_none());
}

#[test]
fn the_startup_check_runs_once_a_session_when_turned_on() {
    let (fetch, calls) = answering(r#"{"tag_name":"v0.0.1"}"#);
    let mut a = app(Some(fetch));
    settle(&mut a);
    assert_eq!(calls.get(), 0, "off by default: nothing is fetched");
    a.run("options.checkForUpdatesOnStartup", json!({"on": true})).unwrap();
    settle(&mut a);
    settle(&mut a);
    assert!(calls.get() >= 1);
    let after_first = calls.get();
    settle(&mut a);
    assert_eq!(calls.get(), after_first, "later frames start no new check");
}
