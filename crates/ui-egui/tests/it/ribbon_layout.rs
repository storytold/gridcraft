//! Ribbon layout: the big split button, the scroll arrows of an overflowing tab, and every tab
//! fitting the card's height.

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::icons::Icon;
use gridcraft_ui_egui::ribbon::{RIBBON_LABEL_H, RibbonLabels, RibbonOverflow, RibbonSections, TABS};
use gridcraft_ui_egui::{SheetApp, widgets};
use serde_json::json;

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
    // Levels are the section's control rows (0 is the compact one-row dropdown): Clipboard 3,
    // Font 3/2, Alignment 3/2, Number 3/2, Styles 3/1, Cells 3/1, Editing 3.
    let expect = |width: f32, expected: [u8; 7]| {
        let h = app_at_size(vec2(width, 768.0), "Home");
        let names = ["Clipboard", "Font", "Alignment", "Number", "Styles", "Cells", "Editing"];
        let want: Vec<(String, u8)> = names.into_iter().zip(expected).map(|(name, level)| (name.to_string(), level)).collect();
        assert_eq!(home_levels(&h), want, "Home rows at {width} pt");
    };
    // 1024 outer px (819 pt): Clipboard keeps its three rows and Styles its three stacked buttons;
    // every other group is the one-row dropdown Excel shows there.
    expect(819.0, [3, 0, 0, 0, 3, 0, 0]);
    // 1228 outer px (983 pt): Font and Alignment take their three rows, Number is still one.
    expect(983.0, [3, 3, 3, 0, 3, 0, 0]);
    // 1400 outer px (1120 pt) and 1535 px (1228 pt): the three-row tab, Cells and Editing too.
    expect(1120.0, [3, 3, 3, 3, 3, 3, 3]);
    expect(1228.0, [3, 3, 3, 3, 3, 3, 3]);
    // 1600 outer px (1280 pt): Font and Alignment spread into their wide two-row forms.
    expect(1280.0, [3, 2, 2, 3, 3, 3, 3]);
    // 1900 outer px (1520 pt): Number does too, and Styles and Cells become one row of three.
    expect(1520.0, [3, 2, 2, 2, 1, 1, 3]);

    let level = |width: f32, label: &str| {
        let h = app_at_size(vec2(width, 768.0), "Home");
        home_levels(&h).into_iter().find(|(name, _)| name == label).map(|(_, level)| level)
    };
    // Just below each measured breakpoint (13 pt, outside the hysteresis dead band) and at it.
    for (width, label, expected) in [
        (806.0, "Clipboard", 0),
        (819.0, "Clipboard", 3),
        (916.0, "Font", 0),
        (929.0, "Font", 3),
        (947.0, "Alignment", 0),
        (960.0, "Alignment", 3),
        (1027.0, "Number", 0),
        (1040.0, "Number", 3),
        (1107.0, "Cells", 0),
        (1120.0, "Cells", 3),
        (1107.0, "Editing", 0),
        (1120.0, "Editing", 3),
        (1267.0, "Font", 3),
        (1280.0, "Font", 2),
        (1267.0, "Alignment", 3),
        (1280.0, "Alignment", 2),
        (1507.0, "Number", 3),
        (1520.0, "Number", 2),
        (1507.0, "Styles", 3),
        (1520.0, "Styles", 1),
        (1507.0, "Cells", 3),
        (1520.0, "Cells", 1),
    ] {
        assert_eq!(level(width, label), Some(expected), "Home › {label} at {width} pt");
    }

    // A compact group draws one row naming itself; its natural body waits in its menu.
    let narrow = app_at_size(vec2(819.0, 614.0), "Home");
    assert!(narrow.query_by_label("Font").is_some(), "Font is one compact row");
    assert!(narrow.query_by_label("Editing").is_some(), "Editing is one compact row");
    assert!(narrow.query_by_label("Border presets").is_none());
}

