//! Next / Previous Sheet from the tab strip's arrows and from Ctrl+PageDown / Ctrl+PageUp skip
//! hidden sheets, and stay on the current sheet when no visible sheet lies that way: a hidden
//! sheet is never reached by stepping sideways.

use std::sync::Arc;

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::model::{Sheet, Visibility, Workbook};
use gridcraft_engine::{DocState, Session};
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// One sheet per letter (`V` visible, `H` hidden, `X` very hidden), with sheet `active` active.
fn harness(kinds: &str, active: usize) -> Harness {
    let mut wb = Workbook::new();
    wb.sheets = kinds
        .chars()
        .enumerate()
        .map(|(i, k)| {
            let mut sh = Sheet::new(format!("{k}{i}"));
            sh.visibility = match k {
                'H' => Visibility::Hidden,
                'X' => Visibility::VeryHidden,
                _ => Visibility::Visible,
            };
            Arc::new(sh)
        })
        .collect();
    wb.active_sheet = active;
    let mut s = Session::new();
    s.add_document(DocState::new(wb, None, "Book1".into()));
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    assert_eq!(active_sheet(&h), active);
    h
}

fn active_sheet(h: &Harness) -> usize {
    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.active().map(|s| s.visibility), Some(Visibility::Visible), "the active sheet must be visible");
    wb.active_sheet
}

fn arrow(h: &mut Harness, label: &str) -> usize {
    h.get_by_label(label).click();
    h.run_steps(3);
    active_sheet(h)
}

fn ctrl_page(h: &mut Harness, key: Key) -> usize {
    h.input_mut().events.push(Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers: CTRL });
    h.run_steps(3);
    active_sheet(h)
}

const NEXT: &str = "Next sheet";
const PREVIOUS: &str = "Previous sheet";

#[test]
fn arrows_skip_hidden_sheets_and_stop_at_the_last_visible_one() {
    // [Visible, Hidden]: nothing to the right is visible, so Next stays on the first sheet.
    let mut h = harness("VH", 0);
    assert_eq!(arrow(&mut h, NEXT), 0);
    assert_eq!(arrow(&mut h, PREVIOUS), 0);

    // [Hidden, Visible]: nothing to the left is visible.
    let mut h = harness("HV", 1);
    assert_eq!(arrow(&mut h, PREVIOUS), 1);
    assert_eq!(arrow(&mut h, NEXT), 1);

    // [Visible, Hidden, Visible]: the hidden sheet in the middle is stepped over both ways.
    let mut h = harness("VHV", 0);
    assert_eq!(arrow(&mut h, NEXT), 2);
    assert_eq!(arrow(&mut h, NEXT), 2);
    assert_eq!(arrow(&mut h, PREVIOUS), 0);
    assert_eq!(arrow(&mut h, PREVIOUS), 0);

    // All but one sheet hidden or very hidden: the arrows never leave it.
    let mut h = harness("HXVHX", 2);
    for label in [NEXT, NEXT, PREVIOUS, PREVIOUS] {
        assert_eq!(arrow(&mut h, label), 2, "{label}");
    }
}

#[test]
fn ctrl_page_down_and_up_skip_hidden_sheets_and_stop_at_the_last_visible_one() {
    let mut h = harness("VH", 0);
    assert_eq!(ctrl_page(&mut h, Key::PageDown), 0);
    assert_eq!(ctrl_page(&mut h, Key::PageUp), 0);

    let mut h = harness("HV", 1);
    assert_eq!(ctrl_page(&mut h, Key::PageUp), 1);
    assert_eq!(ctrl_page(&mut h, Key::PageDown), 1);

    let mut h = harness("VHV", 0);
    assert_eq!(ctrl_page(&mut h, Key::PageDown), 2);
    assert_eq!(ctrl_page(&mut h, Key::PageDown), 2);
    assert_eq!(ctrl_page(&mut h, Key::PageUp), 0);
    assert_eq!(ctrl_page(&mut h, Key::PageUp), 0);

    let mut h = harness("HXVHX", 2);
    for key in [Key::PageDown, Key::PageDown, Key::PageUp, Key::PageUp] {
        assert_eq!(ctrl_page(&mut h, key), 2, "{key:?}");
    }
}
