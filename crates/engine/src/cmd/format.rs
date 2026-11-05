//! Formatting: Home › Font, Alignment, Number, Styles, Cells › Format.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{
    BorderLine, BorderStyle, Borders, CellRuns, Color, Fill, Font, HAlign, NumFmt, RunFont, Style, StyleId, Theme, Underline, VAlign, VertAlign,
};
use serde_json::{Value as Json, json};

use super::{
    CommandSpec, EngineError, Result, Session, bad, bool_param, cmd, edit, f64_param, has_doc, ok, parse_lines, split_sheet, str_param,
    target_ranges, target_sheet,
};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // Font
        cmd!("home.bold", "Bold", ["Home", "Font"], Some("Cmd+B"), "{range?, chars?, on?: bool}", has_doc, bold),
        cmd!("home.italic", "Italic", ["Home", "Font"], Some("Cmd+I"), "{range?, chars?, on?}", has_doc, italic),
        cmd!(
            "home.underline",
            "Underline",
            ["Home", "Font"],
            Some("Cmd+U"),
            "{range?, chars?, on?, style?: single|double|singleAccounting|doubleAccounting}",
            has_doc,
            underline
        ),
        cmd!("home.doubleUnderline", "Double Underline", ["Home", "Font"], None, "{range?, chars?}", has_doc, |s, p| underline(
            s,
            &with(p, "style", json!("double"))
        )),
        cmd!("home.strikethrough", "Strikethrough", ["Home", "Font"], Some("Cmd+Shift+X"), "{range?, chars?, on?}", has_doc, strikethrough),
        cmd!("home.superscript", "Superscript", ["Home", "Font"], None, "{range?, chars?, on?}", has_doc, superscript),
        cmd!("home.subscript", "Subscript", ["Home", "Font"], None, "{range?, chars?, on?}", has_doc, subscript),
        cmd!("home.fontName", "Font", ["Home", "Font"], None, "{range?, chars?, name: \"Calibri\"}", has_doc, font_name),
        cmd!("home.fontSize", "Font Size", ["Home", "Font"], None, "{range?, chars?, size: 11}", has_doc, font_size),
        cmd!("home.increaseFontSize", "Increase Font Size", ["Home", "Font"], Some("Cmd+Shift+>"), "{range?, chars?}", has_doc, |s, p| step_font(
            s, p, true
        )),
        cmd!("home.decreaseFontSize", "Decrease Font Size", ["Home", "Font"], Some("Cmd+Shift+<"), "{range?, chars?}", has_doc, |s, p| step_font(
            s, p, false
        )),
        cmd!(
            "home.fontColor",
            "Font Color",
            ["Home", "Font"],
            None,
            "{range?, chars?, color: \"#C00000\" | \"auto\" | {theme: 4, tint: -0.25}}",
            has_doc,
            font_color
        ),
        cmd!("home.fillColor", "Fill Color", ["Home", "Font"], None, "{range?, color: \"#FFFF00\" | \"none\" | {theme, tint}}", has_doc, fill_color),
        cmd!(
            "home.borders",
            "Borders",
            ["Home", "Font"],
            None,
            "{range?, preset: bottom|top|left|right|none|all|outside|thickOutside|thickBottom|doubleBottom|topBottom|topThickBottom|topDoubleBottom|insideHorizontal|insideVertical|inside|diagonalDown|diagonalUp, style?: thin|medium|thick|dashed|dotted|double|hair…, color?}",
            has_doc,
            borders
        ),
        cmd!(
            "home.formatCells",
            "Format Cells…",
            ["Home", "Cells", "Format"],
            Some("Cmd+1"),
            "{range?, chars?, style: {font?, fill?, borders?, align?, numFmt?, protection?}} (partial Style JSON merged in; with `chars`, `font` only)",
            has_doc,
            format_cells
        ),
        // Alignment
        CommandSpec {
            shortcut_mac: Some("Cmd+L"),
            ..cmd!("home.alignLeft", "Align Left", ["Home", "Alignment"], Some("Cmd+L"), "{range?}", has_doc, |s, p| halign(s, p, HAlign::Left))
        },
        CommandSpec {
            shortcut_mac: Some("Cmd+E"),
            ..cmd!("home.alignCenter", "Center", ["Home", "Alignment"], Some("Cmd+E"), "{range?}", has_doc, |s, p| halign(s, p, HAlign::Center))
        },
        cmd!("home.alignRight", "Align Right", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| halign(s, p, HAlign::Right)),
        cmd!("home.justify", "Justify", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| halign(s, p, HAlign::Justify)),
        cmd!("home.alignTop", "Top Align", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| valign(s, p, VAlign::Top)),
        cmd!("home.alignMiddle", "Middle Align", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| valign(s, p, VAlign::Center)),
        cmd!("home.alignBottom", "Bottom Align", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| valign(s, p, VAlign::Bottom)),
        cmd!("home.wrapText", "Wrap Text", ["Home", "Alignment"], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.align.wrap,
            |st, v| st.align.wrap = v
        )),
        cmd!("home.shrinkToFit", "Shrink to Fit", [], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.align.shrink,
            |st, v| st.align.shrink = v
        )),
        cmd!("home.increaseIndent", "Increase Indent", ["Home", "Alignment"], Some("Ctrl+Alt+Tab"), "{range?}", has_doc, |s, p| indent(s, p, 1)),
        cmd!("home.decreaseIndent", "Decrease Indent", ["Home", "Alignment"], Some("Ctrl+Alt+Shift+Tab"), "{range?}", has_doc, |s, p| indent(
            s, p, -1
        )),
        cmd!(
            "home.orientation",
            "Orientation",
            ["Home", "Alignment"],
            None,
            "{range?, angle: -90..90 | \"vertical\" | \"counterclockwise\" | \"clockwise\" | \"up\" | \"down\" | 0}",
            has_doc,
            orientation
        ),
        cmd!("home.mergeCenter", "Merge & Center", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "center")),
        cmd!("home.mergeAcross", "Merge Across", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "across")),
        cmd!("home.mergeCells", "Merge Cells", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "cells")),
        cmd!("home.unmergeCells", "Unmerge Cells", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "unmerge")),
        // Number
        cmd!(
            "home.numberFormat",
            "Number Format",
            ["Home", "Number"],
            None,
            "{range?, format: \"General\"|\"Number\"|\"Currency\"|\"Accounting\"|\"Short Date\"|\"Long Date\"|\"Time\"|\"Percentage\"|\"Fraction\"|\"Scientific\"|\"Text\" | code: \"0.00\"}",
            has_doc,
            number_format
        ),
        cmd!("home.accounting", "Accounting Number Format", ["Home", "Number"], None, "{range?, symbol?: \"$\"}", has_doc, accounting),
        cmd!("home.percent", "Percent Style", ["Home", "Number"], Some("Ctrl+Shift+%"), "{range?}", has_doc, |s, p| set_fmt(s, p, "0%")),
        cmd!("home.comma", "Comma Style", ["Home", "Number"], None, "{range?}", has_doc, |s, p| set_fmt(s, p, COMMA)),
        cmd!("home.increaseDecimal", "Increase Decimal", ["Home", "Number"], None, "{range?}", has_doc, |s, p| decimals(s, p, 1)),
        cmd!("home.decreaseDecimal", "Decrease Decimal", ["Home", "Number"], None, "{range?}", has_doc, |s, p| decimals(s, p, -1)),
        // Styles
        cmd!(
            "home.cellStyle",
            "Cell Styles",
            ["Home", "Styles"],
            None,
            "{range?, name: \"Good\"|\"Bad\"|\"Neutral\"|\"Heading 1\"|\"Title\"|\"Total\"|\"Accent1\"…|\"Normal\"}",
            has_doc,
            cell_style
        ),
        // Cells › Format
        cmd!(
            "home.rowHeight",
            "Row Height…",
            ["Home", "Cells", "Format"],
            None,
            "{sheet?, rows?: \"2:5\" | \"2:2,4:5\" | \"Sheet2!2:5\", height: 20 (pixels) | points: 15}",
            has_doc,
            row_height
        ),
        cmd!(
            "home.columnWidth",
            "Column Width…",
            ["Home", "Cells", "Format"],
            None,
            "{sheet?, cols?: \"B:D\" | \"B:B,D:F\" | \"Sheet2!B:D\", width: 64 (pixels) | chars: 8.43}",
            has_doc,
            col_width
        ),
        cmd!("home.autofitRowHeight", "AutoFit Row Height", ["Home", "Cells", "Format"], None, "{sheet?, rows?}", has_doc, autofit_rows),
        cmd!("home.autofitColumnWidth", "AutoFit Column Width", ["Home", "Cells", "Format"], None, "{sheet?, cols?}", has_doc, autofit_cols),
        cmd!("home.defaultWidth", "Default Width…", ["Home", "Cells", "Format"], None, "{sheet?, width: 64 (pixels)}", has_doc, default_width),
        cmd!("home.hideRows", "Hide Rows", ["Home", "Cells", "Format", "Hide & Unhide"], Some("Cmd+9"), "{sheet?, rows?}", has_doc, |s, p| hide(
            s, p, true, true
        )),
        cmd!("home.hideColumns", "Hide Columns", ["Home", "Cells", "Format", "Hide & Unhide"], Some("Cmd+0"), "{sheet?, cols?}", has_doc, |s, p| {
            hide(s, p, false, true)
        }),
        cmd!(
            "home.unhideRows",
            "Unhide Rows",
            ["Home", "Cells", "Format", "Hide & Unhide"],
            Some("Cmd+Shift+9"),
            "{sheet?, rows?}",
            has_doc,
            |s, p| hide(s, p, true, false)
        ),
        cmd!(
            "home.unhideColumns",
            "Unhide Columns",
            ["Home", "Cells", "Format", "Hide & Unhide"],
            Some("Cmd+Shift+0"),
            "{sheet?, cols?}",
            has_doc,
            |s, p| hide(s, p, false, false)
        ),
        cmd!("home.lockCell", "Lock Cell", ["Home", "Cells", "Format"], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.protection.locked,
            |st, v| st.protection.locked = v
        )),
    ]
}

