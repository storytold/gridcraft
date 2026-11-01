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

fn assert_shared_baselines(galley: &egui::Galley) {
    for row in &galley.rows {
        if let Some(first) = row.glyphs.first() {
            for glyph in &row.glyphs {
                assert!((glyph.pos.y - first.pos.y).abs() < 0.01, "{:?} is off the shared baseline in {:?}", glyph.chr, galley.text());
            }
        }
    }
}

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
            let expected = match col {
                0 => rect.top() + 1.5 * geo.z,
                1 => rect.center().y - shape.galley.size().y / 2.0,
                _ => rect.bottom() - shape.galley.size().y - 1.5 * geo.z,
            };
            assert!((shape.pos.y - expected).abs() < 1.0, "vertical alignment stays effective at {zoom}%");
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
