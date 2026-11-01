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

/// What the in-cell editor paints while editing A1 with `text`, after `setup`: the colours of
/// its text, and whether a backdrop of `fill` was painted.
fn editor_paint(setup: &dyn Fn(&mut SheetApp), text: &str, fill: Color32) -> (Vec<Color32>, bool) {
    let (chars, backdrop) = editor_chars(setup, text, fill);
    (chars.into_iter().map(|(_, c)| c).collect(), backdrop)
}

/// [`editor_paint`], with the colour of each character of the text.
fn editor_chars(setup: &dyn Fn(&mut SheetApp), text: &str, fill: Color32) -> (Vec<(char, Color32)>, bool) {
    let ctx = Context::default();
    theme::apply(&ctx, false);
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    setup(&mut app);
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    app.begin_edit(Some(text.into()), false);
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    let output = frame(&ctx, &mut app);
    // The formula bar shows the same text above the grid; the in-cell editor's is the lowest.
    let Some((at, shown)) = output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match &s.shape {
            Shape::Text(t) if t.galley.text() == text => Some((i, t)),
            _ => None,
        })
        .max_by(|a, b| a.1.pos.y.total_cmp(&b.1.pos.y))
    else {
        return (Vec::new(), false);
    };
    let colors = shown
        .galley
        .job
        .sections
        .iter()
        .flat_map(|s| text.get(s.byte_range.start.0..s.byte_range.end.0).unwrap_or_default().chars().map(|ch| (ch, s.format.color)))
        .collect();
    // The editor's backdrop is the last filled rectangle painted before its text (the grid's own
    // cell, painted earlier, would match whatever the editor did).
    let backdrop = output.shapes.get(..at).unwrap_or_default().iter().rev().find_map(|s| match &s.shape {
        Shape::Rect(r) if r.fill != Color32::TRANSPARENT && r.rect.width() > 20.0 => Some(r.fill),
        _ => None,
    });
    (colors, backdrop == Some(fill))
}

fn white_on_black(app: &mut SheetApp) {
    app.run("home.fillColor", json!({"range": "A1", "color": "#000000"})).unwrap();
    app.run("home.fontColor", json!({"range": "A1", "color": "#FFFFFF"})).unwrap();
}

#[test]
fn typed_text_takes_the_cell_font_colour_before_it_is_entered() {
    // #3011: white text on a black fill was drawn black (invisible) until Enter.
    let (styled, _) = editor_paint(&white_on_black, "typed", Color32::BLACK);
    assert!(!styled.is_empty() && styled.iter().all(|&c| c == Color32::WHITE), "the cell's white font colour: {styled:?}");
    let (plain, _) = editor_paint(&|_| {}, "typed", Color32::WHITE);
    assert!(!plain.is_empty() && plain.iter().all(|&c| c == Color32::BLACK), "automatic stays black: {plain:?}");
    // Text that reads like a reference is not a formula: no reference colour.
    let (reference, _) = editor_paint(&white_on_black, "A1", Color32::BLACK);
    assert!(!reference.is_empty() && reference.iter().all(|&c| c == Color32::WHITE), "plain `A1` keeps the font colour: {reference:?}");
}

#[test]
fn the_editor_backdrop_is_the_fill_the_cell_shows() {
    // A conditional format's fill, as the grid paints it, so white text stays readable.
    let cf_fill = |app: &mut SheetApp| {
        app.run("home.fontColor", json!({"range": "A1", "color": "#FFFFFF"})).unwrap();
        let rule = json!({"type": "expression", "formula": "=TRUE", "style": {"fill": "#000000"}});
        app.run("home.conditionalFormat", json!({"range": "A1", "rule": rule})).unwrap();
    };
    let (colors, backdrop) = editor_paint(&cf_fill, "typed", Color32::BLACK);
    assert!(backdrop, "the editor paints the conditional fill behind the text");
    assert!(!colors.is_empty() && colors.iter().all(|&c| c == Color32::WHITE), "{colors:?}");
}

#[test]
fn a_formula_keeps_the_font_colour_and_colours_its_references() {
    // #3011 review: in a formula, the reference takes its highlight colour and the rest of the
    // text (`=`, the operator, the number) the cell's font colour.
    let (chars, _) = editor_chars(&white_on_black, "=B2+1", Color32::BLACK);
    let text: String = chars.iter().map(|&(ch, _)| ch).collect();
    assert_eq!(text, "=B2+1", "every character painted once");
    let color = |ch: char| chars.iter().find(|&&(c, _)| c == ch).map(|&(_, c)| c).unwrap();
    for ch in ['=', '+', '1'] {
        assert_eq!(color(ch), Color32::WHITE, "`{ch}` in the font colour: {chars:?}");
    }
    let reference = color('B');
    assert_eq!(color('2'), reference, "the whole reference in one colour");
    assert!(reference != Color32::WHITE && reference != Color32::BLACK, "`B2` in a reference colour: {chars:?}");
    // On an automatic-colour cell the same reference colour, and black for the rest.
    let (plain, _) = editor_chars(&|_| {}, "=B2+1", Color32::WHITE);
    let want = [('=', Color32::BLACK), ('B', reference), ('2', reference), ('+', Color32::BLACK), ('1', Color32::BLACK)];
    assert_eq!(plain, want);
}
