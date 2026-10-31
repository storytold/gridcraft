use super::{Geo, SheetApp, Value, json, vec2};
use crate::grid::DatePickerState;

const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

pub(crate) fn open(app: &mut SheetApp) {
    if !app.commit_edit(0, 0, false, false) {
        return;
    }
    let Some(d) = app.session.active() else { return };
    let Some(sh) = d.wb.active() else { return };
    let cell = d.selection.active;
    let sys = d.wb.date_system;
    let today = gridcraft_engine::core::date::serial_in_system(sys, gridcraft_engine::calc::now_serial()).floor();
    let value = d.wb.active().and_then(|sh| sh.cell(cell)).and_then(|c| match &c.value {
        Value::Number(n) => Some(n.floor()),
        _ => None,
    });
    let date = value
        .and_then(|n| gridcraft_engine::core::date::datetime_from_serial(sys, n))
        .unwrap_or_else(|| gridcraft_engine::core::date::datetime_from_serial(sys, today).unwrap_or_default());
    app.grid.date_picker = Some(DatePickerState {
        cell,
        doc_uid: d.uid,
        sheet: d.wb.active_sheet(),
        sheet_name: sh.name.clone(),
        sheet_file_id: sh.file_id,
        needs_focus: true,
        year: date.year,
        month: date.month.max(1),
        day: date.day.max(1),
    });
}

pub(super) fn show(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, was_open: bool) {
    let Some(mut state) = app.grid.date_picker.clone() else { return };
    let Some(doc) = app.session.active() else {
        app.grid.date_picker = None;
        return;
    };
    if doc.uid != state.doc_uid || doc.wb.active_sheet() != state.sheet {
        app.grid.date_picker = None;
        return;
    }
    let Some(sh) = doc.wb.active() else {
        app.grid.date_picker = None;
        return;
    };
    if sh.name != state.sheet_name || sh.file_id != state.sheet_file_id {
        app.grid.date_picker = None;
        return;
    }
    let sys = doc.wb.date_system;
    let rect = geo.screen_cell_rect(sh, state.cell);
    let screen = ui.ctx().content_rect();
    let (at, pivot) = if geo.rtl {
        if rect.bottom() + 240.0 <= screen.bottom() {
            // Area anchors align within the screen rect, then add the offset.
            (egui::pos2(rect.right() - screen.right(), rect.bottom() - screen.top()), egui::Align2::RIGHT_TOP)
        } else {
            (egui::pos2(rect.right() - screen.right(), rect.top() - screen.bottom()), egui::Align2::RIGHT_BOTTOM)
        }
    } else if rect.bottom() + 240.0 <= screen.bottom() {
        (rect.left_bottom(), egui::Align2::RIGHT_TOP)
    } else {
        (rect.left_top(), egui::Align2::RIGHT_BOTTOM)
    };
    let mut chosen = None;
    let mut close = false;
    let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let mut button_handled_enter = false;
    let moved = ui.input(|i| {
        let delta = if i.key_pressed(egui::Key::ArrowLeft) {
            -1
        } else if i.key_pressed(egui::Key::ArrowRight) {
            1
        } else if i.key_pressed(egui::Key::ArrowUp) {
            -7
        } else if i.key_pressed(egui::Key::ArrowDown) {
            7
        } else {
            0
        };
        if delta != 0 {
            let n = gridcraft_engine::core::date::serial_from_ymd(sys, i64::from(state.year), i64::from(state.month), i64::from(state.day))
                .unwrap_or(0.0)
                + f64::from(delta);
            if let Some(d) = gridcraft_engine::core::date::datetime_from_serial(sys, n) {
                state.year = d.year;
                state.month = d.month;
                state.day = d.day.max(1);
            }
            true
        } else {
            false
        }
    });
    let Some(first) = gridcraft_engine::core::date::serial_from_ymd(sys, i64::from(state.year), i64::from(state.month), 1) else {
        app.grid.date_picker = None;
        return;
    };
    let weekday = gridcraft_engine::core::date::datetime_from_serial(sys, first).map_or(0, |d| i64::from(d.weekday));
    let count = gridcraft_engine::core::date::days_in_month_in(sys, state.year, state.month);
    let response =
        egui::Area::new(egui::Id::unique("gridcraft.date_picker")).anchor(pivot, at.to_vec2()).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(224.0);
                ui.horizontal(|ui| {
                    if ui.button("‹").on_hover_text("Previous month").clicked() {
                        button_handled_enter |= enter_pressed;
                        let (year, month) = if state.month == 1 { (state.year.saturating_sub(1), 12) } else { (state.year, state.month - 1) };
                        if year >= if sys == gridcraft_engine::core::DateSystem::D1904 { 1904 } else { 1900 } {
                            state.year = year;
                            state.month = month;
                            state.day = state.day.min(gridcraft_engine::core::date::days_in_month_in(sys, year, month));
                        }
                    }
                    ui.label(format!(
                        "{} {}",
                        gridcraft_engine::core::date::MONTHS.get(state.month.saturating_sub(1) as usize).copied().unwrap_or("January"),
                        state.year
                    ));
                    if ui.button("›").on_hover_text("Next month").clicked() {
                        button_handled_enter |= enter_pressed;
                        let (year, month) = if state.month == 12 { (state.year.saturating_add(1), 1) } else { (state.year, state.month + 1) };
                        if year <= 9999 {
                            state.year = year;
                            state.month = month;
                            state.day = state.day.min(gridcraft_engine::core::date::days_in_month_in(sys, year, month));
                        }
                    }
                });
                egui::Grid::new("gridcraft.date_picker.days").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
                    for name in DAY_NAMES {
                        ui.label(name);
                    }
                    ui.end_row();
                    for slot in 0..42_i64 {
                        let day = slot - weekday + 1;
                        if (1..=i64::from(count)).contains(&day) {
                            let day_response = ui.selectable_label(u32::try_from(day).ok() == Some(state.day), day.to_string());
                            if (state.needs_focus || moved) && u32::try_from(day).ok() == Some(state.day) {
                                day_response.request_focus();
                            }
                            if day_response.clicked() && (!enter_pressed || u32::try_from(day).ok() == Some(state.day)) {
                                button_handled_enter |= enter_pressed;
                                chosen = Some(first + (day - 1) as f64);
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new(" "));
                        }
                        if slot % 7 == 6 {
                            ui.end_row();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("Today").clicked() {
                        button_handled_enter |= enter_pressed;
                        chosen = Some(gridcraft_engine::core::date::serial_in_system(sys, gridcraft_engine::calc::now_serial()).floor());
                    }
                    if ui.button("Cancel").clicked() {
                        button_handled_enter |= enter_pressed;
                        close = true;
                    }
                });
            });
        });
    ui.input(|i| {
        if i.key_pressed(egui::Key::Escape) {
            close = true;
        }
    });
    chosen = enter_selection(enter_pressed, button_handled_enter, chosen, sys, state.year, state.month, state.day);
    if let Some(serial) = chosen {
        app.run_or_alert("cell.set", json!({"cell": state.cell.a1(), "input": serial.to_string()}));
        close = true;
    }
    if moved {
        ui.ctx().request_repaint();
    }
    if close || super::editor::popup_dismissed(&response.response, was_open) {
        app.grid.date_picker = None;
    } else {
        state.needs_focus = false;
        app.grid.date_picker = Some(state);
    }
}

