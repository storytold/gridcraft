//! Integration tests for Thai line wrapping in committed worksheet cells.
//!
//! Tests real grid renderer integration with egui_kittest harness:
//! - Human-verified Thai break opportunities ("วันนี้" / "อากาศดีมาก")
//! - Full grid painting through `paint_panes` and `paint_quadrant`
//! - Merged cells, frozen panes, and alignments
//! - Undo/redo, clipboard copy-paste, and XLSX byte-for-byte round-trip
//! - Regression protection for non-Thai, numeric, and wrap-off cells

use egui::{Color32, Pos2, Rect, Shape, Vec2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo, thai_layout, theme};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[path = "common/thai_font.rs"]
mod thai_font;

type Harness = egui_kittest::Harness<'static, SheetApp>;
static SNAPSHOT_RENDER_LOCK: Mutex<()> = Mutex::new(());
static SNAPSHOT_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

fn harness_with_session_config(session: Session, zoom: Option<u16>, pixels_per_point: f32, steps: usize) -> Harness {
    let mut session = session;
    if let Some(percent) = zoom {
        session.execute("view.zoom", json!({"percent": percent})).unwrap();
    }
    let mut app = SheetApp::new(session, Default::default());
    app.use_prepared_fonts();
    let fonts = thai_font::required_font_definitions();
    let setup_fonts = fonts.clone();
    let mut h = egui_kittest::Harness::builder()
        .with_context_setup(move |ctx| {
            theme::install_fonts(ctx, setup_fonts.clone());
            theme::apply(ctx, false);
        })
        .with_size(vec2(1200.0, 800.0))
        .with_pixels_per_point(pixels_per_point)
        .build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
    // Harness construction lets SheetApp install its startup font set. Reapply the explicit Thai
    // fixture after setup so the integration assertions do not depend on a system fallback.
    theme::install_fonts(&h.ctx, fonts);
    thai_font::assert_required_coverage(&h.ctx);
    if steps > 0 {
        h.run_steps(steps);
    }
    h
}

fn harness_with_session(session: Session) -> Harness {
    harness_with_session_config(session, None, 1.0, 4)
}

/// Collects all painted text strings from the latest frame output.
fn painted_texts(h: &Harness) -> Vec<String> {
    let mut out = Vec::new();
    for s in &h.output().shapes {
        if let Shape::Text(t) = &s.shape {
            out.push(t.galley.job.text.clone());
        }
    }
    out
}

#[derive(Clone, Debug)]
struct PaintedText {
    text: String,
    pos: Pos2,
    size: Vec2,
    clip: Rect,
}

fn painted_text_shapes(h: &Harness) -> Vec<PaintedText> {
    h.output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Text(text) => {
                Some(PaintedText { text: text.galley.job.text.clone(), pos: text.pos, size: text.galley.size(), clip: shape.clip_rect })
            }
            _ => None,
        })
        .collect()
}

fn assert_near(actual: f32, expected: f32, context: &str) {
    assert!((actual - expected).abs() < 0.75, "{context}: actual={actual:.3}, expected={expected:.3}");
}

fn save_task3_snapshot(h: &mut Harness, filename: &str) {
    let _snapshot_guard = SNAPSHOT_RENDER_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let image = h.render().expect("render TASK 3 geometry snapshot");
    let snapshot_dir = SNAPSHOT_DIRECTORY.get_or_init(|| {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plan");
        std::fs::create_dir_all(&parent).unwrap();
        // Keep diagnostic artifacts for inspection, with exclusive random isolation
        // across test processes (including processes that happen to share a PID).
        let directory = tempfile::Builder::new().prefix("thai-rendering-").tempdir_in(parent).unwrap().keep();
        eprintln!("Thai rendering snapshots: {}", directory.display());
        directory
    });
    image.save(snapshot_dir.join(filename)).unwrap();
}

