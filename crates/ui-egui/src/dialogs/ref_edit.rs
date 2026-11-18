//! Reference boxes (Excel's RefEdit): text fields that take a range by selection on the grid, with
//! a button that collapses the dialog to the box while the person selects.

use gridcraft_engine::DocState;
use gridcraft_engine::core::split_areas;
use gridcraft_engine::model::{Sheet, SheetUid, Workbook};

use super::data::abs;
use super::{Dialog, Field, Icon, Json, SheetApp, json, theme, vec2};

/// What a dialog's reference boxes are doing: which one grid selections fill, and whether the
/// dialog is collapsed to one of them.
#[derive(Clone, Debug, Default)]
pub(super) struct RefBoxes {
    /// The reference box that grid selections are written into.
    pub(super) target: Option<String>,
    /// The selection last written into the target (or present when picking began); only a
    /// different selection replaces what the box holds.
    seen: String,
    /// The sheet shown when `seen` was taken: changing sheets only refreshes `seen`, since the
    /// other sheet's own selection is not a pick.
    seen_sheet: String,
    /// The dialog's own sheet, from the moment the dialog opens until it closes, and the
    /// selection to put back when it runs its command or closes.
    home: Option<Home>,
    /// The target took a selection from the grid and takes the keyboard back once the pointer
    /// is released, so typing still edits the box.
    refocus: bool,
    /// A press on the grid selected this frame ([`note_selecting_press`]): the target takes the
    /// selection even when it is the one it last saw, since clicking the cell already selected
    /// is a pick too. Cleared when a box starts picking and once the frame's boxes are drawn, so
    /// only a press made while the box picks counts.
    pressed: bool,
    /// The target box was drawn this frame. A box the dialog stops showing (another view of the
    /// dialog, a hidden area, a constraint operator that takes no value) is no longer picked
    /// into, so a selection made meanwhile does not land in it once it is shown again (see
    /// [`window_shown`]).
    target_drawn: bool,
    /// The reference box the dialog is collapsed to.
    pub(super) collapsed: Option<String>,
    /// Where the expanded dialog was last drawn: the collapsed dialog keeps its top-left corner,
    /// and the expanded one comes back where it was.
    rect: Option<egui::Rect>,
    /// The dialog was drawn collapsed last frame.
    was_collapsed: bool,
    /// A text field other than the collapsed box (the Name Box, say) owned the keyboard when
    /// the last frame ended: its keys are its own, not the collapsed picker's (see
    /// [`place_window`]). Kept from the frame before because egui drops the focus for Escape
    /// before anything is drawn, so the frame of the Escape no longer says who had it.
    other_field: bool,
}

/// The dialog's own sheet: the sheet shown when the dialog opened ([`record_home`]), kept for as
/// long as the dialog is open (a failed OK leaves it open, and a later OK still needs to know
/// whether that sheet is there), and the selection on it to put back once picking ends.
#[derive(Clone, Debug)]
struct Home {
    /// The sheet's identity while the app runs ([`SheetUid`]): fresh for every sheet added,
    /// inserted, copied or read, and kept through renames, moves, undo and redo. The sheet is
    /// found by it whatever happened meanwhile ([`home_sheet`]): a sheet given its name or a
    /// copy of it is never taken for it, and an undo that brings it back makes it the dialog's
    /// again.
    uid: SheetUid,
    /// The ranges and active cell on the dialog's sheet from before picking began, taken when a
    /// box starts picking (from the sheet's kept selection when another sheet is shown then), and
    /// put back once when the dialog runs its command or closes.
    selection: Option<(String, String)>,
}

/// Whether the dialog has reference boxes: Goal Seek, Solver, Data Analysis, and any form with a
/// [`Field::Ref`].
fn has_reference_boxes(d: &Dialog) -> bool {
    matches!(d.name.as_str(), "goalSeek" | "solver" | "analysisToolPak") || d.fields.iter().any(|f| matches!(f, Field::Ref { .. }))
}

