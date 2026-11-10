//! Spreadsheet text remains logical while the shared renderer lays it out visually.

use egui::{Event, Key, Modifiers, Shape, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{
    Session,
    core::{CellRef, Value},
};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn app(text: &str, order: &str, sheet_rtl: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B2", "input": text})).unwrap();
    session.execute("home.formatCells", json!({"range":"B2", "style":{"align":{"reading_order":order},"font":{"size":20}}})).unwrap();
    session.execute("selection.set", json!({"cell":"B2"})).unwrap();
    if sheet_rtl {
        session.execute("view.rightToLeft", json!({"on":true})).unwrap();
    }
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
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

fn text_shapes<'a>(shape: &'a Shape, text: &str, out: &mut Vec<&'a egui::epaint::TextShape>) {
    match shape {
        Shape::Text(s) if s.galley.text() == text => out.push(s),
        Shape::Vec(shapes) => {
            for s in shapes {
                text_shapes(s, text, out);
            }
        }
        _ => {}
    }
}

#[test]
fn explicit_direction_reaches_cells_on_both_sheet_geometries() {
    let text = "ABC مرحبا 123";
    for sheet_rtl in [false, true] {
        for (order, rtl) in [("LeftToRight", false), ("RightToLeft", true)] {
            let h = app(text, order, sheet_rtl);
            let mut painted = Vec::new();
            for shape in &h.output().shapes {
                text_shapes(&shape.shape, text, &mut painted);
            }
            assert!(painted.iter().any(|s| s.galley.job.direction
                == if rtl { egui::epaint::text::TextDirection::RightToLeft } else { egui::epaint::text::TextDirection::LeftToRight }));
            for shape in painted {
                assert_eq!(shape.galley.rows.iter().flat_map(|r| r.glyphs.iter().map(|g| g.chr)).collect::<String>(), text);
                assert_eq!(shape.galley.rows[0].rtl_paragraph, rtl);
            }
        }
    }
}

#[test]
fn marked_arabic_survives_copy_paste_commit_undo_and_xlsx() {
    let text = "مَرْحَبًا 123 (ABC)";
    for formula_bar in [false, true] {
        let mut h = app(text, "RightToLeft", false);
        if formula_bar {
            h.get_by_label(text).click();
        } else {
            h.key_press(Key::F2);
        }
        h.run_steps(3);
        assert_eq!(h.state().editor.as_ref().map(|e| e.from_formula_bar), Some(formula_bar));
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.run_steps(1);
        h.input_mut().events.push(Event::Copy);
        h.run_steps(1);
        assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == text)));
        let replacement = "تَجْرِبَة ١٢٣ (ABC)";
        h.input_mut().events.push(Event::Paste(replacement.into()));
        h.run_steps(2);
        h.key_press(Key::Enter);
        h.run_steps(2);
        let value = |h: &Harness| h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(1, 1));
        assert_eq!(value(&h), Value::text(replacement));
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(value(&h), Value::text(text));
        h.state_mut().session.execute("edit.redo", json!({})).unwrap();
        assert_eq!(value(&h), Value::text(replacement));
        let wb = &h.state().session.active().unwrap().wb;
        let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
        let (reopened, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        assert_eq!(reopened.active().unwrap().value(CellRef::new(1, 1)), Value::text(replacement));
    }
}

#[test]
fn conditional_reading_order_reaches_the_formula_bar() {
    let text = "ABC مرحبا 123";
    for (rule_style, rtl) in [(json!({"align": {"reading_order": "RightToLeft"}}), true), (json!({"font": {"bold": true}}), false)] {
        let mut h = app(text, "LeftToRight", false);
        h.state_mut().session.execute("home.conditionalFormat", json!({"range": "B2", "rule": {"type": "noBlanks", "style": rule_style}})).unwrap();
        h.run_steps(2);
        let mut painted = Vec::new();
        for shape in &h.output().shapes {
            text_shapes(&shape.shape, text, &mut painted);
        }
        let formula_bar = painted.iter().min_by(|a, b| a.pos.y.total_cmp(&b.pos.y)).unwrap();
        assert_eq!(formula_bar.galley.rows[0].rtl_paragraph, rtl);
    }
}

fn painted_galleys(h: &Harness, text: &str) -> Vec<std::sync::Arc<egui::Galley>> {
    let mut painted = Vec::new();
    for shape in &h.output().shapes {
        text_shapes(&shape.shape, text, &mut painted);
    }
    painted.into_iter().map(|s| s.galley.clone()).collect()
}

