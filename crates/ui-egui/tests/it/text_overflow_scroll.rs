//! Overflowing text stays visible in its empty neighbours when its own cell is scrolled out (#1613).

use egui::{Rect, Shape};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const LEFT: &str = "This is a very long left-aligned text that overflows into many empty neighbouring cells";
const RIGHT: &str = "This is a very long right-aligned text that overflows into many empty neighbouring cells";

/// Build a blank sheet with deterministic fonts and keep the selection away from tested text.
fn harness(setup: impl FnOnce(&mut Session)) -> Harness {
    harness_with_size(egui::vec2(1200.0, 800.0), setup)
}

fn harness_with_size(size: egui::Vec2, setup: impl FnOnce(&mut Session)) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    setup(&mut s);
    s.execute("selection.set", json!({"range": "A20"})).unwrap();
    let mut h = crate::common::builder().with_size(size).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(s),
    );
    h.run_steps(4);
    h
}

/// Scroll so that `col` is the first column of the scrolled area.
fn scroll_to_col(h: &mut Harness, col: u32) {
    let app = h.state_mut();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let fc = sheet.freeze.map(|f| f.1).unwrap_or(0);
    let x = (sheet.col_left(col) - sheet.col_left(fc)) as f32;
    app.view_mut().unwrap().scroll.x = x;
    h.run_steps(4);
}

/// Clip rects of the painted text shapes whose text is `text`.
fn text_clips(h: &Harness, text: &str) -> Vec<Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == text => Some(s.clip_rect),
            _ => None,
        })
        .collect()
}

/// Locate a cell in the current scrolled pane for painted-clip assertions.
fn cell_rect(h: &Harness, cell: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap())
}

/// Unblocked left-aligned text remains visible after its source scrolls out of view.
#[test]
fn overflow_text_stays_visible_when_source_cell_scrolls_out_left() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "B1", "input": LEFT})).unwrap();
    });
    assert!(!text_clips(&h, LEFT).is_empty(), "B1 is drawn before scrolling");
    scroll_to_col(&mut h, 2);
    let c1 = cell_rect(&h, "C1");
    let clips = text_clips(&h, LEFT);
    assert!(clips.iter().any(|r| r.intersects(c1.shrink(1.0))), "B1's overflow is still drawn into C1 after scrolling: {clips:?}");
}

/// Unblocked right-aligned text can enter the view from an offscreen source.
#[test]
fn right_aligned_overflow_stays_visible_when_source_cell_is_right_of_view() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "Z1", "input": RIGHT})).unwrap();
        s.execute("home.formatCells", json!({"range": "Z1", "style": {"align": {"h": "Right"}}})).unwrap();
    });
    // Scroll so that Z1 starts just past the right edge of the cell area.
    {
        let app = h.state_mut();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        let x = sheet.col_left(25) as f32 - (geo.cells.width() - geo.frozen_w) / geo.z - 2.0;
        app.view_mut().unwrap().scroll.x = x;
    }
    h.run_steps(4);
    let (y1, z1, cells) = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        (geo.cell_rect(sheet, CellRef::parse("Y1").unwrap()), geo.cell_rect(sheet, CellRef::parse("Z1").unwrap()), geo.cells)
    };
    assert!(z1.left() >= cells.right(), "Z1 is out of view on the right ({z1:?} vs {cells:?})");
    let visible = y1.intersect(cells).shrink(1.0);
    let clips = text_clips(&h, RIGHT);
    assert!(clips.iter().any(|r| r.intersects(visible)), "Z1's overflow is drawn into Y1: {clips:?} vs {visible:?}");
}

/// Overflow from the scrolling pane must not paint over frozen columns.
#[test]
fn scrolled_overflow_does_not_draw_over_frozen_columns() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "C1", "input": LEFT})).unwrap();
        s.execute("view.freezePanes", json!({"cell": "B2"})).unwrap();
    });
    scroll_to_col(&mut h, 4);
    let a1 = cell_rect(&h, "A1");
    let e1 = cell_rect(&h, "E1");
    let clips = text_clips(&h, LEFT);
    assert!(clips.iter().any(|r| r.intersects(e1.shrink(1.0))), "C1's overflow reaches E1: {clips:?}");
    assert!(clips.iter().all(|r| !r.intersects(a1.shrink(1.0))), "C1's overflow stays out of frozen column A: {clips:?}");
}

