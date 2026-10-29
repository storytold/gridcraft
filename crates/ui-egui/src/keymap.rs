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
    let key = if navigation { key } else { physical.unwrap_or(key) };
    let ctx = LookupContext {
        key,
        modifiers: m,
        ctrl: m.ctrl && !m.alt,
        cmd: m.command && !m.alt,
        alt: m.alt && !m.ctrl && !m.command,
        ctrl_alt: m.alt && (m.ctrl || m.command),
        plain: !m.ctrl && !m.command && !m.alt,
        shift: m.shift,
        mac,
    };
    lookup_number_formats(ctx)
        .or_else(|| lookup_control_shortcuts(ctx))
        .or_else(|| lookup_windows_shortcuts(ctx))
        .or_else(|| lookup_platform_letters(ctx))
        .or_else(|| lookup_selection_shortcuts(ctx))
        .or_else(|| lookup_control_alt_shortcuts(ctx))
        .or_else(|| lookup_alt_shortcuts(ctx))
        .or_else(|| lookup_function_shortcuts(ctx))
}

#[derive(Clone, Copy)]
struct LookupContext {
    key: Key,
    modifiers: Modifiers,
    ctrl: bool,
    cmd: bool,
    alt: bool,
    ctrl_alt: bool,
    plain: bool,
    shift: bool,
    mac: bool,
}

fn lookup_number_formats(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
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
        // Ctrl+[ / Ctrl+] (Excel): select precedent / dependent cells; Shift walks every level.
        OpenBracket if ctrl && !s => run("formulas.selectPrecedents"),
        OpenBracket if ctrl => Shortcut::Run("formulas.selectPrecedents", json!({"all": true})),
        CloseBracket if ctrl && !s => run("formulas.selectDependents"),
        CloseBracket if ctrl => Shortcut::Run("formulas.selectDependents", json!({"all": true})),
        _ => return None,
    })
}

fn lookup_control_shortcuts(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
    Some(match k {
        // Windows Ctrl+; is in `ribbon::shortcut`; Excel for Mac uses Control+; and Command+;.
        Semicolon if mac && ctrl => Shortcut::Date,
        Semicolon if mac && cmd && !s => Shortcut::Time,
        _ => return None,
    })
}

fn lookup_windows_shortcuts(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
    Some(match k {
        // Ctrl+2..5 (Windows).
        Num2 if ctrl && !mac => run("home.bold"),
        Num3 if ctrl && !mac => run("home.italic"),
        Num4 if ctrl && !mac => run("home.underline"),
        Num5 if ctrl && !mac => run("home.strikethrough"),
        // Ctrl+6 hides and shows objects (Windows, and Control+6 in Excel for Mac).
        Num6 if ctrl && !s => run("view.showObjects"),
        _ => return None,
    })
}

fn lookup_platform_letters(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
    Some(match k {
        // Letters that differ between Windows and Mac.
        L if (cmd || ctrl) && s => run("data.filter"),
        L if cmd && mac => run("home.alignLeft"),
        L if ctrl && mac => Shortcut::Dialog("defineName", json!({})),
        // Ctrl+Shift+U (Control+Shift+U in Excel for Mac) expands or collapses the formula bar.
        U if ctrl && s => run("view.expandFormulaBar"),
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
        _ => return None,
    })
}

fn lookup_selection_shortcuts(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
    Some(match k {
        // Shift+Space selects the row; Ctrl+Space (Windows) / Control+Space (Excel for Mac) the
        // column. Cmd+Space stays free: it is the macOS Spotlight shortcut.
        Space if plain && s => run("selection.row"),
        Space if !mac && cmd && !s => run("selection.column"),
        Space if mac && ctrl && !m.command && !s => run("selection.column"),
        Home if cmd && s => Shortcut::ExtendToStart,
        End if cmd && s => Shortcut::ExtendToEnd,
        Home if plain && s => Shortcut::ExtendToRowStart,
        _ => return None,
    })
}

fn lookup_control_alt_shortcuts(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
    Some(match k {
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
        _ => return None,
    })
}

fn lookup_alt_shortcuts(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
    Some(match k {
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
        _ => return None,
    })
}

