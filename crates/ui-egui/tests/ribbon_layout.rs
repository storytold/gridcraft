//! Ribbon layout: the big split button, the scroll arrows of an overflowing tab, and every tab
//! fitting the card's height.

use egui::{FontDefinitions, FontFamily, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::icons::Icon;
use gridcraft_ui_egui::ribbon::{RibbonOverflow, TABS};
use gridcraft_ui_egui::{SheetApp, theme, widgets};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn app_at(width: f32, tab: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.ribbon_tab = tab.into();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(width, 700.0)).build_ui_state(
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
fn big_split_button_separates_the_default_action_from_the_menu() {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(300.0, 200.0)).build_ui_state(
        |ui, c: &mut Clicks| {
            if ui.ctx().cumulative_pass_nr() == 0 {
                // egui's own font stands in for the system UI font; it applies from the next pass.
                let mut fonts = FontDefinitions::default();
                let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
                fonts.families.insert(FontFamily::Name(theme::UI.into()), proportional.clone());
                fonts.families.insert(FontFamily::Name(theme::CELL.into()), proportional);
                ui.ctx().set_fonts(fonts);
                return;
            }
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
    // The icon part sits above the label part, across the full width.
    let main = h.get_by_label("Auto Sum").rect();
    let arrow = h.get_by_label("Auto Sum options").rect();
    assert!(main.max.y <= arrow.min.y + 0.5);
    assert!((main.min.x - arrow.min.x).abs() < 0.5 && (main.max.x - arrow.max.x).abs() < 0.5);
}

#[test]
fn formulas_autosum_runs_on_the_icon_and_opens_its_menu_on_the_label() {
    let mut h = app_at(1600.0, "Formulas");
    h.get_by_label("AutoSum").click();
    h.run_steps(2);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "the icon runs AutoSum");
    assert!(h.query_by_label("AVERAGE").is_none());
    h.get_by_label("AutoSum options").click();
    h.run_steps(2);
    h.get_by_label("AVERAGE").click();
    h.run_steps(2);
    let last = h.state().session.journal.iter().rev().find(|(id, _)| id == "formulas.autoSum").map(|(_, p)| p.clone());
    assert_eq!(last.and_then(|p| p.get("function").and_then(|f| f.as_str().map(str::to_string))).as_deref(), Some("AVERAGE"));
}
