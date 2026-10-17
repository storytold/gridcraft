//! Ribbon hover previews must paint selections without creating workbook edits.
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::{Session, core::CellRef, model::Color};
use gridcraft_ui_egui::{SheetApp, grid::ColorPreview};
use serde_json::json;
use std::sync::Arc;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    harness_render(false)
}

fn harness_render(render: bool) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range":"A1", "values":[["Preview", "Other"], ["Second", "Outside"]]})).unwrap();
    session.execute("home.fillColor", json!({"range":"A2", "color":"#00FF00"})).unwrap();
    session.execute("selection.set", json!({"range":"A1:A2"})).unwrap();
    let builder = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0));
    let builder = if render { builder.wgpu() } else { builder };
    let mut h = builder.build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn hover(h: &mut Harness, pos: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    h.run_steps(2);
}

fn open(h: &mut Harness, command: &'static str) {
    let keys = if command == "home.fillColor" { vec![Key::F10, Key::H, Key::H] } else { vec![Key::F10, Key::H, Key::F, Key::C] };
    for key in keys {
        for pressed in [true, false] {
            h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        h.run_steps(2);
    }
}

#[test]
fn hover_leave_escape_and_commit_preserve_history() {
    for (command, preview) in
        [("home.fillColor", ColorPreview::Fill(Color::Rgb(0xFF0000))), ("home.fontColor", ColorPreview::Font(Color::Rgb(0xFF0000)))]
    {
        let mut h = harness();
        let before = h.state().session.active().unwrap().wb.clone();
        let revision = h.state().session.active().unwrap().revision;
        let undo_len = h.state().session.active().unwrap().undo.len();
        open(&mut h, command);
        let swatch = h.get_by_label("#FF0000").rect().center();
        hover(&mut h, swatch);
        assert_eq!(h.state().grid.color_preview, Some(preview));
        assert!(Arc::ptr_eq(&before, &h.state().session.active().unwrap().wb));
        assert_eq!(revision, h.state().session.active().unwrap().revision);
        assert_eq!(undo_len, h.state().session.active().unwrap().undo.len());
        hover(&mut h, pos2(1100.0, 700.0));
        assert_eq!(h.state().grid.color_preview, None);
        hover(&mut h, swatch);
        h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(3);
        assert_eq!(h.state().grid.color_preview, None);
        assert!(Arc::ptr_eq(&before, &h.state().session.active().unwrap().wb));
        open(&mut h, command);
        let swatch = h.get_by_label("#FF0000").rect().center();
        hover(&mut h, swatch);
        for pressed in [true, false] {
            h.input_mut().events.push(Event::PointerButton { pos: swatch, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        h.run_steps(3);
        assert_eq!(h.state().grid.color_preview, None);
        let doc = h.state().session.active().unwrap();
        assert_eq!(doc.undo.len(), undo_len + 1);
        for cell in ["A1", "A2", "B1"] {
            let style = doc.wb.styles.get(doc.wb.active().unwrap().style_id(CellRef::parse(cell).unwrap()));
            let color = if command == "home.fillColor" { style.fill.fg } else { style.font.color };
            assert_eq!(color, if cell == "B1" { Color::Auto } else { Color::Rgb(0xFF0000) });
        }
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(h.state().session.active().unwrap().wb.styles, before.styles);
    }
}

#[test]
fn automatic_and_no_fill_preview_without_committing() {
    for (command, label, preview) in
        [("home.fillColor", "No Fill", ColorPreview::Fill(Color::Auto)), ("home.fontColor", "Automatic", ColorPreview::Font(Color::Auto))]
    {
        let mut h = harness();
        open(&mut h, command);
        let pos = h.get_by_label(label).rect().center();
        hover(&mut h, pos);
        assert_eq!(h.state().grid.color_preview, Some(preview));
    }
}

fn text_color(h: &Harness, label: &str) -> egui::Color32 {
    fn find(shape: &egui::Shape, label: &str, grid_top: f32) -> Option<egui::Color32> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label && text.pos.y >= grid_top => Some(text.fallback_color),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, label, grid_top)),
            _ => None,
        }
    }
    h.output().shapes.iter().find_map(|s| find(&s.shape, label, h.state().grid.cells_rect.unwrap().top())).expect("cell text painted")
}

#[test]
fn font_preview_paints_only_selected_cells_and_restores_on_leave() {
    let mut h = harness();
    let original = text_color(&h, "Preview");
    let outside = text_color(&h, "Other");
    open(&mut h, "home.fontColor");
    let pos = h.get_by_label("#FF0000").rect().center();
    hover(&mut h, pos);
    assert_eq!(text_color(&h, "Preview"), egui::Color32::RED);
    assert_eq!(text_color(&h, "Second"), egui::Color32::RED);
    assert_eq!(text_color(&h, "Other"), outside);
    hover(&mut h, pos2(1100.0, 700.0));
    assert_eq!(text_color(&h, "Preview"), original);
}

#[test]
#[ignore = "manual GPU screenshot for visual review"]
fn screenshot_fill_preview() {
    let mut h = harness_render(true);
    open(&mut h, "home.fillColor");
    let pos = h.get_by_label("#FF0000").rect().center();
    hover(&mut h, pos);
    h.render().unwrap().save("/tmp/gridcraft-fill-preview.png").unwrap();
}
