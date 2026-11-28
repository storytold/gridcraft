//! Presentation controls do not mutate model settings, pending actions, or provider messages.
use super::{Composer, PanelState, Thread, ToolDetail, history};
use crate::{SheetApp, i18n::Language};
use egui::{Button, Event, Id, Key, Ui};

/// Some local chat templates return a leading tagged block inside the answer instead of
/// a separate field. Interpret it only for display, never inside prose or a code example.
pub(super) fn assistant_parts<'a>(thinking: &'a str, content: &'a str, finished: bool) -> (&'a str, &'a str) {
    let leading = content.trim_start();
    if let Some(block) = leading.strip_prefix("<think>") {
        if let Some((thought, answer)) = block.split_once("</think>") {
            return (if thinking.is_empty() { thought.trim() } else { thinking }, answer.trim_start_matches(['\r', '\n']));
        }
        return (if thinking.is_empty() { block } else { thinking }, "");
    } else if !finished && !leading.is_empty() && "<think>".starts_with(leading) {
        return (thinking, "");
    }
    (thinking, content)
}

impl Thread {
    pub(super) fn assistant_line(&mut self, speaker: &'static str, thinking: &str, content: &str, finished: bool) {
        let (thinking, answer) = assistant_parts(thinking, content, finished);
        if !thinking.is_empty() {
            self.line("Thinking", thinking);
        }
        if !answer.is_empty() {
            self.line(speaker, answer);
        }
    }
}

impl ToolDetail {
    pub(super) fn show(&mut self, ui: &mut Ui, lang: Language, speaker: &str, caption: &str, default_open: bool) {
        let mut open = self.expanded.unwrap_or(default_open);
        let disclosure = if open { "▾" } else { "▸" };
        let header = ui.add(Button::new(format!("{disclosure} {speaker} · {caption}")).frame(false).wrap());
        if header.clicked() {
            open = !open;
            self.expanded = Some(open);
        }
        ui.ctx().accesskit_node_builder(header.id, |node| node.set_expanded(open));
        if open {
            if ui.small_button(lang.tr("Copy")).clicked() {
                ui.ctx().copy_text(self.text.clone());
            }
            ui.monospace(&self.text);
        }
    }
}

impl PanelState {
    fn reset_disclosures(&mut self) {
        for thread in std::iter::once(&mut self.thread).chain(&mut self.archives) {
            for detail in thread.log.iter_mut().filter_map(|line| line.detail.as_mut()) {
                detail.expanded = None;
            }
        }
    }

    pub(super) fn transcript_controls(&mut self, ui: &mut Ui, lang: Language) -> Id {
        ui.horizontal_wrapped(|ui| {
            let tools =
                ui.checkbox(&mut self.expand_tools, lang.tr("Tool details")).on_hover_text(lang.tr("Expand or collapse all tool results (Ctrl+O)"));
            if tools.changed() {
                self.reset_disclosures();
            }
            let thinking = ui
                .checkbox(&mut self.show_thinking, lang.tr("Thinking blocks"))
                .on_hover_text(lang.tr("Show or hide provider thinking in the transcript (Ctrl+T)"));
            for (widget, shortcut) in [(&tools, "Ctrl+O"), (&thinking, "Ctrl+T")] {
                ui.ctx().accesskit_node_builder(widget.id, |node| {
                    node.set_keyboard_shortcut(shortcut);
                    node.set_live(egui::accesskit::Live::Polite);
                });
            }
            tools.id
        })
        .inner
    }

    pub(super) fn transcript_shortcuts(&mut self, app: &SheetApp, ui: &Ui) {
        if !ui.input(|i| i.key_pressed(Key::O) || i.key_pressed(Key::T))
            || app.message.is_some()
            || app.macros.question.is_some()
            || app.dialog.as_ref().is_some_and(|d| d.name != "macroEditor")
            || egui::Popup::is_any_open(ui.ctx())
        {
            return;
        }
        let Some(id) = ui.memory(|m| m.focused()) else { return };
        if id != Composer::id() && (ui.ctx().text_edit_focused() || !history::dock_owns_focus(ui, id)) {
            return;
        }
        ui.input_mut(|i| {
            i.events.retain(|e| {
                let Event::Key { key: key @ (Key::O | Key::T), pressed: true, repeat, modifiers, .. } = e else { return true };
                if !modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.mac_cmd {
                    return true;
                }
                if !repeat {
                    if *key == Key::O {
                        self.expand_tools = !self.expand_tools;
                        self.reset_disclosures();
                    } else {
                        self.show_thinking = !self.show_thinking;
                    }
                }
                false
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::assistant_parts;
    #[test]
    fn only_leading_model_tags_separate_a_thought_from_the_answer() {
        assert_eq!(assistant_parts("", "  <think>work</think>\nResult", true), ("work", "Result"));
        assert_eq!(assistant_parts("", "<think>unfinished", true), ("unfinished", ""));
        assert_eq!(assistant_parts("separate", "<think>same thing</think>result", true), ("separate", "result"));
        let code = "```\n<think>not a model directive</think>\n```";
        assert_eq!(assistant_parts("", code, true), ("", code));
        assert_eq!(assistant_parts("", "<thi", false), ("", ""));
        assert_eq!(assistant_parts("", "<thi", true), ("", "<thi"));
    }
    #[test]
    fn editor_shortcuts_consume_only_the_chat_key_and_leave_other_editors_alone() {
        use super::*;
        #[derive(Default)]
        struct Widgets {
            panel: PanelState,
            chat: String,
            other: String,
            unhandled: Vec<Key>,
        }
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let app = SheetApp::new(session, crate::Services::default());
        let other_id = Id::unique("wingman.fixture.other_text");
        let mut h = egui_kittest::Harness::builder().with_context_setup(crate::framework::install_headless).build_ui_state(
            move |ui, w: &mut Widgets| {
                ui.vertical(|ui| {
                    w.panel.transcript_shortcuts(&app, ui);
                    for key in [Key::O, Key::T] {
                        if ui.input(|i| i.key_pressed(key)) {
                            w.unhandled.push(key);
                        }
                    }
                    ui.add(egui::TextEdit::singleline(&mut w.chat).id(Composer::id()));
                });
                ui.add(egui::TextEdit::singleline(&mut w.other).id(other_id));
            },
            Widgets::default(),
        );
        let key = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::CTRL };
        h.ctx.memory_mut(|m| m.request_focus(Composer::id()));
        h.event(key(Key::O));
        h.event(key(Key::T));
        h.run_steps(2);
        assert!(h.state().panel.expand_tools && h.state().panel.show_thinking);
        assert!(h.state().unhandled.is_empty());
        h.ctx.memory_mut(|m| m.request_focus(other_id));
        h.event(key(Key::T));
        h.run_steps(2);
        assert!(h.state().panel.show_thinking);
        assert_eq!(h.state().unhandled, [Key::T]);
    }
}
