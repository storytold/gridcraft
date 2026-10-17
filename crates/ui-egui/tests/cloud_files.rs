//! Open and Save against cloud storage, driven through the real interface
//! with a service that lives inside the test.
//!
//! The service answers on the calling thread, so a job posted during a frame
//! is in the inbox before the next one. Everything else is the shipping code:
//! the Backstage pages, `SheetApp::run`, the conflict modal and the xlsx path.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::cloud::{self, Account, Capability, CloudBackend, Entry, EntryKind, Event, Fault, Job, Post};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

// ---------------------------------------------------------------- a service in memory

#[derive(Default)]
struct State {
    /// Every file and folder, by its own path. A folder is listed in its
    /// parent like any other entry.
    files: Vec<(Entry, Vec<u8>)>,
    /// Incoming shares, which have no folder of this user's to sit in.
    shared: Vec<(Entry, Vec<u8>)>,
    capabilities: Vec<Capability>,
    linked: bool,
    /// What the service was asked to do, so a test can prove a save went out
    /// once and was not retried.
    writes: Vec<(String, Option<String>, usize)>,
    creates: Vec<(String, String, usize)>,
    reads: Vec<String>,
}

impl State {
    fn account(&self) -> Result<Account, Fault> {
        if !self.linked {
            return Err(Fault::NotLinked);
        }
        Ok(Account { display: "Ada Lovelace".to_owned(), capabilities: self.capabilities.clone(), provider_linked: true })
    }

    fn find(&self, id: &str) -> Option<&(Entry, Vec<u8>)> {
        self.files.iter().chain(self.shared.iter()).find(|(e, _)| e.id == id)
    }

    /// Somebody else saves over a file, which is what makes the next
    /// conditional write fail.
    fn overwrite(&mut self, id: &str, bytes: Vec<u8>) {
        if let Some(slot) = self.files.iter_mut().chain(self.shared.iter_mut()).find(|(e, _)| e.id == id) {
            slot.0.etag = Some(format!("{}-next", slot.0.etag.clone().unwrap_or_default()));
            slot.1 = bytes;
        }
    }

    fn answer(&mut self, job: Job) -> Vec<Event> {
        // Signing out needs no session, and answers nothing. The window
        // clears its own state before the job is ever sent.
        if matches!(job, Job::SignOut) {
            self.linked = false;
            return Vec::new();
        }
        if !self.linked && !matches!(job, Job::LinkStart | Job::LinkPoll(_)) {
            return vec![match job {
                Job::Account => Event::Account(Err(Fault::NotLinked)),
                Job::List(path) => Event::Listed { path, entries: Err(Fault::NotLinked) },
                Job::SharedWithMe => Event::Shared(Err(Fault::NotLinked)),
                Job::Read { entry, into, .. } => Event::Read { entry, into, bytes: Err(Fault::NotLinked) },
                Job::Write { uid, .. } | Job::Create { uid, .. } => Event::Saved { uid, entry: Err(Fault::NotLinked) },
                Job::LinkStart | Job::LinkPoll(_) => Event::Linked(Ok(false)),
                Job::Conversation(_) => Event::Conversation(Err(Fault::NotLinked)),
                // Handled above, before this arm is reached.
                Job::SignOut => return Vec::new(),
            }];
        }
        match job {
            Job::SignOut => Vec::new(),
            Job::Conversation(_) => {
                if !self.capabilities.contains(&Capability::Conversation) {
                    let why = "this provider has nowhere to talk".to_owned();
                    return vec![Event::Conversation(Err(Fault::Refused(why)))];
                }
                let room = craft_cloud::Conversation { url: "https://cloud.example/call/room".to_owned(), name: "Talk".to_owned() };
                vec![Event::Conversation(Ok(room))]
            }
            Job::Account => vec![Event::Account(self.account())],
            Job::LinkStart => {
                vec![Event::Browser { url: "https://cloud.example/link/abc".to_owned(), handle: Some("abc".to_owned()) }]
            }
            Job::LinkPoll(_) => vec![Event::Linked(Ok(self.linked))],
            Job::List(path) => {
                let entries = self.files.iter().filter(|(e, _)| parent(&e.path) == path).map(|(e, _)| e.clone()).collect();
                vec![Event::Listed { path, entries: Ok(entries) }]
            }
            Job::SharedWithMe => {
                if !self.capabilities.contains(&Capability::SharedWithMe) {
                    return vec![Event::Shared(Err(Fault::Refused("this provider has no incoming shares".to_owned())))];
                }
                vec![Event::Shared(Ok(self.shared.iter().map(|(e, _)| e.clone()).collect()))]
            }
            Job::Read { entry, into } => {
                self.reads.push(entry.id.clone());
                let bytes = match self.find(&entry.id) {
                    Some((_, bytes)) => Ok(bytes.clone()),
                    None => Err(Fault::Refused("no such file".to_owned())),
                };
                vec![Event::Read { entry, into, bytes }]
            }
            Job::Write { uid, entry, bytes } => {
                self.writes.push((entry.id.clone(), entry.etag.clone(), bytes.len()));
                let stored = self.find(&entry.id).map(|(e, _)| e.etag.clone());
                let answer = match stored {
                    None => Err(Fault::Refused("no such file".to_owned())),
                    // The condition the save was made under. A stored copy
                    // that moved on refuses it, which is the whole point.
                    Some(stored) if stored != entry.etag => Err(Fault::Conflict),
                    Some(_) => {
                        let slot = self.files.iter_mut().chain(self.shared.iter_mut()).find(|(e, _)| e.id == entry.id);
                        match slot {
                            Some(slot) => {
                                slot.0.etag = Some(format!("{}+", slot.0.etag.clone().unwrap_or_default()));
                                slot.0.size = Some(bytes.len() as u64);
                                slot.1 = bytes;
                                Ok(slot.0.clone())
                            }
                            None => Err(Fault::Refused("no such file".to_owned())),
                        }
                    }
                };
                vec![Event::Saved { uid, entry: answer }]
            }
            Job::Create { uid, folder, name, bytes } => {
                self.creates.push((folder.clone(), name.clone(), bytes.len()));
                let path = format!("{}/{name}", folder.trim_end_matches('/'));
                let entry = file_entry(&path, None);
                self.files.push((entry.clone(), bytes));
                vec![Event::Saved { uid, entry: Ok(entry) }]
            }
        }
    }
}

