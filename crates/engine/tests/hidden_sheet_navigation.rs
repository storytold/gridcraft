//! Hidden sheets stay hidden: Next / Previous Sheet skip them and stay put when no visible sheet
//! lies that way, hiding, deleting or moving sheets never activates one, and a workbook whose
//! saved active sheet is hidden opens on a visible sheet. Scripts may still activate a hidden
//! sheet by name or index (`sheet.activate`).

use std::io::{Read, Write};
use std::sync::Arc;

use gridcraft_engine::model::{Sheet, Visibility, Workbook};
use gridcraft_engine::{DocState, Session};
use serde_json::json;

/// One sheet per letter (`V` visible, `H` hidden, `X` very hidden), named by letter and position
/// (`V0`, `H1`…), with sheet `active` active.
fn session(kinds: &str, active: usize) -> Session {
    let mut wb = Workbook::new();
    wb.sheets = kinds
        .chars()
        .enumerate()
        .map(|(i, k)| {
            let mut sh = Sheet::new(format!("{k}{i}"));
            sh.visibility = match k {
                'H' => Visibility::Hidden,
                'X' => Visibility::VeryHidden,
                _ => Visibility::Visible,
            };
            Arc::new(sh)
        })
        .collect();
    wb.active_sheet = active;
    let mut s = Session::new();
    s.add_document(DocState::new(wb, None, "Book1".into()));
    s
}

fn active(s: &Session) -> String {
    let wb = &s.doc().unwrap().wb;
    wb.active().map(|sh| sh.name.clone()).unwrap_or_default()
}

fn step(s: &mut Session, id: &str) -> String {
    s.execute(id, json!({})).unwrap();
    active(s)
}

#[test]
fn next_and_previous_skip_hidden_sheets_and_stay_at_the_last_visible_one() {
    // [Visible, Hidden]: nothing visible to the right; the hidden sheet must not be activated.
    let mut s = session("VH", 0);
    assert_eq!(step(&mut s, "sheet.next"), "V0");
    assert_eq!(step(&mut s, "sheet.previous"), "V0");

    // [Hidden, Visible]: nothing visible to the left.
    let mut s = session("HV", 1);
    assert_eq!(step(&mut s, "sheet.previous"), "V1");
    assert_eq!(step(&mut s, "sheet.next"), "V1");

    // [Visible, Hidden, Visible]: the middle sheet is stepped over both ways, and the ends hold.
    let mut s = session("VHV", 0);
    assert_eq!(step(&mut s, "sheet.next"), "V2");
    assert_eq!(step(&mut s, "sheet.next"), "V2");
    assert_eq!(step(&mut s, "sheet.previous"), "V0");
    assert_eq!(step(&mut s, "sheet.previous"), "V0");

    // Hidden and very hidden sheets on both sides of the one visible sheet.
    let mut s = session("HXVHX", 2);
    for id in ["sheet.next", "sheet.next", "sheet.previous", "sheet.previous"] {
        assert_eq!(step(&mut s, id), "V2", "{id}");
    }

    // Several hidden sheets in a row are all skipped.
    let mut s = session("VHXHV", 0);
    assert_eq!(step(&mut s, "sheet.next"), "V4");
    assert_eq!(step(&mut s, "sheet.previous"), "V0");
}

#[test]
fn hiding_the_active_sheet_activates_the_next_visible_sheet_else_the_previous_one() {
    let mut s = session("VVHV", 1);
    s.execute("sheet.hide", json!({"sheet": 1})).unwrap();
    assert_eq!(active(&s), "V3", "the next visible sheet, past the hidden one");
    s.execute("sheet.hide", json!({})).unwrap();
    assert_eq!(active(&s), "V0", "no visible sheet after it: the one before");
    // The last visible sheet can't be hidden.
    assert!(s.execute("sheet.hide", json!({})).is_err());
    assert_eq!(active(&s), "V0");
    // Undo brings the sheet back as the active one; redo hides it again and leaves it.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(active(&s), "V3");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(active(&s), "V0");
}

#[test]
fn deleting_the_active_sheet_never_activates_a_hidden_sheet() {
    // The sheet after it is hidden: the next visible one takes over.
    let mut s = session("VVHV", 1);
    s.execute("home.deleteSheet", json!({})).unwrap();
    assert_eq!(active(&s), "V3");
    // The last visible sheet with only a hidden one after it: the one before takes over.
    let mut s = session("VVH", 1);
    s.execute("home.deleteSheet", json!({})).unwrap();
    assert_eq!(active(&s), "V0");
    // Deleting another sheet keeps the active one.
    let mut s = session("VHVV", 3);
    s.execute("home.deleteSheet", json!({"sheet": "V0"})).unwrap();
    assert_eq!(active(&s), "V3");
}

