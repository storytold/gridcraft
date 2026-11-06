//! The Python editor: a resizable window for writing a Python cell's code, with line numbers and
//! Python colours. It opens from the formula bar's editor button (shown in Python mode and on a
//! selected Python cell), from "Edit in Python Editor" in a Python cell's menu, and from
//! `ui.pythonEditor`.
//!
//! The same window, titled Initialization, edits the workbook's initialization code. It opens from the
//! Python tab and Formulas › Python, from what says the code failed, and from
//! `ui.pythonInitialization`. There is no cell then: Apply is `python.initialization`, and the rules
//! below hold for the workbook's code as they do for a cell's, with its own limit.
//!
//! The window keeps its own copy of the code. Enter and Tab work as in the formula bar's Python
//! mode; Ctrl+Enter (Cmd+Enter on macOS) or Apply puts the code in the cell through `python.set`,
//! and the window stays open; Esc or Close closes it. There is one editor at a time, and it belongs
//! to one cell of one workbook. Nothing loses its code without an answer: closing it, opening it
//! on another cell, closing its workbook, switching to another workbook or quitting first asks
//! whether to apply or discard unsaved changes, in the window. Programmatic calls never ask: they
//! refuse while there are unsaved changes, unless told to discard them (`force`).
//!
//! Apply writes only to the cell the window opened on: when the cell no longer holds what it held
//! when the window opened it or last applied (a row or column inserted before it moved another
//! cell to its address, or its code changed from elsewhere), Apply is refused and the code stays in
//! the window, rather than replacing another cell's content.
//!
//! The code can't grow past what a cell holds (`PyCode::fits`, the limit `python.set` checks):
//! typing or pasting past it is refused, with a message, as it happens.
//!
//! With a native window per workbook window (`SheetApp::native_windows`), every native window draws
//! the app in turn, each with its own workbook active. The editor is drawn only in its own
//! workbook's window (the one it opened in, or that workbook's most recent one), so drawing another
//! workbook's window neither closes it nor counts as a switch to that workbook; a question it asks,
//! or an opening on another workbook's cell, brings its window forward.

use std::sync::Arc;

use egui::{Align, Id, Layout, Rect, RichText, Ui, pos2, vec2};
use gridcraft_engine::core::{CellRef, RangeRef, Value, quote_sheet, split_sheet};
use gridcraft_engine::model::python::{MAX_FORMULA_CHARS, MAX_INIT_CHARS, init_fits, normalize_code};
use gridcraft_engine::model::{CellPicture, PyCode, Sheet};
use serde_json::{Value as Json, json};

use crate::code_editor::{self, FieldOptions, Lang};
use crate::python_card::{EXCEL_VALUE, PYTHON_OBJECT};
use crate::theme::{self, Tokens};
use crate::{SheetApp, clean_error, formula_bar};

/// The window's title.
pub const TITLE: &str = "Python Editor";
/// The formula bar button's and the cell menu's label.
pub const EDIT_IN_EDITOR: &str = "Edit in Python Editor";
/// Shown in the window while the code differs from the cell's.
pub const UNSAVED: &str = "Unsaved changes";
/// The question asked before unsaved code would be lost.
pub const ASK: &str = "The code for {cell} has unsaved changes.";
/// The same, once another workbook is active.
pub const ASK_IN: &str = "The code for {cell} in {workbook} has unsaved changes.";
/// Typing or pasting past the limit.
pub const TOO_LONG: &str = "The code can't be longer: a Python cell holds {max} characters, counted as a PY formula.";
/// The cell no longer holds what it held when the window opened it or last applied (another cell
/// moved to its address, or its code changed elsewhere) while the window has unsaved changes.
pub const CHANGED: &str = "{cell} changed while it was open in the Python editor. Copy the code, or close the editor and open it again on the cell.";
/// The window's title for the initialization code, and its label wherever it is named.
pub const INITIALIZATION: &str = "Initialization";
/// What the initialization code is, under the window's title and on the ribbon.
pub const INITIALIZATION_TIP: &str = "The code that runs before this workbook's Python cells";
/// The questions about unsaved initialization code.
pub const ASK_INIT: &str = "The initialization code has unsaved changes.";
pub const ASK_INIT_IN: &str = "The initialization code of {workbook} has unsaved changes.";
/// Typing or pasting past the initialization code's limit.
pub const TOO_LONG_INIT: &str = "The code can't be longer: initialization code holds {max} characters.";
/// The workbook's code changed while the window has unsaved changes.
pub const CHANGED_INIT: &str =
    "The initialization code changed while it was open in the editor. Copy your code, or close the editor and open it again.";
/// Apply refused for the same reason.
pub const STALE_INIT: &str =
    "The initialization code changed while it was open in the editor, so your code was not applied. Copy it, or close the editor and open it again.";
/// Apply refused for the same reason.
pub const STALE: &str = "{cell} changed while it was open in the Python editor, so the code was not applied. Copy the code, or close the editor and open it again on the cell.";
/// The keys, under the code.
pub const HINT: &str = "{apply} applies the code. Esc closes the editor.";
/// The window's code went with a workbook that closed some other way (the last resort).
pub const COPIED: &str = "The Python editor's unsaved code was copied to the clipboard.";
pub const APPLY: &str = "Apply";
pub const DISCARD: &str = "Discard";
pub const CANCEL: &str = "Cancel";
pub const CLOSE: &str = "Close";

/// Lines of code the window shows when it opens, room permitting.
pub const LINES: usize = 40;
/// The window's distance from the app window's edges.
const MARGIN: f32 = 8.0;
/// The window's frame and title bar around its contents (egui's window margins, the title row).
const FRAME: egui::Vec2 = egui::Vec2::new(20.0, 52.0);
/// The header and footer of the contents, around the code.
const CHROME_HEIGHT: f32 = 92.0;

/// What the editor edits: a cell, by its workbook (by uid), sheet and address, or the workbook's
/// initialization code, which has no sheet or cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub doc: u64,
    pub sheet: usize,
    pub cell: CellRef,
    pub initialization: bool,
}

impl Target {
    /// The initialization code of workbook `doc`.
    pub fn initialization(doc: u64) -> Target {
        Target { doc, sheet: 0, cell: CellRef::new(0, 0), initialization: true }
    }
}

/// What the window asks about before unsaved code would be lost, and what happens after.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    /// Esc, Close or the window's X: the window closes.
    Close,
    /// The editor was asked for another cell (or with other code, from the formula bar): it opens
    /// there.
    Open { target: Target, from_bar: bool },
    /// Another workbook is active: the window closes, or Cancel goes back to its workbook.
    Switched,
    /// File › Close on the window's workbook: the workbook closes (with `file.close` and these
    /// params, which ask about the workbook's own changes).
    CloseWorkbook(Json),
    /// The close button of the last window on the window's workbook: the window closes (with
    /// `window.close` and these params, which ask about the workbook's own changes).
    CloseWindow(Json),
    /// Quitting (the app window's close button, Quit or ⌘Q, the Dock or the system): the quit goes
    /// on, asking about each unsaved workbook.
    Quit,
}

/// An answer to [`Pending`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Apply,
    Discard,
    Cancel,
}

/// A message under the code.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// An edit was refused: the code would not fit in a cell.
    TooLong,
    /// `python.set` refused the code; its message.
    Refused(String),
    /// Apply was refused: the cell no longer holds what it held (see [`Held`]).
    Stale,
}

/// What a cell holds, as Apply checks it: a Python cell's code and return type, or another cell's
/// formula, constant value or picture. Not its result or its format, so a recalculation or a new
/// format is not a change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Held {
    python: Option<(String, bool)>,
    formula: Option<(String, Option<RangeRef>)>,
    value: Value,
    picture: Option<Arc<CellPicture>>,
}

impl Held {
    fn of(sh: &Sheet, c: CellRef) -> Held {
        let picture = sh.cell_pictures.get(&c).cloned();
        match sh.cell(c) {
            Some(cell) => match (&cell.py, &cell.formula) {
                (Some(py), _) => Held { python: Some((py.code.clone(), py.object)), picture, ..Held::default() },
                (None, Some(f)) => Held { formula: Some((f.text.clone(), f.array)), picture, ..Held::default() },
                (None, None) => Held { value: cell.value.clone(), picture, ..Held::default() },
            },
            None => Held { picture, ..Held::default() },
        }
    }
}