/// Records the sheet shown as the dialog's own when a dialog with reference boxes opens, from the
/// ribbon or the control channel alike. A sheet shown later, before any box picks, is not the
/// dialog's: references without a sheet name still mean the sheet the dialog was opened on,
/// whose defaults (the active cell, the selection) the boxes start with.
pub(super) fn record_home(app: &SheetApp, d: &mut Dialog) {
    if !has_reference_boxes(d) {
        return;
    }
    if let Some(sheet) = app.session.active().and_then(|doc| doc.wb.active()) {
        d.refs.home = Some(Home { uid: sheet.uid, selection: None });
    }
}

/// The selection on the dialog's own sheet as (ranges, active cell): the one shown while that
/// sheet is active, else the one the sheet keeps for when it is shown again. `None` once the
/// sheet is deleted.
fn home_selection(doc: &DocState, home: &Home) -> Option<(String, String)> {
    let i = doc.wb.sheets.iter().position(|s| s.uid.get() == home.uid.get())?;
    let sel = if i == doc.wb.active_sheet() { &doc.selection } else { doc.sheet_selections.get(i)? };
    let ranges: Vec<String> = sel.ranges.iter().map(|r| r.a1()).collect();
    Some((ranges.join(","), sel.active.a1()))
}

/// The grid's current area (the one being selected) as a reference for a dialog of `d`'s: with
/// the sheet name when it is on another sheet than the dialog's own, as a reference box writes it.
pub(super) fn current_area_ref(app: &SheetApp, d: &Dialog) -> String {
    let Some(doc) = app.session.active() else { return String::new() };
    let area = doc.selection.current().a1();
    match (doc.wb.active(), d.refs.home.as_ref()) {
        (Some(sh), Some(home)) if home.uid.get() != sh.uid.get() => format!("{}!{area}", gridcraft_engine::formula::quote_sheet(&sh.name)),
        _ => area,
    }
}

/// The dialog's own sheet in `wb`, or `None` once it was deleted.
fn home_sheet<'a>(wb: &'a Workbook, home: &Home) -> Option<&'a Sheet> {
    wb.sheets.iter().map(|s| &**s).find(|s| s.uid.get() == home.uid.get())
}

/// Whether the dialog's own sheet is protected, for a dialog with reference boxes and while that
/// sheet exists: the sheet [`end_pick`] shows again. `None` otherwise.
pub(super) fn home_sheet_protected(app: &SheetApp, d: &Dialog) -> Option<bool> {
    let home = d.refs.home.as_ref()?;
    app.session.active().and_then(|doc| home_sheet(&doc.wb, home)).map(Sheet::is_protected)
}

/// The message OK shows once the dialog's own sheet is gone.
pub(crate) const SHEET_DELETED: &str =
    "The sheet this dialog was opened on was deleted. Cancel, then open the dialog again on the sheet it should use.";

/// Whether the sheet a dialog with reference boxes was opened on has been deleted while it was
/// open: its references without a sheet name have nothing to refer to, so OK is refused with
/// [`SHEET_DELETED`].
pub(super) fn home_sheet_deleted(app: &SheetApp, d: &mut Dialog) -> bool {
    d.refs.home.as_ref().is_some_and(|h| app.session.active().is_none_or(|doc| home_sheet(&doc.wb, h).is_none()))
}

/// How a reference box is named for screen readers.
#[derive(Clone, Copy)]
pub(super) enum RefLabel<'a> {
    /// By the label drawn beside it.
    By(egui::Id),
    /// By an English name, translated (a box without a label of its own).
    Named(&'a str),
}

/// The egui id of the reference box `key`, the same wherever the box is drawn.
fn ref_edit_id(key: &str) -> egui::Id {
    egui::Id::unique("gridcraft.ref_edit").with(key)
}

/// Whether the reference box `key` of dialog `dialog` takes several areas joined by commas.
/// Solver's variable cells do; every other box takes one cell or range, which its command parses
/// as a single reference.
fn takes_several_areas(dialog: &str, key: &str) -> bool {
    dialog == "solver" && key == "variables"
}

