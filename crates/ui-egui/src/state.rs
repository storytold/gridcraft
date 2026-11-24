/// Per-sheet view state (scroll position in sheet points).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SheetView {
    pub scroll: egui::Vec2,
}

/// Severity of a notice shown in the info bar under the ribbon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Warning,
}

/// An answer to the Save Changes question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveAnswer {
    Save,
    DontSave,
    Cancel,
}

/// How a save held back for an answer goes on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SaveRequest {
    /// The save command as it was run (`file.save` or `file.saveAs`, with its params).
    Command { id: String, params: serde_json::Value },
    /// The web's Save, which hands the .xlsx file to the browser.
    Download,
}

/// What follows a save once it has written the workbook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AfterSave {
    /// Close the workbook (Save Changes asked while closing it).
    pub close: bool,
    /// Then go on quitting.
    pub quit: bool,
}

/// A save that waits for the answer to what it would lose: all the question is about, so
/// answering it never acts on whatever happens to be active, or on what the question became.
#[derive(Clone, Debug)]
pub(crate) struct PendingSave {
    /// The workbook that asked (its document uid).
    pub doc: u64,
    pub request: SaveRequest,
    /// Exactly what the user is asked about.
    pub losses: gridcraft_engine::cmd::file::SaveLosses,
    pub after: AfterSave,
}

/// An answer to the question a save that would lose something asks first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveLossAnswer {
    /// Save in the format asked for, without what it can't keep.
    Anyway,
    /// Save as a GridCraft workbook (.json), which keeps it.
    Json,
    Cancel,
}

/// One open window, as View › Switch Windows and the macOS Window menu list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenWindow {
    /// The window's id, the `window` param of `view.switchWindows`.
    pub window: u64,
    /// Its workbook's `DocState::uid`.
    pub doc: u64,
    pub title: String,
    pub dirty: bool,
    pub active: bool,
}

impl OpenWindow {
    /// The title ("Book1", or "Book1:2" for a workbook's second window), with a bullet after it
    /// while the workbook has unsaved changes.
    pub fn label(&self) -> String {
        if self.dirty { format!("{} •", self.title) } else { self.title.clone() }
    }
}