/// The open editor.
#[derive(Clone, Debug)]
pub struct PyWindow {
    pub doc: u64,
    /// The engine window it opened in, one of its workbook's: with native windows, it is drawn
    /// there (or, once that window closes, in its workbook's most recent one).
    pub window: Option<u64>,
    pub sheet: usize,
    /// The sheet's name, as it was when the window last saw its sheet.
    pub sheet_name: String,
    /// It edits the workbook's initialization code: `sheet`, `sheet_name` and `cell` mean nothing.
    pub initialization: bool,
    /// The sheet's identity ([`gridcraft_engine::model::SheetUid`]): the window follows its sheet
    /// by it when sheets move or are renamed, and never takes another sheet for it, even one of
    /// the same name in the same place.
    sheet_uid: u64,
    /// This opening of the window: the code field's id, so its caret, focus and undo history stay
    /// with it when its sheet moves or is renamed, and a new opening starts afresh.
    serial: u64,
    pub cell: CellRef,
    /// The code being edited.
    pub text: String,
    /// The cell's code when the window loaded it or last applied it.
    pub saved: String,
    /// What the cell held when the window opened it or last applied: Apply writes only while it
    /// still holds that.
    held: Held,
    pub notice: Option<Notice>,
    pub pending: Option<Pending>,
    /// Put the keyboard in the code on the next frame.
    pub focus: bool,
    /// Put the caret here (a char index) on the next frame.
    pub caret: Option<usize>,
    /// The code's length counted as a PY formula, worked out when the code changes.
    count: Option<usize>,
    /// The line-number gutter's width, last frame.
    pub gutter: f32,
    /// The window on screen, last frame.
    pub rect: Option<Rect>,
}

impl PyWindow {
    pub fn target(&self) -> Target {
        Target { doc: self.doc, sheet: self.sheet, cell: self.cell, initialization: self.initialization }
    }

    /// Whether the code differs from the cell's code as last loaded or applied.
    pub fn dirty(&self) -> bool {
        self.text != self.saved && normalize_code(&self.text) != self.saved
    }

    /// The cell's address, with its sheet: `Sheet1!A1`. Initialization code is called that.
    pub fn address(&self) -> String {
        if self.initialization { INITIALIZATION.to_string() } else { format!("{}!{}", quote_sheet(&self.sheet_name), self.cell.a1()) }
    }

    /// The window's title, as a translation key.
    pub fn title(&self) -> &'static str {
        if self.initialization { INITIALIZATION } else { TITLE }
    }

    fn set_text(&mut self, text: String) {
        self.text = text;
        self.count = None;
    }
}

/// Whether `code` fits in a cell, as `python.set` checks it. The work is bounded by the limit,
/// not by the length of `code`.
pub fn fits(code: &str) -> bool {
    code.chars().nth(MAX_FORMULA_CHARS).is_none() && PyCode::new(code).fits()
}

/// `code`'s length counted as a PY formula, `PY("…",0)` (the measure of [`PyCode::fits`]).
pub fn formula_len(code: &str) -> usize {
    PyCode::new(code).to_file_formula().chars().count()
}

/// Whether the editor takes an edit from `old` to `new`: the new code fits in a cell, or (for code
/// that came in longer than the limit) the edit doesn't make it longer.
pub fn accept_code(new: &str, old: &str) -> bool {
    fits(new) || (new.len() <= old.len() && formula_len(new) <= formula_len(old))
}

/// [`accept_code`] for initialization code, whose limit is on its characters. The work is bounded by
/// the limit, not by the length of `new`.
pub fn accept_init_code(new: &str, old: &str) -> bool {
    init_fits(new) || (new.len() <= old.len() && new.chars().nth(old.chars().count().saturating_add(1)).is_none())
}

/// The window, for egui (its position and size are kept under this id).
pub fn window_id() -> Id {
    Id::unique("gridcraft.python_editor.window")
}

/// The code field's id: one per opening of the window (see `PyWindow::serial`).
pub fn editor_id(w: &PyWindow) -> Id {
    Id::unique("gridcraft.python_editor").with(w.serial)
}

/// Numbers the openings of the window.
static OPENINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn doc_index(app: &SheetApp, uid: u64) -> Option<usize> {
    app.session.documents().iter().position(|d| d.uid == uid)
}

/// The workbook's initialization code (empty for a workbook without any).
fn init_code(app: &SheetApp, doc: u64) -> String {
    let d = app.session.documents().iter().find(|d| d.uid == doc);
    d.and_then(|d| d.wb.python_env.as_ref()).map(|e| e.code.clone()).unwrap_or_default()
}

/// The Python code of a cell (empty for a cell without code), and whether it is a Python object.
/// For the initialization code, that code.
fn cell_code(app: &SheetApp, t: Target) -> (String, bool) {
    if t.initialization {
        return (init_code(app, t.doc), false);
    }
    let py = app
        .session
        .documents()
        .iter()
        .find(|d| d.uid == t.doc)
        .and_then(|d| d.wb.sheet(t.sheet))
        .and_then(|sh| sh.cell(t.cell))
        .and_then(|c| c.py.clone());
    py.map(|p: Arc<PyCode>| (p.code.clone(), p.object)).unwrap_or_default()
}

/// What cell `t` holds, while its workbook and sheet are there. Initialization code is checked by its
/// text.
fn held(app: &SheetApp, t: Target) -> Option<Held> {
    if t.initialization {
        return None;
    }
    let d = app.session.documents().iter().find(|d| d.uid == t.doc)?;
    d.wb.sheet(t.sheet).map(|sh| Held::of(sh, t.cell))
}

/// The window's own sheet, while it is at the window's place under the window's name (`sync`
/// keeps both up to date).
fn own_sheet<'a>(app: &'a SheetApp, w: &PyWindow) -> Option<&'a Sheet> {
    let d = app.session.documents().iter().find(|d| d.uid == w.doc)?;
    d.wb.sheet(w.sheet).filter(|sh| sh.uid.get() == w.sheet_uid && sh.name == w.sheet_name)
}

/// The window's cell's code and return type, while its sheet is there (see `sync`).
fn window_cell(app: &SheetApp, w: &PyWindow) -> Option<(String, bool)> {
    if w.initialization {
        return None;
    }
    own_sheet(app, w)?;
    Some(cell_code(app, w.target()))
}

/// Whether the window's cell no longer holds what it held when the window opened it or last
/// applied: another cell moved to its address, or its content changed. Also when another sheet is
/// at the window's place under its name: its sheet was deleted and another put there (by a macro,
/// say) before the window saw it, and that sheet's cells are not the window's.
fn stale(app: &SheetApp, w: &PyWindow) -> bool {
    if w.initialization {
        return init_code(app, w.doc) != w.saved;
    }
    let d = app.session.documents().iter().find(|d| d.uid == w.doc);
    match d.and_then(|d| d.wb.sheet(w.sheet)).filter(|sh| sh.name == w.sheet_name) {
        Some(sh) if sh.uid.get() == w.sheet_uid => Held::of(sh, w.cell) != w.held,
        Some(_) => true,
        None => false,
    }
}

/// Whether the code of `t` is out of view: a Hidden cell, or the cell whose result spills over
/// it, on a protected sheet. The editor never shows such code.
fn target_hidden(app: &SheetApp, t: Target) -> bool {
    if t.initialization {
        return false;
    }
    let d = app.session.documents().iter().find(|d| d.uid == t.doc);
    d.and_then(|d| d.wb.sheet(t.sheet).map(|sh| gridcraft_engine::display::formula_hidden(&d.wb, sh, t.cell))).unwrap_or(false)
}

