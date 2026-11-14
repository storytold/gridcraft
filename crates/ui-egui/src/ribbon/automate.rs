use super::controls::{control_keytip, sep};
use super::{Icon, SheetApp, Ui, big_button, json};
use crate::rtl::UiRtlExt;
use crate::theme::Tokens;
use crate::widgets::{big_split_button, small_button, small_toggle_button};
use egui::vec2;

pub(super) fn automate(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let is_rtl = lang.is_rtl();

    // =========================================================================
    // הצמדה דינמית לקצה הימני ב-RTL
    // =========================================================================
    let ribbon_id = egui::Id::unique("automate_total_content_width");
    let last_measured_width = ui.data(|d| d.get_temp::<f32>(ribbon_id)).unwrap_or(750.0);
    let container_width = ui.max_rect().width();
    let margin_right = 5.0;

    if is_rtl && container_width > (last_measured_width + margin_right) {
        let left_padding = container_width - last_measured_width - margin_right;
        ui.add_space(left_padding);
    }

    let start_x = ui.cursor().min.x;

    let draw_macros = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_macros_dialog_btn = |app: &mut SheetApp, ui: &mut Ui| {
            if big_button(ui, Icon::Script, "Macros", "Macros and scripts in this workbook (Alt+F8)", false).clicked() {
                app.open_dialog("macros", json!({}));
            }
        };

        let draw_recording_group = |app: &mut SheetApp, ui: &mut Ui| {
            let recording = app.session.is_recording();
            ui.vertical(|ui| {
                let (label, tip) = if recording {
                    ("Stop Recording", "Stop recording and save the macro")
                } else {
                    ("Record Macro", "Record your actions as a macro")
                };
                let r = small_button(ui, Icon::Record, label, tip, false);
                control_keytip(app, ui, &r, "macro.record");
                if r.clicked() {
                    crate::macro_ui::toggle_recording(app);
                }
                let tip = "Record steps relative to the cells selected when recording starts, so the macro works on other data";
                let rel = app.macros.relative;
                let r = ui.add_enabled_ui(!recording, |ui| small_toggle_button(ui, Icon::Relative, "Use Relative References", tip, rel)).inner;
                if r.clicked() && !recording {
                    app.macros.relative = !rel;
                }
                if small_button(ui, Icon::Macro, "New Macro", "Write a macro step by step", false).clicked() {
                    app.open_dialog("macroEditor", json!({}));
                }
            });
        };

        let draw_run_macro = |app: &mut SheetApp, ui: &mut Ui| {
            let macros = crate::macro_ui::macro_list(app);
            let recent: Vec<String> = app.macros.recent.iter().filter(|r| macros.iter().any(|(n, ..)| n.eq_ignore_ascii_case(r))).cloned().collect();
            let (main, arrow) = big_split_button(ui, Icon::Macro, "Run\nMacro", "Run a macro on the selection");
            control_keytip(app, ui, &arrow, "macro.run");
            if main.clicked() {
                match recent.first().or(macros.first().map(|m| &m.0)) {
                    Some(name) if macros.len() == 1 || !recent.is_empty() => {
                        let name = name.clone();
                        crate::macro_ui::run_macro(app, &name);
                    }
                    _ => app.open_dialog("macros", json!({})),
                }
            }
            egui::Popup::menu(&arrow).show(|ui| {
                let t = Tokens::get(ui.ctx());
                ui.with_layout(ui.menu_layout(is_rtl), |ui| {
                    if !recent.is_empty() {
                        ui.label(egui::RichText::new(lang.tr("Recent")).small().color(t.text_dim));
                        for n in &recent {
                            if ui.add(egui::Button::new(n.as_str()).frame(false).min_size(vec2(200.0, 22.0))).clicked() {
                                crate::macro_ui::run_macro(app, n);
                            }
                        }
                        ui.separator();
                    }
                    if macros.is_empty() {
                        ui.label(egui::RichText::new(lang.tr("No macros in this workbook")).italics().color(t.text_dim));
                    }
                    for (n, desc, _) in macros.iter().take(30) {
                        let r = ui.add(egui::Button::new(n.as_str()).frame(false).min_size(vec2(200.0, 22.0)));
                        let r = if desc.is_empty() { r } else { r.on_hover_text(desc) };
                        if r.clicked() {
                            crate::macro_ui::run_macro(app, n);
                        }
                    }
                    ui.separator();
                    if ui.add(egui::Button::new(lang.tr("Macros…")).frame(false).min_size(vec2(200.0, 22.0))).clicked() {
                        app.open_dialog("macros", json!({}));
                    }
                });
            });
        };

        if is_rtl {
            draw_run_macro(app, ui);
            draw_recording_group(app, ui);
            draw_macros_dialog_btn(app, ui);
        } else {
            draw_macros_dialog_btn(app, ui);
            draw_recording_group(app, ui);
            draw_run_macro(app, ui);
        }
    };

    let draw_tools_and_about = |app: &mut SheetApp, ui: &mut Ui| {
        let tools = [
            ("Command\nPalette", "Search and run any command", "commandSearch", true),
            ("Agent\nControl", "How agents drive GridCraft (MCP / control channel)", "agents", true),
            ("Action\nJournal", "Every command run in this session (replayable)", "journal", true),
            ("Check for\nUpdates…", "Look for a newer GridCraft release", "checkForUpdates", false),
            ("About\nGridCraft", "Version, contributors and the AI models that helped", "about", true),
        ];

        let render_tool = |(label, tip, cmd, is_dialog): (&str, &str, &str, bool), app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Script, label, tip, false);
            control_keytip(app, ui, &__k, cmd);
            if __k.clicked() {
                if is_dialog {
                    app.open_dialog(cmd, json!({}));
                } else if cmd == "checkForUpdates" {
                    app.run_or_alert("app.checkForUpdates", json!({}));
                }
            }
        };

        if is_rtl {
            for it in tools.into_iter().rev() {
                render_tool(it, app, ui);
            }
        } else {
            for it in tools {
                render_tool(it, app, ui);
            }
        }
    };

    if is_rtl {
        draw_tools_and_about(app, ui);
        sep(ui);
        draw_macros(app, ui);
    } else {
        draw_macros(app, ui);
        sep(ui);
        draw_tools_and_about(app, ui);
    }

    if is_rtl {
        let total_width = ui.cursor().min.x - start_x;
        ui.data_mut(|d| d.insert_temp(ribbon_id, total_width));
    }
}