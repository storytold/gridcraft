//! Cells show numbers and dates in the display region (`Session::display_locale`): its
//! separators for a format's `.` and `,`, its short date for the built-in date format. The
//! values, codes and everything outside the grid stay en-US.

use egui::Shape;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::numfmt::Locale;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Build an offscreen UI with fixed synthetic values and an optional display locale.
fn harness(tag: Option<&str>) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1:A3", "values": [[1169.5], [45292], [0.25]]})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"num_fmt": "#,##0.00"}})).unwrap();
    s.execute("home.formatCells", json!({"range": "A2", "style": {"num_fmt": "m/d/yyyy"}})).unwrap();
    // Wide enough for every form, so none turns into ####.
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 20})).unwrap();
    s.execute("selection.set", json!({"range": "H20"})).unwrap();
    let mut app = SheetApp::new(s, Default::default());
    if let Some(tag) = tag {
        app.session.display_locale = Locale::from_tag(tag);
    }
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

/// Whether the rendered egui output contains the expected cell text.
fn painted(h: &Harness, text: &str) -> bool {
    h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if t.galley.job.text == text))
}

/// Swiss display uses apostrophe grouping and dotted dates without changing the stored code.
#[test]
fn swiss_region_shows_apostrophe_groups_and_dotted_dates() {
    let h = harness(Some("en_CH"));
    assert!(painted(&h, "1'169.50"));
    assert!(painted(&h, "01.01.2024"));
    assert!(painted(&h, "0.25"));
    // The stored code and value are unchanged.
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(d.wb.styles.get(sh.style_id(gridcraft_engine::core::CellRef::new(1, 0))).num_fmt.as_str(), "m/d/yyyy");
}

/// German grid rendering uses decimal commas and dotted short dates.
#[test]
fn german_region_shows_decimal_commas() {
    let h = harness(Some("de-DE"));
    assert!(painted(&h, "1.169,50"));
    assert!(painted(&h, "01.01.2024"));
    assert!(painted(&h, "0,25"));
}

/// An unconfigured session paints numbers and short dates with the en-US fallback.
#[test]
fn default_region_is_en_us() {
    let h = harness(None);
    assert!(painted(&h, "1,169.50"));
    assert!(painted(&h, "1/1/2024"));
}

/// Romanian grid rendering uses dotted groups, decimal commas and dotted short dates.
#[test]
fn romanian_region_shows_grouped_numbers_and_dates() {
    let h = harness(Some("ro-RO"));
    assert!(painted(&h, "1.169,50"));
    assert!(painted(&h, "01.01.2024"));
    assert!(painted(&h, "0,25"));
}

/// Switching interface languages retains the independent Romanian format region and cell text.
#[test]
fn interface_language_changes_keep_romanian_number_and_date_display() {
    let mut h = harness(Some("ro-RO"));
    for language in [gridcraft_ui_egui::i18n::Language::En, gridcraft_ui_egui::i18n::Language::De, gridcraft_ui_egui::i18n::Language::Ro] {
        h.state_mut().ui.language = language;
        h.run_steps(4);
        assert!(painted(&h, "1.169,50"));
        assert!(painted(&h, "01.01.2024"));
        assert!(painted(&h, "0,25"));
        assert_eq!(h.state().session.display_locale, Locale::from_tag("ro-RO"));
    }
}

/// A system short-date override affects the built-in family while an imported custom code
/// keeps its literal ordering in the same grid.
#[test]
fn system_custom_short_date_and_literal_custom_code_paint_differently() {
    let mut h = harness(Some("en_US@rg=sgzzzz"));
    h.state_mut().session.display_locale = Locale::from_tag("en_US@rg=sgzzzz").with_icu_short_date("dd.MM.y");
    let doc = h.state_mut().session.active_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut doc.wb);
    let style = wb.styles.derive(Default::default(), |st| st.num_fmt = gridcraft_engine::model::NumFmt::custom("m/d/yyyy"));
    let sh = wb.sheet_mut(0).unwrap();
    sh.cells.set(
        gridcraft_engine::core::CellRef::new(3, 0),
        gridcraft_engine::model::Cell { value: gridcraft_engine::core::Value::Number(45292.0), formula: None, style },
    );
    h.run_steps(4);
    assert!(painted(&h, "01.01.2024"));
    assert!(painted(&h, "1/1/2024"));
}

/// Open Format Cells on a synthetic imported custom short date in a Romanian session.
fn custom_date_dialog() -> Harness {
    let mut h = harness(Some("ro-RO"));
    h.state_mut().session.execute("selection.set", json!({"cell":"A2"})).unwrap();
    let wb = std::sync::Arc::make_mut(&mut h.state_mut().session.active_mut().unwrap().wb);
    let id = wb.styles.derive(Default::default(), |st| st.num_fmt = gridcraft_engine::model::NumFmt::custom("m/d/yyyy"));
    wb.sheet_mut(0).unwrap().set_style(gridcraft_engine::core::CellRef::new(1, 0), id);
    h.run_steps(2);
    h.state_mut().open_dialog("formatCells", json!({}));
    h.run_steps(3);
    h
}

/// Confirm the dialog and verify format identity, font state and the stored date
/// before and after an in-memory XLSX round trip.
fn confirm_custom_date(h: &mut Harness, literal: bool, bold: bool) {
    h.get_by_label_contains("OK").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let wb = &h.state().session.active().unwrap().wb;
    let at = gridcraft_engine::core::CellRef::new(1, 0);
    let st = wb.styles.get(wb.sheet(0).unwrap().style_id(at));
    assert_eq!(st.num_fmt.is_literal_short_date(), literal);
    assert_eq!(st.font.bold, bold);
    let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
    let (back, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.value(at), gridcraft_engine::core::Value::Number(45292.0));
    assert_eq!(back.styles.get(sh.style_id(at)).num_fmt, st.num_fmt);
}

/// Confirming an unchanged Format Cells dialog retains the imported literal date.
#[test]
fn opening_and_confirming_format_cells_keeps_custom_date_identity() {
    let mut h = custom_date_dialog();
    confirm_custom_date(&mut h, true, false);
    assert!(painted(&h, "1/1/2024"));
}

/// Changing only the font retains the imported custom number-format identity.
#[test]
fn changing_font_in_format_cells_keeps_custom_date_identity() {
    let mut h = custom_date_dialog();
    h.get_by_label("Font").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Bold").click();
    h.run_steps(2);
    confirm_custom_date(&mut h, true, true);
}

/// Preset, shortcut and code-field edits intentionally replace the imported custom identity.
#[test]
fn intentional_number_format_edits_can_replace_custom_date_identity() {
    for edit in ["preset", "button", "type"] {
        let mut h = custom_date_dialog();
        match edit {
            "preset" => h.get_by_label("Short Date").click(),
            "button" => h.get_by_label("m/d/yyyy").click(),
            _ => {
                h.get_by_label("Type (custom format code):").click();
                h.run_steps(2);
                h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
                h.run_steps(1);
                h.input_mut().events.push(egui::Event::Text("dd.mm.yyyy".into()));
            }
        }
        h.run_steps(3);
        confirm_custom_date(&mut h, false, false);
        assert!(painted(&h, "01.01.2024"));
    }
}
