//! Overflowing text stays visible in its empty neighbours when its own cell is scrolled out (#1613).

use egui::{Rect, Shape};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const LEFT: &str = "This is a very long left-aligned text that overflows into many empty neighbouring cells";
const RIGHT: &str = "This is a very long right-aligned text that overflows into many empty neighbouring cells";

fn harness(setup: impl FnOnce(&mut Session)) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    setup(&mut s);
    s.execute("selection.set", json!({"range": "A20"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
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

fn cell_rect(h: &Harness, cell: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap())
}

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
