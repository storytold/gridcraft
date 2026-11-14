#[test]
fn editor_matches_the_cell_while_the_text_fits() {
    use super::editor_width;
    assert_eq!(editor_width(10.0, 80.0, 7.0), 80.0);
    assert_eq!(editor_width(66.0, 80.0, 7.0), 80.0);
    assert_eq!(editor_width(67.0, 80.0, 7.0), 88.0);
}

#[test]
fn editor_slack_fits_one_more_glyph_at_high_zoom() {
    use super::editor_slack;
    // A glyph is at most about one em wide; at 400% an em is ~60 px, far over the old fixed 14 px.
    for size in [14.7_f32, 25.0, 60.0] {
        assert!(editor_slack(size) - 4.0 >= size, "slack {} too small for a {size} px glyph", editor_slack(size));
    }
}

#[test]
fn explicit_left_and_right_keep_their_side_on_a_right_to_left_sheet() {
    use super::{HAlign, painted_halign};
    // Painted before the mirror: swapped, so they land where the user put them.
    assert_eq!(painted_halign(HAlign::Right, true), HAlign::Left);
    assert_eq!(painted_halign(HAlign::Left, true), HAlign::Right);
    // Left-to-right sheets and the other alignments are painted as they are.
    assert_eq!(painted_halign(HAlign::Right, false), HAlign::Right);
    assert_eq!(painted_halign(HAlign::Left, false), HAlign::Left);
    for h in [HAlign::Center, HAlign::Justify, HAlign::Fill] {
        assert_eq!(painted_halign(h, true), h);
    }
}

#[test]
fn heading_labels_scale_with_the_zoom_without_a_cap() {
    use super::{DISPLAY_SCALE, heading_font_size};
    let at = |percent: f32| heading_font_size(percent / 100.0 * DISPLAY_SCALE);
    assert!((at(400.0) / at(100.0) - 4.0).abs() < 1e-4, "four times as large at 400%");
    assert!(at(200.0) > at(100.0) && at(400.0) > at(200.0));
    assert_eq!(at(10.0), 8.5, "never below a readable size");
}
use super::*;

#[test]
fn list_picker_fits_up_to_eight_rows_then_scrolls() {
    assert_eq!(list_picker_height(5, 18.0, 3.0), 5.0 * 18.0 + 4.0 * 3.0);
    assert_eq!(list_picker_height(8, 18.0, 3.0), 8.0 * 18.0 + 7.0 * 3.0);
    assert_eq!(list_picker_height(30, 18.0, 3.0), list_picker_height(8, 18.0, 3.0));
    assert_eq!(list_picker_height(0, 18.0, 3.0), 18.0);
}

#[test]
fn number_format_colors_resolve_named_and_indexed_rgb() {
    use gridcraft_engine::numfmt::FormatColor as F;
    assert_eq!(numfmt_color(F::Green), Color32::from_rgb(0, 255, 0));
    assert_eq!(numfmt_color(F::Indexed(1)), Color32::BLACK);
    assert_eq!(numfmt_color(F::Indexed(10)), Color32::from_rgb(0, 128, 0));
    assert_eq!(numfmt_color(F::Indexed(46)), Color32::from_rgb(255, 102, 0));
    assert_eq!(numfmt_color(F::Indexed(56)), Color32::from_rgb(51, 51, 51));
    for index in [0, 57, 255] {
        assert_eq!(numfmt_color(F::Indexed(index)), Color32::BLACK);
    }
}

