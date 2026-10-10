//! UI interaction tests: real egui input through the whole app (no GPU needed).

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::l10n::Tr;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    harness_with(session, Default::default())
}

fn harness_with(session: Session, services: gridcraft_ui_egui::Services) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

#[test]
fn vertical_scrollbar_pages_and_drags_without_changing_selection() {
    let mut h = harness(blank());
    let grid = h.state().grid.rect.unwrap();
    let cells = h.state().grid.cells_rect.unwrap();
    let p = egui::pos2(grid.right() + 7.0, cells.bottom() - 20.0);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(h.state().view().scroll.y > 0.0, "clicking the scrollbar track scrolls down");
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));

    h.state_mut().view_mut().unwrap().scroll.y = 0.0;
    h.run_steps(2);
    let start = egui::pos2(grid.right() + 7.0, cells.top() + 8.0);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.input_mut().events.push(Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let end = start + egui::vec2(0.0, 80.0);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.run_steps(2);
    let dragged = h.state().view().scroll.y;
    assert!(dragged > 0.0, "dragging the thumb scrolls down");
    h.run_steps(3);
    assert_eq!(h.state().view().scroll.y, dragged, "holding the thumb still must not keep scrolling");
    h.input_mut().events.push(Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));
}

fn key(h: &mut egui_kittest::Harness<'static, SheetApp>, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn text(h: &mut egui_kittest::Harness<'static, SheetApp>, t: &str) {
    h.input_mut().events.push(Event::Text(t.into()));
    h.run_steps(2);
}

fn value(h: &egui_kittest::Harness<'static, SheetApp>, a: &str) -> Value {
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap_or_default()))).unwrap_or_default()
}

fn click_cell(h: &mut egui_kittest::Harness<'static, SheetApp>, cell: &str) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

#[test]
fn keyboard_navigation_does_not_replay_the_last_pointer_click() {
    let mut h = harness(blank());
    click_cell(&mut h, "B2");
    for (key_code, modifiers, expected) in [
        (Key::Enter, Modifiers::NONE, "B3"),
        (Key::Enter, Modifiers::NONE, "B4"),
        (Key::Enter, Modifiers::NONE, "B5"),
        (Key::Enter, Modifiers::SHIFT, "B4"),
        (Key::Tab, Modifiers::NONE, "C4"),
        (Key::Enter, Modifiers::NONE, "C5"),
    ] {
        key(&mut h, key_code, modifiers);
        let selection = &h.state().session.active().unwrap().selection;
        assert_eq!(selection.active.a1(), expected);
        assert_eq!(selection.anchor, selection.active);
        assert!(h.state().editor.is_none(), "navigation must not start editing at the old pointer location");
    }
    // A subsequent real pointer click must still select the clicked cell.
    click_cell(&mut h, "D6");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "D6");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "D7");
}

#[test]
fn space_toggles_the_active_checkbox_without_clicking_the_hovered_cell() {
    let mut session = blank();
    session.execute("insert.checkbox", json!({"range": "A1:B1"})).unwrap();
    let mut h = harness(session);
    click_cell(&mut h, "A1");
    assert_eq!(value(&h, "A1"), Value::Bool(true));
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "B1");
    assert_eq!(value(&h, "A1"), Value::Bool(true), "the old pointer location must remain unchanged");
    assert_eq!(value(&h, "B1"), Value::Bool(true));
    assert!(h.state().editor.is_none());
}

#[test]
fn typing_enters_values_and_moves_down() {
    let mut h = harness(blank());
    text(&mut h, "42");
    assert!(h.state().editor.is_some(), "typing starts editing");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Number(42.0));
    text(&mut h, "=A1*2");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(value(&h, "A2"), Value::Number(84.0));
    let active = h.state().session.active().map(|d| d.selection.active.a1());
    assert_eq!(active.as_deref(), Some("B2"));
}

#[test]
fn arrows_escape_and_undo() {
    let mut h = harness(blank());
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(h.state().session.active().map(|d| d.selection.active.a1()).as_deref(), Some("B2"));
    text(&mut h, "hello");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), Value::Empty);
    text(&mut h, "x");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), Value::from("x"));
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(value(&h, "B2"), Value::Empty);
}

