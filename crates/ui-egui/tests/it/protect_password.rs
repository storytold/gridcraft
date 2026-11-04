//! Protect Sheet / Unprotect Sheet mask the password as it's typed (issue #347).

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

const SECRET: &str = "s3cret";

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::blank_harness(4)
}

fn type_password(h: &mut egui_kittest::Harness<'static, SheetApp>) {
    h.get_by_role(Role::PasswordInput).click();
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Text(SECRET.into()));
    h.run_steps(2);
}

fn secret_on_screen(h: &egui_kittest::Harness<'static, SheetApp>) -> bool {
    h.query_all_by(|n| n.value().is_some_and(|v| v.contains(SECRET)) || n.label().is_some_and(|l| l.contains(SECRET))).next().is_some()
}

#[test]
fn protect_and_unprotect_sheet_mask_the_password() {
    let mut h = harness();
    h.state_mut().open_dialog("protectSheet", json!({}));
    h.run_steps(3);
    type_password(&mut h);
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("password")), Some(&json!(SECRET)));
    assert!(!secret_on_screen(&h), "the typed password must not be readable");
    assert!(h.get_by_role(Role::PasswordInput).value().is_some_and(|v| !v.is_empty() && !v.contains(SECRET)));

    // Show password reveals it on purpose; unticking masks it again.
    h.get_by_label("Show password").click();
    h.run_steps(3);
    assert!(h.query_by_role(Role::PasswordInput).is_none());
    assert!(secret_on_screen(&h));
    h.get_by_label("Show password").click();
    h.run_steps(3);
    assert!(h.query_by_role(Role::PasswordInput).is_some());
    assert!(!secret_on_screen(&h));

    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert!(h.state().session.active().unwrap().wb.active().unwrap().protection.as_ref().is_some_and(|p| p.password.is_some()));

    // Unprotect asks for the password with a masked field too, and it starts hidden again.
    h.state_mut().open_dialog("unprotectSheet", json!({}));
    h.run_steps(3);
    type_password(&mut h);
    assert!(!secret_on_screen(&h), "the typed password must not be readable");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().wb.active().unwrap().protection.is_none());
}

#[test]
fn protect_sheet_entry_unprotects_a_protected_sheet() {
    // No password: the Protect Sheet entry points (Home > Format, HOP, RPS, tab menu) unprotect straight away.
    let mut h = harness();
    h.state_mut().run("review.protectSheet", json!({})).unwrap();
    assert!(h.state().active_sheet_protected());
    h.state_mut().open_dialog("protectSheet", json!({}));
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "no password: no dialog");
    assert!(!h.state().active_sheet_protected());

    // With a password the Unprotect dialog asks for it, and a wrong one leaves the sheet protected.
    h.state_mut().run("review.protectSheet", json!({"password": SECRET})).unwrap();
    h.state_mut().open_dialog("protectSheet", json!({}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("unprotectSheet"));
    assert!(h.state().active_sheet_protected());
}
