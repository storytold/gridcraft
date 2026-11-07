//! Mixed-script baselines and clipping, without starting the installed-font scanner.

use egui::{Color32, Context, FontFamily, FontId};
use gridcraft_engine::{Session, core::CellRef};
use gridcraft_ui_egui::{SheetApp, i18n::Language, theme};
use serde_json::json;

fn activate(ctx: &Context, definitions: egui::FontDefinitions) {
    ctx.set_fonts(definitions);
    let mut out = ctx.run_ui(Default::default(), |_| {});
    out.textures_delta.clear();
}

/// Allow deferred font binding and editor layout to settle before inspecting painted shapes.
fn frames(ctx: &Context, app: &mut SheetApp) -> egui::FullOutput {
    let mut out = egui::FullOutput::default();
    for _ in 0..4 {
        out = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 900.0))), ..Default::default() },
            |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
        );
        out.textures_delta.clear();
    }
    out
}

/// Check layout baselines rather than bitmap bounds, which differ between scripts.
fn assert_shared_baselines(galley: &egui::Galley) {
    for row in &galley.rows {
        if let Some(first) = row.glyphs.first() {
            for glyph in &row.glyphs {
                assert!((glyph.pos.y - first.pos.y).abs() < 0.01, "{:?} is off the shared baseline in {:?}", glyph.chr, galley.text());
            }
        }
    }
}

fn primary_text_bounds(galley: &egui::Galley) -> (f32, f32) {
    let first = galley.rows.iter().find_map(|row| row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y - glyph.font_ascent));
    let last =
        galley.rows.iter().rev().find_map(|row| row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y - glyph.font_ascent + glyph.font_height));
    first.zip(last).unwrap_or((0.0, galley.size().y))
}

/// Exercise fallback alignment across cell styles, title fonts, zoom, and automatic wrapping.
#[test]
fn cjk_and_latin_share_a_baseline_in_cells_and_window_titles() {
    let ctx = Context::default();
    ctx.set_pixels_per_point(2.0);
    activate(&ctx, theme::font_definitions_for_language(Language::Zh));
    for role in [theme::CELL, theme::CELL_BOLD, theme::CELL_ITALIC, theme::CELL_BOLD_ITALIC, theme::UI_BOLD] {
        for zoom in [0.8, 1.0, 2.8] {
            let font = FontId::new(if role == theme::UI_BOLD { 13.0 } else { 16.0 * zoom }, FontFamily::Name(role.into()));
            if !ctx.fonts_mut(|f| "中文".chars().all(|c| f.has_glyph(&font, c))) {
                eprintln!("skipped {role} baseline: no CJK face");
                break;
            }
            for width in [f32::INFINITY, 100.0 * zoom] {
                let galley = ctx.fonts_mut(|f| f.layout("中文ABC2026中文ABC2026".into(), font.clone(), Color32::BLACK, width));
                assert_shared_baselines(&galley);
            }
        }
    }
}

/// Baseline alignment must not override workbook row heights, clipping, or editor sizing.
#[test]
fn mixed_script_wrapping_keeps_fixed_row_clips_alignment_and_editor_height() {
    const TEXT: &str = "中文ABC2026\n文字xyz123";
    for zoom in [80, 100, 280] {
        let ctx = Context::default();
        ctx.set_pixels_per_point(2.0);
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range":"A1:C1", "values":[[TEXT, TEXT, TEXT]]})).unwrap();
        session.execute("home.columnWidth", json!({"cols":"A:C", "width":180})).unwrap();
        session.execute("home.rowHeight", json!({"rows":"1:1", "height":32})).unwrap();
        for (cell, align) in [("A1", "Top"), ("B1", "Center"), ("C1", "Bottom")] {
            session.execute("home.formatCells", json!({"range":cell, "style":{"align":{"wrap":true, "v":align}}})).unwrap();
        }
        session.execute("view.zoom", json!({"percent":zoom})).unwrap();
        let mut app = SheetApp::new(session, Default::default());
        let out = frames(&ctx, &mut app);
        let font = {
            let doc = app.session.active().unwrap();
            doc.wb.styles.get(doc.wb.active().unwrap().style_id(CellRef::new(0, 0))).font.clone()
        };
        let id = FontId::new(font.size * 96.0 / 72.0 * zoom as f32 / 100.0, theme::cell_family(&font.name, font.bold, font.italic));
        if !ctx.fonts_mut(|f| "中文字".chars().all(|c| f.has_glyph(&id, c))) {
            eprintln!("skipped CJK grid/editor layout: no CJK face");
            return;
        }
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
        assert_eq!(sheet.row_height(0), 32.0, "rendering must preserve the manual row height");
        for col in 0..3 {
            let rect = geo.cell_rect(sheet, CellRef::new(0, col));
            let (shape, clip) = out
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == TEXT && (t.pos.x - rect.left()).abs() < 5.0 * geo.z => Some((t, s.clip_rect)),
                    _ => None,
                })
                .expect("cell text is painted");
            assert_shared_baselines(&shape.galley);
            assert_eq!(shape.galley.rows.len(), 2);
            assert!(clip.top() >= rect.top() && clip.bottom() <= rect.bottom(), "fixed-height cells clip their own text");
            let (text_top, text_bottom) = primary_text_bounds(&shape.galley);
            let expected_top = match col {
                0 => rect.top() + 1.5 * geo.z,
                1 => rect.center().y - (text_bottom - text_top) / 2.0 + 0.5 * geo.z,
                _ => rect.bottom() - (text_bottom - text_top) - 0.5 * geo.z,
            };
            assert!((shape.pos.y + text_top - expected_top).abs() < 1.0, "vertical alignment stays effective at {zoom}%");
        }
        app.begin_edit(None, false);
        let out = frames(&ctx, &mut app);
        let field = ctx.read_response(egui::Id::unique("gridcraft.cell_editor")).expect("editor is visible").rect;
        let editor = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == TEXT && field.contains(t.pos) => Some(t),
                _ => None,
            })
            .expect("editor text is painted");
        assert!(field.height() >= editor.galley.size().y, "the editor grows to fit its painted CJK lines at {zoom}%");
        assert_shared_baselines(&editor.galley);
    }
}

