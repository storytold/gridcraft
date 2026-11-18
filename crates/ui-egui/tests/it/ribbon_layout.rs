//! Ribbon layout: groups pack to the card width, and every tab fits the card's height.

use egui::{Shape, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::icons::Icon;
use gridcraft_ui_egui::ribbon::{HomeStep, PACK_SLACK, RibbonLayout, RibbonOverflow, TABS};
use gridcraft_ui_egui::{SheetApp, widgets};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn app_at(width: f32, tab: &str) -> Harness {
    app_lang(width, tab, Language::En)
}

fn app_lang(width: f32, tab: &str, language: Language) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = tab.into();
    app.ui.language = language;
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

/// Window widths the pack is checked at, narrow to wide. The card is inset 28 px on each side, so
/// [`RibbonLayout::budget`] is the window width minus 56. [`HomeStep::choose`] uses that budget.
const WIDTHS: [f32; 7] = [480.0, 700.0, 1000.0, 1200.0, 1440.0, 1800.0, 2200.0];

/// Home content width of the four steps on the pinned UI font, measured in this test.
/// Order: full labels, alignment icons, stacked style commands, wrapped number icons.
/// A step is the one drawn once `width + PACK_SLACK` fits the card and the wider step does not.
/// English then starts Full at about 1360 px, CompactAlign at 1266, StackStyles at 1226 and
/// WrapNumber at 1166. German: Full at about 1540, CompactAlign at 1392, StackStyles at 1308,
/// WrapNumber at 1248. Narrower windows keep WrapNumber and move whole groups into the menu.
const HOME_CONTENT_EN: [f32; 4] = [1299.2813, 1205.1875, 1165.9375, 1105.9375];
const HOME_CONTENT_DE: [f32; 4] = [1479.8438, 1331.5938, 1247.9063, 1187.9063];

fn assert_inside(h: &Harness, what: &str) {
    let layout = RibbonLayout::get(&h.ctx);
    assert!(layout.ribbon_right > layout.ribbon_left, "{what}: the card was laid out");
    assert!(layout.placed_right <= layout.ribbon_right + 0.5, "{what}: groups end at {} past {}", layout.placed_right, layout.ribbon_right);
    for shape in &h.output().shapes {
        let Shape::Text(text) = &shape.shape else { continue };
        // egui paints this when two widgets share an id. It is not a ribbon label.
        if text.galley.job.text.starts_with('\u{1f525}') {
            continue;
        }
        let right = text.pos.x + text.galley.size().x;
        let y = text.pos.y + text.galley.size().y * 0.5;
        if y < layout.ribbon_top || y > layout.ribbon_bottom {
            continue;
        }
        assert!(right <= layout.ribbon_right + 1.0, "{what}: {:?} ends at {right}, card at {}", text.galley.job.text, layout.ribbon_right);
    }
}

#[test]
fn home_pack_steps_follow_the_measured_widths() {
    for language in [Language::En, Language::De] {
        let wide = app_lang(2200.0, "Home", language);
        let content = RibbonLayout::get(&wide.ctx).content;
        let locked = match language {
            Language::De => HOME_CONTENT_DE,
            _ => HOME_CONTENT_EN,
        };
        for (i, (got, want)) in content.iter().zip(locked).enumerate() {
            assert!((got - want).abs() <= 1.0, "{language:?} step {i}: measured {got}, locked {want}");
        }
        assert!(content[0] > content[1] + 8.0, "{language:?}: full {} then icons {}", content[0], content[1]);
        assert!(content[1] > content[2] + 8.0, "{language:?}: icons {} then stacked styles {}", content[1], content[2]);
        assert!(content[2] > content[3] + 8.0, "{language:?}: stacked styles {} then wrapped numbers {}", content[2], content[3]);
        let mut previous = None;
        for width in WIDTHS {
            let h = app_lang(width, "Home", language);
            let layout = RibbonLayout::get(&h.ctx);
            assert_eq!(layout.step, HomeStep::choose(layout.content, layout.budget) as u8, "{language:?} at {width}");
            assert_inside(&h, &format!("{language:?} Home {width}"));
            if let Some(prev) = previous {
                assert!(layout.step <= prev, "{language:?}: {width}px unpacked to step {} from {prev}", layout.step);
            }
            previous = Some(layout.step);
            if layout.content[layout.step as usize] + PACK_SLACK > layout.budget {
                let menu = language.tr("More ribbon commands");
                assert!(h.query_by_label(menu).is_some(), "{language:?} at {width} still overflows without a menu");
            }
        }
    }
}

#[test]
fn an_overflowing_tab_opens_the_menu_instead_of_clipping() {
    let mut h = app_at(700.0, "Home");
    assert!(RibbonOverflow::get(&h.ctx).overflows(), "Home is wider than a 700 px window");
    assert_inside(&h, "Home 700");
    let chevron = h.get_by_label("More ribbon commands").rect();
    assert!(chevron.max.x <= 700.0, "the menu button ends at {}", chevron.max.x);
    assert!(h.query_by_label("Find & Select").is_none(), "the last Home group is not clipped onto the card");
    h.get_by_label("More ribbon commands").click();
    h.run_steps(3);
    assert!(h.query_by_label("Find & Select").is_some(), "a group that did not fit is in the menu");
    let fitting = app_at(2200.0, "Automate");
    assert!(fitting.query_by_label("More ribbon commands").is_none(), "a tab that fits has no menu");
}

#[test]
fn other_tabs_stay_inside_the_card() {
    for tab in ["Formulas", "Data", "View", "Insert"] {
        for width in [480.0, 700.0, 1200.0, 1600.0] {
            let h = app_at(width, tab);
            assert_inside(&h, &format!("{tab} {width}"));
            let layout = RibbonLayout::get(&h.ctx);
            if layout.fit < layout.groups {
                assert!(h.query_by_label("More ribbon commands").is_some(), "{tab} at {width}");
            }
        }
    }
}

#[test]
fn the_overflow_button_stays_inside_a_narrow_window() {
    for width in [600.0, 480.0] {
        let h = app_at(width, "Home");
        let chevron = h.get_by_label("More ribbon commands").rect();
        assert!(chevron.max.x <= width, "{width}: the menu button ends at {}", chevron.max.x);
        assert_inside(&h, &format!("narrow {width}"));
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
