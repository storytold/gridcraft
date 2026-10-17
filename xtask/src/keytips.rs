//! Regenerate the keytip sequence table and its per-sequence test suite from `keytips.json`.
//!
//! `cargo xtask keytips`          regenerate the table + `tests/keytips_generated.rs`
//! `cargo xtask keytips --check`  fail if either generated file differs from the JSON source
//!
//! Requires the default `engine` feature: the generator probes the live command
//! catalog so each generated test asserts exactly what the command does today
//! (journaled, ran clean without a journal step, or surfaced a missing-params error).

use std::fs;
use std::path::Path;

const TABS: &[&str] = &["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"];

/// Commands whose empty-params behaviour depends on the host machine (e.g. the
/// system dictionary). These are never probed; their tests assert "invoked
/// something observable" so the checked-in file stays byte-identical everywhere.
const ENV_SENSITIVE: &[&str] = &["review.spelling"];

/// Format generated source through `rustfmt` so the checked-in file matches
/// the emitter byte-for-byte.
fn rustfmt(src: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn rustfmt: {e}"))?;
    let stdin = child.stdin.as_mut().ok_or("rustfmt: failed to open stdin")?;
    stdin.write_all(src.as_bytes()).map_err(|e| format!("write rustfmt stdin: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("wait rustfmt: {e}"))?;
    if !out.status.success() {
        return Err(format!("rustfmt failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Clone)]
struct Entry {
    seq: String,
    kind: String,
    target: String,
    pending: bool,
    excel: String,
    note: String,
}

/// 0 = ran and journaled, 1 = ran without a journal step, 2 = errored (params
/// missing, disabled, or environment-dependent).
type Probe = std::collections::HashMap<String, u8>;

#[cfg(feature = "engine")]
fn probe_live_commands(ids: &[String]) -> Probe {
    use gridcraft_engine::Session;
    let mut map = Probe::new();
    for id in ids {
        if ENV_SENSITIVE.contains(&id.as_str()) {
            continue;
        }
        let mut s = Session::new();
        s.new_workbook();
        let status = match s.execute(id, serde_json::json!({})) {
            Ok(_) => {
                if s.journal.iter().any(|(jid, _)| jid == id) {
                    0
                } else {
                    1
                }
            }
            Err(_) => 2,
        };
        map.insert(id.clone(), status);
    }
    map
}

pub fn run(root: &Path, rest: &[&str]) -> Result<(), String> {
    #[cfg(not(feature = "engine"))]
    {
        let _ = (root, rest);
        return Err("cargo xtask keytips needs the default `engine` feature to probe the command catalog".into());
    }
    #[cfg(feature = "engine")]
    {
        let json_path = root.join("crates/ui-egui/src/keytips/keytips.json");
        let table_path = root.join("crates/ui-egui/src/keytips/keytip_table/generated.rs");
        let tests_path = root.join("crates/ui-egui/tests/keytips_generated.rs");
        let source = fs::read_to_string(&json_path).map_err(|e| format!("read {}: {e}", json_path.display()))?;
        let (entries, probe) = parse_and_probe(&source)?;
        let generated =
            [(table_path.as_path(), rustfmt(&render_table(&entries))?), (tests_path.as_path(), rustfmt(&render_tests(&entries, &probe))?)];
        if rest.contains(&"--check") {
            for (path, expected) in &generated {
                let on_disk = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
                if on_disk != *expected {
                    return Err(format!("{} is out of date — run `cargo xtask keytips` and commit the result", path.display()));
                }
                println!("keytips: {} up to date", path.display());
            }
            return Ok(());
        }
        for (path, content) in &generated {
            fs::write(path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
            println!("keytips: wrote {}", path.display());
        }
        Ok(())
    }
}

#[cfg(feature = "engine")]
fn parse_and_probe(source: &str) -> Result<(Vec<Entry>, Probe), String> {
    let doc: serde_json::Value = serde_json::from_str(source).map_err(|e| format!("invalid JSON: {e}"))?;
    let seqs = doc.get("sequences").and_then(|v| v.as_array()).ok_or("missing `sequences` array")?;

    let mut entries: Vec<Entry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, e) in seqs.iter().enumerate() {
        let at = format!("entry {}", i + 1);
        let seq = e.get("seq").and_then(|v| v.as_str()).ok_or_else(|| format!("{at}: missing `seq`"))?.to_string();
        if seq.is_empty() || seq.len() > 6 || !seq.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(format!("{at}: seq {seq:?} must be 1-6 ASCII letters/digits (Excel's longest are 5, e.g. sparkline paths)"));
        }
        if !seen.insert(seq.clone()) {
            return Err(format!("{at}: duplicate seq {seq:?}"));
        }
        let kind = e.get("kind").and_then(|v| v.as_str()).ok_or_else(|| format!("{at}: missing `kind`"))?.to_string();
        let target = e.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let pending = e.get("pending").and_then(|v| v.as_bool()).unwrap_or(false);
        if pending && kind != "command" {
            return Err(format!("{at}: `pending` is only valid with kind `command`"));
        }
        match kind.as_str() {
            "tab" => {
                if target.is_empty() || !TABS.contains(&target.as_str()) {
                    return Err(format!("{at}: tab target {target:?} is not one of {TABS:?}"));
                }
            }
            "command" | "dialog" | "control" => {
                if target.is_empty() {
                    return Err(format!("{at}: {kind} needs a `target`"));
                }
            }
            "continue" => {}
            other => return Err(format!("{at}: unknown kind {other:?}")),
        }
        // A terminal Finish entry on a strict prefix makes the longer sequence
        // unreachable (the prefix fires and clears first), e.g. HC (Copy) kills HCP.
        if matches!(kind.as_str(), "command" | "dialog" | "control") {
            for cut in 1..seq.len() {
                let prefix = &seq[..cut];
                if entries.iter().any(|e| e.seq == prefix && matches!(e.kind.as_str(), "command" | "dialog" | "control")) {
                    return Err(format!(
                        "{at}: seq {seq:?} is unreachable — {prefix:?} already fires first; the deeper item needs a menu (item-letter), not a trie sequence"
                    ));
                }
            }
        }
        entries.push(Entry {
            seq,
            kind,
            target,
            pending,
            excel: e.get("excel").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            note: e.get("note").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        });
    }
    // JSON order is authoritative and curated: the first entry for a tab is its canonical
    // badge (legacy aliases come later), and groups stay together. Do not sort.

    let live_ids: Vec<String> = entries
        .iter()
        .filter(|e| e.kind == "command" && !e.pending)
        .map(|e| e.target.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    // Pending targets that already exist fire live at runtime (T6); their tests
    // must assert the live behaviour, not the silent one. Nudge the maintainer.
    let landed: std::collections::BTreeSet<String> = entries
        .iter()
        .filter(|e| e.pending && gridcraft_engine::find_command(&e.target).is_some())
        .map(|e| {
            eprintln!("keytips: note — pending target {:?} now exists in the catalog; consider dropping `pending`", e.target);
            e.target.clone()
        })
        .collect();
    let mut probe = probe_live_commands(&live_ids);
    for id in landed {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        let status = match s.execute(&id, serde_json::json!({})) {
            Ok(_) => {
                if s.journal.iter().any(|(jid, _)| jid == &id) {
                    0
                } else {
                    1
                }
            }
            Err(_) => 2,
        };
        probe.insert(id, status);
    }
    Ok((entries, probe))
}

fn render_table(entries: &[Entry]) -> String {
    let mut out = String::from(
        "//! GENERATED from keytips.json by `cargo xtask keytips`. Edit the JSON, not this file.\n\n\
         use super::{Action, Entry};\n\n\
         /// All known keytip sequences. Intermediate levels are implicit: the matcher waits\n\
         /// whenever a pressed letter starts a deeper sequence, so only terminal entries are\n\
         /// listed here. See keytip_table.rs for the helpers that read this table.\n\
         pub const SEQUENCES: &[(&str, Entry)] = &[\n",
    );
    for e in entries {
        let value = match e.kind.as_str() {
            "tab" => format!("Entry::Tab({:?})", e.target),
            "command" if e.pending => format!("Entry::Finish(Action::Pending({:?}))", e.target),
            "command" => format!("Entry::Finish(Action::Command({:?}))", e.target),
            "dialog" => format!("Entry::Finish(Action::Dialog({:?}))", e.target),
            "control" => format!("Entry::Finish(Action::Control({:?}))", e.target),
            "continue" => "Entry::Continue".to_string(),
            // All kinds are validated during parse; anything else cannot occur.
            _ => String::new(),
        };
        let mut comment = String::new();
        if e.pending {
            comment.push_str(" pending");
        }
        if !e.excel.is_empty() {
            if !comment.is_empty() {
                comment.push(';');
            }
            comment.push_str(&format!(" Excel: {}", e.excel));
        }
        if !e.note.is_empty() {
            if !comment.is_empty() {
                comment.push(';');
            }
            comment.push_str(&format!(" {}", e.note));
        }
        out.push_str(&format!("    ({:?}, {value}),", e.seq));
        if !comment.is_empty() {
            out.push_str(&format!(" //{comment}"));
        }
        out.push('\n');
    }
    out.push_str("];\n");
    out
}

fn test_name(seq: &str) -> String {
    format!("keytip_{}_", seq.to_ascii_lowercase())
}

fn render_tests(entries: &[Entry], probe: &Probe) -> String {
    let mut out = String::from(HEADER);
    for e in entries {
        let body = match e.kind.as_str() {
            "tab" => format!(
                "sequence(&mut h, {seq:?}, false);\n    assert_eq!(h.state().ui.ribbon_tab, {tab:?}, \"{seq} must switch to the {tab} tab\");",
                seq = e.seq,
                tab = e.target,
            ),
            "command" if e.pending && !probe.contains_key(&e.target) => format!(
                "sequence(&mut h, {seq:?}, false);\n    assert!(h.state().dialog.is_none(), \"{seq} must not open a dialog\");\n    assert!(h.state().message.is_none(), \"{seq} must not open a message box\");\n    assert!(h.state().toast.is_none(), \"{seq} must not raise a toast\");\n    assert!(h.state().session.journal.is_empty(), \"{seq} must not journal\");",
                seq = e.seq,
            ),
            "command" => match probe.get(&e.target).copied().unwrap_or(2) {
                0 => format!(
                    "sequence(&mut h, {seq:?}, false);\n    assert!(h.state().session.journal.iter().any(|(id, _)| id == {id:?}), \"{seq} must journal {id}\");",
                    seq = e.seq,
                    id = e.target,
                ),
                1 => format!(
                    "sequence(&mut h, {seq:?}, false);\n    assert!(h.state().message.is_none() && h.state().toast.is_none(), \"{seq} must run {id} cleanly\");",
                    seq = e.seq,
                    id = e.target,
                ),
                _ => format!(
                    "sequence(&mut h, {seq:?}, false);\n    assert!(h.state().message.is_some() || h.state().toast.is_some(), \"{seq} must invoke {id} and surface its missing-params error\");",
                    seq = e.seq,
                    id = e.target,
                ),
            },
            "dialog" if e.target == "scenarioManager" => format!(
                "sequence(&mut h, {seq:?}, false);\n    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some(\"scenarioManager\"), \"{seq} must open the scenarioManager dialog\");",
                seq = e.seq,
            ),
            "dialog" => format!(
                "sequence(&mut h, {seq:?}, false);\n    assert!(h.state().dialog.is_some(), \"{seq} must open the {dialog} dialog\");",
                seq = e.seq,
                dialog = e.target,
            ),
            "control" => format!(
                "sequence(&mut h, {seq:?}, false);\n    assert!(h.state().message.is_none() && h.state().toast.is_none(), \"{seq} must route to its control without an error box\");\n    assert_eq!(h.state().keytips.prefix(), None);",
                seq = e.seq,
            ),
            "continue" => format!(
                "sequence(&mut h, {seq:?}, false);\n    assert_eq!(h.state().keytips.prefix(), Some({seq:?}), \"{seq} must stay armed waiting for more input\");",
                seq = e.seq,
            ),
            // All kinds are validated during parse; anything else cannot occur.
            _ => String::new(),
        };
        out.push_str(&format!("\n#[test]\nfn {}() {{\n    let mut h = harness(blank());\n    {body}\n}}\n", test_name(&e.seq)));
    }
    out.push_str(&format!(
        "\n#[test]\nfn generated_test_count_matches_the_table() {{\n    const EXPECTED: usize = {};\n    assert_eq!(gridcraft_ui_egui::keytips::keytip_table::SEQUENCES.len(), EXPECTED);\n}}\n",
        entries.len()
    ));
    out.push_str(SAMPLE_TEXT_FIRST);
    out
}

const HEADER: &str = r#"//! GENERATED from keytips.json by `cargo xtask keytips`. Edit the JSON, not this file.
//!
//! One end-to-end test per keytip sequence, through the real egui input path.
//! Assertions are generated from a probe of the live engine catalog (blank
//! workbook, empty params), so each test proves exactly what its command does
//! today: journaled, ran clean, surfaced a missing-params error, opened a
//! dialog, switched tabs, waited for more input, or stayed silent (pending).

use egui::{Event, Key, Modifiers};
use egui::os::OperatingSystem;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder()
        .with_os(OperatingSystem::Nix)
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SheetApp::new(session, Default::default()),
        );
    h.run_steps(4);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

fn events(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    events(h, [key_event(key, true, Modifiers::NONE), key_event(key, false, Modifiers::NONE)]);
}

fn letter(h: &mut Harness, key: Key, text_first: bool) {
    let text = Event::Text(key.name().to_ascii_lowercase());
    let pressed = key_event(key, true, Modifiers::NONE);
    let pair = if text_first { [text, pressed] } else { [pressed, text] };
    events(h, pair.into_iter().chain([key_event(key, false, Modifiers::NONE)]));
}

fn sequence(h: &mut Harness, seq: &str, text_first: bool) {
    key(h, Key::F10);
    for ch in seq.chars() {
        let k = Key::from_name(&ch.to_string()).expect("sequence letters map to keys");
        letter(h, k, text_first);
    }
}

"#;

const SAMPLE_TEXT_FIRST: &str = r#"
#[test]
fn sample_sequences_work_with_text_before_key() {
    // Some IME paths deliver Event::Text ahead of the key press; the matcher
    // must pair them either way. One representative of each action kind;
    // the dialog opener runs last so it does not eat later input.
    // Each step resets first: Escape twice leaves keytip mode so the next
    // F10 arms cleanly (a lingering prefix would make F10 disarm instead).
    let mut h = harness(blank());
    sequence(&mut h, "H", true);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
    reset(&mut h);
    sequence(&mut h, "WVG", true);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.gridlines"));
    reset(&mut h);
    sequence(&mut h, "HV", true);
    assert_eq!(h.state().keytips.prefix(), Some("HV"));
    reset(&mut h);
    sequence(&mut h, "NE", true);
    assert!(h.state().dialog.is_none() && h.state().message.is_none() && h.state().toast.is_none());
    reset(&mut h);
    sequence(&mut h, "HVS", true);
    assert!(h.state().dialog.is_some(), "HVS must open the paste-special dialog");
}

fn reset(h: &mut Harness) {
    key(h, Key::Escape); // clear any prefix, back to the armed root
    key(h, Key::Escape); // leave keytip mode entirely
}
"#;
