//! The title bar's document icon hands its file to the app shell on drag; the shell starts the
//! native macOS drag from `SheetApp::drag_file`.

use egui::{Color32, Event, Modifiers, PointerButton, Pos2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, theme};

type Harness = egui_kittest::Harness<'static, SheetApp>;

const W: f32 = 1200.0;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    // A saved document: the icon (and the drag) exist only for a file on disk. The icon texture
    // itself comes from the platform (the app shell loads it on macOS); the test stands in with a
    // plain patch the same way the shell would hand one over.
    session.active_mut().unwrap().path = Some("/tmp/Book1.xlsx".into());
    let mut h = Harness::builder().with_size(egui::vec2(W, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            if app.doc_icon.is_none() {
                let tex =
                    ui.ctx().load_texture("document_icon", egui::ColorImage::new([4, 4], vec![Color32::BLUE; 16]), egui::TextureOptions::LINEAR);
                app.doc_icon = Some(tex);
            }
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// Where ribbon centres the icon: half the title (measured with the same font ribbon uses) left
/// of the window centre, minus the icon and its gap.
fn icon_centre(h: &Harness) -> Pos2 {
    // The title is the saved file's name (`display_title`), which the measurement must match.
    let title_w = h.ctx.fonts_mut(|f| f.layout_no_wrap("Book1.xlsx".to_owned(), theme::ui_bold(13.0), Color32::WHITE).size().x);
    const ICON: f32 = 16.0;
    const GAP: f32 = 6.0;
    let p = Pos2::new(W / 2.0 - (title_w + ICON + GAP) / 2.0 + ICON / 2.0, 14.0);
    eprintln!("DEBUG title_w={title_w} icon_centre={p:?}");
    p
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

#[test]
fn dragging_the_document_icon_hands_the_file_to_the_shell() {
    let mut h = harness();
    let at = icon_centre(&h);
    frame(&mut h, Event::PointerMoved(at));
    frame(&mut h, button(at, true));
    frame(&mut h, Event::PointerMoved(at + egui::vec2(14.0, 10.0)));
    assert_eq!(h.state().drag_file.as_deref(), Some("/tmp/Book1.xlsx"));
}

#[test]
fn clicking_the_document_icon_does_nothing_yet() {
    // A plain click (press and release in place) is not a drag: nothing is handed over, the file
    // stays where it is.
    let mut h = harness();
    let at = icon_centre(&h);
    frame(&mut h, Event::PointerMoved(at));
    frame(&mut h, button(at, true));
    frame(&mut h, button(at, false));
    assert_eq!(h.state().drag_file, None);
}

#[test]
fn an_unsaved_document_has_no_document_icon_drag() {
    // Without a file there is no icon and nothing to drag; a drag over where it would be lands on
    // the window-dragging title area instead.
    let mut session = Session::new();
    session.new_workbook();
    let mut h = Harness::builder().with_size(egui::vec2(W, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    let at = Pos2::new(W / 2.0 - 60.0, 14.0); // left of the centred title, as the icon would be
    frame(&mut h, Event::PointerMoved(at));
    frame(&mut h, button(at, true));
    frame(&mut h, Event::PointerMoved(at + egui::vec2(14.0, 10.0)));
    assert_eq!(h.state().drag_file, None);
}