/// Filled neighbours block both left- and right-aligned overflow in either theme.
#[test]
fn text_overflow_stops_at_explicit_fills_in_both_directions() {
    for dark in [false, true] {
        for (source, align) in [("B2", "Left"), ("D2", "Right")] {
            let mut h = harness(|s| {
                s.execute("cell.set", json!({"cell":source,"input":LEFT})).unwrap();
                s.execute("home.fillColor", json!({"range":source,"color":"#1F3864"})).unwrap();
                s.execute("home.formatCells", json!({"range":source,"style":{"align":{"h":align}}})).unwrap();
            });
            h.state_mut().ui.dark = dark;
            h.run_steps(4);
            let neighbor = cell_rect(&h, "C2").shrink(1.0);
            assert!(text_clips(&h, LEFT).iter().any(|r| r.intersects(neighbor)), "unfilled neighbour permits overflow");
            h.state_mut().run("home.fillColor", json!({"range":"C2","color":"#FFFF00"})).unwrap();
            h.run_steps(4);
            let clips = text_clips(&h, LEFT);
            assert!(!clips.is_empty(), "source text remains painted");
            assert!(clips.iter().all(|r| !r.intersects(neighbor)), "filled neighbour blocks {align} overflow, dark={dark}: {clips:?}");
        }
    }
}

/// Table, conditional and patterned fills block overflow even when the neighbour has no value.
#[test]
fn text_overflow_stops_at_resolved_fills() {
    for dark in [false, true] {
        for fill in ["conditional", "table", "pattern"] {
            let mut h = harness(|s| {
                s.execute("cell.set", json!({"cell":"B2","input":LEFT})).unwrap();
                s.execute("home.fillColor", json!({"range":"B2","color":"#1F3864"})).unwrap();
                match fill {
                    "conditional" => {
                        s.execute(
                            "home.conditionalFormat",
                            json!({"range":"C2","rule":{"type":"expression","formula":"=TRUE","style":{"fill":"#FFFF00"}}}),
                        )
                        .unwrap();
                    }
                    "table" => {
                        s.execute("cell.set", json!({"cell":"C1","input":"Header"})).unwrap();
                        s.execute("insert.table", json!({"range":"C1:C3","header":true})).unwrap();
                    }
                    "pattern" => {
                        s.execute(
                            "home.formatCells",
                            json!({"range":"C2","style":{"fill":{"pattern":"Gray50","fg":{"Rgb":0},"bg":{"Rgb":16777215}}}}),
                        )
                        .unwrap();
                    }
                    _ => unreachable!(),
                }
            });
            h.state_mut().ui.dark = dark;
            h.run_steps(4);
            let neighbor = cell_rect(&h, "C2").shrink(1.0);
            let clips = text_clips(&h, LEFT);
            assert!(!clips.is_empty(), "source text remains painted");
            assert!(clips.iter().all(|r| !r.intersects(neighbor)), "{fill} fill blocks overflow, dark={dark}: {clips:?}");
        }
    }
}

/// Center Across Selection ends before a filled cell instead of centring across its background.
#[test]
fn center_across_selection_stops_at_resolved_fill() {
    for dark in [false, true] {
        let mut h = harness(|s| {
            s.execute("cell.set", json!({"cell":"B2","input":"Centered"})).unwrap();
            s.execute("home.formatCells", json!({"range":"B2:E2","style":{"align":{"h":"CenterAcross"}}})).unwrap();
            s.execute("home.conditionalFormat", json!({"range":"D2","rule":{"type":"expression","formula":"=TRUE","style":{"fill":"#FFFF00"}}}))
                .unwrap();
        });
        h.state_mut().ui.dark = dark;
        h.run_steps(4);
        let d2 = cell_rect(&h, "D2");
        let clips = text_clips(&h, "Centered");
        assert!(!clips.is_empty(), "centred text remains painted");
        assert!(clips.iter().all(|r| r.right() <= d2.left() + 0.5), "centred span stops before filled D2, dark={dark}: {clips:?}");
        assert!(clips.iter().any(|r| (r.right() - d2.left()).abs() < 0.5), "unfilled C2 remains part of the span");
    }
}

/// An offscreen filled blocker prevents text farther outside the view from spilling past it.
#[test]
fn offscreen_filled_neighbor_blocks_text_from_an_outside_source() {
    for dark in [false, true] {
        let mut h = harness(|s| {
            s.execute("cell.set", json!({"cell":"B1","input":LEFT})).unwrap();
            s.execute("home.fillColor", json!({"range":"C1","color":"#FFFF00"})).unwrap();
        });
        h.state_mut().ui.dark = dark;
        scroll_to_col(&mut h, 3);
        let d1 = cell_rect(&h, "D1").shrink(1.0);
        assert!(text_clips(&h, LEFT).iter().all(|r| !r.intersects(d1)), "offscreen filled C1 blocks B1 before visible D1, dark={dark}");
    }
}

