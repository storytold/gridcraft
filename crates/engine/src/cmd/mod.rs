//! The command registry. Ids follow Excel's ribbon (`home.bold`, `insert.chart`,
//! `data.sortAscending`) plus primitive editing commands (`cell.set`, `selection.set`).

pub mod analysis;
pub mod data;
pub mod draw;
pub mod edit;
pub mod extra;
pub mod file;
pub mod forecast;
pub mod format;
pub mod formulas;
pub mod insert;
pub mod inspect;
pub mod pivot;
pub mod print;
pub mod review;
pub mod sheet;
pub mod slicer;
pub mod solver;
pub mod spelling;
pub mod view;
pub mod whatif;

use gridcraft_calc::Key;
use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::Workbook;
use serde::Serialize;
use serde_json::Value as Json;

use crate::{DocState, EngineError, Result, Selection, Session};

pub type Run = fn(&mut Session, &Json) -> Result<Json>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Ribbon/menu placement, e.g. `["Home", "Font"]`. Empty = not in menus.
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    pub journal: bool,
    pub undoable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

impl CommandSpec {
    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            menu: self.menu.to_vec(),
            shortcut: self.shortcut,
            params: self.params,
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

/// Params that hold a secret: `password`, or a name ending in `Password`.
pub fn is_secret_param(key: &str) -> bool {
    key == "password" || key.ends_with("Password")
}

/// The params as the journal records them: secret params are left out, so a password never
/// reaches the journal dialog, `engine.journal` or a recorded script (saved in the workbook).
/// Replaying such an entry runs the command without that param.
pub fn journal_params(params: &Json) -> Json {
    match params {
        Json::Object(o) => Json::Object(o.iter().filter(|(k, _)| !is_secret_param(k)).map(|(k, v)| (k.clone(), v.clone())).collect()),
        other => other.clone(),
    }
}

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}
pub fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no workbook open".into())
}
pub fn can_undo(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| !d.undo.is_empty()) { Ok(()) } else { Err("nothing to undo".into()) }
}
pub fn can_redo(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| !d.redo.is_empty()) { Ok(()) } else { Err("nothing to redo".into()) }
}
pub fn has_clipboard(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.clipboard.is_some() { Ok(()) } else { Err("the clipboard is empty".into()) }
}

macro_rules! cmd {
    ($id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: true, undoable: true }
    };
    (query $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: false, undoable: false }
    };
    (noundo $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: true, undoable: false }
    };
}
pub(crate) use cmd;

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(file::specs());
        v.extend(edit::specs());
        v.extend(format::specs());
        v.extend(sheet::specs());
        v.extend(insert::specs());
        v.extend(data::specs());
        v.extend(formulas::specs());
        v.extend(review::specs());
        v.extend(view::specs());
        v.extend(inspect::specs());
        v.extend(extra::specs());
        v.extend(draw::specs());
        v.extend(spelling::specs());
        v.extend(solver::specs());
        v.extend(pivot::specs());
        v.extend(slicer::specs());
        v.extend(whatif::specs());
        v.extend(forecast::specs());
        v.extend(analysis::specs());
        v.extend(print::specs());
        v
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    if let Some(spec) = command_specs().iter().find(|c| c.id == id) {
        return Some(spec);
    }
    // Commands registered by UI integration tests (pending keytips go live
    // the moment their command lands, with no table edit).
    test_commands().iter().find(|c| c.id == id).copied()
}

static TEST_COMMANDS: std::sync::Mutex<Vec<&'static CommandSpec>> = std::sync::Mutex::new(Vec::new());

/// Test-only: register an extra command so UI integration tests can prove a pending
/// keytip becomes live the moment its command lands (see crates/ui-egui/tests/keytips.rs).
/// Not part of the public API; harmless no-op for production callers who never call it.
#[doc(hidden)]
pub fn register_test_command(id: &'static str, run: Run) {
    // Leaked intentionally: test commands live for the whole process.
    let spec: &'static CommandSpec = Box::leak(Box::new(CommandSpec {
        id,
        label: id,
        menu: &[],
        shortcut: None,
        params: "",
        enabled: |_| Ok(()),
        run,
        journal: true,
        undoable: false,
    }));
    test_commands().push(spec);
}