#[test]
fn shortcuts_format_and_extend() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    let mut h = harness(s);
    key(&mut h, Key::ArrowRight, Modifiers::SHIFT);
    key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(h.state().session.active().map(|d| d.selection.a1()).as_deref(), Some("A1:B2"));
    key(&mut h, Key::B, Modifiers::COMMAND);
    let bold = h.state_mut().session.run("cell.get", json!({"cell": "B2"})).unwrap()["style"]["font"]["bold"].clone();
    assert_eq!(bold, json!(true));
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Empty);
}

#[test]
fn renders_every_ribbon_tab_without_panicking() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    let mut h = harness(s);
    for tab in ["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"] {
        h.state_mut().ui.ribbon_tab = tab.into();
        h.run_steps(2);
    }
    h.state_mut().ui.dark = true;
    h.run_steps(3);
}

#[test]
fn language_switch_translates_the_ribbon_and_persists() {
    let mut h = harness(blank());
    // English by default whatever the host's locale: only the desktop app reads the system language.
    assert_eq!(h.state().session.locale().ui.tag, "en-US");
    // The language commands are listed with the other UI commands.
    let ctx = h.ctx.clone();
    let gridcraft_ui_egui::control::Outcome::Done(listed) = gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, "engine.commands", &json!({}))
    else {
        panic!("engine.commands returns a response");
    };
    let ids: Vec<&str> = listed["result"].as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()).collect();
    for id in ["app.language.set", "app.language.english", "app.language.japanese"] {
        assert!(ids.contains(&id), "engine.commands lists {id}");
    }
    for (code, tag, expected) in
        [("zh", "zh-CN", "数据"), ("ja", "ja-JP", "データ"), ("ko", "ko-KR", "데이터"), ("ru", "ru-RU", "Данные"), ("pt", "pt-BR", "Dados")]
    {
        h.state_mut().run("app.language.set", json!({"language": code})).unwrap();
        h.run_steps(2);
        assert_eq!(h.state().session.locale().ui.tag, tag);
        assert_eq!(h.state().l10n.ribbon_tab("Data").unwrap(), expected);
        let restored: gridcraft_engine::Prefs = serde_json::from_str(&serde_json::to_string(&h.state().session.prefs).unwrap()).unwrap();
        assert_eq!(restored.ui_language, tag);
    }
    h.state_mut().run("app.language.set", json!({"language": "en"})).unwrap();
    assert_eq!(h.state().session.locale().ui.tag, "en-US");
}

#[test]
fn legacy_ui_language_migrates_once_without_resetting_other_preferences() {
    let mut app = SheetApp::new(blank(), Default::default());
    app.session.set_system_locale("de_DE.UTF-8");
    for (code, tag) in [("pt", "pt-BR"), ("zh", "zh-CN"), ("ja", "ja-JP"), ("ko", "ko-KR"), ("ru", "ru-RU")] {
        app.session.prefs.ui_language = "system".into();
        let saved = json!({"language": code, "dark": true});
        app.ui = serde_json::from_value(saved.clone()).unwrap();
        assert!(app.migrate_ui_language(&saved));
        assert_eq!(app.session.prefs.ui_language, tag);
        assert!(app.ui.dark);
        assert!(!app.migrate_ui_language(&json!({"language": "ja"})));
        assert!(serde_json::to_value(&app.ui).unwrap().get("language").is_none());
    }
    app.session.prefs.ui_language = "system".into();
    for saved in [json!({"language": "xx", "dark": true}), json!({"language": 7, "dark": true}), json!({"dark": true})] {
        app.ui = serde_json::from_value(saved.clone()).unwrap();
        assert!(!app.migrate_ui_language(&saved));
        assert_eq!(app.session.prefs.ui_language, "system");
        assert!(app.ui.dark);
    }
}

