//! Synthetic end-to-end regressions for confined MCP. Fixtures are retained for inspection.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{Backend, Headless, Server};
use gridcraft_engine::{Session, file_access::AutomationRoots};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn fixture(label: &str) -> PathBuf {
    #[allow(clippy::disallowed_methods)] // Native-only filesystem fixture; retain every run.
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("gridcraft-confined-{label}-{}-{stamp}", std::process::id()));
    for sub in ["allowed", "allowed2", "outside"] {
        std::fs::create_dir_all(base.join(sub)).unwrap();
    }
    std::fs::write(base.join("allowed/in.csv"), "inside,7\n").unwrap();
    std::fs::write(base.join("outside/private.csv"), "outside,91\n").unwrap();
    base
}

fn confined(base: &Path, write: bool) -> Headless {
    let read = base.join("allowed");
    let roots = AutomationRoots::new(Some(&read), write.then_some(read.as_path())).unwrap();
    Headless::new().with_automation_roots(roots)
}

fn execute(b: &mut Headless, command: &str, params: Value) -> Result<Value, String> {
    b.call("engine.execute", json!({"command":command,"params":params}))
}

#[test]
fn folder_confinement_import_and_generic_commands_reject_existing_outside_files() {
    let base = fixture("reads");
    let mut b = confined(&base, true);
    execute(&mut b, "file.open", json!({"path":base.join("allowed/in.csv")})).unwrap();
    let before = b.call("sheet.read", json!({"range":"A1:B1"})).unwrap();
    for command in ["file.open", "data.fromTextCsv", "data.getData", "insert.picture"] {
        let err = execute(&mut b, command, json!({"path":base.join("outside/private.csv"),"placement":"overCells"})).unwrap_err();
        assert!(err.contains("outside the read root"), "{command}: {err}");
        assert_eq!(b.call("sheet.read", json!({"range":"A1:B1"})).unwrap(), before);
    }
    assert_eq!(std::fs::read_to_string(base.join("outside/private.csv")).unwrap(), "outside,91\n");
}

#[test]
fn folder_confinement_mcp_protocol_rejects_traversal_and_prefix_siblings() {
    let base = fixture("rpc");
    std::fs::write(base.join("allowed2/private.csv"), "sibling,91\n").unwrap();
    let mut s = Server::new(Box::new(confined(&base, true)));
    for path in [base.join("outside/private.csv"), base.join("allowed/../outside/private.csv"), base.join("allowed2/private.csv")] {
        let result =
            s.handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"open_workbook","arguments":{"path":path}}})).unwrap();
        assert_eq!(result["result"]["isError"], true, "{result}");
        assert!(result.to_string().contains("outside the read root"), "{result}");
    }
}

#[test]
fn folder_confinement_all_exports_and_implicit_save_are_checked() {
    let base = fixture("writes");
    let mut b = confined(&base, true);
    execute(&mut b, "cell.set", json!({"cell":"A1","input":"7"})).unwrap();
    for (command, extension) in [
        ("file.save", "xlsx"),
        ("file.saveAs", "xlsx"),
        ("file.exportCsv", "csv"),
        ("file.exportHtml", "html"),
        ("file.exportPdf", "pdf"),
        ("file.print", "pdf"),
    ] {
        let path = base.join("outside").join(format!("{command}.{extension}"));
        let err = execute(&mut b, command, json!({"path":path})).unwrap_err();
        assert!(err.contains("outside the write root"), "{command}: {err}");
        assert!(!path.exists());
        assert!(!PathBuf::from(format!("{}.sctmp", path.display())).exists());
    }
    let destination = base.join("allowed/new.xlsx");
    execute(&mut b, "file.save", json!({"path":destination})).unwrap();
    assert!(destination.is_file());
    let mut session = Session::new();
    session.execute_headless("file.open", json!({"path":base.join("outside/private.csv")})).unwrap();
    let roots = AutomationRoots::new(Some(&base.join("allowed")), Some(&base.join("allowed"))).unwrap();
    let mut outside = Headless::with_session(session).with_automation_roots(roots);
    let err = execute(&mut outside, "file.save", json!({})).unwrap_err();
    assert!(err.contains("outside the write root"), "{err}");
    assert_eq!(std::fs::read_to_string(base.join("outside/private.csv")).unwrap(), "outside,91\n");
}

