//! File › Info › Protect Workbook › Encrypt with Password sets and removes the password to open
//! the workbook (#770).

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

const PW: &str = "synthetic-pw-5150";
const REQUIRED: &str = "A password is required to open this workbook.";

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn dialog(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name().to_string())
}

fn password(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<String> {
    h.state().session.active().unwrap().password.clone()
}

fn set(h: &mut egui_kittest::Harness<'static, SheetApp>, key: &str, value: &str) {
    h.state_mut().dialog.as_mut().unwrap().values.insert(key.into(), json!(value));
    h.run_steps(2);
}

fn ok(h: &mut egui_kittest::Harness<'static, SheetApp>) {
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
}

/// File › Info › Protect Workbook › `item`.
fn protect_menu(h: &mut egui_kittest::Harness<'static, SheetApp>, item: &str) {
    h.state_mut().run("ui.backstage", json!({"page": "info"})).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, "Protect Workbook").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, item).click();
    h.run_steps(3);
}

fn says_a_password_is_required(h: &egui_kittest::Harness<'static, SheetApp>) -> bool {
    h.query_by_label(REQUIRED).is_some()
}

#[test]
fn encrypt_with_password_asks_twice_and_the_info_page_says_a_password_is_required() {
    let mut h = harness();
    h.state_mut().run("ui.backstage", json!({"page": "info"})).unwrap();
    h.run_steps(3);
    assert!(!says_a_password_is_required(&h));

    protect_menu(&mut h, "Encrypt with Password");
    assert_eq!(dialog(&h).as_deref(), Some("encryptPassword"));
    set(&mut h, "password", PW);
    ok(&mut h);
    assert_eq!(dialog(&h).as_deref(), Some("confirmPassword"), "the password is asked again before it's set");
    assert_eq!(password(&h), None);

    // A different confirmation is refused and nothing changes.
    set(&mut h, "confirmPassword", "something else");
    ok(&mut h);
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("not identical")));
    assert_eq!(password(&h), None);
    h.state_mut().message = None;

    set(&mut h, "confirmPassword", PW);
    ok(&mut h);
    assert_eq!(dialog(&h), None);
    assert_eq!(password(&h).as_deref(), Some(PW));
    assert!(h.state().session.active().unwrap().is_dirty(), "setting the password is an unsaved change");
    assert!(h.state().backstage.is_some(), "the Info page stays open");
    assert!(says_a_password_is_required(&h));
}

#[test]
fn clearing_the_password_field_removes_the_password() {
    let mut h = harness();
    h.state_mut().run("file.encryptWithPassword", json!({"password": PW})).unwrap();
    protect_menu(&mut h, "Encrypt with Password");
    assert!(says_a_password_is_required(&h));
    assert_eq!(dialog(&h).as_deref(), Some("encryptPassword"));
    let field = h.state().dialog.as_ref().unwrap().values.get("password").and_then(|v| v.as_str()).unwrap().to_string();
    assert!(!field.is_empty() && field != PW, "the field shows that a password is set, not the password");

    set(&mut h, "password", "");
    ok(&mut h);
    assert_eq!(dialog(&h), None, "removing the password asks for no confirmation");
    assert_eq!(password(&h), None);
    assert!(!says_a_password_is_required(&h));
}

#[test]
fn protect_workbook_structure_is_still_in_the_menu() {
    let mut h = harness();
    protect_menu(&mut h, "Protect Workbook Structure");
    assert_eq!(dialog(&h).as_deref(), Some("protectWorkbook"));
}

#[test]
fn the_dialog_never_holds_the_current_password_and_ok_keeps_it() {
    let mut h = harness();
    h.state_mut().run("file.encryptWithPassword", json!({"password": PW})).unwrap();
    protect_menu(&mut h, "Encrypt with Password");
    assert_eq!(dialog(&h).as_deref(), Some("encryptPassword"));
    let values = serde_json::to_string(&h.state().dialog.as_ref().unwrap().values).unwrap();
    assert!(!values.contains(PW), "the password to open is not put in the dialog: {values}");
    assert!(h.get_by_role(Role::PasswordInput).value().is_some_and(|v| !v.is_empty()), "the field shows that a password is set");
    assert!(h.query_by_label("Show password").is_none(), "the current password can't be revealed");

    // OK on the untouched field keeps the password as it is.
    ok(&mut h);
    assert_eq!(dialog(&h), None);
    assert_eq!(password(&h).as_deref(), Some(PW));
}

#[test]
fn ui_inspect_leaves_out_the_typed_passwords() {
    let mut h = harness();
    protect_menu(&mut h, "Encrypt with Password");
    set(&mut h, "password", PW);
    ok(&mut h);
    assert_eq!(dialog(&h).as_deref(), Some("confirmPassword"));
    set(&mut h, "confirmPassword", PW);
    let inspected = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx).to_string();
    assert!(inspected.contains("confirmPassword"), "the dialog is listed: {inspected}");
    assert!(!inspected.contains(PW), "ui.inspect shows no password: {inspected}");
    ok(&mut h);
    assert_eq!(password(&h).as_deref(), Some(PW));
}

#[test]
fn esc_on_the_mismatch_message_leaves_confirm_password_open() {
    let mut h = harness();
    protect_menu(&mut h, "Encrypt with Password");
    set(&mut h, "password", PW);
    ok(&mut h);
    set(&mut h, "confirmPassword", "something else");
    ok(&mut h);
    assert!(h.state().message.is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().message.is_none(), "Esc closes the message");
    assert_eq!(dialog(&h).as_deref(), Some("confirmPassword"), "and only the message");
}

#[test]
fn typing_over_the_stand_in_starts_a_new_password() {
    let mut h = harness();
    h.state_mut().run("file.encryptWithPassword", json!({"password": PW})).unwrap();
    protect_menu(&mut h, "Encrypt with Password");
    h.get_by_role(Role::PasswordInput).click();
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Text("x".into()));
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("password"), Some(&json!("x")), "the first key replaces the stand-in");
}
