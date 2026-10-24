//! The multi-threaded calculation setting and thread count: the command and the file round-trip.

use serde_json::json;

use super::s;

#[test]
fn calculation_options_set_the_thread_count() {
    let mut s = s();
    let r = s.execute("formulas.calculationOptions", json!({"multiThreaded": false, "threads": 5000})).unwrap();
    assert_eq!((r["multiThreaded"].as_bool(), r["threads"].as_u64()), (Some(false), Some(1024)));
    assert!(!s.doc().unwrap().wb.calc.multi_threaded);
    assert_eq!(s.doc().unwrap().wb.calc.threads, 1024);
    let r = s.execute("formulas.calculationOptions", json!({"multiThreaded": true, "threads": 0})).unwrap();
    assert_eq!((r["multiThreaded"].as_bool(), r["threads"].as_u64()), (Some(true), Some(0)));
}

#[test]
fn a_workbooks_thread_settings_survive_the_file() {
    let mut s = s();
    s.execute("formulas.calculationOptions", json!({"multiThreaded": false, "threads": 3})).unwrap();
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    assert!(!wb.calc.multi_threaded);
    assert_eq!(wb.calc.threads, 3);
    s.execute("formulas.calculationOptions", json!({"multiThreaded": true})).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap()).unwrap();
    assert!(wb.calc.multi_threaded && wb.calc.threads == 3);
    let r = s.execute("file.options", json!({})).unwrap();
    assert_eq!((r["calculation"]["multiThreaded"].as_bool(), r["calculation"]["threads"].as_u64()), (Some(true), Some(3)));
}
