//! Ribbon groups: captions sit under their clusters and missing features take no space.

use egui::{Event, Modifiers, PointerButton};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn caption(h: &egui_kittest::Harness<'static, SheetApp>, text: &str) -> Option<egui::Rect> {
    h.output().shapes.iter().find_map(|clipped| match &clipped.shape {
        egui::Shape::Text(t) if t.galley.job.text == text => Some(egui::Rect::from_min_size(t.pos, t.galley.size())),
        _ => None,
    })
}

#[test]
fn captions_sit_under_the_controls_and_groups_pack_left() {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = harness(s);
    h.state_mut().ui.ribbon_tab = "Draw".into();
    h.run_steps(3);

    let tools = caption(&h, "Drawing Tools").expect("the Drawing Tools caption");
    assert!(tools.top() > 60.0, "the caption sits in the band under the controls: {tools:?}");
    // The last control of Drawing Tools (Eraser) and the first of Convert (Ink to Shape): with
    // content-sized groups the only space between them is the separator — a feature GridCraft
    // does not implement never gets a reserved slot.
    let eraser = caption(&h, "Eraser").expect("the Eraser label");
    let ink = caption(&h, "Ink to").expect("the Ink to Shape label");
    let gap = ink.left() - eraser.right();
    assert!((0.0..80.0).contains(&gap), "groups pack left without reserved space: gap {gap}");
}

#[test]
fn a_launcher_opens_its_groups_dialog() {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = harness(s);
    let font = caption(&h, "Font").expect("the Font caption");
    // The launcher is a small box in the group's bottom-right corner: walk the caption band to
    // its right until one responds (the band carries no other clickable control).
    let mut opened = false;
    let y = font.center().y;
    let mut x = font.center().x;
    while x < font.center().x + 600.0 && !opened {
        let p = egui::pos2(x, y);
        h.input_mut().events.extend([
            Event::PointerMoved(p),
            Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ]);
        h.run_steps(2);
        opened = h.state().dialog.is_some();
        x += 6.0;
    }
    assert!(opened, "a dialog launcher in the caption band opens a dialog");
}
