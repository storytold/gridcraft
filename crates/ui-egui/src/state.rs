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