const ACCOUNTING: &str = "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)";
const COMMA: &str = "_(* #,##0.00_);_(* \\(#,##0.00\\);_(* \"-\"??_);_(@_)";

/// Accounting Number Format with the currency symbol to use (Excel's own button takes the system
/// currency; `symbol` is how the control channel picks another one, e.g. `"L"` for lempiras).
fn accounting(s: &mut Session, p: &Json) -> Result<Json> {
    let symbol = str_param(p, "symbol").unwrap_or("$");
    // The symbol goes into a quoted literal: a quote or a backslash would end it, and a control
    // character would reach the format's text as-is.
    if symbol.contains(['"', '\\']) || symbol.chars().any(char::is_control) {
        return Err(bad("home.accounting", "the symbol can't contain a quote, a backslash or a control character"));
    }
    set_fmt(s, p, &accounting_format(symbol))
}

fn accounting_format(symbol: &str) -> String {
    let symbol = format!("\"{symbol}\"");
    format!("_({symbol}* #,##0.00_);_({symbol}* \\(#,##0.00\\);_({symbol}* \"-\"??_);_(@_)")
}

fn with(p: &Json, key: &str, v: Json) -> Json {
    let mut m = p.as_object().cloned().unwrap_or_default();
    m.insert(key.into(), v);
    Json::Object(m)
}