#[test]
fn folder_confinement_read_only_and_in_memory_export() {
    let base = fixture("read-only");
    let mut b = confined(&base, false);
    execute(&mut b, "file.open", json!({"path":base.join("allowed/in.csv")})).unwrap();
    let err = execute(&mut b, "file.save", json!({"path":base.join("allowed/new.xlsx")})).unwrap_err();
    assert!(err.contains("write authority is absent"), "{err}");
    let bytes = execute(&mut b, "file.saveBytes", json!({"format":"xlsx"})).unwrap();
    assert!(bytes["base64"].as_str().is_some_and(|s| !s.is_empty()));
}

#[cfg(unix)]
#[test]
fn folder_confinement_symlinks_and_temporary_save_paths_are_checked() {
    use std::os::unix::fs::symlink;
    let base = fixture("links");
    symlink(base.join("outside/private.csv"), base.join("allowed/link.csv")).unwrap();
    symlink(base.join("outside"), base.join("allowed/away")).unwrap();
    symlink(base.join("outside/missing.xlsx"), base.join("allowed/dangling.xlsx")).unwrap();
    symlink(base.join("outside/private.csv"), base.join("allowed/new.xlsx.sctmp")).unwrap();
    let mut b = confined(&base, true);
    assert!(execute(&mut b, "file.open", json!({"path":base.join("allowed/link.csv")})).unwrap_err().contains("outside the read root"));
    for name in ["away/new.xlsx", "dangling.xlsx"] {
        let err = execute(&mut b, "file.save", json!({"path":base.join("allowed").join(name)})).unwrap_err();
        assert!(err.contains("outside the write root") || err.contains("link that leads nowhere"), "{err}");
    }
    execute(&mut b, "file.save", json!({"path":base.join("allowed/new.xlsx")})).unwrap();
    assert!(std::fs::symlink_metadata(base.join("allowed/new.xlsx.sctmp")).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(base.join("outside/private.csv")).unwrap(), "outside,91\n");
}

#[test]
fn folder_confinement_existing_temporary_file_survives_successful_save() {
    let base = fixture("existing-temporary");
    let destination = base.join("allowed/new.xlsx");
    let temporary = base.join("allowed/new.xlsx.sctmp");
    std::fs::write(&temporary, b"existing temporary content").unwrap();
    let mut b = confined(&base, true);
    execute(&mut b, "file.save", json!({"path":destination})).unwrap();
    assert_eq!(std::fs::read(&temporary).unwrap(), b"existing temporary content");
    assert!(destination.is_file());
    execute(&mut b, "file.open", json!({"path":destination})).unwrap();
}

/// The operator may keep inputs readable while allowing writes only to a nested output folder.
#[test]
fn folder_confinement_nested_write_root_preserves_inputs() {
    let base = fixture("nested");
    let read = base.join("allowed");
    let write = read.join("_artcraft");
    std::fs::create_dir_all(&write).unwrap();
    let roots = AutomationRoots::new(Some(&read), Some(&write)).unwrap();
    let mut b = Headless::new().with_automation_roots(roots);
    let input = read.join("in.csv");
    execute(&mut b, "file.open", json!({"path":input})).unwrap();
    execute(&mut b, "cell.set", json!({"cell":"B1","input":"42"})).unwrap();
    // Implicit Save still points at the input, outside the narrower write root.
    assert!(execute(&mut b, "file.save", json!({})).unwrap_err().contains("outside the write root"));
    for path in [read.join("new.xlsx"), read.join("_artcraft2/new.xlsx"), write.join("../new.xlsx"), base.join("outside/new.xlsx")] {
        assert!(execute(&mut b, "file.saveAs", json!({"path":path})).unwrap_err().contains("outside the write root"));
        assert!(!path.exists());
    }
    let destination = write.join("result.xlsx");
    execute(&mut b, "file.saveAs", json!({"path":destination})).unwrap();
    execute(&mut b, "file.open", json!({"path":destination})).unwrap();
    let sheet = b.call("sheet.read", json!({"range":"A1:B1"})).unwrap();
    assert!(sheet.to_string().contains("42"), "{sheet}");
    execute(&mut b, "file.exportCsv", json!({"path":write.join("result.csv")})).unwrap();
    assert!(std::fs::read_to_string(write.join("result.csv")).unwrap().contains("42"));
    assert!(execute(&mut b, "file.open", json!({"path":base.join("outside/private.csv")})).unwrap_err().contains("outside the read root"));
    assert_eq!(std::fs::read_to_string(input).unwrap(), "inside,7\n");
    assert_eq!(std::fs::read_to_string(base.join("outside/private.csv")).unwrap(), "outside,91\n");
}