#[test]
fn a_saved_language_that_old_builds_detected_keeps_following_the_system() {
    // Old builds saved their own system detection, English for languages they lacked: on a German
    // system "en" was never a choice, and German is available now.
    let mut app = SheetApp::new(blank(), Default::default());
    for (system, saved, followed) in [("de_DE.UTF-8", "en", "de-DE"), ("ja-JP", "ja", "ja-JP"), ("pt-PT", "pt", "pt-PT")] {
        app.session.prefs.ui_language = "system".into();
        app.session.set_system_locale(system);
        assert!(!app.migrate_ui_language(&json!({"language": saved})));
        assert_eq!(app.session.prefs.ui_language, "system");
        assert_eq!(app.session.locale().ui.tag, followed);
    }
    // A language other than the detected one was picked by the user: it migrates.
    app.session.set_system_locale("pt-PT");
    assert!(app.migrate_ui_language(&json!({"language": "en"})));
    assert_eq!(app.session.prefs.ui_language, "en-US");
}

#[test]
fn language_commands_negotiate_tags_and_only_set_interface_preference() {
    let mut app = SheetApp::new(blank(), Default::default());
    app.run("app.setLocale", json!({"formulaLanguage": "en-US", "regionalFormat": "de-DE"})).unwrap();
    app.run("app.language.set", json!({"code": "pt-PT"})).unwrap();
    assert_eq!(app.session.prefs.ui_language, "pt-PT");
    assert_eq!(app.session.prefs.formula_language, "en-US");
    assert_eq!(app.session.prefs.regional_format, "de-DE");
    app.run("app.language.set", json!({"language": "de-AT"})).unwrap();
    assert_eq!(app.session.prefs.ui_language, "de-DE");
    assert!(app.run("app.language.set", json!({})).is_err());
    let error = app.run("app.language.set", json!({"language": "xx"})).unwrap_err();
    for language in gridcraft_locale::LANGUAGES {
        assert!(error.contains(language.tag));
    }
    app.run("app.language.japanese", json!({})).unwrap();
    assert_eq!(app.session.prefs.ui_language, "ja-JP");
    app.run("app.language.english", json!({})).unwrap();
    assert_eq!(app.session.prefs.ui_language, "en-US");
}

#[test]
fn dialogs_open_and_close() {
    let mut h = harness(blank());
    for name in [
        "formatCells",
        "insertFunction",
        "find",
        "sort",
        "nameManager",
        "dataValidation",
        "pasteSpecial",
        "commandSearch",
        "spelling",
        "goTo",
        "about",
        "options",
    ] {
        h.state_mut().open_dialog(name, json!({}));
        h.run_steps(2);
        key(&mut h, Key::Escape, Modifiers::NONE);
        h.state_mut().dialog = None;
        h.state_mut().message = None;
    }
}

#[test]
fn about_renders_every_tab() {
    let mut h = harness(blank());
    for tab in ["About", "Contributors", "Models"] {
        h.state_mut().open_dialog("about", json!({"tab": tab}));
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.tab.clone()).as_deref(), Some(tab));
        h.state_mut().dialog = None;
    }
}

#[test]
fn column_autocomplete_completes_on_enter() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["North"], ["East"], ["Eastern"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A4"})).unwrap();
    let mut h = harness(s);
    text(&mut h, "No");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A4"), Value::from("North"));
    text(&mut h, "Ea"); // ambiguous: East / Eastern
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A5"), Value::from("Ea"));
}

#[test]
fn typing_follows_the_regional_format() {
    let mut h = harness(blank());
    h.state_mut().session.execute("app.setLocale", json!({"regionalFormat": "pt-BR"})).unwrap();
    h.run_steps(2);
    text(&mut h, "1,5");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Number(1.5));
    text(&mut h, "=SUM(1,5;2)");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A2"), Value::Number(3.5));
    // The formula is stored canonically and shown in the region's spelling.
    let cell = h.state_mut().session.run("cell.get", json!({"cell": "A2"})).unwrap();
    assert_eq!(cell["formula"], json!("=SUM(1.5,2)"));
    assert_eq!(cell["formulaLocal"], json!("=SUM(1,5;2)"));
}

#[test]
fn options_dialog_applies_the_region_at_once() {
    let mut h = harness(blank());
    h.state_mut().open_dialog("options", json!({}));
    h.run_steps(2);
    h.state_mut().dialog.as_mut().unwrap().values.insert("regionalFormat".into(), json!("de-DE"));
    h.state_mut().dialog.as_mut().unwrap().values.insert("useSystemSeparators".into(), json!(true));
    let app = h.state_mut();
    let d = app.dialog.take().unwrap();
    gridcraft_ui_egui::options::apply(app, &d).unwrap();
    assert_eq!(app.session.locale().regional.tag, "de-DE");
    assert_eq!(app.session.locale().regional.decimal, ',');
}