fn parent(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => String::new(),
        Some(cut) => path[..cut].to_owned(),
    }
}

fn file_entry(path: &str, mime: Option<&str>) -> Entry {
    Entry {
        id: path.to_owned(),
        room: format!("room:{path}"),
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        path: path.to_owned(),
        kind: EntryKind::File,
        size: Some(1024),
        modified: Some("2026-10-01T09:00:00Z".to_owned()),
        mime: mime.map(str::to_owned),
        etag: Some("v1".to_owned()),
        writable: true,
        shared_by: None,
    }
}

fn folder_entry(path: &str) -> Entry {
    Entry { kind: EntryKind::Folder, size: None, etag: None, mime: None, ..file_entry(path, None) }
}

/// A workbook with one value in A1, as real xlsx bytes.
fn workbook_bytes(input: &str) -> Vec<u8> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": input})).unwrap();
    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    gridcraft_engine::io::base64_decode(saved["base64"].as_str().unwrap()).unwrap()
}

struct Fake {
    state: Arc<Mutex<State>>,
    opened_urls: Arc<Mutex<Vec<String>>>,
}

/// The worker half, answering on the calling thread. A job posted during a
/// frame is therefore answered before the next one.
struct Service(Arc<Mutex<State>>);

impl CloudBackend for Service {
    fn submit(&self, job: Job, post: Post) {
        for event in self.0.lock().unwrap().answer(job) {
            post.send(event);
        }
    }
}

/// A sharing service that answers out of a fixed set of people.
///
/// The share dialog is a craft-ui control with its own tests. This exists so
/// the window has somewhere to share to. That is what decides whether the
/// title bar draws a split button at all.
struct Sharing;