fn test_commands() -> std::sync::MutexGuard<'static, Vec<&'static CommandSpec>> {
    #[allow(clippy::unwrap_used)] // a poisoned test registry has no meaningful recovery
    match TEST_COMMANDS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

// ---------------------------------------------------------------- param helpers

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}
pub(crate) fn str_param<'a>(p: &'a Json, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Json::as_str)
}
pub(crate) fn f64_param(p: &Json, key: &str) -> Option<f64> {
    p.get(key).and_then(Json::as_f64)
}
pub(crate) fn bool_param(p: &Json, key: &str) -> Option<bool> {
    p.get(key).and_then(Json::as_bool)
}
pub(crate) fn u32_param(p: &Json, key: &str) -> Option<u32> {
    p.get(key).and_then(Json::as_u64).map(|v| v.min(u32::MAX as u64) as u32)
}

/// An object anchor mode from Excel's wording or the model's: `"moveAndSize"`/`"twoCell"`,
/// `"moveOnly"`/`"oneCell"`, `"absolute"`/`"don't move or size"` (case-insensitive).
pub(crate) fn anchor_mode_param(p: &Json) -> Option<gridcraft_model::AnchorMode> {
    parse_anchor_mode(str_param(p, "mode").or_else(|| str_param(p, "anchorMode"))?)
}

pub(crate) fn parse_anchor_mode(s: &str) -> Option<gridcraft_model::AnchorMode> {
    match s.to_ascii_lowercase().replace([' ', '-', '_', '\''], "").as_str() {
        "moveandsize" | "moveandsizeandcells" | "twocell" => Some(gridcraft_model::AnchorMode::MoveAndSize),
        "moveonly" | "movebutdontsize" | "movebutdontsizewithcells" | "onecell" => Some(gridcraft_model::AnchorMode::MoveOnly),
        "absolute" | "dontmoveorsize" | "dontmoveorsizewithcells" | "none" => Some(gridcraft_model::AnchorMode::Absolute),
        _ => None,
    }
}

/// The wire name of an anchor mode (matches the `mode` param of [`anchor_mode_param`]).
pub(crate) fn anchor_mode_name(m: gridcraft_model::AnchorMode) -> &'static str {
    match m {
        gridcraft_model::AnchorMode::MoveAndSize => "moveAndSize",
        gridcraft_model::AnchorMode::MoveOnly => "moveOnly",
        gridcraft_model::AnchorMode::Absolute => "absolute",
    }
}
pub(crate) fn ok() -> Result<Json> {
    Ok(Json::Null)
}

/// A range parameter (`"A1:B2"`, `"Sheet2!A1"`, `"B:B"`), defaulting to the current selection's
/// areas.
pub(crate) fn target_ranges(s: &Session, p: &Json) -> Result<Vec<RangeRef>> {
    if let Some(r) = str_param(p, "range").or_else(|| str_param(p, "cell")) {
        let (_, body) = split_sheet(r);
        let mut out = Vec::new();
        for part in body.split(',') {
            out.push(RangeRef::parse(part).ok_or_else(|| bad("range", format!("not a range: `{part}`")))?);
        }
        return Ok(out);
    }
    Ok(s.doc()?.selection.ranges.clone())
}

pub(crate) fn target_range(s: &Session, p: &Json) -> Result<RangeRef> {
    Ok(target_ranges(s, p)?.first().copied().unwrap_or_default())
}

