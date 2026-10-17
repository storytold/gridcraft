//! Excel keyboard shortcuts beyond the plain Ctrl/Cmd letters in [`crate::ribbon::shortcut`]:
//! number-format and border combinations, F-keys, Alt/Option combinations and the Excel for Mac
//! variants. Every entry maps to an existing command or dialog. `docs/ui-parity.md` holds the
//! audit against Microsoft's published Windows and Mac shortcut lists.

use egui::{Key, Modifiers};
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::model::Sheet;
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::grid::Geo;

pub(crate) const RESET_INTERFACE_SCALE: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(Modifiers { shift: true, ..Modifiers::COMMAND }, Key::F8);

/// A recovery shortcut must work even when an oversized editor, dialog or Backstage owns focus.
/// Run before widgets and keytips; ordinary grid shortcuts still go through `dispatch` below.
pub(crate) fn reset_interface_scale(app: &mut SheetApp, ctx: &egui::Context) {
    if !ctx.input(|i| i.focused) {
        return;
    }
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let mut reset = false;
    ctx.input_mut(|i| {
        i.events.retain(|event| {
            let egui::Event::Key { key, modifiers: m, pressed, repeat, .. } = event else { return true };
            let command = if mac { m.command && m.mac_cmd && !m.ctrl } else { m.command && m.ctrl && !m.mac_cmd };
            if *key == RESET_INTERFACE_SCALE.logical_key && command && m.shift && !m.alt {
                reset |= *pressed && !repeat;
                false
            } else {
                true
            }
        });
    });
    if reset {
        // Consuming the command key above bypasses keytips' usual cancellation.
        if app.keytips.prefix().is_some() {
            egui::Popup::close_all(ctx);
        }
        app.keytips.clear();
        app.run_or_alert("view.interfaceScale", json!({"percent": "auto"}));
    }
}