#[test]
fn today_shortcut_inserts_a_date_in_the_short_format() {
    let mut h = harness(blank());
    key(&mut h, Key::Semicolon, Modifiers::COMMAND);
    assert!(h.state().editor.is_some());
    let text = h.state().editor.as_ref().map(|e| e.text.clone()).unwrap_or_default();
    assert!(text.contains('/'), "{text}");
}

/// Switches the session to German interface, formulas and region.
fn german(h: &mut egui_kittest::Harness<'static, SheetApp>) {
    h.state_mut().session.execute("app.setLocale", json!({"uiLanguage": "de-DE", "regionalFormat": "de-DE"})).unwrap();
    h.run_steps(2);
}

#[test]
fn quick_dialogs_send_what_was_typed_as_local_text() {
    let mut h = harness(blank());
    german(&mut h);
    let app = h.state_mut();

    app.open_dialog("defineName", json!({}));
    let mut d = app.dialog.take().unwrap();
    d.values.insert("name".into(), json!("Satz"));
    d.values.insert("refersTo".into(), json!("=SUMME(1,5;2)"));
    let params = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap();
    assert_eq!(params["refersToLocal"], json!("=SUMME(1,5;2)"));
    assert!(params.get("refersTo").is_none(), "{params}");
    app.run_typed(d.command.unwrap(), params).unwrap();
    let stored = app.session.active().unwrap().wb.names.iter().find(|n| n.name == "Satz").map(|n| n.formula.clone());
    assert_eq!(stored.as_deref().map(|f| f.trim_start_matches('=')), Some("SUM(1.5,2)"));

    app.open_dialog("dataValidation", json!({}));
    let mut d = app.dialog.take().unwrap();
    d.values.insert("type".into(), json!("decimal"));
    d.values.insert("operator".into(), json!("greater"));
    d.values.insert("formula1".into(), json!("1,5"));
    d.values.insert("formula2".into(), json!(""));
    let params = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap();
    assert_eq!(params["formula1Local"], json!("1,5"));
    assert!(params.get("formula1").is_none() && params.get("formula2Local").is_none(), "{params}");
    app.run_typed(d.command.unwrap(), params).unwrap();
    let f1 = app.session.active().and_then(|d| d.wb.active().and_then(|s| s.validations.first().map(|v| v.f1.clone())));
    assert_eq!(f1.as_deref(), Some("1.5"));

    app.open_dialog("cfQuick", json!({"type": "cellIs", "operator": "greater"}));
    let mut d = app.dialog.take().unwrap();
    d.values.insert("value".into(), json!("1,5"));
    let params = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap();
    assert_eq!(params["rule"]["valueLocal"], json!("1,5"));
    assert!(params["rule"].get("value").is_none(), "{params}");
    app.run_typed(d.command.unwrap(), params).unwrap();
    let rule = app.session.active().and_then(|d| d.wb.active().and_then(|s| s.cond_formats.first().map(|c| format!("{:?}", c.rule))));
    assert!(rule.is_some_and(|r| r.contains("1.5")));
}

#[test]
fn series_stop_is_read_in_the_region_and_bad_numbers_are_reported() {
    let mut h = harness(blank());
    german(&mut h);
    let app = h.state_mut();
    app.open_dialog("series", json!({}));
    let mut d = app.dialog.take().unwrap();
    d.values.insert("stop".into(), json!("1.000,5"));
    let params = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap();
    assert_eq!(params["stop"], json!(1000.5));
    d.values.insert("stop".into(), json!("abc"));
    let error = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap_err();
    assert!(error.contains("abc"), "{error}");
    d.values.insert("stop".into(), json!(""));
    let params = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap();
    assert!(params.get("stop").is_none(), "{params}");
}

