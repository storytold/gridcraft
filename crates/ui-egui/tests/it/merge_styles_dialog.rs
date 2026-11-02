//! Home › Cell Styles: the gallery's Custom styles, New Cell Style… and Merge Styles…, with two
//! open workbooks.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::model::Underline;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Book1 has the styles Src Only (bold) and Shared (italic); Book2, active, has Shared (double
/// underline) when `clash`, else no styles.
fn two_books(clash: bool) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    for (name, cell, cmd) in [("Src Only", "A1", "home.bold"), ("Shared", "A2", "home.italic")] {
        s.execute(cmd, json!({"range": cell})).unwrap();
        s.execute("home.newCellStyle", json!({"name": name, "fromCell": cell})).unwrap();
    }
    s.new_workbook();
    if clash {
        s.execute("home.doubleUnderline", json!({"range": "A2"})).unwrap();
        s.execute("home.newCellStyle", json!({"name": "Shared", "fromCell": "A2"})).unwrap();
    }
    crate::support::harness(s, 6)
}

fn open_gallery(h: &mut Harness) {
    h.get_by_role_and_label(Role::Button, "Cell Styles").click();
    h.run_steps(3);
}

fn styles(h: &Harness) -> Vec<(String, gridcraft_engine::model::Style)> {
    h.state().session.active().unwrap().wb.cell_styles.clone()
}

fn underline_of(h: &Harness, name: &str) -> Underline {
    styles(h).into_iter().find(|(n, _)| n == name).unwrap().1.font.underline
}

#[test]
fn merge_styles_lists_the_other_workbook_and_ok_merges_its_styles() {
    let mut h = two_books(false);
    open_gallery(&mut h);
    assert!(h.query_by_label("Custom").is_none(), "no custom styles yet");
    h.get_by_label("New Cell Style…");
    h.get_by_label("Merge Styles…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("mergeStyles"));
    h.get_by_label("Merge styles from:");
    // The other open workbook, chosen; the active one isn't offered.
    h.get_by_role_and_label(Role::Button, "Book1");
    assert!(h.query_by_role_and_label(Role::Button, "Book2").is_none());
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "OK closes the dialog");
    let names: Vec<String> = styles(&h).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Src Only", "Shared"]);
    // The command names the workbook, so a recorded macro can run it again.
    let merges: Vec<_> = h.state().session.journal.iter().filter(|(id, _)| id == "home.mergeStyles").map(|(_, p)| p["from"].clone()).collect();
    assert_eq!(merges, [json!("Book1")]);
    // The gallery shows them under Custom, and a click applies one.
    open_gallery(&mut h);
    h.get_by_label("Custom");
    h.get_by_label("Shared");
    h.get_by_label("Src Only").click();
    h.run_steps(3);
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    assert!(d.wb.styles.get(sh.style_id(d.selection.active)).font.bold);
}

#[test]
fn styles_with_the_same_names_ask_whether_to_replace_them() {
    let mut h = two_books(true);
    let ask = |h: &mut Harness| {
        h.state_mut().open_dialog("mergeStyles", json!({}));
        h.run_steps(3);
        h.get_by_label("   OK   ").click();
        h.run_steps(3);
        h.get_by_label_contains("Book1 has styles with the same names as this workbook");
        h.get_by_label("Shared");
    };
    // Cancel goes back to the list without merging.
    ask(&mut h);
    h.get_by_role_and_label(Role::Button, " Cancel ").click();
    h.run_steps(3);
    h.get_by_label("Merge styles from:");
    assert_eq!(styles(&h).len(), 1);
    h.state_mut().dialog = None;
    h.run_steps(2);
    // No keeps this workbook's Shared and adds the rest.
    ask(&mut h);
    h.get_by_label(" No ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(styles(&h).len(), 2);
    assert_eq!(underline_of(&h, "Shared"), Underline::Double);
    // Yes replaces it.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    ask(&mut h);
    h.get_by_label(" Yes ").click();
    h.run_steps(3);
    assert_eq!(underline_of(&h, "Shared"), Underline::None);
    assert!(styles(&h).iter().any(|(n, st)| n == "Shared" && st.font.italic));
}

#[test]
fn with_no_other_workbook_open_the_dialog_says_so() {
    let mut h = crate::support::blank_harness(6);
    h.state_mut().open_dialog("mergeStyles", json!({}));
    h.run_steps(3);
    h.get_by_label_contains("No other workbook is open");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.is_none(), "nothing ran, nothing failed");
    assert!(h.state().session.journal.iter().all(|(id, _)| id != "home.mergeStyles"));
}

#[test]
fn new_cell_style_names_a_style_after_the_active_cell() {
    let mut h = crate::support::blank_harness(6);
    h.state_mut().run("home.bold", json!({})).unwrap();
    open_gallery(&mut h);
    h.get_by_label("New Cell Style…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.values.get("name").cloned()), Some(Some(json!("Style 1"))));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    let made = styles(&h);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].0, "Style 1");
    assert!(made[0].1.font.bold);
    // The next one starts on the next free name.
    h.state_mut().open_dialog("newCellStyle", json!({}));
    assert_eq!(h.state().dialog.as_ref().map(|d| d.values.get("name").cloned()), Some(Some(json!("Style 2"))));
}

#[test]
fn the_german_interface_names_the_items_and_asks_in_german() {
    let mut h = two_books(true);
    h.state_mut().ui.language = Language::De;
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, &Language::De.tr("Cell\nStyles").replace('\n', " ")).click();
    h.run_steps(3);
    assert!(h.query_all_by_label("Benutzerdefiniert").next().is_some(), "the Custom group");
    h.get_by_label("Neue Zellenformatvorlage…");
    h.get_by_label("Formatvorlagen zusammenführen…").click();
    h.run_steps(3);
    h.get_by_label("Formatvorlagen zusammenführen aus:");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    h.get_by_label_contains("Book1 enthält Formatvorlagen mit denselben Namen wie diese Arbeitsmappe");
    h.get_by_label(" Ja ");
    // Cancel is in the language's catalog once open i18n changes add it there.
    h.get_by_label(&format!(" {} ", Language::De.tr("Cancel")));
    h.get_by_label(" Nein ").click();
    h.run_steps(3);
    assert_eq!(underline_of(&h, "Shared"), Underline::Double, "Nein keeps this workbook's Shared");
    assert_eq!(styles(&h).len(), 2);
}
