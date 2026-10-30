//! Form dialogs open ready to type, as in Excel: Alt+H, O, W then a number and Enter sets the
//! column width without clicking into the field first.

use egui::{Event, Key, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(6);
    h
}

fn type_and_enter(h: &mut egui_kittest::Harness<'static, SheetApp>, text: &str) {
    for c in text.chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.key_press(Key::Enter);
    h.run_steps(6);
}

#[test]
fn column_width_opens_with_the_width_ready_to_type() {
    let mut h = harness();
    h.state_mut().open_dialog("columnWidth", json!({}));
    h.run_steps(6);
    type_and_enter(&mut h, "12");
    assert!(h.state().dialog.is_none(), "Enter confirmed the dialog");
    let chars = {
        let d = h.state().session.active().unwrap();
        let sheet = d.wb.active().unwrap();
        gridcraft_engine::cmd::format::points_to_chars(sheet.col_width(CellRef::parse("C3").unwrap().col) as f64)
    };
    assert!((chars - 12.0).abs() < 0.2, "column C is 12 characters wide, got {chars}");
}

#[test]
fn row_height_opens_with_the_height_ready_to_type() {
    let mut h = harness();
    h.state_mut().open_dialog("rowHeight", json!({}));
    h.run_steps(6);
    let shown = h.state().dialog.as_ref().and_then(|d| d.values.get("points")).and_then(|v| v.as_f64());
    // A new workbook's rows are 26 px: 19.5 points.
    assert!(shown.is_some_and(|v| (v - 19.5).abs() < 0.01), "a default row opens at 19.5 points, got {shown:?}");
    type_and_enter(&mut h, "30");
    assert!(h.state().dialog.is_none(), "Enter confirmed the dialog");
    let height = {
        let d = h.state().session.active().unwrap();
        d.wb.active().unwrap().row_height(CellRef::parse("C3").unwrap().row)
    };
    assert!((height as f64 - 40.0).abs() < 0.6, "row 3 is 30 points (40 pixels) high, got {height}");
}

#[test]
fn command_search_arrow_and_page_keys_move_the_selection_and_keep_the_search_focused() {
    let mut h = harness();
    h.state_mut().open_dialog("commandSearch", json!({}));
    h.run_steps(6);
    h.input_mut().events.push(Event::Text("a".into()));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.list_index), Some(0));
    for _ in 0..2 {
        h.key_press(Key::ArrowDown);
        h.run_steps(2);
    }
    assert_eq!(h.state().dialog.as_ref().map(|d| d.list_index), Some(2), "two ArrowDown presses");
    h.key_press(Key::PageDown);
    h.run_steps(2);
    let after_page = h.state().dialog.as_ref().map(|d| d.list_index).unwrap_or(0);
    assert!(after_page > 2, "PageDown moved past the third row, got {after_page}");
    assert!(h.ctx.memory(|m| m.focused()).is_some(), "the search field kept focus");
}

#[test]
fn command_palette_runs_the_highlighted_command_with_enter_and_lists_it_as_recent() {
    let mut h = harness();
    h.state_mut().open_dialog("commandSearch", json!({}));
    h.run_steps(4);
    for c in "freeze panes".chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.list_index), Some(0), "a new query starts at the top");
    // Freeze Panes is the first match; one down and back up returns to it.
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.list_index), Some(1));
    h.key_press(Key::ArrowUp);
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert!(h.state().dialog.is_none(), "Enter ran the command and closed the palette");
    let frozen = h.state().session.active().unwrap().wb.active().unwrap().freeze;
    assert!(frozen.is_some(), "Freeze Panes ran: {frozen:?}");
    assert_eq!(gridcraft_ui_egui::dialogs::palette_recent(&h.ctx).first().map(String::as_str), Some("view.freezePanes"));

    // Reopened with no query, it lists the command under Recent; Escape closes it unrun.
    h.state_mut().open_dialog("commandSearch", json!({}));
    h.run_steps(4);
    let _ = h.get_by_label_contains("Freeze Panes");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "Escape closed the palette");
}
