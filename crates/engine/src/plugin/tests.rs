//! Plug-in runs against a real session, with modules assembled from WebAssembly text.

use gridcraft_core::{CellRef, Value as CellValue};
use serde_json::json;

use crate::Session;
use crate::plugin::craft_plugin::bundle::Bundle;

/// A module that calls `calls` (command, params JSON) in order. A refused or failed call fails the
/// run when `strict`; otherwise its message becomes the output and the run goes on. `tail` is WAT
/// run after the calls (a trap, an endless loop), before returning 0.
fn module(calls: &[(&str, &str)], strict: bool, tail: &str) -> Vec<u8> {
    let mut data = String::new();
    let mut body = String::new();
    let mut at = 0usize;
    for (command, params) in calls {
        let (c, p) = (at, at + command.len());
        at = p + params.len();
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        data.push_str(&format!(r#"(data (i32.const {c}) "{}") (data (i32.const {p}) "{}")"#, esc(command), esc(params)));
        let on_error = if strict {
            "(return (i32.const 1))".to_string()
        } else {
            "(call $set_output (i32.const 32768) (call $reply_read (i32.const 32768) (call $reply_len)))".to_string()
        };
        body.push_str(&format!(
            "(if (i32.ne (call $call (i32.const {c}) (i32.const {}) (i32.const {p}) (i32.const {})) (i32.const 0)) (then {on_error}))\n",
            command.len(),
            params.len()
        ));
    }
    let wat = format!(
        r#"(module
          (import "craft" "call" (func $call (param i32 i32 i32 i32) (result i32)))
          (import "craft" "reply_len" (func $reply_len (result i32)))
          (import "craft" "reply_read" (func $reply_read (param i32 i32) (result i32)))
          (import "craft" "set_output" (func $set_output (param i32 i32)))
          (memory (export "memory") 2)
          {data}
          (func (export "craft_run") (param $n i32) (result i32) {body} {tail} (i32.const 0)))"#
    );
    wat::parse_str(&wat).unwrap_or_else(|e| panic!("{e}\n{wat}"))
}

/// The `.craftplugin` of plug-in `id` for GridCraft, asking for `access`.
fn bundle(id: &str, access: &str, wasm: Vec<u8>) -> Vec<u8> {
    let manifest = json!({
        "id": id, "name": "Test Plug-in", "version": "1.0.0", "main": "plugin.wasm",
        "apps": {"gridcraft": {"api": 6}},
        "permissions": {"access": access},
        "commands": [{"id": "go", "label": "Go"}]
    });
    Bundle { manifest: manifest.to_string(), wasm, assets: vec![] }.encode()
}

fn base64(bytes: &[u8]) -> String {
    crate::io::base64_encode(bytes)
}

/// A session with a workbook and plug-in `id` installed for the session.
fn with_plugin(id: &str, access: &str, wasm: Vec<u8>) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let r = s.execute("plugin.install", json!({"data": base64(&bundle(id, access, wasm)), "name": "test.craftplugin"})).unwrap();
    assert_eq!(r["installed"], true);
    assert_eq!(r["stored"], false, "without a store, a plug-in is kept for the session");
    s
}

fn value(s: &Session, a1: &str) -> CellValue {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().undo.len()
}

fn output(r: &serde_json::Value) -> String {
    r["output"].as_str().unwrap_or_default().to_string()
}

