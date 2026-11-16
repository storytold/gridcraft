//! File › Options › Save › "Split CSV files into columns when opening them": a preference, on by
//! default. Off, File › Open (and a double-click, which goes through `open_path`) opens a `.csv`
//! with each line whole in column A, and saving it as CSV gives back the same bytes.

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::UiState;
use serde_json::json;

const ERP_IMPORT: &str = ";;numitems;\r\nCON;FS17-1.5BL;100;0,20639344\r\nARN;313.10;1;6,59016393;\r\n";

#[test]
fn it_is_on_by_default_and_older_settings_read_as_on() {
    assert!(UiState::default().csv_split);
    assert!(serde_json::from_str::<UiState>("{}").unwrap().csv_split);
    let off = UiState { csv_split: false, ..UiState::default() };
    assert!(!serde_json::from_str::<UiState>(&serde_json::to_string(&off).unwrap()).unwrap().csv_split, "it survives a restart");
}

#[test]
fn the_command_sets_and_toggles_it() {
    let mut app = crate::common::app(Session::new());
    assert_eq!(app.run("options.csvSplit", json!({"on": false})), Ok(json!({"on": false})));
    assert!(!app.ui.csv_split);
    assert_eq!(app.run("options.csvSplit", json!({})), Ok(json!({"on": true})), "no parameters: it toggles");
}

#[test]
fn opening_a_csv_follows_the_preference() {
    let dir = std::env::temp_dir().join(format!("gridcraft-csv-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("import.csv");
    std::fs::write(&path, ERP_IMPORT).unwrap();
    let path = path.to_str().unwrap().to_string();

    let mut app = crate::common::app(Session::new());
    app.open_path(&path);
    let wb = &app.session.active().unwrap().wb;
    assert!(!wb.csv_unsplit, "split by default");
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(2, 1)), Value::Number(313.1));

    let mut app = crate::common::app(Session::new());
    app.run("options.csvSplit", json!({"on": false})).unwrap();
    app.open_path(&path);
    let wb = &app.session.active().unwrap().wb;
    assert!(wb.csv_unsplit);
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(2, 0)), Value::text("ARN;313.10;1;6,59016393;"));
    app.run("file.save", json!({})).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("import.csv")).unwrap(), ERP_IMPORT, "saved back byte for byte");
    let _ = std::fs::remove_dir_all(&dir);
}
