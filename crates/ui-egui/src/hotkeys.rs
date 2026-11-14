//! Custom shortcuts for application commands.
//!
//! A binding is an application preference ([`crate::UiState::command_hotkeys`]), not part of the
//! workbook. It is stored in the same portable notation as the engine (`Cmd` is Ctrl on Windows
//! and Linux, and Command on macOS). Dispatch tries these bindings before the hardcoded keymap
//! and the ribbon shortcuts. Clearing a binding removes only the override, so the built-in
//! shortcut comes back when the command has one.

use std::collections::{BTreeMap, HashMap};

use egui::{Key, Modifiers};
use serde::Deserialize;
use serde_json::Value as Json;

use crate::SheetApp;

/// Ribbon tips whose English name is not the command's label.
const TIP_ALIASES: &[(&str, &str)] = &[("insert.link", "Insert Link")];

/// Menu and dialog actions that run a catalog command, so a binding can show on that item.
const ACTION_COMMANDS: &[(&str, &str)] = &[
    ("ui:pasteSpecial", "edit.pasteSpecial"),
    ("dialog:pasteSpecial", "edit.pasteSpecial"),
    ("ui:formatCells", "home.formatCells"),
    ("dialog:formatCells", "home.formatCells"),
    ("ui:defineName", "formulas.defineName"),
    ("dialog:defineName", "formulas.defineName"),
    ("ui:insertLink", "insert.link"),
    ("dialog:insertLink", "insert.link"),
    ("ui:newComment", "review.newComment"),
    ("ui:newNote", "review.newNote"),
    ("ui:rowHeight", "home.rowHeight"),
    ("ui:columnWidth", "home.columnWidth"),
    ("dialog:find", "edit.find"),
    ("dialog:goTo", "edit.goTo"),
];

/// One chord. `cmd` is the portable Command/Ctrl key; `ctrl` is Control when the platform has
/// both (macOS). On Windows and Linux those are the same physical key, so matching treats either
/// token as that key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Chord {
    ctrl: bool,
    cmd: bool,
    alt: bool,
    shift: bool,
    key: Key,
}

impl Chord {
    fn from_press(key: Key, physical: Option<Key>, modifiers: Modifiers, mac: bool) -> Option<Self> {
        if is_modifier(key) {
            return None;
        }
        // Navigation follows the logical key, as the built-in keymap does: Num Lock off reports
        // a digit as the physical key of an arrow.
        let navigation = matches!(
            key,
            Key::ArrowLeft
                | Key::ArrowRight
                | Key::ArrowUp
                | Key::ArrowDown
                | Key::PageUp
                | Key::PageDown
                | Key::Home
                | Key::End
                | Key::Insert
                | Key::Delete
        );
        let key = if navigation { key } else { physical.unwrap_or(key) };
        let (cmd, ctrl) = if mac { (modifiers.command, modifiers.ctrl) } else { (modifiers.command || modifiers.ctrl, false) };
        Some(Self { ctrl, cmd, alt: modifiers.alt, shift: modifiers.shift, key })
    }

