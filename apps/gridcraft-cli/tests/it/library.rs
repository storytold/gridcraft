//! Insert ▸ Library from the CLI: `run` and `mcp` browse the folders the desktop app keeps in its
//! preferences, and only those preferences.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A transparent picture one pixel square.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// A home folder of its own, with the app's settings folder in it as each system names it, and a
/// Library folder holding one picture.
struct Home {
    root: PathBuf,
    pictures: PathBuf,
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn home(tag: &str, prefs: impl Fn(&Path) -> Value) -> Home {
    let root = std::env::temp_dir().join(format!("gridcraft-cli-library-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let pictures = root.join("Pictures");
    std::fs::create_dir_all(&pictures).unwrap();
    std::fs::write(pictures.join("chart-logo.png"), PNG).unwrap();
    let prefs = serde_json::to_vec(&prefs(&pictures)).unwrap();
    // macOS, Windows (APPDATA) and other systems (XDG_CONFIG_HOME) keep it in different places.
    for dir in [root.join("Library/Application Support/GridCraft"), root.join("AppData/GridCraft"), root.join("config/gridcraft")] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("prefs.json"), &prefs).unwrap();
    }
    Home { root, pictures }
}

fn cli(h: &Home) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gridcraft-cli"));
    c.env("HOME", &h.root).env("APPDATA", h.root.join("AppData")).env("XDG_CONFIG_HOME", h.root.join("config")).env_remove("GRIDCRAFT_NO_PREFS");
    c
}

/// The result of `library.list` from `gridcraft-cli run`.
fn listed(c: &mut Command) -> Value {
    let o = c.args(["run", "--cmd", r#"library.list={"pictures":false}"#]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = String::from_utf8(o.stdout).unwrap();
    let line: Value = serde_json::from_str(text.lines().next().unwrap_or("null")).unwrap();
    line["result"].clone()
}

#[test]
fn run_browses_the_apps_library_folders() {
    let h = home("run", |pictures| json!({"r1c1": true, "libraryFolders": [pictures]}));
    let files = listed(&mut cli(&h));
    assert_eq!(files.as_array().map(Vec::len), Some(1), "{files}");
    assert_eq!(files[0]["title"], "chart-logo");
    assert_eq!(files[0]["folder"], h.pictures.to_str().unwrap());
    // GRIDCRAFT_NO_PREFS leaves them out, as it does for the app.
    let none = listed(cli(&h).env("GRIDCRAFT_NO_PREFS", "1"));
    assert_eq!(none, json!([]));
}

#[test]
fn only_the_folders_are_taken_and_a_hostile_file_gives_none() {
    let h = home("other-prefs", |_| json!({"r1c1": true, "libraryFolders": "not a list"}));
    assert_eq!(listed(&mut cli(&h)), json!([]));
    // Another preference (R1C1 references) changes nothing the CLI does.
    let o = cli(&h).args(["run", "--cmd", r#"cell.set={"cell":"B2","input":"=A1"}"#, "--print", "B2", "--formulas"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("=A1"), "{}", String::from_utf8_lossy(&o.stdout));
}

#[test]
fn the_mcp_server_browses_them_too() {
    let h = home("mcp", |pictures| json!({"libraryFolders": [pictures]}));
    let mut child = cli(&h).arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#).unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "execute_command", "arguments": {"command": "library.search", "params": {"query": "logo"}}}});
        writeln!(stdin, "{call}").unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let answer: Value = serde_json::from_str(text.lines().nth(1).unwrap_or("null")).unwrap();
    let found = answer["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(found.contains("chart-logo.png"), "{text}");
}
