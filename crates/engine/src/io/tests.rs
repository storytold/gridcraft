use super::*;

fn book(rtl: bool, cells: &[(&str, gridcraft_core::Value, gridcraft_model::HAlign)]) -> Workbook {
    let mut wb = Workbook::new();
    let ids: Vec<_> = cells.iter().map(|(_, _, h)| wb.styles.derive(gridcraft_model::StyleId::DEFAULT, |st| st.align.h = *h)).collect();
    let sh = wb.sheet_mut(0).unwrap();
    sh.right_to_left = rtl;
    for ((at, value, _), id) in cells.iter().zip(ids) {
        let c = CellRef::parse(at).unwrap();
        sh.set_value(c, value.clone());
        sh.set_style(c, id);
    }
    wb
}

fn copied(wb: &Workbook, range: &str) -> String {
    let r = RangeRef::parse(range).unwrap();
    range_html(wb, 0, r, &crate::display::range_text(wb, 0, r)).unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(unix)]
#[test]
fn save_fails_upfront_when_the_temp_name_is_taken() {
    // Another local user who can write the directory must not be able to point the
    // save's predictable temporary name at their own file: creation is exclusive.
    let dir = std::env::temp_dir().join(format!("gridcraft-fs-safety-temp-name-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, b"original").unwrap();
    let dest = dir.join("book.xlsx");
    std::os::unix::fs::symlink(&victim, dir.join("book.xlsx.sctmp")).unwrap();
    let err = write_file(dest.to_str().unwrap(), b"new").unwrap_err();
    assert_eq!(std::fs::read(&victim).unwrap(), b"original");
    assert!(!dir.join("book.xlsx").exists(), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(unix)]
#[test]
fn save_keeps_the_destination_access_restrictions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("gridcraft-fs-safety-permissions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join("private.xlsx");
    std::fs::write(&dest, b"v1").unwrap();
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600)).unwrap();
    write_file(dest.to_str().unwrap(), b"v2").unwrap();
    let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    // And a brand-new file is created with the restrictive, not permissive, default.
    let fresh = dir.join("new.xlsx");
    write_file(fresh.to_str().unwrap(), b"v").unwrap();
    let mode = std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
    assert!(mode & 0o022 == 0, "mode {mode:o}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn open_refuses_directories_and_unknowable_lengths() {
    let dir = std::env::temp_dir().join(format!("gridcraft-fs-safety-open-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(read_file(dir.to_str().unwrap()).is_err(), "a directory is not a regular file");
    assert!(read_file(dir.join("missing").to_str().unwrap()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_export_refuses_an_outsized_grid_instead_of_growing_forever() {
    // #2387: the used rectangle alone bounds the output (every row writes the whole
    // column span), so a sparse sheet with one cell at A1 and another far right
    // 300 rows down is 4.9M cells → refused before a byte of `<td>` is written.
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::Text("start".into()));
    sh.set_value(CellRef::new(300, 16383), gridcraft_core::Value::Text("corner".into()));
    let err = to_html(&wb, 0).unwrap_err();
    assert!(err.to_string().contains("too tall × wide"), "{err}");
    // A normal sheet is fine.
    let mut ok = Workbook::new();
    ok.sheet_mut(0).unwrap().set_value(CellRef::new(5, 2), gridcraft_core::Value::Text("t".into()));
    assert!(to_html(&ok, 0).is_ok());
}

#[test]
fn html_export_embeds_images_as_data_uris() {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().images.push(gridcraft_model::Image {
        id: 1,
        anchor: gridcraft_model::Anchor { cell: CellRef::new(0, 0), dx: 0.0, dy: 0.0, width: 72.0, height: 36.0, mode: Default::default() },
        data: vec![1, 2, 3].into(),
        mime: "image/png".into(),
        alt: "a \"logo\"".into(),
    });
    let html = to_html(&wb, 0).unwrap();
    assert!(
        html.contains("<img src=\"data:image/png;base64,AQID\" alt=\"a &quot;logo&quot;\" style=\"position:absolute;left:0px;top:0px;width:72px;height:36px\"></div></body>"),
        "{html}"
    );
    assert!(html.contains("<div style=\"position:relative\"><table"), "{html}");
    assert!(!to_html(&Workbook::new(), 0).unwrap().contains("<img"), "no images, no <img>");
}

#[test]
fn html_export_places_images_at_their_anchor_using_sheet_geometry() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.cols.insert(1, gridcraft_model::LineInfo { size: Some(100.0), ..Default::default() });
    sh.rows.insert(1, gridcraft_model::LineInfo { size: Some(30.0), ..Default::default() });
    sh.images.push(gridcraft_model::Image {
        id: 1,
        anchor: gridcraft_model::Anchor { cell: CellRef::new(3, 2), dx: 5.0, dy: 7.0, width: 10.0, height: 20.0, mode: Default::default() },
        data: vec![1].into(),
        mime: "image/png".into(),
        alt: String::new(),
    });
    sh.set_value(CellRef::new(1, 1), gridcraft_core::Value::Number(1.0));
    let html = to_html(&wb, 0).unwrap();
    // The table starts at B2; the picture is at C4: one 100 px column and two rows (30 + 20) further on.
    assert!(html.contains("<col style=\"width:100px\">"), "{html}");
    assert!(html.contains("<tr style=\"height:30px\">"), "{html}");
    assert!(html.contains("left:105px;top:57px;width:10px;height:20px"), "{html}");
}

fn book_with_picture(anchor: &str, cell_at: (u32, u32)) -> Workbook {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_value(CellRef::new(cell_at.0, cell_at.1), gridcraft_core::Value::Number(1.0));
    sh.images.push(gridcraft_model::Image {
        id: 1,
        anchor: gridcraft_model::Anchor {
            cell: CellRef::parse(anchor).unwrap(),
            dx: 0.0,
            dy: 0.0,
            width: 10.0,
            height: 10.0,
            mode: Default::default(),
        },
        data: vec![1].into(),
        mime: "image/png".into(),
        alt: String::new(),
    });
    wb
}

#[test]
fn html_export_places_images_past_hidden_columns_and_rows() {
    let mut wb = book_with_picture("C3", (0, 0));
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_value(CellRef::new(2, 2), gridcraft_core::Value::Number(2.0));
    sh.cols.insert(1, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
    sh.rows.insert(1, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
    let html = to_html(&wb, 0).unwrap();
    // Column B and row 2 take no room: C3 is one default column and row from A1.
    assert_eq!(html.matches("<col ").count(), 2, "{html}");
    assert_eq!(html.matches("<tr ").count(), 2, "{html}");
    assert!(html.contains("left:64px;top:20px"), "{html}");
}

#[test]
fn html_export_places_images_above_and_left_of_the_table() {
    let html = to_html(&book_with_picture("A1", (2, 2)), 0).unwrap();
    // The table starts at C3, so a picture at A1 sits before it instead of failing.
    assert!(html.contains("left:-128px;top:-40px"), "{html}");
}

#[test]
fn html_export_marks_right_to_left_sheets() {
    use gridcraft_core::Value;
    use gridcraft_model::HAlign;
    let wb = book(true, &[("A1", Value::text("Hello!"), HAlign::General), ("B1", Value::Number(-5.0), HAlign::General)]);
    let html = to_html(&wb, 0).unwrap();
    assert!(html.contains("<body><table dir=\"rtl\">"), "{html}");
    assert!(html.contains("<td style=\"\" dir=\"auto\">Hello!</td>"), "Latin text keeps its own direction: {html}");
    assert!(html.contains("<td style=\"text-align:left;\" dir=\"ltr\">-5</td>"), "numbers mirror to the left and keep their sign in front: {html}");
    let clip = copied(&wb, "A1:B1");
    assert!(clip.starts_with("<table dir=\"rtl\" style="), "{clip}");
    assert_eq!(clip.matches("dir=\"auto\"").count(), 1, "{clip}");
    assert!(clip.contains("text-align:left;\" dir=\"ltr\">-5</td>"), "{clip}");

    let ltr = book(false, &[("A1", Value::text("Hello!"), HAlign::General)]);
    assert!(to_html(&ltr, 0).unwrap().contains("<body><table><tr>"));
    assert!(copied(&ltr, "A1").starts_with("<table style="));
}

#[test]
fn html_export_lets_arabic_general_text_align_by_direction() {
    use gridcraft_core::Value;
    use gridcraft_model::HAlign;
    let wb = book(
        false,
        &[
            ("A1", Value::text("مرحبا!"), HAlign::General),
            ("B1", Value::text("שלום"), HAlign::Left),
            ("C1", Value::text("مرحبا"), HAlign::Center),
            ("D1", Value::text("Hello"), HAlign::General),
        ],
    );
    let html = to_html(&wb, 0).unwrap();
    assert!(html.contains("<td style=\"\" dir=\"auto\">مرحبا!</td>"), "General Arabic text aligns by its direction: {html}");
    assert!(html.contains("<td style=\"text-align:left;\" dir=\"auto\">שלום</td>"), "explicit alignment stays: {html}");
    assert!(html.contains("<td style=\"text-align:center;\" dir=\"auto\">مرحبا</td>"), "{html}");
    assert!(html.contains("<td style=\"text-align:left;\">Hello</td>"), "{html}");
    assert!(html.contains("<body><table><tr>"), "a left-to-right sheet has no table direction: {html}");

    let clip = copied(&wb, "A1:D1");
    let cells: Vec<&str> = clip.split("<td ").skip(1).collect();
    assert_eq!(cells.len(), 4, "{clip}");
    assert!(cells[0].contains("dir=\"auto\"") && !cells[0].contains("text-align"), "{clip}");
    assert!(cells[1].contains("text-align:left;") && cells[1].contains("dir=\"auto\""), "{clip}");
    assert!(cells[3].contains("text-align:left;") && !cells[3].contains("dir="), "{clip}");
}

#[test]
fn html_export_of_latin_ltr_sheet_is_unchanged() {
    use gridcraft_core::Value;
    use gridcraft_model::HAlign;
    let mut wb = book(
        false,
        &[
            ("A1", Value::text("Name"), HAlign::General),
            ("B1", Value::text("Qty"), HAlign::Center),
            ("A2", Value::text("Apples & <pears>"), HAlign::General),
            ("B2", Value::Number(3.0), HAlign::General),
            ("A3", Value::text("Total"), HAlign::Right),
        ],
    );
    wb.sheet_mut(0).unwrap().merges.push(RangeRef::parse("A3:B3").unwrap());
    assert_eq!(
        to_html(&wb, 0).unwrap(),
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Sheet1</title><style>table{border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt}td{border:1px solid #d4d4d4;padding:2px 4px;white-space:nowrap}</style></head><body><table>\
             <tr><td style=\"text-align:left;\">Name</td><td style=\"text-align:center;\">Qty</td></tr>\
             <tr><td style=\"text-align:left;\">Apples &amp; &lt;pears&gt;</td><td style=\"text-align:right;\">3</td></tr>\
             <tr><td style=\"text-align:right;\" rowspan=\"1\" colspan=\"2\">Total</td></tr>\
             </table></body></html>"
    );
    let cell = "border:1px solid #d4d4d4;padding:2px 4px;white-space:pre-wrap;";
    assert_eq!(
        copied(&wb, "A1:B2"),
        format!(
            "<table style=\"border-collapse:collapse;font-family:Calibri,Carlito,Arial,sans-serif;font-size:11pt\">\
                 <tr><td style=\"{cell}text-align:left;\">Name</td><td style=\"{cell}text-align:center;\">Qty</td></tr>\
                 <tr><td style=\"{cell}text-align:left;\">Apples &amp; &lt;pears&gt;</td><td style=\"{cell}text-align:right;\">3</td></tr></table>"
        )
    );
}

#[test]
fn html_export_fails_instead_of_dropping_cells_past_the_row_limit() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::text("near"));
    sh.set_value(CellRef::new(149_999, 0), gridcraft_core::Value::text("far"));
    assert!(save_bytes(&wb, "book.html").is_err());
    let sh = wb.sheet_mut(0).unwrap();
    sh.remove_cell(CellRef::new(149_999, 0));
    sh.set_value(CellRef::new(100_000, 0), gridcraft_core::Value::text("far"));
    let html = String::from_utf8(save_bytes(&wb, "book.html").unwrap()).unwrap();
    assert!(html.contains(">near</td>") && html.contains(">far</td>"));
}

#[test]
fn macros_are_saved_only_into_xlsm() {
    let mut wb = Workbook::new();
    wb.vba_project = Some(b"vba".to_vec().into());
    let has_macros = |path: &str| {
        let bytes = save_bytes(&wb, path).unwrap();
        gridcraft_xlsx::read_xlsx(&bytes).unwrap().0.vba_project.is_some()
    };
    assert!(has_macros("book.xlsm"));
    assert!(has_macros("BOOK.XLSM"));
    assert!(!has_macros("book.xlsx"), "an .xlsx never carries a VBA project");
    assert!(has_macros("book.xltm"), "a macro-enabled template keeps the VBA project");
    assert!(!has_macros("book.xltx"), "a macro-free template never carries a VBA project");
    assert!(wb.vba_project.is_some(), "the open workbook keeps its macros");
}

#[test]
fn csv_named_history_does_not_get_the_reserved_sheet_name() {
    for file in ["History.csv", "hIsToRy.csv", "history.tsv"] {
        let (wb, _) = open_bytes(file, b"1\n").unwrap();
        let name = wb.sheets[0].name.clone();
        assert!(wb.check_sheet_name(&name, Some(0)).is_ok(), "{file} opened as {name:?}");
    }
    let (wb, _) = open_bytes("Sales.csv", b"1\n").unwrap();
    assert_eq!(wb.sheets[0].name, "Sales");
}

/// A save that can't be put in place (here a folder already has the workbook's name, as
/// with a file another app holds locked) fails without leaving `<name>.sctmp` behind.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_save_leaves_no_temporary_file() {
    let dir = std::env::temp_dir().join(format!("gridcraft-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blocked = dir.join("Book1.xlsx");
    std::fs::create_dir(&blocked).unwrap();
    assert!(write_file(blocked.to_str().unwrap(), b"workbook").is_err());
    assert!(!dir.join("Book1.xlsx.sctmp").exists(), "the temporary file is removed");
    // An ordinary save still works and leaves only the workbook.
    let ok = dir.join("Book2.xlsx");
    write_file(ok.to_str().unwrap(), b"workbook").unwrap();
    assert_eq!(std::fs::read(&ok).unwrap(), b"workbook");
    assert!(!dir.join("Book2.xlsx.sctmp").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// While a script runs (#2178), file reads and writes are refused whatever command or
/// parameter asked for them; nesting and dropping the sandbox restore access.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn script_sandbox_refuses_file_access_until_dropped() {
    let dir = std::env::temp_dir().join(format!("gridcraft-sandbox-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let existing = dir.join("in.csv");
    std::fs::write(&existing, b"a,b\n").unwrap();
    let (existing, out) = (existing.to_string_lossy().to_string(), dir.join("out.xlsx").to_string_lossy().to_string());
    {
        let _outer = ScriptSandbox::enter();
        {
            let _inner = ScriptSandbox::enter();
            assert!(read_file(&existing).is_err());
        }
        assert!(read_file(&existing).is_err(), "the outer sandbox still holds");
        assert!(write_file(&out, b"leak").is_err());
        assert!(!std::path::Path::new(&out).exists());
    }
    assert_eq!(read_file(&existing).unwrap(), b"a,b\n");
    write_file(&out, b"ok").unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A minimal compound (CFB/OLE2) file — FAT in sector 0, directory in sector 1 — holding a
/// root entry and the given stream names.
fn cfb(streams: &[&str]) -> Vec<u8> {
    let mut b = vec![0u8; 512 * 3];
    let mut put = |at: usize, v: &[u8]| b[at..at + v.len()].copy_from_slice(v);
    put(0, &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    put(30, &9u16.to_le_bytes());
    put(44, &1u32.to_le_bytes());
    put(48, &1u32.to_le_bytes());
    put(68, &0xFFFF_FFFEu32.to_le_bytes());
    for i in 1..109 {
        put(76 + i * 4, &0xFFFF_FFFFu32.to_le_bytes());
    }
    put(512, &0xFFFF_FFFDu32.to_le_bytes());
    put(516, &0xFFFF_FFFEu32.to_le_bytes());
    for (k, name) in std::iter::once("Root Entry").chain(streams.iter().copied()).enumerate() {
        let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        put(1024 + k * 128, &units);
        put(1024 + k * 128 + 64, &(units.len() as u16 + 2).to_le_bytes());
    }
    b
}

/// Cut-list rows as a CAD/CAM program exports them: `;`-separated, CRLF, part codes in the last
/// two columns, a part name containing a comma.
const CUT_LIST: &str = "1;511;184;P3_L1_led;2162PE;16;1MM_2162PE;1MM_2162PE;;;1244121406FB;\r\n\
                            1;562.5;510;Komoda, lijevo;2162PE;19;1MM_2162PE;;;;12441214071A;124412140710\r\n";

#[test]
fn csv_save_keeps_the_separator_it_was_opened_with() {
    let (wb, _) = open_bytes("cut list.csv", CUT_LIST.as_bytes()).unwrap();
    assert_eq!(wb.csv_delimiter, Some(b';'));
    assert_eq!(String::from_utf8(save_bytes(&wb, "cut list.csv").unwrap()).unwrap(), CUT_LIST);
    // Save As another .csv name keeps it too; a comma file stays a comma file (quoting only
    // fields that contain the separator in use).
    assert_eq!(String::from_utf8(save_bytes(&wb, "copy.CSV").unwrap()).unwrap(), CUT_LIST);
    let commas = "\"Komoda, lijevo\",b\r\n1,x;y\r\n";
    let (wb, _) = open_bytes("plain.csv", commas.as_bytes()).unwrap();
    assert_eq!(String::from_utf8(save_bytes(&wb, "plain.csv").unwrap()).unwrap(), commas);
    // A tab-separated file named .csv round-trips with tabs.
    let tabs = "a\tb\r\n1\t2\r\n";
    let (wb, _) = open_bytes("tabs.csv", tabs.as_bytes()).unwrap();
    assert_eq!(String::from_utf8(save_bytes(&wb, "tabs.csv").unwrap()).unwrap(), tabs);
}

#[test]
fn csv_save_of_a_workbook_not_read_from_csv_uses_commas() {
    let mut wb = Workbook::new();
    if let Some(sh) = wb.sheet_mut(0) {
        sh.set_value(CellRef::new(0, 0), gridcraft_core::Value::text("a"));
        sh.set_value(CellRef::new(0, 1), gridcraft_core::Value::Number(1.0));
    }
    assert_eq!(save_bytes(&wb, "new.csv").unwrap(), b"a,1\r\n");
    // Through .xlsx the CSV origin is gone, as in Excel.
    let (from_csv, _) = open_bytes("cut list.csv", CUT_LIST.as_bytes()).unwrap();
    let (reopened, _) = open_bytes("book.xlsx", &save_bytes(&from_csv, "book.xlsx").unwrap()).unwrap();
    assert_eq!(reopened.csv_delimiter, None);
    assert!(String::from_utf8(save_bytes(&reopened, "again.csv").unwrap()).unwrap().starts_with("1,511,184,"));
    // A .tsv/.txt saved as .csv gets commas; as .tsv it keeps tabs.
    let (tsv, _) = open_bytes("list.txt", b"a\tb\r\n").unwrap();
    assert_eq!(save_bytes(&tsv, "list.csv").unwrap(), b"a,b\r\n");
    assert_eq!(save_bytes(&tsv, "list.tsv").unwrap(), b"a\tb\r\n");
}

#[test]
fn encrypted_workbook_is_reported_not_misread() {
    let file = gridcraft_xlsx::testing::encrypt_workbook(b"package", "pw");
    let msg = open_bytes("secret.xlsx", &file).unwrap_err().to_string();
    assert!(msg.contains(PASSWORD_REQUIRED), "got: {msg}");
    assert!(!msg.contains("zip"), "must not surface the zip error: {msg}");
    // A compound file naming the streams but unreadable is damage: no password is asked for,
    // and none helps.
    let file = cfb(&["EncryptionInfo", "EncryptedPackage"]);
    for password in [None, Some("pw")] {
        let msg = open_bytes_with_password("secret.xlsx", &file, password).unwrap_err().to_string();
        assert!(msg.contains("damaged") && !msg.contains(PASSWORD_REQUIRED), "got: {msg}");
    }
}

#[test]
fn encryption_no_password_opens_is_reported_without_asking_for_one() {
    let info: &[u8] = &[3, 0, 2, 0, 4, 0, 0, 0]; // Standard header flags: RC4, not AES
    let file = gridcraft_xlsx::testing::compound_file(&[("EncryptionInfo", info), ("EncryptedPackage", &[0; 8])]);
    let msg = open_bytes("rc4.xlsx", &file).unwrap_err().to_string();
    assert!(msg.contains("RC4") && !msg.contains(PASSWORD_REQUIRED), "got: {msg}");
    let irm = gridcraft_xlsx::testing::compound_file(&[("EncryptedPackage", &[0; 8])]);
    let msg = open_bytes("irm.xlsx", &irm).unwrap_err().to_string();
    assert!(msg.contains("rights-management") && !msg.contains("damaged"), "got: {msg}");
}

#[test]
fn other_compound_files_are_not_called_encrypted() {
    let err = open_bytes("old.doc", &cfb(&["WordDocument", "\u{5}SummaryInformation"])).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("not an Excel workbook"), "got: {msg}");
    assert!(!msg.contains("password"), "got: {msg}");
}

/// Save As CSV/TSV writes cells as displayed: number formats applied, formula results,
/// whole values in narrow columns (never `#####`), quoting unchanged.
#[test]
fn csv_writes_displayed_text() {
    use gridcraft_core::{CellError, Value};
    use gridcraft_model::{Cell, Formula, LineInfo, NumFmt, StyleId};
    let mut wb = Workbook::new();
    let mut styled = |code: &str| wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
    let (date, money, pct, frac, text, fixed, thousands) =
        (styled("yyyy-mm-dd"), styled("$#,##0.00"), styled("0.0%"), styled("# ?/?"), styled("@"), styled("0.00"), styled("#,##0"));
    let sh = wb.sheet_mut(0).unwrap();
    let cells = [
        ("A1", Cell { value: Value::Number(46023.0), formula: None, style: date }),
        ("B1", Cell { value: Value::Number(1234.5), formula: None, style: money }),
        ("C1", Cell { value: Value::Number(0.125), formula: None, style: pct }),
        ("D1", Cell { value: Value::Number(1.5), formula: None, style: frac }),
        ("E1", Cell { value: Value::text("007"), formula: None, style: text }),
        ("F1", Cell { value: Value::Error(CellError::NA), formula: None, style: money }),
        ("G1", Cell { value: Value::Bool(true), formula: None, style: StyleId::DEFAULT }),
        ("A2", Cell { value: Value::Number(46024.0), style: date, ..Cell::formula(Formula::new("A1+1")) }),
        ("B2", Cell { value: Value::Number(12345678.9), formula: None, style: fixed }),
        ("C2", Cell { value: Value::Number(-0.25), formula: None, style: StyleId::DEFAULT }),
        ("D2", Cell { value: Value::Number(1234567.0), formula: None, style: thousands }),
    ];
    for (a, c) in cells {
        sh.set_cell(CellRef::parse(a).unwrap(), c);
    }
    // Column B is far too narrow for its numbers: the grid shows #####, the file must not.
    sh.cols.insert(1, LineInfo { size: Some(1.0), ..Default::default() });
    sh.show_formulas = true;
    let csv = String::from_utf8(save_bytes(&wb, "out.csv").unwrap()).unwrap();
    assert_eq!(csv, "2026-01-01,\"$1,234.50\",12.5%,1 1/2,007,#N/A,TRUE\r\n2026-01-02,12345678.90,-0.25,\"1,234,567\",,,\r\n");
    let tsv = String::from_utf8(save_bytes(&wb, "out.tsv").unwrap()).unwrap();
    assert!(tsv.starts_with("2026-01-01\t$1,234.50\t12.5%\t"), "{tsv}");
}

#[test]
fn delimited_exports_preserve_general_precision_and_explicit_number_formats() {
    use gridcraft_core::Value;
    use gridcraft_model::{Cell, NumFmt, StyleId};
    let mut workbook = Workbook::new();
    let scientific = workbook.styles.derive(StyleId::DEFAULT, |style| style.num_fmt = NumFmt::new("0.00E+00"));
    let sheet = workbook.sheet_mut(0).unwrap();
    sheet.set_cell(CellRef::new(0, 0), Cell { value: Value::Number(124412140710.0), ..Default::default() });
    sheet.set_cell(CellRef::new(0, 1), Cell { value: Value::Number(1.23456789012345), ..Default::default() });
    sheet.set_cell(CellRef::new(0, 2), Cell { value: Value::Number(124412140710.0), style: scientific, ..Default::default() });
    for (path, separator) in [("data.csv", ","), ("data.tsv", "\t")] {
        let output = String::from_utf8(save_bytes(&workbook, path).unwrap()).unwrap();
        assert_eq!(output, format!("124412140710{separator}1.23456789012345{separator}1.24E+11\r\n"));
    }
}

#[test]
fn template_extension_saves_template_content_type() {
    let bytes = save_bytes(&Workbook::new(), "Budget.xltx").unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut types = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("[Content_Types].xml").unwrap(), &mut types).unwrap();
    assert!(
        types.contains(r#"PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml""#),
        "{types}"
    );
}

/// Where `part`'s entry starts in the central directory of `package`.
fn central_entry(package: &[u8], part: &str) -> usize {
    let mut at = 0;
    while let Some(i) = package.get(at..).and_then(|rest| rest.windows(4).position(|w| w == b"PK\x01\x02")) {
        let entry = at + i;
        let len = u16::from_le_bytes([package[entry + 28], package[entry + 29]]) as usize;
        if &package[entry + 46..entry + 46 + len] == part.as_bytes() {
            return entry;
        }
        at = entry + 46;
    }
    panic!("{part} is not in the package");
}

/// `package` with `part` declaring `size` unpacked bytes: the reader refuses the part on the
/// declared size, so no real 512 MB part has to be built.
fn declaring(mut package: Vec<u8>, part: &str, size: u32) -> Vec<u8> {
    let at = central_entry(&package, part);
    package[at + 24..at + 28].copy_from_slice(&size.to_le_bytes());
    package
}

#[test]
fn a_part_over_the_size_limit_says_how_to_get_the_data_in() {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), gridcraft_core::Value::text("big"));
    let package = save_bytes(&wb, "big.xlsx").unwrap();
    // The size reported in storytold/gridcraft#1242.
    let msg = open_bytes("big.xlsx", &declaring(package.clone(), "xl/worksheets/sheet1.xml", 540_278_731)).unwrap_err().to_string();
    assert!(msg.starts_with("We can't open 'big.xlsx': "), "{msg}");
    assert!(msg.contains("xl/worksheets/sheet1.xml") && msg.contains("515 MB") && msg.contains("512 MB"), "{msg}");
    assert!(msg.contains("several sheets") && msg.contains("CSV"), "{msg}");
    assert!(!msg.contains("bytes"), "{msg}");
    // Splitting sheets doesn't shrink the shared strings, which every sheet uses: only CSV helps.
    let msg = open_bytes("big.xlsx", &declaring(package.clone(), "xl/sharedStrings.xml", 540_278_731)).unwrap_err().to_string();
    assert!(msg.contains("xl/sharedStrings.xml") && msg.contains("515 MB") && msg.contains("CSV"), "{msg}");
    assert!(!msg.contains("several sheets"), "{msg}");
    // Just over the limit never reads as a size equal to it.
    let msg = open_bytes("big.xlsx", &declaring(package, "xl/worksheets/sheet1.xml", 512 * 1024 * 1024 + 1)).unwrap_err().to_string();
    assert!(msg.contains("more than 512 MB"), "{msg}");
    // A package that fails for another reason keeps its own error, oversized part or not.
    let mut broken = declaring(save_bytes(&wb, "big.xlsx").unwrap(), "xl/worksheets/sheet1.xml", 540_278_731);
    let at = central_entry(&broken, "xl/workbook.xml");
    broken[at + 46 + "xl/workbook.xml".len() - 1] = b'x';
    let msg = open_bytes("big.xlsx", &broken).unwrap_err().to_string();
    assert!(msg.starts_with("We can't open 'big.xlsx': ") && !msg.contains("unpacks"), "{msg}");
}

#[test]
fn content_wins_over_a_mismatched_extension() {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), gridcraft_core::Value::Number(7.0));
    let package = save_bytes(&wb, "book.xlsx").unwrap();
    for name in ["book.xlsx", "book.ods", "book.xlsb", "book.csv"] {
        let (opened, _) = open_bytes(name, &package).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(opened.sheet(0).unwrap().value(CellRef::new(0, 0)), gridcraft_core::Value::Number(7.0), "{name}");
    }
    for name in ["data.xlsx", "data.csv"] {
        let (opened, _) = open_bytes(name, b"a,b\n1,2\n").unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(opened.sheet(0).unwrap().value(CellRef::new(0, 1)), gridcraft_core::Value::text("b"), "{name}");
    }
}
