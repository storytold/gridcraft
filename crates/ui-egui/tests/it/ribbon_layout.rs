//! Ribbon layout: the big split button, the scroll arrows of an overflowing tab, and every tab
//! fitting the card's height.

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::icons::Icon;
use gridcraft_ui_egui::ribbon::{RIBBON_LABEL_H, RibbonLabels, RibbonOverflow, TABS};
use gridcraft_ui_egui::{SheetApp, widgets};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn app_at(width: f32, tab: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = tab.into();
    let mut h = crate::common::builder().with_size(vec2(width, 700.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

#[test]
fn every_tab_fits_the_card_height() {
    // Columns of small buttons hold three rows; a fourth would be cut off at the card's bottom.
    for tab in TABS {
        let h = app_at(1440.0, tab);
        let o = RibbonOverflow::get(&h.ctx);
        assert!(o.visible_height > 0.0, "{tab}: the card was laid out");
        assert!(o.content_height <= o.visible_height + 0.5, "{tab}: {} px of groups in {} px", o.content_height, o.visible_height);
    }
}

#[test]
fn every_tab_names_its_groups_in_the_label_band() {
    // Excel's card has a third band under the buttons naming each group; the band sits below the
    // rows the buttons use, and each name is centred in its group's width.
    for tab in TABS {
        let h = app_at(1440.0, tab);
        let o = RibbonOverflow::get(&h.ctx);
        let labels = RibbonLabels::get(&h.ctx);
        assert!(!labels.labels.is_empty(), "{tab}: the tab names its groups");
        assert!((labels.band.height() - RIBBON_LABEL_H).abs() < 0.5, "{tab}: the band is {} px tall", labels.band.height());
        assert!(o.content_height <= o.visible_height + 0.5, "{tab}: {} px of buttons in {} px", o.content_height, o.visible_height);
        assert!(o.visible_height + RIBBON_LABEL_H <= 94.5, "{tab}: buttons and band fit the card's inner box");
        let mut right = f32::NEG_INFINITY;
        for g in &labels.labels {
            assert!(g.label.starts_with(g.painted.trim_end_matches('…')), "{tab}: {} was painted as {}", g.label, g.painted);
            assert!((g.rect.center().y - labels.band.center().y).abs() < 0.5, "{tab}: {} is painted in the band", g.label);
            assert!(g.rect.top() >= labels.band.top() - 0.5 && g.rect.bottom() <= labels.band.bottom() + 0.5, "{tab}: {} stays in the band", g.label);
            assert!(g.rect.left() >= right - 0.5, "{tab}: {} overlaps the label before it", g.label);
            right = g.rect.right();
        }
    }
    // Home carries Excel's group names, in Excel's order.
    let h = app_at(1440.0, "Home");
    let names: Vec<String> = RibbonLabels::get(&h.ctx).labels.into_iter().map(|g| g.label).collect();
    assert_eq!(names, ["Clipboard", "Font", "Alignment", "Number", "Styles", "Cells", "Editing"]);
}

#[test]
fn an_overflowing_tab_shows_scroll_arrows_that_scroll() {
    let mut h = app_at(700.0, "Home");
    let o = RibbonOverflow::get(&h.ctx);
    assert!(o.overflows(), "Home is wider than a 700 px window");
    assert_eq!(o.offset, 0.0);
    assert!(h.query_by_label("Scroll ribbon left").is_none(), "nothing to the left yet");
    h.get_by_label("Scroll ribbon right").click();
    h.run_steps(3);
    let scrolled = RibbonOverflow::get(&h.ctx).offset;
    assert!(scrolled > 0.0);
    h.get_by_label("Scroll ribbon left").click();
    h.run_steps(3);
    assert!(RibbonOverflow::get(&h.ctx).offset < scrolled);
    // A tab that fits shows no arrows.
    let h = app_at(1440.0, "Automate");
    assert!(!RibbonOverflow::get(&h.ctx).overflows());
    assert!(h.query_by_label("Scroll ribbon right").is_none());
}

#[test]
fn each_tab_keeps_a_scroll_offset_of_its_own() {
    // Scrolling Home must not hide the first groups of another overflowing tab, and Home keeps
    // its offset while the other tab is used.
    let mut h = app_at(700.0, "Home");
    h.get_by_label("Scroll ribbon right").click();
    h.run_steps(3);
    let home_offset = RibbonOverflow::get(&h.ctx).offset;
    assert!(home_offset > 0.0);
    let other = TABS.iter().copied().find(|t| *t != "Home" && RibbonOverflow::get(&app_at(700.0, t).ctx).overflows());
    let other = other.expect("another tab overflows a 700 px window");
    h.state_mut().ui.ribbon_tab = other.into();
    h.run_steps(3);
    let o = RibbonOverflow::get(&h.ctx);
    assert!(o.overflows(), "{other} overflows");
    assert_eq!(o.offset, 0.0, "{other} starts at its left edge");
    h.get_by_label("Scroll ribbon right").click();
    h.run_steps(3);
    assert!(RibbonOverflow::get(&h.ctx).offset > 0.0);
    h.state_mut().ui.ribbon_tab = "Home".into();
    h.run_steps(3);
    assert_eq!(RibbonOverflow::get(&h.ctx).offset, home_offset, "Home kept its offset");
}

#[test]
fn the_scroll_arrows_stay_inside_a_narrow_window() {
    // The tab row can be wider than a narrow window (more so with wider system fonts); the card
    // and its arrows stay inside the window, and the right arrow still scrolls.
    for width in [600.0, 480.0] {
        let mut h = app_at(width, "Home");
        let arrow = h.get_by_label("Scroll ribbon right").rect();
        assert!(arrow.max.x <= width, "{width}: the arrow ends at {}", arrow.max.x);
        h.get_by_label("Scroll ribbon right").click();
        h.run_steps(3);
        assert!(RibbonOverflow::get(&h.ctx).offset > 0.0, "{width}: the right arrow scrolls");
    }
}

#[derive(Default)]
struct Clicks {
    main: u32,
    arrow: u32,
}

#[test]
/// The main button clicks once and the arrow twice separately; the tooltip keeps the platform shortcut, and the icon sits above the full-width label.
fn big_split_button_separates_the_default_action_from_the_menu() {
    // The ribbon picks the shortcut by the OS egui reports, which kittest pins to Nix unless told:
    // pin macOS, so the check is the same on every host (the other platforms' text is in
    // `widgets::tests`).
    let mut h = crate::common::builder().with_size(vec2(300.0, 200.0)).with_os(egui::os::OperatingSystem::Mac).build_ui_state(
        |ui, c: &mut Clicks| {
            let (main, arrow) = widgets::big_split_button(ui, Icon::Sum, "Auto\nSum", "AutoSum");
            c.main += u32::from(main.clicked());
            c.arrow += u32::from(arrow.clicked());
        },
        Clicks::default(),
    );
    h.run_steps(2);
    h.get_by_label("Auto Sum").click();
    h.run_steps(2);
    h.get_by_label("Auto Sum options").click();
    h.run_steps(2);
    h.get_by_label("Auto Sum options").click();
    h.run_steps(2);
    assert_eq!((h.state().main, h.state().arrow), (1, 2));
    // The tooltip carries the shortcut hint, as every ribbon button's does. egui 0.37 waits for
    // the pointer to settle before opening it, so remove the hover delay and let it settle.
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.get_by_label("Auto Sum").hover();
    h.run_steps(6);
    assert!(h.query_by_value("AutoSum (\u{2318}\u{21e7}T)").is_some(), "tooltip with the macOS shortcut");
    // The icon part sits above the label part, across the full width.
    let main = h.get_by_label("Auto Sum").rect();
    let arrow = h.get_by_label("Auto Sum options").rect();
    assert!(main.max.y <= arrow.min.y + 0.5);
    assert!((main.min.x - arrow.min.x).abs() < 0.5 && (main.max.x - arrow.max.x).abs() < 0.5);
}

#[test]
fn formulas_autosum_runs_sum_and_its_arrow_opens_the_menu() {
    let mut h = app_at(1600.0, "Formulas");
    h.get_by_label("AutoSum").click();
    h.run_steps(2);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "the button runs AutoSum");
    assert!(h.query_by_label("Average").is_none());
    h.get_by_label("AutoSum options").click();
    h.run_steps(2);
    h.get_by_label("Average").click();
    h.run_steps(2);
    let last = h.state().session.journal.iter().rev().find(|(id, _)| id == "formulas.autoSum").map(|(_, p)| p.clone());
    assert_eq!(last.and_then(|p| p.get("function").and_then(|f| f.as_str().map(str::to_string))).as_deref(), Some("AVERAGE"));
}
