//! The default reading direction of new sheets (Excel's Default direction option), which a
//! right-to-left interface language turns on.

use gridcraft_engine::Session;
use serde_json::json;

fn directions(session: &Session, doc: usize) -> Vec<bool> {
    session.documents()[doc].wb.sheets.iter().map(|sh| sh.right_to_left).collect()
}

#[test]
fn an_untouched_new_workbook_follows_the_default_direction_and_stays_clean() {
    let mut session = Session::new();
    let fresh = session.new_workbook();
    let edited = session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    assert_eq!(session.active_index(), edited);

    session.set_default_right_to_left(true);

    assert_eq!(directions(&session, fresh), [true], "a new workbook nobody changed follows");
    assert!(!session.documents()[fresh].is_dirty(), "and is not marked changed");
    assert_eq!(directions(&session, edited), [false], "an edited workbook keeps its direction");

    // New workbooks and new sheets start right to left.
    let next = session.new_workbook();
    assert_eq!(directions(&session, next), [true]);
    session.execute("home.insertSheet", json!({})).unwrap();
    assert_eq!(directions(&session, next), [true, true]);

    // Back to left to right: only workbooks still untouched follow.
    session.set_default_right_to_left(false);
    assert_eq!(directions(&session, fresh), [false]);
    assert_eq!(directions(&session, next), [true, true], "inserting a sheet changed it");
}
