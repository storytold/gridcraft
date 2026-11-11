//! Ribbon layout: the big split button, the scroll arrows of an overflowing tab, and every tab
//! fitting the card's height.

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::icons::Icon;
use gridcraft_ui_egui::ribbon::{RIBBON_LABEL_H, RibbonLabels, RibbonOverflow, RibbonSections, TABS};
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

/// A harness at `size` logical points rendered at Excel's 125% desktop scale, so a case stated in
/// client points can also be checked against the pixels Excel's measurements were taken at.
fn app_at_size(size: egui::Vec2, tab: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = tab.into();
    let mut h = crate::common::builder().with_size(size).with_pixels_per_point(1.25).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

/// The Home groups' planned levels, in Excel's order: 0 is one compact row, 1 a natural body,
/// and 2/3 the narrower row counts of Alignment and Number.
fn home_levels(h: &Harness) -> Vec<(String, u8)> {
    RibbonSections::levels(&h.ctx, "Home")
}

#[test]
fn home_adapts_its_group_rows_to_the_card_width_like_excel() {
    // 819 pt (1024 px at 125%): Clipboard keeps its three rows, Styles its three stacked buttons,
    // and every other group becomes the one-row dropdown Excel shows there.
    let narrow = app_at_size(vec2(819.0, 614.0), "Home");
    assert_eq!(
        home_levels(&narrow),
        [
            ("Clipboard".to_string(), 1),
            ("Font".to_string(), 0),
            ("Alignment".to_string(), 0),
            ("Number".to_string(), 0),
            ("Styles".to_string(), 1),
            ("Cells".to_string(), 0),
            ("Editing".to_string(), 0),
        ]
    );
    assert!(narrow.query_by_label("Font").is_some(), "Font is one compact row");
    assert!(narrow.query_by_label("Editing").is_some(), "Editing is one compact row");
    // Only the compact row is drawn: the natural body waits in its menu.
    assert!(narrow.query_by_label("Border presets").is_none());

    // 983 pt (1228 px at 125%): Font keeps its three rows, Alignment and Number drop to two, and
    // Cells and Editing remain compact.
    let medium = app_at_size(vec2(983.0, 614.0), "Home");
    assert_eq!(
        home_levels(&medium),
        [
            ("Clipboard".to_string(), 1),
            ("Font".to_string(), 1),
            ("Alignment".to_string(), 2),
            ("Number".to_string(), 2),
            ("Styles".to_string(), 1),
            ("Cells".to_string(), 0),
            ("Editing".to_string(), 0),
        ]
    );
    assert!(medium.query_by_label("Font").is_none(), "Font is natural again");
    assert!(medium.query_by_label("Cells").is_some(), "Cells is still one row");

    // 1228 pt: the full Home tab, with Editing's two rows and the three-row groups.
    let wide = app_at_size(vec2(1228.0, 768.0), "Home");
    assert_eq!(
        home_levels(&wide),
        [
            ("Clipboard".to_string(), 1),
            ("Font".to_string(), 1),
            ("Alignment".to_string(), 3),
            ("Number".to_string(), 3),
            ("Styles".to_string(), 1),
            ("Cells".to_string(), 1),
            ("Editing".to_string(), 1),
        ]
    );
    for compact in ["Font", "Alignment", "Number", "Cells", "Editing"] {
        assert!(wide.query_by_label(compact).is_none(), "{compact} is natural at 1228 pt");
    }

    // 1400 outer px (1120 pt): the three-row forms still stand, and Cells and Editing are natural
    // above their 1109 pt breakpoint.
    let rowed = app_at_size(vec2(1120.0, 768.0), "Home");
    assert_eq!(
        home_levels(&rowed),
        [
            ("Clipboard".to_string(), 1),
            ("Font".to_string(), 1),
            ("Alignment".to_string(), 3),
            ("Number".to_string(), 3),
            ("Styles".to_string(), 1),
            ("Cells".to_string(), 1),
            ("Editing".to_string(), 1),
        ]
    );

    // 1600 outer px (1280 pt): Excel spreads Font and Alignment back into their wide two-row
    // forms; Number keeps its three rows until 1900.
    let spread = app_at_size(vec2(1280.0, 768.0), "Home");
    assert_eq!(
        home_levels(&spread),
        [
            ("Clipboard".to_string(), 1),
            ("Font".to_string(), 2),
            ("Alignment".to_string(), 2),
            ("Number".to_string(), 3),
            ("Styles".to_string(), 1),
            ("Cells".to_string(), 1),
            ("Editing".to_string(), 1),
        ]
    );

    // 1900 outer px (1520 pt): Number takes its wide two-row form too.
    let widest = app_at_size(vec2(1520.0, 768.0), "Home");
    assert_eq!(
        home_levels(&widest),
        [
            ("Clipboard".to_string(), 1),
            ("Font".to_string(), 2),
            ("Alignment".to_string(), 2),
            ("Number".to_string(), 2),
            ("Styles".to_string(), 1),
            ("Cells".to_string(), 1),
            ("Editing".to_string(), 1),
        ]
    );
    for natural in ["Font", "Alignment", "Number"] {
        assert!(widest.query_by_label(natural).is_none(), "{natural} is natural at 1900 px");
    }
}

#[test]
fn a_compact_group_holds_its_whole_natural_body_in_its_menu() {
    // At 819 pt the Font group is one row; opening it puts the font name, size and style controls
    // of the natural body in the menu, one click away.
    let mut h = app_at_size(vec2(819.0, 614.0), "Home");
    h.get_by_label("Font").click();
    h.run_steps(2);
    assert!(h.query_by_label("Border presets").is_some(), "the natural Font body is in the menu");
    assert!(h.query_by_label("Increase Font Size").is_some());
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
        let names: Vec<&str> = labels.labels.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(names, expected_groups(tab), "{tab}: a group label is missing or misnamed");
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
}

/// Excel's group names for each standard tab, in the order `sep`/`group_end` record them.
fn expected_groups(tab: &str) -> &'static [&'static str] {
    match tab {
        "Home" => &["Clipboard", "Font", "Alignment", "Number", "Styles", "Cells", "Editing"],
        "Insert" => &["Tables", "Illustrations", "Charts", "Filters", "Sparklines", "Form Controls", "Cell Controls", "Links", "Text", "Symbols"],
        "Draw" => &["Tools", "Pens", "Convert"],
        "Page Layout" => &["Themes", "Page Setup", "Sheet Options"],
        "Formulas" => &["Function Library", "Defined Names", "Formula Auditing", "Calculation"],
        "Data" => &["Get & Transform Data", "Sort & Filter", "Data Tools", "Forecast", "Outline", "Analysis"],
        "Review" => &["Proofing", "Comments", "Protect"],
        "View" => &["Workbook Views", "Show", "Zoom", "Window", "Interface"],
        "Automate" => &["Macros", "Tools"],
        _ => unreachable!("{tab} is not a standard tab"),
    }
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
