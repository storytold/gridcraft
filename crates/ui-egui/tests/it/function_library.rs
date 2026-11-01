//! Formulas › Function Library: the labelled buttons, their category menus, the More Functions
//! cascade, the AutoSum big split button, the Recently Used keytip and list, and the Insert Function
//! dialog's categories.

use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn formulas_tab(width: f32) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = "Formulas".into();
    crate::common::harness_with(crate::common::builder().with_os(OperatingSystem::Nix).with_size(vec2(width, 760.0)), app, 4)
}

fn editor_text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

fn input(h: &Harness, cell: &str) -> String {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().input_text(CellRef::parse(cell).unwrap())
}

/// Clicks a button by its name (a tooltip also names an icon-only button, such as the formula
/// bar's fx, which is not a button to accessibility).
fn click(h: &mut Harness, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(3);
}

/// Clicks a menu row through accessibility, which reaches rows scrolled out of a long menu.
fn pick(h: &mut Harness, label: &str) {
    h.get_by_label(label).click_accesskit();
    h.run_steps(3);
}

fn close_menus(h: &mut Harness) {
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.state_mut().editor = None;
    h.run_steps(2);
}

#[test]
fn autosum_is_a_big_split_button_then_the_categories_in_columns_of_three() {
    let h = formulas_tab(1440.0);
    let columns: [&[&str]; 3] =
        [&["Recently Used", "Financial", "Logical"], &["Text", "Date & Time", "Lookup & Reference"], &["Math & Trig", "More Functions"]];
    let rect = |l: &str| h.get_by_role_and_label(egui::accesskit::Role::Button, l).rect();
    for column in columns {
        for pair in column.windows(2) {
            let (a, b) = (rect(pair[0]), rect(pair[1]));
            assert!((a.left() - b.left()).abs() < 0.5, "{pair:?} share a left edge");
            assert!(a.bottom() <= b.top() + 0.5, "{pair:?} stack in order");
        }
    }
    for pair in columns.windows(2) {
        assert!(rect(pair[0][0]).right() <= rect(pair[1][0]).left(), "columns run left to right");
    }
    // Insert Function and AutoSum are big buttons, to the left of the columns.
    let (insert, sum) = (rect("Insert Function"), rect("AutoSum"));
    assert!(insert.right() <= sum.left() && sum.right() <= rect("Recently Used").left());
    assert!(sum.top() <= rect("Recently Used").top() + 0.5, "AutoSum starts at the top of the group");
    assert!(h.get_by_label("AutoSum options").rect().bottom() > rect("Financial").bottom(), "its arrow part reaches below the second row");
    // A full column stands where the tab's other columns of three small buttons do (Formula
    // Auditing's), so it fits the ribbon in any font.
    let (top, bottom) = (rect("Show Formulas").top(), rect("Evaluate Formula").bottom());
    for column in &columns[..2] {
        let (t, b) = (rect(column[0]).top(), rect(column[2]).bottom());
        assert!((t - top).abs() < 0.5 && (b - bottom).abs() < 0.5, "{column:?} spans {t}..{b}, Formula Auditing {top}..{bottom}");
    }
}

#[test]
fn each_category_menu_lists_its_functions_then_insert_function() {
    let mut h = formulas_tab(1440.0);
    // Recently Used lists the functions of formulas entered.
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "=SUM(1)+AVERAGE(2)"})).unwrap();
    h.run_steps(2);
    for (button, function) in [
        ("Recently Used", "SUM"),
        ("Financial", "PMT"),
        ("Logical", "IF"),
        ("Text", "CONCAT"),
        ("Date & Time", "TODAY"),
        ("Lookup & Reference", "XLOOKUP"),
        ("Math & Trig", "ROUND"),
    ] {
        click(&mut h, button);
        h.get_by_label("Insert Function…");
        pick(&mut h, function);
        assert_eq!(editor_text(&h).as_deref(), Some(format!("={function}(").as_str()), "{button} › {function} starts the formula");
        close_menus(&mut h);
    }
    // The menu's last row opens Insert Function on the menu's category.
    click(&mut h, "Date & Time");
    click(&mut h, "Insert Function…");
    let d = h.state().dialog.as_ref().expect("Insert Function opens");
    assert_eq!(d.name(), "insertFunction");
    assert_eq!(d.values.get("category"), Some(&json!("Date & Time")));
}