/// The grid selection as a reference, with the sheet name when the selection is on another sheet
/// than the one the dialog was opened on: every area joined by commas when `several`, otherwise
/// only the area being selected (the last one).
fn selection_ref(app: &SheetApp, home: Option<&Home>, several: bool) -> String {
    let Some(doc) = app.session.active() else { return String::new() };
    let current = [doc.selection.current()];
    let areas = if several { doc.selection.ranges.as_slice() } else { &current[..] }.iter().map(abs);
    match doc.wb.active() {
        // Once the dialog's own sheet is deleted, every reference carries its sheet's name.
        Some(sh) if home.is_some_and(|h| h.uid.get() != sh.uid.get()) => {
            let sheet = gridcraft_engine::formula::quote_sheet(&sh.name);
            areas.map(|a| format!("{sheet}!{a}")).collect::<Vec<_>>().join(",")
        }
        _ => areas.collect::<Vec<_>>().join(","),
    }
}

/// Places the dialog's window before it is drawn, and returns the reference box the dialog is
/// collapsed to. Escape or Enter expands a collapsed dialog again, rather than closing it or
/// pressing OK, and the arrow keys select on the grid, unless another text field (the Name Box)
/// owns the keyboard: its keys stay its own, as they were before the dialog collapsed.
/// Collapsed, the dialog shrinks to the box where its top-left corner was, leaving the sheet
/// free for selecting; expanded again, it comes back where it was.
pub(super) fn place_window<'a>(
    app: &mut SheetApp,
    ctx: &egui::Context,
    d: &mut Dialog,
    window: egui::Window<'a>,
    window_id: egui::Id,
) -> (egui::Window<'a>, Option<String>) {
    if let Some(key) = d.refs.collapsed.as_deref() {
        let other_field = d.refs.other_field || other_field_focused(ctx, key);
        // Escape and Enter are used up either way: left in the frame's input, the dialog would
        // read them as its own and close or confirm. When another field owned them, that field
        // (drawn before the dialogs) has read them already, and nothing more happens here.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) | i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            if !other_field {
                d.refs.collapsed = None;
            }
        } else if !other_field {
            arrows_move_selection(app, ctx);
        }
    }
    let collapsed = d.refs.collapsed.clone();
    if collapsed.is_none()
        && d.refs.was_collapsed
        && let Some(rect) = d.refs.rect
        && let Some(mut state) = egui::AreaState::load(ctx, window_id)
    {
        // The window's stored position wins over `current_pos` for a window moved by its title
        // bar, so the expanded position is written into its state.
        state.pivot = egui::Align2::CENTER_CENTER;
        state.pivot_pos = Some(rect.center());
        ctx.memory_mut(|m| m.areas_mut().set_state(egui::LayerId::new(egui::Order::Middle, window_id), state));
    }
    let window = match (&collapsed, d.refs.rect) {
        (Some(_), Some(rect)) => window.pivot(egui::Align2::LEFT_TOP).fixed_pos(rect.min),
        _ => window,
    };
    (window, collapsed)
}

/// Whether a text field other than the reference box `key` has the keyboard.
fn other_field_focused(ctx: &egui::Context, key: &str) -> bool {
    ctx.text_edit_focused() && ctx.memory(|m| m.focused()) != Some(ref_edit_id(key))
}

/// While the dialog is collapsed, the arrow keys move the selection on the grid, Shift+arrow
/// extends it and Ctrl+arrow (Cmd on a Mac) jumps to the edge of the data, as they do with no
/// dialog open: the box follows the selection as it does for the mouse. The keys are taken out
/// of the frame's input before the box's text field reads them, so they don't move its caret
/// instead; expanded, they edit the box.
fn arrows_move_selection(app: &mut SheetApp, ctx: &egui::Context) {
    use egui::Key::{ArrowDown, ArrowLeft, ArrowRight, ArrowUp};
    let mut moves = Vec::new();
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::Key { key: key @ (ArrowUp | ArrowDown | ArrowLeft | ArrowRight), pressed: true, modifiers, .. } => {
                moves.push((*key, *modifiers));
                false
            }
            _ => true,
        });
    });
    if moves.is_empty() {
        return;
    }
    // Arrows follow the screen: on a right-to-left sheet Left goes to the next column.
    let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|sh| sh.right_to_left);
    for (key, m) in moves {
        let (dr, dc) = match key {
            ArrowUp => (-1, 0),
            ArrowDown => (1, 0),
            ArrowLeft => (0, if rtl { 1 } else { -1 }),
            _ => (0, if rtl { -1 } else { 1 }),
        };
        let from = app.session.active().map(|d| if m.shift { d.selection.moving_end() } else { d.selection.active });
        let _ = app.run("selection.move", json!({"dr": dr, "dc": dc, "extend": m.shift, "jump": m.command}));
        app.grid.ensure_visible = true;
        app.grid.reveal_moving_end = m.shift;
        app.grid.reveal_from = from;
    }
}

