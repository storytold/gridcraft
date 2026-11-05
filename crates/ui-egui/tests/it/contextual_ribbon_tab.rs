//! A contextual ribbon tab (Table Design, Chart Design) only exists while its object is selected.
//! The tab remembered in the saved UI state, or left behind when the selection moves away, must
//! not leave the ribbon blank: it falls back to Home.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::model::{ObjKind, ObjRef};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    crate::support::harness(session, 4)
}

fn session_with_table() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Qty"], ["a", 1], ["b", 2]]})).unwrap();
    session.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
    session.execute("selection.set", json!({"range": "A2"})).unwrap();
    session
}

/// Sets a saved ribbon tab on a fresh app and builds the stepped harness, for unavailable-tab cases.
fn app_restored_on(tab: &str, session: Session) -> Harness {
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = tab.into();
    crate::support::app_harness(app, 4)
}

#[test]
fn a_saved_table_design_tab_without_a_table_opens_on_home() {
    let mut session = Session::new();
    session.new_workbook();
    let h = app_restored_on("Table Design", session);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn a_saved_chart_design_tab_without_a_chart_opens_on_home() {
    let mut session = Session::new();
    session.new_workbook();
    let h = app_restored_on("Chart Design", session);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn shape_format_is_available_only_while_a_shape_is_selected() {
    let mut session = Session::new();
    session.new_workbook();
    let result = session.execute("insert.shape", json!({"kind": "rectangle", "at": "B2"}));
    assert!(result.is_ok(), "test shape should be created");
    let id = result.ok().and_then(|value| value["shape"].as_u64()).and_then(|id| u32::try_from(id).ok());
    assert!(id.is_some(), "shape creation returns its object id");
    let Some(id) = id else { return };

    let mut h = harness(session);
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Shape, id));
    h.state_mut().ui.ribbon_tab = "Shape Format".into();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Shape Format");
    assert!(h.query_by_label("Rotate selected shape 90 degrees").is_none());
    assert!(h.query_by_label("Bring object forward or to front").is_none());
    assert!(h.query_by_label("Send object backward or to back").is_none());
    h.get_by_label("Selection Pane").click();
    h.run_steps(2);
    assert!(h.query_by_label("Bring Forward").is_none());
    assert!(h.query_by_label("Send Backward").is_none());

    h.state_mut().selected_object = None;
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn slicer_tab_is_available_only_while_a_slicer_is_selected() {
    let mut session = session_with_table();
    let result = session.execute("insert.slicer", json!({"table": "Table1", "column": "Name"}));
    assert!(result.is_ok(), "test slicer should be created");
    let id = result.ok().and_then(|value| value["slicer"].as_u64()).and_then(|id| u32::try_from(id).ok());
    assert!(id.is_some(), "slicer creation returns its object id");
    let Some(id) = id else { return };

    let mut h = harness(session);
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Slicer, id));
    h.state_mut().ui.ribbon_tab = "Slicer".into();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Slicer");

    h.state_mut().selected_object = None;
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn picture_format_is_available_only_while_a_picture_is_selected() {
    let mut session = Session::new();
    session.new_workbook();
    let result = session.execute("insert.picture", json!({"base64": "AQID", "mime": "image/png", "at": "B2"}));
    assert!(result.is_ok(), "test picture should be created");
    let id = result.ok().and_then(|value| value["image"].as_u64()).and_then(|id| u32::try_from(id).ok());
    assert!(id.is_some(), "picture creation returns its object id");
    let Some(id) = id else { return };

    let mut h = harness(session);
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Image, id));
    h.state_mut().ui.ribbon_tab = "Picture Format".into();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Picture Format");

    h.state_mut().selected_object = None;
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