/// What a shortcut does.
#[derive(Clone, Debug, PartialEq)]
pub enum Shortcut {
    /// Run a command with these parameters.
    Run(&'static str, Json),
    /// Open a dialog.
    Dialog(&'static str, Json),
    /// Start editing with the current date or time (Ctrl+; and Ctrl+Shift+;).
    Date,
    Time,
    /// Ctrl+' / Ctrl+Shift+": the formula / value of the cell above, ready to edit.
    FormulaAbove,
    ValueAbove,
    /// Extend the selection to A1, to the last used cell, or to the start of the row.
    ExtendToStart,
    ExtendToEnd,
    ExtendToRowStart,
    /// Alt+Page Down / Alt+Page Up: one screen right / left.
    ScreenRight,
    ScreenLeft,
    /// Excel for Mac Control+U: edit the active cell (like F2).
    Edit,
    /// Shift+F10: the cell context menu.
    ContextMenu,
    /// Alt+Down: the active cell's drop-down list.
    DropDown,
    /// Ctrl+Alt+= / Ctrl+Alt+-: zoom in / out.
    Zoom(i32),
}

fn run(id: &'static str) -> Shortcut {
    Shortcut::Run(id, json!({}))
}

fn number_format(code: &str) -> Shortcut {
    Shortcut::Run("home.numberFormat", json!({"code": code}))
}

fn borders(preset: &str) -> Shortcut {
    Shortcut::Run("home.borders", json!({"preset": preset}))
}

/// The shortcut for a key press in the grid, if any. `physical` is the key's position on a US
/// layout (as Excel matches Ctrl+Shift+4 rather than the `$` character); `mac` selects the Excel
/// for Mac variants where they differ. Navigation keys go by the logical key: with Num Lock off
/// the numpad's arrows and Page Down report the digit keys (Num6, Num3) as their physical keys.
pub fn lookup(key: Key, physical: Option<Key>, m: Modifiers, mac: bool) -> Option<Shortcut> {
    use Key::*;
    let navigation = matches!(key, ArrowLeft | ArrowRight | ArrowUp | ArrowDown | PageUp | PageDown | Home | End | Insert | Delete);
    let k = if navigation { key } else { physical.unwrap_or(key) };
    // Control itself (Excel for Mac also uses Control for these), Ctrl on Windows / Cmd on Mac,
    // and Alt/Option alone.
    let ctrl = m.ctrl && !m.alt;
    let cmd = m.command && !m.alt;
    let alt = m.alt && !m.ctrl && !m.command;
    let ctrl_alt = m.alt && (m.ctrl || m.command);
    let plain = !m.ctrl && !m.command && !m.alt;
    let s = m.shift;
    Some(match k {
        // Ctrl+Shift+~ ! @ # $ % ^ & _ : number formats and borders (Windows and Mac).
        Backtick if ctrl && s => Shortcut::Run("home.numberFormat", json!({"format": "General"})),
        Num1 if ctrl && s => number_format("#,##0.00"),
        Num2 if ctrl && s => number_format("h:mm AM/PM"),
        Num3 if ctrl && s => number_format("d-mmm-yy"),
        Num4 if ctrl && s => number_format("$#,##0.00_);($#,##0.00)"),
        Num5 if ctrl && s => number_format("0%"),
        Num6 if ctrl && s => number_format("0.00E+00"),
        Num7 if ctrl && s => borders("outside"),
        Minus if ctrl && s => borders("none"),
        Num8 if ctrl && s => run("selection.currentRegion"),
        Semicolon if ctrl && s => Shortcut::Time,
        Quote if ctrl && s => Shortcut::ValueAbove,
        Quote if ctrl => Shortcut::FormulaAbove,
        // Windows Ctrl+; is in `ribbon::shortcut`; Excel for Mac uses Control+; and Command+;.
        Semicolon if mac && ctrl => Shortcut::Date,
        Semicolon if mac && cmd && !s => Shortcut::Time,
        // Ctrl+2..5 (Windows).
        Num2 if ctrl && !mac => run("home.bold"),
        Num3 if ctrl && !mac => run("home.italic"),
        Num4 if ctrl && !mac => run("home.underline"),
        Num5 if ctrl && !mac => run("home.strikethrough"),
        // Letters that differ between Windows and Mac.
        L if (cmd || ctrl) && s => run("data.filter"),
        L if cmd && mac => run("home.alignLeft"),
        L if ctrl && mac => Shortcut::Dialog("defineName", json!({})),
        U if ctrl && mac && !s => Shortcut::Edit,
        L if cmd => run("insert.table"),
        E if cmd && mac && !s => run("home.alignCenter"),
        E if ctrl && mac && !s => run("edit.flashFill"),
        F if cmd && s && !mac => Shortcut::Dialog("formatCells", json!({"tab": "Font"})),
        G if cmd && s && !mac => Shortcut::Dialog("statistics", json!({})),
        H if cmd && s && mac => Shortcut::Dialog("find", json!({"replace": true})),
        R if cmd && s && mac => Shortcut::Dialog("sort", json!({})),
        K if cmd && s && mac => run("data.group"),
        J if cmd && s && mac => run("data.ungroup"),
        P if cmd && !s => run("file.print"),
        Period if cmd && s => run("home.increaseFontSize"),
        Comma if cmd && s => run("home.decreaseFontSize"),
        Space if cmd && s => run("edit.selectAll"),
        // Shift+Space selects the row; Ctrl+Space (Windows) / Control+Space (Excel for Mac) the
        // column. Cmd+Space stays free: it is the macOS Spotlight shortcut.
        Space if plain && s => run("selection.row"),
        Space if !mac && cmd && !s => run("selection.column"),
        Space if mac && ctrl && !m.command && !s => run("selection.column"),
        Home if cmd && s => Shortcut::ExtendToStart,
        End if cmd && s => Shortcut::ExtendToEnd,
        Home if plain && s => Shortcut::ExtendToRowStart,
        // Ctrl+Alt / Cmd+Option combinations.
        V if ctrl_alt || (mac && m.ctrl && m.command) => Shortcut::Dialog("pasteSpecial", json!({})),
        Equals | Plus if ctrl_alt => Shortcut::Zoom(10),
        Minus if ctrl_alt && !mac => Shortcut::Zoom(-10),
        F9 if ctrl_alt => run("formulas.calculateNow"),
        R if ctrl_alt && mac => run("view.collapseRibbon"),
        Num0 if ctrl_alt && mac => borders("outside"),
        Minus if ctrl_alt && mac => borders("none"),
        ArrowLeft if ctrl_alt && mac => borders("left"),
        ArrowRight if ctrl_alt && mac => borders("right"),
        ArrowUp if ctrl_alt && mac => borders("top"),
        ArrowDown if ctrl_alt && mac => borders("bottom"),
        // Alt / Option alone.
        Equals if alt && !s && !mac => run("formulas.autoSum"),
        ArrowDown if alt && !s => Shortcut::DropDown,
        PageDown if alt => Shortcut::ScreenRight,
        PageUp if alt => Shortcut::ScreenLeft,
        ArrowRight if alt && s && !mac => run("data.group"),
        ArrowLeft if alt && s && !mac => run("data.ungroup"),
        ArrowRight if alt && !s && mac => run("sheet.next"),
        ArrowLeft if alt && !s && mac => run("sheet.previous"),
        F1 if alt && s => run("home.insertSheet"),
        F1 if alt => run("insert.chart"),
        F10 if alt && s => Shortcut::Dialog("errorChecking", json!({})),
        // Function keys.
        F1 if cmd && !s => run("view.collapseRibbon"),
        F2 if (cmd || ctrl) && s => Shortcut::Dialog("comment", json!({"threaded": true})),
        F2 if plain && s => Shortcut::Dialog("comment", json!({"threaded": false})),
        F3 if cmd && s => run("formulas.createFromSelection"),
        F3 if cmd && mac => Shortcut::Dialog("defineName", json!({})),
        F3 if cmd => Shortcut::Dialog("nameManager", json!({})),
        F3 if plain && s => Shortcut::Dialog("insertFunction", json!({})),
        F4 if cmd && !s => run("file.close"),
        F5 if plain && s => Shortcut::Dialog("find", json!({})),
        F5 if plain => Shortcut::Dialog("goTo", json!({})),
        F7 if plain && !s => Shortcut::Dialog("spelling", json!({})),
        F9 if plain && s => run("formulas.calculateSheet"),
        F10 if plain && s => Shortcut::ContextMenu,
        F11 if plain && s => run("home.insertSheet"),
        F12 if plain && !s => run("file.saveAs"),
        _ => return None,
    })
}

/// Handles a key press in the grid. Returns whether it was a shortcut.
pub(crate) fn dispatch(app: &mut SheetApp, ctx: &egui::Context, key: Key, physical: Option<Key>, m: Modifiers, geo: &Geo, sh: &Sheet) -> bool {
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    // AltGr arrives as Ctrl+Alt on Windows: a key that also typed a character (AltGr+= is `~`
    // on a German layout) is text, not a shortcut.
    if !mac && m.alt && m.ctrl && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_)))) {
        return false;
    }
    let Some(shortcut) = lookup(key, physical, m, mac) else { return false };
    let (active, anchor) = app.session.active().map(|d| (d.selection.active, d.selection.anchor)).unwrap_or_default();
    match shortcut {
        Shortcut::Run(id, params) => {
            app.run_or_alert(id, params);
            app.grid.ensure_visible = true;
        }
        Shortcut::Dialog(name, params) => app.open_dialog(name, params),
        Shortcut::Date | Shortcut::Time => {
            let text = app
                .session
                .active()
                .map(|d| {
                    let now = gridcraft_engine::core::date::serial_in_system(d.wb.date_system, gridcraft_engine::calc::now_serial());
                    let (value, format) = if shortcut == Shortcut::Date { (now.floor(), "m/d/yyyy") } else { (now.fract(), "h:mm AM/PM") };
                    gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(value), format, &d.wb).text
                })
                .unwrap_or_default();
            edit_with(app, text);
        }
        Shortcut::FormulaAbove | Shortcut::ValueAbove => {
            let Some(above) = active.row.checked_sub(1).map(|r| CellRef::new(r, active.col)) else { return true };
            let text = if shortcut == Shortcut::FormulaAbove {
                app.session.active().map(|d| gridcraft_engine::display::edit_text(&d.wb, sh, above)).unwrap_or_default()
            } else {
                app.session
                    .active()
                    .and_then(|d| sh.value_ref(above).map(|v| gridcraft_engine::display::format(v, "General", &d.wb).text))
                    .unwrap_or_default()
            };
            edit_with(app, text);
        }
        Shortcut::ExtendToStart => extend(app, anchor, active, CellRef::new(0, 0)),
        Shortcut::ExtendToEnd => {
            let end = sh.used_range().map(|u| u.end).unwrap_or_default();
            extend(app, anchor, active, end);
        }
        Shortcut::ExtendToRowStart => extend(app, anchor, active, CellRef::new(active.row, 0)),
        Shortcut::ScreenRight | Shortcut::ScreenLeft => {
            let width = ((geo.cells.width() - geo.frozen_w) / geo.z).max(1.0);
            let cols = (width / sh.default_col_width.max(1.0)).floor().max(1.0) as i64;
            let dc = if shortcut == Shortcut::ScreenRight { cols } else { -cols };
            let _ = app.session.run("selection.move", json!({"dr": 0, "dc": dc, "extend": m.shift}));
            app.grid.ensure_visible = true;
            app.grid.reveal_moving_end = m.shift;
        }
        Shortcut::Edit => app.begin_edit(None, false),
        Shortcut::ContextMenu => app.grid.context_menu = Some(geo.cell_rect(sh, active).center()),
        Shortcut::DropDown => {
            if sh.validations.iter().any(|d| d.in_cell_dropdown && d.ranges.iter().any(|r| r.contains(active))) {
                app.grid.list_picker = Some(active);
            } else if sh.has_filter_button(active) {
                let at = geo.mirror_pos(crate::grid::filter_button_rect(geo, sh, active).center());
                app.grid.filter_menu = Some((active.col, active.row, at));
            }
        }
        Shortcut::Zoom(step) => {
            let percent = (i32::from(sh.zoom).saturating_add(step)).clamp(10, 400);
            app.run_or_alert("view.zoom", json!({"percent": percent}));
        }
    }
    true
}

/// Starts editing the active cell with `text` and the caret at its end.
fn edit_with(app: &mut SheetApp, text: String) {
    app.begin_edit(Some(text), false);
    if let Some(ed) = app.editor.as_mut() {
        ed.caret = ed.text.chars().count();
    }
}

/// Selects `anchor`..`to`, keeping the anchor and the active cell.
fn extend(app: &mut SheetApp, anchor: CellRef, active: CellRef, to: CellRef) {
    let _ = app.session.run("selection.set", json!({"range": RangeRef::new(anchor, to).a1(), "active": active.a1()}));
    if let Some(d) = app.session.active_mut() {
        d.selection.anchor = anchor;
    }
    app.grid.ensure_visible = true;
    app.grid.reveal_moving_end = true;
}
