//! Compiled distribution identity and automatic update policy.
//! Cargo tracks `option_env!`; changing a launch-time environment cannot alter this policy.

use std::sync::OnceLock;

/// Empty for ordinary builds.
pub const BUILD_LABEL: &str = match option_env!("GRIDCRAFT_BUILD_LABEL") {
    Some(label) => label,
    None => "",
};

/// The application name used by the desktop host.
pub fn app_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| name_for_label(BUILD_LABEL)).as_str()
}

fn name_for_label(label: &str) -> String {
    if label.is_empty() { "GridCraft".into() } else { format!("GridCraft ({label})") }
}

/// Preserve ordinary document titles; identify labelled distributions in each window.
pub fn window_title(title: &str) -> String {
    title_for_label(title, BUILD_LABEL)
}

fn title_for_label(title: &str, label: &str) -> String {
    if label.is_empty() {
        title.into()
    } else if title.is_empty() {
        name_for_label(label)
    } else {
        format!("{title} - {}", name_for_label(label))
    }
}

/// Labelled distributions do not ask for automatic updates; manual checks remain separate.
pub const fn startup_update_check() -> bool {
    BUILD_LABEL.is_empty()
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
        assert_eq!(title_for_label("Synthetic — Edited", "Preview"), "Synthetic — Edited - GridCraft (Preview)");
        assert_eq!(app_name(), name_for_label(BUILD_LABEL));
        assert_eq!(window_title("Synthetic"), title_for_label("Synthetic", BUILD_LABEL));
        assert_eq!(startup_update_check(), BUILD_LABEL.is_empty());
    }
}