impl craft_ui::share::ShareBackend for Sharing {
    fn submit(&self, request: craft_ui::share::Request, reply: craft_ui::share::Replier) {
        use craft_cloud::{Contact, Permission, Share, ShareTarget};
        use craft_ui::share::{Answer, Request};
        let person = |id: &str, name: &str, email: &str| Contact {
            id: id.to_owned(),
            target: ShareTarget::User(id.to_owned()),
            name: name.to_owned(),
            email: Some(email.to_owned()),
            avatar: None,
            is_group: false,
        };
        match request {
            Request::Capabilities => reply.answer(Answer::Capabilities {
                capabilities: vec![
                    Capability::Sharing,
                    Capability::Contacts,
                    Capability::LinkShares,
                    Capability::LinkPassword,
                    Capability::ShareNotify,
                ],
                linked: true,
            }),
            Request::Lookup { query, .. } => {
                let everybody = [
                    person("dana", "Dana Okafor", "dana@example.org"),
                    person("priya", "Priya Raman", "priya@example.org"),
                    person("tomas", "Tom\u{e1}s Alvarez", "tomas@example.org"),
                ];
                let want = query.to_lowercase();
                reply.answer(Answer::Lookup(everybody.into_iter().filter(|c| c.name.to_lowercase().contains(&want)).collect()));
            }
            Request::Access { .. } => reply.answer(Answer::Access(vec![
                Share {
                    id: "s1".to_owned(),
                    target: ShareTarget::User("dana".to_owned()),
                    display: Some("Dana Okafor".to_owned()),
                    permission: Permission::Edit,
                    url: None,
                    note: None,
                    created: None,
                },
                Share {
                    id: "s2".to_owned(),
                    target: ShareTarget::Link,
                    display: None,
                    permission: Permission::View,
                    url: Some("https://cloud.example/s/abc123".to_owned()),
                    note: None,
                    created: None,
                },
            ])),
            other => reply.refuse(format!("{other:?} is not offered here")),
        }
    }
}

fn harness(state: State) -> (Harness, Fake) {
    build(state, false)
}

/// The same window, with an offscreen renderer attached so a frame can be
/// written to a file. Only the render test asks for one. Starting wgpu for
/// every behaviour test would cost seconds and prove nothing.
fn rendering(state: State) -> (Harness, Fake) {
    build(state, true)
}

fn build(state: State, rendering: bool) -> (Harness, Fake) {
    let store = Arc::new(Mutex::new(state));
    let urls = Arc::new(Mutex::new(Vec::new()));
    let captured = urls.clone();
    let services = Services { open_url: Some(Box::new(move |url: &str| captured.lock().unwrap().push(url.to_owned()))), ..Default::default() };
    let mut session = Session::new();
    session.new_workbook();
    let builder = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 900.0));
    // Two device pixels per point, so a render is legible when somebody looks
    // at it. The behaviour tests keep one, where a click lands on the point it
    // was aimed at.
    let builder = if rendering { builder.with_pixels_per_point(2.0).wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        {
            let mut app = SheetApp::new(session, services);
            app.cloud.connect(Service(store.clone()));
            // Only a render wants one. A behaviour test that asked for
            // capabilities it never uses would answer a question nobody put.
            if rendering {
                app.share.connect(Sharing);
            }
            app
        },
    );
    h.run_steps(4);
    (h, Fake { state: store, opened_urls: urls })
}

/// A linked account with two folders of files and one incoming share.
fn furnished() -> State {
    let mut state = State {
        linked: true,
        capabilities: vec![Capability::Files, Capability::SharedWithMe, Capability::Sharing, Capability::Etags],
        ..State::default()
    };
    state.files.push((folder_entry("/Reports"), Vec::new()));
    state.files.push((file_entry("/Budget.xlsx", Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")), workbook_bytes("42")));
    state.files.push((file_entry("/notes.pdf", Some("application/pdf")), b"%PDF-1.4".to_vec()));
    // No extension at all, and a content type that says what it is.
    state.files.push((file_entry("/exported", Some("text/csv")), b"a,b\n1,2\n".to_vec()));
    state.files.push((file_entry("/Reports/Q3.xlsx", None), workbook_bytes("7")));
    let mut team = file_entry("/shared/Team.xlsx", None);
    team.shared_by = Some("Dana".to_owned());
    state.shared.push((team, workbook_bytes("99")));
    state
}

fn open_page(h: &mut Harness) {
    h.state_mut().run("ui.backstage", json!({"page": "open"})).unwrap();
    h.run_steps(4);
}

fn click(h: &mut Harness, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(4);
}

fn status(h: &mut Harness) -> Json {
    h.state_mut().run("cloud.status", json!({})).unwrap()
}

fn a1(h: &Harness) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0))
}