/// Applies `f` to the style of every targeted cell (whole rows/columns set the line format too).
pub(crate) fn apply_style(s: &mut Session, p: &Json, f: impl Fn(&mut Style)) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let guarded = s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.protection.is_some());
    if s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.protection.as_ref().is_some_and(|pr| !pr.format_cells)) {
        return Err(EngineError::Other("The cell or chart you're trying to change is on a protected sheet.".into()));
    }
    edit(s, |cx| {
        // Formatting on a protected sheet keeps the cell's permission flags: a format call
        // that flips `locked` from the format permission would authorize content edits that
        // the password should still gate, so those flags may not change (Excel keeps that
        // checkbox disabled while the sheet is protected).
        const FLAG_GUARD: &str = "You can't change cell protection on a protected sheet. Unprotect it first.";
        macro_rules! guard_flags {
            ($old:expr, $new:expr) => {
                if guarded && cx.wb.styles.get($old).protection != cx.wb.styles.get($new).protection {
                    return Err(EngineError::Other(FLAG_GUARD.into()));
                }
            };
        }
        let mut cache: std::collections::HashMap<StyleId, StyleId> = std::collections::HashMap::new();
        for r in &ranges {
            let full_rows = r.is_full_rows();
            let full_cols = r.is_full_cols();
            if full_rows || full_cols {
                // Line formats.
                let lines: Vec<u32> = if full_rows && !full_cols { (r.start.row..=r.end.row).collect() } else { (r.start.col..=r.end.col).collect() };
                for line in lines {
                    let cur = {
                        let sh = cx.wb.sheet(sheet);
                        let info = if full_rows && !full_cols { sh.and_then(|s| s.rows.get(&line)) } else { sh.and_then(|s| s.cols.get(&line)) };
                        info.and_then(|i| i.style).unwrap_or_default()
                    };
                    let new = *cache.entry(cur).or_insert_with(|| cx.wb.styles.derive(cur, &f));
                    guard_flags!(cur, new);
                    let sh = cx.sheet_mut(sheet)?;
                    let map = if full_rows && !full_cols { &mut sh.rows } else { &mut sh.cols };
                    map.entry(line).or_default().style = Some(new);
                }
                // Existing cells in those lines.
                let cells: Vec<(CellRef, StyleId)> =
                    cx.wb.sheet(sheet).map(|sh| sh.cells.iter_range(*r).map(|(c, x)| (c, x.style)).collect()).unwrap_or_default();
                for (c, st) in &cells {
                    let new = *cache.entry(*st).or_insert_with(|| cx.wb.styles.derive(*st, &f));
                    guard_flags!(*st, new);
                    cx.sheet_mut(sheet)?.set_style(*c, new);
                }
                // Formatting touched these cells: re-fit their rows (Excel auto-fits).
                cx.fit_rows.extend(cells.iter().map(|(c, _)| (sheet, c.row)));
                continue;
            }
            if r.count() > 4_000_000 {
                return Err(bad("format", "selection too large to format cell by cell"));
            }
            if r.height() <= 100_000 {
                cx.fit_rows.extend((r.start.row..=r.end.row).map(|row| (sheet, row)));
            }
            for c in r.iter() {
                let st = cx.wb.sheet(sheet).map(|sh| sh.style_id(c)).unwrap_or_default();
                let new = *cache.entry(st).or_insert_with(|| cx.wb.styles.derive(st, &f));
                if new != st {
                    guard_flags!(st, new);
                    cx.sheet_mut(sheet)?.set_style(c, new);
                }
            }
        }
        Ok(Json::Null)
    })
}

/// The `chars: [start, len]` param — character-indexed, as the public API speaks — as a byte
/// range into `text`, by counting `char_indices`. Errors when `chars` is missing or not a
/// two-number array, covers no characters, or reaches past the text; ranges so derived always
/// land on UTF-8 boundaries.
fn chars_bytes(text: &str, p: &Json) -> Result<std::ops::Range<usize>> {
    let malformed = || bad("chars", "`chars` must be [start, len] in characters");
    let arr = p.get("chars").and_then(Json::as_array).ok_or_else(malformed)?;
    let start = arr.first().and_then(Json::as_u64).ok_or_else(malformed)?;
    let len = arr.get(1).and_then(Json::as_u64).ok_or_else(malformed)?;
    if len == 0 {
        return Err(bad("chars", "`chars` must cover at least one character"));
    }
    // `last` is the char index just past the range; byte offsets are the positions of the chars
    // at indexes `start` and `last - 1` (counted, so hostile offsets just miss the text).
    let last = start.checked_add(len).ok_or_else(malformed)?;
    let start_one = start.checked_add(1).ok_or_else(malformed)?;
    let (mut start_byte, mut end_byte) = (None, None);
    let mut nth: u64 = 0;
    for (i, c) in text.char_indices() {
        nth += 1;
        if nth == start_one {
            start_byte = Some(i);
        }
        if nth == last {
            end_byte = Some(i.saturating_add(c.len_utf8()));
            break;
        }
    }
    match (start_byte, end_byte) {
        (Some(start), Some(end)) => Ok(start..end),
        _ => Err(bad("chars", format!("`chars` [{start}, {len}] reaches past the cell's text"))),
    }
}

/// The runs of a cell without any: one all-default run over `len` bytes.
fn default_runs(len: u32) -> CellRuns {
    CellRuns { runs: vec![gridcraft_model::Run { start: 0, len, font: RunFont::default() }] }
}

/// Formats a character range of one cell's text with a font delta (the `chars` path of the Font
/// commands, e.g. `home.fontColor {cell, chars, color}`). Requires `cell` (or a one-cell
/// `range`) naming a cell that holds plain text; the cell's value is untouched — the runs
/// side-table carries the format. A result that is flat again deletes the side-table entry.
pub(crate) fn apply_run_font(s: &mut Session, p: &Json, delta: RunFont) -> Result<Json> {
    if str_param(p, "cell").is_none() && str_param(p, "range").is_none() {
        return Err(bad("chars", "`chars` needs `cell` (or a one-cell `range`) naming the text to format"));
    }
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let Some(r) = ranges.first().copied() else { return Err(bad("chars", "`chars` needs `cell` (or a one-cell `range`)")) };
    if ranges.len() > 1 || !r.is_single() {
        return Err(bad("chars", "`chars` formats a range within one cell: pass `cell`, or a one-cell `range`"));
    }
    let at = r.start;
    if s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.protection.as_ref().is_some_and(|pr| !pr.format_cells)) {
        return Err(EngineError::Other("The cell or chart you're trying to change is on a protected sheet.".into()));
    }
    let d = s.doc()?;
    let Some(sh) = d.wb.sheet(sheet) else { return Err(EngineError::Other("no such sheet".into())) };
    let not_text = || bad("chars", "the cell must hold plain text (not a formula or number) to format characters");
    let text = match sh.cell(at) {
        Some(cell) if cell.formula.is_none() => cell.value.as_text().ok_or_else(not_text)?.to_string(),
        _ => return Err(not_text()),
    };
    let len = u32::try_from(text.len()).map_err(|_| not_text())?;
    let range = chars_bytes(&text, p)?;
    // A cell without runs behaves like one all-default run over its text.
    let current = sh.cell_runs.get(&at).cloned().unwrap_or_else(|| default_runs(len));
    let formatted = current.format_range(&text, range, &delta).map_err(|_| bad("home.*", "too many runs"))?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        match formatted {
            Some(runs) => {
                sh.cell_runs.insert(at, runs);
            }
            // Flat again: drop the entry rather than store an empty run list.
            None => {
                sh.cell_runs.remove(&at);
            }
        }
        Ok(Json::Null)
    })
}

