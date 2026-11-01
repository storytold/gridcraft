//! Whole native documents for opt-in embedding hosts.

use gridcraft_ui_egui::SheetApp;
use serde_json::json;

pub struct Snapshot {
    pub bytes: Vec<u8>,
    pub document: String,
    pub revision: String,
}

pub struct Opened {
    pub document: String,
    pub revision: String,
}

/// A correlated host import completes through the real UI/engine command before
/// its identity is returned. Merely enqueueing bytes in the file-picker inbox
/// cannot prove which document was opened.
pub fn open(app: &mut SheetApp, name: &str, bytes: &[u8]) -> Result<Opened, String> {
    // File commands settle drafts. Refuse before that boundary so an error or
    // a replacement cannot silently commit or discard unacknowledged work.
    if crate::draft_state::dirty(app) || app.editor.is_some() || app.text_box_editor.is_some() {
        return Err("Save or discard unsaved work and finish native drafts before opening another workbook.".into());
    }
    let result = app.run("file.open", json!({"name":name,"base64":gridcraft_engine::io::base64_encode(bytes)}))?;
    if let Some(warnings) = result.get("warnings").and_then(serde_json::Value::as_array)
        && !warnings.is_empty()
    {
        return Err(warnings.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join("\n"));
    }
    let doc = app.session.active().ok_or_else(|| "The engine did not open a workbook.".to_string())?;
    Ok(Opened { document: doc.uid.to_string(), revision: doc.revision.to_string() })
}

/// Encode the complete native workbook through the same command as desktop/CLI.
/// `SheetApp::run` settles pending cell and text-box drafts before file commands.
/// Export is not a durable save and deliberately leaves the dirty baseline alone.
pub fn export(app: &mut SheetApp) -> Result<Snapshot, String> {
    let encoded = app.run("file.saveBytes", json!({"format": "json"}))?;
    let bytes = encoded
        .get("base64")
        .and_then(serde_json::Value::as_str)
        .and_then(gridcraft_engine::io::base64_decode)
        .ok_or_else(|| "The engine did not return a native workbook.".to_string())?;
    validate(&bytes)?;
    // A pending draft may have advanced the revision during the file command.
    let doc = app.session.active().ok_or_else(|| "No workbook is open.".to_string())?;
    Ok(Snapshot { bytes, document: doc.uid.to_string(), revision: doc.revision.to_string() })
}

/// Limit native RPC bytes before crossing the JS-to-Rust copy boundary.
/// The private port also bounds file import/export; legacy window import stays unchanged.
pub fn validate_length(length: usize) -> Result<(), String> {
    if length > 32 * 1024 * 1024 {
        return Err("Native validation accepts at most 32 MiB.".into());
    }
    Ok(())
}