/// The sheet a call names (`sheet` param or a `Sheet!` prefix on `range`), else the active one.
pub(crate) fn target_sheet(s: &Session, p: &Json) -> Result<usize> {
    let d = s.doc()?;
    if let Some(v) = p.get("sheet") {
        if let Some(i) = v.as_u64() {
            return if (i as usize) < d.wb.sheets.len() { Ok(i as usize) } else { Err(bad("sheet", "no such sheet")) };
        }
        if let Some(n) = v.as_str() {
            return d.wb.sheet_index(n).ok_or_else(|| bad("sheet", format!("no sheet named `{n}`")));
        }
    }
    if let Some(r) = str_param(p, "range").or_else(|| str_param(p, "cell"))
        && let (Some(sh), _) = split_sheet(r)
    {
        return d.wb.sheet_index(&sh).ok_or_else(|| bad("range", format!("no sheet named `{sh}`")));
    }
    Ok(d.wb.active_sheet)
}

/// `'My Sheet'!A1` → (`Some("My Sheet")`, `A1`).
pub(crate) fn split_sheet(r: &str) -> (Option<String>, &str) {
    match r.rsplit_once('!') {
        Some((sh, body)) => (Some(sh.trim_matches('\'').replace("''", "'")), body),
        None => (None, r),
    }
}

/// A single-cell parameter. Absent or null is `Ok(None)` (callers default it); a value that is not
/// a cell reference, or that names a sheet other than the active one, is an error rather than a
/// silent fallback to some other cell.
pub(crate) fn cell_param(s: &Session, p: &Json, key: &str) -> Result<Option<CellRef>> {
    cell_param_on(s, p, key, s.doc()?.wb.active_sheet)
}

/// Like [`cell_param`], for a command that operates on `sheet` (a `Sheet!` prefix must name it).
pub(crate) fn cell_param_on(s: &Session, p: &Json, key: &str, sheet: usize) -> Result<Option<CellRef>> {
    let t = match p.get(key) {
        None | Some(Json::Null) => return Ok(None),
        Some(Json::String(t)) => t,
        Some(v) => return Err(bad("cell", format!("invalid cell reference `{v}` for `{key}`"))),
    };
    let (sh, body) = split_sheet(t);
    let c = CellRef::parse(body).ok_or_else(|| bad("cell", format!("invalid cell reference `{t}` for `{key}`")))?;
    if let Some(name) = sh
        && s.doc()?.wb.sheet_index(&name) != Some(sheet)
    {
        return Err(bad("cell", format!("`{t}` for `{key}` names a sheet other than the one this command works on")));
    }
    Ok(Some(c))
}

/// Applies an edit to a copy of the active workbook, then recalculates and commits it.
/// `changed` collects cells whose content changed; set `structural` for row/column/sheet edits
/// (full recalc).
pub struct Ctx<'a> {
    pub wb: Workbook,
    pub changed: Vec<Key>,
    pub structural: bool,
    pub sel: &'a mut Selection,
    /// Rows whose automatic height should be recomputed (sheet, row).
    pub fit_rows: Vec<(usize, u32)>,
    /// The command applied sheet protection itself (row/column inserts and deletes the
    /// protection allows), so cells it shifts aren't refused as edits of locked cells.
    pub protection_checked: bool,
}

pub(crate) fn edit<R>(s: &mut Session, f: impl FnOnce(&mut Ctx) -> Result<R>) -> Result<R> {
    let d = s.doc_mut()?;
    commit(d, |cx| {
        let r = f(cx)?;
        crate::tables::sync_headers(cx);
        Ok(r)
    })
}