/// The toggle target for a `chars` selection: true only when every covered character's run
/// font, merged over the cell's base font, has the property `get` reads — the all-covered
/// rule, mirroring how the cell-level toggles read the active style. `false` when any covered
/// character lacks it (or nothing is covered: no runs), so the next press turns it on.
fn run_toggle_target(text: &str, runs: Option<&CellRuns>, range: &std::ops::Range<usize>, base: &Font, get: fn(&RunFont) -> Option<bool>) -> bool {
    let Some(runs) = runs else { return false };
    let end = range.end.min(text.len());
    let start = range.start.min(end);
    let mut covered = false;
    for run in &runs.runs {
        let rs = run.start as usize;
        let re = rs.saturating_add(run.len as usize);
        if re <= start || rs >= end {
            continue;
        }
        // Straddling runs count: format_range keeps their font on the split pieces.
        covered = true;
        let f = run.font.merged(base);
        let resolved = RunFont {
            name: Some(f.name.clone()),
            size: Some(f.size),
            bold: Some(f.bold),
            italic: Some(f.italic),
            underline: Some(f.underline),
            strike: Some(f.strike),
            color: Some(f.color),
            vert: Some(f.vert),
        };
        if get(&resolved) != Some(true) {
            return false;
        }
    }
    covered
}

/// What the Font commands' `chars` branches resolve against: the single target cell, its text,
/// the selected byte range, its current runs (`None` when the cell is flat) and its base font
/// (the cell style's). Validation mirrors [`apply_run_font`], which re-checks as it applies.
struct CharsTarget {
    sheet: usize,
    at: CellRef,
    text: String,
    range: std::ops::Range<usize>,
    runs: Option<CellRuns>,
    base: Font,
}

fn chars_target(s: &Session, p: &Json) -> Result<CharsTarget> {
    if str_param(p, "cell").is_none() && str_param(p, "range").is_none() {
        return Err(bad("chars", "`chars` needs `cell` (or a one-cell `range`) naming the text to format"));
    }
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let Some(r) = ranges.first().copied() else { return Err(bad("chars", "`chars` needs `cell` (or a one-cell `range`)")) };
    if ranges.len() > 1 || !r.is_single() {
        return Err(bad("chars", "`chars` formats a range within one cell: pass `cell`, or a one-cell `range`"));
    }
    let at = r.start;
    let d = s.doc()?;
    let Some(sh) = d.wb.sheet(sheet) else { return Err(EngineError::Other("no such sheet".into())) };
    let not_text = || bad("chars", "the cell must hold plain text (not a formula or number) to format characters");
    let text = match sh.cell(at) {
        Some(cell) if cell.formula.is_none() => cell.value.as_text().ok_or_else(not_text)?.to_string(),
        _ => return Err(not_text()),
    };
    let range = chars_bytes(&text, p)?;
    let base = d.wb.styles.get(sh.style_id(at)).font.clone();
    Ok(CharsTarget { sheet, at, text, range, runs: sh.cell_runs.get(&at).cloned(), base })
}

/// The `chars` branch of the Font toggles (bold, italic, …): `on` wins when given, otherwise
/// the all-covered rule ([`run_toggle_target`]) resolves it like the cell-level toggle reads
/// the active style — and the delta goes onto the runs, not the cell style.
fn chars_toggle(s: &mut Session, p: &Json, get: fn(&RunFont) -> Option<bool>, delta: fn(bool) -> RunFont) -> Result<Json> {
    let on = match bool_param(p, "on") {
        Some(v) => v,
        None => {
            let t = chars_target(s, p)?;
            !run_toggle_target(&t.text, t.runs.as_ref(), &t.range, &t.base, get)
        }
    };
    apply_run_font(s, p, delta(on)).map(|_| json!({"on": on}))
}

/// The style of the active cell (what toggles read).
pub(crate) fn active_style(s: &Session) -> Result<Style> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    Ok(d.wb.styles.get(sh.style_id(d.selection.active)).clone())
}

fn toggle(s: &mut Session, p: &Json, get: fn(&Style) -> bool, set: fn(&mut Style, bool)) -> Result<Json> {
    let on = match bool_param(p, "on") {
        Some(v) => v,
        None => !get(&active_style(s)?),
    };
    apply_style(s, p, |st| set(st, on))?;
    Ok(json!({"on": on}))
}

fn bold(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("chars").is_some() {
        return chars_toggle(s, p, |rf| rf.bold, |on| RunFont { bold: Some(on), ..RunFont::default() });
    }
    toggle(s, p, |st| st.font.bold, |st, v| st.font.bold = v)
}

fn italic(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("chars").is_some() {
        return chars_toggle(s, p, |rf| rf.italic, |on| RunFont { italic: Some(on), ..RunFont::default() });
    }
    toggle(s, p, |st| st.font.italic, |st, v| st.font.italic = v)
}

/// A [`run_toggle_target`] property: the resolved underline equals `u` (a plain `fn`, since the
/// helper takes a function pointer).
fn underline_get(u: Underline) -> fn(&RunFont) -> Option<bool> {
    match u {
        Underline::None => |rf: &RunFont| Some(rf.underline == Some(Underline::None)),
        Underline::Single => |rf: &RunFont| Some(rf.underline == Some(Underline::Single)),
        Underline::Double => |rf: &RunFont| Some(rf.underline == Some(Underline::Double)),
        Underline::SingleAccounting => |rf: &RunFont| Some(rf.underline == Some(Underline::SingleAccounting)),
        Underline::DoubleAccounting => |rf: &RunFont| Some(rf.underline == Some(Underline::DoubleAccounting)),
    }
}

fn underline(s: &mut Session, p: &Json) -> Result<Json> {
    let style = match str_param(p, "style").unwrap_or("single") {
        "double" => Underline::Double,
        "singleAccounting" => Underline::SingleAccounting,
        "doubleAccounting" => Underline::DoubleAccounting,
        "none" => Underline::None,
        _ => Underline::Single,
    };
    if p.get("chars").is_some() {
        let on = match bool_param(p, "on") {
            Some(v) => v,
            None => {
                let t = chars_target(s, p)?;
                !run_toggle_target(&t.text, t.runs.as_ref(), &t.range, &t.base, underline_get(style))
            }
        };
        let u = if on { style } else { Underline::None };
        return apply_run_font(s, p, RunFont { underline: Some(u), ..RunFont::default() }).map(|_| json!({"on": on}));
    }
    let cur = active_style(s)?.font.underline;
    let on = bool_param(p, "on").unwrap_or(cur != style);
    let u = if on { style } else { Underline::None };
    apply_style(s, p, |st| st.font.underline = u)?;
    Ok(json!({"on": on}))
}

