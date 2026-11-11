//! Cells show numbers and dates in the display region (`Session::display_locale`): its
//! separators for a format's `.` and `,`, its short date for the built-in date format. The
//! values and codes stay invariant; interactive scalar entry follows the same numeric region.

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
        gridcraft_engine::model::Cell::styled(gridcraft_engine::core::Value::Number(45292.0), style),
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

/// Enter and F2 share regional text without changing an untouched cell or its undo history.
#[test]
fn romanian_enter_and_unchanged_f2_keep_the_same_number() {
    let mut h = harness(Some("ro-RO"));
    let app = h.state_mut();
    app.session.execute("selection.set", json!({"range":"A4"})).unwrap();
    app.begin_edit(Some("1,234".into()), false);
    assert!(app.commit_edit(0, 0, false, false));
    app.begin_edit(None, false);
    assert_eq!(app.editor.as_ref().unwrap().text, "1,234");
    assert!(app.commit_edit(0, 0, false, false));
    app.session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(
        app.session.active().unwrap().wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(3, 0)),
        gridcraft_engine::core::Value::Empty
    );
}

/// Validation sees the canonical number; a region switch while a draft is open cannot reinterpret it.
#[test]
fn romanian_drafts_capture_the_region_and_validate_the_stored_number() {
    let mut h = harness(Some("ro-RO"));
    let app = h.state_mut();
    app.session.execute("data.validation", json!({"range":"A4","type":"decimal","operator":"between","formula1":"0","formula2":"2"})).unwrap();
    app.session.execute("selection.set", json!({"range":"A4"})).unwrap();
    app.begin_edit(Some("1,234".into()), false);
    app.session.display_locale = Locale::EN_US;
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(
        app.session.active().unwrap().wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(3, 0)),
        gridcraft_engine::core::Value::Number(1.234)
    );
    app.session.display_locale = Locale::from_tag("ro-RO");
    app.begin_edit(Some("3,5".into()), false);
    assert!(!app.commit_edit(0, 0, false, false));
    assert_eq!(app.editor.as_ref().unwrap().text, "3,5");
}

/// The actual UI clipboard path is regional; direct programmatic app commands stay invariant.
#[test]
fn romanian_external_text_paste_and_programmatic_calls_are_distinct() {
    let mut h = harness(Some("ro-RO"));
    let app = h.state_mut();
    app.session.execute("selection.set", json!({"range":"D1"})).unwrap();
    app.paste_external("1,234\t04.03.2026\n1.234,56\t12,5%".into());
    let sh = app.session.active().unwrap().wb.sheet(0).unwrap();
    assert_eq!(sh.value(gridcraft_engine::core::CellRef::new(0, 3)), gridcraft_engine::core::Value::Number(1.234));
    assert_eq!(sh.value(gridcraft_engine::core::CellRef::new(0, 4)), gridcraft_engine::core::Value::Number(46085.0));
    app.run("cell.set", json!({"cell":"A4","input":"1,234"})).unwrap();
    assert_eq!(
        app.session.active().unwrap().wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(3, 0)),
        gridcraft_engine::core::Value::Number(1234.0)
    );
    h.run_steps(3);
}

/// Both Python entry paths retain the numeric region when the draft becomes ordinary input.
#[test]
fn python_drafts_keep_their_numeric_region_after_leaving_python() {
    for existing_python_cell in [false, true] {
        let mut h = harness(Some("ro-RO"));
        let app = h.state_mut();
        app.session.execute("selection.set", json!({"range":"A4"})).unwrap();
        if existing_python_cell {
            app.session.execute("python.set", json!({"cell":"A4","code":"1 + 1"})).unwrap();
            app.begin_edit(None, false);
        } else {
            app.begin_python_edit();
        }
        assert!(app.editor.as_ref().unwrap().python);
        // Changing the displayed region cannot reinterpret a draft already in progress.
        app.session.display_locale = Locale::EN_US;
        app.toggle_python_mode();
        app.editor.as_mut().unwrap().text = "1,234".into();
        assert!(app.commit_edit(0, 0, false, false));
        let wb = &app.session.active().unwrap().wb;
        let cell = gridcraft_engine::core::CellRef::new(3, 0);
        assert_eq!(wb.sheet(0).unwrap().value(cell), gridcraft_engine::core::Value::Number(1.234));
        let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
        let (back, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        assert_eq!(back.sheet(0).unwrap().value(cell), gridcraft_engine::core::Value::Number(1.234));
    }
}

/// Actual host copy/cut uses regional text even when the host provides no rich clipboard.
#[test]
fn regional_host_copy_and_cut_preserve_decimals_between_sessions() {
    for event in [egui::Event::Copy, egui::Event::Cut] {
        let mut source = harness(Some("ro-RO"));
        source.state_mut().run("cell.set", json!({"cell":"D1","input":"1.234"})).unwrap();
        source.state_mut().run("selection.set", json!({"range":"D1"})).unwrap();
        source.input_mut().events.push(event);
        source.step();
        let text = source
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(text, "1,234\n");
        let mut dest = harness(Some("ro-RO"));
        dest.state_mut().run("selection.set", json!({"range":"D1"})).unwrap();
        dest.state_mut().paste_external(text);
        assert_eq!(
            dest.state().session.active().unwrap().wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 3)),
            gridcraft_engine::core::Value::Number(1.234)
        );
    }
}

/// Inspect only the painted Sample area: the correctly formatted grid must not satisfy this check.
fn dialog_number_sample(h: &Harness) -> String {
    let text_position = |label: &str| {
        h.output()
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                Shape::Text(t) if t.galley.job.text == label => Some(t.pos),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing painted label: {label}"))
    };
    let heading = text_position("Sample");
    let following = text_position("Type (custom format code):");
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if t.pos.x >= heading.x - 1.0 && t.pos.y > heading.y + 1.0 && t.pos.y < following.y - 1.0 => {
                Some(t.galley.job.text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn format_cells_number_samples_follow_the_display_region() {
    for (tag, number, date) in [("ro-RO", "1.169,50", "01.01.2024"), ("tr-TR", "1.169,50", "1.01.2024"), ("en-US", "1,169.50", "1/1/2024")] {
        for (cell, expected) in [("A1", number), ("A2", date)] {
            let mut h = harness(Some(tag));
            h.state_mut().session.execute("selection.set", json!({"cell":cell})).unwrap();
            h.run_steps(2);
            h.state_mut().open_dialog("formatCells", json!({}));
            h.run_steps(3);
            assert_eq!(dialog_number_sample(&h), expected, "{tag}, {cell}");
        }
    }
}

#[test]
fn format_cells_sample_keeps_literal_identity_until_a_preset_is_chosen() {
    let mut h = custom_date_dialog();
    assert_eq!(dialog_number_sample(&h), "1/1/2024");
    h.get_by_label("Short Date").click();
    h.run_steps(3);
    assert_eq!(dialog_number_sample(&h), "01.01.2024");
}
