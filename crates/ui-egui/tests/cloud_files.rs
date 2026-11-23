//! The `CloudBackend` contract, driven through the real interface against an in-process mock.
//!
//! The contract and the pages belong to `craft_ui::cloud`, which this crate takes through that
//! crate's `cloud` feature. GridCraft's half is the `CloudApp` implementation in
//! `gridcraft_ui_egui::cloud`. What a host supplies is a backend, and this is the mock standing
//! in for one: no concrete provider lives in the crate under test, and none lives here either.

#[path = "common/mod.rs"]
mod common;

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::backstage::Page;
use gridcraft_ui_egui::cloud::{Account, Capability, CloudBackend, Entry, EntryKind, Event, Job, Post};
use gridcraft_ui_egui::{SaveAnswer, Services, SheetApp};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, gridcraft_ui_egui::SheetApp>;

/// Frames to run before reading the interface back; the title bar lays out again whenever its
/// right-hand controls change width.
const SETTLE: usize = 4;

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn harness() -> Harness {
    common::os_harness(blank(), egui::os::OperatingSystem::Nix)
}

/// A mock backend that answers every job at once: an account with the capabilities it was built
/// with, a fixed listing, a "demo.xlsx" whose bytes decode as an empty workbook, and a save that
/// always succeeds.
struct MockBackend {
    entries: Vec<Entry>,
    capabilities: Vec<Capability>,
}

impl MockBackend {
    fn with(entries: Vec<Entry>) -> MockBackend {
        MockBackend { entries, capabilities: vec![] }
    }

    fn offering(mut self, capability: Capability) -> MockBackend {
        self.capabilities.push(capability);
        self
    }
}

fn mock_entry(name: &str) -> Entry {
    Entry {
        id: name.to_owned(),
        room: String::new(),
        name: name.to_owned(),
        path: String::new(),
        kind: EntryKind::File,
        size: Some(0),
        modified: None,
        mime: None,
        etag: Some("1".into()),
        writable: true,
        shared_by: None,
    }
}

impl CloudBackend for MockBackend {
    fn submit(&self, job: Job, post: Post) {
        match job {
            Job::Account => post.send(Event::Account(Ok(Account {
                display: "Ada Lovelace".into(),
                capabilities: self.capabilities.clone(),
                provider_linked: true,
            }))),
            Job::List(path) => post.send(Event::Listed { path, entries: Ok(self.entries.clone()) }),
            Job::SharedWithMe => post.send(Event::Shared(Ok(vec![]))),
            Job::Read { entry, into } => {
                let bytes = gridcraft_engine::io::save_bytes(&gridcraft_engine::model::Workbook::default(), "mock.xlsx").unwrap_or_default();
                post.send(Event::Read { entry, into, bytes: Ok(bytes) })
            }
            Job::Create { uid, name, .. } => post.send(Event::Saved { uid, entry: Ok(mock_entry(&name)) }),
            Job::Write { uid, entry, .. } => post.send(Event::Saved { uid, entry: Ok(entry) }),
            // Signing in, linking and finding somewhere to talk are a provider's business. A
            // mock that answers nothing leaves the interface exactly where a slow service would.
            Job::SignIn | Job::LinkStart | Job::LinkPoll(_) | Job::SignOut | Job::Conversation(_) => {}
        }
    }
}