/// Fully fitting wrapped lines retain the whole cell clip, including
/// room for glyphs extending below their logical line boxes.
#[test]
fn wrapped_text_fitting_lines_keep_full_cell_clip() {
    const TEXT: &str = "Short wrapped text with two or three lines.";

    let h = harness(|s| {
        s.execute("home.columnWidth", json!({"cols": "B:B", "chars": 15})).unwrap();
        s.execute("cell.set", json!({"cell": "B3", "input": TEXT})).unwrap();
        s.execute("home.wrapText", json!({"range": "B3", "on": true})).unwrap();
        s.execute("home.formatCells", json!({"range": "B3", "style": {"align": {"v": "Top"}}})).unwrap();
        s.execute("home.rowHeight", json!({"rows": "3:3", "height": 120})).unwrap();
    });

    let cell = cell_rect(&h, "B3");

    let shapes: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(t) if t.galley.job.text == TEXT => Some((t, shape.clip_rect)),
            _ => None,
        })
        .collect();

    assert_eq!(shapes.len(), 1, "expected one wrapped text shape");

    let (text, clip) = shapes[0];

    assert!(text.galley.rows.len() >= 2, "text must wrap");
    assert!(text.galley.size().y < cell.height(), "all lines must fit");

    let last_bottom = text.pos.y + text.galley.rows.last().unwrap().rect().bottom();

    assert!(last_bottom < cell.bottom() - 1.0, "room must remain below the final line");

    assert!((clip.bottom() - cell.bottom()).abs() <= 0.5, "full cell clip must be retained: clip={clip:?}, cell={cell:?}");
}
/// Issue #2752: fixed-height wrapped cells must not paint
/// a fraction of a text line against the next row's border.
#[test]
fn wrapped_text_in_fixed_height_row_shows_only_complete_lines() {
    const TEXT: &str = "Row 3 target. This is intentionally long text that must wrap \
         across many lines within a narrow column. It must never \
         paint over the next row.";

    let h = harness(|s| {
        s.execute("home.columnWidth", json!({"cols": "B:B", "chars": 12})).unwrap();

        s.execute("cell.set", json!({"cell": "B3", "input": TEXT})).unwrap();

        s.execute("home.wrapText", json!({"range": "B3", "on": true})).unwrap();

        s.execute("home.formatCells", json!({"range": "B3", "style": {"align": {"v": "Top"}}})).unwrap();

        // Apply the custom height after Wrap Text so AutoFit
        // cannot expand the row for this scenario.
        s.execute("home.rowHeight", json!({"rows": "3:3", "height": 62.67})).unwrap();
    });

    let cell = cell_rect(&h, "B3");

    let shapes: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(t) if t.galley.job.text == TEXT => Some((t, shape.clip_rect)),
            _ => None,
        })
        .collect();

    assert_eq!(shapes.len(), 1, "expected one painted wrapped text shape");

    let (text, clip) = shapes[0];

    assert!(text.galley.rows.len() > 4, "test must produce multiple wrapped lines");

    assert!(text.galley.size().y > cell.height(), "wrapped text must exceed the fixed row height");

    assert!(clip.top() >= cell.top() - 0.5 && clip.bottom() <= cell.bottom() + 0.5, "text clip must stay within B3: {clip:?} vs {cell:?}");

    let mut full_rows_visible = 0;

    for (index, row) in text.galley.rows.iter().enumerate() {
        let top = text.pos.y + row.rect().top();
        let bottom = text.pos.y + row.rect().bottom();
        let full_height = bottom - top;

        let visible_height = (bottom.min(clip.bottom()) - top.max(clip.top())).max(0.0);

        let partly_visible = visible_height > 0.5 && visible_height < full_height - 0.5;

        assert!(
            !partly_visible,
            "wrapped line {} is partially visible: \
             line=[{top:.2}, {bottom:.2}], clip={clip:?}",
            index + 1
        );

        if visible_height >= full_height - 0.5 {
            full_rows_visible += 1;
        }
    }

    assert!(full_rows_visible >= 2, "the clipping fix must still show complete text lines");
}

/// Issue #2752: a row shorter than a text line must not
/// render a fragment of wrapped text.
#[test]
fn wrapped_text_in_tiny_row_has_no_partial_lines() {
    const TEXT: &str = "This wrapped text should not be partly painted.";

    let h = harness(|s| {
        s.execute("home.columnWidth", json!({"cols": "B:B", "chars": 12})).unwrap();

        s.execute("cell.set", json!({"cell": "B3", "input": TEXT})).unwrap();

        s.execute("home.wrapText", json!({"range": "B3", "on": true})).unwrap();

        s.execute("home.formatCells", json!({"range": "B3", "style": {"align": {"v": "Top"}}})).unwrap();

        s.execute("home.rowHeight", json!({"rows": "3:3", "height": 5})).unwrap();
    });

    let cell = cell_rect(&h, "B3");

    let shapes: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(t) if t.galley.job.text == TEXT => Some((t, shape.clip_rect)),
            _ => None,
        })
        .collect();

    assert_eq!(shapes.len(), 1, "expected the wrapped text shape");

    let (text, clip) = shapes[0];

    assert!(text.galley.rows.len() > 1, "text must wrap into multiple lines");

    assert!(cell.height() < text.galley.rows[0].rect().height(), "test setup must produce a row shorter than one line");

    assert!(clip.top() >= cell.top() - 0.5 && clip.bottom() <= cell.bottom() + 0.5, "text clip must remain inside its cell");

    assert!(clip.height() <= 0.5, "no partial line should be visible: clip={clip:?}");
}