/// After the dialog's window is drawn: remembers where the expanded window is, and ends picking
/// into a reference box when the box was not drawn (the dialog stopped showing it) or when
/// another text field of the dialog takes the keyboard. A collapsed dialog shows no other field,
/// so its box keeps picking whatever else is clicked (the Name Box, the formula bar, the
/// ribbon): only Escape, Enter or the expand button end that state.
pub(super) fn window_shown(ctx: &egui::Context, d: &mut Dialog, collapsed: bool, rect: Option<egui::Rect>) {
    if !collapsed {
        d.refs.rect = rect;
    }
    d.refs.was_collapsed = collapsed;
    // A press the boxes did not take this frame (none was picking) is not kept for a later one.
    d.refs.pressed = false;
    d.refs.other_field = d.refs.collapsed.as_deref().is_some_and(|key| other_field_focused(ctx, key));
    // The box compares the grid selection with the one it last saw only while it is drawn: a
    // hidden box would take a selection made meanwhile when it comes back.
    if !std::mem::take(&mut d.refs.target_drawn) {
        d.refs.target = None;
    }
    if !collapsed && ctx.text_edit_focused() && d.refs.target.as_deref().is_none_or(|key| !ctx.memory(|m| m.has_focus(ref_edit_id(key)))) {
        d.refs.target = None;
    }
}

/// Records a press on the grid that selects (see [`RefBoxes::pressed`]).
pub(super) fn note_selecting_press(d: &mut Dialog) {
    d.refs.pressed = true;
}

/// Whether a reference box is being picked into (see [`Dialog::ref_box_picks`]).
pub(super) fn picks(d: &Dialog) -> bool {
    d.refs.target.is_some()
}

/// Makes the reference box `key` the one grid selections fill. The first pick (and the first
/// after an OK that failed) remembers the selection on the dialog's own sheet, which is put back
/// when the dialog runs its command or closes. A dialog opened without [`record_home`] takes the
/// sheet shown at its first pick as its own.
pub(super) fn begin_pick(app: &SheetApp, d: &mut Dialog, key: &str) {
    if let Some(doc) = app.session.active()
        && let Some(sheet) = doc.wb.active()
    {
        let home = d.refs.home.get_or_insert(Home { uid: sheet.uid, selection: None });
        if home.selection.is_none() {
            home.selection = home_selection(doc, home);
        }
    }
    d.refs.target = Some(key.to_string());
    d.refs.refocus = false;
    // Focusing the box keeps its text: only a press after this one picks.
    d.refs.pressed = false;
    d.refs.seen = selection_ref(app, d.refs.home.as_ref(), takes_several_areas(&d.name, key));
    d.refs.seen_sheet = active_sheet_name(app);
}

/// The name of the sheet shown in the active workbook.
fn active_sheet_name(app: &SheetApp) -> String {
    app.session.active().and_then(|doc| doc.wb.active()).map(|sh| sh.name.clone()).unwrap_or_default()
}

/// Stops picking and goes back to the dialog's own sheet, putting back the selection from before
/// picking began (once), unless that sheet was deleted: the selection is then left where it is.
/// The sheet itself is kept until the dialog closes.
///
/// The commands go through [`SheetApp::run`], as the ribbon's do: showing the sheet first saves
/// a text-box draft left open on the sheet picked on, and the UI's after-command pass runs.
///
/// Fails when the sheet could not be shown again (the draft could not be saved): the person
/// stays with their text, the selection waits for the next time the dialog lets go, and a
/// dialog about to run its command must not, since references without a sheet name would be
/// read against the sheet still shown.
pub(super) fn end_pick(app: &mut SheetApp, d: &mut Dialog) -> Result<(), String> {
    // Custom Sort puts back the selection it changed whenever it lets go of the grid.
    super::sort_dialog::restore_selection(app, d);
    d.refs.target = None;
    d.refs.collapsed = None;
    let Some(home) = d.refs.home.as_mut() else { return Ok(()) };
    let Some(sheet) = app.session.active().and_then(|doc| home_sheet(&doc.wb, home)).map(|s| s.name.clone()) else { return Ok(()) };
    // After an OK that failed, the selection is already back and only the sheet is shown again,
    // so references without a sheet name mean it.
    if !active_sheet_name(app).eq_ignore_ascii_case(&sheet) {
        app.run("sheet.activate", json!({"sheet": sheet}))?;
    }
    if let Some((range, active)) = home.selection.take() {
        let _ = app.run("selection.set", json!({"range": range, "active": active}));
    }
    Ok(())
}