    /// `text` is portable notation (`Cmd+Shift+T`, `Ctrl+Alt+V`, `F6`).
    fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('+').map(str::trim).filter(|part| !part.is_empty()).collect();
        let mut ctrl = false;
        let mut cmd = false;
        let mut alt = false;
        let mut shift = false;
        let mut key = None;
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" if !last || parts.len() > 1 => ctrl = true,
                "cmd" | "command" if !last || parts.len() > 1 => cmd = true,
                "alt" | "option" if !last || parts.len() > 1 => alt = true,
                "shift" if !last || parts.len() > 1 => shift = true,
                _ if last => key = key_from_token(part),
                _ => return None,
            }
        }
        Some(Self { ctrl, cmd, alt, shift, key: key? })
    }

    fn matches(self, event: Self, mac: bool) -> bool {
        if self.key != event.key || self.alt != event.alt || self.shift != event.shift {
            return false;
        }
        if mac { self.ctrl == event.ctrl && self.cmd == event.cmd } else { (self.ctrl || self.cmd) == (event.ctrl || event.cmd) }
    }

    /// Portable form written to preferences. The command key is `Cmd`, so one binding follows the
    /// platform the way the built-in shortcuts do.
    fn canonical(self) -> String {
        self.join(self.cmd)
    }

    /// The spelling command search uses: `Cmd` on macOS, `Ctrl` elsewhere.
    fn palette(self, mac: bool) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.cmd {
            parts.push(if mac { "Cmd" } else { "Ctrl" });
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(key_token(self.key));
        dedup_join(&parts)
    }

    /// Ribbon tooltip spelling: glyph chords on macOS, `Ctrl+` elsewhere.
    fn ribbon(self, mac: bool) -> String {
        if !mac {
            return self.palette(false);
        }
        let mut text = String::new();
        if self.ctrl {
            text.push('⌃');
        }
        if self.cmd {
            text.push('⌘');
        }
        if self.alt {
            text.push('⌥');
        }
        if self.shift {
            text.push('⇧');
        }
        text.push_str(key_token(self.key));
        text
    }

    fn join(self, cmd: bool) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if cmd {
            parts.push("Cmd");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(key_token(self.key));
        parts.join("+")
    }

    fn event_modifiers(self, mac: bool) -> Modifiers {
        if mac {
            Modifiers { alt: self.alt, ctrl: self.ctrl, shift: self.shift, mac_cmd: self.cmd, command: self.cmd }
        } else {
            let ctrl = self.cmd || self.ctrl;
            Modifiers { alt: self.alt, ctrl, shift: self.shift, mac_cmd: false, command: ctrl }
        }
    }
}

fn dedup_join(parts: &[&str]) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in parts {
        if out.last() != Some(part) {
            out.push(part);
        }
    }
    out.join("+")
}

fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft | Key::ShiftRight | Key::ControlLeft | Key::ControlRight | Key::AltLeft | Key::AltRight | Key::SuperLeft | Key::SuperRight
    )
}

fn is_function(key: Key) -> bool {
    let Some(rest) = key.name().strip_prefix('F') else { return false };
    rest.parse::<u8>().is_ok()
}

fn is_letter(key: Key) -> bool {
    let name = key.name();
    name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic())
}

fn is_digit(key: Key) -> bool {
    key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_digit())
}

fn is_punctuation(key: Key) -> bool {
    matches!(
        key,
        Key::Comma
            | Key::Period
            | Key::Minus
            | Key::Plus
            | Key::Equals
            | Key::Semicolon
            | Key::Quote
            | Key::Backtick
            | Key::Slash
            | Key::Backslash
            | Key::OpenBracket
            | Key::CloseBracket
            | Key::Colon
            | Key::Pipe
            | Key::Questionmark
            | Key::Exclamationmark
            | Key::OpenCurlyBracket
            | Key::CloseCurlyBracket
    )
}

/// A chord that would insert a character. Those stay with the cell editor.
fn types_into_cell(chord: Chord) -> bool {
    if chord.ctrl || chord.cmd || chord.alt || is_function(chord.key) {
        return false;
    }
    is_letter(chord.key) || is_digit(chord.key) || is_punctuation(chord.key) || (chord.key == Key::Space && !chord.shift)
}

/// Why a chord cannot be assigned, if it is reserved or would type into a cell.
fn refusal(chord: Chord) -> Option<&'static str> {
    if types_into_cell(chord) {
        return Some("That key types into a cell. Use Ctrl, Alt, or a function key.");
    }
    // Ctrl+Shift+letter and Cmd+Shift+letter run macros. They are not application shortcuts.
    if (chord.cmd || chord.ctrl) && chord.shift && !chord.alt && is_letter(chord.key) {
        return Some("Ctrl+Shift+letter and Cmd+Shift+letter shortcuts are reserved for macros.");
    }
    // Alt+letter and F10 open key tips. A binding there would never be reached.
    if chord.alt && !chord.ctrl && !chord.cmd && !chord.shift && is_letter(chord.key) {
        return Some("Alt+letter shortcuts are reserved for key tips.");
    }
    if chord.key == Key::F10 && !chord.alt && !chord.ctrl && !chord.cmd && !chord.shift {
        return Some("F10 is reserved for key tips.");
    }
    None
}