#[test]
fn test_thai_cell_wrapping_in_grid_renderer() {
    let mut s = Session::new();
    s.new_workbook();
    // A1: Thai text with Wrap Text ON and narrow column width (forcing wrap)
    s.execute("cell.set", json!({"cell": "A1", "input": "วันนี้อากาศดีมาก"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A", "width": 60})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1", "height": 50})).unwrap();

    // A2: Same text with Wrap Text OFF (control)
    s.execute("cell.set", json!({"cell": "A2", "input": "วันนี้อากาศดีมาก"})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A", "width": 60})).unwrap();
    // Row 1 is a Thai filter header: its dropdown must reserve space before wrapping.
    s.execute("data.filter", json!({"range": "A1:A2"})).unwrap();

    let h = harness_with_session(s);
    let texts = painted_texts(&h);

    // In A1 with Wrap Text ON: must break at word boundaries ("วันนี้", "อากาศ", "ดีมาก")
    assert!(texts.iter().any(|t| t == "วันนี้"), "Grid renderer should paint 'วันนี้' on its own line in wrapped cell; found: {:?}", texts);
    assert!(
        texts.iter().any(|t| t == "อากาศ" || t == "อากาศดีมาก"),
        "Grid renderer should paint 'อากาศ' or 'อากาศดีมาก' on next line in wrapped cell; found: {:?}",
        texts
    );
    assert!(texts.iter().any(|t| t == "ดีมาก" || t == "อากาศดีมาก"), "Grid renderer should paint 'ดีมาก' or 'อากาศดีมาก'; found: {:?}", texts);

    // In A2 with Wrap Text OFF: the entire unbroken text must be painted on one line
    assert!(texts.iter().any(|t| t == "วันนี้อากาศดีมาก"), "Unwrapped cell A2 must paint full string on one line; found: {:?}", texts);
}

#[test]
fn test_thai_mixed_latin_numbers_currency() {
    let mut s = Session::new();
    s.new_workbook();
    let text = "ยอดรวม Total 1,234.50 บาท (VAT 7%) + ฿";
    s.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A", "width": 80})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1", "height": 60})).unwrap();

    let h = harness_with_session(s);
    let texts = painted_texts(&h);

    // Verify wrapped lines are rendered without panics or corrupt characters
    assert!(texts.iter().any(|t| t.contains("ยอดรวม")), "Wrapped mixed Thai/Latin cell must render Thai prefix; found: {:?}", texts);
}

#[test]
fn test_thai_cell_hard_breaks_and_blank_lines() {
    let mut s = Session::new();
    s.new_workbook();
    let multiline = "ข้อความหนึ่ง\r\nข้อความสอง\n\nข้อความสี่\r\n";
    s.execute("cell.set", json!({"cell": "B2", "input": multiline})).unwrap();
    s.execute("home.formatCells", json!({"range": "B2", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "B", "width": 120})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "2", "height": 80})).unwrap();

    let h = harness_with_session(s);
    let texts = painted_texts(&h);
    let raw = &h.state().session.active().unwrap().wb.active().unwrap().cell(CellRef::parse("B2").unwrap()).unwrap().value;
    assert_eq!(raw, &Value::from(multiline), "rendering and paint ranges must leave LF/CRLF, blank lines, and trailing newline untouched");

    assert!(texts.iter().any(|t| t.contains("ข้อความหนึ่ง") || (t.contains("ข้อความ") && texts.iter().any(|x| x.contains("หนึ่ง")))));
    assert!(texts.iter().any(|t| t.contains("ข้อความสอง") || (t.contains("ข้อความ") && texts.iter().any(|x| x.contains("สอง")))));
    assert!(texts.iter().any(|t| t.contains("ข้อความสี่") || (t.contains("ข้อความ") && texts.iter().any(|x| x.contains("สี่")))));
}

