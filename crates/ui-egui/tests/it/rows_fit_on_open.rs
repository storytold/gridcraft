//! Files named on the command line open on the first frame, so rows of wrapped text fit with the
//! fonts the grid draws with rather than the engine's estimate.

use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

const TEXTS: [&str; 3] =
    ["Check the quote before signing it today please.", "Check the quote before signing it today please, then file it away.", "Short."];

/// The test workbook's file, removed when the test ends, also when an assertion fails.
struct TempBook(std::path::PathBuf);

impl Drop for TempBook {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A workbook with wrapped text in a column 45 characters wide and no row heights, as a file that
/// leaves its rows to fit on open.
fn book() -> TempBook {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.columnWidth", json!({"range": "A1", "chars": 45})).unwrap();
    for (i, t) in TEXTS.iter().enumerate() {
        s.execute("cell.set", json!({"cell": format!("A{}", i + 1), "input": t})).unwrap();
    }
    s.execute("home.wrapText", json!({"range": "A1:A3", "on": true})).unwrap();
    let mut wb = (*s.active().unwrap().wb).clone();
    std::sync::Arc::make_mut(&mut wb.sheets[0]).rows.clear();
    // One file per test: the tests run side by side, and each removes its own.
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("gridcraft-rows-fit-{}-{n}.xlsx", std::process::id()));
    std::fs::write(&path, gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
    TempBook(path)
}

#[test]
fn a_startup_file_opens_on_the_first_frame_with_rows_fit_by_the_grids_fonts() {
    let book = book();
    let path = &book.0;
    let mut s = Session::new();
    s.new_workbook();
    let mut app = crate::common::app(s);
    app.open_on_first_frame(vec![path.to_string_lossy().into_owned()]);
    let mut h = crate::common::builder().with_size(egui::vec2(1000.0, 700.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    let app = h.state();
    let d = app.session.active().unwrap();
    assert_eq!(d.wb.sheets[0].value(gridcraft_engine::core::CellRef::new(2, 0)).display(), "Short.", "the file opened");
    // Its rows are the ones the grid's own measure gives, not the estimate's.
    let measure = app.session.text_width.clone().expect("the grid's measure is installed");
    let bytes = std::fs::read(path).unwrap();
    let (expected, _) = gridcraft_engine::io::open_bytes_measured("book.xlsx", &bytes, None, false, Some(&measure)).unwrap();
    for row in 0..3 {
        assert_eq!(d.wb.sheets[0].row_height(row), expected.sheets[0].row_height(row), "row {}", row + 1);
    }
    assert!(!d.rows_unfit && !d.is_dirty(), "fitted once the fonts were ready, with nothing to save");
}

/// A workbook opened once the app is running fits its rows on a later frame, after its fonts are
/// installed, and with the grid's measure.
#[test]
fn a_workbook_opened_later_fits_its_rows_once_its_fonts_are_ready() {
    let book = book();
    let path = &book.0;
    let mut s = Session::new();
    s.new_workbook();
    let app = crate::common::app(s);
    let mut h = crate::common::builder().with_size(egui::vec2(1000.0, 700.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(2);
    h.state_mut().open_path(&path.to_string_lossy());
    assert!(h.state().session.active().unwrap().rows_unfit, "the rows wait for the fonts");
    h.run_steps(2);
    let app = h.state();
    let d = app.session.active().unwrap();
    assert!(!d.rows_unfit && !d.is_dirty());
    let measure = app.session.text_width.clone().expect("the grid's measure is installed");
    let (expected, _) = gridcraft_engine::io::open_bytes_measured("book.xlsx", &std::fs::read(path).unwrap(), None, false, Some(&measure)).unwrap();
    for row in 0..3 {
        assert_eq!(d.wb.sheets[0].row_height(row), expected.sheets[0].row_height(row), "row {}", row + 1);
    }
}
