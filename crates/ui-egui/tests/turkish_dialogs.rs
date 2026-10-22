//! Exercise rendered controls, not only the translation lookup tables: a translated catalog
//! must actually reach the dialogs, while field identifiers and workbook data stay canonical.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, i18n::Language};
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = Language::Tr;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1440.0, 1000.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    h
}

#[test]
fn turkish_replace_controls_edit_text_without_translating_workbook_data() {
    let mut h = harness();
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "İstanbul ığüşöç"})).unwrap();
    h.state_mut().open_dialog("find", json!({}));
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.tab = "Replace".into();
    d.values.insert("what".into(), json!("İstanbul"));
    d.values.insert("with".into(), json!("İzmir"));
    h.run_steps(6);
    assert!(h.query_by_label("Büyük/küçük harf eşleştir").is_some());
    assert!(h.query_by_label("Çalışma kitabında").is_some());
    h.get_by_label("Tümünü Değiştir").click();
    h.run_steps(6);
    let doc = h.state().session.active().unwrap();
    assert_eq!(doc.wb.active().unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).display(), "İzmir ığüşöç");
    assert_eq!(h.state().toast.as_ref().unwrap().0, "İşlem tamamlandı. 1 değişiklik yapıldı.");
    assert_eq!(h.state().dialog.as_ref().unwrap().tab, "Replace");
}

#[test]
fn turkish_save_prompt_keeps_the_document_title_and_cancel_preserves_edits() {
    let mut h = harness();
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "Kaydedilmemiş çalışma"})).unwrap();
    h.state_mut().open_dialog("saveChanges", json!({"title": "İstanbul {count}.xlsx"}));
    h.run_steps(6);
    assert!(h.query_by_label("İstanbul {count}.xlsx üzerinde yaptığınız değişiklikleri kaydetmek istiyor musunuz?").is_some());
    assert!(h.query_by_label("Kaydetme").is_some());
    h.get_by_label("İptal").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert!(h.state().session.active().unwrap().is_dirty());
}

#[test]
fn turkish_format_cells_dialog_translates_tabs_without_changing_internal_tab_ids() {
    let mut h = harness();
    h.state_mut().open_dialog("formatCells", json!({}));
    h.run_steps(6);
    h.get_by_label("Dolgu").click();
    h.run_steps(6);
    assert_eq!(h.state().dialog.as_ref().unwrap().tab, "Fill");
    assert!(h.query_by_label("Arka Plan Rengi:").is_some());
    assert!(h.query_by_label("   Tamam   ").is_some());
}

#[test]
fn command_search_accepts_turkish_labels_and_canonical_command_ids() {
    let mut h = harness();
    h.state_mut().open_dialog("commandSearch", json!({}));
    for query in ["kaydet", "file.save", "FILE.SAVE", "save"] {
        h.state_mut().dialog.as_mut().unwrap().search = query.into();
        h.run_steps(6);
        assert!(h.query_by_label("Kaydet    Dosya").is_some(), "missing Save result for {query}");
    }
}
