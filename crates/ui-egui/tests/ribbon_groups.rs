//! Ribbon groups: captions sit under their clusters and missing features take no space.

use egui_kittest::kittest::Queryable as _;
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
fn a_launcher_is_named_and_opens_its_groups_dialog() {
    let mut s = Session::new();
    s.new_workbook();
    let mut h = harness(s);
    let font = caption(&h, "Font").expect("the Font caption");
    // The launcher is painted, so its AccessKit name is all a screen reader gets; it sits in the
    // caption band's bottom-right corner, clear of the centred caption text.
    let launcher = h.get_by_role_and_label(egui::accesskit::Role::Button, "Font dialog launcher");
    let rect = launcher.rect();
    assert!(rect.left() > font.right(), "the launcher clears the caption: caption {font:?}, launcher {rect:?}");
    launcher.click();
    h.run_steps(2);
    // Not just any dialog: the launcher has to open its own group's form, on the Font tab.
    let dialog = h.state().dialog.as_ref().expect("a dialog launcher in the caption band opens a dialog");
    assert_eq!((dialog.name.as_str(), dialog.tab.as_str()), ("formatCells", "Font"), "the Font launcher opens Format Cells on Font");
}

#[test]
fn the_outline_launcher_opens_outline_settings() {
    // The Outline group carries Excel's launcher in its caption-band corner (#1795): it must
    // open the Outline Settings form, not just any dialog.
    let mut s = Session::new();
    s.new_workbook();
    let mut h = harness(s);
    h.state_mut().ui.ribbon_tab = "Data".into();
    h.run_steps(3);

    let launcher = h.get_by_role_and_label(egui::accesskit::Role::Button, "Outline dialog launcher");
    launcher.click();
    h.run_steps(2);
    let dialog = h.state().dialog.as_ref().expect("the Outline launcher opens a dialog");
    assert_eq!(dialog.name.as_str(), "outlineSettings", "the Outline launcher opens Outline Settings");
}

#[test]
fn a_long_caption_sets_a_floor_on_a_content_sized_group() {
    // A group is as wide as its controls, or as wide as its caption needs — whichever is larger.
    // In a narrow window that floor is what keeps the caption whole instead of clipping it: on the
    // Data tab "Get & Transform Data" is far wider than its one button, so the caption is centred
    // in a band wider than the control and sticks out on both sides of it.
    let mut s = Session::new();
    s.new_workbook();
    let mut h = harness(s);
    h.state_mut().ui.ribbon_tab = "Data".into();
    h.run_steps(3);

    let group = caption(&h, "Get & Transform Data").expect("the Get & Transform Data caption");
    let button = caption(&h, "Get Data").expect("the Get Data button's label");
    assert!(group.width() > button.width(), "the caption widens the group: caption {group:?}, button {button:?}");
    assert!(group.left() < button.left() && group.right() > button.right(), "the controls stay packed left in the wider band");
}