#[test]
fn explicit_left_keeps_wrapped_right_to_left_lines_on_the_left() {
    let text = "مرحبا بالعالم\nا";
    let mut h = app(text, "RightToLeft", false);
    h.state_mut()
        .session
        .execute("home.formatCells", json!({"range": "B2", "style": {"align": {"h": "Left", "wrap": true, "reading_order": "RightToLeft"}}}))
        .unwrap();
    h.run_steps(2);
    let mut painted = Vec::new();
    for shape in &h.output().shapes {
        text_shapes(&shape.shape, text, &mut painted);
    }
    let cell = painted.iter().max_by(|a, b| a.pos.y.total_cmp(&b.pos.y)).unwrap();
    assert!(cell.galley.rows.len() > 1);
    assert!(cell.galley.rows.iter().all(|row| row.rtl_paragraph));
    for row in &cell.galley.rows {
        let left = row.pos.x + row.glyphs.iter().map(|g| g.pos.x).fold(f32::INFINITY, f32::min);
        assert!(left.abs() < 0.5, "row starts at {left}");
    }
}

#[test]
fn right_to_left_editor_keeps_a_trailing_space_inside_the_text() {
    let text = "مرحبا ";
    let mut h = app("", "RightToLeft", false);
    h.key_press(Key::F2);
    h.run_steps(2);
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(3);
    let galleys = painted_galleys(&h, text);
    assert!(!galleys.is_empty());
    for galley in galleys {
        for row in &galley.rows {
            for glyph in &row.glyphs {
                let x = row.pos.x + glyph.pos.x;
                assert!(x >= -0.5, "{:?} at {x}", glyph.chr);
            }
        }
    }
}

fn wrapped_row_edges(sheet_rtl: bool, h_align: &str, text: &str) -> Vec<(f32, f32)> {
    let mut h = app(text, "RightToLeft", sheet_rtl);
    h.state_mut().session.execute("home.formatCells", json!({"range": "B2", "style": {"align": {"h": h_align, "wrap": true}}})).unwrap();
    h.run_steps(2);
    let mut painted = Vec::new();
    for shape in &h.output().shapes {
        text_shapes(&shape.shape, text, &mut painted);
    }
    let cell = painted.iter().max_by(|a, b| a.pos.y.total_cmp(&b.pos.y)).unwrap();
    assert!(cell.galley.rows.len() > 1);
    cell.galley.rows.iter().map(|row| (cell.pos.x + row.rect().left(), cell.pos.x + row.rect().right())).collect()
}

#[test]
fn wrapped_right_to_left_rows_follow_the_physical_alignment_on_both_sheet_geometries() {
    let text = "مرحبا بالعالم\nا";
    for (sheet_rtl, h_align, on_right) in [
        (false, "General", true),
        (false, "Left", false),
        (false, "Right", true),
        (true, "General", true),
        (true, "Left", false),
        (true, "Right", true),
    ] {
        let edges = wrapped_row_edges(sheet_rtl, h_align, text);
        let side: Vec<f32> = edges.iter().map(|&(left, right)| if on_right { right } else { left }).collect();
        let first = side[0];
        assert!(side.iter().all(|x| (x - first).abs() <= 1.0), "sheet_rtl {sheet_rtl}, {h_align}: rows at {edges:?}");
    }
}

fn single_line_right_edge(text: &str, order: &str, sheet_rtl: bool) -> f32 {
    let h = app(text, order, sheet_rtl);
    let mut painted = Vec::new();
    for shape in &h.output().shapes {
        text_shapes(&shape.shape, text, &mut painted);
    }
    let cell = painted.iter().max_by(|a, b| a.pos.y.total_cmp(&b.pos.y)).unwrap();
    assert_eq!(cell.galley.rows.len(), 1);
    cell.galley
        .rows
        .iter()
        .flat_map(|row| row.glyphs.iter().map(move |g| cell.pos.x + row.pos.x + g.pos.x + g.advance_width))
        .fold(f32::NEG_INFINITY, f32::max)
}

#[test]
fn a_mark_on_the_first_letter_keeps_single_line_right_to_left_text_flush_right() {
    for (marked, plain) in [("يُرجى مراجعة", "يرجى مراجعة"), ("بَاب", "باب")] {
        for sheet_rtl in [false, true] {
            for order in ["RightToLeft", "Context"] {
                let marked_right = single_line_right_edge(marked, order, sheet_rtl);
                let plain_right = single_line_right_edge(plain, order, sheet_rtl);
                assert!(
                    (marked_right - plain_right).abs() <= 1.0,
                    "{marked}, sheet_rtl {sheet_rtl}, {order}: ends at {marked_right}, unmarked at {plain_right}"
                );
            }
        }
    }
}
