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

#[test]
fn cell_text_y_moves_monotonically_in_standard_and_tall_rows() {
    let rect_std = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 20.0));
    let gh = 18.5;
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

    // Overflow case: gh exceeds rect height; Top still starts inside the cell.
    let gh_overflow = 60.0;
    let y_top_ovf = wrapped_cell_text_y(VAlign::Top, rect, gh_overflow, z);
    assert!(y_top_ovf >= rect.top());
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
    assert!((y_top - 1.5).abs() < 0.01, "Top starts at the top edge, got {y_top}");
    assert!((y_center + gh / 2.0 - rect.center().y).abs() < 0.01, "Center is centred, got {y_center}");
    assert!((y_bottom + gh - (rect.bottom() - 1.5)).abs() < 0.01, "Bottom ends at the bottom edge, got {y_bottom}");
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