/// The sheet the window follows, with its name: its own sheet, by identity, wherever it moved and
/// whatever it is called now. A deleted sheet is not followed to the sheet that took its place,
/// even one of the same name: the window keeps its code, and Apply is refused for want of the
/// sheet. An undo of the deletion brings the sheet back, identity and all.
fn followed(app: &SheetApp, w: &PyWindow) -> Option<(usize, String)> {
    let d = app.session.documents().iter().find(|d| d.uid == w.doc)?;
    d.wb.sheets.iter().position(|sh| sh.uid.get() == w.sheet_uid).and_then(|i| d.wb.sheet(i).map(|sh| (i, sh.name.clone())))
}

/// Whether the code the window took from its cell is hidden on any protected sheet of its workbook:
/// a sheet whose cell at the window's address is out of view ([`target_hidden`]) and holds that
/// code. This doesn't depend on which sheet the window follows: once its sheet is deleted it
/// follows none, and a copy of that sheet protected with the cell Hidden never keeps the code in
/// view.
fn saved_code_hidden(app: &SheetApp, w: &PyWindow) -> bool {
    if w.initialization {
        return false;
    }
    let Some(d) = app.session.documents().iter().find(|d| d.uid == w.doc) else { return false };
    let holds = |sh: &Sheet| sh.cell(w.cell).and_then(|c| c.py.as_ref()).is_some_and(|p| p.code == w.saved);
    d.wb.sheets.iter().any(|sh| holds(sh) && gridcraft_engine::display::formula_hidden(&d.wb, sh, w.cell))
}

/// Whether the window's code is out of view now: its cell, on the sheet it follows, is Hidden or
/// inside the spill of a Hidden cell on a protected sheet, or the code it took from its cell is
/// hidden on another sheet ([`saved_code_hidden`]). `sync` closes the window by this rule on the
/// next frame; `state` checks it when called, since a control batch can protect the sheet before
/// that frame.
fn code_hidden(app: &SheetApp, w: &PyWindow) -> bool {
    if w.initialization {
        return false;
    }
    followed(app, w).is_some_and(|(sheet, _)| target_hidden(app, Target { sheet, ..w.target() })) || saved_code_hidden(app, w)
}

/// The message for a cell whose code is hidden.
fn hidden_error(cell: CellRef) -> String {
    format!("The contents of {} are hidden because the sheet is protected, so the Python editor can't open them.", cell.a1())
}

/// Whether the editor would open on `cell` of sheet `sheet` of the active workbook: not on code
/// hidden on a protected sheet. The formula bar's button and the cell menu offer it only then.
pub fn can_open(app: &SheetApp, sheet: usize, cell: CellRef) -> bool {
    let Some(d) = app.session.active() else { return false };
    let Some(sh) = d.wb.sheet(sheet) else { return false };
    let at = sh.merge_at(cell).map(|m| m.start).unwrap_or(cell);
    !gridcraft_engine::display::formula_hidden(&d.wb, sh, at)
}

/// The cell the editor opens on for `cell` on sheet `sheet` of the active workbook: the top left
/// of a merged area, or the Python cell whose result spills over it. Refused when its code is
/// hidden on a protected sheet.
pub fn target(app: &SheetApp, sheet: usize, cell: CellRef) -> Result<Target, String> {
    let d = app.session.active().ok_or("No workbook is open.")?;
    let sh = d.wb.sheet(sheet).ok_or("No such sheet.")?;
    let at = sh.merge_at(cell).map(|m| m.start).unwrap_or(cell);
    if gridcraft_engine::display::formula_hidden(&d.wb, sh, at) {
        return Err(hidden_error(at));
    }
    let at = match sh.spill_anchor(at) {
        Some(anchor) if sh.cell(anchor).is_some_and(|c| c.py.is_some()) => anchor,
        Some(anchor) => return Err(format!("{} is part of the result spilled from {}; it can't hold Python code.", at.a1(), anchor.a1())),
        None => at,
    };
    Ok(Target { doc: d.uid, sheet, cell: at, initialization: false })
}

/// Opens the window on `t`, replacing what it showed: the cell's code, or `text` (with the caret
/// at `caret`) when the code comes from the formula bar.
fn open_fresh(app: &mut SheetApp, t: Target, text: Option<(String, usize)>) {
    let (code, _) = cell_code(app, t);
    let doc = app.session.documents().iter().find(|d| d.uid == t.doc);
    let sheet = doc.and_then(|d| d.wb.sheet(t.sheet));
    let (sheet_name, sheet_uid) = (sheet.map(|s| s.name.clone()), sheet.map_or(0, |s| s.uid.get()));
    let (text, caret) = text.unwrap_or_else(|| (code.clone(), 0));
    let held = held(app, t).unwrap_or_default();
    let active = app.session.active_window();
    let window = active.filter(|w| app.session.window(*w).is_some_and(|x| x.doc_uid == t.doc)).or_else(|| app.session.doc_window(t.doc));
    app.py_window = Some(PyWindow {
        doc: t.doc,
        window,
        sheet: t.sheet,
        sheet_name: sheet_name.unwrap_or_default(),
        initialization: t.initialization,
        sheet_uid,
        serial: OPENINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        cell: t.cell,
        text,
        saved: code,
        held,
        notice: None,
        pending: None,
        focus: true,
        caret: Some(caret),
        count: None,
        gutter: 0.0,
        rect: None,
    });
    bring_forward(app);
}

/// The engine window the editor is drawn in: the one it opened in while that window shows its
/// workbook, else its workbook's most recent window. `None` once its workbook is closed.
fn home(app: &SheetApp, w: &PyWindow) -> Option<u64> {
    w.window.filter(|id| app.session.window(*id).is_some_and(|x| x.doc_uid == w.doc)).or_else(|| app.session.doc_window(w.doc))
}

/// Whether the window being drawn is the editor's: always with a single app window; with native
/// windows, only its home window ([`home`]).
fn drawn_here(app: &SheetApp) -> bool {
    !app.native_windows || app.py_window.as_ref().is_some_and(|w| home(app, w).is_some() && home(app, w) == app.session.active_window())
}

/// With native windows, makes the editor's window the active one, so it comes forward (as a
/// command that activates a window does) and shows what the editor asks or opened.
fn bring_forward(app: &mut SheetApp) {
    if !app.native_windows {
        return;
    }
    if let Some(h) = app.py_window.as_ref().and_then(|w| home(app, w))
        && app.session.active_window() != Some(h)
    {
        app.session.activate_window(h);
    }
}

/// The message for unsaved changes that a programmatic call would discard.
fn unsaved_error(w: &PyWindow) -> String {
    format!("The Python editor has unsaved changes to {}. Apply them, or pass force: true to discard them.", w.address())
}

/// Opens the editor on `t`. `text` is code (and a caret) the formula bar was editing for `t`, when
/// it differs from the cell's. When the window has unsaved changes that this would discard, it
/// asks first (`ask`, the interface's routes), discards them (`force`), or refuses.
pub fn request_open(app: &mut SheetApp, t: Target, text: Option<(String, usize)>, ask: bool, force: bool) -> Result<(), String> {
    if target_hidden(app, t) {
        return Err(hidden_error(t.cell));
    }
    // A window whose cell changed is not on `t` any more, even at the same address: opening it
    // again starts afresh (after asking about its unsaved code).
    let moved = app.py_window.as_ref().is_some_and(|w| stale(app, w));
    let Some(w) = app.py_window.as_mut() else {
        open_fresh(app, t, text);
        return Ok(());
    };
    let same = w.target() == t && !moved;
    let conflict = w.dirty() && (!same || text.as_ref().is_some_and(|(code, _)| normalize_code(code) != normalize_code(&w.text)));
    if conflict && !force {
        if ask {
            w.pending = Some(Pending::Open { target: t, from_bar: text.is_some() });
            bring_forward(app);
            return Ok(());
        }
        return Err(unsaved_error(w));
    }
    if same && !conflict {
        if let Some((code, caret)) = text {
            w.set_text(code);
            w.caret = Some(caret);
        }
        w.focus = true;
        w.pending = None;
        return Ok(());
    }
    open_fresh(app, t, text);
    Ok(())
}

