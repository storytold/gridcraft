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
fn home_groups_are_measured_in_order() {
    let h = app_at(1440.0, "Home");
    let groups = RibbonOverflow::get(&h.ctx).groups.clone();
    assert!(!groups.is_empty(), "Home's groups are recorded");
    assert!(groups.iter().any(|(n, w)| n == "Clipboard" && *w > 50.0));
    // Recording follows render order: Clipboard renders before Font.
    let pos = |name: &str| groups.iter().position(|(n, _)| n == name);
    assert!(pos("Clipboard").is_some_and(|c| pos("Font").is_some_and(|f| c < f)));
}

#[test]
fn every_tab_records_its_groups() {
    for tab in TABS {
        let h = app_at(1440.0, tab);
        let groups = RibbonOverflow::get(&h.ctx).groups.clone();
        assert!(!groups.is_empty(), "{tab}: groups recorded");
        assert!(groups.iter().all(|(_, w)| *w > 1.0), "{tab}: widths positive");
    }
}

#[test]
fn an_overflowing_tab_shows_scroll_arrows_that_scroll() {
    // 540 px is the collapse floor: fully collapsed, Home's row (measured ≈ 485 px) is still
    // wider than the card (width − 56), so arrows are the last resort — every group whose
    // launcher saves width is already collapsed (probed 480–550: overflow ⟺ width < 540.5).
    let mut h = app_at(540.0, "Home");
    let o = RibbonOverflow::get(&h.ctx);
    assert!(o.overflows(), "Home is wider than a 540 px window even collapsed");
    let plan = gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Home");
    let saving: Vec<_> =
        gridcraft_ui_egui::ribbon::group_measures(&h.ctx).iter().filter(|m| (m.width - m.launcher) > 0.5).map(|m| m.name.clone()).collect();
    assert!(
        !saving.is_empty() && saving.iter().all(|s| plan.contains(s)),
        "collapse exhausted: every saving>0 group {saving:?} is a launcher in {plan:?}"
    );
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
fn collapsed_group_opens_popup_and_runs_its_command() {
    // 640 px: the collapse engine itself turns Font into a launcher (the plan is computed per
    // frame now, so a hand-written plan would be recomputed away).
    let mut h = app_at(640.0, "Home");
    assert!(gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Home").iter().any(|n| n == "Font"), "Font is collapsed at 640 px");
    h.run_steps(1);
    let launcher = h.get_by_label("Font"); // accessible name from launcher_button
    let hidden = h.query_by_label("Bold").is_none(); // toggle_button's accessible name
    assert!(hidden, "body hidden while collapsed");
    launcher.click();
    h.run_steps(2);
    h.get_by_label("Bold").click(); // toggle_button paints its icon; queried by its accessible name
    h.run_steps(2);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.bold"), "control inside the popup runs");
    h.get_by_label("Font").click(); // second click closes; command not duplicated
    h.run_steps(1);
    assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == "home.bold").count(), 1);
}