#[test]
fn nothing_cloud_shaped_is_offered_without_a_backend() {
    let mut h = harness();
    assert!(!gridcraft_ui_egui::cloud::available(h.state()));
    assert_eq!(h.state_mut().run("cloud.open", json!({})).unwrap_err(), gridcraft_ui_egui::cloud::UNAVAILABLE);
    assert_eq!(h.state_mut().run("cloud.saveAs", json!({})).unwrap_err(), gridcraft_ui_egui::cloud::UNAVAILABLE);
    assert!(!gridcraft_ui_egui::cloud::has_target(h.state()));

    // "Cloud" is a real heading this crate draws, in `backstage::open_page`, only when a
    // backend is connected; checking for it is a surface this build could actually get wrong,
    // unlike a mock-specific string ("Ada Lovelace") nothing could produce either way, which is
    // what this test asserted before and which passed whether or not the gate around the
    // section worked. "Recent" is the local Open page's own heading, drawn with no backend at
    // all, so its presence confirms the page itself rendered rather than the assertion vacuously
    // passing on an empty page.
    h.state_mut().open_backstage(Page::Open);
    h.run_steps(SETTLE);
    assert!(h.query_by_label("Recent").is_some(), "the local Open page draws with no backend connected");
    assert!(h.query_by_label("Cloud").is_none(), "no cloud section without a connected backend");
}

/// Who is signed in is asked for when a page that needs it opens, not because a title bar was
/// drawn. A workbook somebody opened from their own disk reaches no service at all.
#[test]
fn a_connected_backend_is_available_and_reports_its_account() {
    let mut h = harness();
    h.state_mut().cloud.connect(MockBackend::with(vec![mock_entry("demo.xlsx")]));
    h.run_steps(SETTLE);
    assert!(gridcraft_ui_egui::cloud::available(h.state()));
    assert_eq!(h.state_mut().run("cloud.status", json!({})).unwrap()["account"], json!(null), "nothing was asked for yet");

    h.state_mut().open_backstage(Page::Open);
    h.run_steps(SETTLE);
    let answer = h.state_mut().run("cloud.status", json!({})).unwrap();
    assert_eq!(answer["available"], json!(true));
    assert_eq!(answer["account"]["display"], json!("Ada Lovelace"));
    h.state_mut().backstage = None;
    h.run_steps(SETTLE);
    assert!(h.query_by_label_contains("Ada Lovelace").is_some(), "the title bar shows the signed-in account");
}

#[test]
fn open_page_lists_the_connected_backends_folder() {
    let mut h = harness();
    h.state_mut().cloud.connect(MockBackend::with(vec![mock_entry("demo.xlsx")]));
    h.state_mut().open_backstage(Page::Open);
    h.run_steps(SETTLE);
    assert!(h.query_by_label("demo.xlsx").is_some(), "the Open page's cloud section lists the mock folder");
}

#[test]
fn only_a_workbook_gridcraft_opens_is_offered() {
    let mut h = harness();
    h.state_mut().cloud.connect(MockBackend::with(vec![mock_entry("demo.xlsx"), mock_entry("holiday.mp4")]));
    h.state_mut().open_backstage(Page::Open);
    h.run_steps(SETTLE);
    let answer = h.state_mut().run("cloud.status", json!({})).unwrap();
    assert_eq!(answer["openable"], json!(["demo.xlsx"]), "a video in the same folder is listed and not offered");
}

#[test]
fn save_as_then_save_round_trips_through_a_cloud_origin() {
    let mut h = harness();
    h.state_mut().cloud.connect(MockBackend::with(vec![]));
    assert!(!gridcraft_ui_egui::cloud::has_target(h.state()));
    h.state_mut().run("cloud.saveAs", json!({"folder": "", "name": "demo"})).unwrap();
    h.run_steps(SETTLE);
    assert!(gridcraft_ui_egui::cloud::has_target(h.state()), "a successful Save As records the cloud origin");
    // `file.save` on this workbook now writes back to that origin instead of asking for a path
    // on this computer: it succeeds with none configured.
    h.state_mut().run("file.save", json!({})).unwrap();
    h.run_steps(SETTLE);
    assert!(gridcraft_ui_egui::cloud::has_target(h.state()));
}

/// A share backend that reports `Capability::Sharing` (and nothing else) and answers an access
/// lookup with an empty list, so the dialog's "Who has access" section has something to show
/// rather than loading forever.
struct MockShareBackend;