fn lookup_function_shortcuts(ctx: LookupContext) -> Option<Shortcut> {
    use Key::*;
    let LookupContext { key: k, ctrl, cmd, alt, ctrl_alt, plain, shift: s, mac, .. } = ctx;
    let _ = (ctrl, cmd, alt, ctrl_alt, plain, s, mac);
    let m = ctx.modifiers;
    let _ = m;
    Some(match k {
        // Function keys.
        F1 if cmd && !s => run("view.collapseRibbon"),
        F2 if (cmd || ctrl) && s => Shortcut::Dialog("comment", json!({"threaded": true})),
        F2 if plain && s => Shortcut::Dialog("comment", json!({"threaded": false})),
        F3 if cmd && s => run("formulas.createFromSelection"),
        F3 if cmd && mac => Shortcut::Dialog("defineName", json!({})),
        F3 if cmd => Shortcut::Dialog("nameManager", json!({})),
        F3 if plain && s => Shortcut::Dialog("insertFunction", json!({})),
        F3 if plain => Shortcut::Dialog("pasteName", json!({"shortcut": true})),
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

/// AltGr arrives as Ctrl+Alt on Windows: a key that also typed a character (AltGr+= is `~` on a German
/// layout; a user-defined Ctrl+Alt+A hotkey may type `ä`) is text, not a shortcut.
pub(crate) fn altgr_text(ctx: &egui::Context, m: Modifiers, mac: bool) -> bool {
    !mac && m.alt && m.ctrl && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_))))
}

/// Canonical name for an egui Key.
pub fn key_name(k: Key) -> Option<&'static str> {
    Some(match k {
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Escape => "Escape",
        Key::Space => "Space",
        Key::Insert => "Insert",
        Key::Equals | Key::Plus => "=",
        Key::Minus => "-",
        Key::Period => ".",
        Key::Comma => ",",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Semicolon => ";",
        Key::Quote => "'",
        Key::Backtick => "`",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        _ => return None,
    })
}

/// Canonical representation of a key combination, e.g. "Ctrl+Shift+E" or "Cmd+Option+E".
pub fn canonical_combo(key: Key, m: Modifiers, mac: bool) -> Option<String> {
    let name = key_name(key)?;
    let is_function_key =
        matches!(key, Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6 | Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12);
    if !m.ctrl && !m.command && !m.alt && !m.shift && !is_function_key {
        return None;
    }
    let mut s = String::new();
    if mac {
        if m.command || m.mac_cmd {
            s.push_str("Cmd+");
        }
        if m.ctrl {
            s.push_str("Ctrl+");
        }
        if m.alt {
            s.push_str("Option+");
        }
        if m.shift {
            s.push_str("Shift+");
        }
    } else {
        if m.ctrl || m.command {
            s.push_str("Ctrl+");
        }
        if m.alt {
            s.push_str("Alt+");
        }
        if m.shift {
            s.push_str("Shift+");
        }
    }
    s.push_str(name);
    Some(s)
}

fn canonical_key_name(s: &str) -> Option<String> {
    if s.len() == 1 {
        let c = s.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_alphanumeric() || matches!(c, '=' | '-' | '.' | ',' | '[' | ']' | ';' | '\'' | '`') {
            return Some(c.to_string());
        }
        return None;
    }
    if let Some(f) = s.strip_prefix(['f', 'F'])
        && let Ok(n) = f.parse::<u8>()
        && (1..=12).contains(&n)
    {
        return Some(format!("F{n}"));
    }
    match s.to_ascii_lowercase().as_str() {
        "left" | "arrowleft" => Some("Left".into()),
        "right" | "arrowright" => Some("Right".into()),
        "up" | "arrowup" => Some("Up".into()),
        "down" | "arrowdown" => Some("Down".into()),
        "home" => Some("Home".into()),
        "end" => Some("End".into()),
        "pageup" | "pgup" => Some("PageUp".into()),
        "pagedown" | "pgdn" => Some("PageDown".into()),
        "enter" | "return" => Some("Enter".into()),
        "tab" => Some("Tab".into()),
        "backspace" => Some("Backspace".into()),
        "delete" | "del" => Some("Delete".into()),
        "escape" | "esc" => Some("Escape".into()),
        "space" => Some("Space".into()),
        "insert" | "ins" => Some("Insert".into()),
        _ => None,
    }
}

/// Normalizes any user-entered shortcut string for the specified platform.
pub fn normalize_for_platform(s: &str, mac: bool) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return None;
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut cmd = false;
    let key_part = parts.last()?;
    for p in &parts[..parts.len() - 1] {
        match p.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            "cmd" | "command" | "mod" => cmd = true,
            _ => return None,
        }
    }
    let key_name = canonical_key_name(key_part)?;
    let is_function_key = key_name.starts_with('F') && key_name[1..].parse::<u8>().is_ok();
    if !ctrl && !alt && !shift && !cmd && !is_function_key {
        return None;
    }
    let mut out = String::new();
    if mac {
        if cmd {
            out.push_str("Cmd+");
        }
        if ctrl {
            out.push_str("Ctrl+");
        }
        if alt {
            out.push_str("Option+");
        }
        if shift {
            out.push_str("Shift+");
        }
    } else {
        if cmd || ctrl {
            out.push_str("Ctrl+");
        }
        if alt {
            out.push_str("Alt+");
        }
        if shift {
            out.push_str("Shift+");
        }
    }
    out.push_str(&key_name);
    Some(out)
}

