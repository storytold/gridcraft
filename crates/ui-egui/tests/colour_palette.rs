//! The colour palette's tints and layout follow Excel 365: white, black, very light and very dark
//! theme colours get their own tint steps, and No Fill sits below the swatches.

use egui::{Event, Key, Modifiers, os::OperatingSystem};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::model::style::apply_tint;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::widgets::theme_tints;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_os(OperatingSystem::Nix).with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

fn press(h: &mut Harness, key: Key) {
    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([ev(true), ev(false)]);
    h.run_steps(2);
}

fn open(h: &mut Harness, keys: &[Key]) {
    for &k in keys {
        press(h, k);
    }
    h.run_steps(3);
}

fn hex(rgb: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

#[test]
fn tints_match_excel_for_light_dark_and_mid_colours() {
    // Excel 365's swatches under each theme colour, top to bottom (read off its Fill Color menu).
    for (base, excel) in [
        (0xFFFFFFu32, ["F2F2F2", "D9D9D9", "BFBFBF", "A6A6A6", "808080"]),
        (0x000000, ["808080", "595959", "404040", "262626", "0D0D0D"]),
        (0xE8E8E8, ["D1D1D1", "AEAEAE", "747474", "3A3A3A", "171717"]),
        (0x0E2841, ["DCEAF7", "A6C9EC", "4E95D9", "215F9A", "153D64"]),
        (0xE97132, ["FBE2D5", "F7C7AC", "F2AA84", "BF4E14", "80350E"]),
    ] {
        let rgb = [(base >> 16) as u8, (base >> 8) as u8, base as u8];
        let tints = theme_tints(base);
        assert_eq!(tints[0], 0.0, "the first row is the colour itself");
        for (row, want) in excel.iter().enumerate() {
            let got = apply_tint(rgb, tints[row + 1]);
            let want = u32::from_str_radix(want, 16).unwrap_or(0);
            let want = [(want >> 16) as u8, (want >> 8) as u8, want as u8];
            let close = got.iter().zip(want).all(|(g, w)| (*g as i32 - w as i32).abs() <= 2);
            assert!(close, "{base:06X} row {}: {} vs Excel {}", row + 1, hex(got), hex(want));
        }
    }
}

#[test]
fn no_fill_sits_below_the_swatches_and_automatic_above() {
    let mut h = harness();
    // H, H: Fill Color. No Fill and More Colors… come after Standard Colors, as in Excel.
    open(&mut h, &[Key::F10, Key::H, Key::H]);
    let standard = h.get_by_label("Standard Colors").rect().top();
    assert!(h.get_by_label("No Fill").rect().top() > standard, "No Fill is below the swatches");
    assert!(h.get_by_label("More Colors…").rect().top() > h.get_by_label("No Fill").rect().top(), "More Colors… is last");
    press(&mut h, Key::Escape);
    press(&mut h, Key::Escape);
    // H, F, C: Font Color. Automatic comes first, above Theme Colors.
    open(&mut h, &[Key::F10, Key::H, Key::F, Key::C]);
    assert!(h.get_by_label("Automatic").rect().top() < h.get_by_label("Theme Colors").rect().top(), "Automatic is above the swatches");
}

#[test]
fn more_colors_expands_and_collapses_and_starts_collapsed() {
    let mut h = harness();
    open(&mut h, &[Key::F10, Key::H, Key::H]);
    assert!(h.query_by_label("Hex").is_none(), "the menu opens with More Colors… collapsed");
    h.get_by_label("More Colors…").click();
    h.run_steps(3);
    assert!(h.query_by_label("Hex").is_some(), "More Colors… expands the section");
    h.get_by_label("More Colors…").click();
    h.run_steps(3);
    assert!(h.query_by_label("Hex").is_none(), "clicking it again collapses it");
    h.get_by_label("More Colors…").click();
    h.run_steps(3);
    press(&mut h, Key::Escape);
    press(&mut h, Key::Escape);
    open(&mut h, &[Key::F10, Key::H, Key::H]);
    assert!(h.query_by_label("Theme Colors").is_some(), "the menu is open again");
    assert!(h.query_by_label("Hex").is_none(), "and More Colors… is collapsed again");
}
