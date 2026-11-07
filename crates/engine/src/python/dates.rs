//! Dates and times in Python cells. The worker never sees a serial number: only here is the
//! workbook's date system known, so a cell's number becomes a date here when its format says it is
//! one, and a result's dates become serials here. The format a result's date asks for is written
//! to its cell when the cell has none of its own, and goes again when the date does.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::date::DateSystem;
use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::{Cell, NumFmt, PyCode, PyDateKind, PyOutput, Sheet, StyleId, Workbook};
use gridcraft_numfmt::NumberFormat;
use gridcraft_python::{Dated, Stamp};

use crate::DocState;

/// The date formats among a workbook's styles, so that reading a column of numbers parses no format
/// more than once.
#[derive(Default)]
pub(super) struct DateFormats(HashMap<StyleId, Option<NumberFormat>>);

impl DateFormats {
    /// What a number in a cell of this style stands for, or `None` for a plain number. An elapsed
    /// format such as `[h]:mm` shows a length of time, not a moment, so it stays a number of days.
    pub fn stamp(&mut self, wb: &Workbook, style: StyleId, n: f64) -> Option<Stamp> {
        let format = self
            .0
            .entry(style)
            .or_insert_with(|| Some(crate::display::number_format(wb.styles.get(style).num_fmt.as_str())).filter(NumberFormat::is_date));
        Stamp::from_serial(wb.date_system, format.as_ref()?.date_shape(n)?, n)
    }
}

/// The numbers of a result that are dates, by their place in it.
pub(super) type ResultDates = Arc<[(u32, PyDateKind)]>;

/// A result with dates in it, as a cell keeps it: its dates are serial numbers in `sys`, or `#NUM!`
/// where a serial can't hold one, with the format each number asks for.
pub(super) fn converted(dated: Dated, sys: DateSystem) -> (PyOutput, ResultDates) {
    let Dated { mut value, stamps } = dated;
    let number = |stamp: Stamp| stamp.serial(sys).map_or_else(Value::Error, Value::Number);
    if let Value::Array(a) = &mut value {
        let (rows, cols) = (a.rows, a.cols);
        let array = Arc::make_mut(a);
        for (at, stamp) in &stamps {
            if let Some(slot) = array.data.get_mut(*at as usize) {
                *slot = number(*stamp);
            }
        }
        let output =
            PyOutput { value, type_name: "list".into(), preview: format!("{rows} rows, {cols} columns"), convertible: true, ..Default::default() };
        return (output, kinds_by_column(&stamps, sys, cols));
    }
    match stamps.first() {
        Some((_, stamp)) => (
            PyOutput {
                value: number(*stamp),
                type_name: stamp.type_name().into(),
                preview: stamp.preview(),
                convertible: true,
                ..Default::default()
            },
            stamps.iter().filter_map(|(at, stamp)| Some((*at, kind_of(*stamp, sys)?))).collect(),
        ),
        None => (
            PyOutput {
                value: Value::Error(CellError::Value),
                type_name: "error".into(),
                preview: String::new(),
                convertible: true,
                ..Default::default()
            },
            ResultDates::default(),
        ),
    }
}

/// The format a stamp's cell asks for. An error has none, and neither has a negative length of time:
/// there is no format that shows one (Excel fills the cell with `#`), so it stays a number of days.
fn kind_of(stamp: Stamp, sys: DateSystem) -> Option<PyDateKind> {
    stamp.serial(sys).ok()?;
    match stamp {
        Stamp::Date(_) => Some(PyDateKind::Date),
        Stamp::DateTime(..) => Some(PyDateKind::DateTime),
        Stamp::Time(_) => Some(PyDateKind::Time),
        Stamp::Duration(days) => (days >= 0.0).then_some(PyDateKind::Duration),
    }
}

/// The formats of an array. A column where some dates have times shows them all with their times,
/// rather than some with and some without.
fn kinds_by_column(stamps: &[(u32, Stamp)], sys: DateSystem, cols: usize) -> ResultDates {
    let mut kinds: Vec<(u32, PyDateKind)> = stamps.iter().filter_map(|(at, stamp)| Some((*at, kind_of(*stamp, sys)?))).collect();
    let column = |at: u32| (at as usize).checked_rem(cols);
    let mut with_time = vec![false; cols];
    for (at, kind) in &kinds {
        if *kind == PyDateKind::DateTime
            && let Some(flag) = column(*at).and_then(|c| with_time.get_mut(c))
        {
            *flag = true;
        }
    }
    for (at, kind) in &mut kinds {
        if *kind == PyDateKind::Date && column(*at).and_then(|c| with_time.get(c)).is_some_and(|flag| *flag) {
            *kind = PyDateKind::DateTime;
        }
    }
    kinds.into()
}

/// The type names [`converted`] gives a single date, for saying why a cell shows `#NUM!`.
pub(super) fn is_date_type(name: &str) -> bool {
    matches!(name, "date" | "datetime" | "time" | "timedelta")
}

/// Whether a Python cell's result is on the sheet. With no result, or a spill something is in the
/// way of, its dates aren't, and a cell in the way is not to take on a date's format.
pub(super) fn laid_out(sh: &Sheet, at: CellRef, cell: &Cell, py: &PyCode) -> bool {
    match py.output.as_ref().map(|output| &output.value) {
        None => false,
        // A Python object shows its type's name.
        Some(Value::Array(_)) if py.object => false,
        Some(Value::Array(a)) => (a.data.len() == 1 || sh.spill_ranges.contains_key(&at)) && cell.value != Value::Error(CellError::Spill),
        Some(_) => true,
    }
}

