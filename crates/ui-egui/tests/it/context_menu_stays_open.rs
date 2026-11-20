//! A right-click on a cell opens its context menu, and the menu stays open until the user picks
//! an item, presses Escape or clicks elsewhere.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::{SheetApp, grid};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    harness_at(1.0)
}

/// `ppp` physical pixels per point: 2 on a Retina display.
fn harness_at(ppp: f32) -> Harness {
    harness_sized(ppp, egui::vec2(1200.0, 800.0))
}

/// Builds a viewport whose width can constrain translated context menus.
fn harness_sized(ppp: f32, size: egui::Vec2) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_size(size).with_pixels_per_point(ppp).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn center(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sh, CellRef::parse(cell).unwrap()).center()
}

fn button(h: &mut Harness, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
}

/// Union of the rendered command rectangles, rather than the stored popup anchor.
fn context_menu_content_bounds(h: &Harness) -> egui::Rect {
    [
        "Cut",
        "Copy",
        "Paste",
        "Paste Special…",
        "Insert…",
        "Delete…",
        "Clear Contents",
        "Filter by Selected Cell's Value",
        "Sort A to Z",
        "Sort Z to A",
        "New Comment",
        "New Note",
        "Format Cells…",
        "Pick From Drop-down List…",
        "Define Name…",
        "Link…",
    ]
    .into_iter()
    .map(|label| {
        h.query_all_by_label(h.state().ui.language.tr(label))
            .max_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
            .expect("context item is rendered")
            .rect()
    })
    .fold(egui::Rect::NOTHING, egui::Rect::union)
}

/// Press and release arrive in the same frame (a quick click).
fn quick_right_click(h: &mut Harness, cell: &str) {
    let pos = center(h, cell);
    h.input_mut().events.push(Event::PointerMoved(pos));
    button(h, pos, true);
    button(h, pos, false);
    h.run_steps(4);
}

/// Press and release in separate frames (a slower click).
fn slow_right_click(h: &mut Harness, cell: &str) {
    let pos = center(h, cell);
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(1);
    button(h, pos, true);
    h.run_steps(1);
    button(h, pos, false);
    h.run_steps(4);
}

#[test]
fn a_quick_right_click_keeps_the_menu_open() {
    let mut h = harness();
    quick_right_click(&mut h, "C5");
    assert!(h.state().grid.context_menu.is_some(), "the menu stays open");
}

#[test]
fn a_slow_right_click_keeps_the_menu_open() {
    let mut h = harness();
    slow_right_click(&mut h, "C5");
    assert!(h.state().grid.context_menu.is_some(), "the menu stays open");
}

#[test]
fn right_clicking_another_cell_moves_the_menu_there() {
    for quick in [true, false] {
        let mut h = harness();
        slow_right_click(&mut h, "C5");
        if quick {
            quick_right_click(&mut h, "M2");
        } else {
            slow_right_click(&mut h, "M2");
        }
        let at = h.state().grid.context_menu;
        assert!(at.is_some(), "the menu stays open (quick: {quick})");
        assert_eq!(at, Some(center(&h, "M2")), "at the second cell (quick: {quick})");
    }
}

/// A real pointer sits between pixels (x = 252.46), and the menu's corner snaps to the nearest
/// physical pixel (252.5 on a Retina display), just past the pointer. The press that comes in the
/// same frame as the release then counted as a press outside the menu and closed it at once.
#[test]
fn a_quick_right_click_between_pixels_keeps_the_menu_open_on_retina() {
    let mut h = harness_at(2.0);
    let pos = center(&h, "C5") + egui::vec2(0.46, 0.37);
    h.input_mut().events.push(Event::PointerMoved(pos));
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.run_steps(4);
    assert!(h.state().grid.context_menu.is_some(), "the menu stays open");
}

#[test]
/// Hovering a long command keeps both its size and rendered position fixed.
fn hovering_a_long_context_item_does_not_resize_the_menu() {
    let mut h = harness();
    quick_right_click(&mut h, "C5");
    let item = h.get_by_label("Filter by Selected Cell's Value").rect();
    let rendered_menu = context_menu_content_bounds(&h);
    let menu = h.state().grid.context_menu.unwrap();
    h.input_mut().events.push(Event::PointerMoved(item.center()));
    h.run_steps(2);

    let hovered_item = h.get_by_label("Filter by Selected Cell's Value").rect();
    assert_eq!(hovered_item.size(), item.size(), "hovering keeps the command's dimensions fixed");
    assert_eq!(hovered_item.min, item.min, "hovering keeps the rendered command position fixed");
    assert_eq!(context_menu_content_bounds(&h), rendered_menu, "hovering preserves the rendered menu bounds");
    let menu_after = h.state().grid.context_menu.unwrap();
    assert_eq!(menu_after, menu, "hovering does not move the menu anchor");
}