#[test]
fn region_numbers_are_not_completed_from_the_column() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["1,5 kg"], ["Osten"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A3"})).unwrap();
    let mut h = harness(s);
    german(&mut h);
    text(&mut h, "1,5");
    assert!(h.state().editor.as_ref().is_some_and(|e| e.completion.is_none()), "numbers are never completed");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A3"), Value::Number(1.5));
    text(&mut h, "Ost");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A4"), Value::from("Osten"));
}

#[test]
fn number_helpers_and_filter_blanks_follow_the_region() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Kopf"], ["x"], [""], ["y"]]})).unwrap();
    s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();
    let mut h = harness(s);
    german(&mut h);
    assert_eq!(h.state().localize_decimal("12.50"), "12,50");
    assert_eq!(h.state().number_text(2.5), "2,5");
    assert_eq!(h.state().parse_number("1.000,5"), Some(1000.5));
    assert_eq!(h.state().parse_number("1.5x"), None);
    // Number boxes (sizes, widths) read plain numbers; text fields read like a cell, so a date
    // series takes a date as its stop value.
    let boxes = h.state().number_style();
    assert_eq!(boxes.parse("1.000,5"), Some(1000.5));
    for text in ["1/2", "10:30", "1.2.2026"] {
        assert_eq!(boxes.parse(text), None, "{text}");
    }
    assert!(h.state().parse_number("1.2.2026").is_some_and(|n| n > 40_000.0));
    // The filter menu hides the engine's "(Blanks)" row behind the interface language's word.
    let wb = h.state().session.active().unwrap().wb.clone();
    let values = gridcraft_engine::cmd::data::filter_values(&wb, 0, 0);
    assert!(values.iter().any(|(label, _)| label == "(Leere)"), "{values:?}");
}

#[test]
fn formula_checkers_show_the_formula_language() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["=SUM(1.5,2)/0"]]})).unwrap();
    let mut h = harness(s);
    german(&mut h);
    let report = h.state_mut().session.run("formulas.errorChecking", json!({})).unwrap();
    let shown = gridcraft_ui_egui::dialogs::local_view(&report);
    assert_eq!(shown["errors"][0]["formula"], json!("=SUMME(1,5;2)/0"), "{shown}");
    assert_eq!(shown["errors"][0]["error"], report["errors"][0]["errorLocal"]);
    assert!(shown["errors"][0].get("formulaLocal").is_none(), "{shown}");
    // The Evaluate Formula steps read the same way.
    let steps = h.state_mut().session.run("formulas.evaluateFormula", json!({"cell": "A1"})).unwrap();
    let shown = gridcraft_ui_egui::dialogs::local_view(&steps);
    assert_eq!(shown["formula"], json!("=SUMME(1,5;2)/0"), "{shown}");
    assert!(
        shown["steps"].as_array().is_some_and(|s| s.iter().any(|st| st["expression"] == json!("SUMME(1,5;2)") && st["value"] == json!("3,5"))),
        "{shown}"
    );
}

#[test]
fn the_options_dialog_knows_the_system_region_whatever_is_stored() {
    let mut h = harness(blank());
    h.state_mut().session.set_system_locale("de_DE.UTF-8");
    h.state_mut().session.execute("app.setLocale", json!({"regionalFormat": "en-US"})).unwrap();
    h.state_mut().open_dialog("options", json!({}));
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!(d.values["regionalFormat"], json!("en-US"));
    assert_eq!(d.values["systemRegion"], json!("de-DE"));
    // The fields start with the stored region's separators.
    assert_eq!((d.values["decimalSeparator"].clone(), d.values["thousandsSeparator"].clone()), (json!("."), json!(",")));
}

#[test]
fn engine_prefills_are_spelled_once() {
    let mut h = harness(blank());
    h.state_mut().session.execute("app.setLocale", json!({"uiLanguage": "fr-FR", "regionalFormat": "fr-FR"})).unwrap();
    h.run_steps(2);
    // French spells MIRR `TRIM` and VARP `VAR.P`: translating the engine's text again would
    // turn `TRIM` into `SUPPRESPACE` (the French name of TRIM).
    for (name, expected) in [("MIRR", "=TRIM("), ("VARP", "=VAR.P("), ("TRIM", "=SUPPRESPACE(")] {
        h.state_mut().run_or_alert("formulas.insertFunction", json!({"name": name}));
        let shown = h.state().editor.as_ref().map(|e| e.text.clone());
        assert_eq!(shown.as_deref(), Some(expected), "{name}");
        h.state_mut().editor = None;
    }
}