pub(crate) fn commit<R>(d: &mut DocState, f: impl FnOnce(&mut Ctx) -> Result<R>) -> Result<R> {
    // `GRIDCRAFT_PROFILE` timing. Read the clock only when profiling, and never on wasm: there
    // `Instant::now` traps (`RuntimeError: unreachable`), which broke every edit in the web app.
    #[allow(clippy::disallowed_methods)]
    let t0 = if cfg!(target_arch = "wasm32") { None } else { std::env::var_os("GRIDCRAFT_PROFILE").map(|_| std::time::Instant::now()) };
    let mut sel = d.selection.clone();
    let mut ctx = Ctx { wb: (*d.wb).clone(), changed: Vec::new(), structural: false, sel: &mut sel, fit_rows: Vec::new(), protection_checked: false };
    let r = f(&mut ctx)?;
    let Ctx { mut wb, mut changed, structural, mut fit_rows, protection_checked, .. } = ctx;
    if !protection_checked {
        check_protection(&d.wb, &wb, &d.unlocked_ranges)?;
    }
    if !structural {
        check_arrays(&d.wb, &wb)?;
    }
    if wb.calc.precision_as_displayed {
        round_edited_as_displayed(&d.wb, &mut wb, &mut changed);
    }
    if changed.len() <= 200_000 {
        fit_rows.extend(changed.iter().map(|(s, c)| (*s, c.row)));
    }
    if let Some(t0) = t0 {
        eprintln!("commit: edit {:?}, {} changed", t0.elapsed(), changed.len());
    }
    if structural {
        d.calc.recalc_all(&mut wb);
    } else if !changed.is_empty() {
        d.calc.cells_changed(&mut wb, &changed);
    }
    if let Some(t0) = t0 {
        eprintln!("commit: + recalc {:?}", t0.elapsed());
    }
    fit_rows.sort_unstable();
    fit_rows.dedup();
    for (si, row) in fit_rows {
        auto_row_height(&mut wb, si, row);
    }
    if let Some(t0) = t0 {
        eprintln!("commit: + rows {:?}", t0.elapsed());
    }
    d.wb = std::sync::Arc::new(wb);
    d.selection = sel;
    Ok(r)
}

/// "Set precision as displayed": a number typed, pasted or reformatted is stored as its format
/// shows it (Excel rounds a constant when it is entered and when its cell's format changes,
/// observed through COM). Only cells the edit touched are visited, so this costs nothing for
/// the rest of the workbook; formula cells whose format changed are recalculated, and the
/// recalculation rounds them.
fn round_edited_as_displayed(old: &Workbook, wb: &mut Workbook, changed: &mut Vec<gridcraft_calc::Key>) {
    let mut prec = gridcraft_calc::DisplayedPrecision::new();
    for si in 0..wb.sheets.len() {
        let Some(after) = wb.sheets.get(si) else { continue };
        // Sheets are matched by name; a new sheet holds cells copied from rounded ones.
        let Some(before) = old.sheet_index(&after.name).and_then(|i| old.sheets.get(i)) else { continue };
        if std::sync::Arc::ptr_eq(before, after) {
            continue;
        }
        let mut updates = Vec::new();
        for c in before.cells.diff(&after.cells) {
            let Some(cell) = after.cell(c) else { continue };
            if cell.formula.is_some() {
                changed.push((si, c));
            } else if let gridcraft_core::Value::Number(n) = cell.value {
                let r = prec.round(wb, si, c, n);
                if r.to_bits() != n.to_bits() {
                    updates.push((c, r));
                }
            }
        }
        if updates.is_empty() {
            continue;
        }
        let Some(sh) = wb.sheet_mut(si) else { continue };
        for (c, r) in updates {
            if let Some(cell) = sh.cells.get_mut(c) {
                cell.value = gridcraft_core::Value::Number(r);
                changed.push((si, c));
            }
        }
    }
}

