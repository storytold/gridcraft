//! Insert ▸ Library: the Library pane the Craft apps share ([`craft_library_egui`]), with
//! GridCraft's words and colours, over its `library.*` commands. The Pictures tab shows the
//! pictures in the user's Library folders; clicking one puts it on the sheet at the active cell,
//! as Insert ▸ Pictures ▸ Place over Cells does.
//!
//! The pane is one of the right-hand task panes ([`crate::panes`]), opened with the Insert tab's
//! Library button or the `insert.library` UI command.

use craft_library_egui::{Host, Labels, Palette};
pub use craft_library_egui::{ITEMS, MAX_SHOWN_FILES, State};
use egui::Ui;
use gridcraft_engine::library::NOUNS;
use serde_json::Value as Json;

use crate::SheetApp;
use crate::theme::Tokens;

/// The pane's id in [`crate::grid::GridState::pane`].
pub const PANE: &str = "library";

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
};

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