fn enter_selection(
    enter_pressed: bool,
    button_handled_enter: bool,
    chosen: Option<f64>,
    sys: gridcraft_engine::core::DateSystem,
    year: i32,
    month: u32,
    day: u32,
) -> Option<f64> {
    if enter_pressed && !button_handled_enter && chosen.is_none() {
        gridcraft_engine::core::date::serial_from_ymd(sys, i64::from(year), i64::from(month), i64::from(day))
    } else {
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::enter_selection;

    #[test]
    fn selected_days_use_the_workbooks_date_system_and_keep_february_1900() {
        use gridcraft_engine::core::DateSystem;
        assert_eq!(gridcraft_engine::core::date::serial_from_ymd(DateSystem::D1900, 1900, 2, 29), Some(60.0));
        assert_eq!(
            gridcraft_engine::core::date::serial_from_ymd(DateSystem::D1904, 2024, 2, 29)
                .and_then(|serial| gridcraft_engine::core::date::datetime_from_serial(DateSystem::D1904, serial))
                .map(|d| (d.year, d.month, d.day)),
            Some((2024, 2, 29))
        );
        assert_eq!(gridcraft_engine::core::date::serial_from_ymd(DateSystem::D1904, 1904, 1, 1), Some(0.0));
    }

    #[test]
    fn enter_does_not_replace_a_popup_button_action() {
        use gridcraft_engine::core::DateSystem;
        assert_eq!(enter_selection(true, true, None, DateSystem::D1900, 2024, 2, 29), None);
        assert_eq!(enter_selection(true, true, Some(60.0), DateSystem::D1900, 2024, 2, 29), Some(60.0));
    }
}
