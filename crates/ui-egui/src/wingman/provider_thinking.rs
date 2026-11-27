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
        match raw.trim() {
            "none" | "off" | "false" => Ok(Self::Off),
            "on" | "true" if protocol == Protocol::Ollama => Ok(Self::On),
            "minimal" | "low" | "medium" | "high" | "xhigh" | "max" => Ok(Self::Level(raw.trim().to_owned())),
            // Native Ollama supports model-defined names, not just the OpenAI effort enum.
            s if protocol == Protocol::Ollama && s != "default" && !s.is_empty() && s.len() <= 64 && !s.chars().any(char::is_control) => {
                Ok(Self::Level(s.to_owned()))
            }
            _ => Err("Unknown thinking level for this provider.".into()),
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Default => "Server default",
            Self::Off => "Off",
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
        }
        if let Some(s) = scalar {
            out.choose(Choice::parse(s, protocol)?)?;
        }
        if out.choices.is_empty() && supports == Some(true) && protocol == Protocol::Ollama {
            out.choices.extend([Choice::Off, Choice::On]);
        }
        Ok(out)
    }

    fn add_listed(&mut self, choice: Choice) -> Result<(), String> {
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
        if !self.choices.contains(&choice) {
            if self.listed {
                return Err("The selected thinking level is outside this model's declared reasoning_effort_levels.".into());
            }
            self.add_listed(choice.clone())?;
        }
        self.selected = choice;
        Ok(())
    }

    pub(super) fn preference(&mut self, protocol: Protocol, enabled: Option<bool>, effort: Option<&str>) -> Result<(), String> {
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
            if let Some(choice) = self.choices.iter().rev().find(|c| **c != Choice::Off) {
                self.selected = choice.clone();
            } else if protocol == Protocol::Ollama {
                self.choose(Choice::On)?;
            } else {
                return Err("Specify an effort supported by this Chat Completions model to enable thinking.".into());
            }
        }
        Ok(())
    }

    pub(in super::super) fn picker(&mut self, ui: &mut egui::Ui, lang: Language) {
        if self.choices.is_empty() || self.disabled {
            return;
        }
        egui::ComboBox::from_label(lang.tr("Thinking")).selected_text(lang.tr(self.selected.label())).show_ui(ui, |ui| {
            for choice in std::iter::once(&Choice::Default).chain(&self.choices) {
                ui.selectable_value(&mut self.selected, choice.clone(), lang.tr(choice.label()));
            }
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