#[test]
fn switching_the_language_rebuilds_texts_and_shortcuts() {
    let mut h = harness(blank());
    assert_eq!(h.state().l10n.tag(), "en-US");
    h.state_mut().run_or_alert("app.setLocale", json!({"uiLanguage": "pt-BR"}));
    h.run_steps(2);
    assert_eq!(h.state().l10n.tag(), "pt-BR");
    // Portuguese Excel binds Bold to Ctrl+N (and Save to Ctrl+B); English binds Ctrl+N to New.
    key(&mut h, Key::N, Modifiers::COMMAND);
    let bold = h.state_mut().session.run("cell.get", json!({"cell": "A1"})).unwrap()["style"]["font"]["bold"].clone();
    assert_eq!(bold, json!(true));
    h.state_mut().run_or_alert("app.setLocale", json!({"uiLanguage": "en-US"}));
    h.run_steps(2);
    assert_eq!(h.state().l10n.tag(), "en-US");
}

#[test]
fn options_dialog_applies_custom_separators_and_keeps_invalid_ones_open() {
    let mut h = harness(blank());
    h.state_mut().open_dialog("options", json!({}));
    h.run_steps(2);
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("useSystemSeparators".into(), json!(false));
        d.values.insert("decimalSeparator".into(), json!(","));
        d.values.insert("thousandsSeparator".into(), json!(","));
    }
    h.run_steps(2);
    h.get_by_label_contains("OK").click();
    h.run_steps(3);
    assert!(h.state().dialog.as_ref().is_some_and(|d| d.name == "options"), "an invalid pair keeps the dialog open");
    assert!(h.state().message.is_some(), "and says why");
    assert!(h.state().session.prefs.use_system_separators, "nothing was applied");

    h.state_mut().message = None;
    h.state_mut().dialog.as_mut().unwrap().values.insert("thousandsSeparator".into(), json!("."));
    h.run_steps(2);
    h.get_by_label_contains("OK").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let prefs = &h.state().session.prefs;
    assert!(!prefs.use_system_separators);
    assert_eq!((prefs.decimal_separator.as_str(), prefs.thousands_separator.as_str()), (",", "."));
    assert_eq!(h.state().session.locale().regional.decimal, ',');
}

#[test]
fn the_options_dialog_asks_for_the_cjk_fonts() {
    let mut h = harness(blank());
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    h.state_mut().open_dialog("options", json!({}));
    h.run_steps(2);
    // The language list shows native names such as 日本語.
    assert!(h.state().dialog.as_ref().is_some_and(|d| d.name == "options"));
    let needs = h.state_mut().font_needs();
    assert!(needs.cjk);
}

#[test]
fn the_font_set_is_stable_across_opening_and_closing_the_options_dialog() {
    let mut h = harness(blank());
    h.run_steps(2);
    let before = h.state_mut().font_needs().key();
    assert!(before.0.is_none(), "{before:?}: no CJK needed yet");
    h.state_mut().open_dialog("options", json!({}));
    h.run_steps(2);
    let open = h.state_mut().font_needs().key();
    assert!(open.0.is_some());
    h.state_mut().dialog = None;
    h.run_steps(2);
    assert_eq!(h.state_mut().font_needs().key(), open, "closing the dialog must not rebuild the fonts again");
}

#[test]
fn the_language_picker_asks_for_the_cjk_fonts_when_it_opens() {
    let mut h = harness(blank());
    h.state_mut().ui.ribbon_tab = "View".into();
    h.run_steps(2);
    assert!(!h.state_mut().font_needs().cjk, "a closed picker shows only the current language");
    h.get_by_value("English (United States)").click();
    h.run_steps(2);
    assert!(h.state_mut().font_needs().cjk, "the open list shows 日本語, 中文 and 한국어");
}

