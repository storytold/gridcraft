//! Reference boxes (Excel's RefEdit): text fields that take a range by selection on the grid, with
//! a button that collapses the dialog to the box while the person selects.

use super::data::abs;
use super::{Dialog, Icon, Json, SheetApp, json, theme, vec2};

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
    /// The dialog's own sheet, from the first pick until the dialog closes, and the selection to
    /// put back when it runs its command or closes.
    home: Option<Home>,
    /// The target took a selection from the grid and takes the keyboard back once the pointer
    /// is released, so typing still edits the box.
    refocus: bool,
    /// The reference box the dialog is collapsed to.
    pub(super) collapsed: Option<String>,
    /// Where the expanded dialog was last drawn: the collapsed dialog keeps its top-left corner,
    /// and the expanded one comes back where it was.
    rect: Option<egui::Rect>,
    /// The dialog was drawn collapsed last frame.
    was_collapsed: bool,
}

/// Where picking began: the dialog's own sheet, kept for as long as the dialog is open (a failed
/// OK leaves it open, and a later OK still needs to know whether that sheet is there), and the
/// selection to put back once.
#[derive(Clone, Debug)]
struct Home {
    /// The sheet's name, or `None` once the sheet is deleted. Followed by name when other sheets
    /// are inserted, moved or deleted (indexes change), and by position when the sheet itself is
    /// renamed.
    sheet: Option<String>,
    /// The workbook's sheet names, in order, when last seen: a rename is the one change that
    /// keeps the list's length and replaces only the home sheet's entry.
    names: Vec<String>,
    /// The ranges and active cell from before picking began on the dialog's sheet, put back once
    /// when the dialog runs its command or closes.
    selection: Option<(String, String)>,
}

/// The message OK shows once the dialog's own sheet is gone.
pub(super) const SHEET_DELETED: &str =
    "The sheet this dialog was opened on was deleted. Cancel, then open the dialog again on the sheet it should use.";

/// The workbook's sheet names, in order.
fn sheet_names(app: &SheetApp) -> Vec<String> {
    app.session.active().map(|doc| doc.wb.sheets.iter().map(|s| s.name.clone()).collect()).unwrap_or_default()
}

/// Keeps the dialog's own sheet across changes made while it is open. When its name is still
/// there, it is still the sheet (others were inserted, moved or deleted). When it is gone and the
/// only difference from last frame is that one entry, it was renamed. Any other way of losing the
/// name (the sheet count changed, or other names changed too) means it was deleted, for good: a
/// sheet inserted later is never taken for it.
fn follow_home(app: &SheetApp, refs: &mut RefBoxes) {
    let Some(home) = refs.home.as_mut() else { return };
    let names = sheet_names(app);
    if let Some(sheet) = home.sheet.as_mut()
        && !names.iter().any(|n| n.eq_ignore_ascii_case(sheet))
    {
        home.sheet = renamed_to(&names, &home.names, sheet);
    }
    home.names = names;
}

/// The new name of the sheet `own` when `names` differs from `before` only by `own`'s entry.
fn renamed_to(names: &[String], before: &[String], own: &str) -> Option<String> {
    if names.len() != before.len() {
        return None;
    }
    let mut changed = names.iter().zip(before).filter(|(now, then)| now != then);
    let (now, then) = changed.next()?;
    (changed.next().is_none() && then.eq_ignore_ascii_case(own)).then(|| now.clone())
}

/// Whether the sheet the dialog was opened on has been deleted while it picked: its references
/// without a sheet name have nothing to refer to, so OK is refused with [`SHEET_DELETED`].
pub(super) fn home_sheet_deleted(app: &SheetApp, d: &mut Dialog) -> bool {
    follow_home(app, &mut d.refs);
    d.refs.home.as_ref().is_some_and(|h| h.sheet.is_none())
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
        Some(sh) if home.is_some_and(|h| h.sheet.as_ref().is_none_or(|own| !own.eq_ignore_ascii_case(&sh.name))) => {
            let sheet = gridcraft_engine::formula::quote_sheet(&sh.name);
            areas.map(|a| format!("{sheet}!{a}")).collect::<Vec<_>>().join(",")
        }
        _ => areas.collect::<Vec<_>>().join(","),
    }
}

