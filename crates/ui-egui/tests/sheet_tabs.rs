//! Overflowing worksheet tabs, exercised through their actual painted geometry and input.
use egui::{Event, Modifiers, PointerButton, Rect, Shape, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(width: f32, active: usize) -> Harness {
    harness_with(width, 30, active)
}

fn harness_with(width: f32, sheets: usize, active: usize) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"sheet":0,"name":"Tab00"})).unwrap();
    for i in 1..sheets {
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

/// The New sheet button's left edge. The scrollbar track starts 8 px after the 26 px button.
fn new_sheet_left(h: &Harness) -> f32 {
    h.state().grid.hscroll.expect("the tab bar has a horizontal scrollbar").left() - 8.0 - 26.0
}

/// The painted background of the active tab: the smallest fill enclosing its name (the bar's own fill encloses it too).
fn active_tab_rect(h: &Harness, name: &str) -> Rect {
    let (text, _) = tab(h, name).unwrap();
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Rect(r) if r.rect.contains_rect(text) => Some(r.rect),
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap_or_else(|| panic!("{name} has no painted tab background"))
}

#[test]
fn new_sheet_button_follows_the_last_tab() {
    for width in [700.0, 1200.0] {
        let h = harness_with(width, 2, 1);
        let last = active_tab_rect(&h, "Tab01");
        let plus = new_sheet_left(&h);
        assert!(plus >= last.right() && plus - last.right() <= 12.0, "{width}: + at {plus}, last tab ends at {}", last.right());
    }
}

#[test]
fn overflowing_tabs_keep_their_share_and_the_new_sheet_button_after_it() {
    for width in [700.0, 1200.0] {
        let h = harness(width, 0);
        let (_, viewport) = tab(&h, "Tab00").unwrap();
        assert!(viewport.width() <= width * 0.6, "{width}: the tab strip grew past its share: {viewport:?}");
        let plus = new_sheet_left(&h);
        assert!(plus >= viewport.right() && plus - viewport.right() <= 12.0, "{width}: + at {plus}, strip ends at {}", viewport.right());
    }
}