/// Turning "Set precision as displayed" on rounds every number constant in the workbook to its
/// format, permanently (Excel warns "Data will permanently lose accuracy"). Formulas are rounded
/// by the recalculation that follows.
pub(crate) fn round_all_constants_as_displayed(wb: &mut Workbook) {
    let mut prec = gridcraft_calc::DisplayedPrecision::new();
    for si in 0..wb.sheets.len() {
        let Some(sh) = wb.sheets.get(si) else { continue };
        let updates: Vec<(CellRef, f64)> = sh
            .cells
            .iter()
            .filter_map(|(c, cell)| match cell.value {
                gridcraft_core::Value::Number(n) if cell.formula.is_none() => Some((c, n)),
                _ => None,
            })
            .filter_map(|(c, n)| {
                let r = prec.round(wb, si, c, n);
                (r.to_bits() != n.to_bits()).then_some((c, r))
            })
            .collect();
        if updates.is_empty() {
            continue;
        }
        let Some(sh) = wb.sheet_mut(si) else { continue };
        for (c, r) in updates {
            if let Some(cell) = sh.cells.get_mut(c) {
                cell.value = gridcraft_core::Value::Number(r);
            }
        }
    }
}

/// Excel's message for an edit of a locked cell on a protected sheet.
pub(crate) const PROTECTED: &str = "The cell or chart you're trying to change is on a protected sheet.";

/// Refuses an edit that changed what a locked cell on a protected sheet holds (its value or
/// formula), whichever command made it. Sheets are matched by name, so moving, renaming or
/// deleting a sheet isn't an edit of its cells.
fn check_protection(old: &Workbook, new: &Workbook, unlocked: &[(String, String)]) -> Result<()> {
    for sh in &old.sheets {
        if !sh.is_protected() {
            continue;
        }
        let Some(after) = new.sheet_index(&sh.name).and_then(|i| new.sheets.get(i)) else { continue };
        if std::sync::Arc::ptr_eq(sh, after) || !after.is_protected() {
            continue;
        }
        for c in sh.cells.diff(&after.cells) {
            if !same_content(sh.cell(c), after.cell(c))
                && old.styles.get(sh.style_id(c)).protection.locked
                && let Some(m) = locked_cell_error(sh, c, unlocked)
            {
                return Err(EngineError::Other(m));
            }
        }
    }
    Ok(())
}

/// Whether a locked cell on a protected sheet stays locked: `None` when it's in an Allow Edit
/// Range without a password or one unlocked in this session, else the message to show.
pub(crate) fn locked_cell_error(sh: &gridcraft_model::Sheet, c: CellRef, unlocked: &[(String, String)]) -> Option<String> {
    let mut needs_password = None;
    for r in sh.edit_ranges.iter().filter(|r| r.contains(c)) {
        if r.password.is_none() || unlocked.iter().any(|(s, t)| *s == sh.name && *t == r.title) {
            return None;
        }
        needs_password.get_or_insert(r.title.as_str());
    }
    Some(match needs_password {
        Some(t) => format!("The range \"{t}\" needs its password before it can be changed (Review › Allow Edit Ranges)."),
        None => PROTECTED.into(),
    })
}

/// Refuses an edit that put something into a cell of a multi-cell legacy array other than its
/// formula cell, while the array stays (Excel: "You can't change part of an array."). The array
/// would show its value there while the file kept the new content. Cells shifted by row and
/// column edits aren't edits of their content, so structural commits aren't checked.
fn check_arrays(old: &Workbook, new: &Workbook) -> Result<()> {
    for sh in &old.sheets {
        if sh.spill.is_empty() {
            continue;
        }
        let Some(after) = new.sheet_index(&sh.name).and_then(|i| new.sheets.get(i)) else { continue };
        if std::sync::Arc::ptr_eq(sh, after) {
            continue;
        }
        for c in sh.cells.diff(&after.cells) {
            let filled = after.cell(c).is_some_and(|x| x.formula.is_some() || !x.value.is_empty());
            if !filled || !sh.spill.contains_key(&c) || same_content(sh.cell(c), after.cell(c)) {
                continue;
            }
            let in_array = sh.spill_ranges.keys().any(|&anchor| {
                anchor != c
                    && after.cell(anchor).and_then(|x| x.formula.as_ref()).and_then(|f| f.array).is_some_and(|a| a.start == anchor && a.contains(c))
            });
            if in_array {
                return Err(EngineError::Other("You can't change part of an array.".into()));
            }
        }
    }
    Ok(())
}

