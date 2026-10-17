//! What the embedding page sends, as plain JSON (no browser types, so it is tested natively):
//! workbooks to open (`gridcraft:open`) and control requests (`gridcraft:control`) wait in one
//! queue, in the order they arrived, and run on the app in that order after its per-frame logic.
//! Each request's answer becomes a `gridcraft:reply` message. `host` moves the messages in and
//! out of JavaScript.
//!
//! ```text
//! { type: "gridcraft:control", id: 7, method: "cell.get", params: { cell: "A1" } }
//! { type: "gridcraft:reply", id: 7, ok: true, result: { … } }
//! { type: "gridcraft:reply", id: 7, ok: false, error: "…" }
//! ```
//!
//! The methods are the desktop control channel's (`docs/control-protocol.md`). A message that
//! can't be read is answered with an error, under the id read from it beforehand (`null` when it
//! has none).

use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::{ControlResponse, SheetApp};
use serde_json::{Value as Json, json};

/// The `type` of a request from the page.
pub const CONTROL: &str = "gridcraft:control";
/// The `type` of an answer to the page.
pub const REPLY: &str = "gridcraft:reply";
/// The largest control message heard: 16 MiB of JSON text (UTF-8). Workbooks go as
/// `gridcraft:open` bytes, which this doesn't limit.
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;
/// The answer to a request over [`MAX_MESSAGE`].
pub const TOO_LARGE: &str = "request too large (the limit is 16 MiB of JSON)";

/// A request from the page.
#[derive(Debug, PartialEq)]
pub struct Call {
    /// Echoed in the reply: a number or a string.
    pub id: Json,
    pub method: String,
    /// Always an object (`{}` when the message had none).
    pub params: Json,
}

/// Reads a `gridcraft:control` message's JSON text. `id` is the message's id read beforehand
/// (`null` without a usable one): it labels the answer when the text itself can't be read. `Err`
/// is the answer that says why the request can't be served.
pub fn request(text: &str, id: Json) -> Result<Call, Json> {
    let id = if matches!(id, Json::Number(_) | Json::String(_)) { id } else { Json::Null };
    if text.len() > MAX_MESSAGE {
        return Err(error(id, TOO_LARGE));
    }
    let Ok(Json::Object(mut message)) = serde_json::from_str::<Json>(text) else {
        return Err(error(id, "a control message is a JSON object"));
    };
    if message.get("type").and_then(Json::as_str) != Some(CONTROL) {
        return Err(error(id, "not a `gridcraft:control` message"));
    }
    let id = match message.remove("id") {
        Some(id @ (Json::Number(_) | Json::String(_))) => id,
        _ => return Err(error(Json::Null, "missing `id` (a number or a string)")),
    };
    let method = match message.remove("method") {
        Some(Json::String(method)) if !method.is_empty() => method,
        _ => return Err(error(id, "missing `method` (a string)")),
    };
    let params = match message.remove("params") {
        None | Some(Json::Null) => json!({}),
        Some(params @ Json::Object(_)) => params,
        Some(_) => return Err(error(id, "`params` must be an object")),
    };
    Ok(Call { id, method, params })
}

/// The page's answer for `id` from the app's `{ok, result}` or `{ok: false, error}`.
pub fn reply(id: Json, mut response: ControlResponse) -> Json {
    let ok = response.get("ok").and_then(Json::as_bool) == Some(true);
    let fields = response.as_object_mut();
    if !ok {
        let message = fields.and_then(|f| f.remove("error")).and_then(|e| e.as_str().map(str::to_string));
        return error(id, message.as_deref().unwrap_or("the request failed"));
    }
    let result = fields.and_then(|f| f.remove("result")).unwrap_or(Json::Null);
    json!({"type": REPLY, "id": id, "ok": true, "result": result})
}

/// An error answer for `id`.
pub fn error(id: Json, message: &str) -> Json {
    json!({"type": REPLY, "id": id, "ok": false, "error": message})
}

/// The answer to `ui.screenshot`, which waits for the desktop window's clock: the web build has
/// none, so the picture would never be taken.
const NO_SCREENSHOT: &str = "`ui.screenshot` isn't available in the browser";

/// Why the browser can't serve `method`, if it can't.
pub fn unavailable(method: &str) -> Option<&'static str> {
    match method {
        "ui.screenshot" => Some(NO_SCREENSHOT),
        _ => None,
    }
}

enum Item {
    Open { name: String, bytes: Vec<u8> },
    Control(Call),
}

/// What the page asked for, in the order it asked. A request sent right after a workbook works
/// on that workbook, whatever frame each arrives in.
#[derive(Default)]
pub struct Queue {
    items: Vec<Item>,
}

impl Queue {
    /// A workbook to open (`gridcraft:open`).
    pub fn open(&mut self, name: String, bytes: Vec<u8>) {
        self.items.push(Item::Open { name, bytes });
    }

