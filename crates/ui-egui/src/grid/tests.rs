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
        let (lk, _) = look(&wb, 0, sh, c, cf);
        display_text(&wb, sh, c, &sh.value(c), &lk.style, 200.0, 7.0).0
    };
    assert_eq!(text(0, &mut cf), "1");
    assert_eq!(text(1, &mut cf), "3.00");
    assert_eq!(text(2, &mut cf), "4.00");
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
    // With no unpainted descent, Top and Bottom share the same inset and Middle stays centred.
    assert!((y_top - (rect.top() + GLYPH_EDGE_INSET * z)).abs() < 0.01, "Top starts at the inset, got {y_top}");
    assert!((y_center + gh / 2.0 - rect.center().y).abs() < 0.01, "Center is centred, got {y_center}");
    assert!((y_bottom + gh - (rect.bottom() - GLYPH_EDGE_INSET * z)).abs() < 0.01, "Bottom ends one inset above the bottom edge, got {y_bottom}");
}

#[test]
fn bottom_align_keeps_the_same_painted_gap_as_top_align() {
    let rect = Rect::from_min_max(pos2(0.0, 10.0), pos2(100.0, 90.0));
    let (gh, z) = (18.0, 2.0);
    // Calibri leaves much more unpainted room under the baseline than above the capitals.
    let slack = GlyphSlack { above: 1.0, below: 6.0 };
    let top = cell_text_y_slack(VAlign::Top, rect, gh, z, slack);
    let bottom = cell_text_y_slack(VAlign::Bottom, rect, gh, z, slack);
    let center = cell_text_y_slack(VAlign::Center, rect, gh, z, slack);
    let ink_top_gap = top + slack.above - rect.top();
    let ink_bottom_gap = rect.bottom() - (bottom + gh - slack.below);
    assert!((ink_top_gap - ink_bottom_gap).abs() < 0.01, "painted gaps differ: top {ink_top_gap}, bottom {ink_bottom_gap}");
    assert!((ink_top_gap - (GLYPH_EDGE_INSET * z + slack.above)).abs() < 0.01, "{ink_top_gap}");
    let ink_mid = center + (slack.above + gh - slack.below) / 2.0;
    assert!((ink_mid - rect.center().y).abs() < 0.01, "painted middle is off centre, got {ink_mid}");
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

// 100% sheet zoom is one 96-dpi sheet pixel per egui point, on the grid and in Page Layout
// (both multiply by DISPLAY_SCALE). Three rows, top / middle / bottom, with a tall top row
// so the top inset is not the same as sitting on the edge.
#[test]
fn zoom_100_is_one_to_one_and_top_align_keeps_an_inset() {
    use gridcraft_engine::model::LineInfo;
    use gridcraft_engine::paper::PageGrid;
    let mut wb = Workbook::new();
    {
        let sh = wb.sheet_mut(0).unwrap();
        sh.zoom = 100;
        sh.cols.insert(1, LineInfo { size: Some(140.0), custom: true, ..LineInfo::default() });
        for (row, height) in [(1u32, 64.0), (3, 40.0), (5, 24.0)] {
            sh.rows.insert(row, LineInfo { size: Some(height), custom: true, ..LineInfo::default() });
        }
    }
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0));
    {
        let sh = wb.sheet(0).unwrap();
        let geo = Geo::new(sh, rect, egui::Vec2::ZERO);
        assert!((geo.z - 1.0).abs() < 1e-4, "100% must be 1 sheet pixel per point, got {}", geo.z);
        let tall = geo.cell_rect(sh, CellRef::new(1, 1));
        let mid_row = geo.cell_rect(sh, CellRef::new(3, 1));
        let short = geo.cell_rect(sh, CellRef::new(5, 1));
        assert!((tall.height() - 64.0).abs() < 0.05 && (tall.width() - 140.0).abs() < 0.05, "{tall:?}");
        assert!((mid_row.height() - 40.0).abs() < 0.05, "{mid_row:?}");
        assert!((short.height() - 24.0).abs() < 0.05, "{short:?}");
        let gh = 16.0;
        let inset = GLYPH_EDGE_INSET * geo.z;
        let top = cell_text_y(VAlign::Top, tall, gh, geo.z);
        let middle = cell_text_y(VAlign::Center, tall, gh, geo.z);
        let bottom = cell_text_y(VAlign::Bottom, tall, gh, geo.z);
        assert!((top - (tall.top() + inset)).abs() < 0.01, "top inset, got {top}");
        assert!((bottom - (tall.bottom() - gh - inset)).abs() < 0.01, "bottom inset, got {bottom}");
        assert!((middle + gh / 2.0 - tall.center().y).abs() < 0.01, "middle stays centred, got {middle}");
        assert!(top + 8.0 < middle && middle < bottom, "top stays at the top of a tall row");
        // The inset tracks the sheet zoom: twice as large at 200%, and at 50% it does not eat the row.
        let at_200 = cell_text_y(VAlign::Top, tall, gh, 2.0);
        assert!((at_200 - (tall.top() + GLYPH_EDGE_INSET * 2.0)).abs() < 0.01, "{at_200}");
        let half_rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(80.0, 20.0));
        let at_50 = cell_text_y(VAlign::Top, half_rect, gh, 0.5);
        assert!((at_50 - GLYPH_EDGE_INSET * 0.5).abs() < 0.01, "{at_50}");
        assert!(at_50 + gh < half_rect.bottom(), "50% inset must not eat the row");
        // A single line taller than the row is clamped to the top inset (`y_bottom.max(y_top)`).
        let gh_over = short.height() + 8.0;
        assert!(gh_over > short.height(), "the glyph must be taller than the row");
        let over_top = cell_text_y(VAlign::Top, short, gh_over, geo.z);
        let over_mid = cell_text_y(VAlign::Center, short, gh_over, geo.z);
        let over_bot = cell_text_y(VAlign::Bottom, short, gh_over, geo.z);
        let unclamped = short.bottom() - gh_over - inset;
        assert!(unclamped < short.top() + inset, "without the clamp the line would start above the top inset");
        assert!((over_top - (short.top() + inset)).abs() < 0.01, "tall glyph stays at the top inset, got {over_top}");
        assert!((over_bot - over_top).abs() < 0.01, "bottom cannot start above the top inset, got {over_bot}");
        assert!((over_mid - over_top).abs() < 0.01, "middle collapses onto the top inset, got {over_mid}");
        let wrapped_top = wrapped_cell_text_y(VAlign::Top, tall, gh, geo.z);
        let wrapped_mid = wrapped_cell_text_y(VAlign::Center, tall, gh, geo.z);
        let wrapped_bot = wrapped_cell_text_y(VAlign::Bottom, tall, gh, geo.z);
        assert!((wrapped_top - (tall.top() + 1.5)).abs() < 0.01, "wrapped top inset, got {wrapped_top}");
        assert!((wrapped_bot - (tall.bottom() - gh - 1.5)).abs() < 0.01, "wrapped bottom inset, got {wrapped_bot}");
        assert!((wrapped_mid + gh / 2.0 - tall.center().y).abs() < 0.01, "wrapped middle stays centred");
    }
    let page = PageGrid::build(&wb, 0, 0, 0).unwrap();
    // `PageGrid::scale` is the print scale (1.0 at 100%). `Geo::paged` multiplies the sheet
    // zoom by it, so comparing `paged.z` with `page.scale` only restates that formula.
    assert!((page.scale - 1.0).abs() < 1e-4, "100% print scale is 1:1, got {}", page.scale);
    let sh = wb.sheet(0).unwrap();
    let paged = Geo::paged(sh, rect, egui::Vec2::ZERO, std::sync::Arc::new(page));
    assert!((paged.z - 1.0).abs() < 1e-4, "page layout at 100% sheet zoom is 1:1, got {}", paged.z);
    // Sheet zoom reaches `Geo::z` and the cell size. Passing 2.0 or 0.5 straight into
    // `cell_text_y` does not exercise that.
    for (percent, factor) in [(50u16, 0.5f32), (200, 2.0)] {
        if let Some(sh) = wb.sheet_mut(0) {
            sh.zoom = percent;
            let geo = Geo::new(sh, rect, egui::Vec2::ZERO);
            let cell = geo.cell_rect(sh, CellRef::new(1, 1));
            let mid_row = geo.cell_rect(sh, CellRef::new(3, 1));
            let short_row = geo.cell_rect(sh, CellRef::new(5, 1));
            assert!((geo.z - factor).abs() < 1e-4, "{percent}% must be {factor} sheet pixels per point, got {}", geo.z);
            assert!((cell.height() - 64.0 * factor).abs() < 0.05 && (cell.width() - 140.0 * factor).abs() < 0.05, "{percent}% {cell:?}");
            assert!((mid_row.height() - 40.0 * factor).abs() < 0.05, "{percent}% {mid_row:?}");
            assert!((short_row.height() - 24.0 * factor).abs() < 0.05, "{percent}% {short_row:?}");
        } else {
            assert!(wb.sheet(0).is_some(), "sheet 0");
        }
    }
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
