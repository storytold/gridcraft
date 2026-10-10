//! Sequential ribbon shortcuts. The host paints hints from `prefix` and runs returned actions.

use egui::{Context, Event, Key, Modifiers, os::OperatingSystem};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Home,
    Command(&'static str),
    Dialog(&'static str),
}

#[derive(Default)]
pub struct KeyTips {
    prefix: Option<String>,
    alt_armed: bool,
    // Keep consuming a held mnemonic after its action closes keytips.
    held: Vec<Key>,
}

impl KeyTips {
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub fn clear(&mut self) {
        self.prefix = None;
        self.alt_armed = false;
        self.held.clear();
    }

    fn toggle(&mut self) {
        self.prefix = if self.prefix.is_some() { None } else { Some(String::new()) };
        self.alt_armed = false;
    }

    /// Remove only handled events before any text editors or the grid see them.
    pub fn process(&mut self, ctx: &Context, enabled: bool) -> Vec<Action> {
        if !enabled || !ctx.input(|i| i.focused) {
            self.clear();
            return Vec::new();
        }
        let mac = ctx.os() == OperatingSystem::Mac;
        let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
        let pairs = text_pairs(&events);
        let mut consumed = std::collections::HashSet::new();
        let mut actions = Vec::new();
        for (index, event) in events.iter().enumerate() {
            let mut handled = false;
            match event {
                Event::PointerButton { pressed: true, .. } | Event::MouseWheel { .. } | Event::WindowFocused(false) | Event::Ime(_) => self.clear(),
                Event::Copy | Event::Cut | Event::Paste(_) => self.clear(),
                Event::ModifiersChanged(m) => {
                    if command_modifier(*m) || (mac && m.alt) {
                        self.clear();
                    } else if m.shift {
                        self.alt_armed = false;
                    }
                }
                Event::Key { key: Key::AltLeft, pressed, repeat, modifiers, .. } if !mac && !command_modifier(*modifiers) && !modifiers.shift => {
                    if *pressed {
                        if !repeat {
                            self.alt_armed = true;
                        }
                        handled = true;
                    } else if self.alt_armed {
                        self.toggle();
                        handled = true;
                    }
                }
                Event::Key { key, pressed, repeat, modifiers, .. } => {
                    self.alt_armed = false;
                    if command_modifier(*modifiers)
                        || modifiers.alt
                        || matches!(key, Key::AltLeft | Key::AltRight | Key::ControlLeft | Key::ControlRight | Key::SuperLeft | Key::SuperRight)
                    {
                        self.clear();
                    } else if !pressed {
                        handled = self.held.contains(key);
                        self.held.retain(|held| held != key);
                    } else if *repeat && (self.prefix.is_some() || self.held.contains(key)) {
                        handled = true;
                    } else if *key == Key::F10 && modifiers.is_none() {
                        self.toggle();
                        handled = true;
                    } else if self.prefix.is_some() && !matches!(key, Key::ShiftLeft | Key::ShiftRight) {
                        handled = true;
                        if *key == Key::Escape {
                            if let Some(prefix) = self.prefix.as_mut()
                                && prefix.pop().is_none()
                            {
                                self.prefix = None;
                            }
                        } else if key.name().len() == 1 {
                            let next = format!("{}{key_name}", self.prefix.as_deref().unwrap_or_default(), key_name = key.name());
                            match next.as_str() {
                                "H" => {
                                    self.prefix = Some(next);
                                    actions.push(Action::Home);
                                }
                                "HA" | "HV" | "HO" | "E" | "O" | "OC" => self.prefix = Some(next),
                                "HAR" => self.finish(Action::Command("home.alignRight"), &mut actions),
                                "HAL" => self.finish(Action::Command("home.alignLeft"), &mut actions),
                                "HAC" => self.finish(Action::Command("home.alignCenter"), &mut actions),
                                "HVS" | "ES" => self.finish(Action::Dialog("pasteSpecial"), &mut actions),
                                "HOI" | "OCA" => self.finish(Action::Command("home.autofitColumnWidth"), &mut actions),
                                _ => {}
                            }
                        }
                    }
                    if handled && *pressed && !self.held.contains(key) {
                        self.held.push(*key);
                    }
                }
                Event::Text(_) => {
                    self.alt_armed = false;
                    // Paired text is decided with its key, which may occur later on web.
                    handled = self.prefix.is_some() && !pairs.iter().any(|(_, text)| *text == index);
                }
                _ => {}
            }
            if handled {
                consumed.insert(index);
                if let Some((_, text)) = pairs.iter().find(|(key, _)| *key == index) {
                    consumed.insert(*text);
                }
            }
        }
        ctx.input_mut(|i| {
            i.events = events.into_iter().enumerate().filter_map(|(index, event)| (!consumed.contains(&index)).then_some(event)).collect()
        });
        actions
    }

    fn finish(&mut self, action: Action, actions: &mut Vec<Action>) {
        self.prefix = None;
        actions.push(action);
    }
}

fn command_modifier(m: Modifiers) -> bool {
    m.ctrl || m.command || m.mac_cmd
}

/// Native integrations emit Key then Text; browsers can emit Text then Key.
fn text_pairs(events: &[Event]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (text_index, event) in events.iter().enumerate() {
        let Event::Text(_) = event else { continue };
        let before = events.iter().enumerate().take(text_index).rev().find(|(_, e)| !matches!(e, Event::ModifiersChanged(_)));
        let after = events.iter().enumerate().skip(text_index + 1).find(|(_, e)| !matches!(e, Event::ModifiersChanged(_)));
        for (key_index, event) in before.into_iter().chain(after) {
            if let Event::Key { key, pressed: true, modifiers, .. } = event
                && (key.name().len() == 1 || modifiers.alt || command_modifier(*modifiers))
                && !pairs.iter().any(|(paired, _)| *paired == key_index)
            {
                pairs.push((key_index, text_index));
                break;
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
    }

    #[test]
    fn active_mode_preserves_modified_text_before_key() {
        for (os, letter, text, modifiers) in [
            (OperatingSystem::Windows, Key::E, "€", Modifiers::CTRL | Modifiers::ALT),
            (OperatingSystem::Nix, Key::E, "€", Modifiers::ALT),
            (OperatingSystem::Mac, Key::O, "œ", Modifiers::ALT),
            (OperatingSystem::Mac, Key::C, "c", Modifiers::COMMAND),
        ] {
            let ctx = Context::default();
            ctx.set_os(os);
            let mut tips = KeyTips { prefix: Some("H".into()), ..Default::default() };
            let events = vec![Event::Text(text.into()), key(letter, true, modifiers)];
            let mut remaining = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput { events: events.clone(), ..Default::default() }, |ui| {
                assert!(tips.process(ui.ctx(), true).is_empty());
                remaining = ui.input(|i| i.events.clone());
            });
            output.textures_delta.clear();
            assert_eq!(tips.prefix(), None);
            assert_eq!(remaining, events);
        }
    }

    #[test]
    fn shift_between_alt_press_and_release_does_not_activate() {
        let ctx = Context::default();
        ctx.set_os(OperatingSystem::Windows);
        let mut tips = KeyTips::default();
        let events =
            vec![key(Key::AltLeft, true, Modifiers::ALT), key(Key::ShiftLeft, true, Modifiers::NONE), key(Key::AltLeft, false, Modifiers::NONE)];
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            assert!(tips.process(ui.ctx(), true).is_empty());
        });
        output.textures_delta.clear();
        assert_eq!(tips.prefix(), None);
    }
}