#[test]
fn test_thai_cell_narrow_oversized_clips_safely() {
    let mut s = Session::new();
    s.new_workbook();
    // Word much wider than the column
    s.execute("cell.set", json!({"cell": "A1", "input": "สวัสดีประเทศไทย"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A", "width": 10})).unwrap(); // very narrow

    let h = harness_with_session(s);
    let texts = painted_texts(&h);
    // Should render without panic; text is clipped to cell bounds
    assert!(texts.iter().any(|t| t.contains("สวัสดี")));
}

#[test]
fn test_thai_cell_merged_and_frozen_panes() {
    let mut s = Session::new();
    s.new_workbook();
    let text = "วันนี้อากาศดีมาก";
    s.execute("cell.set", json!({"cell": "C3", "input": text})).unwrap();
    s.execute("home.formatCells", json!({"range": "C3", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("cell.set", json!({"cell": "C6", "input": text})).unwrap();
    s.execute("home.formatCells", json!({"range": "C6", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "C:D", "width": 34})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "3:4", "height": 60})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "6", "height": 60})).unwrap();
    s.execute("home.mergeCells", json!({"range": "C3:D4"})).unwrap();
    s.execute("view.freezePanes", json!({"cell": "B2"})).unwrap();

    let mut h = harness_with_session_config(s, None, 1.0, 0);
    {
        let app = h.state_mut();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        let scroll = vec2((sh.col_left(2) - sh.col_left(1)) as f32 + 8.0, (sh.row_top(2) - sh.row_top(1)) as f32 + 8.0);
        app.view_mut().unwrap().scroll = scroll;
    }
    thai_layout::begin_frame_budget(&h.ctx);
    h.step();
    let usage = thai_layout::frame_budget_usage(&h.ctx);
    assert!(usage.segmentations > 0 && usage.measurements > 0, "merged frozen cell must reach the Thai adapter: {usage:?}");
    h.run_steps(3);

    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let merge = gridcraft_engine::core::RangeRef::new(CellRef::parse("C3").unwrap(), CellRef::parse("D4").unwrap());
    let merge_rect = geo.range_rect(sh, merge);
    let split = egui::pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
    assert!(
        merge_rect.left() < split.x && merge_rect.top() < split.y,
        "scrolling merged rectangle begins behind the frozen boundaries: {merge_rect:?}, {split:?}"
    );
    assert!(merge_rect.right() > split.x && merge_rect.bottom() > split.y, "merged rectangle extends into the scrolling quadrant");
    let pane = *geo.range_panes(merge).first().expect("merged area is in the scrolling quadrant");
    let expected_clip = merge_rect.intersect(pane);
    let shapes: Vec<_> = painted_text_shapes(&h)
        .into_iter()
        .filter(|shape| shape.text.contains("วันนี้") || thai_layout::has_thai_chars(&shape.text))
        .filter(|shape| shape.clip.intersect(merge_rect.shrink(0.1)).is_positive())
        .collect();
    assert!(shapes.len() >= 2, "merged Thai cell wraps using its actual merged display rectangle: {shapes:?}");
    let control_rect = geo.cell_rect(sh, CellRef::parse("C6").unwrap());
    let control_lines: Vec<_> = painted_text_shapes(&h)
        .into_iter()
        .filter(|shape| thai_layout::has_thai_chars(&shape.text) && shape.clip.intersect(control_rect.shrink(0.1)).is_positive())
        .collect();
    assert!(
        control_lines.len() > shapes.len(),
        "the merged C3:D4 rectangle must wrap into fewer lines than the single-column C6 control: merged={shapes:?}, control={control_lines:?}"
    );
    for shape in &shapes {
        assert_near(shape.clip.left(), expected_clip.left(), "merged frozen clip left");
        assert_near(shape.clip.top(), expected_clip.top(), "merged frozen clip top");
        assert_near(shape.clip.right(), expected_clip.right(), "merged frozen clip right");
        assert_near(shape.clip.bottom(), expected_clip.bottom(), "merged frozen clip bottom");
        assert!(
            shape.clip.left() >= split.x - 0.75,
            "scrolling text clip must start at or right of the frozen column split: {:?} vs {split:?}",
            shape.clip
        );
        assert!(shape.clip.top() >= split.y - 0.75, "scrolling text clip must start at or below the frozen row split: {:?} vs {split:?}", shape.clip);
    }
    save_task3_snapshot(&mut h, "thai_rendering_task3_frozen_merge.png");
}

#[test]
fn formula_text_results_wrap_but_show_formulas_keeps_formula_source_on_legacy_path() {
    let mut session = Session::new();
    session.new_workbook();
    let formula = "=\"ผลลัพธ์ไทยวันนี้อากาศดีมาก\"";
    let result = "ผลลัพธ์ไทยวันนี้อากาศดีมาก";
    session.execute("cell.set", json!({"cell": "A1", "input": formula})).unwrap();
    session.execute("home.formatCells", json!({"range": "A1", "style": {"align": {"wrap": true}}})).unwrap();
    session.execute("home.columnWidth", json!({"cols": "A", "width": 60})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "1", "height": 70})).unwrap();

    let mut h = harness_with_session_config(session, None, 1.0, 0);
    thai_layout::begin_frame_budget(&h.ctx);
    h.step();
    let result_usage = thai_layout::frame_budget_usage(&h.ctx);
    assert!(
        result_usage.segmentations > 0 && result_usage.measurements > 0,
        "a committed formula returning Thai text must use the Thai adapter: {result_usage:?}"
    );
    h.run_steps(3);
    {
        let app = h.state();
        let sh = app.session.active().unwrap().wb.active().unwrap();
        let cell = sh.cell(CellRef::parse("A1").unwrap()).unwrap();
        assert_eq!(cell.input_text(), formula, "painting must not rewrite formula source");
        assert_eq!(cell.value, Value::from(result), "formula result remains the stored value");
    }

    h.state_mut().run("formulas.showFormulas", json!({"on": true})).unwrap();
    thai_layout::begin_frame_budget(&h.ctx);
    h.step();
    let source_usage = thai_layout::frame_budget_usage(&h.ctx);
    assert_eq!(source_usage.segmentations, 0, "Show Formulas must not submit formula source to Thai segmentation");
    assert_eq!(source_usage.measurements, 0, "Show Formulas must keep source text on legacy layout");
    assert!(painted_texts(&h).iter().any(|text| text == formula), "Show Formulas paints the original formula source");
    let cell = h.state().session.active().unwrap().wb.active().unwrap().cell(CellRef::parse("A1").unwrap()).unwrap();
    assert_eq!(cell.input_text(), formula, "Show Formulas must not change raw formula source");
}

