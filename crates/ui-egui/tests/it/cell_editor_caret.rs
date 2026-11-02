//! The caret of the in-cell editor stays visible on the cell's fill, whatever the theme.
use egui::{Color32, Context, RawInput, Rect, Shape, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, theme};
use serde_json::json;

/// Run a full application frame so editor colour assertions inspect the painted UI.
fn frame(ctx: &Context, app: &mut SheetApp) -> egui::FullOutput {
    let mut output =
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), ..Default::default() }, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        });
    output.textures_delta.clear();
    output
}

/// The caret painted while editing A1 over `fill` (`None`: no fill), with the app in `dark` mode.
fn caret(fill: Option<&str>, dark: bool) -> Color32 {
    let ctx = Context::default();
    theme::apply(&ctx, dark);
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    app.ui.dark = dark;
    if let Some(color) = fill {
        app.run("home.fillColor", json!({"range": "A1", "color": color})).unwrap();
    }
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    app.begin_edit(Some("abc".into()), false);
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    let output = frame(&ctx, &mut app);
    // The editor paints its backdrop first; the caret is the vertical segment after it.
    let paper = fill.map_or(if dark { Color32::BLACK } else { Color32::WHITE }, |c| Color32::from_hex(c).unwrap());
    let backdrop = output
        .shapes
        .iter()
        .rposition(|s| matches!(&s.shape, Shape::Rect(r) if r.fill == paper && r.rect.width() > 20.0))
        .expect("editor backdrop painted");
    let text = output.shapes[backdrop..]
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == "abc" => Some(t),
            _ => None,
        })
        .expect("editor text painted");
    let ink = if paper == Color32::BLACK || fill == Some("#1F3864") { Color32::WHITE } else { Color32::BLACK };
    assert!(text.galley.job.sections.iter().all(|s| s.format.color == ink), "editor text contrasts with fill");
    output.shapes[backdrop..]
        .iter()
        .find_map(|s| match &s.shape {
            Shape::LineSegment { points, stroke } if points[0].x == points[1].x => Some(stroke.color),
            _ => None,
        })
        .expect("caret painted")
}

/// Automatic editor text and its caret stay readable on default, dark and saturated fills.
#[test]
fn caret_contrasts_with_the_cell_fill() {
    assert_eq!(caret(Some("#1F3864"), false), Color32::WHITE, "dark fill, light theme");
    assert_eq!(caret(Some("#1F3864"), true), Color32::WHITE, "dark fill, dark theme");
    assert_eq!(caret(Some("#FFFF00"), true), Color32::BLACK, "light fill, dark theme");
    assert_eq!(caret(Some("#00FF00"), false), Color32::BLACK, "bright green fill, light theme");
    assert_eq!(caret(Some("#00FF00"), true), Color32::BLACK, "bright green fill, dark theme");
    assert_eq!(caret(None, true), Color32::WHITE, "no fill, dark theme");
    assert_eq!(caret(None, false), Color32::BLACK, "no fill, light theme");
}

/// Switching light to dark and back repaints both ordinary text and merged-cell backdrops.
#[test]
fn ordinary_and_merged_cells_repaint_when_the_theme_changes() {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B2", "input": "Ordinary"})).unwrap();
    session.execute("cell.set", json!({"cell": "D2", "input": "Merged"})).unwrap();
    session.execute("home.mergeCells", json!({"range": "D2:E2"})).unwrap();
    let mut app = crate::common::app(session);
    for dark in [false, true, false] {
        theme::apply(&ctx, dark);
        app.ui.dark = dark;
        for _ in 0..4 {
            frame(&ctx, &mut app);
        }
        let output = frame(&ctx, &mut app);
        let (paper, ink) = if dark { (Color32::BLACK, Color32::WHITE) } else { (Color32::WHITE, Color32::BLACK) };
        for label in ["Ordinary", "Merged"] {
            let text = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    Shape::Text(t) if t.galley.job.text == label => Some(t),
                    _ => None,
                })
                .expect("cell text painted");
            assert!(text.galley.job.sections.iter().all(|s| s.format.color == ink), "{label}, dark={dark}");
            if label == "Merged" {
                assert!(
                    output.shapes.iter().any(|s| matches!(&s.shape, Shape::Rect(r)
                    if r.fill == paper && r.rect.contains(text.pos) && r.rect.width() < 400.0 && r.rect.height() < 50.0)),
                    "merged cell backdrop, dark={dark}"
                );
            }
        }
    }
}

