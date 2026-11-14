//! Compiled distribution identity and automatic update policy.
//! Cargo tracks `option_env!`; launch-time environment changes cannot alter this policy.

use std::borrow::Cow;
use std::sync::OnceLock;

/// Raw compile-time label, empty when unset. Display and policy helpers trim surrounding
/// whitespace and ignore labels containing controls or more than 32 Unicode characters.
/// Ordinary punctuation, including `&` and `)`, is preserved in the displayed identity.
pub const BUILD_LABEL: &str = match option_env!("GRIDCRAFT_BUILD_LABEL") {
    Some(label) => label,
    None => "",
};

fn normalized_label(label: &str) -> &str {
    let trimmed = label.trim();
    if label.chars().any(char::is_control) || trimmed.chars().count() > 32 { "" } else { trimmed }
}

/// Cached desktop identity: `GridCraft` for an empty/invalid label, otherwise
/// `GridCraft (<label>)`, for example `GridCraft (Preview)` or `GridCraft (R&D)`.
pub fn app_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| name_for_label(BUILD_LABEL)).as_str()
}

fn name_for_label(label: &str) -> String {
    let label = normalized_label(label);
    if label.is_empty() { "GridCraft".into() } else { format!("GridCraft ({label})") }
}

/// Name for muda menus only: `R&D` becomes `R&&D` so muda displays a literal `&`.
/// macOS also removes muda's mnemonic escaping before passing titles to AppKit.
pub fn menu_name() -> String {
    menu_name_for_label(BUILD_LABEL)
}

fn menu_name_for_label(label: &str) -> String {
    name_for_label(label).replace('&', "&&")
}

/// Borrow ordinary workbook titles; prefix labelled ones with the cached identity.
/// An empty workbook title displays the distribution name for labelled builds.
pub fn window_title(title: &str) -> Cow<'_, str> {
    title_for_name(title, app_name(), normalized_label(BUILD_LABEL).is_empty())
}

fn title_for_name<'a>(title: &'a str, name: &str, unlabelled: bool) -> Cow<'a, str> {
    if unlabelled {
        Cow::Borrowed(title)
    } else if title.is_empty() {
        Cow::Owned(name.into())
    } else {
        Cow::Owned(format!("{name} - {title}"))
    }
}

#[cfg(test)]
pub(crate) fn title_for_label<'a>(title: &'a str, label: &str) -> Cow<'a, str> {
    title_for_name(title, &name_for_label(label), normalized_label(label).is_empty())
}

/// Ordinary builds check at startup; valid labelled distributions do not.
/// Manual Help > Check for Updates remains available for either policy.
pub fn startup_update_check() -> bool {
    startup_check_for_label(BUILD_LABEL)
}

fn startup_check_for_label(label: &str) -> bool {
    normalized_label(label).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_identity_preserves_official_titles_and_labels_preview_windows() {
        assert_eq!(name_for_label(""), "GridCraft");
        assert_eq!(name_for_label("Preview"), "GridCraft (Preview)");
        assert_eq!(title_for_label("Book1", ""), "Book1");
        assert_eq!(title_for_label("", "Preview"), "GridCraft (Preview)");
        assert_eq!(title_for_label("Synthetic — Edited", "Preview"), "GridCraft (Preview) - Synthetic — Edited");
        assert_eq!(menu_name_for_label("R&D"), "GridCraft (R&&D)");
        assert_eq!(menu_name_for_label("Preview)"), "GridCraft (Preview))");
        assert_eq!(menu_name(), menu_name_for_label(BUILD_LABEL));
        assert_eq!(app_name(), name_for_label(BUILD_LABEL));
        assert_eq!(window_title("Synthetic"), title_for_label("Synthetic", BUILD_LABEL));
    }

    #[test]
    fn startup_policy_exercises_both_labelled_and_unlabelled_builds() {
        assert!(startup_check_for_label(""));
        assert!(!startup_check_for_label("Preview"));
        assert_eq!(startup_update_check(), startup_check_for_label(BUILD_LABEL));
        assert!(matches!(title_for_label("Book1", ""), Cow::Borrowed("Book1")));
    }

    #[test]
    fn labels_are_trimmed_and_invalid_labels_use_the_ordinary_policy() {
        assert_eq!(name_for_label("  Preview  "), "GridCraft (Preview)");
        assert!(!startup_check_for_label("  Preview  "));
        for label in ["   ", "line\nbreak", "control\u{0000}", &"x".repeat(33)] {
            assert_eq!(name_for_label(label), "GridCraft");
            assert_eq!(title_for_label("Book1", label), "Book1");
            assert!(startup_check_for_label(label));
        }
        assert_eq!(normalized_label(&"é".repeat(32)).chars().count(), 32);
    }
}