/// Decode using the application's engine without changing the open workbook.
pub fn validate(bytes: &[u8]) -> Result<(), String> {
    // The file reader also sniffs XLSX irrespective of the supplied extension.
    // This endpoint promises native JSON, not an arbitrary import.
    serde_json::from_slice::<serde_json::Value>(bytes).map_err(|e| e.to_string())?;
    let (_, warnings) = gridcraft_engine::io::open_bytes("snapshot.json", bytes).map_err(|e| e.to_string())?;
    if !warnings.is_empty() {
        return Err(warnings.join("\n"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use gridcraft_engine::Session;
    use gridcraft_ui_egui::{Services, SheetApp};
    use serde_json::json;

    fn app() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        SheetApp::new(session, Services::default())
    }

    #[test]
    fn correlated_open_identity_belongs_to_the_import_not_a_later_file_new() {
        let mut app = app();
        let mut source = Session::new();
        source.new_workbook();
        source.execute("cell.set", json!({"cell":"A1","input":"import A"})).unwrap();
        let encoded = source.execute("file.saveBytes", json!({"format":"json"})).unwrap();
        let bytes = gridcraft_engine::io::base64_decode(encoded["base64"].as_str().unwrap()).unwrap();
        let opened = super::open(&mut app, "owned.json", &bytes).unwrap();
        assert_eq!(opened.document, app.session.active().unwrap().uid.to_string());
        assert_eq!(opened.revision, app.session.active().unwrap().revision.to_string());
        assert_eq!(app.run("cell.get", json!({"cell":"A1"})).unwrap()["value"], "import A");
        app.run("file.new", json!({})).unwrap();
        assert_ne!(opened.document, app.session.active().unwrap().uid.to_string());
        let before = app.session.active().unwrap().uid;
        assert!(super::open(&mut app, "invalid.json", b"invalid JSON").is_err());
        assert_eq!(before, app.session.active().unwrap().uid);
    }

    fn imported_bytes(value: &str) -> Vec<u8> {
        let mut source = Session::new();
        source.new_workbook();
        source.execute("cell.set", json!({"cell":"A1","input":value})).unwrap();
        let encoded = source.execute("file.saveBytes", json!({"format":"json"})).unwrap();
        gridcraft_engine::io::base64_decode(encoded["base64"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn same_import_name_with_new_bytes_opens_a_new_actual_document() {
        let mut app = app();
        let first = super::open(&mut app, "owned.json", &imported_bytes("import A")).unwrap();
        let second = super::open(&mut app, "owned.json", &imported_bytes("import B")).unwrap();
        assert_ne!(first.document, second.document);
        assert_eq!(second.document, app.session.active().unwrap().uid.to_string());
        assert_eq!(app.run("cell.get", json!({"cell":"A1"})).unwrap()["value"], "import B");
    }

    #[test]
    fn owned_import_refuses_dirty_work_without_replacing_or_settling_it() {
        let mut app = app();
        app.run("cell.set", json!({"cell":"A1","input":"unsaved"})).unwrap();
        let before = app.session.active().unwrap().wb.clone();
        let uid = app.session.active().unwrap().uid;
        assert!(super::open(&mut app, "owned.json", &imported_bytes("replacement")).is_err());
        assert_eq!(uid, app.session.active().unwrap().uid);
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.active().unwrap().wb));
        assert!(crate::draft_state::dirty(&app));
    }

    #[test]
    fn owned_import_error_retains_pending_cell_without_committing_it() {
        let mut app = app();
        let before = app.session.active().unwrap().wb.clone();
        app.begin_edit(Some("pending cell".into()), false);
        assert!(super::open(&mut app, "invalid.json", b"invalid JSON").is_err());
        assert_eq!(app.editor.as_ref().unwrap().text, "pending cell");
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.active().unwrap().wb));
        assert!(crate::draft_state::dirty(&app));
    }

    #[test]
    fn owned_import_refuses_pending_text_box_without_committing_it() {
        let mut app = app();
        let id = app.run("insert.textBox", json!({"at":"B2","text":"original"})).unwrap()["shape"].as_u64().unwrap() as u32;
        let doc = app.session.active_mut().unwrap();
        doc.saved = doc.wb.clone();
        let before = doc.wb.clone();
        assert!(app.begin_text_box_edit(id));
        app.text_box_editor.as_mut().unwrap().text = "pending text box".into();
        assert!(super::open(&mut app, "owned.json", &imported_bytes("replacement")).is_err());
        assert_eq!(app.text_box_editor.as_ref().unwrap().text, "pending text box");
        assert!(std::sync::Arc::ptr_eq(&before, &app.session.active().unwrap().wb));
        assert!(crate::draft_state::dirty(&app));
    }

    #[test]
    fn native_export_commits_the_cell_draft_without_claiming_durable_save() {
        let mut app = app();
        app.run("cell.set", json!({"cell":"A1", "input":"21"})).unwrap();
        app.run("cell.set", json!({"cell":"B1", "input":"=A1*2"})).unwrap();
        app.run("home.bold", json!({"range":"A1", "on":true})).unwrap();
        app.run("home.numberFormat", json!({"range":"B1", "code":"0.00"})).unwrap();
        app.run("home.mergeCells", json!({"range":"A4:C4"})).unwrap();
        app.run("home.rowHeight", json!({"rows":"5:5", "height":42})).unwrap();
        app.run("home.insertSheet", json!({"name":"Second"})).unwrap();
        app.run("cell.set", json!({"cell":"A1", "input":"Second sheet"})).unwrap();
        app.run("sheet.activate", json!({"sheet":"Sheet1"})).unwrap();
        app.run("selection.set", json!({"cell":"A1"})).unwrap();
        let before = app.session.active().unwrap().revision;
        let document = app.session.active().unwrap().uid.to_string();
        app.begin_edit(Some("22".into()), false);
        let snapshot = super::export(&mut app).unwrap();
        assert_eq!(snapshot.document, document);
        assert!(app.session.active().unwrap().revision > before);
        assert_eq!(snapshot.revision, app.session.active().unwrap().revision.to_string());
        let bytes = snapshot.bytes;
        assert!(app.editor.is_none());
        assert!(app.session.active().unwrap().is_dirty());
        let (wb, _) = gridcraft_engine::io::open_bytes("snapshot.json", &bytes).unwrap();
        // Formula AST caches and rebuilt style indices are intentionally not persisted.
        assert_eq!(serde_json::to_value(&wb).unwrap(), serde_json::to_value(app.session.active().unwrap().wb.as_ref()).unwrap());
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.get("sheets").is_some());
    }

    #[test]
    fn native_validation_length_boundary_is_checked_without_allocating_payloads() {
        assert!(super::validate_length(31 * 1024 * 1024).is_ok());
        assert!(super::validate_length(32 * 1024 * 1024).is_ok());
        assert!(super::validate_length(32 * 1024 * 1024 + 1).is_err());
    }

    #[test]
    fn native_validation_rejects_invalid_documents() {
        assert!(super::validate(b"not JSON").is_err());
        assert!(super::validate(br#"{"sheets":"broken"}"#).is_err());
    }

    #[test]
    fn rejected_draft_is_retained_instead_of_exporting_an_older_cell() {
        let mut app = app();
        app.begin_edit(Some("=1+".into()), false);
        assert!(super::export(&mut app).is_err());
        assert!(app.editor.is_some());
    }

    #[test]
    fn native_validation_does_not_accept_an_xlsx_disguised_as_json() {
        let mut session = Session::new();
        session.new_workbook();
        let encoded = session.execute("file.saveBytes", json!({"format":"xlsx"})).unwrap();
        let bytes = gridcraft_engine::io::base64_decode(encoded["base64"].as_str().unwrap()).unwrap();
        assert!(super::validate(&bytes).is_err());
    }

    #[test]
    fn native_export_has_no_four_mib_checkpoint_limit() {
        let mut app = app();
        let values: Vec<Vec<String>> = (0..130).map(|_| vec!["x".repeat(32767)]).collect();
        app.run("range.setValues", json!({"range":"A1", "values":values})).unwrap();
        let bytes = super::export(&mut app).unwrap().bytes;
        assert!(bytes.len() > 4 * 1024 * 1024);
        super::validate(&bytes).unwrap();
    }
}