// ---------------------------------------------------------------- the tests

#[test]
fn the_open_page_lists_my_files_and_what_was_shared_with_me() {
    let (mut h, _fake) = harness(furnished());
    open_page(&mut h);
    assert!(h.query_by_label("My files").is_some(), "the Open page browses your own cloud files");
    assert!(h.query_by_label("Shared with me").is_some(), "and what other people shared with you, beside it");
    assert!(h.query_by_label("Budget.xlsx").is_some());
    assert!(h.query_by_label("Reports").is_some(), "a folder is offered so the user can walk into it");
    assert!(h.query_by_label("exported").is_some(), "no extension, and a content type that says spreadsheet");
    assert!(h.query_by_label("notes.pdf").is_none(), "GridCraft cannot open a PDF, so it is not offered");
    assert!(h.query_by_label("Team.xlsx").is_some());
    let shared = status(&mut h)["shared"].clone();
    assert_eq!(shared[0]["name"], "Team.xlsx");
    assert_eq!(shared[0]["sharedBy"], "Dana", "each incoming share says who shared it");
    // Walking into a folder lists it.
    click(&mut h, "Reports");
    assert_eq!(status(&mut h)["folder"], "/Reports");
    assert!(h.query_by_label("Q3.xlsx").is_some());
    click(&mut h, "Up");
    assert_eq!(status(&mut h)["folder"], "");
}

#[test]
fn shared_with_me_is_hidden_when_the_provider_cannot_do_it() {
    let mut state = furnished();
    state.capabilities = vec![Capability::Files];
    let (mut h, _fake) = harness(state);
    open_page(&mut h);
    assert!(h.query_by_label("My files").is_some());
    assert!(h.query_by_label("Shared with me").is_none(), "a feature the provider does not have is hidden, not shown empty");
    assert!(h.query_by_label("Team.xlsx").is_none());
}

#[test]
fn opening_a_cloud_workbook_loads_its_cells_and_remembers_where_it_came_from() {
    let (mut h, fake) = harness(furnished());
    open_page(&mut h);
    click(&mut h, "Budget.xlsx");
    assert_eq!(a1(&h), Value::Number(42.0), "the bytes went through the ordinary xlsx path");
    assert!(h.state().backstage.is_none(), "a workbook that opened shows the sheet, not the File view");
    let origin = status(&mut h)["origin"].clone();
    assert_eq!(origin["id"], "/Budget.xlsx", "the opaque handle is kept for the save");
    assert_eq!(origin["etag"], "v1", "so is the version it was read at");
    assert_eq!(origin["room"], "room:/Budget.xlsx", "and the collaboration identity, which is not the handle");
    assert_eq!(fake.state.lock().unwrap().reads, vec!["/Budget.xlsx".to_owned()]);
    assert!(h.state().session.active().unwrap().path.is_none(), "a cloud document has no file on this computer");
}

#[test]
fn save_writes_back_under_the_version_it_was_opened_at() {
    let (mut h, fake) = harness(furnished());
    open_page(&mut h);
    click(&mut h, "Budget.xlsx");
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "43"})).unwrap();
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().is_dirty());
    h.state_mut().run("file.save", json!({})).unwrap();
    h.run_steps(4);
    let state = fake.state.lock().unwrap();
    assert_eq!(state.writes.len(), 1, "one save, one write");
    assert_eq!(state.writes[0].0, "/Budget.xlsx");
    assert_eq!(state.writes[0].1, Some("v1".to_owned()), "the etag it was read at is the condition");
    assert!(state.writes[0].2 > 0);
    drop(state);
    assert!(!h.state().session.active().unwrap().is_dirty(), "the version that reached the service is the saved one");
    assert_eq!(status(&mut h)["origin"]["etag"], "v1+", "and the document moves on to the version the service returned");
    // The stored copy really changed.
    let stored = fake.state.lock().unwrap().find("/Budget.xlsx").unwrap().1.clone();
    let (wb, _) = gridcraft_engine::io::open_bytes("Budget.xlsx", &stored).unwrap();
    assert_eq!(wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(43.0));
}