#[test]
fn thai_adapter_geometry_is_active_in_ui_page_layout() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B2", "input": "วันนี้อากาศดีมาก"})).unwrap();
    session.execute("home.formatCells", json!({"range": "B2", "style": {"align": {"wrap": true}}})).unwrap();
    session.execute("home.columnWidth", json!({"cols": "B", "width": 30})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "2", "height": 80})).unwrap();
    session.execute("view.pageLayout", json!({})).unwrap();

    let mut h = harness_with_session_config(session, None, 1.0, 0);
    thai_layout::begin_frame_budget(&h.ctx);
    h.step();
    let usage = thai_layout::frame_budget_usage(&h.ctx);
    assert!(usage.segmentations > 0 && usage.measurements > 0, "UI Page Layout must share the Thai renderer and pass budget: {usage:?}");
    h.run_steps(3);

    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = app.grid.geo.as_ref().expect("Page Layout uses the production page geometry");
    assert!(geo.paper.is_some(), "the painted grid is in UI Page Layout view");
    let rect = geo.cell_rect(sheet, CellRef::parse("B2").unwrap());
    let visible: Vec<_> = painted_text_shapes(&h)
        .into_iter()
        .filter(|shape| thai_layout::has_thai_chars(&shape.text) && shape.clip.intersect(rect.shrink(0.1)).is_positive())
        .collect();
    assert!(visible.len() >= 2, "Page Layout paints wrapped lines through the actual page renderer: {visible:?}");
    for line in &visible {
        assert!(geo.cells.contains_rect(line.clip), "Page Layout Thai clip must stay inside the page workspace: {:?} vs {:?}", line.clip, geo.cells);
        assert!(line.clip.intersect(rect).is_positive(), "Page Layout line clip must intersect B2: {:?} vs {rect:?}", line.clip);
    }
    save_task3_snapshot(&mut h, "thai_rendering_task3_page_layout.png");
}

