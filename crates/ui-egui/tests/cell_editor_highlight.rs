//! The highlight of a cell being typed into is the selection's own outline: it neither moves
//! nor grows. Text that runs past the cell is laid over the cells after it, a whole cell at a
//! time, and clears the lines it runs over.

use egui::{Color32, Pos2, Rect, Shape, StrokeKind, pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::{Geo, outline_sides};
use gridcraft_ui_egui::theme::Tokens;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const LONG: &str = "Text that is a good deal wider than a cell";

fn session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell": "C4"})).unwrap();
    session
}

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// Where a cell is on screen.
fn cell(h: &Harness, a1: &str) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap())
}

/// The sides of the outline round a cell: left, top, right, bottom.
fn sides(h: &Harness, a1: &str) -> [Rect; 4] {
    outline_sides(cell(h, a1), h.ctx.pixels_per_point())
}

fn typed(h: &mut Harness, text: &str) {
    h.input_mut().events.push(egui::Event::Text(text.into()));
    h.run_steps(3);
    assert!(h.state().editor.is_some());
}

/// The shapes of the last frame in the order they were painted, each with what it is clipped to.
fn painted(h: &Harness) -> Vec<(Rect, &Shape)> {
    fn flat<'a>(clip: Rect, shape: &'a Shape, out: &mut Vec<(Rect, &'a Shape)>) {
        match shape {
            Shape::Vec(inner) => inner.iter().for_each(|s| flat(clip, s, out)),
            other => out.push((clip, other)),
        }
    }
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        flat(clipped.clip_rect, &clipped.shape, &mut out);
    }
    out
}

/// Those of them that may show at `at`.
fn shapes(h: &Harness, at: Pos2) -> Vec<&Shape> {
    painted(h).into_iter().filter(|(clip, _)| clip.contains(at)).map(|(_, shape)| shape).collect()
}

/// What a filled or stroked shape paints at a point: a fill, or a line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Paint {
    Fill(Color32),
    Line(Color32),
}

fn paints(shape: &Shape, at: Pos2) -> Option<Paint> {
    match shape {
        Shape::Rect(r) => {
            let w = r.stroke.width;
            let (outer, inner) = match r.stroke_kind {
                StrokeKind::Inside => (r.rect, r.rect.shrink(w)),
                StrokeKind::Middle => (r.rect.expand(w / 2.0), r.rect.shrink(w / 2.0)),
                StrokeKind::Outside => (r.rect.expand(w), r.rect),
            };
            if w > 0.0 && r.stroke.color != Color32::TRANSPARENT && outer.contains(at) && !inner.contains(at) {
                Some(Paint::Line(r.stroke.color))
            } else if r.fill != Color32::TRANSPARENT && r.rect.contains(at) {
                Some(Paint::Fill(r.fill))
            } else {
                None
            }
        }
        Shape::LineSegment { points, stroke } => {
            Rect::from_two_pos(points[0], points[1]).expand(stroke.width / 2.0).contains(at).then_some(Paint::Line(stroke.color))
        }
        _ => None,
    }
}

/// What the last frame left on top at `at`, of its fills and lines (text aside).
fn on_top(h: &Harness, at: Pos2) -> Option<Paint> {
    shapes(h, at).into_iter().filter_map(|shape| paints(shape, at)).next_back()
}

/// Whether the cell text `text` has a fill painted over it at `at` in the last frame.
fn painted_over(h: &Harness, text: &str, at: Pos2) -> bool {
    let shapes = shapes(h, at);
    let own = shapes.iter().position(|s| matches!(s, Shape::Text(t) if t.galley.text() == text)).expect("the cell's text is painted");
    shapes[own + 1..].iter().any(|s| matches!(paints(s, at), Some(Paint::Fill(_))))
}

#[test]
fn typing_leaves_the_highlight_where_the_selection_outline_was() {
    let mut h = harness(session());
    let green = Paint::Fill(Tokens::get(&h.ctx).sel_border);
    let all = sides(&h, "C4");
    for side in all {
        assert_eq!(on_top(&h, side.center()), Some(green), "the selection's outline, at {side:?}");
    }
    typed(&mut h, "Hi");
    // The editor's highlight is those same four sides…
    for side in all {
        assert_eq!(on_top(&h, side.center()), Some(green), "at {side:?}");
    }
    // …with nothing of it further out, and the cell's own background just inside.
    let [left, top, right, bottom] = all;
    let c = cell(&h, "C4").center();
    for outside in [pos2(left.left() - 0.5, c.y), pos2(c.x, top.top() - 0.5), pos2(right.right() + 0.5, c.y), pos2(c.x, bottom.bottom() + 0.5)] {
        assert_ne!(on_top(&h, outside), Some(green), "at {outside:?}");
    }
    for inside in [pos2(left.right() + 0.5, c.y), pos2(c.x, top.bottom() + 0.5), pos2(right.left() - 0.5, c.y), pos2(c.x, bottom.top() - 0.5)] {
        assert_eq!(on_top(&h, inside), Some(Paint::Fill(Color32::WHITE)), "at {inside:?}");
    }
}