/// Check the resolved editor fill and text independently from its high-contrast caret.
fn assert_editor_colors(session: Session, dark: bool, fill: Color32, ink: Color32) {
    let ctx = Context::default();
    theme::apply(&ctx, dark);
    let mut app = crate::common::app(session);
    app.ui.dark = dark;
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    app.begin_edit(Some("abc".into()), false);
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    let output = frame(&ctx, &mut app);
    let rect = ctx.data(|d| d.get_temp::<Rect>(egui::Id::unique("gridcraft.cell_editor.rect"))).expect("editor rect");
    let backdrop = output.shapes.iter().rposition(|s| matches!(&s.shape, Shape::Rect(r) if r.rect == rect)).expect("editor backdrop");
    let Shape::Rect(backdrop_shape) = &output.shapes[backdrop].shape else { unreachable!() };
    assert_eq!(backdrop_shape.fill, fill, "resolved editor fill, dark={dark}");
    let text = output.shapes[backdrop..]
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == "abc" => Some(t),
            _ => None,
        })
        .expect("editor text");
    assert!(text.galley.job.sections.iter().all(|s| s.format.color == ink), "resolved editor text, dark={dark}");
    let caret = output.shapes[backdrop..]
        .iter()
        .find_map(|s| match &s.shape {
            Shape::LineSegment { points, stroke } if points[0].x == points[1].x => Some(stroke.color),
            _ => None,
        })
        .expect("editor caret");
    assert!(matches!(caret, Color32::BLACK | Color32::WHITE), "caret uses contrasting black or white, dark={dark}");
    assert!(theme::contrast(caret, fill) >= 4.5, "caret must remain visible independently of text colour, dark={dark}");
}

/// Editing retains an explicitly chosen font colour while keeping the caret visible.
#[test]
fn editor_preserves_explicit_font_color() {
    for dark in [false, true] {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("home.fillColor", json!({"range":"A1", "color":"#FFFF00"})).unwrap();
        session.execute("home.fontColor", json!({"range":"A1", "color":"#FF0000"})).unwrap();
        assert_editor_colors(session, dark, Color32::YELLOW, Color32::RED);
    }
}

/// Conditional fill and font colours must survive entering edit mode in either theme.
#[test]
fn editor_preserves_conditional_fill_and_font_color() {
    for dark in [false, true] {
        let mut session = Session::new();
        session.new_workbook();
        session
            .execute(
                "home.conditionalFormat",
                json!({"range":"A1", "rule":{
                    "type":"expression", "formula":"=TRUE", "style":{"fill":"#00FF00", "font":{"color":"#FF0000"}}
                }}),
            )
            .unwrap();
        assert_editor_colors(session, dark, Color32::GREEN, Color32::RED);
    }
}

/// Editing a cell retains its colour-scale result instead of reverting to the base fill.
#[test]
fn editor_preserves_color_scale_fill() {
    for dark in [false, true] {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range":"A1", "values":[[1],[10]]})).unwrap();
        session.execute("home.conditionalFormat", json!({"range":"A1:A2", "rule":{"type":"colorScale"}})).unwrap();
        assert_editor_colors(session, dark, Color32::from_rgb(0xF8, 0x69, 0x6B), Color32::BLACK);
    }
}

/// Table header fill and font colours apply to the editor as they do to the grid.
#[test]
fn editor_preserves_table_header_fill_and_font_color() {
    for dark in [false, true] {
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range":"A1", "values":[["Header"],[1]]})).unwrap();
        session.execute("insert.table", json!({"range":"A1:A2", "header":true})).unwrap();
        let wb = &session.active().unwrap().wb;
        let cell = gridcraft_engine::core::CellRef::new(0, 0);
        let table = wb.sheet(0).unwrap().table_at(cell).unwrap();
        let style = gridcraft_engine::tables::table_cell_style(wb, 0, table, cell);
        let fill = theme::color32(style.fill.fg.resolve(&wb.theme).expect("table header fill"));
        let ink = theme::color32(style.font.color.resolve(&wb.theme).expect("table header ink"));
        assert_editor_colors(session, dark, fill, ink);
    }
}

/// The editor matches the grid's blended approximation of a patterned workbook fill.
#[test]
fn editor_preserves_patterned_fill() {
    use gridcraft_engine::model::{Color, Fill, PatternType, Style};
    for dark in [false, true] {
        let mut session = Session::new();
        session.new_workbook();
        let wb = std::sync::Arc::make_mut(&mut session.active_mut().unwrap().wb);
        let style = Style { fill: Fill { pattern: PatternType::Gray50, fg: Color::Rgb(0), bg: Color::Rgb(0xFFFFFF) }, ..Default::default() };
        let id = wb.styles.intern(style);
        wb.sheet_mut(0).unwrap().set_style(gridcraft_engine::core::CellRef::new(0, 0), id);
        // The grid approximates patterns with one part foreground to three parts background.
        assert_editor_colors(session, dark, Color32::from_gray(191), Color32::BLACK);
    }
}

/// Explicit text may intentionally match its fill; the editing caret must still be visible.
#[test]
fn editor_caret_contrasts_when_explicit_font_matches_fill() {
    for dark in [false, true] {
        for color in ["#000000", "#FFFFFF"] {
            let mut session = Session::new();
            session.new_workbook();
            session.execute("home.fillColor", json!({"range":"A1", "color":color})).unwrap();
            session.execute("home.fontColor", json!({"range":"A1", "color":color})).unwrap();
            let color = Color32::from_hex(color).unwrap();
            assert_editor_colors(session, dark, color, color);
        }
    }
}