#[test]
fn test_thai_xlsx_roundtrip_undo_redo_copy_paste() {
    let mut s = Session::new();
    s.new_workbook();
    let original = "วันนี้อากาศดีมาก\r\n\nเก้าอี้ น้ำ กำลัง ซื้อ เสื้อ เผื่อ\n";
    let formula = "=\"วันนี้อากาศดีมาก\"";
    s.execute("cell.set", json!({"cell": "A1", "input": original})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": formula})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1:A2", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "width": 80})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1:2", "height": 80})).unwrap();

    let cell_a1 = CellRef::parse("A1").unwrap();
    let cell_a2 = CellRef::parse("A2").unwrap();
    let cell_b1 = CellRef::parse("B1").unwrap();
    let cell_b2 = CellRef::parse("B2").unwrap();

    // Paint first, then verify no display-only range or newline changed the workbook data.
    let mut h = harness_with_session(s);
    {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        assert_eq!(sheet.cell(cell_a1).unwrap().value, Value::from(original));
        assert_eq!(sheet.cell(cell_a2).unwrap().input_text(), formula);
        assert_eq!(sheet.cell(cell_a2).unwrap().value, Value::from("วันนี้อากาศดีมาก"));
    }

    // Copy the wrapped text and formula source to B1:B2.
    h.state_mut().session.execute("edit.copy", json!({"range": "A1:A2"})).unwrap();
    h.state_mut().session.execute("edit.paste", json!({"at": "B1"})).unwrap();

    {
        let sheet = h.state().session.active().unwrap().wb.active().unwrap();
        assert_eq!(sheet.cell(cell_b1).unwrap().value, Value::from(original), "copy preserves the original CRLF/LF bytes");
        assert_eq!(sheet.cell(cell_b2).unwrap().input_text(), formula, "copy preserves formula source");
    }

    // Undo paste
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    {
        let sheet = h.state().session.active().unwrap().wb.active().unwrap();
        assert!(sheet.cell(cell_b1).is_none_or(|cell| cell.value.is_empty()));
        assert!(sheet.cell(cell_b2).is_none_or(|cell| cell.value.is_empty()));
        assert_eq!(sheet.cell(cell_a1).unwrap().value, Value::from(original));
        assert_eq!(sheet.cell(cell_a2).unwrap().input_text(), formula);
    }

    // Redo paste
    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
    {
        let sheet = h.state().session.active().unwrap().wb.active().unwrap();
        assert_eq!(sheet.cell(cell_b1).unwrap().value, Value::from(original));
        assert_eq!(sheet.cell(cell_b2).unwrap().input_text(), formula);
    }

    // XLSX round-trip keeps both the raw value and the formula source after rendering and undo/redo.
    let wb = h.state().session.active().unwrap().wb.clone();
    let bytes = gridcraft_xlsx::write_xlsx(&wb).expect("write xlsx");
    let (reopened, _) = gridcraft_xlsx::read_xlsx(&bytes).expect("read xlsx");
    let reopened_sheet = reopened.active().unwrap();
    assert_eq!(reopened_sheet.cell(cell_a1).unwrap().value, Value::from(original), "XLSX preserves source LF/CRLF and trailing newline bytes");
    assert_eq!(reopened_sheet.cell(cell_a2).unwrap().input_text(), formula);
    assert_eq!(reopened_sheet.cell(cell_b1).unwrap().value, Value::from(original));
    assert_eq!(reopened_sheet.cell(cell_b2).unwrap().input_text(), formula);
}

