//! `gridcraft-cli mcp --automation-read-root DIR --automation-write-root DIR --allow-commands LIST`:
//! through the MCP protocol only, workbooks open from inside the read root and are saved and
//! exported only inside the write root; paths that leave a root are refused and nothing is
//! written outside. Only listed commands run, whether asked for directly, through a dedicated
//! tool (`write_range`, `format_range`, `save_workbook`…) or as the steps of a stored script.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gridcraft_engine::automation::{Automation, CommandAllowlist};
use gridcraft_mcp::{Headless, Server};
use serde_json::{Value, json};

/// A fresh folder with `in` (read root, holding `data.csv`), `out` (write root), `in2` (a sibling
/// sharing the prefix) and `outside` (holding `secret.csv`).
fn folders(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("gc-mcp-auto-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    for d in ["in", "out", "in2", "outside"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    std::fs::write(base.join("in/data.csv"), "Item,Cost\nRent,1200\nFood,400\n").unwrap();
    std::fs::write(base.join("outside/secret.csv"), "Secret,1\n").unwrap();
    std::fs::write(base.join("in2/data.csv"), "Sibling,1\n").unwrap();
    base
}

/// A headless server confined to `base/in` (read), `base/out` (write) and `commands`.
fn server(base: &Path, commands: Option<&str>) -> Server {
    let list = commands.map(|l| CommandAllowlist::parse(l).unwrap());
    let automation = Automation::new(Some(&base.join("in")), Some(&base.join("out")), list).unwrap().map(Arc::new);
    Server::new(Box::new(Headless::new().with_automation(automation)))
}

/// `p` as text, the way tools receive paths.
fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// `tools/call` -> (isError, text).
fn call(server: &mut Server, name: &str, args: Value) -> (bool, String) {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string();
    let reply: Value = serde_json::from_str(&server.handle_line(&line).unwrap()).unwrap();
    let r = &reply["result"];
    let text = r["content"].as_array().map(|c| c.iter().filter_map(|x| x["text"].as_str()).collect::<String>()).unwrap_or_default();
    (r["isError"] == true, text)
}

/// `tools/call` that must succeed; returns its text.
fn ok(server: &mut Server, name: &str, args: Value) -> String {
    let (err, text) = call(server, name, args.clone());
    assert!(!err, "{name} {args}: {text}");
    text
}

/// `tools/call` that must be refused with `needle` in the message.
fn refused(server: &mut Server, name: &str, args: Value, needle: &str) {
    let (err, text) = call(server, name, args.clone());
    assert!(err, "{name} {args} should be refused: {text}");
    assert!(text.contains(needle), "{name}: expected `{needle}` in: {text}");
}

/// Files open only from the read root and save only into the write root, imports and exports too.
#[test]
fn files_open_and_save_only_inside_their_roots() {
    let base = folders("files");
    let mut srv = server(&base, None);
    ok(&mut srv, "open_workbook", json!({"path": s(&base.join("in/data.csv"))}));
    for path in [base.join("outside/secret.csv"), base.join("in2/data.csv"), base.join("in/../outside/secret.csv")] {
        refused(&mut srv, "open_workbook", json!({"path": s(&path)}), "automation path rejected");
    }
    ok(&mut srv, "write_range", json!({"range": "C1", "values": [["Total"], ["=SUM(B2:B3)"]]}));
    ok(&mut srv, "save_workbook", json!({"path": s(&base.join("out/budget.xlsx"))}));
    assert!(base.join("out/budget.xlsx").is_file());
    for path in [base.join("outside/x.xlsx"), base.join("in/x.xlsx"), base.join("out/../outside/x.xlsx"), base.join("out/a/../../outside/y.xlsx")] {
        refused(&mut srv, "save_workbook", json!({"path": s(&path)}), "automation path rejected");
    }
    // Exports and imports go through the same two functions.
    refused(
        &mut srv,
        "execute_command",
        json!({"command": "file.exportCsv", "params": {"path": s(&base.join("outside/x.csv"))}}),
        "automation path rejected",
    );
    refused(
        &mut srv,
        "execute_command",
        json!({"command": "data.fromTextCsv", "params": {"path": s(&base.join("outside/secret.csv"))}}),
        "automation path rejected",
    );
    assert_eq!(std::fs::read_dir(base.join("outside")).unwrap().count(), 1, "nothing was written outside");
    assert!(!base.join("in/x.xlsx").exists(), "the read root is not writable");
    let _ = std::fs::remove_dir_all(&base);
}

/// The write root itself is not a file to save: its temporary sibling would land outside it.
#[test]
fn the_write_root_itself_is_not_a_destination() {
    // A write root named like a file: saving to the root's own path would write the temporary
    // sibling `out.csv.sctmp` next to the root, outside it, and its cleanup could delete a file
    // already there. The save must be refused before anything is touched.
    let base = std::env::temp_dir().join(format!("gc-mcp-auto-rootname-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("out.csv");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(base.join("out.csv.sctmp"), "keep me").unwrap();
    let automation = Automation::new(Some(&root), Some(&root), None).unwrap().map(Arc::new);
    let mut srv = Server::new(Box::new(Headless::new().with_automation(automation)));
    ok(&mut srv, "write_range", json!({"range": "A1", "values": [["x"]]}));
    refused(&mut srv, "save_workbook", json!({"path": s(&root)}), "automation path rejected");
    refused(&mut srv, "execute_command", json!({"command": "file.saveAs", "params": {"path": s(&root)}}), "automation path rejected");
    // The same folder spelled another way: refused too (here the format check may answer first).
    for path in [root.join("."), root.join("sub").join("..")] {
        assert!(call(&mut srv, "save_workbook", json!({"path": s(&path)})).0, "{path:?}");
    }
    assert_eq!(std::fs::read_to_string(base.join("out.csv.sctmp")).unwrap(), "keep me", "the sibling outside the root is untouched");
    assert!(root.is_dir(), "the root is still a folder");
    ok(&mut srv, "save_workbook", json!({"path": s(&root.join("book.csv"))}));
    let mut names: Vec<_> = std::fs::read_dir(&base).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["out.csv", "out.csv.sctmp"], "nothing new beside the root");
    let _ = std::fs::remove_dir_all(&base);
}

/// Listed commands run, through any tool; the rest are refused.
#[test]
fn listed_commands_run_and_the_rest_are_refused() {
    let base = folders("commands");
    let mut srv = server(&base, Some("range.setValues, cell.set, cell.get, sheet.read, document.inspect, home.*, file.save"));
    ok(&mut srv, "write_range", json!({"range": "A1", "values": [["Item", "Cost"], ["Rent", 1200], ["Total", "=SUM(B2:B2)"]]}));
    ok(&mut srv, "format_range", json!({"range": "A1:B1", "bold": true, "fillColor": "FFFF00"}));
    let read = ok(&mut srv, "read_range", json!({"range": "B3"}));
    assert!(read.contains("1200"), "{read}");
    ok(&mut srv, "save_workbook", json!({"path": s(&base.join("out/ok.xlsx"))}));
    refused(&mut srv, "execute_command", json!({"command": "insert.chart", "params": {"range": "A1:B2", "type": "column"}}), "not allowed");
    refused(&mut srv, "insert_chart", json!({"range": "A1:B2", "type": "column"}), "insert.chart");
    refused(&mut srv, "open_workbook", json!({"path": s(&base.join("in/data.csv"))}), "file.open");
    refused(&mut srv, "sort_range", json!({"keys": ["B"], "range": "A1:B3"}), "data.sort");
    let _ = std::fs::remove_dir_all(&base);
}

/// A tool built from several commands stops at the first one off the list.
#[test]
fn a_composite_tool_stops_at_the_first_command_off_the_list() {
    let base = folders("composite");
    // Bold may run, fill colour may not: format_range applies bold, then stops.
    let mut srv = server(&base, Some("range.setValues, home.bold, cell.get, document.inspect"));
    ok(&mut srv, "write_range", json!({"range": "A1", "values": [["x"]]}));
    refused(&mut srv, "format_range", json!({"range": "A1", "bold": true, "fillColor": "FF0000"}), "home.fillColor");
    let _ = std::fs::remove_dir_all(&base);
}

/// A stored script with a step off the list is refused before any step runs.
#[test]
fn a_script_is_not_a_way_around_the_list() {
    let base = folders("scripts");
    let mut srv = server(&base, Some("cell.set, cell.get, automate.*, document.inspect"));
    ok(
        &mut srv,
        "execute_command",
        json!({"command": "automate.newScript", "params": {"name": "Sneak", "commands": [
            {"command": "cell.set", "params": {"cell": "A1", "input": "ran"}},
            {"command": "insert.chart", "params": {"range": "A1:A1", "type": "column"}}
        ]}}),
    );
    refused(&mut srv, "execute_command", json!({"command": "automate.runScript", "params": {"name": "Sneak", "trusted": true}}), "insert.chart");
    // Refused before any step ran.
    let cell = ok(&mut srv, "get_cell", json!({"cell": "A1"}));
    assert!(!cell.contains("ran"), "{cell}");
    let _ = std::fs::remove_dir_all(&base);
}

/// `list_commands` shows only the commands on the list.
#[test]
fn list_commands_shows_only_what_may_run() {
    let base = folders("list");
    let mut srv = server(&base, Some("home.*, cell.set"));
    let text = ok(&mut srv, "list_commands", json!({}));
    let list: Value = serde_json::from_str(&text).unwrap();
    let ids: Vec<&str> = list.as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    assert!(ids.contains(&"home.bold") && ids.contains(&"cell.set"), "{ids:?}");
    assert!(ids.iter().all(|id| id.starts_with("home.") || *id == "cell.set"), "{ids:?}");
    let _ = std::fs::remove_dir_all(&base);
}

/// Without automation the server is as it was: any file, any command.
#[test]
fn without_automation_nothing_changes() {
    let base = folders("none");
    let mut srv = Server::new(Box::new(Headless::new()));
    ok(&mut srv, "open_workbook", json!({"path": s(&base.join("outside/secret.csv"))}));
    ok(&mut srv, "insert_chart", json!({"range": "A1:B1", "type": "column"}));
    let text = ok(&mut srv, "list_commands", json!({}));
    let list: Value = serde_json::from_str(&text).unwrap();
    assert!(list.as_array().unwrap().len() > 300);
    let _ = std::fs::remove_dir_all(&base);
}