/// A contextual delete targets the selected kind/id, leaving another object of that kind intact.
#[test]
fn contextual_delete_uses_the_selected_object_identity() {
    for (kind, command, params, result_key, tab, label) in [
        (ObjKind::Shape, "insert.shape", json!({"kind": "rectangle", "at": "B2"}), "shape", "Shape Format", "Delete Shape"),
        (ObjKind::Image, "insert.picture", json!({"base64": "AQID", "mime": "image/png", "at": "B2"}), "image", "Picture Format", "Delete Picture"),
        (ObjKind::Slicer, "insert.slicer", json!({"table": "Table1", "column": "Name"}), "slicer", "Slicer", "Delete Slicer"),
    ] {
        let mut session = session_with_table();
        let first = session.execute(command, params.clone()).unwrap()[result_key].as_u64().unwrap() as u32;
        let second = session.execute(command, params).unwrap()[result_key].as_u64().unwrap() as u32;
        assert_ne!(first, second);
        let mut h = harness(session);
        h.state_mut().selected_object = Some(ObjRef::new(kind, second));
        h.state_mut().ui.ribbon_tab = tab.into();
        h.run_steps(2);
        h.get_by_label(label).click();
        h.run_steps(3);
        let sh = h.state().session.active().unwrap().wb.active().unwrap();
        assert!(sh.object_anchor(ObjRef::new(kind, first)).is_some(), "{tab} leaves the other object intact");
        assert!(sh.object_anchor(ObjRef::new(kind, second)).is_none(), "{tab} deletes the selected object");
        assert!(h.state().selected_object.is_none());
        assert_eq!(h.state().ui.ribbon_tab, "Home");
    }
}

/// An id from a different object kind or an object that was deleted cannot keep a tab active.
#[test]
fn contextual_tabs_reject_wrong_kind_and_deleted_objects() {
    let mut session = session_with_table();
    let id = session.execute("insert.shape", json!({"kind": "rectangle", "at": "B2"})).unwrap()["shape"].as_u64().unwrap() as u32;
    let mut h = harness(session);
    for (kind, tab) in [(ObjKind::Image, "Picture Format"), (ObjKind::Slicer, "Slicer"), (ObjKind::Chart, "Chart Design")] {
        h.state_mut().selected_object = Some(ObjRef::new(kind, id));
        h.state_mut().ui.ribbon_tab = tab.into();
        h.run_steps(2);
        assert_eq!(h.state().ui.ribbon_tab, "Home", "{tab} cannot use a shape's id");
    }
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Shape, id));
    h.state_mut().ui.ribbon_tab = "Shape Format".into();
    h.run_steps(2);
    h.state_mut().run("object.delete", json!({"kind": "shape", "id": id})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn an_equation_masquerading_as_a_shape_cannot_expose_shape_actions() {
    let mut session = session_with_table();
    let id = session.execute("insert.equation", json!({"latex":"x^2", "cell":"C4"})).unwrap()["equation"].as_u64().unwrap() as u32;
    let sh = session.active().unwrap().wb.active().unwrap();
    // This is the mismatch missed by anchor-only validation: equations share shape storage.
    assert!(sh.object_anchor(ObjRef::new(ObjKind::Shape, id)).is_some());
    assert_eq!(sh.object_ref(id), Some(ObjRef::new(ObjKind::Equation, id)));
    let mut h = harness(session);
    let journal = h.state().session.journal.len();
    for kind in [ObjKind::Shape, ObjKind::Equation] {
        h.state_mut().selected_object = Some(ObjRef::new(kind, id));
        h.state_mut().ui.ribbon_tab = "Shape Format".into();
        h.run_steps(3);
        assert_eq!(h.state().ui.ribbon_tab, "Home");
        for label in ["Shape Fill", "Shape Outline", "Delete Shape"] {
            assert!(h.query_by_label(label).is_none(), "{kind:?} must not expose {label}");
        }
        assert_eq!(h.state().session.journal.len(), journal);
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().object_ref(id), Some(ObjRef::new(ObjKind::Equation, id)));
    }
}

#[test]
fn portuguese_shape_palettes_translate_no_change_and_leave_the_shape_unchanged() {
    for label in ["Preenchimento da Forma", "Contorno da Forma"] {
        let mut session = session_with_table();
        let id = session.execute("insert.shape", json!({"kind":"rectangle", "at":"D4"})).unwrap()["shape"].as_u64().unwrap() as u32;
        let mut h = harness(session);
        h.state_mut().run("app.language.set", json!({"language":"pt"})).unwrap();
        h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Shape, id));
        h.state_mut().ui.ribbon_tab = "Shape Format".into();
        h.run_steps(3);
        let journal = h.state().session.journal.len();
        let undo = h.state().session.active().unwrap().undo.len();
        h.get_by_label(label).click();
        h.run_steps(3);
        assert!(h.query_by_label("No change").is_none());
        h.get_by_label("Sem alteração").click();
        h.run_steps(3);
        assert_eq!(h.state().session.journal.len(), journal, "{label}: no color command");
        assert_eq!(h.state().session.active().unwrap().undo.len(), undo, "{label}: no edit to undo");
    }
}

