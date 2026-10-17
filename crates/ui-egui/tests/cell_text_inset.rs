//! Cell text sits in its row the way Excel's does (ft-121.7.4): top aligned text keeps the same
//! small inset from the top border as in a standard row, bottom aligned text sits just above the
//! bottom border, and middle aligned text halfway between. Wrapped text follows the same rule.
//!
//! Reference, measured on Excel for Microsoft 365 (Windows, 100% zoom, Calibri 11 and Aptos Narrow
//! 11): the baseline of top aligned text is about 1.0 em below the cell's top edge, and the
//! baseline of bottom aligned text about 0.3 em above its bottom edge (gridline included), in any
//! row height and for wrapped text too.

use egui::Shape;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let set = |s: &mut Session, cell: &str, input: &str| {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    };
    // Row 2: a 40 pt row (53.33 screen points), one cell per vertical alignment.
    set(&mut s, "B2", "Top");
    set(&mut s, "C2", "Mid");
    set(&mut s, "D2", "Bot");
    // Row 4: a standard row, top and bottom aligned.
    set(&mut s, "B4", "StdTop");
    set(&mut s, "C4", "StdBot");
    // Row 6: a tall row of wrapped text, top and bottom aligned. Short words, so it wraps to two
    // lines that fit the row with any fallback font (long words break mid-word and overflow).
    set(&mut s, "B6", "Top1 Top2");
    set(&mut s, "C6", "Bot1 Bot2");
    for (cmd, range) in [
        ("home.alignTop", "B2"),
        ("home.alignMiddle", "C2"),
        ("home.alignBottom", "D2"),
        ("home.alignTop", "B4"),
        ("home.alignBottom", "C4"),
        ("home.alignTop", "B6"),
        ("home.alignBottom", "C6"),
    ] {
        s.execute(cmd, json!({"range": range})).unwrap();
    }
    s.execute("home.wrapText", json!({"range": "B6:C6", "on": true})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "2:2", "height": 53.33})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "6:6", "height": 80})).unwrap();
    s.execute("selection.set", json!({"range": "H20"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

/// The painted text whose text is `text`: (font size, first baseline y, last baseline y).
fn baselines(h: &Harness, text: &str) -> (f32, f32, f32) {
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == text => {
                let size = t.galley.job.sections.first()?.format.font_id.size;
                let base = |row: &egui::epaint::text::PlacedRow| Some(t.pos.y + row.pos.y + row.glyphs.first()?.pos.y);
                Some((size, base(t.galley.rows.first()?)?, base(t.galley.rows.last()?)?))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text:?} is drawn"))
}

fn cell(h: &Harness, a1: &str) -> egui::Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(a1).unwrap())
}

#[test]
fn top_middle_and_bottom_text_keep_excels_insets_in_a_tall_row() {
    let h = harness();
    let (em, top, _) = baselines(&h, "Top");
    let (_, mid, _) = baselines(&h, "Mid");
    let (_, bot, _) = baselines(&h, "Bot");
    let row = cell(&h, "B2");
    let from_top = (top - row.top()) / em;
    let from_bottom = (row.bottom() - bot) / em;
    assert!((from_top - 1.0).abs() < 0.1, "top aligned baseline is {from_top} em below the top edge (Excel: 1.0)");
    assert!((from_bottom - 0.3).abs() < 0.08, "bottom aligned baseline is {from_bottom} em above the bottom edge (Excel: 0.3)");
    // Middle: the line box is centred, so the baseline is halfway between the other two.
    assert!((mid - (top + bot) / 2.0).abs() <= 1.0, "middle baseline {mid} sits halfway between {top} and {bot}");
}

#[test]
fn a_tall_row_shows_aligned_text_where_a_standard_row_does() {
    let h = harness();
    let (_, tall_top, _) = baselines(&h, "Top");
    let (_, tall_bot, _) = baselines(&h, "Bot");
    let (_, std_top, _) = baselines(&h, "StdTop");
    let (_, std_bot, _) = baselines(&h, "StdBot");
    let (tall, std) = (cell(&h, "B2"), cell(&h, "B4"));
    assert!(((tall_top - tall.top()) - (std_top - std.top())).abs() <= 1.0, "top inset matches the standard row");
    assert!(((tall.bottom() - tall_bot) - (std.bottom() - std_bot)).abs() <= 1.0, "bottom inset matches the standard row");
    // A standard row is about one line tall, so top and bottom aligned text land (almost) together.
    assert!(((std_top - std.top()) - (std_bot - std.top())).abs() <= 1.5, "standard row: top {std_top}, bottom {std_bot}");
}

#[test]
fn wrapped_text_uses_the_same_insets_as_a_single_line() {
    let h = harness();
    let (_, top, _) = baselines(&h, "Top");
    let (_, bot, _) = baselines(&h, "Bot");
    let (_, wrap_top, _) = baselines(&h, "Top1 Top2");
    let (_, _, wrap_bot) = baselines(&h, "Bot1 Bot2");
    let (row, wrap_row) = (cell(&h, "B2"), cell(&h, "B6"));
    assert!(
        ((wrap_top - wrap_row.top()) - (top - row.top())).abs() <= 1.5,
        "first wrapped line sits like single-line top text: wrapped {} vs single {}",
        wrap_top - wrap_row.top(),
        top - row.top()
    );
    assert!(
        ((wrap_row.bottom() - wrap_bot) - (row.bottom() - bot)).abs() <= 1.5,
        "last wrapped line sits like single-line bottom text: wrapped {} vs single {}",
        wrap_row.bottom() - wrap_bot,
        row.bottom() - bot
    );
}
