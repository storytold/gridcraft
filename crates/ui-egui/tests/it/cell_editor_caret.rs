//! The caret of the in-cell editor stays visible on the cell's fill, whatever the theme.
use egui::{Color32, Context, RawInput, Rect, Shape, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, theme};
use serde_json::json;

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
    let paper = fill.map_or(Color32::WHITE, |c| Color32::from_hex(c).unwrap());
    let backdrop = output
        .shapes
        .iter()
        .rposition(|s| matches!(&s.shape, Shape::Rect(r) if r.fill == paper && r.rect.width() > 20.0))
        .expect("editor backdrop painted");
    output.shapes[backdrop..]
        .iter()
        .find_map(|s| match &s.shape {
            Shape::LineSegment { points, stroke } if points[0].x == points[1].x => Some(stroke.color),
            _ => None,
        })
        .expect("caret painted")
}

#[test]
fn caret_contrasts_with_the_cell_fill() {
    assert_eq!(caret(Some("#1F3864"), false), Color32::WHITE, "dark fill, light theme");
    assert_eq!(caret(Some("#1F3864"), true), Color32::WHITE, "dark fill, dark theme");
    assert_eq!(caret(Some("#FFFF00"), true), Color32::BLACK, "light fill, dark theme");
    assert_eq!(caret(None, true), Color32::BLACK, "no fill, dark theme: the cell is still white paper");
    assert_eq!(caret(None, false), Color32::BLACK, "no fill, light theme");
}

/// The colours of the text the in-cell editor paints while editing A1 (white text on a black
/// fill when `styled`).
fn editor_text_colors(styled: bool) -> Vec<Color32> {
    let ctx = Context::default();
    theme::apply(&ctx, false);
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    if styled {
        app.run("home.fillColor", json!({"range": "A1", "color": "#000000"})).unwrap();
        app.run("home.fontColor", json!({"range": "A1", "color": "#FFFFFF"})).unwrap();
    }
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    app.begin_edit(Some("typed".into()), false);
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    let output = frame(&ctx, &mut app);
    // The formula bar shows the same text above the grid; the in-cell editor's is the lowest.
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.text() == "typed" => Some(t),
            _ => None,
        })
        .max_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .map(|t| t.galley.job.sections.iter().map(|s| s.format.color).collect())
        .unwrap_or_default()
}

#[test]
fn typed_text_takes_the_cell_font_colour_before_it_is_entered() {
    // #3011: white text on a black fill was drawn black (invisible) until Enter.
    let styled = editor_text_colors(true);
    assert!(!styled.is_empty(), "the editor painted its text");
    assert!(styled.iter().all(|&c| c == Color32::WHITE), "the cell's white font colour: {styled:?}");
    let plain = editor_text_colors(false);
    assert!(!plain.is_empty() && plain.iter().all(|&c| c == Color32::BLACK), "automatic stays black: {plain:?}");
}
