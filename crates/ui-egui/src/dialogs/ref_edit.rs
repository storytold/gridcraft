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
    /// The sheet, ranges and active cell from before picking began, put back when the dialog
    /// runs its command or closes.
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

/// Where picking began: the dialog's own sheet and the selection to put back.
#[derive(Clone, Debug)]
struct Home {
    /// The sheet's name. Followed by name when sheets are inserted or moved while the dialog is
    /// open (indexes change), and by position when the sheet itself is renamed.
    sheet: String,
    /// The sheet's index and the workbook's sheet count when last seen, to tell a rename in
    /// place (same count, the name gone from that index) from a deleted sheet.
    index: usize,
    count: usize,
    ranges: String,
    active: String,
}

/// Keeps the dialog's own sheet across changes made while it is open: a sheet inserted, moved or
/// deleted elsewhere moves its index, and a rename of the sheet itself changes its name.
fn follow_home(app: &SheetApp, refs: &mut RefBoxes) {
    let (Some(doc), Some(home)) = (app.session.active(), refs.home.as_mut()) else { return };
    let count = doc.wb.sheets.len();
    if let Some(i) = doc.wb.sheet_index(&home.sheet) {
        home.index = i;
    } else if count == home.count
        && let Some(sheet) = doc.wb.sheets.get(home.index)
    {
        // The sheet was renamed where it is.
        home.sheet = sheet.name.clone();
    }
    home.count = count;
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
fn selection_ref(app: &SheetApp, home_sheet: Option<&str>, several: bool) -> String {
    let Some(doc) = app.session.active() else { return String::new() };
    let current = [doc.selection.current()];
    let areas = if several { doc.selection.ranges.as_slice() } else { &current[..] }.iter().map(abs);
    match doc.wb.active() {
        Some(sh) if home_sheet.is_some_and(|h| !h.eq_ignore_ascii_case(&sh.name)) => {
            let sheet = gridcraft_engine::formula::quote_sheet(&sh.name);
            areas.map(|a| format!("{sheet}!{a}")).collect::<Vec<_>>().join(",")
        }
        _ => areas.collect::<Vec<_>>().join(","),
    }
}

/// Places the dialog's window before it is drawn, and returns the reference box the dialog is
/// collapsed to. Escape or Enter expands a collapsed dialog again, rather than closing it or
/// pressing OK. Collapsed, the dialog shrinks to the box where its top-left corner was, leaving
/// the sheet free for selecting; expanded again, it comes back where it was.
pub(super) fn place_window<'a>(
    app: &SheetApp,
    ctx: &egui::Context,
    d: &mut Dialog,
    window: egui::Window<'a>,
    window_id: egui::Id,
) -> (egui::Window<'a>, Option<String>) {
    follow_home(app, &mut d.refs);
    if d.refs.collapsed.is_some()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) | i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    {
        d.refs.collapsed = None;
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

/// After the dialog's window is drawn: remembers where the expanded window is, and ends picking
/// into a reference box when another text field of the dialog takes the keyboard.
pub(super) fn window_shown(ctx: &egui::Context, d: &mut Dialog, collapsed: bool, rect: Option<egui::Rect>) {
    if !collapsed {
        d.refs.rect = rect;
    }
    d.refs.was_collapsed = collapsed;
    if ctx.text_edit_focused() && d.refs.target.as_deref().is_none_or(|key| !ctx.memory(|m| m.has_focus(ref_edit_id(key)))) {
        d.refs.target = None;
    }
}

/// Makes the reference box `key` the one grid selections fill. The first pick remembers the
/// sheet and selection, which are put back when the dialog runs its command or closes.
pub(super) fn begin_pick(app: &SheetApp, d: &mut Dialog, key: &str) {
    if d.refs.home.is_none()
        && let Some(doc) = app.session.active()
        && let Some(sheet) = doc.wb.active()
    {
        let ranges: Vec<String> = doc.selection.ranges.iter().map(|r| r.a1()).collect();
        d.refs.home = Some(Home {
            sheet: sheet.name.clone(),
            index: doc.wb.active_sheet(),
            count: doc.wb.sheets.len(),
            ranges: ranges.join(","),
            active: doc.selection.active.a1(),
        });
    }
    d.refs.target = Some(key.to_string());
    d.refs.refocus = false;
    d.refs.seen = selection_ref(app, d.refs.home.as_ref().map(|h| h.sheet.as_str()), takes_several_areas(&d.name, key));
    d.refs.seen_sheet = active_sheet_name(app);
}

/// The name of the sheet shown in the active workbook.
fn active_sheet_name(app: &SheetApp) -> String {
    app.session.active().and_then(|doc| doc.wb.active()).map(|sh| sh.name.clone()).unwrap_or_default()
}

/// Stops picking and puts back the sheet and selection from before picking began.
pub(super) fn end_pick(app: &mut SheetApp, d: &mut Dialog) {
    d.refs.target = None;
    d.refs.collapsed = None;
    follow_home(app, &mut d.refs);
    if let Some(home) = d.refs.home.take() {
        let _ = app.session.run("selection.set", json!({"sheet": home.sheet, "range": home.ranges, "active": home.active}));
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
            let sel = selection_ref(app, d.refs.home.as_ref().map(|h| h.sheet.as_str()), takes_several_areas(&d.name, key));
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