const SET_A1: (&str, &str) = ("cell.set", r#"{"cell":"A1","input":"42"}"#);
const SET_B2: (&str, &str) = ("cell.set", r#"{"cell":"B2","input":"=A1*2"}"#);

#[test]
fn a_write_plugin_runs_real_commands_as_one_undo_step_named_after_it() {
    let mut s = with_plugin("io.example.cells", "write", module(&[SET_A1, SET_B2], true, ""));
    let undo = undo_len(&s);
    let journal = s.journal.len();
    let r = s.execute("plugin.run", json!({"id": "io.example.cells"})).unwrap();
    assert_eq!(r["calls"], 2);
    assert_eq!(value(&s, "A1"), CellValue::Number(42.0));
    assert_eq!(value(&s, "B2"), CellValue::Number(84.0));
    let history = &s.doc().unwrap().undo;
    assert_eq!(history.len(), undo + 1, "two commands, one undo step");
    assert_eq!(history.last().unwrap().label, "Test Plug-in");
    let journaled: Vec<&str> = s.journal[journal..].iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(journaled, ["plugin.run"], "the run is journaled, not the commands it ran");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!((value(&s, "A1"), value(&s, "B2")), (CellValue::Empty, CellValue::Empty), "one Undo takes the whole run back");
}

#[test]
fn a_read_plugin_that_edits_is_put_back_and_refused() {
    let read = ("cell.get", r#"{"cell":"A1"}"#);
    let mut s = with_plugin("io.example.reader", "read", module(&[read, SET_A1], false, ""));
    let undo = undo_len(&s);
    let wb = s.doc().unwrap().wb.clone();
    let r = s.execute("plugin.run", json!({"id": "io.example.reader"})).unwrap();
    assert!(output(&r).contains("may only read it"), "{r}");
    assert_eq!(value(&s, "A1"), CellValue::Empty);
    assert!(std::sync::Arc::ptr_eq(&wb, &s.doc().unwrap().wb), "the workbook is the one from before the call");
    assert_eq!(undo_len(&s), undo, "nothing to undo");
}

#[test]
fn refused_commands_and_file_paths_are_refused() {
    for (call, why) in [
        (("file.save", "{}"), "isn't available to plugins"),
        (("file.open", r#"{"path":"/etc/hosts"}"#), "isn't available to plugins"),
        (("file.exportCsv", "{}"), "isn't available to plugins"),
        (("edit.undo", "{}"), "isn't available to plugins"),
        (("edit.redo", "{}"), "isn't available to plugins"),
        (("edit.copy", r#"{"range":"A1"}"#), "isn't available to plugins"),
        (("edit.paste", r#"{"at":"A1"}"#), "isn't available to plugins"),
        (("data.getData", r#"{"text":"a,b"}"#), "isn't available to plugins"),
        (("data.refreshAll", "{}"), "isn't available to plugins"),
        (("data.queriesConnections", "{}"), "isn't available to plugins"),
        (("macro.run", r#"{"name":"M"}"#), "isn't available to plugins"),
        (("automate.runScript", r#"{"name":"S"}"#), "isn't available to plugins"),
        (("python.set", r#"{"cell":"A1","code":"1"}"#), "isn't available to plugins"),
        (("file.options", "{}"), "isn't available to plugins"),
        (("review.addToDictionary", r#"{"word":"x"}"#), "isn't available to plugins"),
        (("window.activate", "{}"), "isn't available to plugins"),
        (("link.open", r#"{"url":"https://example.com"}"#), "isn't available to plugins"),
        (("review.protectSheet", r#"{"password":"x"}"#), "isn't available to plugins"),
        (("plugin.list", "{}"), "isn't available to plugins"),
        (("plugin.install", r#"{"repo":"example/tools"}"#), "isn't available to plugins"),
        (("home.bold", r#"{"range":"A1","path":"/etc/hosts"}"#), "can't name files"),
    ] {
        let mut s = with_plugin("io.example.sneaky", "write", module(&[call], false, ""));
        let r = s.execute("plugin.run", json!({"id": "io.example.sneaky"})).unwrap();
        assert!(output(&r).contains(why), "{}: {r}", call.0);
    }
}

#[test]
fn a_command_outside_the_manifest_allowlist_is_refused() {
    let mut s = Session::new();
    s.new_workbook();
    let manifest = json!({"id": "io.example.narrow", "name": "Narrow", "version": "1.0.0", "main": "p.wasm",
        "apps": {"gridcraft": {"api": 6}}, "permissions": {"access": "write", "commands": ["home.*"]}});
    let bytes = Bundle { manifest: manifest.to_string(), wasm: module(&[SET_A1], false, ""), assets: vec![] }.encode();
    s.execute("plugin.install", json!({"data": base64(&bytes)})).unwrap();
    let r = s.execute("plugin.run", json!({"id": "io.example.narrow"})).unwrap();
    assert!(output(&r).contains("asked permission for"), "{r}");
    assert_eq!(value(&s, "A1"), CellValue::Empty);
}

#[test]
fn a_trapping_or_endless_plugin_leaves_the_workbook_unchanged() {
    for (tail, fuel) in [("unreachable", None), ("(loop $l (br $l))", Some(200_000))] {
        let mut s = with_plugin("io.example.crash", "write", module(&[SET_A1, SET_B2], true, tail));
        let (wb, undo, journal) = (s.doc().unwrap().wb.clone(), undo_len(&s), s.journal.len());
        let mut params = json!({"id": "io.example.crash"});
        if let Some(f) = fuel {
            params["fuel"] = json!(f);
        }
        let e = s.execute("plugin.run", params).unwrap_err().to_string();
        assert!(e.contains("Test Plug-in"), "{e}");
        assert!(std::sync::Arc::ptr_eq(&wb, &s.doc().unwrap().wb), "{tail}: the edits before the failure are rolled back");
        assert_eq!(undo_len(&s), undo);
        assert_eq!(s.journal.len(), journal, "the rolled-back calls aren't journaled");
        assert_eq!(s.journal_paused, 0);
    }
}

#[test]
fn a_dialog_command_is_refused_and_put_back() {
    // Format Cells with no style asks for its dialog.
    let mut s = with_plugin("io.example.dialog", "write", module(&[("home.formatCells", "{}")], false, ""));
    let r = s.execute("plugin.run", json!({"id": "io.example.dialog"})).unwrap();
    assert!(output(&r).contains("opens a dialog"), "{r}");
    assert!(s.ui_requests.is_empty(), "nothing is left for the UI: {:?}", s.ui_requests);
}

#[test]
fn a_run_cannot_write_a_whole_column_or_make_unbounded_edits() {
    // A whole column at once, by range or by the selection, is refused before it runs.
    for calls in [
        vec![("range.fill", r#"{"range":"A:A","input":"1"}"#)],
        vec![("selection.set", r#"{"range":"B:B"}"#), ("edit.fillDown", "{}")],
        vec![("edit.autoFill", r#"{"source":"A1","target":"A1:A1048576"}"#)],
    ] {
        let mut s = with_plugin("io.example.flood", "write", module(&calls, false, ""));
        let undo = undo_len(&s);
        let r = s.execute("plugin.run", json!({"id": "io.example.flood"})).unwrap();
        assert!(output(&r).contains("may write at most"), "{calls:?}: {r}");
        assert!(undo_len(&s) <= undo + 1);
        assert_eq!(value(&s, "A1048576"), CellValue::Empty);
    }
    // Many small edits: the run stops at the cap.
    let edits: Vec<(&str, String)> =
        (0..super::MAX_RUN_EDITS + 5).map(|i| ("cell.set", format!(r#"{{"cell":"C{}","input":"{i}"}}"#, i + 1))).collect();
    let calls: Vec<(&str, &str)> = edits.iter().map(|(c, p)| (*c, p.as_str())).collect();
    let mut s = with_plugin("io.example.many", "write", module(&calls, false, ""));
    let r = s.execute("plugin.run", json!({"id": "io.example.many"})).unwrap();
    assert!(output(&r).contains(&format!("at most {}", super::MAX_RUN_EDITS)), "{r}");
    assert_eq!(value(&s, &format!("C{}", super::MAX_RUN_EDITS)), CellValue::Number((super::MAX_RUN_EDITS - 1) as f64));
    assert_eq!(value(&s, &format!("C{}", super::MAX_RUN_EDITS + 1)), CellValue::Empty);
    assert_eq!(s.doc().unwrap().undo.len(), 1, "still one step");
}

// A run stays inside the undo history.
const _: () = assert!(super::MAX_RUN_EDITS < crate::UNDO_LIMIT);

#[test]
fn plugin_calls_cannot_read_files() {
    // Even a command that would read a file under another parameter name runs in the sandbox.
    let _sandbox = crate::io::ScriptSandbox::enter();
    assert!(crate::io::read_file("/etc/hosts").is_err());
}

#[test]
fn every_allowed_command_exists_and_none_reaches_outside_the_workbook() {
    for id in super::ALLOWED.iter().chain(super::VALUE_WRITERS) {
        assert!(crate::find_command(id).is_some(), "{id}");
        assert!(super::ALLOWED.contains(id), "{id} is a value writer but not allowed");
    }
    for refused in [
        "file.save",
        "file.properties",
        "data.getData",
        "data.fromTextCsv",
        "data.fromTableRange",
        "data.refreshAll",
        "macro.run",
        "automate.runScript",
        "python.set",
        "control.press",
        "insert.picture",
        "pageLayout.background",
        "edit.copy",
        "edit.paste",
        "edit.undo",
        "history.list",
        "file.options",
        "view.statusBar",
        "link.open",
        "sheet.move",
        "app.commands",
        "plugin.run",
    ] {
        assert!(super::policy().refuses(refused), "{refused}");
    }
    // A command added later is refused until it is reviewed and listed.
    assert!(super::policy().refuses("some.newCommand"));
}

#[test]
fn macros_and_scripts_cannot_run_or_install_plugins() {
    use crate::macros::policy::{Policy, macro_policy};
    for id in ["plugin.run", "plugin.install", "plugin.remove"] {
        assert!(matches!(macro_policy(id, &json!({}), crate::Trust::Local), Policy::Deny(_)), "{id}");
    }
    let mut s = with_plugin("io.example.cells", "write", module(&[SET_A1], true, ""));
    s.execute("automate.newScript", json!({"name": "p", "commands": [{"command": "plugin.run", "params": {"id": "io.example.cells"}}]})).unwrap();
    let e = s.execute("automate.runScript", json!({"name": "p"})).unwrap_err().to_string();
    assert!(e.contains("plugin.run"), "{e}");
    assert_eq!(value(&s, "A1"), CellValue::Empty);
}

#[test]
fn settings_are_bounded_and_kept_per_plugin() {
    let mut all = Default::default();
    super::set_setting(&mut all, "a", "k", Some("{\"x\": 1}")).unwrap();
    assert_eq!(all["a"]["k"], json!({"x": 1}));
    assert!(super::set_setting(&mut all, "a", &"k".repeat(129), Some("1")).is_err());
    assert!(super::set_setting(&mut all, "a", "big", Some(&format!("\"{}\"", "x".repeat(64 * 1024)))).is_err());
    assert!(super::set_setting(&mut all, "a", "bad", Some("not json")).is_err());
    super::set_setting(&mut all, "a", "k", None).unwrap();
    assert!(!all.contains_key("a"));
}

#[test]
fn install_review_list_and_remove_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new();
    s.plugins.open_store(dir.path());
    let bytes = bundle("io.example.stored", "write", module(&[], true, ""));
    let data = base64(&bytes);
    // The confirmation: described, nothing written.
    let review = s.execute("plugin.install", json!({"data": data, "review": true})).unwrap();
    assert_eq!(review["installed"], false);
    assert_eq!(review["permissions"]["access"], "write");
    assert_eq!(review["kind"], "command");
    assert_eq!(review["sha256"], crate::plugin::craft_plugin::sha256_hex(&bytes));
    assert!(s.plugins.list().is_empty());
    assert!(std::fs::read_dir(dir.path()).map_or(true, |mut d| d.next().is_none()), "nothing is written before the install");
    // Another bundle than the one reviewed isn't installed.
    let e = s.execute("plugin.install", json!({"data": data, "sha256": "0".repeat(64)})).unwrap_err();
    assert!(e.to_string().contains("reviewed"), "{e}");
    s.execute("plugin.install", json!({"data": data, "review": true})).unwrap();
    let r = s.execute("plugin.install", json!({"data": data, "sha256": review["sha256"]})).unwrap();
    assert_eq!((r["installed"].clone(), r["stored"].clone()), (json!(true), json!(true)));
    // A new session reads it back from the folder.
    let mut again = Session::new();
    again.plugins.open_store(dir.path());
    let list = again.execute("plugin.list", json!({})).unwrap();
    assert_eq!(list["plugins"][0]["id"], "io.example.stored");
    assert_eq!(list["plugins"][0]["source"]["type"], "file");
    assert_eq!(list["stored"], true);
    again.execute("plugin.remove", json!({"id": "io.example.stored"})).unwrap();
    assert_eq!(again.execute("plugin.list", json!({})).unwrap()["plugins"], json!([]));
    let mut third = Session::new();
    third.plugins.open_store(dir.path());
    assert!(third.plugins.list().is_empty(), "removed from the folder");
}

#[test]
fn plugins_for_other_apps_or_not_bundles_are_refused() {
    let mut s = Session::new();
    let manifest = json!({"id": "io.example.other", "name": "Other", "version": "1.0.0", "main": "p.wasm", "apps": {"figcraft": {"api": 6}}});
    let bytes = Bundle { manifest: manifest.to_string(), wasm: module(&[], true, ""), assets: vec![] }.encode();
    let e = s.execute("plugin.install", json!({"data": base64(&bytes)})).unwrap_err().to_string();
    assert!(e.contains("figcraft"), "{e}");
    let e = s.execute("plugin.install", json!({"data": base64(b"not a bundle")})).unwrap_err().to_string();
    assert!(e.contains("bundle"), "{e}");
    assert!(s.execute("plugin.install", json!({})).is_err());
}

#[test]
fn a_local_catalog_installs_its_package() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = bundle("io.example.listed", "write", module(&[], true, ""));
    let file = dir.path().join("listed.craftplugin");
    std::fs::write(&file, &bytes).unwrap();
    let url = |p: &std::path::Path| format!("file://{}", p.to_string_lossy().replace('\\', "/"));
    let index = json!({"schema": 1, "packages": [{"id": "io.example.listed", "name": "Listed", "repo": "https://github.com/example/listed",
        "versions": [{"version": "1.0.0", "apps": {"gridcraft": {"api": 6}}, "url": url(&file),
            "sha256": crate::plugin::craft_plugin::sha256_hex(&bytes), "size": bytes.len()}]}]});
    let catalog = dir.path().join("index.json");
    std::fs::write(&catalog, index.to_string()).unwrap();
    let mut s = Session::new();
    s.execute("plugin.catalog.add", json!({"url": url(&catalog)})).unwrap();
    let listed = s.execute("plugin.catalog.list", json!({})).unwrap();
    assert_eq!(listed["packages"][0]["id"], "io.example.listed", "{listed}");
    let r = s.execute("plugin.install", json!({"catalog": url(&catalog), "id": "io.example.listed"})).unwrap();
    assert_eq!(r["inCatalog"], true);
    assert_eq!(r["source"]["type"], "catalog");
    assert_eq!(s.execute("plugin.update", json!({})).unwrap()["updates"], json!([]), "up to date");
    assert!(s.execute("plugin.catalog.add", json!({"url": "https://example.com/index.json"})).is_err(), "GitHub's hosts only");
}

#[test]
fn a_repository_install_without_network_says_so() {
    let mut s = Session::new();
    let e = s.execute("plugin.install", json!({"repo": "example/tools"})).unwrap_err().to_string();
    assert!(e.contains("no network access"), "{e}");
}