/// A reference box (RefEdit): a text field that the grid selection fills while it is the box
/// being picked into, and a button that collapses the dialog to the box or expands it again.
pub(super) fn ref_edit(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog, key: &str, label: RefLabel<'_>, width: f32) {
    let lang = crate::i18n::current(ui.ctx());
    let mut s = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
    let collapsed = d.refs.collapsed.as_deref() == Some(key);
    ui.horizontal(|ui| {
        let id = ref_edit_id(key);
        let r = ui.add(egui::TextEdit::singleline(&mut s).id(id).desired_width(width - 30.0));
        let r = match label {
            RefLabel::By(label) => r.labelled_by(label),
            RefLabel::Named(name) => r.accessible_name(lang.tr(name)),
        };
        // F4 cycles the anchors of the reference in the box, as in Excel's reference boxes.
        if r.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F4)) {
            s = cycle_anchors(&s);
        }
        if r.gained_focus() {
            begin_pick(app, d, key);
        } else if d.refs.target.as_deref() == Some(key) {
            let sel = selection_ref(app, d.refs.home.as_ref(), takes_several_areas(&d.name, key));
            let sheet = active_sheet_name(app);
            let pressed = std::mem::take(&mut d.refs.pressed);
            if sheet != d.refs.seen_sheet && !pressed {
                // Another sheet was shown: its selection is where that sheet was left, not a pick.
                d.refs.seen = sel;
                d.refs.seen_sheet = sheet;
            } else if pressed || sel != d.refs.seen {
                // The person selected on the grid, even the cell already selected: the selection
                // becomes the reference.
                s = sel.clone();
                d.refs.seen = sel;
                d.refs.seen_sheet = sheet;
                d.refs.refocus = true;
            }
            if d.refs.refocus && !ui.input(|i| i.pointer.any_down()) {
                d.refs.refocus = false;
                r.request_focus();
            }
        }
        let t = theme::Tokens::get(ui.ctx());
        let (icon, tip) = if collapsed { (Icon::Expand, "Expand dialog") } else { (Icon::Collapse, "Collapse dialog") };
        let b = crate::widgets::icon_button(ui, icon, t.text_dim, tip, vec2(22.0, 20.0));
        let name = lang.tr(tip).to_string();
        b.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        if b.clicked() {
            if collapsed {
                d.refs.collapsed = None;
            } else {
                d.collapse_to(app, key);
            }
            ui.memory_mut(|m| m.request_focus(id));
        }
    });
    if d.refs.target.as_deref() == Some(key) {
        d.refs.target_drawn = true;
    }
    d.values.insert(key.into(), json!(s));
}

/// The anchors of one part of a reference (`$A$1`, `A`, `$3`): its column letters and row digits,
/// and whether each carries a `$`.
struct Anchors {
    letters: String,
    digits: String,
    col_abs: bool,
    row_abs: bool,
}

/// Reads `part` as a cell (`A1`), a column (`A`) or a row (`3`) with its anchors: at most three
/// letters and seven digits, nothing else. A `$` alone before the digits of a row anchors the row.
fn anchors(part: &str) -> Option<Anchors> {
    let (col_abs, rest) = match part.strip_prefix('$') {
        Some(rest) => (true, rest),
        None => (false, part),
    };
    let letters: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
    let rest = rest.get(letters.len()..)?;
    let (row_abs, rest) = match rest.strip_prefix('$') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if !rest.get(digits.len()..)?.is_empty() || letters.len() > 3 || digits.len() > 7 || (letters.is_empty() && digits.is_empty()) {
        return None;
    }
    if letters.is_empty() {
        // `$3`: the one `$` belongs to the row; `$$3` is not a reference.
        if col_abs && row_abs {
            return None;
        }
        return Some(Anchors { letters, digits, col_abs: false, row_abs: col_abs || row_abs });
    }
    Some(Anchors { letters, digits, col_abs, row_abs })
}