fn strikethrough(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("chars").is_some() {
        return chars_toggle(s, p, |rf| rf.strike, |on| RunFont { strike: Some(on), ..RunFont::default() });
    }
    toggle(s, p, |st| st.font.strike, |st, v| st.font.strike = v)
}

fn superscript(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("chars").is_some() {
        return chars_toggle(
            s,
            p,
            |rf| rf.vert.map(|v| v == VertAlign::Superscript),
            |on| RunFont { vert: Some(if on { VertAlign::Superscript } else { VertAlign::Baseline }), ..RunFont::default() },
        );
    }
    toggle(s, p, |st| st.font.vert == VertAlign::Superscript, |st, v| st.font.vert = if v { VertAlign::Superscript } else { VertAlign::Baseline })
}

fn subscript(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("chars").is_some() {
        return chars_toggle(
            s,
            p,
            |rf| rf.vert.map(|v| v == VertAlign::Subscript),
            |on| RunFont { vert: Some(if on { VertAlign::Subscript } else { VertAlign::Baseline }), ..RunFont::default() },
        );
    }
    toggle(s, p, |st| st.font.vert == VertAlign::Subscript, |st, v| st.font.vert = if v { VertAlign::Subscript } else { VertAlign::Baseline })
}

fn font_name(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(name) = str_param(p, "name").map(str::to_string) else { return Err(bad("home.fontName", "missing `name`")) };
    if p.get("chars").is_some() {
        return apply_run_font(s, p, RunFont { name: Some(name), ..RunFont::default() });
    }
    apply_style(s, p, |st| {
        st.font.name = name.clone();
        st.font.charset = None;
        st.font.family = None;
    })
}

fn font_size(s: &mut Session, p: &Json) -> Result<Json> {
    let size = f64_param(p, "size").ok_or_else(|| bad("home.fontSize", "missing `size`"))?;
    if !(1.0..=409.0).contains(&size) {
        return Err(EngineError::Other("The number must be between 1 and 409.".into()));
    }
    let size = ((size * 2.0).round() / 2.0) as f32;
    if p.get("chars").is_some() {
        return apply_run_font(s, p, RunFont { size: Some(size), ..RunFont::default() });
    }
    apply_style(s, p, |st| st.font.size = size)
}

const SIZES: [f32; 16] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0, 48.0, 72.0, 96.0, 409.0];

/// One size step from `cur`, saturating at the cell-level bounds (1–409) the way `step_font`
/// saturates past hostile values.
fn step_size(cur: f32, up: bool) -> f32 {
    if !cur.is_finite() {
        return if up { 409.0 } else { 1.0 };
    }
    if up { (cur + 1.0).clamp(1.0, 409.0) } else { (cur - 1.0).clamp(1.0, 409.0) }
}

/// Increase/Decrease Font Size over a `chars` selection: not one delta — every covered run
/// steps from its own merged size. The runs split at the selection edges first (an all-`None`
/// delta is the identity, so the pass only splits), each covered run takes `merged.size ± 1`
/// clamped like [`font_size`], and a final identity pass over the whole text coalesces,
/// detects flat and checks the cap. One command, one undo step.
fn step_chars_font(s: &mut Session, p: &Json, up: bool) -> Result<Json> {
    let t = chars_target(s, p)?;
    let not_text = || bad("chars", "the cell's text is too long to format");
    let len = u32::try_from(t.text.len()).map_err(|_| not_text())?;
    let current = t.runs.unwrap_or_else(|| default_runs(len));
    let split = current.format_range(&t.text, t.range.clone(), &RunFont::default()).map_err(|_| bad("home.*", "too many runs"))?;
    let mut split = split.unwrap_or_else(|| default_runs(len));
    for run in split.runs.iter_mut() {
        let rs = run.start as usize;
        if rs < t.range.start || rs.saturating_add(run.len as usize) > t.range.end {
            continue; // Not covered (the split put the selection edges on run boundaries).
        }
        let cur = run.font.merged(&t.base).size;
        run.font.size = Some(step_size(cur, up));
    }
    let formatted = split.format_range(&t.text, 0..t.text.len(), &RunFont::default()).map_err(|_| bad("home.*", "too many runs"))?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(t.sheet)?;
        match formatted {
            Some(runs) => {
                sh.cell_runs.insert(t.at, runs);
            }
            // Flat again: drop the entry rather than store an empty run list.
            None => {
                sh.cell_runs.remove(&t.at);
            }
        }
        Ok(Json::Null)
    })
}

fn step_font(s: &mut Session, p: &Json, up: bool) -> Result<Json> {
    if p.get("chars").is_some() {
        return step_chars_font(s, p, up);
    }
    let cur = active_style(s)?.font.size;
    let next =
        if up { SIZES.iter().copied().find(|x| *x > cur).unwrap_or(409.0) } else { SIZES.iter().rev().copied().find(|x| *x < cur).unwrap_or(1.0) };
    apply_style(s, p, |st| st.font.size = next)?;
    Ok(json!({"size": next}))
}

pub(crate) fn color_param(v: Option<&Json>) -> Option<Color> {
    match v? {
        Json::String(s) if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("automatic") || s.eq_ignore_ascii_case("none") => {
            Some(Color::Auto)
        }
        Json::String(s) => Color::from_hex(s),
        Json::Object(o) => {
            let theme = o.get("theme")?.as_u64()? as u8;
            let tint = o.get("tint").and_then(Json::as_f64).unwrap_or(0.0);
            Some(Color::Theme(theme.min(11), (tint.clamp(-1.0, 1.0) * 1000.0) as i16))
        }
        _ => None,
    }
}

fn font_color(s: &mut Session, p: &Json) -> Result<Json> {
    let c = color_param(p.get("color")).ok_or_else(|| bad("home.fontColor", "missing or invalid `color`"))?;
    if p.get("chars").is_some() {
        // A character range: the delta goes onto the cell's runs, not its cell style.
        return apply_run_font(s, p, RunFont { color: Some(c), ..RunFont::default() });
    }
    apply_style(s, p, |st| st.font.color = c)
}

