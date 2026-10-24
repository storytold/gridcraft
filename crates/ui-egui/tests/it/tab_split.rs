//! The handle between the sheet tabs and the horizontal scrollbar: dragging it gives the tabs more
//! or less of the tab bar, as in Excel.
use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Shape, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(width: f32, sheets: usize) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"sheet":0,"name":"Tab00"})).unwrap();
    for i in 1..sheets {
        s.execute("home.insertSheet", json!({"name":format!("Tab{i:02}")})).unwrap();
    }
    s.execute("sheet.activate", json!({"sheet":0})).unwrap();
    let mut fonts_ready = false;
    let mut h = Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(width, 120.0)).build_ui_state(
        move |ui, app: &mut SheetApp| {
            if !fonts_ready {
                ui.ctx().set_fonts(gridcraft_ui_egui::theme::font_definitions());
                fonts_ready = true;
                return;
            }
            app.grid.cells_rect = Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(ui.max_rect().width(), 80.0)));
            gridcraft_ui_egui::tabs::sheet_tabs(app, ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(20);
    h
}

fn split(h: &Harness) -> Rect {
    h.state().grid.tab_split.expect("the tab bar has a split handle")
}

fn track(h: &Harness) -> Rect {
    h.state().grid.hscroll.expect("the tab bar has a horizontal scrollbar")
}

/// The clip rectangle of the tab strip: the viewport the tab names are painted in.
fn strip(h: &Harness) -> Rect {
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == "Tab00" => Some(s.clip_rect),
            _ => None,
        })
        .expect("Tab00 was painted")
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    h.input_mut().events.extend([
        Event::PointerMoved(from),
        Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.step();
    for i in 1..=6 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(20);
}

#[test]
fn dragging_the_handle_right_gives_overflowing_tabs_more_room() {
    let mut h = harness(1200.0, 30);
    let (before, handle) = (strip(&h), split(&h));
    assert!(before.width() <= 1200.0 * 0.6, "the tabs start at their default share: {before:?}");
    drag(&mut h, handle.center(), pos2(1000.0, handle.center().y));
    let after = strip(&h);
    assert!(after.width() > before.width() + 200.0, "the strip grew: {before:?} -> {after:?}");
    assert!((split(&h).center().x - 1000.0).abs() <= 2.0, "the handle follows the pointer: {:?}", split(&h));
    assert_eq!(track(&h).left(), split(&h).right(), "the scrollbar starts at the handle");
    assert!(track(&h).width() >= 100.0, "the scrollbar keeps a usable width: {:?}", track(&h));
    assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 30, "dragging the handle does not edit the workbook");
}

#[test]
fn dragging_the_handle_left_gives_the_scrollbar_more_room() {
    let mut h = harness(1200.0, 30);
    let (before, handle) = (track(&h), split(&h));
    drag(&mut h, handle.center(), pos2(handle.center().x - 300.0, handle.center().y));
    let after = track(&h);
    assert!(after.width() > before.width() + 280.0, "the scrollbar grew: {before:?} -> {after:?}");
    assert!(strip(&h).width() < 1200.0 * 0.6 - 280.0, "the strip shrank: {:?}", strip(&h));
}

#[test]
fn the_handle_stops_before_the_scrollbar_gets_too_narrow_and_keeps_new_sheet_shown() {
    let mut h = harness(1200.0, 30);
    let handle = split(&h);
    drag(&mut h, handle.center(), pos2(1195.0, handle.center().y));
    assert!(track(&h).width() >= 100.0 - 0.5, "the scrollbar keeps 100 px: {:?}", track(&h));
    let (handle, bar_left) = (split(&h), strip(&h).left());
    drag(&mut h, handle.center(), pos2(0.0, handle.center().y));
    // The tab strip has no width left, and the New sheet button (26 px after a 6 px gap) still
    // fits before the handle.
    assert!((split(&h).left() - bar_left - 32.0).abs() <= 1.0, "the handle is 32 px after the strip's start {bar_left}: {:?}", split(&h));
}

#[test]
fn a_narrower_window_keeps_the_scrollbar_after_the_handle_was_dragged_right() {
    let mut h = harness(1200.0, 30);
    let handle = split(&h);
    drag(&mut h, handle.center(), pos2(1195.0, handle.center().y));
    h.set_size(vec2(700.0, 120.0));
    h.run_steps(20);
    let track = track(&h);
    assert!(track.width() >= 100.0 - 0.5 && track.right() <= 700.5, "the scrollbar keeps 100 px in the bar: {track:?}");
    assert_eq!(track.left(), split(&h).right(), "the scrollbar starts at the handle");
    assert!(strip(&h).right() <= split(&h).left() - 32.0 + 0.5, "the tabs end before New sheet: {:?}", strip(&h));
}

#[test]
fn room_the_tabs_do_not_fill_stays_empty_and_new_sheet_follows_the_last_tab() {
    let mut h = harness(1200.0, 2);
    let plus_before = track(&h).left() - 8.0 - 26.0;
    let handle = split(&h);
    drag(&mut h, handle.center(), pos2(900.0, handle.center().y));
    assert!((split(&h).center().x - 900.0).abs() <= 2.0, "the handle stays where it was dragged: {:?}", split(&h));
    assert_eq!(track(&h).left(), split(&h).right());
    // New sheet (+) is still right after the last tab: clicking there adds a sheet.
    let at = pos2(plus_before + 13.0, handle.center().y);
    h.input_mut().events.extend([
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(20);
    assert_eq!(h.state().session.doc().unwrap().wb.sheets.len(), 3, "New sheet stayed after the last tab");
}

#[test]
fn the_handle_shows_the_resize_cursor() {
    let mut h = harness(1200.0, 30);
    let at = split(&h).center();
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
}

#[test]
fn each_workbook_keeps_its_own_tab_share() {
    let mut h = harness(1200.0, 30);
    let handle = split(&h);
    drag(&mut h, handle.center(), pos2(1000.0, handle.center().y));
    assert!(track(&h).left() > 990.0, "the first workbook's scrollbar starts at the handle: {:?}", track(&h));
    h.state_mut().session.new_workbook();
    h.run_steps(20);
    // The new workbook's one tab takes its default room, so its scrollbar starts right after New sheet.
    assert!(track(&h).left() < 400.0, "a new workbook starts at the default share: {:?}", track(&h));
    assert_eq!(h.state().grid.tab_shares.len(), 1, "only the dragged workbook has a share");
}