/// The formula bar's Python draft of `t`, when it belongs to the active window: a draft of another
/// workbook, or of another window of the same workbook, at the same sheet and cell is not `t`'s
/// (the next frame commits it in its own window).
fn bar_draft(app: &SheetApp, t: Target) -> Option<&crate::editor::EditState> {
    let active = app.session.active().map(|d| d.uid) == Some(t.doc);
    app.editor.as_ref().filter(|e| {
        active
            && e.python
            && e.doc == t.doc
            && e.window.is_none_or(|w| app.session.active_window() == Some(w))
            && e.sheet == t.sheet
            && e.cell == t.cell
    })
}

/// The formula bar's Python edit of `t`, when its code differs from the cell's: its text and caret.
fn bar_code(app: &SheetApp, t: Target) -> Option<(String, usize)> {
    let ed = bar_draft(app, t)?;
    let (code, _) = cell_code(app, t);
    (normalize_code(&ed.text) != code).then(|| (ed.text.clone(), ed.caret))
}

/// Ends the formula bar's Python edit of `t`: its code went to the window. A draft that isn't
/// `t`'s ([`bar_draft`]) is left alone.
fn end_bar_edit(app: &mut SheetApp, t: Target) {
    if bar_draft(app, t).is_some() {
        app.cancel_edit();
    }
}

/// The cell the formula bar's editor button is for: the cell being edited in Python mode, or the
/// selected Python cell.
fn bar_cell(app: &SheetApp) -> Option<(usize, CellRef)> {
    match app.editor.as_ref() {
        Some(ed) if ed.python => Some((ed.sheet, ed.cell)),
        _ => crate::python_card::active_python_cell(app).map(|(_, sheet, cell, _)| (sheet, cell)),
    }
}

/// Whether the formula bar offers its editor button: on a cell `open_from_bar` would open.
pub fn bar_offers(app: &SheetApp) -> bool {
    bar_cell(app).is_some_and(|(sheet, cell)| can_open(app, sheet, cell))
}

/// Whether the cell menu offers "Edit in Python Editor": on a selected Python cell whose code is
/// not hidden.
pub fn menu_offers(app: &SheetApp) -> bool {
    crate::python_card::active_python_cell(app).is_some_and(|(_, sheet, cell, _)| can_open(app, sheet, cell))
}

/// The formula bar's editor button: opens the editor on the cell being edited in Python mode,
/// with its code as typed so far, or on the selected Python cell. The formula bar's edit moves to
/// the window. Nothing on code hidden on a protected sheet.
pub fn open_from_bar(app: &mut SheetApp) {
    if let Some((sheet, cell)) = bar_cell(app).filter(|&(sheet, cell)| can_open(app, sheet, cell)) {
        open_from_ui(app, sheet, cell, true);
    }
}

/// The cell menu's "Edit in Python Editor", on the selected Python cell. Nothing on code hidden on
/// a protected sheet.
pub fn open_from_menu(app: &mut SheetApp) {
    if let Some((_, sheet, cell, _)) = crate::python_card::active_python_cell(app).filter(|(_, sheet, cell, _)| can_open(app, *sheet, *cell)) {
        open_from_ui(app, sheet, cell, false);
    }
}

fn open_from_ui(app: &mut SheetApp, sheet: usize, cell: CellRef, from_bar: bool) {
    let t = match target(app, sheet, cell) {
        Ok(t) => t,
        Err(e) => {
            app.message = Some(("GridCraft".into(), e));
            return;
        }
    };
    let text = if from_bar { bar_code(app, t) } else { None };
    // An open window that asks first keeps the formula bar's edit until it is answered.
    if request_open(app, t, text, true, false).is_ok() && app.py_window.as_ref().is_some_and(|w| w.pending.is_none()) && from_bar {
        end_bar_edit(app, t);
    }
}

/// Whether the window has the initialization code of workbook `doc`.
pub fn has_initialization(app: &SheetApp, doc: u64) -> bool {
    app.py_window.as_ref().is_some_and(|w| w.initialization && w.doc == doc)
}

/// The Python tab's and Formulas' Initialization buttons, and what says the code failed: opens the
/// editor on the active workbook's initialization code. An open window with unsaved code asks first.
pub fn open_initialization(app: &mut SheetApp) {
    let Some(doc) = app.session.active().map(|d| d.uid) else { return };
    let _ = request_open(app, Target::initialization(doc), None, true, false);
}

/// `ui.pythonInitialization {on?, force?}`: opens the editor on the active workbook's initialization
/// code (`on: true`), or closes it (`on: false`); without `on` it opens or closes. As with
/// `ui.pythonEditor` it never asks: with unsaved changes it refuses, unless `force: true` discards
/// them. Replies with the editor's state.
pub fn initialization_command(app: &mut SheetApp, p: &Json) -> Result<Json, String> {
    let force = p.get("force").and_then(Json::as_bool).unwrap_or(false);
    let doc = app.session.active().map(|d| d.uid).ok_or("No workbook is open.")?;
    let on = match p.get("on") {
        None | Some(Json::Null) => !has_initialization(app, doc),
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err("`on` is true or false".into()),
    };
    if on {
        request_open(app, Target::initialization(doc), None, false, force)?;
    } else {
        // Only closes what it opens: a window on a cell stays.
        if has_initialization(app, doc) {
            request_close(app, false, force)?;
        }
    }
    Ok(state(app))
}

/// Closes the editor. With unsaved changes it asks first (`ask`), discards them (`force`), or
/// refuses.
pub fn request_close(app: &mut SheetApp, ask: bool, force: bool) -> Result<(), String> {
    let Some(w) = app.py_window.as_mut() else { return Ok(()) };
    if w.dirty() && !force {
        if ask {
            w.pending = Some(Pending::Close);
            bring_forward(app);
            return Ok(());
        }
        return Err(unsaved_error(w));
    }
    app.py_window = None;
    Ok(())
}

/// Asks about unsaved changes before `p`.
pub fn ask(app: &mut SheetApp, p: Pending) {
    if let Some(w) = app.py_window.as_mut() {
        w.pending = Some(p);
    }
    // The File view covers the sheet and the window: close it so the question is in sight.
    app.backstage = None;
    bring_forward(app);
}

/// Whether `file.close` with `params` would close the active workbook and so lose the editor's
/// unsaved code. With another window on the workbook, `file.close` closes only the active window
/// (unless `document` is set), and the editor stays with its workbook.
pub fn blocks_close(app: &SheetApp, params: &Json) -> bool {
    let active = app.session.active().map(|d| d.uid);
    let Some(w) = app.py_window.as_ref().filter(|w| w.dirty() && Some(w.doc) == active) else { return false };
    params.get("document").and_then(Json::as_bool) == Some(true) || app.session.windows().iter().filter(|x| x.doc_uid == w.doc).count() <= 1
}

/// Whether the editor has unsaved changes.
pub fn is_dirty(app: &SheetApp) -> bool {
    app.py_window.as_ref().is_some_and(PyWindow::dirty)
}

/// Whether `window.close` with `params` would close the window's workbook (its last window) and
/// so lose the editor's unsaved code. A window the params don't name is not closed, so it blocks
/// nothing.
pub fn blocks_window_close(app: &SheetApp, params: &Json) -> bool {
    let Some(w) = app.py_window.as_ref().filter(|w| w.dirty()) else { return false };
    let Ok(window) = gridcraft_engine::Target::from_json(params).and_then(|t| app.session.target_window(t)) else { return false };
    let doc = app.session.window(window).map(|x| x.doc_uid);
    doc == Some(w.doc) && app.session.windows().iter().filter(|x| x.doc_uid == w.doc).count() == 1
}

/// Asks about the editor's unsaved code before quitting, in its workbook's window, which comes
/// forward. Answering it goes on with the quit ([`answer`]). Returns whether it asks.
pub fn ask_before_quit(app: &mut SheetApp) -> bool {
    let Some(doc) = app.py_window.as_ref().filter(|w| w.dirty()).map(|w| w.doc) else { return false };
    if let Some(window) = app.session.doc_window(doc) {
        app.enter_window(window);
    }
    // Its native window is restored and shown too, when it is minimized or the app is hidden.
    app.bring_forward = true;
    ask(app, Pending::Quit);
    true
}