/// Newly supported form controls must not inherit another floating object's action tab.
#[test]
fn form_controls_cannot_expose_shape_picture_slicer_or_chart_actions() {
    let mut session = session_with_table();
    let id = session.execute("insert.button", json!({"at":"D4", "text":"Test control"})).unwrap()["control"].as_u64().unwrap() as u32;
    let mut h = harness(session);
    let journal = h.state().session.journal.len();
    for kind in [ObjKind::Control, ObjKind::Shape] {
        for tab in ["Shape Format", "Picture Format", "Slicer", "Chart Design"] {
            h.state_mut().selected_object = Some(ObjRef::new(kind, id));
            h.state_mut().ui.ribbon_tab = tab.into();
            h.run_steps(3);
            assert_eq!(h.state().ui.ribbon_tab, "Home", "{kind:?} must not expose {tab}");
            for label in ["Shape Fill", "Shape Outline", "Delete Shape", "Delete Picture", "Delete Slicer"] {
                assert!(h.query_by_label(label).is_none(), "form controls must not expose {label}");
            }
            assert_eq!(h.state().session.journal.len(), journal);
            assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().object_ref(id), Some(ObjRef::new(ObjKind::Control, id)));
        }
    }
}

#[test]
fn leaving_the_table_while_on_table_design_returns_to_home() {
    let mut h = harness(session_with_table());
    h.state_mut().ui.ribbon_tab = "Table Design".into();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Table Design", "the tab stays while the cell is in the table");
    h.state_mut().session.execute("selection.set", json!({"range": "D10"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn startup_opens_on_home_not_the_saved_tab() {
    let saved = gridcraft_ui_egui::UiState { ribbon_tab: "Table Design".into(), dark: true, ..Default::default() };
    let json = serde_json::to_string(&saved).unwrap();
    assert!(json.contains(r#""ribbonTab":"Table Design""#), "{json}");
    let restored = serde_json::from_str::<gridcraft_ui_egui::UiState>(&json).unwrap().at_startup();
    assert_eq!(restored.ribbon_tab, "Home", "the saved tab is not restored");
    assert!(restored.dark, "the other preferences are");
}

#[test]
fn an_ordinary_tab_stays_put_in_a_session() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = harness(session);
    h.state_mut().ui.ribbon_tab = "Data".into();
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, "Data");
}

#[test]
fn a_second_window_does_not_reset_the_first_windows_table_design_tab() {
    use egui::{Event, PointerButton, Shape};

    let mut h = harness(session_with_table());
    let first = h.state().session.active_window().unwrap();
    // A second window on the same workbook, with its active cell outside the table.
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    let second = h.state().session.active_window().unwrap();
    h.state_mut().run("selection.set", json!({"cell": "D10"})).unwrap();
    h.state_mut().run("window.activate", json!({"window": first})).unwrap();
    h.run_steps(2);
    let tab_center = h
        .output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == "Table Design" => Some(t.pos + t.galley.size() / 2.0),
            _ => None,
        })
        .expect("the Table Design tab is painted while a cell is in the table");
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton {
            pos: tab_center,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Table Design");
    // DocWindows draws every window each frame. The second window's cell is outside the table,
    // so its frame falls back to Home; the first window's Table Design tab must survive it.
    h.state_mut().show_window(second);
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home", "the second window shows Home for its own selection");
    h.state_mut().enter_window(first);
    h.run_steps(2);
    assert_eq!(h.state().session.active_window(), Some(first));
    assert_eq!(h.state().ui.ribbon_tab, "Table Design", "the first window's tab survives the second window's frame");
    let d = h.state().session.active().unwrap();
    assert_eq!(d.selection.active.a1(), "A2", "the first window still selects a cell in the table");
    assert!(d.wb.active().unwrap().table_at(d.selection.active).is_some(), "the table is still there");
}