fn fill_color(s: &mut Session, p: &Json) -> Result<Json> {
    let c = color_param(p.get("color")).ok_or_else(|| bad("home.fillColor", "missing or invalid `color`"))?;
    apply_style(s, p, |st| st.fill = if c == Color::Auto { Fill::default() } else { Fill::solid(c) })
}

fn border_style(name: &str) -> BorderStyle {
    match name {
        "medium" => BorderStyle::Medium,
        "thick" => BorderStyle::Thick,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        "hair" => BorderStyle::Hair,
        "mediumDashed" => BorderStyle::MediumDashed,
        "dashDot" => BorderStyle::DashDot,
        "mediumDashDot" => BorderStyle::MediumDashDot,
        "dashDotDot" => BorderStyle::DashDotDot,
        "slantDashDot" => BorderStyle::SlantDashDot,
        "none" => BorderStyle::None,
        _ => BorderStyle::Thin,
    }
}

fn borders(s: &mut Session, p: &Json) -> Result<Json> {
    let preset = str_param(p, "preset").unwrap_or("bottom").to_string();
    let color = color_param(p.get("color")).unwrap_or(Color::Auto);
    let base = border_style(str_param(p, "style").unwrap_or("thin"));
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    if s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.protection.as_ref().is_some_and(|pr| !pr.format_cells)) {
        return Err(EngineError::Other("The cell or chart you're trying to change is on a protected sheet.".into()));
    }
    edit(s, |cx| {
        for r in &ranges {
            if r.count() > 2_000_000 {
                return Err(bad("home.borders", "selection too large"));
            }
            for c in r.iter() {
                let (top, bottom, left, right) = (c.row == r.start.row, c.row == r.end.row, c.col == r.start.col, c.col == r.end.col);
                let st = cx.wb.sheet(sheet).map(|sh| sh.style_id(c)).unwrap_or_default();
                let line = |style: BorderStyle| BorderLine { style, color };
                let new = cx.wb.styles.derive(st, |s| {
                    let b = &mut s.borders;
                    match preset.as_str() {
                        "none" => *b = Borders::default(),
                        "all" => {
                            b.top = line(base);
                            b.bottom = line(base);
                            b.left = line(base);
                            b.right = line(base);
                        }
                        "outside" | "thickOutside" => {
                            let st = if preset == "thickOutside" { BorderStyle::Thick } else { base };
                            if top {
                                b.top = line(st);
                            }
                            if bottom {
                                b.bottom = line(st);
                            }
                            if left {
                                b.left = line(st);
                            }
                            if right {
                                b.right = line(st);
                            }
                        }
                        "inside" | "insideHorizontal" | "insideVertical" => {
                            if preset != "insideVertical" {
                                if !top {
                                    b.top = line(base);
                                }
                                if !bottom {
                                    b.bottom = line(base);
                                }
                            }
                            if preset != "insideHorizontal" {
                                if !left {
                                    b.left = line(base);
                                }
                                if !right {
                                    b.right = line(base);
                                }
                            }
                        }
                        "top" if top => b.top = line(base),
                        "bottom" if bottom => b.bottom = line(base),
                        "left" if left => b.left = line(base),
                        "right" if right => b.right = line(base),
                        "thickBottom" if bottom => b.bottom = line(BorderStyle::Thick),
                        "doubleBottom" if bottom => b.bottom = line(BorderStyle::Double),
                        "topBottom" => {
                            if top {
                                b.top = line(base);
                            }
                            if bottom {
                                b.bottom = line(base);
                            }
                        }
                        "topThickBottom" => {
                            if top {
                                b.top = line(base);
                            }
                            if bottom {
                                b.bottom = line(BorderStyle::Thick);
                            }
                        }
                        "topDoubleBottom" => {
                            if top {
                                b.top = line(base);
                            }
                            if bottom {
                                b.bottom = line(BorderStyle::Double);
                            }
                        }
                        "diagonalDown" => b.diag_down = line(base),
                        "diagonalUp" => b.diag_up = line(base),
                        _ => {}
                    }
                });
                cx.sheet_mut(sheet)?.set_style(c, new);
                if preset == "none" {
                    // A shared line can be stored on either cell. Selected cells are handled
                    // by this loop; remove only facing edges outside this range.
                    for (boundary, dr, dc) in [(top, -1, 0), (bottom, 1, 0), (left, 0, -1), (right, 0, 1)] {
                        if !boundary {
                            continue;
                        }
                        let Some(neighbor) = c.offset(dr, dc) else { continue };
                        let st = cx.wb.sheet(sheet).map(|sh| sh.style_id(neighbor)).unwrap_or_default();
                        let new = cx.wb.styles.derive(st, |s| {
                            let edge = match (dr, dc) {
                                (-1, _) => &mut s.borders.bottom,
                                (1, _) => &mut s.borders.top,
                                (_, -1) => &mut s.borders.right,
                                _ => &mut s.borders.left,
                            };
                            *edge = BorderLine::default();
                        });
                        if new != st {
                            cx.sheet_mut(sheet)?.set_style(neighbor, new);
                        }
                    }
                }
            }
        }
        Ok(Json::Null)
    })
}

fn format_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(patch) = p.get("style").map(snake_case_keys) else {
        if p.get("chars").is_some() {
            return Err(bad("home.formatCells", "`chars` needs `style.font`: only a font can be applied to a character range"));
        }
        // Menu invocation without params: the UI opens the dialog.
        s.ui_requests.push(crate::UiRequest::Dialog("formatCells".into(), json!({})));
        return ok();
    };
    if p.get("chars").is_some() {
        return format_cells_chars(s, p, patch);
    }
    // Validate by merging into the active style once.
    let base = serde_json::to_value(active_style(s)?).map_err(|e| EngineError::Other(e.to_string()))?;
    let merged = merge_json(base, &patch);
    let _: Style = serde_json::from_value(merged).map_err(|e| bad("home.formatCells", e.to_string()))?;
    apply_style(s, p, |st| {
        if let Ok(cur) = serde_json::to_value(&*st)
            && let Ok(new) = serde_json::from_value::<Style>(merge_json(cur, &patch))
        {
            *st = new;
        }
    })
}