/// Excel's F4 in a reference box: `A1` → `$A$1` → `A$1` → `$A1` → `A1`, every part of every
/// area moving together from the first part's anchors (`A1:B2,D4` → `$A$1:$B$2,$D$4`), sheet
/// names kept; whole columns and rows have one anchor each (`A:A` → `$A:$A` → `A:A`). Text that
/// isn't a reference comes back as it is.
pub(crate) fn cycle_anchors(reference: &str) -> String {
    let mut areas: Vec<(&str, Vec<Anchors>)> = Vec::new();
    for area in split_areas(reference) {
        let area = area.trim();
        let (prefix, body) = match area.rsplit_once('!') {
            Some((sheet, body)) => (area.get(..sheet.len() + 1).unwrap_or_default(), body),
            None => ("", area),
        };
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() > 2 {
            return reference.to_string();
        }
        let Some(parts) = parts.iter().map(|p| anchors(p)).collect::<Option<Vec<_>>>() else { return reference.to_string() };
        areas.push((prefix, parts));
    }
    let Some(first) = areas.first().and_then(|(_, parts)| parts.first()) else { return reference.to_string() };
    let (col_abs, row_abs) = if first.letters.is_empty() {
        (!first.row_abs, !first.row_abs)
    } else if first.digits.is_empty() {
        (!first.col_abs, !first.col_abs)
    } else {
        match (first.col_abs, first.row_abs) {
            (false, false) => (true, true),
            (true, true) => (false, true),
            (false, true) => (true, false),
            (true, false) => (false, false),
        }
    };
    let anchor = |on: bool, text: &str| if on && !text.is_empty() { format!("${text}") } else { text.to_string() };
    areas
        .iter()
        .map(|(prefix, parts)| {
            let body: Vec<String> = parts.iter().map(|a| format!("{}{}", anchor(col_abs, &a.letters), anchor(row_abs, &a.digits))).collect();
            format!("{prefix}{}", body.join(":"))
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::cycle_anchors;

    #[test]
    fn f4_keeps_a_comma_inside_a_quoted_sheet_name() {
        assert_eq!(cycle_anchors("'Sales, West'!$A$1"), "'Sales, West'!A$1");
        assert_eq!(cycle_anchors("'O''Brien, Jr'!A1:B2,D4"), "'O''Brien, Jr'!$A$1:$B$2,$D$4");
        assert_eq!(cycle_anchors("B2:B4,D1"), "$B$2:$B$4,$D$1");
    }

    #[test]
    fn f4_cycles_cells_ranges_columns_and_rows() {
        assert_eq!(cycle_anchors("A1"), "$A$1");
        assert_eq!(cycle_anchors("$A$1"), "A$1");
        assert_eq!(cycle_anchors("A$1"), "$A1");
        assert_eq!(cycle_anchors("$A1"), "A1");
        assert_eq!(cycle_anchors("A1:B2"), "$A$1:$B$2");
        assert_eq!(cycle_anchors("$A$1:$B$2"), "A$1:B$2");
        // Every area follows the first part, and sheet names stay as typed.
        assert_eq!(cycle_anchors("'O''Brien Rates'!A2:A4, D4"), "'O''Brien Rates'!$A$2:$A$4,$D$4");
        assert_eq!(cycle_anchors("Sheet1!$A$1:B2"), "Sheet1!A$1:B$2");
        assert_eq!(cycle_anchors("A:A"), "$A:$A");
        assert_eq!(cycle_anchors("$A:$A"), "A:A");
        assert_eq!(cycle_anchors("1:3"), "$1:$3");
        assert_eq!(cycle_anchors("$1:$3"), "1:3");
    }

    #[test]
    fn f4_leaves_text_that_is_not_a_reference() {
        for text in ["", "hello", "A1:hello", "ABCD1", "A12345678", "$$1", "$$A1", "A1:B2:C3", "Sheet1!", "A1,", "=A1"] {
            assert_eq!(cycle_anchors(text), text);
        }
    }
}