/// The refusal of a programmatic quit while the editor has unsaved changes (programmatic calls
/// never ask).
pub fn quit_refused(app: &SheetApp) -> Option<String> {
    let w = app.py_window.as_ref().filter(|w| w.dirty())?;
    Some(format!(
        "The Python editor has unsaved changes to {}. Apply or discard them first (ui.pythonEditor with on: false and force: true discards them).",
        w.address()
    ))
}

/// Puts the code in its cell through `python.set`, in the window's workbook (active or not). On a
/// refusal the code stays as typed and the command's message shows under it. A cell that no longer
/// holds what it held when the window opened it or last applied, or whose sheet is gone, is
/// refused before `python.set`: the code would replace another cell's content. Returns whether the
/// code went in.
pub fn apply(app: &mut SheetApp) -> bool {
    let Some(w) = app.py_window.as_ref() else { return false };
    // Only into its own sheet: `python.set` finds the sheet by name, and a sheet deleted and
    // another of its name added (anywhere) would take the code.
    if stale(app, w) || (!w.initialization && own_sheet(app, w).is_none()) {
        if let Some(w) = app.py_window.as_mut() {
            w.notice = Some(Notice::Stale);
            w.focus = true;
        }
        return false;
    }
    let (command, params) = if w.initialization {
        ("python.initialization", json!({"code": w.text}))
    } else {
        ("python.set", json!({"sheet": w.sheet_name, "cell": w.cell.a1(), "code": w.text}))
    };
    let t = w.target();
    let Some(i) = doc_index(app, t.doc) else {
        if let Some(w) = app.py_window.as_mut() {
            w.notice = Some(Notice::Refused("The workbook this code belongs to is closed.".into()));
        }
        return false;
    };
    let before = app.session.active_index();
    app.session.set_active(i);
    let r = app.session.run(command, params);
    app.session.set_active(before);
    app.after_engine();
    let (code, _) = cell_code(app, t);
    let now = held(app, t);
    let Some(w) = app.py_window.as_mut() else { return r.is_ok() };
    match r {
        Ok(_) => {
            w.saved = code;
            if let Some(now) = now {
                w.held = now;
            }
            w.notice = None;
            w.count = None;
            true
        }
        Err(e) => {
            w.notice = Some(Notice::Refused(clean_error(&e)));
            w.focus = true;
            false
        }
    }
}

/// Acts on the answer to the window's question.
pub fn answer(app: &mut SheetApp, ctx: &egui::Context, a: Answer) {
    let Some(p) = app.py_window.as_ref().and_then(|w| w.pending.clone()) else { return };
    // Cancel, or an Apply that was refused: the question goes and the code is kept. After a switch
    // to another workbook the window goes back to the code's workbook; with the other one still
    // active the next frame would ask again, and the refusal under the code would never show.
    let refused = a == Answer::Apply && !apply(app);
    if a == Answer::Cancel || refused {
        if let Some(w) = app.py_window.as_mut() {
            w.pending = None;
            w.focus = true;
        }
        if p == Pending::Switched
            && let Some(i) = app.py_window.as_ref().and_then(|w| doc_index(app, w.doc))
        {
            app.session.set_active(i);
        }
        return;
    }
    let doc = app.py_window.as_ref().map(|w| w.doc);
    app.py_window = None;
    match p {
        Pending::Close | Pending::Switched => {}
        // The code there was hidden while the question was open: nothing to show.
        Pending::Open { target, .. } if target_hidden(app, target) => app.message = Some(("GridCraft".into(), hidden_error(target.cell))),
        Pending::Open { target, from_bar } => {
            let text = if from_bar { bar_code(app, target) } else { None };
            open_fresh(app, target, text);
            if from_bar {
                end_bar_edit(app, target);
            }
        }
        Pending::CloseWorkbook(params) => {
            if app.session.active().map(|d| d.uid) == doc {
                app.run_or_alert("file.close", params);
            }
        }
        // Only while the window is still there, on the code's workbook.
        Pending::CloseWindow(params) => {
            let window = gridcraft_engine::Target::from_json(&params).and_then(|t| app.session.target_window(t)).ok();
            if window.and_then(|x| app.session.window(x)).map(|x| x.doc_uid) == doc {
                app.run_or_alert("window.close", params);
            }
        }
        // The quit goes on: each unsaved workbook is asked about next, as Quit does.
        Pending::Quit => app.request_quit(ctx),
    }
}

/// `ui.pythonEditor {on?, cell?, force?}`: opens the editor (`on: true`) on `cell` (`A1` or
/// `Sheet2!B3`; default: the active cell), or closes it (`on: false`); without `on` it opens or
/// closes. It never asks: with unsaved changes it refuses to close, or to open on another cell,
/// unless `force: true` discards them. Replies with the editor's state.
pub fn command(app: &mut SheetApp, p: &Json) -> Result<Json, String> {
    let force = p.get("force").and_then(Json::as_bool).unwrap_or(false);
    let on = match p.get("on") {
        None | Some(Json::Null) => app.py_window.is_none(),
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err("`on` is true or false".into()),
    };
    if !on {
        request_close(app, false, force)?;
        return Ok(state(app));
    }
    let d = app.session.active().ok_or("No workbook is open.")?;
    let (sheet, cell) = match p.get("cell") {
        None | Some(Json::Null) => (d.wb.active_sheet(), d.selection.active),
        Some(Json::String(s)) => {
            let (sheet, a1) = split_sheet(s.trim());
            let sheet = match sheet {
                Some(name) => d.wb.sheet_index(&name).ok_or_else(|| format!("no sheet named {name:?}"))?,
                None => d.wb.active_sheet(),
            };
            (sheet, CellRef::parse(a1).ok_or_else(|| format!("{s:?} is not a cell"))?)
        }
        Some(_) => return Err("`cell` is a cell address, such as A1 or Sheet2!B3".into()),
    };
    let t = target(app, sheet, cell)?;
    let text = bar_code(app, t);
    let carried = text.is_some();
    request_open(app, t, text, false, force)?;
    if carried || app.editor.as_ref().is_some_and(|e| e.python) {
        end_bar_edit(app, t);
    }
    Ok(state(app))
}

/// The editor's state, for `ui.pythonEditor` and `ui.inspect`. While the cell's code is hidden on
/// a protected sheet the code is left out (`text` is null, `hidden` true), even before the next
/// frame closes the window.
pub fn state(app: &SheetApp) -> Json {
    let Some(w) = app.py_window.as_ref() else { return json!({"open": false}) };
    let hidden = code_hidden(app, w);
    let pending = w.pending.as_ref().map(|p| match p {
        Pending::Close => "close",
        Pending::Open { .. } => "open",
        Pending::Switched => "switched",
        Pending::CloseWorkbook(_) => "closeWorkbook",
        Pending::CloseWindow(_) => "closeWindow",
        Pending::Quit => "quit",
    });
    let notice = w.notice.as_ref().map(|n| match n {
        Notice::TooLong if w.initialization => TOO_LONG_INIT.replace("{max}", &MAX_INIT_CHARS.to_string()),
        Notice::TooLong => TOO_LONG.replace("{max}", &MAX_FORMULA_CHARS.to_string()),
        Notice::Refused(m) => m.clone(),
        Notice::Stale if w.initialization => STALE_INIT.to_string(),
        Notice::Stale => STALE.replace("{cell}", &w.address()),
    });
    let place = |text: String| if w.initialization { Json::Null } else { json!(text) };
    json!({
        "open": true,
        "initialization": w.initialization,
        "title": w.title(),
        "sheet": place(w.sheet_name.clone()),
        "cell": place(w.cell.a1()),
        "text": if hidden { Json::Null } else { json!(w.text) },
        "hidden": hidden,
        "dirty": w.dirty(),
        "pending": pending,
        "notice": notice,
        "rect": w.rect.map(|r| [r.left(), r.top(), r.width(), r.height()]),
    })
}

/// Closes the window without asking, as when its workbook closed or its code became hidden: its
/// unsaved code goes to the clipboard, with a notice, so it is not lost silently.
fn close_keeping_code(app: &mut SheetApp, ctx: &egui::Context) {
    if let Some(w) = app.py_window.take()
        && w.dirty()
    {
        ctx.copy_text(w.text);
        app.toast = Some((app.ui.language.tr(COPIED).to_string(), crate::now_ms()));
    }
}