fn key_from_token(token: &str) -> Option<Key> {
    // Built-in metadata names the shifted character (`Ctrl+Shift+%`, `Cmd+Shift+>`). The key
    // event reports the unshifted physical key, which is what a recorded chord stores.
    match token {
        "%" | "^" | "&" | "*" | "(" | ")" | "!" | "@" | "#" | "$" => shifted_digit(token),
        ">" => Some(Key::Period),
        "<" => Some(Key::Comma),
        "_" => Some(Key::Minus),
        "~" => Some(Key::Backtick),
        other => Key::from_name(other),
    }
}

fn shifted_digit(token: &str) -> Option<Key> {
    match token {
        "!" => Some(Key::Num1),
        "@" => Some(Key::Num2),
        "#" => Some(Key::Num3),
        "$" => Some(Key::Num4),
        "%" => Some(Key::Num5),
        "^" => Some(Key::Num6),
        "&" => Some(Key::Num7),
        "*" => Some(Key::Num8),
        "(" => Some(Key::Num9),
        ")" => Some(Key::Num0),
        _ => None,
    }
}

fn key_token(key: Key) -> &'static str {
    match key {
        Key::Equals => "=",
        Key::Plus => "+",
        Key::Minus => "-",
        Key::Period => ".",
        Key::Comma => ",",
        Key::Backtick => "`",
        Key::Quote => "'",
        Key::Semicolon => ";",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        other => other.name(),
    }
}

fn command_label(id: &str) -> String {
    gridcraft_engine::find_command(id).map(|spec| spec.label.to_string()).unwrap_or_else(|| id.to_string())
}

/// The shortcut the engine shows for this platform, before any user binding.
fn builtin_text(id: &str, mac: bool) -> Option<String> {
    let spec = gridcraft_engine::find_command(id)?;
    let keys = if mac {
        spec.shortcut_mac.or(spec.shortcut)
    } else if spec.shortcut_mac.is_some() && spec.shortcut_mac == spec.shortcut {
        // An identical Mac override marks a Mac-only shortcut.
        None
    } else {
        spec.shortcut
    }?;
    Some(if mac { keys.to_string() } else { keys.split('+').map(|part| if part == "Cmd" { "Ctrl" } else { part }).collect::<Vec<_>>().join("+") })
}

fn metadata_holder(chord: Chord, mac: bool) -> Option<&'static str> {
    gridcraft_engine::command_specs().iter().find_map(|spec| {
        let text = builtin_text(spec.id, mac)?;
        let stored = Chord::parse(&text)?;
        stored.matches(chord, mac).then_some(spec.id)
    })
}

fn hardcoded_holder(chord: Chord, mac: bool) -> Option<&'static str> {
    let modifiers = chord.event_modifiers(mac);
    if let Some(shortcut) = crate::keymap::lookup(chord.key, Some(chord.key), modifiers, mac)
        && let Some(id) = crate::keymap::shortcut_command(&shortcut)
    {
        return Some(id);
    }
    // Ribbon letter shortcuts run only with the command key held.
    if modifiers.command { crate::ribbon::bound_command(chord.key, modifiers) } else { None }
}

/// Another command that currently owns `chord`, ignoring `except` (the command being changed).
fn holder(map: &BTreeMap<String, String>, chord: Chord, mac: bool, except: &str) -> Option<String> {
    for (id, stored) in map {
        if id == except {
            continue;
        }
        if Chord::parse(stored).is_some_and(|bound| bound.matches(chord, mac)) {
            return Some(id.clone());
        }
    }
    for id in [metadata_holder(chord, mac), hardcoded_holder(chord, mac)].into_iter().flatten() {
        if id != except && !map.contains_key(id) {
            return Some(id.to_string());
        }
    }
    None
}

/// The command a user binding gives this key, if one does.
pub fn matched(map: &BTreeMap<String, String>, key: Key, physical: Option<Key>, modifiers: Modifiers, mac: bool) -> Option<String> {
    let event = Chord::from_press(key, physical, modifiers, mac)?;
    map.iter().find(|(_, stored)| Chord::parse(stored).is_some_and(|bound| bound.matches(event, mac))).map(|(id, _)| id.clone())
}