#[test]
fn more_functions_cascades_into_the_remaining_categories() {
    let mut h = formulas_tab(1440.0);
    click(&mut h, "More Functions");
    for c in ["Statistical", "Engineering", "Cube", "Information", "Compatibility", "Web"] {
        h.get_by_label(c);
    }
    click(&mut h, "Engineering");
    h.get_by_label("Insert Function…");
    pick(&mut h, "BIN2DEC");
    assert_eq!(editor_text(&h).as_deref(), Some("=BIN2DEC("));
    close_menus(&mut h);
    click(&mut h, "More Functions");
    click(&mut h, "Statistical");
    pick(&mut h, "Insert Function…");
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("category")), Some(&json!("Statistical")));
}

#[test]
fn autosum_runs_sum_and_its_arrow_opens_the_shared_menu() {
    let mut h = formulas_tab(1440.0);
    {
        let s = &mut h.state_mut().session;
        s.execute("range.setValues", json!({"range": "A1", "values": [[1, 4], [2, 5], [3, 6]]})).unwrap();
        s.execute("selection.set", json!({"cell": "A4"})).unwrap();
    }
    h.run_steps(2);
    click(&mut h, "AutoSum");
    assert_eq!(input(&h, "A4"), "=SUM(A1:A3)");
    h.state_mut().session.execute("selection.set", json!({"cell": "B4"})).unwrap();
    click(&mut h, "AutoSum options");
    for item in ["Sum", "Average", "Max", "Min", "More Functions…"] {
        h.get_by_label(item);
    }
    click(&mut h, "Count Numbers");
    assert_eq!(input(&h, "B4"), "=COUNT(B1:B3)");
    click(&mut h, "AutoSum options");
    click(&mut h, "More Functions…");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("insertFunction"));
}

fn key_event(key: Key, pressed: bool) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }
}

#[test]
fn the_recently_used_keytip_opens_its_menu() {
    let mut h = formulas_tab(1440.0);
    // Recently Used lists the functions of formulas entered.
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "=SUM(1)+AVERAGE(2)"})).unwrap();
    h.run_steps(2);
    h.input_mut().events.extend([key_event(Key::F10, true), key_event(Key::F10, false)]);
    h.run_steps(2);
    for k in [Key::M, Key::R] {
        h.input_mut().events.extend([key_event(k, true), Event::Text(k.name().to_ascii_lowercase()), key_event(k, false)]);
        h.run_steps(3);
    }
    assert_eq!(h.state().keytips.prefix(), None, "MR finishes the sequence");
    h.get_by_label("Insert Function…");
    pick(&mut h, "AVERAGE");
    assert_eq!(editor_text(&h).as_deref(), Some("=AVERAGE("));
}

#[test]
fn recently_used_lists_the_engines_list_newest_first() {
    let mut h = formulas_tab(1440.0);
    // The ribbon and the control channel go through app.run; MCP and scripts through the session.
    // The engine keeps the list (see the engine's recently_used tests); the menu shows it.
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "=NPV(0.1,1,2)"})).unwrap();
    h.state_mut().session.execute("cell.set", json!({"cell": "A2", "input": "=IRR({-10,6,6})+ROUND(1,0)"})).unwrap();
    h.state_mut().session.execute("range.setValues", json!({"range": "B1", "values": [["=FV(0.1,2,-1)", 4, "text"]]})).unwrap();
    h.run_steps(2);
    assert_eq!(&h.state().session.prefs.recent_functions[..4], ["FV", "IRR", "ROUND", "NPV"]);
    click(&mut h, "Recently Used");
    let top = |h: &Harness, name: &str| h.get_by_label(name).rect().top();
    assert!(top(&h, "FV") < top(&h, "IRR") && top(&h, "IRR") < top(&h, "ROUND") && top(&h, "ROUND") < top(&h, "NPV"));
}

