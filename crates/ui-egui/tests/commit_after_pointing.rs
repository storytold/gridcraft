//! A formula started on one sheet and pointed at another sheet lands on the sheet it started on.

use egui::vec2;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

/// Two sheets, `range` selected on the first, and `=1+2` typed there; then pointing moves to the second sheet.
fn pointing_on_second_sheet(range: &str) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.insertSheet", json!({})).unwrap();
    session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    session.execute("selection.set", json!({"range": range})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().begin_edit(Some("=1+2".into()), false);
    h.run_steps(2);
    // Pointing moves to the other sheet without finishing the edit.
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h
}

#[test]
fn commit_after_pointing_on_another_sheet_writes_to_the_origin_sheet() {
    let mut h = pointing_on_second_sheet("F8");
    assert!(h.state_mut().commit_edit(0, 0, false, false));
    let at = CellRef::parse("F8").unwrap();
    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.active_sheet, 0, "the origin sheet is shown again");
    assert!(wb.sheet(0).unwrap().cell(at).is_some_and(|c| c.formula.is_some()), "formula written on the origin sheet");
    assert!(wb.sheet(1).unwrap().cell(at).is_none(), "the pointed-at sheet is untouched");
}

#[test]
fn ctrl_enter_after_pointing_on_another_sheet_fills_the_origin_selection() {
    let mut h = pointing_on_second_sheet("B2:C3");
    assert!(h.state_mut().commit_edit(0, 0, false, true));
    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.active_sheet, 0, "the origin sheet is shown again");
    for a1 in ["B2", "C2", "B3", "C3"] {
        let at = CellRef::parse(a1).unwrap();
        assert!(wb.sheet(0).unwrap().cell(at).is_some_and(|c| c.formula.is_some()), "{a1} filled on the origin sheet");
        assert!(wb.sheet(1).unwrap().cell(at).is_none(), "{a1} on the pointed-at sheet is untouched");
    }
    assert!(wb.sheet(1).unwrap().cell(CellRef::parse("A1").unwrap()).is_none(), "the pointed-at sheet's selection is untouched");
}