/// `true` when a user binding has replaced this command's shortcut.
pub fn is_overridden(map: &BTreeMap<String, String>, id: &str) -> bool {
    map.contains_key(id)
}

/// Runs a command from its user shortcut. Copy, cut and paste go through the system clipboard,
/// as the built-in chords do.
pub fn run(app: &mut SheetApp, ctx: &egui::Context, id: &str) {
    match id {
        "edit.copy" | "edit.cut" => app.copy_to_clipboard(ctx, id),
        "edit.paste" => app.paste_from_clipboard(),
        _ => app.run_or_alert(id, serde_json::json!({})),
    }
    app.grid.ensure_visible = true;
}

/// Assign `recorded` to `id`. A reserved chord, or one another command still owns, is refused.
pub fn assign(map: &mut BTreeMap<String, String>, id: &str, recorded: &str, mac: bool) -> Result<(), String> {
    let Some(chord) = Chord::parse(recorded) else { return Err("That shortcut was not recognized.".into()) };
    if let Some(message) = refusal(chord) {
        return Err(message.into());
    }
    if gridcraft_engine::find_command(id).is_none() {
        return Err("That command does not exist.".into());
    }
    if let Some(other) = holder(map, chord, mac, id) {
        return Err(format!("{} is already used by {}. Move that command first.", chord.palette(mac), command_label(&other)));
    }
    map.insert(id.to_string(), chord.canonical());
    Ok(())
}

/// Removes the override. The built-in shortcut returns. Refused when another user binding still
/// holds that built-in chord, because restoring it would steal the chord back.
pub fn clear(map: &mut BTreeMap<String, String>, id: &str, mac: bool) -> Result<(), String> {
    if !map.contains_key(id) {
        return Ok(());
    }
    if let Some(text) = builtin_text(id, mac)
        && let Some(builtin) = Chord::parse(&text)
    {
        for (other, stored) in map.iter() {
            if other == id {
                continue;
            }
            if Chord::parse(stored).is_some_and(|bound| bound.matches(builtin, mac)) {
                return Err(format!(
                    "{} is still assigned to {}. Move that command before restoring the built-in shortcut.",
                    builtin.palette(mac),
                    command_label(other)
                ));
            }
        }
    }
    map.remove(id);
    Ok(())
}

/// Command-search spelling of a stored chord (`Ctrl+Q` on Windows, `Cmd+Q` on macOS).
pub fn display_chord(text: &str, mac: bool) -> Option<String> {
    Chord::parse(text).map(|chord| chord.palette(mac))
}

/// The shortcut command search shows: the user binding, otherwise the engine's.
pub fn search_shortcut(map: &BTreeMap<String, String>, id: &str, builtin: Option<String>, mac: bool) -> Option<String> {
    if let Some(stored) = map.get(id) {
        return Chord::parse(stored).map(|chord| chord.palette(mac));
    }
    builtin
}

/// The current shortcut of the command `action` runs, in command-search spelling.
pub fn current_shortcut(map: &BTreeMap<String, String>, action: &str, mac: bool) -> Option<String> {
    let id = resolve_action(action)?;
    search_shortcut(map, id, builtin_text(id, mac), mac)
}

fn resolve_action(action: &str) -> Option<&str> {
    if let Some((_, id)) = ACTION_COMMANDS.iter().find(|(name, _)| *name == action) {
        return Some(*id);
    }
    gridcraft_engine::find_command(action).map(|spec| spec.id)
}

/// Hover text for a menu item, including its current shortcut when it has one.
pub fn hover_label(app: &SheetApp, label: &str, action: &str, mac: bool) -> Option<String> {
    let keys = current_shortcut(&app.ui.command_hotkeys, action, mac)?;
    Some(format!("{} ({keys})", app.ui.language.tr(label)))
}

/// Adds the current shortcut to a menu item's hover tooltip.
pub fn with_shortcut(app: &SheetApp, ui: &egui::Ui, response: egui::Response, label: &str, action: &str) -> egui::Response {
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    match hover_label(app, label, action, mac) {
        Some(text) => response.on_hover_text(text),
        None => response,
    }
}

fn tip_id() -> egui::Id {
    egui::Id::unique("gridcraft.command_hotkey_tips")
}