#[test]
fn measured_section_rows_switch_at_their_widths() {
    // Excel's measured 1↔3 transitions for the other tabs, in client points: the section is one
    // compact row below the width and its natural rows from it, except Symbols and Show, which
    // keep one row below and take a second one above.
    for (tab, label, below, at, below_level, at_level) in [
        ("Insert", "Illustrations", 1507.0, 1520.0, 0, 3),
        ("Insert", "Filters", 1267.0, 1280.0, 0, 2),
        ("Insert", "Filters", 1507.0, 1520.0, 2, 1),
        ("Insert", "Symbols", 1507.0, 1520.0, 1, 2),
        ("Page Layout", "Themes", 747.0, 760.0, 0, 3),
        ("Page Layout", "Scale to Fit", 667.0, 680.0, 0, 2),
        ("Formulas", "Defined Names", 1267.0, 1280.0, 0, 3),
        ("Formulas", "Formula Auditing", 1027.0, 1040.0, 0, 3),
        ("Data", "Data Tools", 1027.0, 1040.0, 0, 3),
        ("Data", "Outline", 1507.0, 1520.0, 0, 3),
        ("View", "Sheet View", 915.0, 928.0, 1, 2),
        ("View", "Workbook Views", 867.0, 880.0, 0, 3),
        ("View", "Show", 1267.0, 1280.0, 1, 2),
        ("View", "Window", 806.0, 819.0, 0, 3),
    ] {
        for (width, expected) in [(below, below_level), (at, at_level)] {
            let h = app_at_size(vec2(width, 768.0), tab);
            let level = RibbonSections::levels(&h.ctx, tab).into_iter().find(|(name, _)| name == label).map(|(_, level)| level);
            assert_eq!(level, Some(expected), "{tab} › {label} at {width} pt");
        }
    }
}

#[test]
fn filters_stacks_its_slicer_over_its_timeline_between_1280_and_1520() {
    // Excel's Filters group: two rows while the card is between 1280 and 1520 client points, one
    // row from 1520, and the whole group compact below 1280.
    let stacked = app_at_size(vec2(1280.0, 768.0), "Insert");
    let slicer = stacked.get_by_label("Slicer").rect();
    let timeline = stacked.get_by_label("Timeline").rect();
    assert!(timeline.top() >= slicer.bottom() - 0.5, "Filters stacks Slicer over Timeline at 1280 pt");
    assert!(timeline.center().y > slicer.center().y);

    let side_by_side = app_at_size(vec2(1520.0, 768.0), "Insert");
    let slicer = side_by_side.get_by_label("Slicer").rect();
    let timeline = side_by_side.get_by_label("Timeline").rect();
    assert!((timeline.center().y - slicer.center().y).abs() < 0.5, "Filters puts them on one row at 1520 pt");
    assert!(timeline.left() >= slicer.right() - 0.5);

    let compact = app_at_size(vec2(1267.0, 768.0), "Insert");
    assert!(compact.query_by_label("Filters").is_some(), "Filters is one compact row below 1280 pt");
    assert!(compact.query_by_label("Timeline").is_none(), "the compact row holds no controls until its menu opens");
}

#[test]
fn symbols_stack_the_equation_over_the_symbol_gallery_from_1520() {
    // Equation is a split button, so its main part is only the top half of its 72 px cell: the
    // two buttons share a cell top while they sit on one row.
    let one_row = app_at_size(vec2(1507.0, 768.0), "Insert");
    let equation = one_row.get_by_label("Equation").rect();
    let symbol = one_row.get_by_label("Symbol").rect();
    assert!((symbol.top() - equation.top()).abs() < 0.5, "Symbols is one row below 1520 pt");
    assert!(symbol.left() >= equation.right() - 0.5);

    let stacked = app_at_size(vec2(1520.0, 768.0), "Insert");
    let equation = stacked.get_by_label("Equation").rect();
    let symbol = stacked.get_by_label("Symbol").rect();
    assert!(symbol.top() > equation.top(), "Symbols stacks Equation over Symbol at 1520 pt");
}

#[test]
fn scale_to_fit_has_two_rows_of_fields_from_680() {
    let two_rows = app_at_size(vec2(680.0, 768.0), "Page Layout");
    assert_eq!(RibbonSections::levels(&two_rows.ctx, "Page Layout").into_iter().find(|(name, _)| name == "Scale to Fit").map(|(_, l)| l), Some(2));
    for label in ["Width:", "Height:", "Scale:"] {
        assert!(two_rows.query_by_label(label).is_some(), "Scale to Fit shows {label} at 680 pt");
    }
    let width = two_rows.get_by_label("Width:").rect();
    let scale = two_rows.get_by_label("Scale:").rect();
    assert!(scale.center().y > width.center().y, "Width and Height sit above Scale at 680 pt");

    let compact = app_at_size(vec2(667.0, 768.0), "Page Layout");
    assert!(compact.query_by_label("Scale to Fit").is_some(), "Scale to Fit is one compact row below 680 pt");
    assert!(compact.query_by_label("Width:").is_none());
}

#[test]
fn sheet_view_is_one_row_below_928_and_two_from_it() {
    let one_row = app_at_size(vec2(915.0, 768.0), "View");
    let new = one_row.get_by_label("New Sheet View").rect();
    let views = one_row.get_by_label("Custom Views").rect();
    assert!((views.center().y - new.center().y).abs() < 0.5, "Sheet View is one row below 928 pt");
    assert!(views.left() >= new.right() - 0.5);

    let two_rows = app_at_size(vec2(928.0, 768.0), "View");
    let new = two_rows.get_by_label("New Sheet View").rect();
    let views = two_rows.get_by_label("Custom Views").rect();
    assert!(views.center().y > new.center().y, "Sheet View stacks New over Custom Views at 928 pt");
}