#[test]
fn text_that_runs_past_the_cell_clears_the_lines_it_crosses_and_the_highlight_does_not_grow() {
    let mut h = harness(session());
    let t = Tokens::get(&h.ctx);
    let (green, white) = (Paint::Fill(t.sel_border), Paint::Fill(Color32::WHITE));
    let (at, d4) = (cell(&h, "C4"), cell(&h, "D4"));
    let [left, top, right, bottom] = sides(&h, "C4");
    // The gridline between D4 and E4, and the same line a row up.
    let gridline = pos2(d4.right().round() - 0.5, at.center().y);
    let row_above = pos2(gridline.x, cell(&h, "C3").center().y);
    assert_eq!(on_top(&h, gridline), Some(Paint::Line(t.gridline)));
    assert_eq!(on_top(&h, right.center()), Some(green));
    typed(&mut h, LONG);
    // Its right side is cleared where the text runs over it; the other three stay.
    assert_eq!(on_top(&h, right.center()), Some(white), "the line the text crosses");
    for side in [left, top, bottom] {
        assert_eq!(on_top(&h, side.center()), Some(green), "at {side:?}");
    }
    // The highlight has not grown: past the cell there is no top or bottom side.
    for x in [right.right() + 3.0, d4.center().x, d4.right() + 6.0] {
        for y in [top.center().y, bottom.center().y] {
            assert_ne!(on_top(&h, pos2(x, y)), Some(green), "at {x}, {y}");
        }
    }
    // The gridline under the text is cleared, in this row only.
    assert_eq!(on_top(&h, gridline), Some(white));
    assert_eq!(on_top(&h, row_above), Some(Paint::Line(t.gridline)));
    // The gridlines above and below the cells the text runs over stay.
    for y in [d4.top().round() - 0.5, d4.bottom().round() - 0.5] {
        assert_eq!(on_top(&h, pos2(d4.center().x, y)), Some(Paint::Line(t.gridline)), "at y {y}");
    }
    // Finishing gives the lines back.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().editor.is_none());
    assert_eq!(on_top(&h, right.center()), Some(green));
    assert_eq!(on_top(&h, gridline), Some(Paint::Line(t.gridline)));
}

#[test]
fn a_cell_the_editor_grows_over_is_covered_whole() {
    let mut s = session();
    s.execute("cell.set", json!({"cell": "D4", "input": "WWW"})).unwrap();
    let mut h = harness(s);
    let d4 = cell(&h, "D4");
    // Just inside D4's far gridline: much further than a little text typed into C4 reaches.
    let far = pos2(d4.right() - 3.0, d4.center().y);
    assert!(!painted_over(&h, "WWW", far));
    // Text in C4 that fits leaves D4 alone.
    typed(&mut h, "Hi");
    assert!(!painted_over(&h, "WWW", far));
    // A little more than fits: D4 is covered to its far gridline, so none of it shows beside
    // what is being typed. The gridline itself stays.
    typed(&mut h, "llllllllllllll");
    assert!(painted_over(&h, "WWW", far));
    assert!(painted_over(&h, "WWW", pos2(d4.left() + 3.0, d4.center().y)));
    let gridline = Paint::Line(Tokens::get(&h.ctx).gridline);
    assert_eq!(on_top(&h, pos2(d4.right().round() - 0.5, d4.center().y)), Some(gridline), "D4's far gridline");
    // Leaving the editor gives D4 back.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().editor.is_none());
    assert!(!painted_over(&h, "WWW", far));
}

#[test]
fn lines_added_below_take_whole_rows_and_clear_the_bottom_line() {
    let mut s = session();
    s.execute("cell.set", json!({"cell": "C5", "input": "WWWWWW"})).unwrap();
    s.execute("cell.set", json!({"cell": "C6", "input": "MMMMMM"})).unwrap();
    let mut h = harness(s);
    let t = Tokens::get(&h.ctx);
    let (green, white) = (Paint::Fill(t.sel_border), Paint::Fill(Color32::WHITE));
    let (c5, c6) = (cell(&h, "C5"), cell(&h, "C6"));
    let [left, top, right, bottom] = sides(&h, "C4");
    h.state_mut().begin_edit(Some("one\ntwo".into()), false);
    h.run_steps(3);
    assert_eq!(on_top(&h, bottom.center()), Some(white), "the line the second line of text crosses");
    for side in [left, top, right] {
        assert_eq!(on_top(&h, side.center()), Some(green), "at {side:?}");
    }
    // Two lines take two rows: C5 is covered and C6 is not, with the gridline between them kept.
    assert!(painted_over(&h, "WWWWWW", pos2(c5.left() + 8.0, c5.center().y)));
    assert!(!painted_over(&h, "MMMMMM", pos2(c6.left() + 8.0, c6.center().y)));
    assert_eq!(on_top(&h, pos2(c5.center().x, c5.bottom().round() - 0.5)), Some(Paint::Line(t.gridline)));
}