/// Opens the Insert Function category list (the dialog's only combo box).
fn open_categories(h: &mut Harness) {
    h.get_by_role(egui::accesskit::Role::ComboBox).click();
    h.run_steps(3);
}

/// Picks a category from the open list (the ribbon has buttons with the same names).
fn choose(h: &mut Harness, category: &str) {
    h.get_all_by_label(category).last().expect("the category is listed").click();
    h.run_steps(3);
}

fn row(name: &str, category: &str) -> String {
    format!("{name} ({category})")
}

#[test]
fn insert_function_has_its_title_and_a_working_category_list() {
    let mut h = formulas_tab(1440.0);
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "=NPV(0.1,1,2)"})).unwrap();
    h.run_steps(2);
    click(&mut h, "Insert Function");
    let d = h.state().dialog.as_ref().expect("Insert Function opens");
    assert_eq!(d.title, "Insert Function");
    // Most Recently Used comes first, newest first.
    let npv = h.get_by_label(&row("NPV", "Financial")).rect();
    assert!(npv.top() < h.get_by_label(&row("SUM", "Math & Trig")).rect().top());
    assert!(h.query_by_label(&row("PMT", "Financial")).is_some(), "the default list follows");
    // Pick a category from the list.
    open_categories(&mut h);
    choose(&mut h, "Date & Time");
    assert!(h.query_by_label(&row("TODAY", "Date & Time")).is_some());
    assert!(h.query_by_label(&row("NPV", "Financial")).is_none());
    // All lists everything; a category given by its engine id works too (control channel).
    open_categories(&mut h);
    choose(&mut h, "All");
    assert!(h.query_by_label(&row("TODAY", "Date & Time")).is_some() && h.query_by_label(&row("NPV", "Financial")).is_some());
    h.state_mut().dialog.as_mut().unwrap().values.insert("category".into(), json!("MathTrig"));
    h.run_steps(2);
    assert!(h.query_by_label(&row("ROUND", "Math & Trig")).is_some() && h.query_by_label(&row("TODAY", "Date & Time")).is_none());
    // A search looks through every category.
    h.state_mut().dialog.as_mut().unwrap().search = "lookup".into();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("category"), Some(&json!("All")));
    assert!(h.query_by_label(&row("XLOOKUP", "Lookup & Reference")).is_some());
    // OK starts the formula. The function counts as recently used once the formula is entered,
    // not before (the edit may still be cancelled, or there may be no cell to edit).
    pick(&mut h, &row("XLOOKUP", "Lookup & Reference"));
    click(&mut h, "   OK   ");
    assert_eq!(editor_text(&h).as_deref(), Some("=XLOOKUP("));
    assert_ne!(h.state().session.prefs.recent_functions[0], "XLOOKUP");
}

#[test]
fn insert_function_lets_tab_reach_the_category_list() {
    use egui::accesskit::Role;
    let mut h = formulas_tab(1440.0);
    click(&mut h, "Insert Function");
    h.run_steps(3);
    let search_focused = |h: &Harness| h.get_all_by_role(Role::TextInput).any(|n| n.is_focused());
    assert!(search_focused(&h), "the search box takes the focus when the dialog opens");
    h.input_mut().events.push(Event::Text("npv".into()));
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.search.as_str()), Some("npv"));
    // Tab moves on to the category list, and the search box does not take the focus back.
    h.input_mut().events.extend([key_event(Key::Tab, true), key_event(Key::Tab, false)]);
    h.run_steps(5);
    assert!(h.get_by_role(Role::ComboBox).is_focused(), "Tab reaches the category list");
    assert!(!search_focused(&h));
    // The list opens from the keyboard (its Date & Time joins the ribbon's).
    let listed = |h: &Harness| h.get_all_by_label("Date & Time").count();
    let before = listed(&h);
    h.input_mut().events.extend([key_event(Key::Space, true), key_event(Key::Space, false)]);
    h.run_steps(3);
    assert_eq!(listed(&h), before + 1, "the list is open");
}