    /// A control message's JSON text, with its id read beforehand (see [`request`]). Returns the
    /// answer to send at once when the request can't be served; otherwise it waits its turn.
    pub fn control(&mut self, text: &str, id: Json) -> Option<Json> {
        let call = match request(text, id) {
            Ok(call) => call,
            Err(reply) => return Some(reply),
        };
        if let Some(why) = unavailable(&call.method) {
            return Some(error(call.id, why));
        }
        self.items.push(Item::Control(call));
        None
    }

    /// Everything queued so far; the queue is left empty.
    pub fn take(&mut self) -> Queue {
        Queue { items: std::mem::take(&mut self.items) }
    }

    /// Runs the queue on `app`, in order: a workbook opens as one picked with File › Open does,
    /// a request runs as it would over the desktop channel. Returns the answers, in order: every
    /// request is answered within this call.
    pub fn run(self, app: &mut SheetApp, ctx: &egui::Context) -> Vec<Json> {
        let mut replies = Vec::new();
        for item in self.items {
            match item {
                Item::Open { name, bytes } => app.open_bytes(&name, &bytes),
                Item::Control(call) => replies.push(match control::handle(app, ctx, &call.method, &call.params) {
                    Outcome::Done(response) => reply(call.id, response),
                    // Refused when heard (see `unavailable`); still an answer, never a wait.
                    Outcome::Screenshot { .. } => error(call.id, NO_SCREENSHOT),
                }),
            }
        }
        replies
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;
    use gridcraft_ui_egui::Services;

    fn message(id: Json, method: Json, params: Json) -> String {
        json!({"type": CONTROL, "id": id, "method": method, "params": params}).to_string()
    }

    fn error_text(reply: &Json) -> &str {
        assert_eq!(reply["type"], REPLY, "{reply}");
        assert_eq!(reply["ok"], false, "{reply}");
        reply["error"].as_str().unwrap_or_default()
    }

    /// The app as the web shell has it after its first frame: one blank workbook.
    fn app() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        SheetApp::new(session, Services::default())
    }

    /// A workbook with `a1` in A1, as file bytes.
    fn workbook(a1: &str) -> Vec<u8> {
        let mut s = Session::new();
        s.new_workbook();
        s.run("cell.set", json!({"cell": "A1", "input": a1})).unwrap();
        gridcraft_engine::io::save_bytes(&s.active().unwrap().wb, "book.xlsx").unwrap()
    }

    fn a1(app: &mut SheetApp) -> Json {
        match control::handle(app, &egui::Context::default(), "cell.get", &json!({"cell": "A1"})) {
            Outcome::Done(reply) => reply["result"]["value"].clone(),
            Outcome::Screenshot { .. } => Json::Null,
        }
    }