/// The style patch with camelCase keys (`numFmt`, `diagDown`, as documented) renamed to the
/// style's own snake_case (`num_fmt`, `diag_down`). Keys starting with a capital (`Rgb`) stay.
fn snake_case_keys(v: &Json) -> Json {
    match v {
        Json::Object(m) => Json::Object(
            m.iter()
                .map(|(k, v)| {
                    let key = if k.starts_with(|c: char| c.is_ascii_lowercase()) {
                        k.chars().fold(String::new(), |mut out, c| {
                            if c.is_ascii_uppercase() {
                                out.push('_');
                            }
                            out.push(c.to_ascii_lowercase());
                            out
                        })
                    } else {
                        k.clone()
                    };
                    (key, snake_case_keys(v))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The `chars` branch of Format Cells: the style object may carry only `font` — fill, borders,
/// align, numFmt and protection cannot apply to a character range — and the font part becomes
/// the run delta, read the same way the cell-level path reads it.
fn format_cells_chars(s: &mut Session, p: &Json, patch: Json) -> Result<Json> {
    let missing = || bad("home.formatCells", "`chars` needs `style.font`: only a font can be applied to a character range");
    let obj = patch.as_object().ok_or_else(missing)?;
    if !obj.contains_key("font") {
        return Err(missing());
    }
    if obj.len() > 1 {
        return Err(bad("home.formatCells", "`chars` accepts only `style.font` — apply the other style parts to whole cells"));
    }
    let font = obj.get("font").cloned().ok_or_else(missing)?;
    let delta: RunFont = serde_json::from_value(font).map_err(|e| bad("home.formatCells", e.to_string()))?;
    apply_run_font(s, p, delta)
}

/// Deep-merges `patch` into `base` (objects merge, everything else replaces).
pub(crate) fn merge_json(mut base: Json, patch: &Json) -> Json {
    match (&mut base, patch) {
        (Json::Object(b), Json::Object(p)) => {
            for (k, v) in p {
                let cur = b.remove(k).unwrap_or(Json::Null);
                b.insert(k.clone(), merge_json(cur, v));
            }
            base
        }
        (_, p) => p.clone(),
    }
}

fn halign(s: &mut Session, p: &Json, a: HAlign) -> Result<Json> {
    // Clicking the active alignment again returns to General.
    let cur = active_style(s)?.align.h;
    let a = if cur == a && p.get("on").is_none() { HAlign::General } else { a };
    apply_style(s, p, |st| st.align.h = a)
}

fn valign(s: &mut Session, p: &Json, a: VAlign) -> Result<Json> {
    apply_style(s, p, |st| st.align.v = a)
}

fn indent(s: &mut Session, p: &Json, d: i32) -> Result<Json> {
    apply_style(s, p, |st| {
        st.align.indent = (st.align.indent as i32 + d).clamp(0, 250) as u8;
        if st.align.indent > 0 && matches!(st.align.h, HAlign::General | HAlign::Center | HAlign::Fill | HAlign::Justify | HAlign::CenterAcross) {
            st.align.h = HAlign::Left;
        }
    })
}

fn orientation(s: &mut Session, p: &Json) -> Result<Json> {
    let angle: i16 = match p.get("angle") {
        Some(Json::Number(n)) => n.as_f64().unwrap_or(0.0).clamp(-90.0, 90.0) as i16,
        Some(Json::String(t)) => match t.as_str() {
            "vertical" => 255,
            "counterclockwise" => 45,
            "clockwise" => -45,
            "up" => 90,
            "down" => -90,
            _ => 0,
        },
        _ => 0,
    };
    apply_style(s, p, |st| st.align.rotation = angle)
}

fn merge(s: &mut Session, p: &Json, how: &str) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let how =
        if how == "center" && s.doc()?.wb.sheet(sheet).is_some_and(|sh| ranges.iter().all(|r| sh.merges.contains(r))) { "unmerge" } else { how };
    edit(s, |cx| {
        let mut touched = Vec::new();
        // Areas whose merging changes: a merged cell blocks a dynamic array's spill.
        let mut areas: Vec<RangeRef> = ranges.clone();
        for r in &ranges {
            let sh = cx.sheet_mut(sheet)?;
            areas.extend(sh.merges.iter().filter(|m| m.intersects(r)));
            sh.merges.retain(|m| !m.intersects(r));
            match how {
                "unmerge" => {}
                "across" => {
                    for row in r.start.row..=r.end.row {
                        let m = RangeRef::new(CellRef::new(row, r.start.col), CellRef::new(row, r.end.col));
                        if !m.is_single() {
                            sh.merges.push(m);
                            // As below: only the first value of each row survives.
                            let others: Vec<CellRef> = sh.cells.iter_range(m).map(|(c, _)| c).filter(|c| *c != m.start).collect();
                            for c in others {
                                if let Some(mut cell) = sh.cells.get(c).cloned() {
                                    cell.clear_content();
                                    sh.set_cell(c, cell);
                                }
                                touched.push((sheet, c));
                            }
                        }
                    }
                }
                _ => {
                    if !r.is_single() {
                        sh.merges.push(*r);
                    }
                    // Only the upper-left value survives (Excel warns about this).
                    if r.count() <= 1_000_000 {
                        let others: Vec<CellRef> = sh.cells.iter_range(*r).map(|(c, _)| c).filter(|c| *c != r.start).collect();
                        for c in others {
                            if let Some(mut cell) = sh.cells.get(c).cloned() {
                                cell.clear_content();
                                sh.set_cell(c, cell);
                            }
                            touched.push((sheet, c));
                        }
                    }
                }
            }
        }
        // Spills over the areas, and blocked ones that could reach them, are evaluated again.
        let sh = cx.sheet_mut(sheet)?;
        for a in &areas {
            touched.extend(sh.spill_ranges.iter().filter(|(_, r)| r.intersects(a)).map(|(anchor, _)| (sheet, *anchor)));
            let reach = RangeRef::new(CellRef::new(0, 0), a.end);
            let blocked = sh
                .cells
                .iter_range(reach)
                // A Python cell's result is blocked and unblocked like a formula's.
                .filter(|(_, x)| {
                    (x.formula.is_some() || x.py.is_some()) && x.value == gridcraft_core::Value::Error(gridcraft_core::CellError::Spill)
                });
            touched.extend(blocked.map(|(c, _)| (sheet, c)));
        }
        cx.changed.extend(touched);
        Ok(())
    })?;
    if how == "center" {
        apply_style(s, p, |st| st.align.h = HAlign::Center)?;
    }
    ok()
}

/// Ribbon format names → codes.
pub fn format_code_for(name: &str) -> &str {
    match name {
        "General" => "General",
        "Number" => "0.00",
        "Currency" => "\"$\"#,##0.00",
        "Accounting" => ACCOUNTING,
        "Short Date" => "m/d/yyyy",
        "Long Date" => "[$-F800]dddd, mmmm dd, yyyy",
        "Time" => "[$-F400]h:mm:ss AM/PM",
        "Percentage" => "0.00%",
        "Fraction" => "# ?/?",
        "Scientific" => "0.00E+00",
        "Text" => "@",
        "Comma" => COMMA,
        other => other,
    }
}

fn number_format(s: &mut Session, p: &Json) -> Result<Json> {
    let code = str_param(p, "code")
        .map(str::to_string)
        .or_else(|| str_param(p, "format").map(|f| format_code_for(f).to_string()))
        .ok_or_else(|| bad("home.numberFormat", "missing `format` or `code`"))?;
    set_fmt(s, p, &code)
}

fn set_fmt(s: &mut Session, p: &Json, code: &str) -> Result<Json> {
    let nf = NumFmt::new(code);
    apply_style(s, p, |st| st.num_fmt = nf.clone())
}

/// Adds or removes one decimal place in the number format (Increase/Decrease Decimal).
pub fn adjust_decimals(code: &str, value: &gridcraft_core::Value, delta: i32) -> String {
    if code == "General" {
        // Start from the displayed number of decimals.
        let text = value.as_f64().map(gridcraft_core::number_to_text).unwrap_or_default();
        let dec = text.split_once('.').map_or(0, |(_, d)| d.len()) as i32;
        let n = (dec + delta).max(0) as usize;
        return if n == 0 { "0".into() } else { format!("0.{}", "0".repeat(n)) };
    }
    let sections: Vec<String> = code
        .split(';')
        .map(|sec| {
            // Find the last digit placeholder run before any exponent / percent etc.
            let chars: Vec<char> = sec.chars().collect();
            let mut in_quote = false;
            let mut in_bracket = false;
            let mut last_digit = None;
            let mut point = None;
            for (i, c) in chars.iter().enumerate() {
                if *c == '"' {
                    in_quote = !in_quote;
                }
                if *c == '[' && !in_quote {
                    in_bracket = true;
                }
                if *c == ']' && !in_quote {
                    in_bracket = false;
                    continue;
                }
                if in_quote || in_bracket {
                    continue;
                }
                if matches!(c, '0' | '#' | '?') {
                    last_digit = Some(i);
                }
                if *c == '.' && point.is_none() {
                    point = Some(i);
                }
                if matches!(c, 'E' | 'e') {
                    break;
                }
            }
            let Some(ld) = last_digit else { return sec.to_string() };
            let mut out: Vec<char> = chars.clone();
            if delta > 0 {
                match point {
                    Some(_) => out.insert(ld + 1, '0'),
                    None => {
                        out.insert(ld + 1, '0');
                        out.insert(ld + 1, '.');
                    }
                }
            } else if let Some(pt) = point
                && ld > pt
            {
                out.remove(ld);
                if ld == pt + 1 {
                    out.remove(pt);
                }
            }
            out.into_iter().collect()
        })
        .collect();
    sections.join(";")
}

fn decimals(s: &mut Session, p: &Json, delta: i32) -> Result<Json> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let a = d.selection.active;
    let code = d.wb.styles.get(sh.style_id(a)).num_fmt.as_str().to_string();
    let new = adjust_decimals(&code, &sh.value(a), delta);
    set_fmt(s, p, &new)
}

pub use gridcraft_model::points_to_chars;

mod cell_styles;
mod sizes;

use cell_styles::cell_style;
pub use cell_styles::{CELL_STYLE_NAMES, builtin_cell_style};
pub use sizes::{approx_text_width, chars_to_points};
use sizes::{autofit_cols, autofit_rows, col_width, default_width, hide, row_height};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_across_keeps_the_first_value_of_each_row() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
        s.execute("cell.set", json!({"cell": "D1", "input": "=SUM(A1:B2)"})).unwrap();
        s.execute("home.mergeAcross", json!({"range": "A1:B2"})).unwrap();
        let v = |s: &mut crate::Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["value"].clone();
        assert_eq!(v(&mut s, "B1"), Json::Null);
        assert_eq!(v(&mut s, "B2"), Json::Null);
        assert_eq!(v(&mut s, "D1"), json!(4.0));
    }

    #[test]
    fn the_default_symbol_reproduces_the_accounting_format() {
        assert_eq!(accounting_format("$"), ACCOUNTING);
    }

    #[test]
    fn decimal_steps() {
        let v = gridcraft_core::Value::Number(1.5);
        assert_eq!(adjust_decimals("General", &v, 1), "0.00");
        assert_eq!(adjust_decimals("General", &v, -1), "0");
        assert_eq!(adjust_decimals("0", &v, 1), "0.0");
        assert_eq!(adjust_decimals("0.00", &v, -1), "0.0");
        assert_eq!(adjust_decimals("0.0", &v, -1), "0");
        assert_eq!(adjust_decimals("#,##0.00;[Red]-#,##0.00", &v, 1), "#,##0.000;[Red]-#,##0.000");
        assert_eq!(adjust_decimals("0%", &v, 1), "0.0%");
    }

    #[test]
    fn format_cells_takes_the_documented_keys() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "5"})).unwrap();
        s.execute("home.formatCells", json!({"range": "A1", "style": {"numFmt": "0.0", "borders": {"diagUp": {"style": "Thin"}}}})).unwrap();
        let st = &s.execute("cell.get", json!({"cell": "A1"})).unwrap()["style"];
        assert_eq!(st["num_fmt"], "0.0");
        assert_eq!(st["borders"]["diag_up"]["style"], "Thin");
        // The style's own snake_case keys keep working.
        s.execute("home.formatCells", json!({"range": "A1", "style": {"num_fmt": "0.00"}})).unwrap();
        assert_eq!(s.execute("cell.get", json!({"cell": "A1"})).unwrap()["style"]["num_fmt"], "0.00");
    }

    #[test]
    fn width_units() {
        assert_eq!(chars_to_points(8.43), 64.0);
        assert!((points_to_chars(64.0) - 8.43).abs() < 0.01);
    }
}