#[test]
fn wheel_zoom_steps_along_the_ladder_and_stops_at_100() {
    for (from, zoom_in, to) in [
        (100, true, 120),
        (120, false, 100),
        (100, false, 85),
        // Between steps, the next one each way (Excel: 112% goes to 115% or 100%).
        (112, true, 120),
        (112, false, 100),
        // A step less than 5% away is skipped…
        (121, false, 100),
        (86, false, 70),
        (139, true, 170),
        // …but never 100% or a limit.
        (99, true, 100),
        (101, false, 100),
        (96, true, 100),
        (385, true, 400),
        (11, false, 10),
        (400, true, 400),
        (10, false, 10),
        // Out-of-range zooms (from a file) come into range first.
        (5, true, 12),
        (1000, false, 340),
        (0, false, 10),
        (u16::MAX, true, 400),
    ] {
        assert_eq!(wheel_zoom_step(from, zoom_in), to, "{from}% {}", if zoom_in { "in" } else { "out" });
    }
    // Every step on the ladder is 15–25% from the next, and a notch in then out returns.
    // (The glide between steps is `zoom_glide_moves_by_ratio_and_always_arrives`.)
    for w in WHEEL_ZOOM_LADDER.windows(2) {
        let ratio = f32::from(w[1]) / f32::from(w[0]);
        assert!((1.15..=1.25).contains(&ratio), "{w:?}");
        assert_eq!(wheel_zoom_step(w[0], true), w[1]);
        assert_eq!(wheel_zoom_step(w[1], false), w[0]);
    }
}

#[test]
fn zoom_glide_moves_by_ratio_and_always_arrives() {
    // A third of the way per frame, as at 60 frames a second.
    for (from, to) in [(100, 120), (120, 100), (10, 12), (340, 400), (400, 10), (85, 100)] {
        let (mut z, mut frames) = (from, 0);
        while z != to {
            let next = zoom_glide_step(z, to, 0.32);
            assert!(next != z && next.abs_diff(to) < z.abs_diff(to), "{from} → {to}: {z} then {next}");
            z = next;
            frames += 1;
            assert!(frames < 40, "{from} → {to} arrives");
        }
        assert!(frames >= 2, "{from} → {to} glides over more than one frame ({frames})");
    }
    assert_eq!(zoom_glide_step(100, 120, 0.32), 106, "a third of the way by ratio");
    assert_eq!(zoom_glide_step(120, 100, 0.32), 113);
    // No progress asked or nonsense: still a percent closer, or straight there.
    assert_eq!(zoom_glide_step(100, 120, 0.0), 101);
    assert_eq!(zoom_glide_step(120, 100, f32::NAN), 100);
    assert_eq!(zoom_glide_step(100, 120, 5.0), 120);
    assert_eq!(zoom_glide_step(120, 120, 0.5), 120);
    assert_eq!(zoom_glide_step(0, 10, 0.5), 3);
}

#[test]
fn center_across_run_stops_at_first_non_continuing_column() {
    assert_eq!(center_across_end(0, 20, |c| c <= 2), 2);
    assert_eq!(center_across_end(0, 20, |_| false), 0);
    assert_eq!(center_across_end(0, 1, |_| true), 1);
}

#[test]
fn conditional_number_format_reaches_the_cell_text() {
    // #1163: a rule's number format replaces the cell's own; unmatched cells keep theirs.
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [[1], [3], [4]]})).unwrap();
    session.execute("home.numberFormat", json!({"range": "A3", "code": "0.0"})).unwrap();
    let rule = json!({"type": "cellIs", "operator": "greater", "value": "2", "style": {"num_fmt": "0.00"}});
    session.execute("home.conditionalFormat", json!({"range": "A1:A3", "rule": rule})).unwrap();
    let wb = session.active().unwrap().wb.clone();
    let sh = wb.sheet(0).unwrap();
    let mut cf = CfCache::new();
    let text = |row: u32, cf: &mut CfCache| {
        let c = CellRef::new(row, 0);
        let (lk, _) = look(&wb, 0, sh, c, cf, false);
        display_text(&wb, sh, c, &sh.value(c), &lk.style, 200.0, 7.0).0
    };
    assert_eq!(text(0, &mut cf), "1");
    assert_eq!(text(1, &mut cf), "3.00");
    assert_eq!(text(2, &mut cf), "4.00");
}