/// Keeps the window with its cell: follows its sheet when sheets move or are renamed, takes in a
/// change of the cell's code when it has no changes of its own, closes when its workbook closes or
/// another is active (asking first when it has changes), and closes when its cell's code becomes
/// hidden on a protected sheet (its unsaved code goes to the clipboard).
fn sync(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(w) = app.py_window.as_ref() else { return };
    if !app.session.documents().iter().any(|d| d.uid == w.doc) {
        // Its workbook closed without asking (the interface asks first, and programmatic calls
        // are refused): the code is not lost silently.
        close_keeping_code(app, ctx);
        return;
    }
    if w.initialization {
        return sync_initialization(app);
    }
    let found = followed(app, w);
    // Its sheet was protected with the cell Hidden: the window stops showing the code at once.
    // Applying is not offered (the sheet is protected), and asking would keep the code on screen.
    if code_hidden(app, w) {
        close_keeping_code(app, ctx);
        return;
    }
    let active = app.session.active().map(|d| d.uid);
    let at = found.as_ref().map(|(sheet, _)| Target { sheet: *sheet, ..w.target() });
    let code = at.map(|t| cell_code(app, t).0);
    let now = at.and_then(|t| held(app, t));
    let Some(w) = app.py_window.as_mut() else { return };
    match (found, code) {
        (Some((sheet, name)), Some(code)) => {
            (w.sheet, w.sheet_name) = (sheet, name);
            if code != w.saved && (!w.dirty() || normalize_code(&w.text) == code) {
                // Nothing of the window's own to keep: it shows the cell's code as it is now.
                //
                // When Apply's reference (`held`) follows the cell: only here, when the window
                // had no unsaved typing and takes in the cell's code (an undo of Apply, a change
                // from elsewhere), and only when the cell is a Python cell or empty. The window then
                // shows exactly what the cell holds, so Apply edits what the user sees, even when
                // another cell moved to the address. With unsaved typing, `held` stays as it was,
                // even when the cell's new code matches the typing: that typing was written for the
                // cell the window opened on, and a cell that moved in (a row insert, a sort) or
                // changed elsewhere must refuse Apply rather than take it. A cell holding a formula,
                // a value or a picture is not shown by the window, so it keeps refusing Apply.
                if !w.dirty() {
                    w.set_text(code.clone());
                    if let Some(now) = now.filter(|h| h.python.is_some() || *h == Held::default()) {
                        w.held = now;
                    }
                }
                w.saved = code;
            }
        }
        // Its sheet was deleted: with nothing to keep, the window goes with it.
        _ if !w.dirty() => {
            app.py_window = None;
            return;
        }
        _ => {}
    }
    follow_active(app, active);
}

/// With no cell to follow, the window takes in the workbook's code when it has no changes of its own.
fn sync_initialization(app: &mut SheetApp) {
    let Some(doc) = app.py_window.as_ref().map(|w| w.doc) else { return };
    let code = init_code(app, doc);
    let active = app.session.active().map(|d| d.uid);
    if let Some(w) = app.py_window.as_mut()
        && code != w.saved
        && (!w.dirty() || normalize_code(&w.text) == code)
    {
        if !w.dirty() {
            w.set_text(code.clone());
        }
        w.saved = code;
    }
    follow_active(app, active);
}

/// Closes when another workbook is active, asking first when it has changes.
fn follow_active(app: &mut SheetApp, active: Option<u64>) {
    let Some(w) = app.py_window.as_mut() else { return };
    if active != Some(w.doc) {
        if !w.dirty() {
            app.py_window = None;
        } else if !matches!(w.pending, Some(Pending::Switched | Pending::Quit)) {
            w.pending = Some(Pending::Switched);
        }
    } else if w.pending == Some(Pending::Switched) {
        w.pending = None;
    }
}

/// Before the sheet takes this frame's keys: Esc answers the window's question with Cancel. (The
/// question's buttons give the focus up on Esc, and the sheet would take the key.)
pub fn before_ui(app: &mut SheetApp, ctx: &egui::Context) {
    // Esc in another native window is that window's key.
    let asking = app.py_window.as_ref().is_some_and(|w| w.pending.is_some()) && drawn_here(app);
    if asking
        && app.dialog.is_none()
        && app.message.is_none()
        && app.backstage.is_none()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        answer(app, ctx, Answer::Cancel);
    }
}

/// Draws the editor, if it is open.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    if !drawn_here(app) {
        // Another native window is being drawn, with its own workbook active: the editor isn't
        // drawn there, and that workbook being active is no switch. It still closes when its
        // workbook is gone (its unsaved code goes to the clipboard).
        let gone = app.py_window.as_ref().is_some_and(|w| home(app, w).is_none());
        if gone {
            close_keeping_code(app, ctx);
        }
        return;
    }
    sync(app, ctx);
    if app.py_window.is_none() || app.backstage.is_some() {
        return;
    }
    let t = Tokens::get(ctx);
    let title = app.py_window.as_ref().map_or_else(String::new, |w| app.ui.language.tr(w.title()).to_string());
    let screen = ctx.content_rect();
    let row_h = ctx.fonts_mut(|f| f.row_height(&code_editor::font()));
    // The contents fit inside the app window with the window's frame and a margin around them.
    let max = (screen.size() - FRAME - vec2(2.0 * MARGIN, 2.0 * MARGIN)).max(vec2(160.0, 120.0));
    let size = vec2(760.0, LINES as f32 * row_h + CHROME_HEIGHT).min(max);
    let min = vec2(360.0, 200.0).min(max);
    // At the right, below the formula bar, so the data the code reads stays in view.
    let bar = formula_bar::BarLayout::get(ctx).field;
    let top = if bar.is_positive() { bar.bottom() + 6.0 } else { screen.top() + 120.0 };
    let top = top.min(screen.bottom() - MARGIN - size.y - FRAME.y).max(screen.top() + MARGIN);
    let left = (screen.right() - MARGIN - size.x - FRAME.x).max(screen.left() + MARGIN);
    let mut open = true;
    let shown = egui::Window::new(title)
        .id(window_id())
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .min_size(min)
        .max_size(max)
        .default_pos(pos2(left, top))
        .constrain_to(screen.shrink(MARGIN))
        .open(&mut open)
        .show(ctx, |ui| contents(app, ui, &t));
    if let Some(w) = app.py_window.as_mut() {
        w.rect = shown.map(|s| s.response.rect);
    }
    if !open {
        let _ = request_close(app, true, false);
    }
}

/// What a key in the code does, besides editing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    /// Ctrl+Enter (Cmd+Enter on macOS).
    Apply,
    /// Esc.
    Close,
}

fn editor_key(ev: &egui::Event) -> Option<Key> {
    match ev {
        egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers: m, .. } if (m.ctrl || m.command || m.mac_cmd) && !m.alt => {
            Some(Key::Apply)
        }
        egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => Some(Key::Close),
        _ => None,
    }
}

fn contents(app: &mut SheetApp, ui: &mut Ui, t: &Tokens) {
    update_checks(app);
    egui::Panel::top("gridcraft.python_editor.header")
        .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 0, top: 0, bottom: 6 }))
        .show(ui, |ui| header(app, ui, t));
    egui::Panel::bottom("gridcraft.python_editor.footer")
        .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: 0, top: 6, bottom: 0 }))
        .show(ui, |ui| footer(app, ui, t));
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(t.input_bg).stroke(egui::Stroke::new(1.0, t.input_border)).corner_radius(4.0).inner_margin(4.0))
        .show(ui, |ui| code(app, ui));
}

/// Brings the checks of the window's code up to date.
fn update_checks(app: &mut SheetApp) {
    let Some(w) = app.py_window.as_ref() else { return };
    let (text, doc, sheet, cell, initialization) = (w.text.clone(), w.doc, followed(app, w).map(|(sheet, _)| sheet), w.cell, w.initialization);
    let mut checks = std::mem::take(&mut app.python.window_checks);
    if initialization {
        crate::python_ui::checks::update_initialization(app, &mut checks, &text, doc);
    } else {
        crate::python_ui::checks::update(app, &mut checks, &text, doc, sheet, cell);
    }
    app.python.window_checks = checks;
}