#[test]
fn a_save_that_would_overwrite_somebody_else_is_refused_and_offers_a_reload() {
    let (mut h, fake) = harness(furnished());
    open_page(&mut h);
    click(&mut h, "Budget.xlsx");
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "43"})).unwrap();
    h.run_steps(2);
    // Somebody else saves first.
    fake.state.lock().unwrap().overwrite("/Budget.xlsx", workbook_bytes("500"));
    h.state_mut().run("file.save", json!({})).unwrap();
    h.run_steps(4);
    assert_eq!(fake.state.lock().unwrap().writes.len(), 1, "a refused save is never retried");
    let stored = fake.state.lock().unwrap().find("/Budget.xlsx").unwrap().1.clone();
    let (wb, _) = gridcraft_engine::io::open_bytes("Budget.xlsx", &stored).unwrap();
    assert_eq!(wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(500.0), "their work is still there");
    assert_eq!(status(&mut h)["conflict"]["name"], "Budget.xlsx");
    assert!(h.query_by_label("This workbook was not saved").is_some(), "the user is told, and asked what to do");
    assert_eq!(a1(&h), Value::Number(43.0), "their own edit is still open");
    assert!(h.state().session.active().unwrap().is_dirty(), "and still unsaved");
    // Reloading is explicit, and only then are the local edits replaced.
    click(&mut h, "Reload from the cloud");
    assert_eq!(a1(&h), Value::Number(500.0));
    assert!(!h.state().session.active().unwrap().is_dirty());
    assert_eq!(h.state().session.documents().len(), 1, "the reload replaced the document, it did not add one");
    assert_eq!(status(&mut h)["conflict"], Json::Null);
    assert_eq!(fake.state.lock().unwrap().writes.len(), 1, "and still nothing was written");
}

#[test]
fn save_as_offers_the_cloud_account_beside_this_computer() {
    let (mut h, fake) = harness(furnished());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "11"})).unwrap();
    h.state_mut().run("ui.backstage", json!({"page": "saveAs"})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("This PC").is_some(), "the local destination is still there");
    assert!(h.query_by_label("Ada Lovelace").is_some(), "and the cloud account beside it");
    click(&mut h, "Ada Lovelace");
    click(&mut h, "Reports");
    h.state_mut().cloud.save_name = "Plan".to_owned();
    h.run_steps(2);
    click(&mut h, "Save to the cloud");
    let state = fake.state.lock().unwrap();
    assert_eq!(state.creates.len(), 1);
    assert_eq!(state.creates[0].0, "/Reports", "into the folder that was picked");
    assert_eq!(state.creates[0].1, "Plan.xlsx", "a name typed without an extension is still a workbook");
    drop(state);
    assert!(!h.state().session.active().unwrap().is_dirty(), "the new document belongs to the cloud copy now");
    assert_eq!(status(&mut h)["origin"]["id"], "/Reports/Plan.xlsx");
    // Save now goes back to that entry, with no further questions.
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "12"})).unwrap();
    h.state_mut().run("file.save", json!({})).unwrap();
    h.run_steps(4);
    assert_eq!(fake.state.lock().unwrap().writes[0].0, "/Reports/Plan.xlsx");
}

#[test]
fn no_account_yet_is_an_offer_to_link_and_never_an_error() {
    let mut state = furnished();
    state.linked = false;
    let (mut h, fake) = harness(state);
    open_page(&mut h);
    assert!(h.state().message.is_none(), "not having linked an account yet is not a failure");
    assert_eq!(status(&mut h)["link"], "offered");
    assert_eq!(status(&mut h)["accountError"], Json::Null);
    click(&mut h, "Link a cloud account");
    assert_eq!(fake.opened_urls.lock().unwrap().len(), 1, "the browser is opened for them");
    assert_eq!(status(&mut h)["link"]["waiting"], "https://cloud.example/link/abc");
    // They finish in the browser, and the next poll finds the account there.
    fake.state.lock().unwrap().linked = true;
    h.state_mut().cloud.link =
        Some(cloud::Link::Waiting { url: "https://cloud.example/link/abc".to_owned(), handle: Some("abc".to_owned()), next_poll: 0.0 });
    h.run_steps(6);
    assert_eq!(status(&mut h)["link"], Json::Null);
    assert_eq!(status(&mut h)["account"]["display"], "Ada Lovelace");
    assert!(h.query_by_label("Budget.xlsx").is_some(), "and the files are listed without being asked again");
}