#[test]
fn cell_colours_are_authored_on_the_light_sheet_and_mapped_on_the_dark_one() {
    use crate::theme::{INK_TEXT_MIN, contrast, ink};
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1:A5", "values": [["auto"], ["on yellow"], ["blue"], ["heading"], [-5]]})).unwrap();
    session.execute("home.fillColor", json!({"range": "A2", "color": "#FFFF00"})).unwrap();
    session.execute("home.fontColor", json!({"range": "A3", "color": "#0000FF"})).unwrap();
    session.execute("home.fontColor", json!({"range": "A4", "color": "#1F4E79"})).unwrap();
    session.execute("home.fillColor", json!({"range": "A4", "color": "#DDEBF7"})).unwrap();
    session.execute("home.numberFormat", json!({"range": "A5", "code": "0;[Red]-0"})).unwrap();
    let wb = session.active().unwrap().wb.clone();
    let sh = wb.sheet(0).unwrap();
    let mut cf = CfCache::new();
    let mut at = |row: u32, dark: bool| look(&wb, 0, sh, CellRef::new(row, 0), &mut cf, dark).0;
    let rgb = |hex: u32| Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let sheet = ink(Color32::WHITE);

    // Light sheet: exactly as authored (today's look).
    assert_eq!((at(0, false).text_color, at(0, false).fill), (Color32::BLACK, None));
    assert_eq!((at(1, false).text_color, at(1, false).fill), (Color32::BLACK, Some(rgb(0xFFFF00))));
    assert_eq!(at(2, false).text_color, rgb(0x0000FF));
    assert_eq!((at(3, false).text_color, at(3, false).fill), (rgb(0x1F4E79), Some(rgb(0xDDEBF7))));

    // Dark sheet: automatic text is white on the sheet.
    assert_eq!((at(0, true).text_color, at(0, true).fill), (Color32::WHITE, None));
    // Automatic text on a pure yellow fill: the fill keeps its hue, the text is guarded dark.
    let y = at(1, true);
    assert_eq!(y.fill, Some(ink(rgb(0xFFFF00))));
    assert!(contrast(y.text_color, ink(rgb(0xFFFF00))) >= INK_TEXT_MIN, "{:?}", y.text_color);
    // Explicit blue text is lifted to 4.5:1 on the sheet.
    assert!(contrast(at(2, true).text_color, sheet) >= INK_TEXT_MIN);
    // A dark heading on a pale fill becomes light on deep: the design note's pair.
    let h = at(3, true);
    assert_eq!((h.text_color, h.fill), (rgb(0x9EC3E6), Some(rgb(0x39444E))));
    // A number format's [Red] goes through the same guard as the cell's text.
    let n = at(4, true);
    let red = n.text_ink(rgb(0xFF0000));
    assert_ne!(red, rgb(0xFF0000));
    assert!(contrast(red, sheet) >= contrast(rgb(0xFF0000), Color32::WHITE) - 0.01);
    assert_eq!(at(4, false).text_ink(rgb(0xFF0000)), rgb(0xFF0000));
    // Automatic borders: black on white, white on the dark sheet.
    assert_eq!((n.line_ink(Color32::BLACK), at(4, false).line_ink(Color32::BLACK)), (Color32::WHITE, Color32::BLACK));
}