#[test]
fn moving_or_copying_a_hidden_sheet_keeps_the_active_sheet() {
    let mut s = session("VHV", 2);
    s.execute("sheet.move", json!({"sheet": "H1", "to": 0})).unwrap();
    assert_eq!(active(&s), "V2");
    s.execute("sheet.move", json!({"sheet": "H1", "to": 2})).unwrap();
    assert_eq!(active(&s), "V2");
    s.execute("sheet.move", json!({"sheet": "H1", "to": 0, "copy": true})).unwrap();
    assert_eq!(active(&s), "V2");
    let wb = &s.doc().unwrap().wb;
    assert_eq!(wb.sheets[0].visibility, Visibility::Hidden, "the copy stays hidden like its original");
    // A visible sheet moved or copied becomes the active one, as before.
    s.execute("sheet.move", json!({"sheet": "V0", "to": 3})).unwrap();
    assert_eq!(active(&s), "V0");
    s.execute("sheet.move", json!({"sheet": "V2", "to": 0, "copy": true})).unwrap();
    assert_eq!(active(&s), "V2 (2)");
}

/// The workbook saved by `s`, with its saved active sheet (`activeTab`) changed to `tab`, as
/// another program could write it.
fn with_active_tab(s: &mut Session, tab: usize) -> String {
    let saved = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let bytes = gridcraft_engine::io::base64_decode(saved["base64"].as_str().unwrap()).unwrap();
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut zout = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut part = zin.by_index(i).unwrap();
        let mut data = Vec::new();
        part.read_to_end(&mut data).unwrap();
        if part.name() == "xl/workbook.xml" {
            let xml = String::from_utf8(data).unwrap();
            assert!(xml.contains("activeTab=\"0\""), "{xml}");
            data = xml.replace("activeTab=\"0\"", &format!("activeTab=\"{tab}\"")).into_bytes();
        }
        zout.start_file(part.name(), zip::write::SimpleFileOptions::default()).unwrap();
        zout.write_all(&data).unwrap();
    }
    gridcraft_engine::io::base64_encode(&zout.finish().unwrap().into_inner())
}

#[test]
fn a_workbook_saved_with_a_hidden_active_sheet_opens_on_a_visible_one() {
    for (kinds, tab, expected) in [("VHV", 1, "V2"), ("VH", 1, "V0"), ("VXHV", 2, "V3")] {
        let mut s = session(kinds, 0);
        let file = with_active_tab(&mut s, tab);
        let mut opened = Session::new();
        opened.execute("file.open", json!({"name": "hidden.xlsx", "base64": file})).unwrap();
        assert_eq!(active(&opened), expected, "{kinds} with activeTab {tab}");
        assert!(!opened.doc().unwrap().is_dirty(), "choosing the sheet shown doesn't edit the workbook");
        // And stepping from there still never reaches a hidden sheet.
        for id in ["sheet.next", "sheet.previous", "sheet.next"] {
            let name = step(&mut opened, id);
            assert!(name.starts_with('V'), "{kinds}: {id} activated {name}");
        }
    }
    // A workbook built in code or read from JSON with a hidden active sheet.
    assert_eq!(active(&session("VHV", 1)), "V2");
    assert_eq!(active(&session("VH", 1)), "V0");
    assert_eq!(active(&session("VH", 7)), "V0", "an active sheet out of range opens on a visible one too");
}

#[test]
fn scripts_can_still_activate_a_hidden_sheet_and_step_back_to_visible_ones() {
    let mut s = session("VHV", 0);
    s.execute("sheet.activate", json!({"sheet": "H1"})).unwrap();
    assert_eq!(active(&s), "H1");
    s.execute("cell.set", json!({"cell": "A1", "input": "kept"})).unwrap();
    assert_eq!(step(&mut s, "sheet.next"), "V2");
    s.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    assert_eq!(step(&mut s, "sheet.previous"), "V0");
    // From a hidden sheet with no visible sheet that way, it stays where the script put it.
    let mut s = session("VH", 0);
    s.execute("sheet.activate", json!({"sheet": "H1"})).unwrap();
    assert_eq!(step(&mut s, "sheet.next"), "H1");
    assert_eq!(step(&mut s, "sheet.previous"), "V0");
}