#[test]
fn styles_and_cells_spread_their_three_buttons_over_one_row_at_1520() {
    let stacked = app_at_size(vec2(1507.0, 768.0), "Home");
    for buttons in [["Conditional Formatting", "Format as Table", "Cell Styles"], ["Insert", "Delete", "Format"]] {
        let ys: Vec<f32> = buttons.iter().map(|label| stacked.get_by_label(label).rect().center().y).collect();
        assert!(ys.windows(2).all(|w| w[1] > w[0]), "Home's three buttons stack below 1520 pt ({buttons:?})");
    }

    let one_row = app_at_size(vec2(1520.0, 768.0), "Home");
    for buttons in [["Conditional Formatting", "Format as Table", "Cell Styles"], ["Insert", "Delete", "Format"]] {
        let rects: Vec<egui::Rect> = buttons.iter().map(|label| one_row.get_by_label(label).rect()).collect();
        assert!(rects.windows(2).all(|w| (w[1].center().y - w[0].center().y).abs() < 0.5), "one row at 1520 pt ({buttons:?})");
        assert!(rects.windows(2).all(|w| w[1].left() >= w[0].right() - 0.5), "side by side at 1520 pt ({buttons:?})");
    }
}

#[test]
fn sheet_view_saves_and_lists_a_view() {
    // View › Sheet View › New opens Excel's name prompt; OK saves the view, and the group's
    // Custom Views menu lists it.
    let mut h = app_at_size(vec2(1440.0, 768.0), "View");
    h.get_by_label("New Sheet View").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("newSheetView"), "New Sheet View asks for a name");
    h.get_by_label_contains("OK").click();
    h.run_steps(3);
    let views = h.state_mut().session.execute("view.customViews", json!({})).unwrap();
    assert!(views.get("View1").is_some_and(|v| v.is_object()), "the view is saved as View1: {views}");

    h.get_by_label("Custom Views").click();
    h.run_steps(3);
    assert!(h.query_by_label("View1").is_some(), "the menu lists the saved view");
}

#[test]
fn every_new_group_renders_the_measured_rows_at_its_transition_widths() {
    // Renders each newly modelled group just below and at each of its transitions: the planner's
    // level is Excel's measured row count there (0 is the compact row, 1 and 2 its natural rows),
    // the group is named in the label band, and a tab that fits the card also fits its height.
    for (tab, group, width, level) in [
        ("Insert", "Filters", 1267.0, 0),
        ("Insert", "Filters", 1280.0, 2),
        ("Insert", "Filters", 1507.0, 2),
        ("Insert", "Filters", 1520.0, 1),
        ("Insert", "Symbols", 1507.0, 1),
        ("Insert", "Symbols", 1520.0, 2),
        ("Page Layout", "Scale to Fit", 667.0, 0),
        ("Page Layout", "Scale to Fit", 680.0, 2),
        ("View", "Sheet View", 915.0, 1),
        ("View", "Sheet View", 928.0, 2),
        ("View", "Show", 1267.0, 1),
        ("View", "Show", 1280.0, 2),
        ("Home", "Styles", 1507.0, 3),
        ("Home", "Styles", 1520.0, 1),
        ("Home", "Cells", 1507.0, 3),
        ("Home", "Cells", 1520.0, 1),
    ] {
        let h = app_at_size(vec2(width, 768.0), tab);
        let planned = RibbonSections::levels(&h.ctx, tab).into_iter().find(|(name, _)| name == group).map(|(_, level)| level);
        assert_eq!(planned, Some(level), "{tab} › {group} at {width} pt");
        let labels = RibbonLabels::get(&h.ctx);
        assert!(labels.labels.iter().any(|l| l.label == group), "{tab} › {group} is named at {width} pt");
        let o = RibbonOverflow::get(&h.ctx);
        if !o.overflows() {
            assert!(
                o.content_height <= o.visible_height + 0.5,
                "{tab} › {group}: {} px of groups in {} px at {width} pt",
                o.content_height,
                o.visible_height
            );
        }
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
        "Page Layout" => &["Themes", "Page Setup", "Scale to Fit", "Sheet Options"],
        "Formulas" => &["Function Library", "Defined Names", "Formula Auditing", "Calculation"],
        "Data" => &["Get & Transform Data", "Sort & Filter", "Data Tools", "Forecast", "Outline", "Analysis"],
        "Review" => &["Proofing", "Comments", "Protect"],
        "View" => &["Sheet View", "Workbook Views", "Show", "Zoom", "Window", "Interface"],
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
