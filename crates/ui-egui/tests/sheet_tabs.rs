//! Overflowing worksheet tabs, exercised through their actual painted geometry and input.
use egui::{Event, Modifiers, PointerButton, Rect, Shape, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(width: f32, active: usize) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"sheet":0,"name":"Tab00"})).unwrap();
    for i in 1..30 {
        s.execute("home.insertSheet", json!({"name":format!("Tab{i:02}")})).unwrap();
    }
    s.execute("sheet.activate", json!({"sheet":active})).unwrap();
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

fn tab(h: &Harness, name: &str) -> Option<(Rect, Rect)> {
    h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Text(t) if t.galley.job.text == name => Some((t.visual_bounding_rect(), s.clip_rect)),
        _ => None,
    })
}

fn assert_visible(h: &Harness, name: &str) {
    let (text, clip) = tab(h, name).unwrap_or_else(|| panic!("{name} was not painted in the tab viewport"));
    assert!(clip.contains_rect(text), "{name} is clipped: text={text:?}, viewport={clip:?}");
}

fn click(h: &mut Harness, at: egui::Pos2) {
    h.input_mut().events.extend([
        Event::PointerMoved(at),
        Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(20);
}

#[test]
fn opening_and_activating_late_sheets_reveals_their_tabs() {
    for width in [500.0, 1000.0] {
        let mut h = harness(width, 29);
        assert_visible(&h, "Tab29");
        h.state_mut().run("sheet.activate", json!({"sheet":0})).unwrap();
        h.run_steps(20);
        assert_visible(&h, "Tab00");
        h.state_mut().run("sheet.activate", json!({"sheet":20})).unwrap();
        h.run_steps(20);
        assert_visible(&h, "Tab20");
    }
}

#[test]
fn navigation_arrows_reveal_the_selected_tab() {
    let mut h = harness(700.0, 0);
    let (_, clip) = tab(&h, "Tab00").unwrap();
    // The Next arrow sits left of the All Sheets button (24 px) and the 8 px gap before the tabs.
    let next = pos2(clip.left() - 44.0, clip.center().y);
    for _ in 0..29 {
        click(&mut h, next);
    }
    assert_eq!(h.state().session.doc().unwrap().wb.active_sheet, 29);
    assert_visible(&h, "Tab29");
}

#[test]
fn manual_wheel_and_trackpad_browsing_does_not_change_selection_or_snap_back() {
    for delta in [vec2(0.0, -350.0), vec2(-350.0, 0.0)] {
        let mut h = harness(700.0, 0);
        let before = h.state().session.doc().unwrap().wb.clone();
        let (_, clip) = tab(&h, "Tab00").unwrap();
        h.input_mut().events.extend([
            Event::PointerMoved(clip.center()),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Start,
                delta: vec2(0.0, 0.0),
                modifiers: Modifiers::NONE,
            },
        ]);
        h.step();
        h.input_mut().events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.input_mut().events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::End,
            delta: vec2(0.0, 0.0),
            modifiers: Modifiers::NONE,
        });
        h.run_steps(40);
        assert!(!tab(&h, "Tab00").is_some_and(|(r, c)| c.contains_rect(r)), "gesture should browse away from the selected tab: {delta:?}");
        assert_eq!(h.state().session.doc().unwrap().wb, before, "browsing tabs must not edit the workbook");
        h.run_steps(40);
        assert!(!tab(&h, "Tab00").is_some_and(|(r, c)| c.contains_rect(r)), "idle frames must not snap back");
        h.state_mut().run("sheet.activate", json!({"sheet":29})).unwrap();
        h.run_steps(20);
        assert_visible(&h, "Tab29");
    }
}
#[test]
fn hidden_sheets_long_names_and_window_resize_keep_active_tab_visible() {
    let mut h = harness(1000.0, 0);
    for sheet in 1..10 {
        h.state_mut().run("sheet.hide", json!({"sheet":sheet})).unwrap();
    }
    let name = "Quarterly results and forecasts";
    h.state_mut().run("sheet.rename", json!({"sheet":29,"name":name})).unwrap();
    h.state_mut().run("sheet.activate", json!({"sheet":29})).unwrap();
    h.run_steps(20);
    assert_visible(&h, name);
    // Narrow, but still wide enough for the long tab next to the arrows and the All Sheets button.
    h.set_size(vec2(560.0, 120.0));
    h.run_steps(20);
    assert_visible(&h, name);
    h.set_size(vec2(1000.0, 120.0));
    h.run_steps(20);
    assert_visible(&h, name);
    assert_eq!(h.state().session.doc().unwrap().wb.active_sheet, 29);
}

#[test]
fn new_sheet_button_stays_reachable_and_reveals_the_inserted_tab() {
    let mut h = harness(700.0, 29);
    let (_, clip) = tab(&h, "Tab29").unwrap();
    click(&mut h, pos2(clip.right() + 18.0, clip.center().y));
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.wb.sheets.len(), 31);
    let name = d.wb.active().unwrap().name.clone();
    assert_visible(&h, &name);
}

#[test]
fn overflow_leaves_the_worksheet_scrollbar_reachable() {
    // The tab bar's horizontal scrollbar track keeps its share of the bar however many tabs
    // overflow; dragging its thumb is covered with a rendered grid in `interaction.rs`.
    for width in [500.0, 700.0, 1200.0] {
        let h = harness(width, 0);
        let track = h.state().grid.hscroll.expect("the tab bar has a horizontal scrollbar");
        assert!(track.width() >= 100.0, "{width}: the scrollbar keeps a usable width: {track:?}");
        assert!(track.left() >= 0.0 && track.right() <= width + 0.5, "{width}: the scrollbar is offscreen: {track:?}");
    }
}

#[test]
fn protected_sheet_tabs_carry_a_padlock_before_the_name() {
    let mut h = harness(1200.0, 0);
    let x = |h: &Harness, name: &str| tab(h, name).unwrap().0.left();
    let (gap_before, next_before) = (x(&h, "Tab02") - x(&h, "Tab01"), x(&h, "Tab03"));
    // Protect Tab02 (the active sheet), then go back to Tab00 so Tab02 is an ordinary tab again.
    h.state_mut().run("sheet.activate", json!({"sheet":2})).unwrap();
    h.state_mut().run("review.protectSheet", json!({})).unwrap();
    h.state_mut().run("sheet.activate", json!({"sheet":0})).unwrap();
    h.run_steps(20);
    assert!(h.state().session.doc().unwrap().wb.sheets[2].is_protected());
    // The name moves right to make room for the padlock, and the tab grows (the 72 px minimum width can absorb part
    // of the padlock's 16 px, so the growth is at most that).
    assert!(x(&h, "Tab02") - x(&h, "Tab01") > gap_before, "the protected tab's name should move right");
    let grown = x(&h, "Tab03") - next_before;
    assert!(grown > 0.0 && grown <= 16.0, "the protected tab should grow by up to the padlock's room, grew {grown}");
    // The padlock is painted just left of the name.
    let name = tab(&h, "Tab02").unwrap().0;
    let lock_body = h.output().shapes.iter().any(|s| match &s.shape {
        Shape::Rect(r) => r.rect.right() <= name.left() && r.rect.left() >= name.left() - 16.0 && r.rect.width() < 12.0,
        _ => false,
    });
    assert!(lock_body, "no padlock painted before the protected tab's name");
}