/// Normalizes any user-entered shortcut string (case-insensitively, e.g. "ctrl+shift+f" -> "Ctrl+Shift+F").
pub fn normalize(s: &str) -> Option<String> {
    normalize_for_platform(s, cfg!(target_os = "macos"))
}

pub(crate) fn dispatch_target(app: &mut SheetApp, ctx: &egui::Context, target: &str) {
    if target.is_empty() {
        return;
    }
    if let Some(dialog_name) = target.strip_prefix("dialog:") {
        app.open_dialog(dialog_name, json!({}));
    } else if matches!(target, "edit.copy" | "edit.cut") {
        app.copy_to_clipboard(ctx, target);
    } else if target == "view.zoom" {
        app.open_dialog("zoom", json!({}));
    } else {
        let params = match target {
            "home.borders" => {
                let preset = if app.grid.last_border.is_empty() { "all" } else { &app.grid.last_border };
                json!({"preset": preset})
            }
            _ => json!({}),
        };
        app.run_or_alert(target, params);
    }
    app.grid.ensure_visible = true;
}

/// Handles a key press in the grid. Returns whether it was a shortcut.
pub(crate) fn dispatch(app: &mut SheetApp, ctx: &egui::Context, key: Key, physical: Option<Key>, m: Modifiers, geo: &Geo, sh: &Sheet) -> bool {
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    if altgr_text(ctx, m, mac) {
        return false;
    }
    // Check custom shortcuts before built-in shortcuts
    if let Some(combo) = canonical_combo(key, m, mac)
        && let Some(target) =
            app.ui.shortcuts.get(&combo).or_else(|| normalize_for_platform(&combo, mac).as_ref().and_then(|nc| app.ui.shortcuts.get(nc))).cloned()
    {
        // Web emits both a Key event and a browser clipboard event for Cmd/Ctrl+C and Cmd/Ctrl+X.
        // Skip key dispatch when a clipboard event is also queued so the action does not run twice.
        let is_clipboard_chord = matches!(key, Key::C | Key::X) && !m.alt && !m.shift;
        if is_clipboard_chord && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut))) {
            return false;
        }
        if target.is_empty() {
            return true;
        }
        dispatch_target(app, ctx, &target);
        return true;
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
            let from = app.session.active().map(|d| if m.shift { d.selection.moving_end() } else { d.selection.active });
            let _ = app.session.run("selection.move", json!({"dr": 0, "dc": dc, "extend": m.shift}));
            app.grid.ensure_visible = true;
            app.grid.reveal_moving_end = m.shift;
            app.grid.reveal_from = from;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_combo_formats_keys() {
        let mac_cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
        let ctrl_shift = Modifiers { ctrl: true, shift: true, command: true, ..Modifiers::NONE };
        assert_eq!(canonical_combo(Key::B, mac_cmd, true), Some("Cmd+B".into()));
        assert_eq!(canonical_combo(Key::K, ctrl_shift, false), Some("Ctrl+Shift+K".into()));
        assert_eq!(canonical_combo(Key::F5, Modifiers::NONE, false), Some("F5".into()));
        assert_eq!(canonical_combo(Key::F, Modifiers::NONE, false), None);
    }

    #[test]
    fn normalize_shortcuts() {
        assert_eq!(normalize_for_platform("ctrl+shift+k", false), Some("Ctrl+Shift+K".into()));
        assert_eq!(normalize_for_platform("cmd+b", false), Some("Ctrl+B".into()));
        assert_eq!(normalize_for_platform("cmd+b", true), Some("Cmd+B".into()));
        assert_eq!(normalize_for_platform("alt+k", true), Some("Option+K".into()));
        assert_eq!(normalize_for_platform("option+k", false), Some("Alt+K".into()));
        assert_eq!(normalize_for_platform("mod+shift+m", true), Some("Cmd+Shift+M".into()));
        assert_eq!(normalize_for_platform("ctrl + alt + f1", false), Some("Ctrl+Alt+F1".into()));
        assert_eq!(normalize_for_platform("ctrl+pageup", false), Some("Ctrl+PageUp".into()));
        assert_eq!(normalize_for_platform("ctrl+pagedown", false), Some("Ctrl+PageDown".into()));
        assert_eq!(normalize_for_platform("ctrl+whatever", false), None);
        assert_eq!(normalize_for_platform("ctrl+f13", false), None);
        assert_eq!(normalize_for_platform("f", false), None);
        assert_eq!(normalize_for_platform("f2", false), Some("F2".into()));
    }
}