#[test]
fn a_downloaded_font_that_is_not_a_font_is_dropped_once() {
    // Static hosts answer a missing `fonts/cjk-ja.ttf` with their HTML page (status 200); egui
    // panics on font data it cannot parse.
    let inbox: gridcraft_ui_egui::Inbox = Default::default();
    let requested = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let (fetch_inbox, fetch_requested) = (inbox.clone(), requested.clone());
    let services = gridcraft_ui_egui::Services {
        fetch_font: Some(Box::new(move |name: &str| {
            fetch_requested.borrow_mut().push(name.to_string());
            fetch_inbox.lock().unwrap().push((name.to_string(), b"<!doctype html><html><body>app</body></html>".to_vec()));
        })),
        font_inbox: Some(inbox),
        ..Default::default()
    };
    let mut h = harness_with(blank(), services);
    h.state_mut().run("app.language.set", json!({"language": "ja"})).unwrap();
    h.run_steps(4);
    assert!(h.state_mut().font_needs().downloaded.is_empty());
    assert_eq!(*requested.borrow(), ["cjk-ja.ttf"], "a failed download is not requested again every frame");
}

#[test]
fn the_name_scope_is_a_drop_down_with_the_localized_workbook() {
    let mut h = harness(blank());
    german(&mut h);
    let app = h.state_mut();
    app.open_dialog("defineName", json!({}));
    let mut d = app.dialog.take().unwrap();
    let options = d
        .fields
        .iter()
        .find_map(|f| match f {
            gridcraft_ui_egui::dialogs::Field::Combo { key: "scope", options, .. } => Some(options.clone()),
            _ => None,
        })
        .expect("scope is a drop-down");
    assert_eq!(options[0], ("Workbook".to_string(), app.l10n.tr("Workbook").into_owned()));
    let sheet = app.session.active().and_then(|d| d.wb.active().map(|s| s.name.clone())).unwrap();
    assert!(options.iter().any(|(value, shown)| *value == sheet && *shown == sheet), "{options:?}");
    d.values.insert("name".into(), json!("Satz"));
    d.values.insert("refersTo".into(), json!("=1"));
    d.values.insert("scope".into(), json!("Workbook"));
    let params = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap();
    assert!(params.get("scope").is_none(), "the workbook scope is the default: {params}");
    d.values.insert("scope".into(), json!(sheet.clone()));
    let params = gridcraft_ui_egui::dialogs::build_params(&d, app).unwrap();
    assert_eq!(params["scope"], json!(sheet));
}

#[test]
fn name_values_are_spelled_in_the_region() {
    let mut h = harness(blank());
    german(&mut h);
    let loc = h.state().session.locale();
    let text = |v| gridcraft_ui_egui::dialogs::value_text(&loc, &v);
    assert_eq!(text(json!(1.5)), "1,5");
    assert_eq!(text(json!("x")), "x");
    assert_eq!(text(json!(true)), loc.formula.bool_true);
    assert_ne!(text(json!(true)), "TRUE");
    assert_eq!(text(json!({"error": "#N/A"})), loc.formula.local_error("#N/A"));
    let (col, row) = (loc.regional.array_col, loc.regional.array_row);
    assert_eq!(text(json!([[1.5, 2], [3, 4]])), format!("{{1,5{col}2{row}3{col}4}}"));
}

#[test]
fn the_formula_being_edited_follows_a_language_switch() {
    let mut h = harness(blank());
    german(&mut h);
    h.state_mut().begin_edit(Some("=SUMME(1,5;2)".into()), false);
    h.run_steps(2);
    h.state_mut().run_or_alert("app.setLocale", json!({"uiLanguage": "en-US", "regionalFormat": "en-US"}));
    h.run_steps(2);
    let ed = h.state().editor.as_ref().expect("the edit stays open");
    assert_eq!(ed.text, "=SUM(1.5,2)");
    assert_eq!(ed.caret, ed.text.chars().count());
}

#[test]
fn a_language_switch_closes_the_filter_menu_and_forgets_its_check_boxes() {
    let mut h = harness(blank());
    h.state_mut().grid.filter_checks = Some((0, vec![("a".into(), false)]));
    h.state_mut().grid.close_filter_menu();
    assert!(h.state().grid.filter_checks.is_none());
    h.state_mut().grid.filter_checks = Some((0, vec![("a".into(), false)]));
    h.state_mut().run_or_alert("app.setLocale", json!({"uiLanguage": "pt-BR"}));
    h.run_steps(2);
    assert!(h.state().grid.filter_checks.is_none());
}