/// Issue #2752: the viewport bottom can cut through a line
/// that otherwise fits fully inside a wrapped cell.
#[test]
fn wrapped_text_clipping_survives_partial_vertical_scroll() {
    let text = "Wrapped text must stay within both the worksheet cell \
        and the viewport during vertical scrolling. "
        .repeat(40);

    let mut h = harness_with_size(egui::vec2(1200.0, 520.0), |s| {
        s.execute("home.columnWidth", json!({"cols": "B:B", "chars": 12})).unwrap();
        s.execute("cell.set", json!({"cell": "B3", "input": text})).unwrap();
        s.execute("home.wrapText", json!({"range": "B3", "on": true})).unwrap();
        s.execute("home.formatCells", json!({"range": "B3", "style": {"align": {"v": "Top"}}})).unwrap();
        s.execute("home.rowHeight", json!({"rows": "3:3", "height": 350})).unwrap();
    });

    let initial_cell = cell_rect(&h, "B3");

    let (target_center, zoom) = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);

        let shapes: Vec<_> = h
            .output()
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                Shape::Text(t) if t.galley.job.text == text => Some(t),
                _ => None,
            })
            .collect();

        assert_eq!(shapes.len(), 1, "expected wrapped B3 text");

        let painted = shapes[0];

        let center = painted
            .galley
            .rows
            .iter()
            .map(|row| {
                let top = painted.pos.y + row.rect().top();
                let bottom = painted.pos.y + row.rect().bottom();
                (top, bottom)
            })
            .find(|(top, bottom)| {
                let center = (top + bottom) / 2.0;
                center > geo.cells.bottom() + 32.0 && *bottom < initial_cell.bottom() - 1.0
            })
            .map(|(top, bottom)| (top + bottom) / 2.0)
            .expect("fixture must provide a cell-fitting line below the viewport");

        (center, geo.z)
    };

    let viewport_bottom_before_scroll = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        geo.cells.bottom()
    };

    {
        let app = h.state_mut();
        app.view_mut().unwrap().scroll.y = (target_center - viewport_bottom_before_scroll) / zoom;
    }

    h.run_steps(4);

    let cell = cell_rect(&h, "B3");

    let (viewport, scroll_zoom) = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        (geo.cells, geo.z)
    };

    assert!(scroll_zoom > 0.0);

    let shapes: Vec<_> = h
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(t) if t.galley.job.text == text => Some((t, shape.clip_rect)),
            _ => None,
        })
        .collect();

    assert_eq!(shapes.len(), 1, "B3 must remain painted");

    let (painted, clip) = shapes[0];

    assert!(clip.top() >= viewport.top() - 0.5, "clip exceeds viewport top: {clip:?} vs {viewport:?}");

    assert!(clip.bottom() <= viewport.bottom() + 0.5, "clip exceeds viewport bottom: {clip:?} vs {viewport:?}");

    assert!(clip.bottom() <= cell.bottom() + 0.5, "clip exceeds cell bottom: {clip:?} vs {cell:?}");

    let full_line_bottoms: Vec<f32> =
        painted.galley.rows.iter().map(|row| painted.pos.y + row.rect().bottom()).filter(|bottom| *bottom <= cell.bottom() + 0.01).collect();

    let cell_line_bottom = full_line_bottoms.last().copied().expect("at least one line must fit within B3");

    assert!(
        painted.galley.rows.iter().any(|row| {
            let top = painted.pos.y + row.rect().top();
            let bottom = painted.pos.y + row.rect().bottom();

            top + 0.5 < viewport.bottom() && viewport.bottom() < bottom - 0.5 && bottom <= cell.bottom() + 0.01
        }),
        "viewport bottom must cross a line that fits B3: \
         viewport={viewport:?}, cell={cell:?}"
    );

    let expected_bottom = viewport.bottom().min(cell_line_bottom);

    assert!(
        (clip.bottom() - expected_bottom).abs() <= 0.5,
        "clip must respect viewport and complete-line limits: \
         clip={clip:?}, expected={expected_bottom:.2}"
    );
}
