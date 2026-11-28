//! Increase/Decrease Font Size step through the Font Size list from 8 to 72 and go no further, as
//! in Excel (Decrease jumped from 8 straight to 1, Increase went on past 72 to 96 and 409).

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
fn decrease_stops_at_8_and_increase_climbs_back() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    assert_eq!(size(&s), 11.0, "the default font size");
    let down: Vec<f32> = (0..5).map(|_| step(&mut s, false)).collect();
    assert_eq!(down, [10.0, 9.0, 8.0, 8.0, 8.0], "Decrease stops at 8, as in Excel");
    let up: Vec<f32> = (0..4).map(|_| step(&mut s, true)).collect();
    assert_eq!(up, [9.0, 10.0, 11.0, 12.0]);
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
        [(10.5, true, 11.0), (10.5, false, 10.0), (7.5, false, 7.5), (7.5, true, 8.0), (1.0, true, 8.0), (100.0, false, 72.0), (100.0, true, 100.0)]
    {
        set(&mut s, from);
        assert_eq!(step(&mut s, up), to, "{from} {}", if up { "up" } else { "down" });
    }
}

/// The active cell's size picks the next one for the whole selection, as in Excel: 11 pt active
/// makes every selected cell 12. At the end of the steps nothing changes, other cells included
/// (with the active cell at 72 pt, an 11 pt cell used to become 72).
#[test]
fn a_selection_follows_the_active_cell_and_stays_put_at_either_end() {
    let mut s = gridcraft_engine::Session::new();
    s.new_workbook();
    let sizes = |s: &gridcraft_engine::Session| -> Vec<f32> {
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        ["A1", "B1"].iter().map(|a| d.wb.styles.get(sh.style_id(gridcraft_engine::core::CellRef::parse(a).unwrap())).font.size).collect()
    };
    s.execute("home.fontSize", json!({"range": "A1", "size": 72})).unwrap();
    s.execute("selection.set", json!({"range": "A1:B1"})).unwrap();
    s.execute("home.increaseFontSize", json!({})).unwrap();
    assert_eq!(sizes(&s), [72.0, 11.0], "at 72 Increase changes nothing");
    s.execute("home.fontSize", json!({"range": "A1", "size": 8})).unwrap();
    s.execute("home.decreaseFontSize", json!({})).unwrap();
    assert_eq!(sizes(&s), [8.0, 11.0], "at 8 Decrease changes nothing");
    s.execute("selection.set", json!({"range": "A1:B1", "active": "B1"})).unwrap();
    s.execute("home.increaseFontSize", json!({})).unwrap();
    assert_eq!(sizes(&s), [12.0, 12.0], "the active cell's next size for every selected cell");
}
