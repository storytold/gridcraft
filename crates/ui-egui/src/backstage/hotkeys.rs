//! File › Options › Hotkeys: search any application command, record a chord, assign it or clear it.

use egui::{RichText, Ui, vec2};

use super::{Language, SheetApp};
use crate::hotkeys;
use crate::theme::Tokens;

#[derive(Clone, Default)]
struct Editor {
    query: String,
    selected: String,
    recording: bool,
    recorded: String,
    notice: String,
    /// Keys already held when Record was clicked. A key still in this list is not the new chord.
    held_at_arm: Vec<egui::Key>,
}

fn editor_id() -> egui::Id {
    egui::Id::unique("gridcraft.hotkey_editor")
}

fn page_id() -> egui::Id {
    egui::Id::unique("gridcraft.hotkey_page")
}

/// The General / Hotkeys switch. Returns whether the Hotkeys page is showing.
pub(super) fn open(ui: &mut Ui, lang: Language) -> bool {
    let mut hotkeys_page = ui.data(|data| data.get_temp::<bool>(page_id()).unwrap_or(false));
    ui.horizontal(|ui| {
        if ui.selectable_label(!hotkeys_page, lang.tr("General")).clicked() {
            hotkeys_page = false;
        }
        if ui.selectable_label(hotkeys_page, lang.tr("Hotkeys")).clicked() {
            hotkeys_page = true;
        }
    });
    ui.data_mut(|data| data.insert_temp(page_id(), hotkeys_page));
    ui.add_space(12.0);
    hotkeys_page
}

/// The Hotkeys page: every application command, a recorded chord, Assign and Clear.
pub(super) fn page(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    let mut editor = ui.data(|data| data.get_temp::<Editor>(editor_id()).unwrap_or_default());
    if editor.recording {
        capture(ui, &mut editor, mac);
    }
    ui.label(lang.tr("Search for a command, record a shortcut, then assign it. Clear restores the built-in shortcut."));
    ui.add_space(8.0);
    let search = ui.add(egui::TextEdit::singleline(&mut editor.query).desired_width(ui.available_width()).hint_text(lang.tr("Search commands")));
    search.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, lang.tr("Search commands")));
    if editor.recording {
        ui.memory_mut(|memory| memory.surrender_focus(search.id));
    }

    let query = editor.query.to_ascii_lowercase();
    let commands = matching_commands(app, &query);
    if editor.selected.is_empty()
        && let Some((id, ..)) = commands.first()
    {
        editor.selected = id.clone();
    }
    if !commands.iter().any(|(id, ..)| id == &editor.selected)
        && let Some((id, ..)) = commands.first()
    {
        editor.selected = id.clone();
    }

    ui.add_space(8.0);
    command_list(ui, lang, &commands, &mut editor.selected);
    ui.add_space(10.0);
    let selected = editor.selected.clone();
    let current = hotkeys::current_shortcut(&app.ui.command_hotkeys, &selected, mac).unwrap_or_else(|| lang.tr("None").to_string());
    ui.label(format!("{} {current}", lang.tr("Current shortcut:")));
    let pending = if editor.recording && editor.recorded.is_empty() {
        lang.tr("Press a shortcut…").to_string()
    } else {
        hotkeys::display_chord(&editor.recorded, mac).unwrap_or_else(|| lang.tr("None").to_string())
    };
    ui.label(format!("{} {pending}", if editor.recording { lang.tr("Recording:") } else { lang.tr("Recorded:") }));
    if !editor.notice.is_empty() {
        ui.label(RichText::new(&editor.notice).color(Tokens::get(ui.ctx()).danger));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let record = ui.button(lang.tr(if editor.recording { "Stop" } else { "Record" }));
        if record.clicked() {
            editor.recording = !editor.recording;
            if editor.recording {
                editor.recorded.clear();
                editor.notice.clear();
                editor.held_at_arm = ui.input(|input| input.keys_down.iter().copied().collect());
            } else {
                editor.held_at_arm.clear();
            }
        }
        if ui.add_enabled(!editor.recorded.is_empty() && !selected.is_empty(), egui::Button::new(lang.tr("Assign"))).clicked() {
            match hotkeys::assign(&mut app.ui.command_hotkeys, &selected, &editor.recorded, mac) {
                Ok(()) => {
                    editor.notice.clear();
                    editor.recorded.clear();
                    editor.recording = false;
                }
                Err(message) => editor.notice = message,
            }
        }
        let has_override = app.ui.command_hotkeys.contains_key(&selected);
        if ui.add_enabled(has_override, egui::Button::new(lang.tr("Clear"))).clicked() {
            match hotkeys::clear(&mut app.ui.command_hotkeys, &selected, mac) {
                Ok(()) => editor.notice.clear(),
                Err(message) => editor.notice = message,
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(editor_id(), editor));
}

fn capture(ui: &mut Ui, editor: &mut Editor, mac: bool) {
    let pressed = ui.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key { key, physical_key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *physical_key, *modifiers)),
            _ => None,
        })
    });
    let outcome = if let Some((key, physical, modifiers)) = pressed {
        ui.input_mut(|input| {
            input.consume_key(modifiers, key);
        });
        hotkeys::record(key, physical, modifiers, mac)
    } else {
        // The press can land on a pass that never reaches this page. The key stays down.
        let (down, modifiers) = ui.input(|input| (input.keys_down.iter().copied().collect::<Vec<_>>(), input.modifiers));
        editor.held_at_arm.retain(|key| down.contains(key));
        hotkeys::record_held(&down, &editor.held_at_arm, modifiers, mac)
    };
    match outcome {
        Ok(chord) => {
            editor.recorded = chord;
            editor.notice.clear();
            editor.recording = false;
            editor.held_at_arm.clear();
        }
        Err(message) if !message.is_empty() => {
            editor.notice = message.to_string();
            editor.recording = false;
            editor.held_at_arm.clear();
        }
        Err(_) => {}
    }
}

fn matching_commands(app: &SheetApp, query: &str) -> Vec<(String, String, String)> {
    app.session
        .commands()
        .into_iter()
        .filter(|command| {
            query.is_empty()
                || command.label.to_ascii_lowercase().contains(query)
                || command.id.to_ascii_lowercase().contains(query)
                || command.menu.join(" ").to_ascii_lowercase().contains(query)
        })
        .map(|command| (command.id.to_string(), command.label.to_string(), command.menu.join(" › ")))
        .collect()
}

fn command_list(ui: &mut Ui, lang: Language, commands: &[(String, String, String)], selected: &mut String) {
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, false]).show(ui, |ui| {
        if commands.is_empty() {
            ui.label(lang.tr("No commands match."));
            return;
        }
        let width = ui.available_width();
        for (id, label, menu) in commands {
            let text = if menu.is_empty() { label.clone() } else { format!("{label}    {menu}") };
            let response = ui.add(egui::Button::selectable(selected == id, text).truncate().min_size(vec2(width, 22.0)));
            if response.clicked() {
                *selected = id.clone();
            }
        }
    });
}