/// Long translations stay on-screen and hovering cannot resize the actual popup frame.
#[test]
fn translated_context_menus_fit_a_narrow_viewport_without_resizing_on_hover() {
    for language in [Language::En, Language::De, Language::Pl, Language::Fr] {
        for (width, font_size) in [(300.0, 24.0), (1200.0, 13.5)] {
            let size = egui::vec2(width, 800.0);
            let mut h = harness_sized(1.0, size);
            h.ctx.all_styles_mut(|style| {
                // Menu sizing uses Button; egui's button theme paints its text with Body.
                style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(font_size));
                style.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(font_size));
            });
            h.state_mut().ui.language = language;
            h.state_mut().grid.context_menu = Some(egui::pos2(width - 8.0, 120.0));
            h.run_steps(4);
            let popup = |h: &Harness| h.ctx.memory(|m| m.area_rect(egui::Id::unique("cell_context"))).unwrap();
            let before = popup(&h);
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            assert!(viewport.expand(0.5).contains_rect(before), "{language:?} popup exceeds {width}pt viewport: {before:?}");
            assert!(before.width() >= 200.0, "normal English-menu minimum is retained");
            let label = language.tr("Filter by Selected Cell's Value");
            let item = h.get_by_label(label).rect();
            let galley = h
                .output()
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label && item.contains(text.pos) => Some(&text.galley),
                    _ => None,
                })
                .expect("the context command has a painted text galley");
            // The layout job retains the full label even when the painted glyphs are elided.
            let rendered: String = galley.rows.iter().flat_map(|row| row.glyphs.iter().map(|glyph| glyph.chr)).collect();
            assert_eq!(galley.rows.len(), 1, "{language:?} command stays on one line");
            if width == 300.0 {
                assert!(galley.elided, "{language:?} narrow command must truncate");
                assert!(
                    rendered != label && rendered.ends_with('…') && label.starts_with(rendered.trim_end_matches('…')),
                    "{language:?} narrow command paints a prefix and ellipsis: {rendered:?}"
                );
            } else {
                assert!(!galley.elided, "{language:?} wide command must remain complete");
                assert_eq!(rendered, label, "{language:?} wide command paints the full label");
            }
            let content = context_menu_content_bounds(&h);
            h.input_mut().events.push(Event::PointerMoved(item.center()));
            h.run_steps(3);
            assert_eq!(popup(&h), before, "{language:?} popup frame stays fixed while hovering");
            assert_eq!(context_menu_content_bounds(&h), content, "{language:?} commands stay fixed while hovering");
        }
    }
}

#[test]
fn a_linked_cell_offers_to_edit_open_and_remove_its_link() {
    // #2120: the menu only offered "Link…", and its dialog was titled Insert Hyperlink.
    use egui_kittest::kittest::Queryable as _;
    let mut h = harness();
    h.state_mut().session.execute("insert.link", serde_json::json!({"cell": "B2", "target": "#Sheet1!D9", "text": "Go"})).unwrap();
    h.run_steps(2);
    quick_right_click(&mut h, "B2");
    assert!(h.query_by_label("Edit Hyperlink…").is_some() && h.query_by_label("Open Hyperlink").is_some(), "linked cell menu");
    assert!(h.query_by_label("Link…").is_none());
    // Open Hyperlink follows it (an internal reference selects its cell).
    h.get_by_label("Open Hyperlink").click();
    h.run_steps(4);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::parse("D9").unwrap());
    // Edit Hyperlink opens the link's own dialog.
    quick_right_click(&mut h, "B2");
    h.get_by_label("Edit Hyperlink…").click();
    h.run_steps(4);
    let d = h.state().dialog.as_ref().expect("the link dialog");
    assert_eq!(d.title, "Edit Hyperlink");
    assert_eq!(d.values.get("target").and_then(|v| v.as_str()), Some("#Sheet1!D9"));
    h.state_mut().dialog = None;
    h.run_steps(2);
    // Remove Hyperlink takes the link away; a cell without one offers Link… again.
    quick_right_click(&mut h, "B2");
    h.get_by_label("Remove Hyperlink").click();
    h.run_steps(4);
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    assert!(!sh.hyperlinks.contains_key(&CellRef::parse("B2").unwrap()));
    quick_right_click(&mut h, "B2");
    assert!(h.query_by_label("Link…").is_some() && h.query_by_label("Edit Hyperlink…").is_none());
}