/// The state of the window's cell, or of the initialization code, for its PY tag.
fn tag_state(app: &SheetApp, w: &PyWindow) -> crate::python_card::TagState {
    if w.initialization {
        return match app.session.python_init_failure() {
            Some(_) if app.session.active().map(|d| d.uid) == Some(w.doc) => crate::python_card::TagState::Error,
            _ => crate::python_card::TagState::Ready,
        };
    }
    let d = app.session.active().filter(|d| d.uid == w.doc);
    let shown = d.and_then(|d| {
        let cell = d.wb.sheet(w.sheet)?.cell(w.cell)?;
        cell.py.as_deref().map(|py| crate::python_card::tag_state(&app.session, w.sheet, w.cell, py, &cell.value))
    });
    shown.unwrap_or(crate::python_card::TagState::NotRunning)
}

/// The PY tag, the cell's address, its return type, the interpreter, whether there are unsaved
/// changes, and Python's state.
fn header(app: &mut SheetApp, ui: &mut Ui, t: &Tokens) {
    let Some(w) = app.py_window.as_ref() else { return };
    let lang = app.ui.language;
    let object = window_cell(app, w).is_some_and(|(_, object)| object);
    let initialization = w.initialization;
    let address = if initialization { lang.tr(INITIALIZATION).to_string() } else { w.address() };
    let dirty = w.dirty();
    let state = tag_state(app, w);
    let interpreter = app.session.python_report().interpreter.as_ref().map(|i| i.path.display().to_string());
    ui.horizontal(|ui| {
        let (tag, _) = ui.allocate_exact_size(vec2(22.0, 15.0), egui::Sense::hover());
        crate::python_card::paint_tag(ui.painter(), tag, t, state);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            crate::python_ui::state_chip(app, ui);
            if dirty {
                ui.label(RichText::new(lang.tr(UNSAVED)).font(theme::ui_font(12.0)).color(t.text_dim));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(address).font(theme::ui_bold(13.0)).color(t.text)).truncate());
                let output = lang.tr(if initialization {
                    INITIALIZATION_TIP
                } else if object {
                    PYTHON_OBJECT
                } else {
                    EXCEL_VALUE
                });
                ui.add(egui::Label::new(RichText::new(output).font(theme::ui_font(12.0)).color(t.text_dim)).truncate());
                if let Some(path) = &interpreter {
                    ui.add(egui::Label::new(RichText::new(path).font(theme::ui_font(11.5)).color(t.text_dim)).truncate());
                }
            });
        });
    });
}

/// The rows of the problems strip that show before it scrolls.
const STRIP_ROWS: usize = 4;

/// This cell's diagnostics and the checks of its code, under the code. A click on one that has a
/// place in the code goes there. Only the rows in view are drawn.
fn problems_strip(app: &mut SheetApp, ui: &mut Ui, t: &Tokens) {
    let count = app.python.window_checks.entries.len();
    if count == 0 {
        return;
    }
    let lang = app.ui.language;
    let row = ui.text_style_height(&egui::TextStyle::Body) + 2.0;
    let mut go = None;
    egui::ScrollArea::vertical()
        .id_salt("gridcraft.python_editor.problems")
        .max_height(row * STRIP_ROWS as f32)
        .auto_shrink([false, true])
        .show_rows(ui, row, count, |ui, rows| {
            for index in rows {
                let Some(e) = app.python.window_checks.entries.get(index) else { continue };
                let message = lang.tr(e.kind.message);
                let said = match e.line {
                    Some(line) => lang.tr("Line {line}: {message}").replace("{line}", &line.to_string()).replace("{message}", message),
                    None => message.to_string(),
                };
                let text = if e.detail.is_empty() { said } else { format!("{said} ({})", e.detail.lines().next().unwrap_or_default()) };
                let color = match e.level {
                    code_editor::MarkLevel::Error => t.danger,
                    code_editor::MarkLevel::Warning | code_editor::MarkLevel::Info => t.text,
                };
                let label = egui::Label::new(RichText::new(text).font(theme::ui_font(12.0)).color(color)).truncate().sense(egui::Sense::click());
                if ui.add(label).clicked() {
                    go = e.char_at;
                }
            }
        });
    ui.add_space(4.0);
    if let (Some(at), Some(w)) = (go, app.py_window.as_mut()) {
        w.caret = Some(at);
        w.focus = true;
    }
}

/// The problems strip with the message under the code, the question about unsaved changes, or the
/// keys, the code's length and the Apply and Close buttons.
fn footer(app: &mut SheetApp, ui: &mut Ui, t: &Tokens) {
    let lang = app.ui.language;
    if app.py_window.as_ref().is_none_or(|w| w.pending.is_none()) {
        problems_strip(app, ui, t);
    }
    let Some(w) = app.py_window.as_ref() else { return };
    if w.pending.is_some() {
        return question(app, ui, t);
    }
    let moved = stale(app, w);
    let Some(w) = app.py_window.as_mut() else { return };
    let initialization = w.initialization;
    let max = if initialization { MAX_INIT_CHARS } else { MAX_FORMULA_CHARS };
    let message = match &w.notice {
        Some(Notice::TooLong) => Some(lang.tr(if initialization { TOO_LONG_INIT } else { TOO_LONG }).replace("{max}", &max.to_string())),
        Some(Notice::Refused(m)) => Some(m.clone()),
        Some(Notice::Stale) if initialization => Some(lang.tr(STALE_INIT).to_string()),
        Some(Notice::Stale) => Some(lang.tr(STALE).replace("{cell}", &w.address())),
        None if w.dirty() && moved && initialization => Some(lang.tr(CHANGED_INIT).to_string()),
        None if w.dirty() && moved => Some(lang.tr(CHANGED).replace("{cell}", &w.address())),
        None => None,
    };
    if let Some(m) = message {
        ui.add(egui::Label::new(RichText::new(m).font(theme::ui_font(12.0)).color(t.danger)).wrap());
        ui.add_space(4.0);
    }
    let count = *w.count.get_or_insert_with(|| if initialization { w.text.chars().count() } else { formula_len(&w.text) });
    let keys = if ui.ctx().os() == egui::os::OperatingSystem::Mac { "Cmd+Enter" } else { "Ctrl+Enter" };
    let hint = lang.tr(HINT).replace("{apply}", keys);
    let mut apply_now = false;
    let mut close_now = false;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.button(lang.tr(CLOSE)).clicked() {
            close_now = true;
        }
        if ui.button(lang.tr(APPLY)).clicked() {
            apply_now = true;
        }
        let color = if count > max { t.danger } else { t.text_dim };
        ui.label(RichText::new(format!("{count} / {max}")).font(theme::ui_font(11.5)).color(color));
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(hint).font(theme::ui_font(11.5)).color(t.text_dim)).truncate());
        });
    });
    if apply_now {
        apply(app);
        // The keyboard goes back to the code.
        if let Some(w) = app.py_window.as_mut() {
            w.focus = true;
        }
    }
    if close_now {
        let _ = request_close(app, true, false);
    }
}

/// "The code for Sheet1!A1 has unsaved changes." with Apply, Discard and Cancel (Esc, see
/// [`before_ui`]).
fn question(app: &mut SheetApp, ui: &mut Ui, t: &Tokens) {
    let lang = app.ui.language;
    let Some(w) = app.py_window.as_ref() else { return };
    let switched = w.pending == Some(Pending::Switched);
    let text = if switched {
        let title = app.session.documents().iter().find(|d| d.uid == w.doc).map(|d| d.display_title()).unwrap_or_default();
        let question = lang.tr(if w.initialization { ASK_INIT_IN } else { ASK_IN });
        question.replace("{cell}", &w.address()).replace("{workbook}", &title)
    } else {
        lang.tr(if w.initialization { ASK_INIT } else { ASK }).replace("{cell}", &w.address())
    };
    ui.add(egui::Label::new(RichText::new(text).font(theme::ui_font(12.5)).color(t.text)).wrap());
    ui.add_space(4.0);
    let mut picked = None;
    let blocked = app.dialog.is_some() || app.message.is_some();
    ui.horizontal_wrapped(|ui| {
        for (label, a) in [(APPLY, Answer::Apply), (DISCARD, Answer::Discard), (CANCEL, Answer::Cancel)] {
            let b = ui.button(lang.tr(label));
            // The keyboard goes to the question while it is asked, not to the sheet behind it.
            if a == Answer::Cancel && !blocked && ui.memory(|m| m.focused().is_none()) {
                b.request_focus();
            }
            if b.clicked() {
                picked = Some(a);
            }
        }
    });
    if let Some(a) = picked {
        let ctx = ui.ctx().clone();
        answer(app, &ctx, a);
    }
}