/// The override a ribbon tooltip should show for `tip`, when the user has set one.
pub fn published_tip(ui: &egui::Ui, tip: &str) -> Option<String> {
    ui.data(|data| data.get_temp::<HashMap<String, String>>(tip_id()).and_then(|map| map.get(tip).cloned()))
}

/// Publishes user shortcuts for ribbon tooltips, keyed by the English control name.
pub fn publish(ctx: &egui::Context, app: &SheetApp) {
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let mut map = HashMap::new();
    for (id, stored) in &app.ui.command_hotkeys {
        let Some(chord) = Chord::parse(stored) else { continue };
        let shown = chord.ribbon(mac);
        if let Some(spec) = gridcraft_engine::find_command(id) {
            map.insert(spec.label.to_string(), shown.clone());
        }
        if let Some((_, alias)) = TIP_ALIASES.iter().find(|(command, _)| *command == id) {
            map.insert((*alias).to_string(), shown);
        }
    }
    ctx.data_mut(|data| data.insert_temp(tip_id(), map));
}

/// A saved `commandHotkeys` value. Anything that is not a map of command ids to chords is dropped
/// so a hand-edited mistake cannot reject the rest of `ui.json`.
pub fn lenient<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error> {
    let value = Json::deserialize(deserializer)?;
    Ok(sanitize(&value))
}

fn sanitize(value: &Json) -> BTreeMap<String, String> {
    let Some(object) = value.as_object() else { return BTreeMap::new() };
    let mut map = BTreeMap::new();
    for (id, stored) in object {
        if id.is_empty() {
            continue;
        }
        let Some(text) = stored.as_str() else { continue };
        let Some(chord) = Chord::parse(text) else { continue };
        // A reserved or text-typing chord is not a binding, same as the Options page refusing it.
        if refusal(chord).is_some() {
            continue;
        }
        map.insert(id.clone(), chord.canonical());
    }
    map
}

/// Records one key press as a portable chord, or the reason it was refused.
pub fn record(key: Key, physical: Option<Key>, modifiers: Modifiers, mac: bool) -> Result<String, &'static str> {
    let Some(chord) = Chord::from_press(key, physical, modifiers, mac) else { return Err("") };
    if let Some(message) = refusal(chord) {
        return Err(message);
    }
    Ok(chord.canonical())
}