#[test]
fn pattern_fill_text_is_guarded_against_the_blend_actually_painted() {
    use crate::theme::{INK_TEXT_MIN, contrast};
    use gridcraft_engine::model::{Color, PatternType};
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "pattern"})).unwrap();
    let mut wb = (*session.active().unwrap().wb).clone();
    let mut style = wb.styles.get(wb.sheet(0).unwrap().style_id(CellRef::new(0, 0))).clone();
    // Red on green: ink() of the authored blend is far lighter than the blend of the mapped colours.
    style.fill.pattern = PatternType::Gray25;
    style.fill.fg = Color::Rgb(0xFF0000);
    style.fill.bg = Color::Rgb(0x00FF00);
    let id = wb.styles.intern(style);
    wb.sheet_mut(0).unwrap().set_style(CellRef::new(0, 0), id);
    let sh = wb.sheet(0).unwrap();
    let lk = look(&wb, 0, sh, CellRef::new(0, 0), &mut CfCache::new(), true).0;
    let painted = lk.fill.unwrap();
    assert_eq!(lk.behind, painted);
    let target = INK_TEXT_MIN.min(contrast(Color32::BLACK, lk.paper));
    assert!(contrast(lk.text_color, painted) >= target - 0.05, "{:?} on {painted:?}", lk.text_color);
    // The light sheet is untouched.
    let light = look(&wb, 0, sh, CellRef::new(0, 0), &mut CfCache::new(), false).0;
    assert_eq!(light.text_color, Color32::BLACK);
}

#[test]
fn filter_menu_uses_the_table_whose_header_was_clicked() {
    use gridcraft_engine::model::Table;
    let table = |id: u32, r0: u32, r1: u32| Table {
        id,
        name: format!("T{id}"),
        range: RangeRef::new(CellRef::new(r0, 0), CellRef::new(r1, 0)),
        header_row: true,
        totals_row: false,
        columns: vec![],
        style: String::new(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    };
    let mut sh = Sheet::new("S");
    sh.tables.push(table(1, 0, 2));
    sh.tables.push(table(2, 4, 7));
    assert_eq!(filter_table_range(&sh, 0, 4), Some(sh.tables[1].range));
    assert_eq!(filter_table_range(&sh, 0, 0), Some(sh.tables[0].range));
    assert_eq!(filter_table_range(&sh, 1, 4), None);
}

/// The `("Label", "id")` keys written in `src`, without separators.
fn menu_labels(src: &str) -> Vec<&str> {
    src.split("(\"").skip(1).filter_map(|rest| rest.split_once("\", \"").map(|(label, _)| label)).filter(|label| *label != "-").collect()
}

/// The source of `menus.rs` from `start` to the first `end` after it.
fn menu_source<'a>(src: &'a str, start: &str, end: &str) -> &'a str {
    let from = src.find(start).unwrap_or_else(|| panic!("{start:?} not in menus.rs"));
    let rest = &src[from..];
    &rest[..rest.find(end).unwrap_or_else(|| panic!("{end:?} not after {start:?}"))]
}

/// German read "Einfügen" for Paste and "Einfügen…"/"Einfügen" for Insert in the same menu
/// (#2874): no two items of the cell, row header or column header menu may read alike, ignoring
/// case and a trailing ellipsis. The items are read from `menus.rs`, so new ones are checked too.
#[test]
fn german_context_menus_have_no_two_items_alike() {
    use crate::i18n::Language;
    let src = include_str!("menus.rs");
    let cell = menu_labels(menu_source(src, "fn context_menu(", "let lang ="));
    let (rows, columns) =
        menu_source(src, "fn header_menu(", "let lang =").split_once("} else {").expect("header menu has a rows and a columns list");
    let insert_copied = menu_labels(menu_source(src, "fn insert_copied_item(", "\n}"));
    assert_eq!(insert_copied.len(), 3, "{insert_copied:?}");
    for (menu, keys) in [("cell", cell), ("row header", menu_labels(rows)), ("column header", menu_labels(columns))] {
        assert!(keys.contains(&"Paste") && keys.len() > 8, "{menu} menu not read from menus.rs: {keys:?}");
        // A menu holds at most one Insert Copied/Cut Cells entry.
        for extra in &insert_copied {
            let mut seen: Vec<(String, &str)> = Vec::new();
            for key in keys.iter().chain([extra]) {
                let shown = Language::De.tr(key).trim_end_matches('…').trim().to_lowercase();
                if let Some((_, other)) = seen.iter().find(|(text, _)| *text == shown) {
                    panic!("German {menu} menu shows {key:?} and {other:?} both as {shown:?}");
                }
                seen.push((shown, key));
            }
        }
    }
    assert_eq!(Language::De.tr("Insert…"), "Zellen einfügen…");
    assert_eq!(Language::De.tr("Header Menu|Insert"), "Zellen einfügen");
    // Only German changed: the header menu's Insert reads as the plain Insert everywhere else.
    for language in Language::ALL.into_iter().filter(|l| *l != Language::De) {
        assert_eq!(language.tr("Header Menu|Insert"), language.tr("Insert"), "{}", language.code());
    }
}

