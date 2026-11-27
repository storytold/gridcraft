//! Model-specific thinking controls. References: Zed's public [provider settings](https://zed.dev/docs/ai/use-api-access)
//! and [agent settings](https://zed.dev/docs/ai/agent-settings), [Ollama thinking](https://docs.ollama.com/capabilities/thinking)
//! and [Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create).
use super::settings::Protocol;
use crate::i18n::Language;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum Levels {
    Csv(String),
    List(Vec<Value>),
}

#[derive(Clone, PartialEq, Eq)]
enum Choice {
    Default,
    Off,
    On,
    Level(String),
}

impl Choice {
    fn parse(raw: &str, protocol: Protocol) -> Result<Self, String> {
        let name = raw.trim();
        match name.to_ascii_lowercase().as_str() {
            "default" | "server default" => Ok(Self::Default),
            "none" | "off" | "false" => Ok(Self::Off),
            "on" | "true" if protocol == Protocol::Ollama => Ok(Self::On),
            "med" => Ok(Self::Level("medium".into())),
            "extra-high" | "extra high" => Ok(Self::Level("xhigh".into())),
            s @ ("minimal" | "low" | "medium" | "high" | "xhigh" | "max") => Ok(Self::Level(s.into())),
            // Native Ollama supports model-defined names, not just the OpenAI effort enum.
            _ if protocol == Protocol::Ollama && !name.is_empty() && name.len() <= 64 && !name.chars().any(char::is_control) => {
                Ok(Self::Level(name.to_owned()))
            }
            _ => Err("Unknown thinking level for this provider.".into()),
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Default => "Server default",
            Self::Off => "None (off)",
            Self::On => "On",
            Self::Level(name) => name,
        }
    }
}

/// The selected choice is copied with the agent at the start of a turn. Configuration
/// reloads restore the declared settings; a panel edit does not rewrite the JSON.
#[derive(Clone)]
pub(in super::super) struct Thinking {
    choices: Vec<Choice>,
    selected: Choice,
    listed: bool,
    disabled: bool,
}

impl Default for Thinking {
    fn default() -> Self {
        Self { choices: vec![], selected: Choice::Default, listed: false, disabled: false }
    }
}

impl Thinking {
    /// Model-list endpoints return IDs, not per-model reasoning capabilities. These are manual
    /// protocol choices, not a capability claim; an untouched model uses its server default.
    pub(super) fn manual(protocol: Protocol) -> Self {
        let mut choices = vec![Choice::Off];
        let levels: &[&str] = match protocol {
            Protocol::Completions => &["minimal", "low", "medium", "high", "xhigh", "max"],
            Protocol::Ollama => {
                choices.push(Choice::On);
                &["low", "medium", "high"]
            }
        };
        choices.extend(levels.iter().map(|&name| Choice::Level(name.into())));
        Self { choices, ..Default::default() }
    }

    pub(super) fn configured(protocol: Protocol, supports: Option<bool>, effort: Option<&Levels>, levels: Option<&Levels>) -> Result<Self, String> {
        let mut out = Self { disabled: supports == Some(false), ..Default::default() };
        let scalar = match effort {
            Some(Levels::Csv(s)) if !s.contains(',') => Some(s.as_str()),
            _ => None,
        };
        let inline_list = effort.filter(|_| scalar.is_none());
        if inline_list.is_some() && levels.is_some() {
            return Err("Use one list of thinking levels, not two.".into());
        }
        if let Some(levels) = levels.or(inline_list) {
            out.listed = true;
            match levels {
                Levels::Csv(s) => {
                    for name in s.split(',') {
                        out.add_listed(Choice::parse(name, protocol)?)?;
                    }
                }
                Levels::List(values) => {
                    for v in values {
                        let choice = match v {
                            Value::String(s) => Choice::parse(s, protocol)?,
                            Value::Bool(b) if protocol == Protocol::Ollama => {
                                if *b {
                                    Choice::On
                                } else {
                                    Choice::Off
                                }
                            }
                            _ => return Err("Expected a thinking level name (or an Ollama boolean).".into()),
                        };
                        out.add_listed(choice)?;
                    }
                }
            }
            out.selected = out.choices.last().ok_or("A thinking level list must not be empty.")?.clone();
        } else if !out.disabled {
            out = Self::manual(protocol);
        }
        if let Some(s) = scalar {
            out.choose(Choice::parse(s, protocol)?)?;
        }
        Ok(out)
    }

    fn add_listed(&mut self, choice: Choice) -> Result<(), String> {
        if choice == Choice::Default {
            return Err("Server default is always available; omit it from reasoning_effort_levels.".into());
        }
        if self.choices.len() >= 32 || self.choices.contains(&choice) {
            return Err("Thinking levels must be unique, with at most 32 choices.".into());
        }
        if self.disabled && choice != Choice::Off {
            return Err("A non-thinking model cannot declare reasoning levels.".into());
        }
        self.choices.push(choice);
        Ok(())
    }