#[test]
fn the_pick_list_offers_what_is_typed_in_the_region() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1.5], ["x"], [1.5]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A4"})).unwrap();
    let mut h = harness(s);
    german(&mut h);
    assert_eq!(gridcraft_ui_egui::dialogs::pick_list_items(h.state()), vec!["1,5".to_string(), "x".to_string()]);
}

#[test]
fn the_undo_list_shows_command_labels_in_the_interface_language() {
    use gridcraft_engine::command_specs;
    let mut h = harness(blank());
    h.state_mut().run_or_alert("app.setLocale", json!({"uiLanguage": "de-DE"}));
    h.run_steps(2);
    assert_eq!(h.state().l10n.tag(), "de-DE");
    let l = h.state().l10n;
    let specs = command_specs();
    let unique = |label: &str| specs.iter().filter(|s| s.label.trim_end_matches('…') == label).count() == 1;
    let (english, shown) = specs
        .iter()
        .filter(|s| !s.label.ends_with('…') && unique(s.label))
        .find_map(|s| l.command_label(s.id).filter(|t| *t != s.label).map(|t| (s.label.to_string(), t.trim_end_matches('…').to_string())))
        .expect("some command is translated");
    assert_eq!(gridcraft_ui_egui::l10n::history_label(&l, &english), shown);
    assert_eq!(gridcraft_ui_egui::l10n::history_label(&l, "Free text"), "Free text");
}

#[test]
fn general_numbers_fit_by_the_cells_own_font() {
    // 9pt marks in narrow columns (a stored width of 4, i.e. 28px; common in mark sheets): "100" fits.
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[100, 123456789]]})).unwrap();
    s.execute("home.fontSize", json!({"range": "A1:B1", "size": 9})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "width": 28})).unwrap();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let h = harness(s);
    let texts: Vec<String> = h
        .output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t == "100"), "100 drawn in full: {texts:?}");
    // A number that really doesn't fit still shows #### (once: only B1).
    assert_eq!(texts.iter().filter(|t| t.starts_with('#')).count(), 1, "{texts:?}");
}

fn text_shapes(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(egui::epaint::TextShape, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.clone(), clip)),
        egui::Shape::Vec(v) => v.iter().for_each(|s| text_shapes(s, clip, out)),
        _ => {}
    }
}

#[test]
fn rotated_wrapped_text_is_drawn_rotated_inside_its_cell() {
    // Narrow columns with tall, upright headers (mark sheets): rotation and Wrap Text together.
    let header = "Term 1 : Drama : Continuous Assessment";
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[header, header]]})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1:B1", "on": true})).unwrap();
    s.execute("home.orientation", json!({"range": "A1", "angle": "up"})).unwrap();
    s.execute("home.orientation", json!({"range": "B1", "angle": "down"})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "chars": 6})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1:1", "height": 267})).unwrap();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let h = harness(s);
    let mut found = Vec::new();
    for c in &h.output().shapes {
        text_shapes(&c.shape, c.clip_rect, &mut found);
    }
    let rotated: Vec<_> = found.iter().filter(|(t, _)| t.galley.job.text == header).collect();
    assert_eq!(rotated.len(), 2, "both headers drawn as one text block each");
    for (t, clip) in rotated {
        assert!((t.angle.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "drawn at ±90°, got {}", t.angle);
        // Wrapped along the row height into a couple of long lines, not across the narrow column.
        assert!(t.galley.rows.len() <= 3, "{} lines", t.galley.rows.len());
        let bb = t.visual_bounding_rect();
        assert!(bb.height() > bb.width(), "upright, not flat: {bb:?}");
        assert!(clip.expand(1.0).contains_rect(bb), "inside its cell: {bb:?} not in {clip:?}");
        // Upright text never spills into the neighbouring columns: the clip is the cell itself.
        assert!(clip.width() < 70.0, "clip spans one 6-character column, got {clip:?}");
    }
}
