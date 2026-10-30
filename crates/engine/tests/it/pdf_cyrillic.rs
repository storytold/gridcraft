//! Ukrainian text reaches exported PDFs in an embedded system font (Tinta Calculo, stage 1).
//! Tests that need a system font skip when the machine has none.

use serde_json::json;

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn exported_pdf_embeds_a_font_for_cyrillic() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "Сімейний бюджет"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Їжа"})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "1250"})).unwrap();
    let out = s.execute("file.exportPdf", json!({})).unwrap();
    let pdf = gridcraft_engine::io::base64_decode(out["base64"].as_str().unwrap()).unwrap();
    // The export installed the system fonts itself; asking afterwards only reports them.
    gridcraft_engine::pdf_fonts::ensure_installed();
    if !gridcraft_pdf::can_draw(gridcraft_pdf::Font::Helvetica, 'С') {
        eprintln!("no system TrueType font with Cyrillic; skipped");
        return;
    }
    assert!(find(&pdf, b"/FontFile2") && find(&pdf, b"/ToUnicode"), "Cyrillic is embedded");
    // `С` (U+0421) is in the ToUnicode map, so the text can be searched and copied.
    assert!(find(&pdf, b"<0421>"));
}

#[test]
fn font_search_survives_missing_and_broken_folders() {
    let dir = std::env::temp_dir().join(format!("gridcraft-pdf-fonts-{}", std::process::id()));
    let nested = dir.join("a").join("b");
    std::fs::create_dir_all(&nested).unwrap();
    // A file with a font's name that is not a font is skipped.
    std::fs::write(nested.join("arial.ttf"), b"not a font").unwrap();
    std::fs::write(dir.join("LiberationSans-Regular.ttf"), []).unwrap();
    assert_eq!(gridcraft_engine::pdf_fonts::install_from(&[dir.clone(), dir.join("missing")]), 0);
    std::fs::remove_dir_all(&dir).ok();
}
