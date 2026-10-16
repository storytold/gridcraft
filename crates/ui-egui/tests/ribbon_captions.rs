//! Ribbon captions belong to their buttons, and chrome text is not selectable (#929). Wrap Text and
//! Merge & Center stand as one column, as in Excel.

use egui::text_selection::LabelSelectionState;
use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Shape, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.run("range.setValues", json!({"range": "A1", "values": [["title"]]})).unwrap();
    session.run("selection.set", json!({"range": "A1:C1"})).unwrap();
    let mut ready = false;
    let mut h = Harness::builder().with_size(vec2(1600.0, 800.0)).build_ui_state(
        move |ui, app: &mut SheetApp| {
            // The app's own one-time setup, so the test sees the style the window gets.
            if !ready {
                SheetApp::setup_context_for_language(ui.ctx(), false, gridcraft_ui_egui::i18n::Language::En);
                ready = true;
                return;
            }
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn text_rect(h: &Harness, text: &str) -> Rect {
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == text => Some(t.visual_bounding_rect()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text:?} was not painted"))
}

/// The centre of the first dropdown chevron painted right of `rect`, on its line.
fn chevron_right_of(h: &Harness, rect: Rect) -> Option<Pos2> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Path(p) if !p.closed && p.points.len() == 3 && p.points[0].y == p.points[2].y && p.points[1].y > p.points[0].y => {
                Some(Rect::from_points(&p.points).center())
            }
            _ => None,
        })
        .filter(|c| c.x > rect.right() && rect.y_range().contains(c.y))
        .min_by(|a, b| a.x.total_cmp(&b.x))
}

fn click(h: &mut Harness, at: Pos2) {
    h.input_mut().events.extend([
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(6);
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    h.input_mut().events.extend([
        Event::PointerMoved(from),
        Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.step();
    for i in 1..=8 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(6);
}

fn text_selected(h: &Harness) -> bool {
    h.ctx.with_plugin(|s: &mut LabelSelectionState| s.has_selection()).unwrap_or(false)
}

#[test]
fn clicking_the_merge_and_center_caption_merges() {
    let mut h = harness();
    let caption = text_rect(&h, "Merge & Center");
    click(&mut h, caption.center());
    let doc = h.state().session.active().unwrap();
    let merge = doc.wb.active().unwrap().merge_at(CellRef::parse("A1").unwrap());
    assert_eq!(merge.map(|r| r.to_string()), Some("A1:C1".to_string()), "the caption must run the button's action");
}

#[test]
fn clicking_the_wrap_text_caption_wraps() {
    let mut h = harness();
    let caption = text_rect(&h, "Wrap Text");
    click(&mut h, caption.center());
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    assert!(doc.wb.styles.get(sheet.style_id(CellRef::parse("A1").unwrap())).align.wrap, "the caption must run the button's action");
}

#[test]
fn wrap_text_and_merge_center_line_up_as_a_column() {
    let mut h = harness();
    let wrap = text_rect(&h, "Wrap Text");
    let merge = text_rect(&h, "Merge & Center");
    // The glyphs' side bearings differ by font ("W" vs "M"), so allow a pixel or two; misaligned captions were 13 px apart.
    assert!((wrap.left() - merge.left()).abs() < 2.0, "captions start at {} and {}", wrap.left(), merge.left());
    let arrow = chevron_right_of(&h, merge).expect("Merge & Center has no arrow right of its caption");
    assert!(arrow.x - merge.right() < 20.0, "the arrow at {} does not follow the caption ending at {}", arrow.x, merge.right());
    // Wrap Text is as wide as Merge & Center: its row is still Wrap Text above the arrow.
    click(&mut h, Pos2::new(arrow.x, wrap.center().y));
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    assert!(doc.wb.styles.get(sheet.style_id(CellRef::parse("A1").unwrap())).align.wrap, "Wrap Text stops short of the Merge & Center arrow");
}

#[test]
fn merge_menu_drops_under_the_whole_button_with_excels_items() {
    let mut h = harness();
    let merge = text_rect(&h, "Merge & Center");
    let arrow = chevron_right_of(&h, merge).expect("Merge & Center has an arrow");
    click(&mut h, arrow);
    // Excel: Merge & Center, Merge Across, Merge Cells, Unmerge Cells, Center Across Selection,
    // listed under the whole button (left edges together), not hanging off the arrow.
    let mut last_top = merge.bottom();
    for item in ["Merge Across", "Merge Cells", "Unmerge Cells", "Center Across Selection"] {
        let r = text_rect(&h, item);
        assert!(r.top() > last_top, "{item} is listed below the previous row");
        assert!((r.left() - merge.left()).abs() < 20.0, "{item} starts at {}, under the caption at {}", r.left(), merge.left());
        last_top = r.top();
    }
    let center_across = text_rect(&h, "Center Across Selection").center();
    click(&mut h, center_across);
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    let h_align = doc.wb.styles.get(sheet.style_id(CellRef::parse("A1").unwrap())).align.h;
    assert_eq!(h_align, gridcraft_engine::model::HAlign::CenterAcross, "the item centres the text across the selection");
}

#[test]
fn dragging_across_ribbon_captions_selects_no_text() {
    for text in ["Merge & Center", "Wrap Text"] {
        let mut h = harness();
        let caption = text_rect(&h, text);
        drag(&mut h, caption.left_center() + vec2(1.0, 0.0), caption.right_center() - vec2(1.0, 0.0));
        assert!(!text_selected(&h), "dragging across {text:?} selected it as text");
    }
}

#[test]
fn dragging_across_status_bar_text_selects_nothing() {
    let mut h = harness();
    let ready = text_rect(&h, "Ready");
    drag(&mut h, ready.left_center() + vec2(1.0, 0.0), ready.right_center() - vec2(1.0, 0.0));
    assert!(!text_selected(&h), "the status bar's mode label was selected as text");
}