#[test]
fn galley_baseline_bounds_preserves_empty_edge_rows() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    for text in ["\nHello", "Hello\n", "\n\nHello\n\n", "\n\n"] {
        let galley = ctx.fonts_mut(|fonts| fonts.layout(text.to_string(), egui::FontId::proportional(16.0), Color32::BLACK, 200.0));
        assert_eq!(galley.rows.len(), text.matches('\n').count() + 1);
        let (top, bottom) = super::cell_paint::galley_baseline_bounds(&galley);
        let first = galley.rows.first().expect("layout has a first row");
        let last = galley.rows.last().expect("layout has a last row");
        if first.glyphs.is_empty() {
            assert_eq!(top, first.rect().top(), "leading blank rows contribute to the top bound: {text:?}");
        }
        if last.glyphs.is_empty() {
            assert_eq!(bottom, last.rect().bottom(), "trailing blank rows contribute to the bottom bound: {text:?}");
        }
    }
}

#[test]
fn galley_baseline_bounds_ignores_centered_fallback_glyphs() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    for text in ["\u{e618}Hello", "Hello\n\u{e618}Hello", "\u{e618}Hello\nHello", "😀Hello", "Hello\n😀Hello", "😀Hello\nHello"] {
        let galley = ctx.fonts_mut(|fonts| fonts.layout(text.to_string(), egui::FontId::monospace(16.0), Color32::BLACK, 200.0));
        let fallback =
            galley.rows.iter().flat_map(|row| &row.glyphs).find(|glyph| matches!(glyph.chr, '\u{e618}' | '😀')).expect("fallback is laid out");
        assert_eq!(fallback.font_face_align, egui::epaint::text::FallbackAlign::Center);
        assert_ne!(fallback.font_face_ascent, fallback.font_ascent, "test must exercise a fallback with different metrics");
        let (top, bottom) = super::cell_paint::galley_baseline_bounds(&galley);
        for (row, bound, lower) in [(galley.rows.first().unwrap(), top, false), (galley.rows.last().unwrap(), bottom, true)] {
            let primary = row.glyphs.iter().find(|glyph| glyph.chr == 'H').expect("primary-font text is present");
            let expected = row.pos.y + primary.pos.y - primary.font_ascent + if lower { primary.font_height } else { 0.0 };
            assert_eq!(bound, expected, "a centered fallback must not shift the primary text: {text:?}");
        }
    }
}

#[test]
fn galley_baseline_bounds_keeps_centered_only_rows() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |_| {});
    output.textures_delta.clear();
    for text in ["\u{e618}", "\u{e618}\n\u{e618}", "😀", "😀\n😀"] {
        let galley = ctx.fonts_mut(|fonts| fonts.layout(text.to_string(), egui::FontId::monospace(16.0), Color32::BLACK, 200.0));
        let (top, bottom) = super::cell_paint::galley_baseline_bounds(&galley);
        assert_eq!(top, galley.rows.first().unwrap().rect().top(), "centered-only rows keep their logical top");
        assert_eq!(bottom, galley.rows.last().unwrap().rect().bottom(), "centered-only rows keep their logical bottom");
    }
}

