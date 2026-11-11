//! Mixed-script baselines and clipping, without starting the installed-font scanner.

use egui::{Color32, Context, FontFamily, FontId};
use gridcraft_engine::{Session, core::CellRef};
use gridcraft_ui_egui::{SheetApp, theme};
use serde_json::json;

fn activate(ctx: &Context, definitions: egui::FontDefinitions) {
    ctx.set_fonts(definitions);
    let mut out = ctx.run_ui(Default::default(), |_| {});
    out.textures_delta.clear();
}

/// Allow deferred font binding and editor layout to settle before inspecting painted shapes.
fn frames(ctx: &Context, app: &mut SheetApp) -> egui::FullOutput {
    frames_at_native_scale(ctx, app, 2.0)
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
    activate(&ctx, theme::pinned_font_definitions());
    for role in [theme::CELL, theme::CELL_BOLD, theme::CELL_ITALIC, theme::CELL_BOLD_ITALIC, theme::UI_BOLD] {
        for zoom in [0.8, 1.0, 2.8] {
            let font = FontId::new(if role == theme::UI_BOLD { 13.0 } else { 16.0 * zoom }, FontFamily::Name(role.into()));
            if !ctx.fonts_mut(|f| "中文".chars().all(|c| f.has_glyph(&font, c))) {
                assert!(!theme::craft_fonts_required(), "required CJK fixture is missing for {role}");
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
        activate(&ctx, theme::pinned_font_definitions());
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
        app.use_prepared_fonts();
        let out = frames(&ctx, &mut app);
        let font = {
            let doc = app.session.active().unwrap();
            doc.wb.styles.get(doc.wb.active().unwrap().style_id(CellRef::new(0, 0))).font.clone()
        };
        let id = FontId::new(font.size * 96.0 / 72.0 * zoom as f32 / 100.0, theme::cell_family(&font.name, font.bold, font.italic));
        if !ctx.fonts_mut(|f| "中文字".chars().all(|c| f.has_glyph(&id, c))) {
            assert!(!theme::craft_fonts_required(), "required CJK fixture is missing");
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
            activate(&ctx, theme::pinned_font_definitions());
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
                assert!(!theme::craft_fonts_required(), "required CJK fixture is missing");
                eprintln!("skipped same-row fallback baseline: no CJK face");
                return;
            }
            let mut app = SheetApp::new(session, Default::default());
            app.use_prepared_fonts();
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

fn frames_at_native_scale(ctx: &Context, app: &mut SheetApp, native_ppp: f32) -> egui::FullOutput {
    let mut out = egui::FullOutput::default();
    for _ in 0..4 {
        let mut raw =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1800.0, 900.0))), ..Default::default() };
        raw.viewports.get_mut(&egui::ViewportId::ROOT).expect("root viewport").native_pixels_per_point = Some(native_ppp);
        out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
    }
    assert!((ctx.zoom_factor() - 1.0).abs() < 1e-6);
    assert!((ctx.pixels_per_point() - native_ppp).abs() < 1e-6);
    assert!((out.pixels_per_point - native_ppp).abs() < 1e-6);
    out
}

// Every fixture row starts in the primary Latin face. Compute the expected span
// independently of the production row_primary_bounds helper.
fn fixture_line_span(row: &egui::epaint::text::PlacedRow) -> (f32, f32) {
    let g = row.glyphs.first().expect("nonempty fixture row");
    assert!(g.chr.is_ascii_alphabetic());
    assert_eq!(g.font_face_ascent, g.font_ascent);
    assert_eq!(g.font_face_height, g.font_height);
    let top = row.pos.y + g.pos.y - g.font_ascent;
    (top, top + g.font_height)
}

fn cjk_fixture_shape(app: &SheetApp, out: &egui::FullOutput, text: &str) -> (egui::epaint::TextShape, egui::Rect, egui::Rect, f32) {
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = gridcraft_ui_egui::grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let cell = geo.cell_rect(sh, CellRef::new(0, 0));
    assert!(geo.cells.contains_rect(cell), "the whole fixture cell must be inside the viewport");
    let (shape, clip) = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == text && (t.pos.x - cell.left()).abs() < 5.0 * geo.z => Some((t.clone(), s.clip_rect)),
            _ => None,
        })
        .expect("A1 text shape must exist even with an empty clip");
    (shape, clip, cell, geo.z)
}

