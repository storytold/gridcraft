//! Keyboard shortcuts of the grid in the active language.
//!
//! The chords come from the language's `keymap.toml` ([`gridcraft_l10n::Localizer::shortcut_for`]):
//! a chord is modifiers joined by `+` and a final key, where `Cmd` is the platform command
//! modifier (Ctrl on Windows and Linux, ⌘ on macOS). [`Keymap`] indexes the chords of the commands
//! the grid handles itself and [`dispatch`] runs the command of a key press.

use std::collections::HashMap;

use egui::{Key, Modifiers};
use serde_json::json;

use crate::SheetApp;
use crate::l10n::{Localizer, Platform};

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    /// Run the engine command with no parameters.
    Command(&'static str),
    /// Open a UI dialog (some commands open them through the engine, these do it directly).
    Dialog(&'static str),
    /// Open Find & Replace on its Replace tab.
    Replace,
}

/// Commands the grid runs from a chord, with the action they trigger. Everything else with a
/// keymap entry is handled elsewhere (editing keys) or is not bound to a key in the grid.
const HANDLED: &[(&str, Action)] = &[
    ("home.bold", Action::Command("home.bold")),
    ("home.italic", Action::Command("home.italic")),
    ("home.underline", Action::Command("home.underline")),
    ("edit.undo", Action::Command("edit.undo")),
    ("edit.redo", Action::Command("edit.redo")),
    ("edit.selectAll", Action::Command("edit.selectAll")),
    ("edit.fillDown", Action::Command("edit.fillDown")),
    ("edit.fillRight", Action::Command("edit.fillRight")),
    ("edit.flashFill", Action::Command("edit.flashFill")),
    ("file.saveAs", Action::Command("file.saveAs")),
    ("file.save", Action::Command("file.save")),
    ("file.open", Action::Command("file.open")),
    ("file.new", Action::Command("file.new")),
    ("file.close", Action::Command("file.close")),
    ("insert.link", Action::Dialog("insertLink")),
    ("insert.table", Action::Command("insert.table")),
    ("data.filter", Action::Command("data.filter")),
    ("edit.find", Action::Dialog("find")),
    ("edit.replace", Action::Replace),
    ("edit.goTo", Action::Dialog("goTo")),
    ("formulas.autoSum", Action::Command("formulas.autoSum")),
    ("formulas.showFormulas", Action::Command("formulas.showFormulas")),
    ("home.formatCells", Action::Dialog("formatCells")),
    ("home.hideRows", Action::Command("home.hideRows")),
    ("home.hideColumns", Action::Command("home.hideColumns")),
    ("home.unhideRows", Action::Command("home.unhideRows")),
    ("home.unhideColumns", Action::Command("home.unhideColumns")),
    ("home.deleteRows", Action::Dialog("deleteCells")),
    ("home.insertRows", Action::Dialog("insertCells")),
];

/// Chord that redoes on every platform besides the language's own `edit.redo` chord.
const REDO_ALIAS: &str = "Cmd+Shift+Z";
/// Chord that inserts today's date (no command: the text depends on the region).
const TODAY_CHORD: &str = "Cmd+;";

/// A normalized chord: `(ctrl, cmd, alt, shift, key)`. On PCs Cmd and Ctrl are the same key, so
/// both are stored as `ctrl`.
type Chord = (bool, bool, bool, bool, String);

fn parse_chord(chord: &str, platform: Platform) -> Option<Chord> {
    let (mut ctrl, mut cmd, mut alt, mut shift) = (false, false, false, false);
    let mut parts: Vec<&str> = chord.split('+').collect();
    // A trailing `+` key (`Ctrl++`) splits into an empty last part.
    let key = if chord.ends_with("++") || chord == "+" {
        parts.pop();
        parts.pop();
        "+".to_string()
    } else {
        parts.pop()?.to_string()
    };
    for m in parts {
        match m {
            "Ctrl" => ctrl = true,
            "Cmd" => {
                if platform == Platform::Pc {
                    ctrl = true;
                } else {
                    cmd = true;
                }
            }
            "Alt" => alt = true,
            "Shift" => shift = true,
            _ => return None,
        }
    }
    let key = if key == "+" { "=".to_string() } else { key.to_ascii_uppercase() };
    Some((ctrl, cmd, alt, shift, key))
}

fn key_name(key: Key) -> String {
    match key {
        Key::Plus | Key::Equals => "=".to_string(),
        Key::Minus => "-".to_string(),
        Key::Semicolon => ";".to_string(),
        Key::Backtick => "`".to_string(),
        other => other.name().to_ascii_uppercase(),
    }
}

fn event_chord(key: Key, m: Modifiers, platform: Platform) -> Chord {
    let (ctrl, cmd) = if platform == Platform::Pc { (m.ctrl || m.command, false) } else { (m.ctrl, m.mac_cmd) };
    (ctrl, cmd, m.alt, m.shift, key_name(key))
}

/// The chords of the handled commands in one language, for one keyboard family.
#[derive(Clone, Debug)]
pub struct Keymap {
    platform: Platform,
    actions: HashMap<Chord, Action>,
}

impl Keymap {
    /// The chords of `l` as typed on `platform` (the running system, see [`crate::l10n::platform`]).
    pub fn new(l: &Localizer, platform: Platform) -> Keymap {
        let mut actions: HashMap<Chord, Action> = HashMap::new();
        for (id, action) in HANDLED {
            let Some(chord) = l.shortcut_for(id, platform) else { continue };
            let Some(parsed) = parse_chord(chord, platform) else { continue };
            // On macOS the grid has always taken the ⌘ form of the chords the keymap lists with a
            // literal Ctrl (go to, replace), because Ctrl+key belongs to the system there.
            if platform == Platform::Mac && parsed.0 && !parsed.1 {
                actions.entry((false, true, parsed.2, parsed.3, parsed.4.clone())).or_insert(*action);
            }
            actions.insert(parsed, *action);
        }
        if let Some(c) = parse_chord(REDO_ALIAS, platform) {
            actions.entry(c).or_insert(Action::Command("edit.redo"));
        }
        Keymap { platform, actions }
    }

    /// The keyboard family the chords were built for.
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Whether `key` with `m` is a bound chord.
    fn action(&self, key: Key, m: Modifiers) -> Option<Action> {
        self.actions.get(&event_chord(key, m, self.platform)).copied()
    }
}

/// Runs the shortcut for `key` pressed with `m` when the grid has focus. Returns whether the chord
/// was bound.
pub fn dispatch(app: &mut SheetApp, key: Key, m: Modifiers) -> bool {
    let platform = app.keymap.platform();
    if parse_chord(TODAY_CHORD, platform).is_some_and(|c| c == event_chord(key, m, platform)) {
        insert_today(app);
        return true;
    }
    let Some(action) = app.keymap.action(key, m) else { return false };
    match action {
        Action::Command(id) => {
            app.run_or_alert(id, json!({}));
            app.grid.ensure_visible = true;
        }
        Action::Dialog(name) => app.open_dialog(name, json!({})),
        Action::Replace => app.open_dialog("find", json!({"replace": true})),
    }
    true
}

/// Starts editing the active cell with today's date in the region's short date format.
fn insert_today(app: &mut SheetApp) {
    let today = gridcraft_engine::calc::now_serial().floor();
    let text = app
        .session
        .active()
        .map(|d| {
            let code = d.wb.locale.regional.short_date;
            gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(today), code, &d.wb).text
        })
        .unwrap_or_default();
    app.begin_edit(Some(text), false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> Modifiers {
        Modifiers { ctrl: true, command: true, ..Modifiers::NONE }
    }

    /// What egui reports on macOS for the Command key.
    fn mac_cmd() -> Modifiers {
        Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE }
    }

    #[test]
    fn chords_parse_and_match_events() {
        let m = Modifiers { shift: true, ..ctrl() };
        assert_eq!(parse_chord("Cmd+Shift+S", Platform::Pc), Some(event_chord(Key::S, m, Platform::Pc)));
        let m = Modifiers { shift: true, ..mac_cmd() };
        assert_eq!(parse_chord("Cmd+Shift+S", Platform::Mac), Some(event_chord(Key::S, m, Platform::Mac)));
        assert_eq!(parse_chord("Ctrl+Shift+=", Platform::Pc).map(|c| c.4), Some("=".into()));
        assert_eq!(parse_chord("Ctrl+`", Platform::Pc).map(|c| c.4), Some("`".into()));
        assert!(parse_chord("Hyper+X", Platform::Pc).is_none());
    }

    #[test]
    fn punctuation_keys_match_their_chords() {
        for (chord, key) in [("Cmd+;", Key::Semicolon), ("Cmd+-", Key::Minus), ("Ctrl+`", Key::Backtick), ("Cmd+1", Key::Num1)] {
            assert_eq!(parse_chord(chord, Platform::Pc), Some(event_chord(key, ctrl(), Platform::Pc)), "{chord}");
        }
        let shifted = Modifiers { shift: true, ..ctrl() };
        assert_eq!(parse_chord("Cmd+Shift+=", Platform::Pc), Some(event_chord(Key::Plus, shifted, Platform::Pc)));
    }

    #[test]
    fn language_keymaps_differ() {
        let en = Keymap::new(&Localizer::new("en-US"), Platform::Pc);
        let pt = Keymap::new(&Localizer::new("pt-BR"), Platform::Pc);
        assert_eq!(en.action(Key::B, ctrl()), Some(Action::Command("home.bold")));
        assert_eq!(pt.action(Key::N, ctrl()), Some(Action::Command("home.bold")));
        assert_eq!(pt.action(Key::B, ctrl()), Some(Action::Command("file.save")));
    }

    /// The platform decides which chord of the keymap is bound: a Mac browser gets the ⌘ chords
    /// (and the Mac list's own letters), not the PC ones.
    #[test]
    fn the_platform_selects_the_chord_set() {
        let pt = Localizer::new("pt-BR");
        let pc = Keymap::new(&pt, Platform::Pc);
        let mac = Keymap::new(&pt, Platform::Mac);
        assert_eq!(pc.platform(), Platform::Pc);
        assert_eq!(mac.platform(), Platform::Mac);
        // pt-BR Windows: Ctrl+B saves. Mac keeps the Command-key letters: ⌘B is bold, ⌘S saves.
        assert_eq!(pc.action(Key::B, ctrl()), Some(Action::Command("file.save")));
        assert_eq!(mac.action(Key::B, mac_cmd()), Some(Action::Command("home.bold")));
        assert_eq!(mac.action(Key::S, mac_cmd()), Some(Action::Command("file.save")));
        // Ctrl is not the command key on a Mac.
        assert_eq!(mac.action(Key::B, Modifiers { ctrl: true, ..Modifiers::NONE }), None);
    }

    #[test]
    fn egui_operating_systems_map_to_keyboard_families() {
        use egui::os::OperatingSystem as Os;
        for (os, platform) in [
            (Os::Mac, Platform::Mac),
            (Os::IOS, Platform::Mac),
            (Os::Windows, Platform::Pc),
            (Os::Nix, Platform::Pc),
            (Os::Android, Platform::Pc),
            (Os::Unknown, Platform::Pc),
        ] {
            assert_eq!(crate::l10n::platform_of(os), platform, "{os:?}");
        }
    }
}