#[test]
fn cell_text_y_moves_monotonically_when_the_text_fits() {
    let rect_std = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 20.0));
    let gh = 16.0;
    let z = 1.0;

    let y_top_std = cell_text_y(VAlign::Top, rect_std, gh, z);
    let y_center_std = cell_text_y(VAlign::Center, rect_std, gh, z);
    let y_bottom_std = cell_text_y(VAlign::Bottom, rect_std, gh, z);

    assert!(y_top_std < y_center_std, "Top ({y_top_std}) must be higher than Center ({y_center_std})");
    assert!(y_center_std < y_bottom_std, "Center ({y_center_std}) must be higher than Bottom ({y_bottom_std})");

    // Verify tall row (e.g. 40px)
    let rect_tall = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 40.0));
    let y_top_tall = cell_text_y(VAlign::Top, rect_tall, gh, z);
    let y_center_tall = cell_text_y(VAlign::Center, rect_tall, gh, z);
    let y_bottom_tall = cell_text_y(VAlign::Bottom, rect_tall, gh, z);

    assert!(y_top_tall < y_center_tall, "Top ({y_top_tall}) must be higher than Center ({y_center_tall})");
    assert!(y_center_tall < y_bottom_tall, "Center ({y_center_tall}) must be higher than Bottom ({y_bottom_tall})");
}

#[test]
fn wrapped_cell_text_y_positions_correctly_and_safeguards_overflow() {
    let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 40.0));
    let gh = 30.0;
    let z = 1.0;

    let y_top = wrapped_cell_text_y(VAlign::Top, rect, gh, z);
    let y_center = wrapped_cell_text_y(VAlign::Center, rect, gh, z);
    let y_bottom = wrapped_cell_text_y(VAlign::Bottom, rect, gh, z);

    assert!(y_top < y_center);
    assert!(y_center < y_bottom);

    // Overflow keeps the same top inset as fitting text; the cell clips the excess.
    let gh_overflow = 60.0;
    let y_top_ovf = wrapped_cell_text_y(VAlign::Top, rect, gh_overflow, z);
    assert_eq!(y_top_ovf, y_top);
}

#[test]
fn wrapped_cell_text_y_keeps_vertical_alignment_when_text_is_taller_than_the_row() {
    // A default-height row holding three wrapped lines.
    let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 20.0));
    let (gh, z) = (60.0, 1.0);
    let y_top = wrapped_cell_text_y(VAlign::Top, rect, gh, z);
    let y_center = wrapped_cell_text_y(VAlign::Center, rect, gh, z);
    let y_bottom = wrapped_cell_text_y(VAlign::Bottom, rect, gh, z);
    // Taller text overflows upward for Bottom and both ways for Center (the cell clips it).
    assert!((y_top - 1.5 * z).abs() < 0.01, "Top leaves space inside the cell, got {y_top}");
    assert!((y_center + gh / 2.0 - (rect.center().y + 0.5 * z)).abs() < 0.01, "Center follows the shared insets, got {y_center}");
    assert!((y_bottom + gh - (rect.bottom() - 0.5 * z)).abs() < 0.01, "Bottom keeps the single-line inset, got {y_bottom}");
}

/// The screen box of a galley of `size` drawn at `pos` turned by `angle` about `pos`, as egui
/// draws a rotated `TextShape`.
fn rotated_box(pos: Pos2, size: egui::Vec2, angle: f32) -> Rect {
    let rot = egui::emath::Rot2::from_angle(angle);
    let pts = [vec2(0.0, 0.0), vec2(size.x, 0.0), vec2(0.0, size.y), size].map(|v| pos + rot * v);
    Rect::from_points(&pts)
}