    #[test]
    fn a_control_message_becomes_a_request() {
        let call = request(&message(json!(7), json!("cell.get"), json!({"cell": "A1"})), json!(7)).unwrap();
        assert_eq!(call, Call { id: json!(7), method: "cell.get".into(), params: json!({"cell": "A1"}) });
        // A string id is fine, and missing or null params read as `{}`.
        let call = request(r#"{"type":"gridcraft:control","id":"a-1","method":"document.inspect"}"#, json!("a-1")).unwrap();
        assert_eq!((call.id, call.params), (json!("a-1"), json!({})));
        let call = request(&message(json!(2), json!("ui.inspect"), Json::Null), json!(2)).unwrap();
        assert_eq!(call.params, json!({}));
    }

    #[test]
    fn replies_carry_the_id_and_the_outcome() {
        assert_eq!(
            reply(json!(7), json!({"ok": true, "result": {"value": 42}})),
            json!({"type": "gridcraft:reply", "id": 7, "ok": true, "result": {"value": 42}})
        );
        assert_eq!(reply(json!("x"), json!({"ok": true})), json!({"type": "gridcraft:reply", "id": "x", "ok": true, "result": null}));
        assert_eq!(
            reply(json!(7), json!({"ok": false, "error": "unknown method `nope`"})),
            json!({"type": "gridcraft:reply", "id": 7, "ok": false, "error": "unknown method `nope`"})
        );
        // An answer that isn't the app's shape still becomes an error, never a success.
        assert_eq!(error_text(&reply(json!(1), json!("garbage"))), "the request failed");
        assert_eq!(error_text(&reply(json!(1), json!({"ok": false, "error": 3}))), "the request failed");
    }

    #[test]
    fn a_malformed_message_is_answered_with_an_error() {
        let no_id = request(r#"{"type":"gridcraft:control","method":"cell.get"}"#, Json::Null).unwrap_err();
        assert_eq!(no_id["id"], Json::Null);
        assert!(error_text(&no_id).contains("`id`"));
        let bad_method = request(&message(json!(3), json!(42), json!({})), json!(3)).unwrap_err();
        assert_eq!(bad_method["id"], 3, "the id is kept once it was read");
        assert!(error_text(&bad_method).contains("`method`"));
        let bad_params = request(&message(json!(4), json!("cell.get"), json!(["A1"])), json!(4)).unwrap_err();
        assert_eq!(bad_params["id"], 4);
        assert!(error_text(&bad_params).contains("`params`"));
        for text in [
            "",
            "not json",
            "[1, 2]",
            "null",
            r#"{"type":"gridcraft:open","id":1,"method":"cell.get"}"#,
            r#"{"type":"gridcraft:control","id":true,"method":"cell.get"}"#,
            r#"{"type":"gridcraft:control","id":null,"method":"cell.get"}"#,
            r#"{"type":"gridcraft:control","id":1,"method":""}"#,
            r#"{"type":"gridcraft:control","id":1}"#,
        ] {
            let reply = request(text, Json::Null).unwrap_err();
            assert!(!error_text(&reply).is_empty(), "{text}");
        }
        // Text that can't be read keeps the id the page gave, read before parsing.
        assert_eq!(request("{", json!(5)).unwrap_err()["id"], 5);
        // Deep nesting is refused by the parser instead of overflowing the stack.
        let deep = format!(r#"{{"type":"gridcraft:control","id":1,"method":"x","params":{{"a":{}0{}}}}}"#, "[".repeat(100_000), "]".repeat(100_000));
        assert!(request(&deep, json!(1)).is_err());
    }

    #[test]
    fn a_message_over_16_mib_is_refused_unread() {
        let head = r#"{"type":"gridcraft:control","id":"big","method":"cell.set","params":{"cell":"A1","input":""#;
        let tail = r#""}}"#;
        let mut text = String::with_capacity(head.len() + MAX_MESSAGE + tail.len());
        text.push_str(head);
        text.extend(std::iter::repeat_n('x', MAX_MESSAGE));
        text.push_str(tail);
        let reply = request(&text, json!("big")).unwrap_err();
        assert_eq!(reply["id"], "big", "answered under the id read beforehand");
        assert_eq!(error_text(&reply), TOO_LARGE);
    }

    #[test]
    fn opens_and_requests_run_in_the_order_they_arrived() {
        let mut app = app();
        let mut queue = Queue::default();
        queue.open("Sent.xlsx".into(), workbook("1"));
        assert!(queue.control(&message(json!(1), json!("cell.set"), json!({"cell": "A1", "input": "5"})), json!(1)).is_none());
        assert!(queue.control(&message(json!(2), json!("cell.get"), json!({"cell": "A1"})), json!(2)).is_none());
        let replies = queue.take().run(&mut app, &egui::Context::default());
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert_eq!((replies[0]["id"].clone(), replies[0]["ok"].clone()), (json!(1), json!(true)));
        assert_eq!(replies[1]["result"]["value"].as_f64(), Some(5.0), "the edit went to the workbook opened before it");
        let docs = app.session.documents();
        assert_eq!(docs.len(), 1, "the sent workbook replaced the untouched Book1");
        assert_eq!(docs[0].display_title(), "Sent.xlsx");
        assert!(docs[0].is_dirty());
    }

    #[test]
    fn a_request_sent_before_an_open_applies_to_the_workbook_open_then() {
        let mut app = app();
        let mut queue = Queue::default();
        assert!(queue.control(&message(json!(1), json!("cell.set"), json!({"cell": "A1", "input": "9"})), json!(1)).is_none());
        queue.open("Sent.xlsx".into(), workbook("1"));
        let replies = queue.take().run(&mut app, &egui::Context::default());
        assert_eq!(replies.len(), 1);
        assert_eq!(app.session.documents().len(), 2, "Book1 was edited, so the sent workbook opens beside it");
        assert_eq!(app.session.active().map(|d| d.display_title()).as_deref(), Some("Sent.xlsx"));
        assert_eq!(a1(&mut app).as_f64(), Some(1.0));
        assert!(app.session.documents()[0].is_dirty(), "the edit stayed in Book1");
    }

    #[test]
    fn each_request_is_answered_once_in_order() {
        let mut app = app();
        let mut queue = Queue::default();
        queue.control(&message(json!("a"), json!("cell.set"), json!({"cell": "B2", "input": "x"})), json!("a"));
        queue.control(&message(json!("b"), json!("no.such.method"), json!({})), json!("b"));
        let replies = queue.take().run(&mut app, &egui::Context::default());
        assert_eq!(replies.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(), [json!("a"), json!("b")]);
        assert_eq!(replies[0]["ok"], true);
        assert!(error_text(&replies[1]).contains("no.such.method"));
        assert!(queue.take().run(&mut app, &egui::Context::default()).is_empty(), "nothing is answered twice");
    }

    #[test]
    fn unreadable_or_unavailable_requests_are_answered_when_heard() {
        let mut queue = Queue::default();
        let reply = queue.control("{", json!(4)).unwrap();
        assert_eq!(reply["id"], 4);
        let reply = queue.control(&message(json!(5), json!("ui.screenshot"), json!({})), json!(5)).unwrap();
        assert_eq!(reply["id"], 5);
        assert!(error_text(&reply).contains("ui.screenshot"));
        assert!(queue.take().run(&mut app(), &egui::Context::default()).is_empty(), "the app never saw them");
    }

    #[test]
    fn only_screenshots_are_unavailable_in_the_browser() {
        assert!(unavailable("ui.screenshot").is_some());
        for method in ["engine.execute", "cell.get", "app.open", "file.saveBytes", "ui.click"] {
            assert!(unavailable(method).is_none(), "{method}");
        }
    }
}