#[test]
fn enter_on_the_category_list_does_not_confirm_insert_function() {
    use egui::accesskit::Role;
    let mut h = formulas_tab(1440.0);
    click(&mut h, "Insert Function");
    h.run_steps(3);
    let open = |h: &Harness| h.state().dialog.as_ref().is_some_and(|d| d.name == "insertFunction");
    let listed = |h: &Harness| h.get_all_by_label("Date & Time").count();
    let closed = listed(&h);
    h.input_mut().events.extend([key_event(Key::Tab, true), key_event(Key::Tab, false)]);
    h.run_steps(5);
    assert!(h.get_by_role(Role::ComboBox).is_focused(), "Tab reaches the category list");
    // Enter on the focused list opens it, as Space does; the dialog stays open and nothing is
    // inserted.
    h.input_mut().events.extend([key_event(Key::Enter, true), key_event(Key::Enter, false)]);
    h.run_steps(3);
    assert!(open(&h), "Enter on the category list leaves the dialog open");
    assert_eq!(editor_text(&h), None, "no function is inserted");
    assert_eq!(listed(&h), closed + 1, "Enter opens the list");
    // Enter again, with the list open, closes it and still does not confirm.
    h.input_mut().events.extend([key_event(Key::Enter, true), key_event(Key::Enter, false)]);
    h.run_steps(3);
    assert!(open(&h), "Enter in the open list leaves the dialog open");
    assert_eq!(editor_text(&h), None);
    // Back in the search box (Shift+Tab), Enter confirms as before.
    let shift_tab = |pressed| Event::Key { key: Key::Tab, physical_key: None, pressed, repeat: false, modifiers: Modifiers::SHIFT };
    h.input_mut().events.extend([shift_tab(true), shift_tab(false)]);
    h.run_steps(3);
    assert!(!h.get_by_role(Role::ComboBox).is_focused(), "Shift+Tab leaves the category list");
    h.input_mut().events.extend([key_event(Key::Enter, true), key_event(Key::Enter, false)]);
    h.run_steps(3);
    assert!(!open(&h), "Enter in the search box confirms");
    assert!(editor_text(&h).is_some_and(|t| t.starts_with('=') && t.ends_with('(')), "{:?}", editor_text(&h));
}

/// The text shapes the last frame painted, with their left edge and width.
fn painted_text(h: &Harness) -> Vec<(String, f32, f32)> {
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            out.push((t.galley.text().to_string(), t.pos.x, t.galley.size().x));
        }
    }
    out
}

#[test]
fn insert_function_keeps_the_category_column_clear_of_long_names() {
    let mut h = formulas_tab(1440.0);
    // A wide interface font (as on some Windows and Linux desktops, or a long formula language)
    // makes every name wider than the dialog was laid out for.
    h.ctx.all_styles_mut(|s| {
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(20.0));
    });
    click(&mut h, "Insert Function");
    // Some of the longest function names in the catalog, and a short one, in one list (the
    // signature under the list starts with a name too, and is not a row).
    h.state_mut().dialog.as_mut().unwrap().search = "forecast".into();
    h.run_steps(3);
    let shapes = painted_text(&h);
    let names: Vec<&(String, f32, f32)> = shapes.iter().filter(|(t, _, _)| t.starts_with("FORECAST") && !t.contains('(')).collect();
    let column = shapes.iter().filter(|(t, _, _)| t == "Statistical").map(|(_, x, _)| *x).fold(f32::INFINITY, f32::min);
    assert!(names.iter().any(|(t, _, _)| t == "FORECAST.ETS.SEASONALITY"), "the long name is listed: {shapes:?}");
    for (name, x, w) in names {
        assert!(x + w < column, "{name} ends at {} and the category column starts at {column}", x + w);
    }
}
