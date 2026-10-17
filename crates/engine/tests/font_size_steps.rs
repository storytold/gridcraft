//! Increase/Decrease Font Size step by one point from 1 to 8, then through the Font Size list up
//! to 72, and go no further than 1 or 72 (Decrease jumped from 8 straight to 1).

use serde_json::json;

fn size(s: &gridcraft_engine::Session) -> f32 {
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(d.selection.active)).font.size
}

fn step(s: &mut gridcraft_engine::Session, up: bool) -> f32 {
    let id = if up { "home.increaseFontSize" } else { "home.decreaseFontSize" };
    s.execute(id, json!({})).unwrap();
    size(s)
}

fn set(s: &mut gridcraft_engine::Session, size: f32) {
    s.execute("home.fontSize", json!({"size": size})).unwrap();
}

#[test]
fn decrease_steps_down_to_one_by_one_point_and_increase_climbs_back() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    assert_eq!(size(&s), 11.0, "the default font size");
    let down: Vec<f32> = (0..11).map(|_| step(&mut s, false)).collect();
    assert_eq!(down, [10.0, 9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0, 1.0], "one point at a time below 8, stopping at 1");
    let up: Vec<f32> = (0..11).map(|_| step(&mut s, true)).collect();
    assert_eq!(up, [2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0], "back up one point at a time, then the list");
}

#[test]
fn increase_follows_the_font_size_list_and_stops_at_72() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    let up: Vec<f32> = (0..16).map(|_| step(&mut s, true)).collect();
    assert_eq!(up, [12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0, 72.0, 72.0, 72.0, 72.0]);
    let down: Vec<f32> = (0..6).map(|_| step(&mut s, false)).collect();
    assert_eq!(down, [48.0, 36.0, 28.0, 26.0, 24.0, 22.0]);
}

#[test]
fn sizes_off_the_list_step_to_the_neighbouring_size() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    for (from, up, to) in
        [(10.5, true, 11.0), (10.5, false, 10.0), (7.5, false, 7.0), (7.5, true, 8.0), (1.5, false, 1.0), (100.0, false, 72.0), (100.0, true, 100.0)]
    {
        set(&mut s, from);
        assert_eq!(step(&mut s, up), to, "{from} {}", if up { "up" } else { "down" });
    }
}
