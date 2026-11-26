//! Generate an original, fictional workbook demonstrating Change Data Source repair.
//! See `docs/pivot-source-repair.md` for CLI steps.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::PathBuf;

use gridcraft_engine::Session;
use serde_json::json;

/// Write a fictional workbook with an invalid unused source header, refusing to overwrite files.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("pivot-source-repair.xlsx"));
    let mut session = Session::new();
    session.new_workbook();
    session.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["Month", "Status", "Amount", "Category", "Unused"],
            ["2026-01", "Open", 17, "Demo", ""],
            ["2026-02", "Open", 23, "Demo", ""]
        ]}),
    )?;
    session.execute(
        "insert.pivotTable",
        json!({
            "source": "Sheet1!$A$1:$E$3", "destination": "Sheet1!G3",
            "name": "SourceRepair", "rows": ["Month"], "values": ["Amount"]
        }),
    )?;
    // Keep the rendered report but invalidate its old, unused fifth field. The report's
    // selected fields all remain available in the explicit replacement A1:D3.
    session.execute("edit.clearContents", json!({"range": "E1:E3"}))?;
    let bytes = gridcraft_xlsx::write_xlsx(&session.doc()?.wb)?;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    file.write_all(&bytes)?;
    println!("Created {} (fictional data only; existing files are never overwritten).", path.display());
    println!("Replace SourceRepair's source with Sheet1!$A$1:$D$3; expect January 17, February 23, total 40.");
    Ok(())
}