#[test]
fn a_read_only_share_is_never_saved_over() {
    let mut state = furnished();
    if let Some(slot) = state.shared.first_mut() {
        slot.0.writable = false;
    }
    let (mut h, fake) = harness(state);
    open_page(&mut h);
    click(&mut h, "Team.xlsx");
    assert_eq!(a1(&h), Value::Number(99.0));
    // Save has nowhere of its own to go, so it asks, exactly as it does for a
    // workbook that has never been saved.
    h.state_mut().run("file.save", json!({})).unwrap();
    h.run_steps(3);
    assert!(fake.state.lock().unwrap().writes.is_empty(), "nothing was written back over somebody else's file");
    let refused = h.state_mut().run("cloud.save", json!({})).unwrap_err();
    assert!(refused.contains("reading only"), "and saying why names the fix: {refused}");
}

#[test]
fn a_local_workbook_still_saves_to_disk_with_a_cloud_service_wired() {
    let dir = std::env::temp_dir().join(format!("gridcraft-cloud-local-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Local.xlsx");
    let (mut h, fake) = harness(furnished());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
    h.state_mut().run("file.save", json!({"path": path.to_string_lossy()})).unwrap();
    h.run_steps(3);
    assert!(path.exists(), "the local Save path is untouched");
    assert!(fake.state.lock().unwrap().writes.is_empty());
    // And Save keeps going to that file afterwards.
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "6"})).unwrap();
    h.state_mut().run("file.save", json!({})).unwrap();
    h.run_steps(3);
    assert!(fake.state.lock().unwrap().writes.is_empty());
    let (wb, _) = gridcraft_engine::io::open_bytes("Local.xlsx", &std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(6.0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_can_be_opened_by_name_from_the_control_channel() {
    let (mut h, _fake) = harness(furnished());
    h.state_mut().run("cloud.open", json!({})).unwrap();
    h.run_steps(4);
    h.state_mut().run("cloud.openFile", json!({"name": "Budget.xlsx"})).unwrap();
    h.run_steps(4);
    assert_eq!(a1(&h), Value::Number(42.0));
    let refused = h.state_mut().run("cloud.openFile", json!({"name": "notes.pdf"})).unwrap_err();
    assert!(refused.contains("cannot open"), "{refused}");
    assert!(h.state_mut().run("cloud.openFile", json!({"name": "nothing.xlsx"})).is_err());
}

#[test]
fn names_and_paths_are_handled_as_a_user_types_them() {
    assert_eq!(cloud::workbook_name(" Plan "), "Plan.xlsx");
    assert_eq!(cloud::workbook_name("Plan.xlsx"), "Plan.xlsx");
    assert_eq!(cloud::workbook_name("Plan.csv"), "Plan.csv");
    assert_eq!(cloud::workbook_name("Plan.v2"), "Plan.v2.xlsx", "an unknown suffix is part of the name");
    assert_eq!(cloud::workbook_name("/a/b/Plan"), "Plan.xlsx", "a pasted path cannot become a folder");
    assert_eq!(cloud::workbook_name("   "), "");
    assert_eq!(cloud::parent_of(""), None);
    assert_eq!(cloud::parent_of("/Reports"), Some(String::new()));
    assert_eq!(cloud::parent_of("/Reports/2026"), Some("/Reports".to_owned()));
}

#[test]
fn working_together_opens_where_the_provider_says_to_go() {
    let mut state = furnished();
    state.capabilities.push(Capability::Conversation);
    let (mut h, fake) = harness(state);
    h.state_mut().run("cloud.open", json!({})).unwrap();
    h.run_steps(4);
    h.state_mut().run("cloud.openFile", json!({"name": "Budget.xlsx"})).unwrap();
    h.run_steps(4);

    assert!(cloud::can_converse(h.state()), "the capability was reported");
    cloud::work_together(h.state_mut());
    h.run_steps(4);

    let opened = fake.opened_urls.lock().unwrap().clone();
    assert_eq!(opened, vec!["https://cloud.example/call/room".to_owned()]);
    assert_eq!(status(&mut h)["status"], json!("Opened in Talk"));
}

#[test]
fn a_workbook_that_was_never_in_the_cloud_is_asked_to_be_saved_there_first() {
    let mut state = furnished();
    state.capabilities.push(Capability::Conversation);
    let (mut h, fake) = harness(state);

    // A new workbook, which is what the harness starts with. There is no
    // handle to key a conversation on.
    cloud::work_together(h.state_mut());
    h.run_steps(4);

    assert!(fake.opened_urls.lock().unwrap().is_empty(), "nowhere to go, so nothing opened");
    assert_eq!(status(&mut h)["status"], json!("Save this workbook to the cloud first"));
}

#[test]
fn a_provider_with_nowhere_to_talk_never_offers_it() {
    // furnished() reports no conversation capability.
    let (h, _fake) = harness(furnished());
    assert!(!cloud::can_converse(h.state()), "the control has nothing to draw");
}

/// Writes one PNG per state to the directory in `GRIDCRAFT_CLOUD_RENDERS`.
///
/// Skipped without it. The renders are review material for a human being, and
/// the behaviour they illustrate is asserted by the tests above. Starting wgpu
/// in ordinary CI would buy nothing.
#[test]
fn renders_for_review() {
    let Ok(into) = std::env::var("GRIDCRAFT_CLOUD_RENDERS") else {
        eprintln!("set GRIDCRAFT_CLOUD_RENDERS to a directory to write renders");
        return;
    };
    let into = std::path::PathBuf::from(into);
    std::fs::create_dir_all(&into).expect("the render directory must be writable");

    let save = |h: &mut Harness, name: &str| {
        h.run_steps(8);
        let image = h.render().expect("an offscreen renderer is required to write renders");
        let path = into.join(format!("gridcraft-{name}.png"));
        image.save(&path).expect("the render directory must be writable");
        eprintln!("wrote {}", path.display());
    };

    // The file list, where every document carries an overflow menu.
    // One provider that does everything, built twice. Two windows are needed
    // because one of them has a workbook open and the other is browsing.
    let talkative = || {
        let mut state = furnished();
        state.capabilities.push(Capability::Conversation);
        state
    };
    let (mut h, _fake) = rendering(talkative());
    open_page(&mut h);
    save(&mut h, "open-page");

    // The same page with one of those menus open.
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "More options").next().expect("a row offers its menu").click();
    save(&mut h, "open-page-overflow");

    // The title bar: the account, who else is here, and Share as a split
    // button with the chevron pressed.
    let (mut h, _fake) = rendering(talkative());
    h.state_mut().run("cloud.open", json!({})).unwrap();
    h.run_steps(4);
    h.state_mut().run("cloud.openFile", json!({"name": "Budget.xlsx"})).unwrap();
    h.run_steps(4);
    let peers = ["Dana Okafor", "Priya Raman", "Tomás Alvarez", "Wen Li", "Sam Iredale", "Rosa Beltrán"];
    h.state_mut().presence.set(peers.iter().enumerate().map(|(n, who)| craft_ui::presence::Peer::new(format!("peer-{n}"), *who)).collect());
    save(&mut h, "title-bar-presence");
    h.get_by_role_and_label(egui::accesskit::Role::Button, "More share options").click();
    save(&mut h, "share-split-menu");

    // And the dialog the primary half opens, with real access on it.
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Share").click();
    save(&mut h, "share-dialog");
}

#[test]
fn a_workbook_goes_back_in_the_format_its_name_asks_for() {
    // `exported` in the fixture is a CSV the provider reported as text/csv.
    // Saving it as XLSX under that name would hand somebody a file their
    // spreadsheet refuses to open, and nothing would say why.
    let (mut h, fake) = harness(furnished());
    h.state_mut().run("cloud.open", json!({})).unwrap();
    h.run_steps(4);
    h.state_mut().run("cloud.openFile", json!({"name": "exported"})).unwrap();
    h.run_steps(4);
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "9"})).unwrap();
    h.run_steps(2);
    h.state_mut().run("cloud.save", json!({})).unwrap();
    h.run_steps(6);

    let saved = fake.state.lock().unwrap().files.iter().find(|(e, _)| e.name == "exported").map(|(_, b)| b.clone()).expect("the file is there");
    assert!(saved.starts_with(b"9"), "a CSV starts with its first cell, not a zip header: {:?}", &saved[..saved.len().min(8)]);
    assert!(!saved.starts_with(b"PK"), "XLSX bytes under a CSV name is the defect this test exists for");
}