#[test]
fn test_non_thai_and_unwrapped_regression() {
    let mut s = Session::new();
    s.new_workbook();
    // English wrapped text
    s.execute("cell.set", json!({"cell": "A1", "input": "Quick brown fox jumps"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"align": {"wrap": true}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A", "width": 50})).unwrap();

    // Numbers & dates: should not enter Thai wrapping
    s.execute("cell.set", json!({"cell": "A2", "input": "12345.67"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A2", "style": {"num_fmt": "#,##0.00", "align": {"wrap": true}}})).unwrap();

    // Rotated text
    s.execute("cell.set", json!({"cell": "A3", "input": "สวัสดี"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A3", "style": {"align": {"rotation": 45, "wrap": true}}})).unwrap();

    let h = harness_with_session(s);
    let texts = painted_texts(&h);

    assert!(texts.iter().any(|t| t == "Quick brown fox jumps"), "English wrapped uses standard LayoutJob");
    assert!(texts.iter().any(|t| t == "12,345.67"), "Formatted number rendered");
}

#[test]
fn thai_out_of_scope_alignments_and_rotation_255_stay_on_legacy_renderer() {
    let mut session = Session::new();
    session.new_workbook();
    let cases = [
        ("A1", json!({"wrap": true, "rotation": 255})),
        ("A2", json!({"wrap": true, "h": "CenterAcross"})),
        ("A3", json!({"wrap": true, "h": "Justify"})),
        ("A4", json!({"wrap": true, "h": "Distributed"})),
        ("A5", json!({"wrap": true, "v": "Justify"})),
    ];
    session.execute("home.columnWidth", json!({"cols": "A", "width": 60})).unwrap();
    for (cell, alignment) in cases {
        session.execute("cell.set", json!({"cell": cell, "input": "ข้อความไทยวันนี้อากาศดีมาก"})).unwrap();
        session.execute("home.formatCells", json!({"range": cell, "style": {"align": alignment}})).unwrap();
    }

    let mut h = harness_with_session_config(session, None, 1.0, 0);
    thai_layout::begin_frame_budget(&h.ctx);
    h.step();
    let usage = thai_layout::frame_budget_usage(&h.ctx);
    assert_eq!(
        usage.segmentations, 0,
        "rotation 255, CenterAcross, Justify, Distributed, and vertical Justify must not enter the Thai adapter: {usage:?}"
    );
    assert_eq!(usage.measurements, 0, "all out-of-scope cases must keep legacy measurement");
    assert!(painted_texts(&h).iter().any(|text| text.contains("ข้อความไทย")), "legacy-rendered Thai text remains visible");
}

#[test]
fn test_thai_alignments_and_indent() {
    let mut s = Session::new();
    s.new_workbook();

    // Center alignment
    s.execute("cell.set", json!({"cell": "A1", "input": "วันนี้อากาศดีมาก"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A1", "style": {"align": {"wrap": true, "h": "Center", "v": "Center"}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A", "width": 45})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1", "height": 60})).unwrap();

    // Right alignment + indent
    s.execute("cell.set", json!({"cell": "A2", "input": "วันนี้อากาศดีมาก"})).unwrap();
    s.execute("home.formatCells", json!({"range": "A2", "style": {"align": {"wrap": true, "h": "Right", "indent": 1}}})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A", "width": 45})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "2", "height": 60})).unwrap();

    let h = harness_with_session(s);
    let texts = painted_texts(&h);

    assert!(texts.iter().any(|t| t == "วันนี้"));
    assert!(texts.iter().any(|t| t == "อากาศ" || t == "อากาศดีมาก"));
}

#[test]
fn thai_grid_geometry_tracks_alignment_indent_decorations_zoom_and_dpi() {
    let horizontal = ["General", "Left", "Center", "Right"];
    let vertical = ["Top", "Center", "Bottom"];
    let cells = [["A1", "B1", "C1", "D1"], ["A2", "B2", "C2", "D2"], ["A3", "B3", "C3", "D3"]];

    for (zoom, pixels_per_point) in [(100, 1.0), (100, 2.0), (50, 1.0), (200, 1.0)] {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("home.columnWidth", json!({"cols": "A:F", "width": 60})).unwrap();
        session.execute("home.rowHeight", json!({"rows": "1:3", "height": 55})).unwrap();
        for (row_index, row) in cells.iter().enumerate() {
            for (col_index, cell) in row.iter().enumerate() {
                session.execute("cell.set", json!({"cell": cell, "input": "วันนี้อากาศดีมาก"})).unwrap();
                session
                    .execute(
                        "home.formatCells",
                        json!({"range": cell, "style": {"align": {"wrap": true, "h": horizontal[col_index], "v": vertical[row_index], "indent": 1}}}),
                    )
                    .unwrap();
            }
        }

        // Autofilter reserves its button at the right edge of a wrapped Thai header.
        session.execute("cell.set", json!({"cell": "E1", "input": "วันนี้อากาศดีมาก"})).unwrap();
        session.execute("cell.set", json!({"cell": "E2", "input": "ข้อมูล"})).unwrap();
        session.execute("home.formatCells", json!({"range": "E1", "style": {"align": {"wrap": true, "h": "Left", "v": "Top"}}})).unwrap();
        session.execute("data.filter", json!({"range": "E1:E2"})).unwrap();

        // Icon sets are numeric by design. A Thai literal in the number format lets the test check
        // the renderer's actual conditional-format icon reservation while numeric wrapping remains
        // on the legacy path.
        session.execute("cell.set", json!({"cell": "F1", "input": "42"})).unwrap();
        session
            .execute(
                "home.formatCells",
                json!({"range": "F1", "style": {"num_fmt": "\"รายการ \"0", "align": {"wrap": true, "h": "Left", "v": "Top"}}}),
            )
            .unwrap();
        session.execute("home.conditionalFormat", json!({"range": "F1", "rule": {"type": "iconSet", "set": "3Arrows"}})).unwrap();

        let mut h = harness_with_session_config(session, Some(zoom), pixels_per_point, 0);
        thai_layout::begin_frame_budget(&h.ctx);
        h.step();
        let usage = thai_layout::frame_budget_usage(&h.ctx);
        assert!(
            usage.segmentations > 0 && usage.measurements > 0,
            "the grid renderer must call the Thai adapter at zoom {zoom}% / {pixels_per_point}x: {usage:?}"
        );
        h.run_steps(3);

        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        let shapes = painted_text_shapes(&h);
        let z = geo.z;
        let pad = 2.5 * z;
        let indent = 9.0 * z;

        for (row_index, row) in cells.iter().enumerate() {
            for (col_index, cell) in row.iter().enumerate() {
                let cell_ref = CellRef::parse(cell).unwrap();
                let rect = geo.cell_rect(sheet, cell_ref);
                let mut lines: Vec<_> = shapes
                    .iter()
                    .filter(|shape| thai_layout::has_thai_chars(&shape.text) && shape.clip.intersect(rect.shrink(0.1)).is_positive())
                    .collect();
                lines.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
                assert!(lines.len() >= 2, "{cell} must paint wrapped Thai lines in its real cell rect {rect:?}: {lines:?}");
                for line in &lines {
                    assert_near(line.clip.left(), rect.left(), &format!("{cell} clip left"));
                    assert_near(line.clip.right(), rect.right(), &format!("{cell} clip right"));
                    assert_near(line.clip.top(), rect.top(), &format!("{cell} clip top"));
                    assert_near(line.clip.bottom(), rect.bottom(), &format!("{cell} clip bottom"));
                }

                let total_height: f32 = lines.iter().map(|line| line.size.y).sum();
                let y_top = rect.top() + 1.5 * z;
                let y_bottom = rect.bottom() - total_height - 1.5 * z;
                let mut expected_y = match vertical[row_index] {
                    "Top" => y_top,
                    "Bottom" => y_bottom,
                    _ => (y_top + y_bottom) / 2.0,
                };
                for line in &lines {
                    let line_width = line.size.x;
                    let expected_x = match horizontal[col_index] {
                        "Center" => (rect.left() + pad + rect.right() - pad - line_width) / 2.0,
                        "Right" => rect.right() - pad - indent - line_width,
                        _ => rect.left() + pad + indent,
                    };
                    assert_near(line.pos.x, expected_x, &format!("{cell} {:?} x", horizontal[col_index]));
                    assert_near(line.pos.y, expected_y, &format!("{cell} {:?} y", vertical[row_index]));
                    expected_y += line.size.y;
                }
            }
        }

        let filter_cell = CellRef::parse("E1").unwrap();
        let filter_rect = geo.cell_rect(sheet, filter_cell);
        let reserved_right = filter_rect.right() - sheet.filter_button_space(filter_cell) * z;
        assert!(sheet.filter_button_space(filter_cell) > 0.0, "E1 must have a real filter button reservation");
        let filter_lines: Vec<_> = shapes
            .iter()
            .filter(|shape| thai_layout::has_thai_chars(&shape.text) && shape.clip.intersect(filter_rect.shrink(0.1)).is_positive())
            .collect();
        assert!(!filter_lines.is_empty(), "filter header must paint Thai lines");
        for line in filter_lines {
            assert_near(line.clip.right(), reserved_right, "filter button reserved-space clip");
        }

        let icon_cell = CellRef::parse("F1").unwrap();
        let icon_cell_rect = geo.cell_rect(sheet, icon_cell);
        let icon_look = gridcraft_engine::cf::CfCache::new().look(&app.session.active().unwrap().wb, 0, icon_cell).unwrap();
        assert!(icon_look.icon.is_some(), "F1 must receive a conditional-format icon");
        let icon_text = shapes.iter().find(|shape| shape.text.contains("รายการ")).expect("Thai-literal numeric format is painted");
        assert!(
            icon_text.pos.x >= icon_cell_rect.left() + 17.0 * z,
            "CF icon reservation shifts text from the cell's left edge: {icon_text:?}, {icon_cell_rect:?}"
        );

        if zoom == 100 && pixels_per_point == 1.0 {
            let icon_rect =
                Rect::from_min_size(egui::pos2(icon_cell_rect.left() + 2.0, icon_cell_rect.center().y - 6.0 * z), vec2(12.0 * z, 12.0 * z));
            assert!(
                h.output().shapes.iter().any(|shape| {
                    matches!(&shape.shape, Shape::Path(path) if path.fill != Color32::TRANSPARENT)
                        && icon_rect.contains_rect(shape.shape.visual_bounding_rect())
                }),
                "the actual conditional-format icon is painted in its reserved area {icon_rect:?}"
            );
            save_task3_snapshot(&mut h, "thai_rendering_task3_geometry.png");
        }
    }
}