#[test]
fn rotated_text_box_follows_the_cell_alignment() {
    // A tall, narrow merged block (one 3-character column over four 60pt rows).
    let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(40.0, 340.0));
    let size = vec2(90.0, 15.0);
    let pad = 2.5;
    let near = |a: f32, b: f32| (a - b).abs() < 0.01;
    for deg in [90i16, -90, 45, -45] {
        let angle = -(deg as f32).to_radians();
        for v in [VAlign::Top, VAlign::Center, VAlign::Bottom] {
            for h in [HAlign::Left, HAlign::Center, HAlign::Right] {
                let bb = rotated_box(rotated_text_pos(rect, size, angle, h, v, pad, 0.0), size, angle);
                let what = format!("{deg}° {h:?}/{v:?}: {bb:?} in {rect:?}");
                assert!(near(bb.height(), rotated_box(Pos2::ZERO, size, angle).height()), "{what}");
                assert!(bb.top() >= rect.top() && bb.bottom() <= rect.bottom(), "{what}");
                // A narrow column can't hold a 45° line across: only check where it fits.
                let fits_x = bb.width() <= rect.width() - 2.0 * pad;
                match h {
                    HAlign::Left => assert!(near(bb.left(), rect.left() + pad), "{what}"),
                    HAlign::Right => assert!(near(bb.right(), rect.right() - pad), "{what}"),
                    _ => assert!(near(bb.center().x, rect.center().x), "{what}"),
                }
                if fits_x {
                    assert!(bb.left() >= rect.left() && bb.right() <= rect.right(), "{what}");
                }
                match v {
                    VAlign::Top => assert!(near(bb.top(), rect.top() + pad), "{what}"),
                    VAlign::Center => assert!(near(bb.center().y, rect.center().y), "{what}"),
                    // Bottom is the default: downward text keeps starting at the top, as before.
                    _ if deg < 0 => assert!(near(bb.top(), rect.top() + pad), "{what}"),
                    _ => assert!(near(bb.bottom(), rect.bottom() - pad), "{what}"),
                }
            }
        }
    }
}

#[test]
fn upright_text_with_default_alignment_keeps_its_place() {
    let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(40.0, 340.0));
    let size = vec2(90.0, 15.0);
    let pad = 2.5;
    let near = |a: Pos2, b: Pos2| (a - b).length() < 0.01;
    // Upward text starts at the bottom-left corner, downward text one block-height in from the top-left.
    let up = rotated_text_pos(rect, size, -std::f32::consts::FRAC_PI_2, HAlign::General, VAlign::Bottom, pad, 0.0);
    assert!(near(up, pos2(rect.left() + pad, rect.bottom() - pad)), "{up:?}");
    let down = rotated_text_pos(rect, size, std::f32::consts::FRAC_PI_2, HAlign::General, VAlign::Bottom, pad, 0.0);
    assert!(near(down, pos2(rect.left() + pad + size.y, rect.top() + pad)), "{down:?}");
}

#[test]
fn upward_text_with_middle_align_is_centred_in_a_merged_block() {
    // Issue #435: "Licht Strom (?)" turned up in A1:A4 with Middle Align was drawn at the bottom.
    let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(40.0, 340.0));
    let size = vec2(90.0, 15.0);
    let angle = -std::f32::consts::FRAC_PI_2;
    let bb = rotated_box(rotated_text_pos(rect, size, angle, HAlign::Center, VAlign::Center, 2.5, 0.0), size, angle);
    assert!((bb.center().y - rect.center().y).abs() < 0.01, "{bb:?} not centred in {rect:?}");
    assert!((bb.center().x - rect.center().x).abs() < 0.01, "{bb:?} not centred in {rect:?}");
}

/// One grid frame of `app` in a headless context.
fn grid_frame(app: &mut crate::SheetApp) {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 800.0));
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| super::show(app, ui));
    out.textures_delta.clear(); // nothing consumes the uploads in a headless test
}

