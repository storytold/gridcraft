//! A hidden wrapped header must not push data below the frozen rows out of view (#2724).

use egui::Shape;
use gridcraft_engine::core::{CellRef, RangeRef, Value};
use gridcraft_engine::model::Workbook;
use gridcraft_engine::{DocState, Session, Trust};
use gridcraft_ui_egui::grid::Geo;
use serde_json::json;

#[test]
fn imported_hidden_header_keeps_data_visible_below_freeze_at_different_zooms() {
    let mut wb = Workbook::new();
    let wrapped = wb.styles.derive(Default::default(), |s| s.align.wrap = true);
    let sh = wb.sheet_mut(0).unwrap();
    for row in 0..40 {
        for col in 0..47 {
            sh.set_value(CellRef::new(row, col), Value::text(format!("R{}C{}", row + 1, col + 1)));
        }
    }
    // Preserve the issue's 47-column A1:AU3968 used range without a binary fixture.
    sh.set_value(CellRef::new(3967, 46), Value::text("last cell"));
    sh.set_value(CellRef::new(7, 46), Value::text("Hidden wrapped budget heading"));
    sh.set_style(CellRef::new(7, 46), wrapped);
    let col = sh.cols.entry(46).or_default();
    col.size = Some(64.0);
    col.hidden = true;
    sh.rows.entry(7).or_default().size = Some(70.0);
    sh.freeze = Some((8, 10));
    sh.zoom = 130;
    sh.view_active = CellRef::new(7, 0);
    sh.view_top_left = CellRef::new(8, 10);
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (opened, _) = gridcraft_engine::io::open_bytes("freeze-zoom.xlsx", &bytes).unwrap();
    let mut session = Session::new();
    session.add_document(DocState::new(opened, None, "Freeze zoom".into(), Trust::FromFile));
    let mut h = crate::common::harness_with(crate::common::builder().with_size(egui::vec2(1896.0, 1014.0)), crate::common::app(session), 4);
    for zoom in [50, 75, 100, 110, 125, 130, 150, 175, 200] {
        for freeze in ["A1", "A9", "K1", "K9", "C4"] {
            h.state_mut().session.execute("view.freezePanes", json!({"cell": freeze})).unwrap();
            h.state_mut().session.execute("view.zoom", json!({"percent": zoom})).unwrap();
            for scroll in [egui::Vec2::ZERO, egui::vec2(37.5, 205.25)] {
                h.state_mut().view_mut().unwrap().scroll = scroll;
                h.run_steps(4);
                let app = h.state();
                let sh = app.session.active().unwrap().wb.active().unwrap();
                assert_eq!(sh.row_height(7), 77.0, "the frozen header fits its hidden text at the stored width");
                let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
                let mut checked_data = 0;
                let mut checked_header = 0;
                let freezes_header = matches!(freeze, "A9" | "K9");
                let first_row = if freezes_header { 7 } else { 8 };
                for row in first_row..40 {
                    for col in [0, 9, 10, 11] {
                        let cell = CellRef::new(row, col);
                        let rect = geo.cell_rect(sh, cell);
                        if !geo.cells.contains_rect(rect) || !geo.range_panes(RangeRef::cell(cell)).iter().any(|pane| pane.contains_rect(rect)) {
                            continue;
                        }
                        let text = format!("R{}C{}", row + 1, col + 1);
                        let painted = h.output().shapes.iter().any(|s| match &s.shape {
                            Shape::Text(t) if t.galley.job.text == text => {
                                s.clip_rect.intersect(t.galley.rect.translate(t.pos.to_vec2())).intersects(rect.shrink(1.0))
                            }
                            _ => false,
                        });
                        assert!(painted, "{} must paint at zoom {zoom}, freeze {freeze}, scroll {scroll:?}", cell.a1());
                        if row == 7 {
                            checked_header += 1;
                        } else {
                            checked_data += 1;
                        }
                    }
                }
                if freezes_header {
                    assert!(checked_header > 0, "the frozen header paints at zoom {zoom}, freeze {freeze}, scroll {scroll:?}");
                }
                assert!(checked_data > 0, "data rows are visible at zoom {zoom}, freeze {freeze}, scroll {scroll:?}");
            }
        }
    }
}
