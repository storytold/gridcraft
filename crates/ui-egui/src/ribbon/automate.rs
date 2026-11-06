use super::controls::{control_keytip, sep};
use super::{Icon, SheetApp, Ui, big_button, json};
use crate::theme::Tokens;
use crate::widgets::{big_split_button, small_button, small_toggle_button};
use egui::vec2;

pub(super) fn automate(app: &mut SheetApp, ui: &mut Ui) {
    // Macros: the Macros dialog; Record Macro (Stop Recording while recording), Use Relative
    // References; Run Macro, whose main part runs the last macro and whose menu lists them all.
    if big_button(ui, Icon::Script, "Macros", "Macros and scripts in this workbook (Alt+F8)", false).clicked() {
        app.open_dialog("macros", json!({}));
    }
    let recording = app.session.is_recording();
    ui.vertical(|ui| {
        let (label, tip) =
            if recording { ("Stop Recording", "Stop recording and save the macro") } else { ("Record Macro", "Record your actions as a macro") };
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
    let macros = crate::macro_ui::macro_list(app);
    let recent: Vec<String> = app.macros.recent.iter().filter(|r| macros.iter().any(|(n, ..)| n.eq_ignore_ascii_case(r))).cloned().collect();
    let (main, arrow) = big_split_button(ui, Icon::Macro, "Run\nMacro", "Run a macro on the selection");
    control_keytip(app, ui, &arrow, "macro.run");
    if main.clicked() {
        // The last macro run, or the only one; otherwise choose in the Macros dialog.
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
        if !recent.is_empty() {
            ui.label(egui::RichText::new(app.ui.language.tr("Recent")).small().color(t.text_dim));
            for n in &recent {
                if ui.add(egui::Button::new(n.as_str()).frame(false).min_size(vec2(200.0, 22.0))).clicked() {
                    crate::macro_ui::run_macro(app, n);
                }
            }
            ui.separator();
        }
        if macros.is_empty() {
            ui.label(egui::RichText::new("No macros in this workbook").italics().color(t.text_dim));
        }
        for (n, desc, _) in macros.iter().take(30) {
            let r = ui.add(egui::Button::new(n.as_str()).frame(false).min_size(vec2(200.0, 22.0)));
            let r = if desc.is_empty() { r } else { r.on_hover_text(desc) };
            if r.clicked() {
                crate::macro_ui::run_macro(app, n);
            }
        }
        ui.separator();
        if ui.add(egui::Button::new(app.ui.language.tr("Macros…")).frame(false).min_size(vec2(200.0, 22.0))).clicked() {
            app.open_dialog("macros", json!({}));
        }
    });
    sep(ui);
    let __k = big_button(ui, Icon::Script, "Command\nPalette", "Search and run any command", false);
    control_keytip(app, ui, &__k, "commandSearch");
    if __k.clicked() {
        app.open_dialog("commandSearch", json!({}));
    }
    if app.ask_agent.hub.is_some()
        && big_button(ui, Icon::Comment, "Ask\nAgent", "Hand the selected cells and an instruction to the agent you connected", false).clicked()
    {
        crate::ask_agent::open_from_ui(app);
    }
    let __k = big_button(ui, Icon::Script, "Agent\nControl", "How agents drive GridCraft (MCP / control channel)", false);
    control_keytip(app, ui, &__k, "agents");
    if __k.clicked() {
        app.open_dialog("agents", json!({}));
    }
    let __k = big_button(ui, Icon::Script, "Action\nJournal", "Every command run in this session (replayable)", false);
    control_keytip(app, ui, &__k, "journal");
    if __k.clicked() {
        app.open_dialog("journal", json!({}));
    }
    let __k = big_button(ui, Icon::Script, "Check for\nUpdates…", "Look for a newer GridCraft release", false);
    control_keytip(app, ui, &__k, "checkForUpdates");
    if __k.clicked() {
        app.run_or_alert("app.checkForUpdates", json!({}));
    }
    let __k = big_button(ui, Icon::Script, "About\nGridCraft", "Version, contributors and the AI models that helped", false);
    control_keytip(app, ui, &__k, "about");
    if __k.clicked() {
        app.open_dialog("about", json!({}));
    }
}
