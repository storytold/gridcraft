use egui::{Align, Button, Color32, Layout, RichText, ScrollArea, TextEdit, Ui};
use serde_json::{Value as Json, json};

use super::Dialog;
use crate::SheetApp;

/// Dialog allowing users to customize keyboard shortcuts for ribbon and menu commands.
pub(crate) fn customize_keyboard(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, open: &mut bool) {
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    let categories = ["Home", "Insert", "Page Layout", "Formulas", "Data", "Review", "View", "Automate", "All Commands"];
    let cat = d.values.get("category").and_then(Json::as_str).unwrap_or("Home").to_string();
    let mut pressed = d.values.get("pressed").and_then(Json::as_str).unwrap_or("").to_string();

    ui.horizontal_top(|ui| {
        // Left Column: Categories
        ui.vertical(|ui| {
            ui.label(RichText::new("Categories:").strong());
            ScrollArea::vertical().id_salt("cat_scroll").max_height(160.0).min_scrolled_width(140.0).show(ui, |ui| {
                for &c in &categories {
                    let selected = cat == c;
                    if ui.selectable_label(selected, c).clicked() {
                        d.values.insert("category".into(), json!(c));
                        d.values.insert("command".into(), json!(""));
                        d.values.remove("selected_key");
                    }
                }
            });
        });

        ui.add_space(8.0);

        // Refresh category and current_cmd after category interaction
        let cat = d.values.get("category").and_then(Json::as_str).unwrap_or("Home").to_string();
        let current_cmd = d.values.get("command").and_then(Json::as_str).unwrap_or("").to_string();

        // Right Column: Commands
        ui.vertical(|ui| {
            ui.label(RichText::new("Commands:").strong());
            let all_specs = gridcraft_engine::command_specs();
            let filtered: Vec<&gridcraft_engine::cmd::CommandSpec> =
                all_specs.iter().filter(|s| if cat == "All Commands" { true } else { s.menu.first().is_some_and(|m| *m == cat) }).collect();

            let active_cmd = if current_cmd.is_empty() { filtered.first().map(|s| s.id).unwrap_or("") } else { &current_cmd };

            ScrollArea::vertical().id_salt("cmd_scroll").max_height(160.0).min_scrolled_width(220.0).show(ui, |ui| {
                for s in &filtered {
                    let selected = active_cmd == s.id;
                    if ui.selectable_label(selected, s.label).clicked() {
                        d.values.insert("command".into(), json!(s.id));
                        d.values.remove("selected_key");
                    }
                }
            });

            if current_cmd.is_empty() && !active_cmd.is_empty() {
                d.values.insert("command".into(), json!(active_cmd));
            }
        });
    });

    ui.separator();

    let sel_id = d.values.get("command").and_then(Json::as_str).unwrap_or("").to_string();
    let sel_spec = gridcraft_engine::find_command(&sel_id);

    let mut current_keys: Vec<String> = Vec::new();
    if let Some(spec) = sel_spec
        && let Some(sc) = spec.shortcut
    {
        let norm_sc = crate::keymap::normalize_for_platform(sc, mac).unwrap_or_else(|| sc.to_string());
        let override_val = app.ui.shortcuts.get(&norm_sc).or_else(|| app.ui.shortcuts.get(sc));
        if override_val.is_none() || override_val == Some(&sel_id) {
            current_keys.push(norm_sc);
        }
    }
    for (k, v) in &app.ui.shortcuts {
        if v == &sel_id && !current_keys.contains(k) && !v.is_empty() {
            current_keys.push(k.clone());
        }
    }

    let mut selected_key = d.values.get("selected_key").and_then(Json::as_str).unwrap_or("").to_string();
    if (selected_key.is_empty() || !current_keys.contains(&selected_key)) && !current_keys.is_empty() {
        selected_key = current_keys[0].clone();
        d.values.insert("selected_key".into(), json!(selected_key));
    }

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label("Current keys:");
            if current_keys.is_empty() {
                ui.weak("[None]");
            } else {
                for k in &current_keys {
                    let is_sel = selected_key == *k;
                    if ui.selectable_label(is_sel, k).clicked() {
                        selected_key = k.clone();
                        d.values.insert("selected_key".into(), json!(selected_key));
                    }
                }
            }
        });

        ui.add_space(20.0);

        ui.vertical(|ui| {
            ui.label("Press new keyboard shortcut:");
            let r = ui.add(TextEdit::singleline(&mut pressed).hint_text("e.g. Control+Option+E, Ctrl+Shift+F"));
            if r.changed() {
                d.values.insert("pressed".into(), json!(pressed.clone()));
            }

            if r.has_focus() {
                ui.input(|i| {
                    for e in &i.events {
                        if let egui::Event::Key { key, pressed: true, modifiers, .. } = e
                            && let Some(combo) = crate::keymap::canonical_combo(*key, *modifiers, mac)
                        {
                            d.values.insert("pressed".into(), json!(combo));
                        }
                    }
                });
            }

            let p_val = d.values.get("pressed").and_then(Json::as_str).unwrap_or("");
            let norm_p_val = crate::keymap::normalize_for_platform(p_val, mac);
            ui.horizontal(|ui| {
                ui.label("Currently assigned to:");
                if p_val.is_empty() {
                    ui.weak("[unassigned]");
                } else if let Some(assigned_to) = norm_p_val.as_ref().and_then(|norm| app.ui.shortcuts.get(norm)) {
                    if assigned_to.is_empty() {
                        ui.weak("[disabled]");
                    } else {
                        let lbl = gridcraft_engine::find_command(assigned_to).map(|s| s.label).unwrap_or(assigned_to);
                        ui.label(RichText::new(lbl).strong());
                    }
                } else {
                    let mut found = None;
                    for spec in gridcraft_engine::command_specs() {
                        if let Some(sc) = spec.shortcut
                            && (sc.eq_ignore_ascii_case(p_val) || norm_p_val.as_deref().is_some_and(|n| sc.eq_ignore_ascii_case(n)))
                            && !app.ui.shortcuts.contains_key(sc)
                        {
                            found = Some(spec.label);
                            break;
                        }
                    }
                    if let Some(lbl) = found {
                        ui.label(RichText::new(lbl).strong());
                    } else {
                        ui.weak("[unassigned]");
                    }
                }
            });
        });
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let p_val = d.values.get("pressed").and_then(Json::as_str).unwrap_or("").to_string();
        let can_assign = !sel_id.is_empty() && !p_val.is_empty();
        if ui.add_enabled(can_assign, Button::new("Assign")).clicked()
            && let Some(norm) = crate::keymap::normalize_for_platform(&p_val, mac)
        {
            app.ui.shortcuts.insert(norm, sel_id.clone());
            d.values.insert("pressed".into(), json!(""));
        }

        let can_remove = !selected_key.is_empty() && current_keys.contains(&selected_key);
        if ui.add_enabled(can_remove, Button::new("Remove")).clicked() {
            let default_sc = sel_spec.and_then(|s| s.shortcut);
            let is_default = default_sc.is_some_and(|sc| {
                sc.eq_ignore_ascii_case(&selected_key) || crate::keymap::normalize_for_platform(sc, mac).as_deref() == Some(&selected_key)
            });
            if is_default {
                if let Some(norm) = crate::keymap::normalize_for_platform(&selected_key, mac) {
                    app.ui.shortcuts.insert(norm, "".into());
                } else {
                    app.ui.shortcuts.insert(selected_key.clone(), "".into());
                }
            } else {
                app.ui.shortcuts.remove(&selected_key);
                if let Some(norm) = crate::keymap::normalize_for_platform(&selected_key, mac) {
                    app.ui.shortcuts.remove(&norm);
                }
            }
            d.values.remove("selected_key");
        }

        let confirm_reset = d.values.get("confirm_reset").and_then(Json::as_bool).unwrap_or(false);
        if !confirm_reset {
            if ui.add_enabled(!app.ui.shortcuts.is_empty(), Button::new("Reset All…")).clicked() {
                d.values.insert("confirm_reset".into(), json!(true));
            }
        } else {
            ui.label(RichText::new("Reset all?").color(Color32::RED));
            if ui.button("Yes").clicked() {
                app.ui.shortcuts.clear();
                d.values.remove("confirm_reset");
                d.values.remove("selected_key");
            }
            if ui.button("Cancel").clicked() {
                d.values.remove("confirm_reset");
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("OK").clicked() {
                *open = false;
            }
        });
    });
}
