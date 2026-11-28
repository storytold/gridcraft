//! The multi-threaded calculation setting and thread count: the command and the file round-trip.

use serde_json::json;

use super::s;

#[test]
fn calculation_options_set_the_thread_count() {
    let mut s = s();
    // 1024 is Excel's limit, and the most the command takes.
    let r = s.execute("formulas.calculationOptions", json!({"multiThreaded": false, "threads": 1024})).unwrap();
    assert_eq!((r["multiThreaded"].as_bool(), r["threads"].as_u64()), (Some(false), Some(1024)));
    assert!(!s.doc().unwrap().wb.calc.multi_threaded);
    assert_eq!(s.doc().unwrap().wb.calc.threads, 1024);
    let r = s.execute("formulas.calculationOptions", json!({"multiThreaded": true, "threads": 0})).unwrap();
    assert_eq!((r["multiThreaded"].as_bool(), r["threads"].as_u64()), (Some(true), Some(0)));
    // A count that isn't a non-negative integer is refused, not ignored, and changes nothing. The
    // error names the value given (a text in quotes).
    for (bad, shown) in [(json!(-1), "not -1"), (json!(2.5), "not 2.5"), (json!("eight"), "not \"eight\"")] {
        let e = s.execute("formulas.calculationOptions", json!({"threads": bad})).unwrap_err().to_string();
        assert!(e.contains("non-negative integer") && e.contains(shown), "{bad}: {e}");
    }
    assert_eq!(s.doc().unwrap().wb.calc.threads, 0);
    // So is a multi-threaded flag that isn't a boolean.
    for (bad, shown) in [(json!("yes"), "not \"yes\""), (json!(2.5), "not 2.5")] {
        let e = s.execute("formulas.calculationOptions", json!({"multiThreaded": bad})).unwrap_err().to_string();
        assert!(e.contains("boolean") && e.contains(shown), "{bad}: {e}");
    }
    assert!(s.doc().unwrap().wb.calc.multi_threaded);
}

#[test]
fn a_thread_count_past_the_limit_is_refused_and_changes_nothing() {
    let mut s = s();
    s.execute("formulas.calculationOptions", json!({"multiThreaded": false, "threads": 3})).unwrap();
    // Past u32 too: the error names the count given, not a saturated one.
    for (over, shown) in [(json!(1025), "1025"), (json!(5_000_000_000u64), "5000000000")] {
        let e = s.execute("formulas.calculationOptions", json!({"threads": over, "multiThreaded": true})).unwrap_err().to_string();
        assert!(e.contains("at most 1024") && e.contains(shown), "{over}: {e}");
        let calc = &s.doc().unwrap().wb.calc;
        assert_eq!((calc.threads, calc.multi_threaded), (3, false), "{over}: the settings stay as they were");
    }
}

#[test]
fn a_null_thread_setting_is_not_given() {
    // As for `maxIterations`, null means the setting isn't given: the others still apply.
    let mut s = s();
    s.execute("formulas.calculationOptions", json!({"multiThreaded": false, "threads": 3})).unwrap();
    let r = s.execute("formulas.calculationOptions", json!({"threads": null, "multiThreaded": null, "maxIterations": 50})).unwrap();
    assert_eq!((r["multiThreaded"].as_bool(), r["threads"].as_u64(), r["maxIterations"].as_u64()), (Some(false), Some(3), Some(50)));
    let calc = &s.doc().unwrap().wb.calc;
    assert_eq!((calc.threads, calc.multi_threaded, calc.max_iterations), (3, false, 50));
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

#[test]
fn calculation_threads_survive_a_json_save_and_leave_default_files_unchanged() {
    let save = |s: &mut crate::Session| {
        let r = s.execute("file.saveBytes", json!({"format": "json"})).unwrap();
        crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap()
    };
    // The defaults (several threads, the automatic count) add no keys, so files written before the
    // settings existed are saved back as they were.
    let mut plain = s();
    let text = String::from_utf8(save(&mut plain)).unwrap();
    assert!(!text.contains("multi_threaded") && !text.contains("\"threads\""), "{text}");
    // Other values are written, each on its own, and read back.
    for (multi, threads) in [(false, 3), (false, 0), (true, 3)] {
        let mut s = s();
        s.execute("formulas.calculationOptions", json!({"multiThreaded": multi, "threads": threads})).unwrap();
        let bytes = save(&mut s);
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(text.contains("\"multi_threaded\": false"), !multi, "{text}");
        assert_eq!(text.contains("\"threads\": 3"), threads == 3, "{text}");
        let mut back = super::s();
        back.execute("file.open", json!({"name": "threads.json", "base64": crate::io::base64_encode(&bytes)})).unwrap();
        let calc = &back.doc().unwrap().wb.calc;
        assert_eq!((calc.multi_threaded, calc.threads), (multi, threads));
    }
}

#[test]
fn a_json_thread_count_past_the_limit_is_kept_at_it() {
    let r = s().execute("file.saveBytes", json!({"format": "json"})).unwrap();
    let mut book: serde_json::Value = serde_json::from_slice(&crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap()).unwrap();
    let mut open = |threads: serde_json::Value| {
        book["calc"]["threads"] = threads;
        let mut s = s();
        let bytes = serde_json::to_vec_pretty(&book).unwrap();
        let r = s.execute("file.open", json!({"name": "threads.json", "base64": crate::io::base64_encode(&bytes)}));
        r.map(|r| {
            (s.doc().unwrap().wb.calc.threads, r["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect::<Vec<_>>())
        })
    };
    // 1024 is Excel's limit: kept as it is, without a warning.
    assert_eq!(open(json!(1024)).unwrap(), (1024, vec![]));
    // Past it, the count is kept at 1024 and the warning names the value read.
    for past in [1025u64, 4_294_967_295] {
        let (threads, warnings) = open(json!(past)).unwrap();
        assert_eq!(threads, 1024, "{past}");
        assert_eq!(warnings, vec![format!("threads.json: calculation threads {past} is above Excel's limit; kept 1024")]);
    }
    // A count that isn't one, or doesn't fit, makes the file unreadable: an error, not a panic.
    for bad in [json!(-1), json!(2.5), json!("eight"), json!(4_294_967_296u64)] {
        let e = open(bad.clone()).unwrap_err().to_string();
        assert!(e.contains("not a GridCraft JSON workbook") && e.contains("u32"), "{bad}: {e}");
    }
}