impl craft_ui::share::ShareBackend for MockShareBackend {
    fn submit(&self, request: craft_ui::share::Request, reply: craft_ui::share::Replier) {
        match request {
            craft_ui::share::Request::Capabilities => {
                reply.answer(craft_ui::share::Answer::Capabilities { capabilities: vec![Capability::Sharing], linked: true });
            }
            craft_ui::share::Request::Access { .. } => reply.answer(craft_ui::share::Answer::Access(vec![])),
            _ => {}
        }
    }
}

/// Taking the shared pages means holding a share dialog, which is not the same as offering one:
/// `ShareDialog::show` has to run every frame for its own capability question to ever be asked,
/// independent of whether the cloud backend's `Account.capabilities` echo correctly (which is all
/// the test this replaces checked; it passed with the dialog never opening at all, because
/// nothing called `ShareDialog::connect` or `ShareDialog::show`). This drives the row menu's Share
/// action through the real interface and checks the dialog itself opens.
#[test]
fn sharing_opens_the_dialog_from_the_row_menus_share_action() {
    let mut h = harness();
    h.state_mut().cloud.connect(MockBackend::with(vec![mock_entry("demo.xlsx")]).offering(Capability::Sharing));
    h.state_mut().sharing.connect(MockShareBackend);
    h.state_mut().open_backstage(Page::Open);
    h.run_steps(SETTLE);
    assert!(!h.state().sharing.is_open());

    h.get_by_role_and_label(egui::accesskit::Role::Button, "More options").click();
    h.run_steps(2);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Share\u{2026}").click();
    h.run_steps(SETTLE);
    assert!(h.state().sharing.is_open(), "the row menu's Share action should have opened the dialog");
}

/// A provider that does not report sharing is never drawn a control for it, not even the row
/// menu item that would open the dialog: a hidden feature beats a button that fails on use.
#[test]
fn a_provider_without_sharing_offers_no_share_row_action() {
    let mut h = harness();
    h.state_mut().cloud.connect(MockBackend::with(vec![mock_entry("demo.xlsx")]));
    h.state_mut().sharing.connect(MockShareBackend);
    h.state_mut().open_backstage(Page::Open);
    h.run_steps(SETTLE);

    h.get_by_role_and_label(egui::accesskit::Role::Button, "More options").click();
    h.run_steps(2);
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Button, "Share\u{2026}").is_none(), "no Share action without the capability");
    assert!(!h.state().sharing.is_open());
}

/// Answering Save during a quit must not abandon it. A local save clears the dirty flag
/// synchronously, so `answer_save_changes` can close the document and go on quitting in the
/// same call; a cloud save is still on the wire at that point, and nothing short of
/// `cloud::pump` seeing it finish may close the document or continue the quit.
#[test]
fn save_answered_during_quit_waits_for_an_in_flight_cloud_save_before_closing() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.cloud.connect(MockBackend::with(vec![]));
    let ctx = egui::Context::default();

    // A cloud origin, as an earlier Save As would have left.
    app.run("cloud.saveAs", json!({"folder": "", "name": "demo"})).unwrap();
    app.logic(&ctx);
    assert!(gridcraft_ui_egui::cloud::has_target(&app));
    let uid = app.session.active().unwrap().uid;

    // Dirty again, as if the user kept typing after that save landed.
    app.session.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    assert!(app.session.active().unwrap().is_dirty());

    // Save Changes, answered Save, while quitting.
    app.answer_save_changes(SaveAnswer::Save, true, &ctx);
    assert!(!app.allow_close, "the quit must not finish before the cloud save does");
    assert!(app.session.document_index(uid).is_some(), "the document stays open until the save lands");

    // The mock answered synchronously, but only `cloud::pump`, run from `logic` on a later
    // frame, drains that answer and finishes what Save started.
    app.logic(&ctx);
    assert!(app.session.document_index(uid).is_none(), "the document closes once the cloud save lands");
    assert!(app.allow_close, "the quit continues once the cloud save finishes");
}