/// A key that is held now and was not held when recording started.
///
/// A frame that never sees the key event (a discarded pass, the font gate) still leaves the key
/// down. Recording reads that, once, instead of losing the chord.
pub fn record_held(down: &[Key], ignore: &[Key], modifiers: Modifiers, mac: bool) -> Result<String, &'static str> {
    let Some(key) = down.iter().copied().find(|key| !is_modifier(*key) && !ignore.contains(key)) else {
        return Err("");
    };
    record(key, Some(key), modifiers, mac)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
    const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL };
    const ALT: Modifiers = Modifiers { alt: true, ctrl: false, shift: false, mac_cmd: false, command: false };
    const MAC_CMD: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, mac_cmd: true, command: true };

    fn press(key: Key, modifiers: Modifiers, mac: bool) -> Chord {
        Chord::from_press(key, Some(key), modifiers, mac).unwrap()
    }

    #[test]
    fn windows_ctrl_matches_both_ctrl_and_cmd_tokens() {
        let event = press(Key::B, CTRL, false);
        assert!(Chord::parse("Cmd+B").unwrap().matches(event, false));
        assert!(Chord::parse("Ctrl+B").unwrap().matches(event, false));
        assert_eq!(press(Key::B, CTRL, false).canonical(), "Cmd+B");
        assert_eq!(Chord::parse("Cmd+B").unwrap().palette(false), "Ctrl+B");
        assert_eq!(Chord::parse("Cmd+B").unwrap().palette(true), "Cmd+B");
    }

    #[test]
    fn mac_command_and_control_stay_distinct() {
        let command = press(Key::E, MAC_CMD, true);
        let control = press(Key::E, Modifiers { ctrl: true, command: false, mac_cmd: false, ..MAC_CMD }, true);
        assert!(Chord::parse("Cmd+E").unwrap().matches(command, true));
        assert!(!Chord::parse("Ctrl+E").unwrap().matches(command, true));
        assert!(Chord::parse("Ctrl+E").unwrap().matches(control, true));
        assert!(!Chord::parse("Cmd+E").unwrap().matches(control, true));
    }

    #[test]
    fn shifted_symbols_use_the_physical_key() {
        let event = press(Key::Num5, CTRL_SHIFT, false);
        assert!(Chord::parse("Ctrl+Shift+%").unwrap().matches(event, false));
        assert!(Chord::parse("Cmd+Shift+>").unwrap().matches(press(Key::Period, CTRL_SHIFT, false), false));
    }

    #[test]
    fn reserved_chords_are_refused_and_conflicts_name_the_owner() {
        let mut map = BTreeMap::new();
        assert!(record(Key::M, Some(Key::M), CTRL_SHIFT, false).unwrap_err().contains("reserved for macros"));
        assert!(record(Key::H, Some(Key::H), ALT, false).unwrap_err().contains("reserved for key tips"));
        assert!(record(Key::F10, Some(Key::F10), Modifiers::NONE, false).unwrap_err().contains("F10 is reserved"));
        assert!(record(Key::B, Some(Key::B), Modifiers::NONE, false).unwrap_err().contains("types into a cell"));

        let recorded = record(Key::Q, Some(Key::Q), CTRL, false).unwrap();
        assign(&mut map, "home.alignRight", &recorded, false).unwrap();
        let err = assign(&mut map, "home.alignLeft", &recorded, false).unwrap_err();
        assert!(err.contains("Align Right") && err.contains("Move that command"), "{err}");
        let bold = assign(&mut map, "home.alignRight", "Cmd+B", false).unwrap_err();
        assert!(bold.contains("Bold") && bold.contains("already used"), "{bold}");
    }

    #[test]
    fn clearing_restores_the_builtin_unless_another_binding_holds_it() {
        let mut map = BTreeMap::new();
        assign(&mut map, "home.bold", "F6", false).unwrap();
        assign(&mut map, "home.alignRight", "Cmd+B", false).unwrap();
        let refused = clear(&mut map, "home.bold", false).unwrap_err();
        assert!(refused.contains("Align Right") && refused.contains("built-in"), "{refused}");
        assert!(map.contains_key("home.bold"), "a refused clear leaves the override");
        clear(&mut map, "home.alignRight", false).unwrap();
        clear(&mut map, "home.bold", false).unwrap();
        assert!(map.is_empty());
        clear(&mut map, "home.bold", false).unwrap();
    }

    #[test]
    fn a_bad_command_hotkeys_value_is_ignored() {
        let ui: crate::UiState = serde_json::from_str(r#"{"dark":true,"commandHotkeys":"nope"}"#).unwrap();
        assert!(ui.dark && ui.command_hotkeys.is_empty());
        let ui: crate::UiState =
            serde_json::from_str(r#"{"dark":true,"commandHotkeys":{"home.bold":"Cmd+Q","edit.copy":7,"nope":"not a chord"}}"#).unwrap();
        assert_eq!(ui.command_hotkeys.get("home.bold").map(String::as_str), Some("Cmd+Q"));
        assert_eq!(ui.command_hotkeys.len(), 1);
        assert!(ui.dark);
        let saved = serde_json::to_value(&ui).unwrap();
        assert_eq!(saved["commandHotkeys"]["home.bold"], "Cmd+Q");
        assert!(serde_json::to_value(crate::UiState::default()).unwrap().get("commandHotkeys").is_none());
    }

    #[test]
    fn search_shows_the_override_and_otherwise_the_builtin() {
        let mut map = BTreeMap::new();
        assert_eq!(search_shortcut(&map, "home.bold", builtin_text("home.bold", false), false).as_deref(), Some("Ctrl+B"));
        assign(&mut map, "home.bold", "F6", false).unwrap();
        assert_eq!(search_shortcut(&map, "home.bold", builtin_text("home.bold", false), false).as_deref(), Some("F6"));
        assert_eq!(current_shortcut(&map, "ui:formatCells", false).as_deref(), Some("Ctrl+1"));
    }
}