/// The code, with line numbers, in the code editor.
fn code(app: &mut SheetApp, ui: &mut Ui) {
    let blocked = app.dialog.is_some() || app.message.is_some();
    let name = app.py_window.as_ref().map_or_else(String::new, |w| app.ui.language.tr(w.title()).to_string());
    let Some(w) = app.py_window.as_mut() else { return };
    let initialization = w.initialization;
    let interactive = !blocked && w.pending.is_none();
    let id = editor_id(w);
    let mut caret = None;
    // A window's first frame only measures it (its widgets aren't there for the focus), and a text
    // field gives the focus up in a frame with a click elsewhere (the button that asked for it):
    // the code takes the keyboard in the next frame without either.
    let clicking = ui.input(|i| i.pointer.any_pressed() || i.pointer.any_released());
    if interactive && !ui.is_sizing_pass() && !clicking {
        if std::mem::take(&mut w.focus) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        caret = w.caret.take();
    }
    let stop = |ev: &egui::Event| editor_key(ev).is_some();
    let accept = move |new: &str, old: &str| if initialization { accept_init_code(new, old) } else { accept_code(new, old) };
    let size = ui.available_size();
    let checks = std::mem::take(&mut app.python.window_checks);
    let options = FieldOptions {
        width: size.x,
        max_height: size.y,
        fill: true,
        gutter: true,
        interactive,
        stop: &stop,
        accept: Some(&accept),
        keep_escape: true,
        caret,
        name: &name,
        marks: &checks.marks,
        ..FieldOptions::new(Lang::Python)
    };
    let out = code_editor::code_field(ui, id, &mut w.text, &options);
    app.python.window_checks = checks;
    // The marks catch up with this edit in the next frame.
    if out.response.changed() {
        ui.ctx().request_repaint();
    }
    let Some(w) = app.py_window.as_mut() else { return };
    w.gutter = out.gutter;
    if out.refused {
        w.notice = Some(Notice::TooLong);
    } else if out.response.changed() {
        w.notice = None;
    }
    if out.refused || out.response.changed() {
        w.count = None;
    }
    match out.stop.as_ref().and_then(editor_key) {
        Some(Key::Apply) => {
            apply(app);
        }
        Some(Key::Close) => {
            let _ = request_close(app, true, false);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two workbooks, the second active, and a formula-bar Python draft of the first one's A1.
    fn two_books_with_a_draft_in_the_first() -> (SheetApp, Target) {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        let uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
        assert_eq!(app.session.active().map(|d| d.uid), uids.get(1).copied(), "the second workbook is active");
        let cell = CellRef::new(0, 0);
        app.editor = Some(crate::editor::EditState::python(uids[0], 0, cell, "x = 1".into()));
        (app, Target { doc: uids[1], sheet: 0, cell, initialization: false })
    }

    #[test]
    fn the_formula_bar_hands_over_only_a_draft_of_the_targets_workbook() {
        let (mut app, t) = two_books_with_a_draft_in_the_first();
        // The editor opens on the active workbook's A1: the other workbook's draft at the same
        // sheet and cell is neither taken as its code nor cancelled.
        assert_eq!(bar_code(&app, t), None);
        end_bar_edit(&mut app, t);
        assert!(app.editor.as_ref().is_some_and(|e| e.text == "x = 1"), "the other workbook's draft stays");
        // A draft of the target's own workbook is handed over.
        if let Some(ed) = app.editor.as_mut() {
            ed.doc = t.doc;
        }
        assert_eq!(bar_code(&app, t).map(|(code, _)| code), Some("x = 1".to_string()));
        end_bar_edit(&mut app, t);
        assert!(app.editor.is_none());
    }

    #[test]
    fn the_formula_bar_hands_over_only_a_draft_of_the_active_window() {
        let (mut app, t) = two_books_with_a_draft_in_the_first();
        // A second window on the active workbook; the draft belongs to its first window.
        let first = app.session.active_window();
        app.session.execute("view.newWindow", serde_json::json!({})).unwrap();
        assert_ne!(app.session.active_window(), first);
        if let Some(ed) = app.editor.as_mut() {
            ed.doc = t.doc;
            ed.window = first;
        }
        assert_eq!(bar_code(&app, t), None);
        end_bar_edit(&mut app, t);
        assert!(app.editor.is_some(), "the other window's draft stays, to be committed there");
        // The same draft in the active window is handed over.
        if let Some(ed) = app.editor.as_mut() {
            ed.window = app.session.active_window();
        }
        assert!(bar_code(&app, t).is_some());
        end_bar_edit(&mut app, t);
        assert!(app.editor.is_none());
    }

    #[test]
    fn a_quit_held_by_unsaved_code_restores_the_editors_window() {
        let (mut app, _) = two_books_with_a_draft_in_the_first();
        app.editor = None;
        command(&mut app, &serde_json::json!({"on": true})).unwrap();
        if let Some(w) = app.py_window.as_mut() {
            w.set_text("y = 2".into());
        }
        assert!(is_dirty(&app));
        assert!(ask_before_quit(&mut app));
        assert_eq!(app.py_window.as_ref().and_then(|w| w.pending.clone()), Some(Pending::Quit));
        assert!(app.bring_forward, "a minimized window or a hidden app is restored, not only focused");
    }

    #[test]
    fn the_initialization_limit_is_the_one_the_command_checks_and_long_code_can_still_shrink() {
        let at = "x".repeat(MAX_INIT_CHARS);
        assert!(accept_init_code(&at, ""));
        assert!(!accept_init_code(&format!("{at}y"), &at), "one more character is refused");
        // Characters, not bytes.
        assert!(accept_init_code(&"é".repeat(MAX_INIT_CHARS), ""));
        assert!(!accept_init_code(&"é".repeat(MAX_INIT_CHARS + 1), ""));
        // Code that came in past the limit can be shortened, not lengthened.
        let over = format!("{at}y");
        assert!(accept_init_code(&at, &over));
        assert!(accept_init_code(&over, &over), "no change");
        assert!(!accept_init_code(&format!("{at}yz"), &over));
        // A huge paste is refused without counting it all.
        assert!(!accept_init_code(&"z".repeat(50_000_000), ""));
    }

    #[test]
    fn the_limit_is_the_one_python_set_checks_and_long_code_can_still_shrink() {
        // The longest run of one letter that fits: its formula is the limit's length.
        let n = (0..=MAX_FORMULA_CHARS).rev().find(|n| fits(&"x".repeat(*n))).unwrap_or(0);
        let at = "x".repeat(n);
        assert!(n > 8_000 && formula_len(&at) <= MAX_FORMULA_CHARS);
        assert!(!fits(&format!("{at}y")));
        assert!(accept_code(&at, ""));
        assert!(!accept_code(&format!("{at}y"), &at), "one more character is refused");
        // A quote counts twice in the formula.
        let quotes = "\"".repeat(MAX_FORMULA_CHARS / 2);
        assert!(!fits(&quotes));
        // Code that came in past the limit can be shortened, not lengthened.
        let over = format!("{at}yy");
        assert!(accept_code(&format!("{at}y"), &over));
        assert!(!accept_code(&format!("{at}yyy"), &over));
        // A huge paste is refused without building its formula.
        assert!(!accept_code(&"z".repeat(50_000_000), ""));
    }
}