/// The cells a result asks formats for, as (rows, columns) from its cell, in order.
fn wanted(sh: &Sheet, at: CellRef, cell: &Cell, py: &PyCode) -> Vec<(u32, u32, PyDateKind)> {
    if !laid_out(sh, at, cell, py) {
        return Vec::new();
    }
    match py.output.as_ref().map(|output| &output.value) {
        Some(Value::Array(a)) => {
            let cols = u32::try_from(a.cols).unwrap_or(u32::MAX).max(1);
            py.dates.result.iter().map(|(place, kind)| (place / cols, place % cols, *kind)).collect()
        }
        _ => py.dates.result.iter().take(1).map(|(_, kind)| (0, 0, *kind)).collect(),
    }
}

/// What to do about a cell's format: the code to write, if any, and what the result is then to
/// remember of it. A format no result wrote is the person's and stays. A cell a result wrote a format
/// for that now shows General had it taken off, and stays that way while the result has a date there.
fn decide(code: &str, written: Option<PyDateKind>, wanted: Option<PyDateKind>) -> (Option<&'static str>, Option<PyDateKind>) {
    let ours = written.is_some_and(|kind| kind.format_code() == code);
    let general = code.is_empty() || code.eq_ignore_ascii_case("General");
    match wanted {
        Some(_) if general && written.is_some() && !ours => (None, written),
        Some(kind) if ours || general => ((kind.format_code() != code).then_some(kind.format_code()), Some(kind)),
        None if ours => (Some("General"), None),
        _ => (None, None),
    }
}

/// Walks two lists in (rows, columns) order, pairing the entries for one place.
fn each_place(
    written: &[(u32, u32, PyDateKind)],
    wanted: &[(u32, u32, PyDateKind)],
    f: &mut dyn FnMut((u32, u32), Option<PyDateKind>, Option<PyDateKind>),
) {
    let (mut was, mut want) = (written.iter().peekable(), wanted.iter().peekable());
    loop {
        match (was.peek().map(|e| (e.0, e.1)), want.peek().map(|e| (e.0, e.1))) {
            (None, None) => return,
            (Some(a), Some(b)) if a == b => f(a, was.next().map(|e| e.2), want.next().map(|e| e.2)),
            (Some(a), b) if b.is_none_or(|b| a < b) => f(a, was.next().map(|e| e.2), None),
            (_, Some(b)) => f(b, None, want.next().map(|e| e.2)),
            (Some(a), None) => f(a, was.next().map(|e| e.2), None),
        }
    }
}

/// What it takes to bring a cell's formats in line with its result.
struct FormatChanges {
    /// The formats to write.
    writes: Vec<(CellRef, &'static str)>,
    /// What the result will then have written.
    written: Vec<(u32, u32, PyDateKind)>,
}

/// The changes a cell's formats need, or `None` when they are in line with its result.
fn changes(wb: &Workbook, sh: &Sheet, at: CellRef) -> Option<FormatChanges> {
    let cell = sh.cell(at)?;
    let py = cell.py.as_deref()?;
    let want = wanted(sh, at, cell, py);
    if want.is_empty() && py.dates.written.is_empty() {
        return None;
    }
    let (mut writes, mut written) = (Vec::new(), Vec::with_capacity(want.len()));
    each_place(&py.dates.written, &want, &mut |(rows, cols), was, wanted| {
        // A cell moved with its result can leave places past the edge of the sheet.
        let Some(c) = at.offset(i64::from(rows), i64::from(cols)) else { return };
        let (write, kept) = decide(wb.styles.get(sh.style_id(c)).num_fmt.as_str(), was, wanted);
        writes.extend(write.map(|code| (c, code)));
        written.extend(kept.map(|kind| (rows, cols, kind)));
    });
    (!writes.is_empty() || *py.dates.written != *written).then_some(FormatChanges { writes, written })
}

/// Brings the formats of a Python cell's result in line with what it holds now. A result's date
/// gives its cell a format when the cell has none of its own, and a format a result wrote goes with
/// the date it was written for. Like the result, this makes no undo step. Returns whether it wrote.
pub(super) fn sync_formats(d: &mut DocState, sheet: usize, at: CellRef) -> bool {
    let Some(FormatChanges { writes, written }) = d.wb.sheet(sheet).and_then(|sh| changes(&d.wb, sh, at)) else { return false };
    let done = crate::cmd::commit(d, |cx| {
        let mut derived: HashMap<(StyleId, &str), StyleId> = HashMap::new();
        for (c, code) in &writes {
            let style = cx.sheet_mut(sheet)?.style_id(*c);
            let new = *derived.entry((style, code)).or_insert_with(|| cx.wb.styles.derive(style, |s| s.num_fmt = NumFmt::new(code)));
            cx.sheet_mut(sheet)?.set_style(*c, new);
        }
        if let Some(py) = cx.sheet_mut(sheet)?.cells.get_mut(at).and_then(|c| c.py.as_mut()) {
            Arc::make_mut(py).dates.written = written.into();
        }
        Ok(())
    });
    if let Err(e) = done {
        log::warn!("Python cells: {e}");
        return false;
    }
    d.revision = d.revision.saturating_add(1);
    true
}
