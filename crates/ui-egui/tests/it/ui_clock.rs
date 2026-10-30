use gridcraft_engine::core::date::unix_now_ms;

#[test]
fn ui_clock_returns_current_unix_milliseconds() {
    let before = unix_now_ms().unwrap();
    let now = gridcraft_ui_egui::now_ms();
    let after = unix_now_ms().unwrap();
    assert!(now.is_finite() && now > 0.0);
    assert!((before.min(after)..=before.max(after)).contains(&now));
}
