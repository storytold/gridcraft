//! UI text lookup on top of `gridcraft-l10n`.
//!
//! Two ways to get a message:
//!
//! * [`Tr::tr`]`("English text")` for fixed texts without variables. The key is derived from the
//!   English text ([`key`]), so the call site stays readable and an untranslated or missing entry
//!   falls back to the English literal itself. Strings that are stored first and shown later (dialog
//!   titles, field labels, table rows) are wrapped in `msg!("English text")`, which evaluates to the
//!   literal and marks it for the key-coverage test, and passed to `tr` at display time.
//! * `Localizer::text("ui-<name>", &[("var", Arg::from(..))])` for messages with variables; the key
//!   is chosen by hand and lives in `locales/<tag>/ui.ftl`.
//!
//! `locales/en-US/ui.ftl` must hold, for every literal given to `tr`/`msg!`, a message whose key is
//! [`key`] of the literal and whose value is the literal (a test in `tests/l10n_keys.rs` checks it),
//! so English output never changes.

use std::borrow::Cow;

pub use gridcraft_l10n::{Arg, Localizer, Platform};

/// Marks an English UI text for translation; evaluates to the literal. Show it with [`Tr::tr`].
macro_rules! msg {
    ($s:literal) => {
        $s
    };
}
pub(crate) use msg;

/// Fluent key of an English UI text: `ui-` plus the lower-case ASCII letters and digits of the text
/// (other runs become one `-`), with `…` spelled `ellipsis` so `Open…` and `Open` differ.
pub fn key(english: &str) -> String {
    format!("ui-{}", gridcraft_l10n::slug(&english.replace('…', " ellipsis ")))
}

/// Localized fixed texts.
pub trait Tr {
    /// The message for `english` in the active language, or `english` when no message exists.
    fn tr(&self, english: &str) -> Cow<'static, str>;
}

impl Tr for Localizer {
    fn tr(&self, english: &str) -> Cow<'static, str> {
        self.get(&key(english), &[]).unwrap_or_else(|| Cow::Owned(english.to_string()))
    }
}

/// Keyboard layout family of an operating system: ⌘ chords on macOS and iOS, Ctrl chords elsewhere
/// (an unknown system, as on the web before the user agent is known, counts as a PC).
pub fn platform_of(os: egui::os::OperatingSystem) -> Platform {
    if os.is_mac() { Platform::Mac } else { Platform::Pc }
}

/// Keyboard layout family of the running system. egui reports the compile-time target natively and
/// the browser's user agent on the web, so a Mac browser gets the ⌘ chords.
pub fn platform(ctx: &egui::Context) -> Platform {
    platform_of(ctx.os())
}

/// The undo-list text of a history entry: the engine records the command's English label (without
/// a trailing `…`), which is shown in the interface language through the command's label. A label
/// that belongs to no command stays as the engine wrote it.
pub fn history_label(l: &Localizer, english: &str) -> String {
    gridcraft_engine::command_specs()
        .iter()
        .filter(|spec| spec.label.trim_end_matches('…') == english)
        .find_map(|spec| l.command_label(spec.id))
        .map_or_else(|| english.to_string(), |label| label.trim_end_matches('…').to_string())
}

/// A chord as shown in a tooltip. On a PC the Windows way: `Cmd+Shift+S` → `Ctrl+Shift+S` (the
/// command modifier is Ctrl there), `Alt+F5` → `Alt+F5`. On a Mac the glyphs: `Cmd+Shift+S` →
/// `⌘⇧S`, `Ctrl+G` → `⌃G`, `Alt+F5` → `⌥F5`.
pub fn chord_hint(chord: &str, platform: Platform) -> String {
    let mut rest = chord;
    let mut modifiers: Vec<&str> = Vec::new();
    // A chord may end in the key `+` (`Ctrl+Shift++`), so modifiers are peeled off the front.
    while let Some((name, tail)) =
        ["Cmd", "Shift", "Ctrl", "Alt"].iter().find_map(|m| rest.strip_prefix(m).and_then(|t| t.strip_prefix('+')).map(|t| (*m, t)))
    {
        modifiers.push(name);
        rest = tail;
    }
    match platform {
        Platform::Mac => {
            let glyphs: String = modifiers
                .iter()
                .map(|m| match *m {
                    "Cmd" => '⌘',
                    "Shift" => '⇧',
                    "Ctrl" => '⌃',
                    _ => '⌥',
                })
                .collect();
            format!("{glyphs}{rest}")
        }
        Platform::Pc => {
            let mut words: Vec<&str> = modifiers.iter().map(|m| if *m == "Cmd" { "Ctrl" } else { m }).collect();
            words.push(rest);
            words.join("+")
        }
    }
}

/// `Name (Ctrl+B)` on a PC, `Name (⌘B)` on a Mac: the localized `english` name plus the hint of the chord the active language assigns
/// to command `id` on `platform`; just the name when the command has no shortcut.
pub fn tip(l: &Localizer, platform: Platform, english: &str, id: &str) -> String {
    let name = l.tr(english);
    match l.shortcut_for(id, platform) {
        Some(chord) => format!("{name} ({})", chord_hint(chord, platform)),
        None => name.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_read_ctrl_on_a_pc_and_glyphs_on_a_mac() {
        assert_eq!(chord_hint("Cmd+N", Platform::Pc), "Ctrl+N");
        assert_eq!(chord_hint("Cmd+Shift+S", Platform::Pc), "Ctrl+Shift+S");
        assert_eq!(chord_hint("Ctrl+G", Platform::Pc), "Ctrl+G");
        assert_eq!(chord_hint("Alt+F5", Platform::Pc), "Alt+F5");
        assert_eq!(chord_hint("F9", Platform::Pc), "F9");
        assert_eq!(chord_hint("Ctrl+Shift++", Platform::Pc), "Ctrl+Shift++");
        assert_eq!(chord_hint("Cmd+Shift+S", Platform::Mac), "⌘⇧S");
        assert_eq!(chord_hint("Ctrl+G", Platform::Mac), "⌃G");
        assert_eq!(chord_hint("Alt+F5", Platform::Mac), "⌥F5");
        assert_eq!(chord_hint("Ctrl+Shift++", Platform::Mac), "⌃⇧+");
    }

    #[test]
    fn tooltips_follow_the_platform() {
        let en = Localizer::new("en-US");
        assert_eq!(tip(&en, Platform::Pc, "Save", "file.save"), "Save (Ctrl+S)");
        assert_eq!(tip(&en, Platform::Mac, "Save", "file.save"), "Save (⌘S)");
    }
}