#[test]
fn collapsed_group_dropdown_survives_nested_control_popups() {
    // 640 px: the collapse engine itself turns Font into a launcher (the plan is computed per
    // frame now, so a hand-written plan would be recomputed away).
    let mut h = app_at(640.0, "Home");
    h.get_by_label("Font").click();
    h.run_steps(2);
    // The Font combo inside the dropdown opens a popup of its own.
    h.get_by_label("Calibri").click();
    h.run_steps(2);
    assert!(h.query_by_label("Bold").is_some(), "group dropdown still open next to the nested popup");
    h.get_by_label("Arial").click(); // a row of the nested font list
    h.run_steps(2);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.fontName"), "nested popup row runs its command");
    assert!(h.query_by_label("Bold").is_some(), "dropdown still open after the nested pick");
    // A click on the sheet outside closes the dropdown.
    h.hover_at(egui::pos2(800.0, 400.0));
    for pressed in [true, false] {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: egui::pos2(800.0, 400.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    h.run_steps(1);
    assert!(h.query_by_label("Bold").is_none(), "click outside closes the dropdown");
    // Escape closes a freshly opened dropdown.
    h.get_by_label("Font").click();
    h.run_steps(2);
    h.key_press(egui::Key::Escape);
    h.run_steps(1);
    assert!(h.query_by_label("Bold").is_none(), "Escape closes the dropdown");
}

#[test]
fn tab_switch_without_a_click_does_not_reopen_a_dropdown() {
    // Tab switches that no pointer click carries (the control channel, a command, header/footer
    // editing) must close an open group dropdown for good: returning to the tab must not
    // re-open it without a click on its launcher.
    let mut h = app_at(640.0, "Home");
    assert!(gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Home").iter().any(|n| n == "Font"), "Font is collapsed at 640 px");
    h.get_by_label("Font").click();
    h.run_steps(2);
    assert!(h.query_by_label("Bold").is_some(), "dropdown open on Home");
    // The control channel's path: `ribbon_tab` set directly, no click in the frame.
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(2);
    h.state_mut().ui.ribbon_tab = "Home".into();
    h.run_steps(2);
    assert!(h.query_by_label("Bold").is_none(), "returning to Home must not re-open the dropdown");
    h.get_by_label("Font").click();
    h.run_steps(2);
    assert!(h.query_by_label("Bold").is_some(), "clicking the launcher re-opens it");
}

#[test]
fn formulas_autosum_runs_on_the_icon_and_opens_its_menu_on_the_label() {
    let mut h = app_at(1600.0, "Formulas");
    h.get_by_label("AutoSum").click();
    h.run_steps(2);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "the icon runs AutoSum");
    assert!(h.query_by_label("Average").is_none());
    h.get_by_label("AutoSum options").click();
    h.run_steps(2);
    h.get_by_label("Average").click();
    h.run_steps(2);
    let last = h.state().session.journal.iter().rev().find(|(id, _)| id == "formulas.autoSum").map(|(_, p)| p.clone());
    assert_eq!(last.and_then(|p| p.get("function").and_then(|f| f.as_str().map(str::to_string))).as_deref(), Some("AVERAGE"));
}

#[test]
fn full_width_keeps_every_group_expanded() {
    let h = app_at(1440.0, "Home");
    assert!(!RibbonOverflow::get(&h.ctx).groups.is_empty());
    assert!(gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Home").is_empty(), "nothing collapsed at full width");
}

#[test]
fn plan_does_not_flip_flop_at_the_boundary() {
    // Stability: two harnesses at the same width reach the same plan, and a
    // second run of steps does not change it (harness cannot resize in place;
    // rebuild via app_at, which settles the one-frame measurement lag).
    let a = app_at(900.0, "Home");
    let plan_a = gridcraft_ui_egui::ribbon::collapsed_names(&a.ctx, "Home");
    let mut b = app_at(900.0, "Home");
    b.run_steps(2);
    assert_eq!(gridcraft_ui_egui::ribbon::collapsed_names(&b.ctx, "Home"), plan_a, "plan is stable across frames");
    // Strengthened (the brief's form passed trivially): a flip-flop alternates frame by frame,
    // so the plan must match after every further single-frame step, not just once settled.
    for _ in 0..2 {
        b.run_steps(1);
        assert_eq!(gridcraft_ui_egui::ribbon::collapsed_names(&b.ctx, "Home"), plan_a, "plan is stable in every frame");
    }
    // Monotonicity: 920 px collapses no more groups than 900 px.
    let c = app_at(920.0, "Home");
    assert!(gridcraft_ui_egui::ribbon::collapsed_names(&c.ctx, "Home").len() <= plan_a.len());
}

#[test]
fn narrow_home_collapses_rightmost_groups_first() {
    let h = app_at(900.0, "Home");
    let plan = gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Home");
    assert!(!plan.is_empty(), "some groups collapsed at 900 px");
    assert!(!RibbonOverflow::get(&h.ctx).overflows(), "the collapsed tab fits");
    let names: Vec<_> = RibbonOverflow::get(&h.ctx).groups.iter().map(|(n, _)| n.clone()).collect();
    assert!(plan.iter().all(|p| names.contains(p)), "plan names exist on this tab");
    // Rightmost-first: the rightmost collapsed group renders right of the leftmost expanded one.
    let pos = |n: &str| names.iter().position(|x| x == n);
    if let (Some(max_collapsed), Some(min_expanded)) =
        (plan.iter().filter_map(|p| pos(p)).max(), names.iter().enumerate().filter(|(_, n)| !plan.contains(*n)).map(|(i, _)| i).min())
    {
        assert!(max_collapsed > min_expanded, "rightmost groups collapse first");
    }
}

#[test]
fn very_narrow_home_collapses_all_groups() {
    // 550 px: narrow enough that even the leftmost group must become a launcher for the row to
    // fit (six collapsed still overflow), and wide enough that the fully collapsed row fits.
    let h = app_at(550.0, "Home");
    let plan = gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Home");
    let saving = gridcraft_ui_egui::ribbon::group_measures(&h.ctx).iter().filter(|m| (m.width - m.launcher) > 0.5).count();
    assert!(!RibbonOverflow::get(&h.ctx).overflows(), "fits fully collapsed");
    assert_eq!(plan.len(), saving, "every collapsible group is collapsed");
    // A collapsed group's measure records its launcher row (never a stale expanded slot), and
    // keeps the expanded width the plan arithmetic needs.
    assert!(
        gridcraft_ui_egui::ribbon::group_measures(&h.ctx).iter().all(|m| m.launcher > 40.0 && m.width >= m.launcher),
        "collapsed groups record launcher-row measures: {:?}",
        gridcraft_ui_egui::ribbon::group_measures(&h.ctx)
    );
}

#[test]
fn every_tab_survives_320_px() {
    for tab in TABS {
        let h = app_at(320.0, tab); // implicit no-panic assertion
        assert!(RibbonOverflow::get(&h.ctx).visible_height > 0.0, "{tab}: card laid out");
        assert!(RibbonOverflow::get(&h.ctx).content_height <= RibbonOverflow::get(&h.ctx).visible_height + 0.5, "{tab}: still fits the card height");
    }
}

#[test]
fn every_tab_at_700_px_overflows_only_when_collapse_is_exhausted() {
    // The mixed regime's width: the card may still fit (nothing collapsed needed) or collapse a
    // few groups — but if it still overflows, every group whose launcher saves width must already
    // be a launcher (scroll arrows are the last resort, never the first).
    for tab in TABS {
        let h = app_at(700.0, tab);
        assert!(RibbonOverflow::get(&h.ctx).visible_height > 0.0, "{tab}: card laid out");
        assert!(RibbonOverflow::get(&h.ctx).content_height <= RibbonOverflow::get(&h.ctx).visible_height + 0.5, "{tab}: still fits the card height");
        if RibbonOverflow::get(&h.ctx).overflows() {
            let plan = gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, tab);
            let saving: Vec<_> =
                gridcraft_ui_egui::ribbon::group_measures(&h.ctx).iter().filter(|m| (m.width - m.launcher) > 0.5).map(|m| m.name.clone()).collect();
            assert!(
                saving.iter().all(|s| plan.contains(s)),
                "{tab}: overflows at 700 px with saving>0 groups {saving:?} not all collapsed in {plan:?}"
            );
        }
    }
}

#[test]
fn switching_tabs_does_not_apply_the_previous_tabs_plan() {
    let mut h = app_at(700.0, "Home");
    let home_plan = gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Home");
    assert!(!home_plan.is_empty(), "Home collapses at 700 px");
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(1);
    // Insert's plan key starts unset: its first frame treats all groups expanded,
    // then recomputes only from Insert's own measurements.
    let insert_plan = gridcraft_ui_egui::ribbon::collapsed_names(&h.ctx, "Insert");
    assert!(insert_plan != home_plan);
}