#[test]
fn same_row_cells_keep_their_baseline_with_baseline_aligned_fallbacks() {
    for wraps in [[false, false], [true, true], [false, true], [true, false]] {
        for valign in ["Top", "Bottom", "Center"] {
            let mixed = if wraps[1] && valign == "Top" { "Hello\n中文" } else { "Hello 中文" };
            let ctx = Context::default();
            ctx.set_pixels_per_point(2.0);
            activate(&ctx, theme::font_definitions_for_language(Language::Zh));
            let mut session = Session::new();
            session.new_workbook();
            session.execute("range.setValues", json!({"range":"A1:B1", "values":[[1, mixed]]})).unwrap();
            session.execute("home.columnWidth", json!({"cols":"A:B", "width":180})).unwrap();
            for (cell, wrap) in ["A1", "B1"].into_iter().zip(wraps) {
                session.execute("home.formatCells", json!({"range":cell, "style":{"align":{"wrap":wrap, "v":valign}}})).unwrap();
            }
            let font = {
                let doc = session.active().unwrap();
                doc.wb.styles.get(doc.wb.active().unwrap().style_id(CellRef::new(0, 0))).font.clone()
            };
            let font_id = FontId::new(font.size * 96.0 / 72.0, theme::cell_family(&font.name, font.bold, font.italic));
            if !ctx.fonts_mut(|fonts| fonts.has_glyph(&font_id, '中')) {
                eprintln!("skipped same-row fallback baseline: no CJK face");
                return;
            }
            let mut app = SheetApp::new(session, Default::default());
            let out = frames(&ctx, &mut app);
            let sheet = app.session.active().unwrap().wb.active().unwrap();
            let geo = gridcraft_ui_egui::grid::Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
            let mut baselines = Vec::new();
            for (col, text) in ["1", mixed].iter().enumerate() {
                let rect = geo.cell_rect(sheet, CellRef::new(0, col as u32));
                let shape = out
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t)
                            if t.galley.text() == *text && t.visual_bounding_rect().intersect(s.clip_rect).intersect(rect).is_positive() =>
                        {
                            Some(t)
                        }
                        _ => None,
                    })
                    .expect("cell text is painted");
                assert_shared_baselines(&shape.galley);
                if valign == "Top" {
                    let (text_top, _) = primary_text_bounds(&shape.galley);
                    assert!(shape.pos.y + text_top > rect.top(), "top-aligned text keeps space inside the cell above its primary line box");
                }
                let row = shape.galley.rows.first().expect("cell text has a row");
                let glyph = row.glyphs.first().expect("first-line baseline is present");
                if text.contains('中') {
                    assert_eq!(shape.galley.rows.len(), if mixed.contains('\n') { 2 } else { 1 });
                    assert!(
                        shape
                            .galley
                            .rows
                            .iter()
                            .flat_map(|row| &row.glyphs)
                            .any(|glyph| { glyph.chr == '中' && glyph.font_face_align == egui::epaint::text::FallbackAlign::Baseline }),
                        "CJK fallback must use baseline alignment"
                    );
                }
                baselines.push(shape.pos.y + row.pos.y + glyph.pos.y);
            }
            assert!(
                (baselines[0] - baselines[1]).abs() < 0.6,
                "same-row cells with wraps={wraps:?}, {valign} alignment share a baseline: {baselines:?}"
            );
        }
    }
}
