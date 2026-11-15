//! Insert ▸ Library: the Library pane the Craft apps share ([`craft_library_egui`]), with
//! GridCraft's words and colours, over its `library.*` commands. The Pictures tab shows the
//! pictures in the user's Library folders; clicking one puts it on the sheet at the active cell,
//! as Insert ▸ Pictures ▸ Place over Cells does.
//!
//! The pane is one of the right-hand task panes ([`crate::panes`]), opened with the Insert tab's
//! Library button or the `insert.library` UI command.

use std::borrow::Cow;

use craft_library_egui::{Host, Labels, Palette};
pub use craft_library_egui::{ITEMS, MAX_SHOWN_FILES, State, words};
use egui::Ui;
use gridcraft_engine::library::NOUNS;
use serde_json::Value as Json;

use crate::SheetApp;
use crate::theme::Tokens;

/// The pane's id in [`crate::grid::GridState::pane`].
pub const PANE: &str = "library";

/// The context the catalogs key the pane's words with (`Library|Add`): some of its words, such as
/// "Add" and "Hidden", already have translations meant for other places.
pub const CONTEXT: &str = "Library|";

/// GridCraft's words in the shared pane.
pub const LABELS: Labels = Labels {
    nouns: NOUNS,
    tabs: &[(ITEMS, "Pictures")],
    item: "Picture",
    insert_item: "Insert picture",
    // A picture has no shapes or named elements of its own to place.
    insert_shapes: None,
    insert_element: None,
    search_hint: "Search pictures",
    add_folder_tip: "Show the pictures in a folder of PNG and JPEG files",
    empty: "Add a folder of PNG or JPEG pictures to see them here. Click a picture to put it on the sheet.",
    not_in_browser: "The Library shows the pictures in folders on this computer, which the browser version can't read. Use the desktop app to browse a Library.",
    aspect: 0.75,
    // A file is one picture: every picture in one grid, named under its thumbnail.
    one_per_file: true,
};

/// Every word of [`LABELS`] the pane shows, besides [`words::ALL`] (the catalogs translate both).
pub fn label_words() -> Vec<&'static str> {
    let l = &LABELS;
    let mut out: Vec<&'static str> = l.tabs.iter().map(|(_, label)| *label).collect();
    out.extend([l.item, l.insert_item, l.search_hint, l.add_folder_tip, l.empty, l.not_in_browser]);
    out.extend(l.insert_shapes.into_iter().chain(l.insert_element));
    out
}

/// `english`, one of the pane's words, in `language`: the catalogs' `Library|` entry, else English.
pub fn text(language: crate::i18n::Language, english: &str) -> Cow<'_, str> {
    let keyed = format!("{CONTEXT}{english}");
    let translated = language.tr(&keyed);
    if translated == english { Cow::Borrowed(english) } else { Cow::Owned(translated.to_owned()) }
}

/// The app, as the shared pane sees it.
pub struct SheetHost<'a>(pub &'a mut SheetApp);

impl Host for SheetHost<'_> {
    fn act(&mut self, id: &str, params: Json) -> Result<Json, String> {
        let r = self.0.run(id, params);
        if let Err(e) = &r {
            self.0.message = Some(("GridCraft".into(), crate::clean_error(e)));
        }
        r
    }
    fn ask(&mut self, id: &str, params: Json) -> Result<Json, String> {
        self.0.session.execute(id, params).map_err(|e| e.to_string())
    }
    fn folders(&self) -> Vec<String> {
        self.0.session.prefs.library_folders.clone()
    }
    fn pick_folder(&mut self) -> Option<Option<String>> {
        self.0.services.pick_folder.as_ref().map(|pick| pick())
    }
    fn can_place(&self) -> bool {
        self.0.session.active().is_some()
    }
    fn report(&mut self, message: String) {
        self.0.toast = Some((message, crate::now_ms()));
    }
    fn text<'a>(&self, english: &'a str) -> Cow<'a, str> {
        text(self.0.ui.language, english)
    }
}

/// Draw the pane's content (the task pane draws its title and close button).
pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let pal = Palette {
        text_dim: t.text_dim,
        text_faint: t.text_disabled,
        separator: t.separator,
        control: t.hover,
        border: t.input_border,
        accent: t.accent,
    };
    // The pane's state is the app's; it is lent to the pane while the app is the pane's host.
    let mut state = std::mem::take(&mut app.library);
    craft_library_egui::show(ui, &mut state, &mut SheetHost(app), &LABELS, &pal);
    app.library = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    /// The `{name}` placeholders in `s`, in order.
    fn placeholders(s: &str) -> Vec<&str> {
        let mut out = vec![];
        let mut rest = s;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            out.push(&rest[open + 1..open + close]);
            rest = &rest[open + close + 1..];
        }
        out.sort_unstable();
        out
    }

    #[test]
    fn every_language_translates_the_pane_and_keeps_its_placeholders() {
        let all: Vec<&str> = words::ALL.iter().copied().chain(label_words()).collect();
        for language in Language::ALL {
            for english in &all {
                let shown = text(language, english);
                assert!(!shown.trim().is_empty(), "{language:?} {english:?}");
                assert_eq!(placeholders(&shown), placeholders(english), "{language:?} {english:?} → {shown:?}");
            }
            if language == Language::En {
                assert!(all.iter().all(|e| text(language, e) == *e));
                continue;
            }
            // The words a person reads are translated (templates of names and numbers may stay).
            for english in [words::ADD_FOLDER, words::ALL_FOLDERS, words::READING, LABELS.empty, LABELS.search_hint] {
                assert_ne!(text(language, english), english, "{language:?} leaves {english:?} in English");
            }
            assert_ne!(language.tr("Library"), "Library", "{language:?}: the ribbon button");
        }
        // The pane's words have their own entries: "Add" is the arithmetic one elsewhere.
        assert_eq!(text(Language::Ja, words::ADD), "追加");
        assert_eq!(Language::Ja.tr("Add"), "加算");
    }
}