    fn choose(&mut self, choice: Choice) -> Result<(), String> {
        if choice != Choice::Default && !self.choices.contains(&choice) {
            if self.listed {
                return Err("The selected thinking level is outside this model's declared reasoning_effort_levels.".into());
            }
            self.add_listed(choice.clone())?;
        }
        self.selected = choice;
        Ok(())
    }

    pub(super) fn preference(&mut self, protocol: Protocol, enabled: Option<bool>, effort: Option<&str>) -> Result<(), String> {
        if enabled == Some(true) && self.disabled {
            return Err("enable_thinking=true conflicts with supports_thinking=false.".into());
        }
        if enabled == Some(false) {
            // A level-only model cannot turn thinking off. Leave that decision with its
            // server, and label the selection accordingly instead of sending a made-up level.
            if self.listed && !self.choices.contains(&Choice::Off) {
                self.selected = Choice::Default;
                return Ok(());
            }
            return self.choose(Choice::Off);
        }
        if let Some(level) = effort {
            let choice = Choice::parse(level, protocol)?;
            if enabled == Some(true) && choice == Choice::Off {
                return Err("enable_thinking=true conflicts with an off effort.".into());
            }
            return self.choose(choice);
        }
        if enabled == Some(true) && matches!(self.selected, Choice::Default | Choice::Off) {
            if !self.listed {
                // A profile requested thinking, not the most expensive manual option.
                return self.choose(if protocol == Protocol::Ollama { Choice::On } else { Choice::Level("medium".into()) });
            } else if let Some(choice) = self.choices.iter().rev().find(|c| **c != Choice::Off) {
                self.selected = choice.clone();
            } else {
                return Err("This model has no enabled thinking level in reasoning_effort_levels.".into());
            }
        }
        Ok(())
    }

    pub(in super::super) fn picker(&mut self, ui: &mut egui::Ui, lang: Language) {
        let why = lang.tr(if self.disabled {
            "Thinking is disabled for this model in wingman.json."
        } else if self.listed {
            "Choose a configured level for the next turn. Server default sends no override."
        } else {
            "Manual protocol choices; your model may accept only some. Server default sends no override."
        });
        let label = ui.label(lang.tr("Thinking mode"));
        ui.add_enabled_ui(!self.disabled, |ui| {
            let value = if self.disabled { "Disabled by configuration" } else { self.selected.label() };
            egui::ComboBox::from_id_salt("wingman_thinking")
                .width(ui.available_width().min(170.0))
                .truncate()
                .selected_text(lang.tr(value))
                .show_ui(ui, |ui| {
                    if !self.listed {
                        ui.weak(why);
                        ui.separator();
                    }
                    for choice in std::iter::once(&Choice::Default).chain(&self.choices) {
                        ui.selectable_value(&mut self.selected, choice.clone(), lang.tr(choice.label()));
                    }
                })
                .response
                .labelled_by(label.id)
                .on_hover_text(why)
                .on_disabled_hover_text(why);
        });
    }

    pub(super) fn wire(&self, protocol: Protocol) -> Value {
        if self.disabled {
            return Value::Null;
        }
        match &self.selected {
            Choice::Default => Value::Null,
            Choice::Off if protocol == Protocol::Ollama => false.into(),
            Choice::Off => "none".into(),
            Choice::On => true.into(),
            Choice::Level(level) => json!(level),
        }
    }

    pub(super) fn supports_reasoning_history(&self) -> bool {
        !self.disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_menus_follow_each_protocol_but_explicit_capabilities_take_precedence() {
        let labels = |t: &Thinking| t.choices.iter().map(|c| c.label().to_owned()).collect::<Vec<_>>();
        let chat = Thinking::manual(Protocol::Completions);
        assert!(labels(&chat).iter().any(|v| v == "xhigh") && labels(&chat).iter().any(|v| v == "max"));
        let ollama = Thinking::manual(Protocol::Ollama);
        assert!(ollama.choices.contains(&Choice::On));
        assert!(!labels(&ollama).iter().any(|v| v == "xhigh" || v == "max"));
        let only = Levels::Csv("low,med".into());
        let mut restricted = Thinking::configured(Protocol::Completions, None, None, Some(&only)).unwrap();
        assert_eq!(labels(&restricted), ["low", "medium"]);
        assert!(restricted.preference(Protocol::Completions, None, Some("max")).is_err());
        let mut disabled = Thinking::configured(Protocol::Completions, Some(false), None, None).unwrap();
        assert!(disabled.preference(Protocol::Completions, Some(true), Some("default")).is_err());
        assert!(disabled.wire(Protocol::Completions).is_null());
    }
}