/// Places the dialog's window before it is drawn, and returns the reference box the dialog is
/// collapsed to. Escape or Enter expands a collapsed dialog again, rather than closing it or
/// pressing OK, and the arrow keys select on the grid. Collapsed, the dialog shrinks to the box
/// where its top-left corner was, leaving the sheet free for selecting; expanded again, it comes
/// back where it was.
pub(super) fn place_window<'a>(
    app: &mut SheetApp,
    ctx: &egui::Context,
    d: &mut Dialog,
    window: egui::Window<'a>,
    window_id: egui::Id,
) -> (egui::Window<'a>, Option<String>) {
    follow_home(app, &mut d.refs);
    if d.refs.collapsed.is_some() {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) | i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            d.refs.collapsed = None;
        } else {
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
        let _ = app.session.run("selection.move", json!({"dr": dr, "dc": dc, "extend": m.shift, "jump": m.command}));
        app.grid.ensure_visible = true;
        app.grid.reveal_moving_end = m.shift;
        app.grid.reveal_from = from;
    }
}

/// After the dialog's window is drawn: remembers where the expanded window is, and ends picking
/// into a reference box when another text field of the dialog takes the keyboard. A collapsed
/// dialog shows no other field, so its box keeps picking whatever else is clicked (the Name Box,
/// the formula bar, the ribbon): only Escape, Enter or the expand button end that state.
pub(super) fn window_shown(ctx: &egui::Context, d: &mut Dialog, collapsed: bool, rect: Option<egui::Rect>) {
    if !collapsed {
        d.refs.rect = rect;
    }
    d.refs.was_collapsed = collapsed;
    if !collapsed && ctx.text_edit_focused() && d.refs.target.as_deref().is_none_or(|key| !ctx.memory(|m| m.has_focus(ref_edit_id(key)))) {
        d.refs.target = None;
    }
}

/// Makes the reference box `key` the one grid selections fill. The first pick remembers the
/// sheet and selection, which are put back when the dialog runs its command or closes.
pub(super) fn begin_pick(app: &SheetApp, d: &mut Dialog, key: &str) {
    if let Some(doc) = app.session.active()
        && let Some(sheet) = doc.wb.active()
    {
        let ranges: Vec<String> = doc.selection.ranges.iter().map(|r| r.a1()).collect();
        let selection = Some((ranges.join(","), doc.selection.active.a1()));
        match d.refs.home.as_mut() {
            None => {
                d.refs.home = Some(Home { sheet: Some(sheet.name.clone()), names: doc.wb.sheets.iter().map(|s| s.name.clone()).collect(), selection })
            }
            // Picking again after a failed OK: the selection to put back is the one now on the
            // dialog's sheet.
            Some(home) if home.selection.is_none() && home.sheet.as_deref().is_some_and(|own| own.eq_ignore_ascii_case(&sheet.name)) => {
                home.selection = selection;
            }
            Some(_) => {}
        }
    }
    d.refs.target = Some(key.to_string());
    d.refs.refocus = false;
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
pub(super) fn end_pick(app: &mut SheetApp, d: &mut Dialog) {
    d.refs.target = None;
    d.refs.collapsed = None;
    follow_home(app, &mut d.refs);
    let Some(home) = d.refs.home.as_mut() else { return };
    let Some(sheet) = home.sheet.clone() else { return };
    if let Some((range, active)) = home.selection.take() {
        let _ = app.session.run("selection.set", json!({"sheet": sheet, "range": range, "active": active}));
    } else if !active_sheet_name(app).eq_ignore_ascii_case(&sheet) {
        // Already put back by an earlier OK that failed: only the sheet is shown again, so
        // references without a sheet name mean it.
        let _ = app.session.run("sheet.activate", json!({"sheet": sheet}));
    }
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
        if r.gained_focus() {
            begin_pick(app, d, key);
        } else if d.refs.target.as_deref() == Some(key) {
            let sel = selection_ref(app, d.refs.home.as_ref(), takes_several_areas(&d.name, key));
            let sheet = active_sheet_name(app);
            if sheet != d.refs.seen_sheet {
                // Another sheet was shown: its selection is where that sheet was left, not a pick.
                d.refs.seen = sel;
                d.refs.seen_sheet = sheet;
            } else if sel != d.refs.seen {
                // The person selected on the grid: the selection becomes the reference.
                s = sel.clone();
                d.refs.seen = sel;
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
    d.values.insert(key.into(), json!(s));
}