/// The line positions, used range and conditional formatting statistics the grid keeps between
/// frames are rebuilt after every change of the sheet, whichever way it is changed, and match
/// what a fresh build gives.
#[test]
fn kept_sheet_geometry_and_cf_statistics_follow_every_change() {
    use gridcraft_engine::cf::CfCache;
    use gridcraft_engine::core::CellRef;
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    let mut app = crate::SheetApp::new(session, Default::default());
    app.run("range.setValues", json!({"range": "A1:A3", "values": [[1], [2], [3]]})).unwrap();
    app.run("home.conditionalFormat", json!({"range": "A:A", "rule": {"type": "colorScale", "colors": ["#FFFFFF", "#FF0000"]}})).unwrap();
    let kept = |app: &crate::SheetApp| app.grid.lines.as_ref().map(|(_, l)| (Arc::clone(&l.ri), l.used)).unwrap();
    // The kept statistics give the look a fresh cache gives.
    let check_cf = |app: &mut crate::SheetApp| {
        let wb = app.session.active().unwrap().wb.clone();
        for row in 0..4 {
            let c = CellRef::new(row, 0);
            let kept = app.grid.cf.as_mut().map(|(_, cf)| cf.look(&wb, 0, c));
            assert_eq!(kept, Some(CfCache::new().look(&wb, 0, c)), "A{}", row + 1);
        }
    };
    grid_frame(&mut app);
    let (ri, used) = kept(&app);
    assert_eq!(used.map(|r| r.a1()).as_deref(), Some("A1:A3"));
    check_cf(&mut app);
    grid_frame(&mut app);
    assert!(Arc::ptr_eq(&kept(&app).0, &ri), "an unchanged sheet keeps its line positions");

    // A command: the new value moves the colour scale's maximum and the used range.
    app.run("cell.set", json!({"cell": "A5", "input": "30"})).unwrap();
    grid_frame(&mut app);
    assert_eq!(kept(&app).1.map(|r| r.a1()).as_deref(), Some("A1:A5"));
    check_cf(&mut app);
    // Hiding a row moves the rows under it.
    app.run("selection.set", json!({"range": "A2"})).unwrap();
    app.run("home.hideRows", json!({})).unwrap();
    grid_frame(&mut app);
    let geo = app.grid.geo.clone().unwrap();
    let sh = app.session.active().unwrap().wb.sheet(0).unwrap().clone();
    assert_eq!(geo.y(&sh, 2), geo.y(&sh, 1), "row 2 is hidden");
    assert!(!Arc::ptr_eq(&kept(&app).0, &ri));
    // Undo brings the row back.
    app.run("edit.undo", json!({})).unwrap();
    grid_frame(&mut app);
    let geo = app.grid.geo.clone().unwrap();
    let sh = app.session.active().unwrap().wb.sheet(0).unwrap().clone();
    assert!(geo.y(&sh, 2) > geo.y(&sh, 1), "row 2 is shown again");
    let fresh = super::geo::SheetLines::of(&sh);
    for row in [0, 1, 2, 3, 100, 1_048_575] {
        assert_eq!(kept(&app).0.start(row), fresh.ri.start(row), "row {row}");
    }
    assert_eq!(kept(&app).1, fresh.used);

    // A change with nothing else holding the sheet (no undo step, no saved copy): the grid's weak
    // reference makes `Arc::make_mut` move the sheet to a new allocation, and the grid notices.
    let ri = kept(&app).0;
    {
        let d = app.session.active_mut().unwrap();
        d.undo.clear();
        d.redo.clear();
        d.saved = Arc::new(gridcraft_engine::model::Workbook::default());
        let sheet = Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap();
        sheet.rows.entry(0).or_default().hidden = true;
        sheet.rows.entry(1).or_default().hidden = true;
        sheet.set_value(CellRef::new(2, 0), gridcraft_engine::core::Value::Number(300.0));
    }
    grid_frame(&mut app);
    let geo = app.grid.geo.clone().unwrap();
    let sh = app.session.active().unwrap().wb.sheet(0).unwrap().clone();
    assert_eq!(geo.y(&sh, 2), geo.y(&sh, 0), "rows 1 and 2 are hidden");
    assert!(!Arc::ptr_eq(&kept(&app).0, &ri));
    check_cf(&mut app);
}