#[test]
fn fractional_scale_preserves_fitting_baseline_cjk_lines() {
    const TEXT: &str = "ABC中文\nDEF中文\nGHI中文";
    const FIT: f32 = 0.01;
    const EPS: f32 = 0.0001;
    if !theme::chinese_font_loaded() {
        assert!(!theme::craft_fonts_required(), "required pinned Chinese font is missing");
        eprintln!("skipped pinned CJK clipping: build without craft-fonts");
        return;
    }
    assert!(theme::craft_fonts_cover("中文"), "pinned fixture must map the Han characters");
    let mut fractional_witnesses = 0;

    for native_ppp in [1.008_333_3, 1.0, 1.25, 1.5, 2.0] {
        for zoom in [80, 100, 280] {
            let ctx = Context::default();
            activate(&ctx, theme::pinned_font_definitions());
            let mut s = Session::new();
            s.new_workbook();
            s.execute("cell.set", json!({"cell":"A1", "input":TEXT})).unwrap();
            s.execute("home.columnWidth", json!({"cols":"A:A", "width":180})).unwrap();
            s.execute(
                "home.formatCells",
                json!({
                    "range":"A1", "style":{"align":{"wrap":true,"v":"Top"}}
                }),
            )
            .unwrap();
            s.execute("view.zoom", json!({"percent":zoom})).unwrap();
            // Set the manual height after wrapping so AutoFit cannot replace it.
            s.execute("home.rowHeight", json!({"rows":"1:1","height":100})).unwrap();
            s.execute("selection.set", json!({"range":"B20"})).unwrap();
            let mut app = SheetApp::new(s, Default::default());
            app.use_prepared_fonts();
            let out = frames_at_native_scale(&ctx, &mut app, native_ppp);
            let (probe, _, initial_cell, z) = cjk_fixture_shape(&app, &out, TEXT);
            assert_eq!(probe.galley.rows.len(), 3);
            assert_shared_baselines(&probe.galley);
            assert!((probe.galley.pixels_per_point - native_ppp).abs() < 1e-6);
            for row in &probe.galley.rows {
                assert!(
                    row.glyphs.iter().any(|g| { g.chr == '中' && g.font_face_align == egui::epaint::text::FallbackAlign::Baseline }),
                    "every row must exercise a baseline-aligned CJK fallback"
                );
            }

            // Exercise no fitting rows, one fitting row, and all fitting rows.
            // Put the cell edge between the two fit criteria when they differ.
            for expected_rows in [0, 1, 3] {
                let boundary = if expected_rows == 0 {
                    let (top, bottom) = fixture_line_span(&probe.galley.rows[0]);
                    probe.pos.y + (top + bottom) * 0.5
                } else {
                    let row = &probe.galley.rows[expected_rows - 1];
                    let primary = probe.pos.y + fixture_line_span(row).1;
                    let expanded = probe.pos.y + row.rect().bottom();
                    let gap = expanded - primary;
                    if gap > 8.0 * EPS {
                        if native_ppp == 1.008_333_3 {
                            fractional_witnesses += 1;
                        }
                        primary - FIT + gap.min(FIT) * 0.5
                    } else {
                        // Still check preservation when the two metrics do not measurably differ.
                        primary + 2.0 * FIT
                    }
                };
                let height = (boundary - initial_cell.top()) / z;
                app.session
                    .execute(
                        "home.rowHeight",
                        json!({
                            "rows":"1:1", "height":height
                        }),
                    )
                    .unwrap();
                let out = frames_at_native_scale(&ctx, &mut app, native_ppp);
                let (paint, clip, cell, _) = cjk_fixture_shape(&app, &out, TEXT);
                assert_eq!(paint.galley.rows.len(), 3);
                assert_shared_baselines(&paint.galley);
                assert_fixture_clip(&paint, clip, cell, expected_rows, native_ppp, zoom);
            }
        }
    }
    assert!(fractional_witnesses > 0, "fractional fixture did not reproduce the difference between row bounds");
}

/// Assert exact fitting rows and visible ink, rather than merely finding a text shape.
fn assert_fixture_clip(paint: &egui::epaint::TextShape, clip: egui::Rect, cell: egui::Rect, expected_rows: usize, native_ppp: f32, zoom: i32) {
    const FIT: f32 = 0.01;
    const EPS: f32 = 0.0001;
    let spans: Vec<_> = paint
        .galley
        .rows
        .iter()
        .map(|row| {
            let (top, bottom) = fixture_line_span(row);
            (paint.pos.y + top, paint.pos.y + bottom)
        })
        .collect();
    let fitting: Vec<_> = spans.iter().enumerate().filter(|(_, (_, bottom))| *bottom <= cell.bottom() + FIT).map(|(i, _)| i).collect();
    assert_eq!(
        fitting,
        (0..expected_rows).collect::<Vec<_>>(),
        "fixture precondition: ppp={native_ppp}, zoom={zoom}, cell={cell:?}, spans={spans:?}"
    );
    if expected_rows > 0 {
        let index = expected_rows - 1;
        let primary = spans[index].1;
        let expanded = paint.pos.y + paint.galley.rows[index].rect().bottom();
        if expanded - primary > 8.0 * EPS {
            assert!(primary <= cell.bottom() + FIT);
            assert!(expanded > cell.bottom() + FIT, "cell edge must distinguish the old and corrected fit criteria");
        }
    }
    assert!(clip.top() >= cell.top() - EPS);
    assert!(clip.bottom() <= cell.bottom() + EPS);
    let expected_bottom = match fitting.last().copied() {
        None => clip.top(),
        Some(2) => cell.bottom(),
        Some(i) => spans[i].1.min(cell.bottom()),
    };
    assert!(
        (clip.bottom() - expected_bottom).abs() < EPS,
        "clip: ppp={native_ppp}, zoom={zoom}, rows={expected_rows}, clip={clip:?}, expected={expected_bottom}"
    );
    for i in fitting {
        assert!(clip.bottom() + FIT + EPS >= spans[i].1);
        let ink = paint.galley.rows[i].visuals.mesh_bounds.translate(paint.pos.to_vec2() + paint.galley.rows[i].pos.to_vec2());
        assert!(ink.intersect(clip).is_positive(), "row {i} lost all visible ink");
    }
}
