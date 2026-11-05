//! Whole native documents for same-origin embedding hosts.

use gridcraft_ui_egui::SheetApp;
use serde_json::json;

pub struct Snapshot {
    pub bytes: Vec<u8>,
    pub document: String,
    pub revision: String,
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

/// Limit the new host validation RPC before crossing the JS-to-Rust copy boundary.
/// Native export and the legacy file-open path keep their existing behavior.
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