/// Two cells hold the same value or formula (formats aside).
fn same_content(a: Option<&gridcraft_model::Cell>, b: Option<&gridcraft_model::Cell>) -> bool {
    let empty = |c: Option<&gridcraft_model::Cell>| c.is_none_or(|c| c.formula.is_none() && c.value.is_empty());
    match (a, b) {
        (Some(a), Some(b)) => match (&a.formula, &b.formula) {
            (Some(x), Some(y)) => x.text == y.text && x.array == y.array,
            (None, None) => a.value == b.value,
            _ => false,
        },
        _ => empty(a) && empty(b),
    }
}

/// Recomputes an automatic row height from its content: the largest font, and wrapped or
/// multi-line text. Rows with a user-set height keep it.
pub fn auto_row_height(wb: &mut Workbook, si: usize, row: u32) {
    let Some(sh) = wb.sheet(si) else { return };
    if sh.rows.get(&row).is_some_and(|i| i.custom) {
        return;
    }
    let default = sh.default_row_height;
    // Fast path: rows of default-size, unwrapped, single-line content keep the default height.
    let plain = sh.cells.row(row, 0, gridcraft_core::MAX_COLS - 1).all(|(_, cell)| {
        let st = wb.styles.get(cell.style);
        st.font.size <= gridcraft_model::DEFAULT_FONT_SIZE && !st.align.wrap && !cell.value.as_text().is_some_and(|t| t.contains('\n'))
    });
    if plain {
        if sh.rows.get(&row).is_some_and(|i| i.size.is_some())
            && let Some(shm) = wb.sheet_mut(si)
            && let Some(e) = shm.rows.get_mut(&row)
        {
            {
                e.size = None;
                if *e == gridcraft_model::LineInfo::default() {
                    shm.rows.remove(&row);
                }
            }
        }
        return;
    }
    let mut need: f32 = default;
    for (c, cell) in sh.cells.row(row, 0, gridcraft_core::MAX_COLS - 1) {
        let st = wb.styles.get(cell.style);
        let size = st.font.size;
        let line = (size * 1.8).round().max(default * size / gridcraft_model::DEFAULT_FONT_SIZE);
        let mut lines = 1.0f32;
        if !cell.value.is_empty() || cell.formula.is_some() {
            let text = crate::display::cell_text(wb, sh, CellRef::new(row, c));
            let explicit = text.lines().count().max(1) as f32;
            lines = if st.align.wrap {
                let w = sh.col_width(c).max(1.0) - 6.0;
                text.lines()
                    .map(|l| (crate::cmd::format::approx_text_width(l, size, st.font.bold) / w.max(1.0)).ceil().max(1.0))
                    .sum::<f32>()
                    .max(1.0)
            } else {
                explicit
            };
        } else if st.font.size <= gridcraft_model::DEFAULT_FONT_SIZE {
            continue;
        }
        let h = if lines > 1.0 { (size * 96.0 / 72.0 * 1.22 * lines + 5.0).ceil() } else { line };
        need = need.max(h.min(546.0));
    }
    let Some(shm) = wb.sheet_mut(si) else { return };
    let e = shm.rows.entry(row).or_default();
    e.size = if (need - default).abs() < 0.5 { None } else { Some(need) };
    if *e == gridcraft_model::LineInfo::default() {
        shm.rows.remove(&row);
    }
}

impl Ctx<'_> {
    pub fn sheet_index(&self) -> usize {
        self.wb.active_sheet
    }
    pub fn sheet_mut(&mut self, i: usize) -> Result<&mut gridcraft_model::Sheet> {
        self.wb.sheet_mut(i).ok_or_else(|| EngineError::Other("no such sheet".into()))
    }
    pub fn touch(&mut self, sheet: usize, c: CellRef) {
        self.changed.push((sheet, c));
    }
}
