//! Page-boundary moves use the same pagination and settings as printing and XLSX.
use gridcraft_engine::{
    Session,
    cmd::print::{PrintOptions, layout_sheet},
};
use serde_json::json;

/// Creates a synthetic large print area with titles and headings so scale calculations include them.
fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "pageLayout.pageSetup",
        json!({"printArea":"B2:Z100", "titleRows":"1:2", "titleCols":"A:B", "headings":true, "rowBreaks":[12,25], "colBreaks":["G","K"]}),
    )
    .unwrap();
    s
}

/// Both axes retain unrelated breaks, produce the requested page boundary, round-trip, and undo once.
#[test]
fn moves_fit_printed_pages_and_round_trip() {
    for (axis, from, to) in [("row", 25, 55), ("column", 11, 16)] {
        let mut s = session();
        let before = s.doc().unwrap().wb.active().unwrap().print.clone();
        s.execute("pageLayout.movePageBreak", json!({"axis":axis,"from":from,"to":to})).unwrap();
        let wb = &s.doc().unwrap().wb;
        let sh = wb.active().unwrap();
        let lay = layout_sheet(wb, 0, &PrintOptions::default(), None).unwrap();
        let bands = if axis == "row" { &lay.row_bands } else { &lay.col_bands };
        assert!(bands.iter().any(|(_, end)| *end == to - 2));
        let previous = if axis == "row" { 11 } else { 6 };
        assert!(bands.iter().any(|(start, end)| *start == previous && *end == to - 2), "{bands:?}");
        assert!(sh.print.scale <= before.scale);
        if axis == "row" {
            assert_eq!(sh.print.col_breaks, before.col_breaks);
        } else {
            assert_eq!(sh.print.row_breaks, before.row_breaks);
        }
        let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
        let (loaded, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        assert_eq!(serde_json::to_value(&loaded.active().unwrap().print).unwrap(), serde_json::to_value(&sh.print).unwrap());
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.doc().unwrap().wb.active().unwrap().print).unwrap(), serde_json::to_value(before).unwrap());
    }
}

/// Automatic breaks can be moved outwards without leaving the old automatic break behind.
#[test]
fn outward_automatic_break_reduces_scale() {
    let mut s = session();
    s.execute("pageLayout.pageSetup", json!({"rowBreaks":[],"colBreaks":[]})).unwrap();
    let lay = layout_sheet(&s.doc().unwrap().wb, 0, &PrintOptions::default(), None).unwrap();
    let from = lay.row_bands[0].1 + 2;
    let to = from + 8;
    s.execute("pageLayout.movePageBreak", json!({"axis":"row","from":from,"to":to})).unwrap();
    let after = layout_sheet(&s.doc().unwrap().wb, 0, &PrintOptions::default(), None).unwrap();
    assert_eq!(after.row_bands[0], (1, to - 2));
    assert!(after.scale < lay.scale);
}

/// An inward move preserves an existing integer percentage without float-rounding drift.
#[test]
fn inward_move_preserves_fixed_scale() {
    let mut s = session();
    s.execute("pageLayout.pageSetup", json!({"scale":53})).unwrap();
    s.execute("pageLayout.movePageBreak", json!({"axis":"row","from":12,"to":10})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().print.scale, 53);
}

/// Crossing another manual break keeps it and fits from that surviving boundary, without needless shrinking.
#[test]
fn moving_past_another_manual_break_preserves_scale_when_it_fits() {
    let mut s = session();
    s.execute("pageLayout.movePageBreak", json!({"axis":"row","from":12,"to":55})).unwrap();
    let sh = s.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.print.scale, 100);
    assert_eq!(sh.print.row_breaks, vec![24, 54]);
}

/// Fitted moves on either axis discard inactive breaks, preserve perpendicular pagination, and undo together.
#[test]
fn fitted_pages_switch_to_fixed_scale_and_explicit_sheet_is_respected() {
    for rows in [true, false] {
        let mut s = session();
        s.execute("pageLayout.pageSetup", json!({"fitWidth":2,"fitHeight":2})).unwrap();
        let before = serde_json::to_value(&s.doc().unwrap().wb.active().unwrap().print).unwrap();
        let lay = layout_sheet(&s.doc().unwrap().wb, 0, &PrintOptions::default(), None).unwrap();
        let bands = if rows { &lay.row_bands } else { &lay.col_bands };
        assert!(bands.len() > 1);
        let from = bands[0].1 + 2;
        let to = from + 2;
        s.execute("pageLayout.movePageBreak", json!({"sheet":0,"axis":if rows {"row"} else {"column"},"from":from,"to":to})).unwrap();
        let wb = &s.doc().unwrap().wb;
        let after = layout_sheet(wb, 0, &PrintOptions::default(), None).unwrap();
        assert!(after.settings.fit_to.is_none());
        let (moved, perpendicular) = if rows { (&after.row_bands, &after.col_bands) } else { (&after.col_bands, &after.row_bands) };
        assert_eq!(moved[0].1, to - 2);
        assert!(perpendicular.len() <= 2, "{perpendicular:?}");
        let print = &wb.active().unwrap().print;
        let (changed, other) = if rows { (&print.row_breaks, &print.col_breaks) } else { (&print.col_breaks, &print.row_breaks) };
        assert_eq!(changed, &vec![to - 1]);
        assert!(other.is_empty());
        let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
        let (loaded, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        assert_eq!(serde_json::to_value(&loaded.active().unwrap().print).unwrap(), serde_json::to_value(print).unwrap());
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(serde_json::to_value(&s.doc().unwrap().wb.active().unwrap().print).unwrap(), before);
        assert!(s.execute("pageLayout.movePageBreak", json!({"sheet":999,"axis":"row","from":12,"to":13})).is_err());
    }
}
/// Invalid, excessive and sub-minimum-scale destinations leave settings untouched; manual removal works.
#[test]
fn hostile_destinations_are_atomic_and_manual_breaks_can_be_removed() {
    let mut s = session();
    let before = serde_json::to_value(&s.doc().unwrap().wb.active().unwrap().print).unwrap();
    for params in
        [json!({}), json!({"axis":"row","from":12,"to":0}), json!({"axis":"row","from":12,"to":u64::MAX}), json!({"axis":"row","from":3,"to":4})]
    {
        assert!(s.execute("pageLayout.movePageBreak", params).is_err());
        assert_eq!(serde_json::to_value(&s.doc().unwrap().wb.active().unwrap().print).unwrap(), before);
    }
    s.execute("pageLayout.movePageBreak", json!({"axis":"row","from":12,"to":1})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().print.row_breaks, vec![24]);
    s.execute("pageLayout.pageSetup", json!({"printArea":"A1:D1048575","rowBreaks":[12]})).unwrap();
    let before = serde_json::to_value(&s.doc().unwrap().wb.active().unwrap().print).unwrap();
    assert!(s.execute("pageLayout.movePageBreak", json!({"axis":"row","from":12,"to":1048575})).is_err());
    assert_eq!(serde_json::to_value(&s.doc().unwrap().wb.active().unwrap().print).unwrap(), before);
}
