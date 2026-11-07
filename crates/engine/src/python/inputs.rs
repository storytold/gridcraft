//! What a Python cell reads: each literal `xl("…")` in its code, resolved before the cell runs (the
//! worker never calls back), and the fingerprint that says whether any of it changed.
//!
//! A reference is resolved as a formula's would be, from the Python cell: a cell, a range, whole
//! columns or rows, a formula's spill (`A1#`), on any sheet, a defined name or a table. A range running to the sheet's edge
//! (`A:A`, `A5:A1048576`) stops at the sheet's used range, as formulas read it. A cell waiting for a
//! new result reads as the result it keeps, so a cell that reads one further down settles instead of
//! seeing `#BUSY!` each time it runs. A number whose format is a date or a time goes as that date or
//! time, in the workbook's date system, and the fingerprint counts which it is, so a new format runs
//! what reads the cell.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use gridcraft_calc::eval::Ev;
use gridcraft_core::{Array, CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_formula::{BinOp, Expr, UnOp};
use gridcraft_model::python::xl_calls;
use gridcraft_model::{Cell, Sheet, StyleId, Workbook};
use gridcraft_python::Input;
use gridcraft_python::protocol::MAX_ARRAY_CELLS;

use super::PythonStats;
use super::dates::DateFormats;

/// One literal `xl("…")` of a cell's code.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Lit {
    /// The reference as the code spells it, which is the key the worker looks it up by.
    pub text: String,
    pub target: Target,
}

/// What a literal reference names.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Area {
        sheet: usize,
        range: RangeRef,
    },
    /// A defined name that holds a value rather than a reference.
    Constant(Value),
    /// Nothing that can be read; the text says why.
    Bad(String),
}

/// Why a cell's inputs can't be given to it, so that it doesn't run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Problem {
    /// More cells than a request carries.
    TooLarge { cells: u64 },
    /// A reference that names nothing.
    Bad { reference: String, why: String },
}

/// The literal references of `code`, each once, in the order they first appear, as seen from the
/// cell `at` on sheet `sheet`.
pub(super) fn literals(wb: &Workbook, sheet: usize, at: CellRef, code: &str) -> Vec<Lit> {
    let mut out: Vec<Lit> = Vec::new();
    for call in xl_calls(code) {
        let Some(text) = call.reference else { continue };
        if !out.iter().any(|l| l.text == text) {
            let target = target(wb, sheet, at, &text);
            out.push(Lit { text, target });
        }
    }
    out
}

/// Whether reading `code`'s references depends on more than the cells they name: names and tables
/// can come to name other cells.
pub(super) fn is_volatile(code: &str) -> bool {
    xl_calls(code).iter().filter_map(|c| c.reference.as_deref()).any(|r| !matches!(parse_reference(r), Some(Expr::Ref(_))))
}

fn parse_reference(text: &str) -> Option<Expr> {
    gridcraft_formula::parse(text.trim().trim_start_matches('=')).ok()
}

fn target(wb: &Workbook, sheet: usize, at: CellRef, text: &str) -> Target {
    let not_a_reference = || Target::Bad("it isn't a cell, range, table or defined name".into());
    let Some(expr) = parse_reference(text) else { return not_a_reference() };
    if let Expr::Error(e) = expr {
        return Target::Bad(if e == CellError::Ref { "the cells it named were deleted".into() } else { e.as_str().to_string() });
    }
    let mut reference = true;
    expr.walk(&mut |e| {
        reference &= matches!(
            e,
            Expr::Ref(_)
                | Expr::Name(_)
                | Expr::Struct(_)
                | Expr::Paren(_)
                | Expr::Binary(BinOp::Range | BinOp::Intersect, ..)
                | Expr::Unary(UnOp::Spill, _)
        );
    });
    if !reference {
        return not_a_reference();
    }
    match gridcraft_calc::evaluate_reference(wb, sheet, at, &expr) {
        Ev::R(areas) => match areas.as_slice() {
            [a] => Target::Area { sheet: a.sheet, range: a.range },
            [] => not_a_reference(),
            _ => Target::Bad("it names more than one area".into()),
        },
        Ev::V(Value::Error(CellError::Ref)) => Target::Bad("there is no such sheet, or the cells it named were deleted".into()),
        Ev::V(Value::Error(CellError::Name)) => Target::Bad("there is no such table or defined name".into()),
        Ev::V(Value::Error(e)) => Target::Bad(format!("it gives {}", e.as_str())),
        Ev::V(v) => Target::Constant(v),
        Ev::L(_) => not_a_reference(),
    }
}

fn hash_value(v: &Value, h: &mut DefaultHasher) {
    match v {
        Value::Empty => 0u8.hash(h),
        Value::Number(n) => {
            1u8.hash(h);
            n.to_bits().hash(h);
        }
        Value::Text(t) => {
            2u8.hash(h);
            t.hash(h);
        }
        Value::Bool(b) => {
            3u8.hash(h);
            b.hash(h);
        }
        Value::Error(e) => {
            4u8.hash(h);
            e.as_str().hash(h);
        }
        Value::Array(a) => {
            5u8.hash(h);
            (a.rows, a.cols).hash(h);
            for x in &a.data {
                // An array holds scalars; one nested in it counts by its shape alone.
                match x {
                    Value::Array(inner) => (inner.rows, inner.cols).hash(h),
                    scalar => hash_value(scalar, h),
                }
            }
        }
    }
}

/// A hash of the code and the return type.
pub(super) fn text_hash(code: &str, object: bool) -> u64 {
    let mut h = DefaultHasher::new();
    code.hash(&mut h);
    object.hash(&mut h);
    h.finish()
}

/// A hash of the code without what its literal references spell, and the return type: moving the
/// cells a reference names rewrites its text, not what the code does.
pub(super) fn code_hash(code: &str, object: bool) -> u64 {
    let mut h = DefaultHasher::new();
    let mut last = 0;
    for span in xl_calls(code).into_iter().filter_map(|c| c.span) {
        code.get(last..span.start).unwrap_or_default().hash(&mut h);
        last = span.end;
    }
    code.get(last..).unwrap_or_default().hash(&mut h);
    object.hash(&mut h);
    h.finish()
}

/// What a cell holds as an input: its value, or while it waits for a new result what the one it
/// keeps shows, so that what reads it doesn't see a change when the wait ends with the same result.
fn input_value(sh: &Sheet, c: CellRef, cell: &Cell) -> Value {
    match cell.py.as_deref() {
        Some(p) if p.busy => match p.result_value() {
            Some(Value::Array(a)) if a.data.len() == 1 || sh.spill_ranges.contains_key(&c) => a.data.first().cloned().unwrap_or_default(),
            // Something is in its way.
            Some(Value::Array(_)) => Value::Error(CellError::Spill),
            Some(v) => v,
            None => Value::Error(p.failed.unwrap_or(CellError::Busy)),
        },
        _ => cell.value.clone(),
    }
}

fn shows_own_value(cell: &Cell) -> bool {
    !cell.value.is_empty() || cell.formula.is_some()
}

/// The values in `r` that aren't empty, with their positions and styles: stored cells first, then
/// spilled values no cell hides. The work follows what is stored, not the size of `r`.
fn for_each_value(sh: &Sheet, r: RangeRef, f: &mut dyn FnMut(CellRef, Value, StyleId)) {
    for (c, cell) in sh.cells.iter_range(r) {
        if shows_own_value(cell) {
            let v = input_value(sh, c, cell);
            if !v.is_empty() {
                f(c, v, cell.style);
            }
        }
    }
    for (c, v) in sh.spill.range(r.start..=r.end) {
        if r.contains(*c) && !v.is_empty() && sh.cell(*c).is_none_or(|cell| !shows_own_value(cell)) {
            f(*c, v.clone(), sh.style_id(*c));
        }
    }
}

/// Reads a workbook's cells for Python cells, remembering what several cells share: each sheet's
/// used range, and each area's hash, so that thousands of cells reading one column cost one read.
pub(super) struct Reader<'a> {
    wb: &'a Workbook,
    used: HashMap<usize, Option<RangeRef>>,
    hashes: HashMap<(usize, RangeRef), u64>,
    formats: DateFormats,
    stats: &'a mut PythonStats,
}

impl<'a> Reader<'a> {
    pub fn new(wb: &'a Workbook, stats: &'a mut PythonStats) -> Reader<'a> {
        Reader { wb, used: HashMap::new(), hashes: HashMap::new(), formats: DateFormats::default(), stats }
    }

    /// `r`, stopped at the sheet's used range where it runs to the sheet's edge.
    fn capped(&mut self, sheet: usize, r: RangeRef) -> RangeRef {
        let to_bottom = r.end.row == MAX_ROWS - 1;
        let to_right = r.end.col == MAX_COLS - 1;
        if !to_bottom && !to_right {
            return r;
        }
        let wb = self.wb;
        let used = *self.used.entry(sheet).or_insert_with(|| wb.sheet(sheet).and_then(Sheet::used_range));
        let (mut row, mut col) = (r.end.row, r.end.col);
        if to_bottom {
            row = used.map_or(r.start.row, |u| u.end.row.clamp(r.start.row, row));
        }
        if to_right {
            col = used.map_or(r.start.col, |u| u.end.col.clamp(r.start.col, col));
        }
        RangeRef::new(r.start, CellRef::new(row, col))
    }

    fn area_hash(&mut self, sheet: usize, r: RangeRef) -> u64 {
        let r = self.capped(sheet, r);
        if let Some(h) = self.hashes.get(&(sheet, r)) {
            return *h;
        }
        self.stats.areas_hashed = self.stats.areas_hashed.saturating_add(1);
        // The shape and the values, not where they are: cells that moved read the same.
        let mut h = DefaultHasher::new();
        (r.height(), r.width()).hash(&mut h);
        let (wb, formats) = (self.wb, &mut self.formats);
        if let Some(sh) = wb.sheet(sheet) {
            for_each_value(sh, r, &mut |c, v, style| {
                (c.row - r.start.row, c.col - r.start.col).hash(&mut h);
                hash_value(&v, &mut h);
                if let Value::Number(n) = v
                    && let Some(stamp) = formats.stamp(wb, style, n)
                {
                    stamp.type_name().hash(&mut h);
                }
            });
        }
        let out = h.finish();
        self.hashes.insert((sheet, r), out);
        out
    }

    /// What a cell's run depends on: its code and return type, and what its references read now.
    pub fn fingerprint(&mut self, code_hash: u64, lits: &[Lit]) -> u64 {
        let mut h = DefaultHasher::new();
        code_hash.hash(&mut h);
        for lit in lits {
            match &lit.target {
                Target::Area { sheet, range } => self.area_hash(*sheet, *range).hash(&mut h),
                Target::Constant(v) => hash_value(v, &mut h),
                Target::Bad(why) => why.hash(&mut h),
            }
        }
        h.finish()
    }

    /// The values to send with a cell's request, one per reference. Over
    /// [`MAX_ARRAY_CELLS`] cells in all, or with a reference that names nothing, the cell doesn't
    /// run; the sizes are checked before anything is read.
    pub fn inputs(&mut self, lits: &[Lit]) -> Result<BTreeMap<String, Input>, Problem> {
        let mut cells = 0u64;
        for lit in lits {
            match &lit.target {
                Target::Bad(why) => return Err(Problem::Bad { reference: lit.text.clone(), why: why.clone() }),
                Target::Area { sheet, range } => cells = cells.saturating_add(self.capped(*sheet, *range).count()),
                Target::Constant(_) => {}
            }
        }
        if cells > MAX_ARRAY_CELLS as u64 {
            return Err(Problem::TooLarge { cells });
        }
        Ok(lits.iter().map(|lit| (lit.text.clone(), self.read(lit))).collect())
    }

    fn read(&mut self, lit: &Lit) -> Input {
        match &lit.target {
            Target::Constant(v) => Input::new(v.clone()),
            Target::Bad(_) => Input::new(Value::Error(CellError::Ref)),
            Target::Area { sheet, range } => {
                let r = self.capped(*sheet, *range);
                let wb = self.wb;
                let Some(sh) = wb.sheet(*sheet) else { return Input::new(Value::Error(CellError::Ref)) };
                if r.is_single() && !lit.text.contains(':') {
                    read_one(wb, sh, r.start, &mut self.formats)
                } else {
                    read_block(wb, sh, r, &mut self.formats)
                }
            }
        }
    }
}

fn read_one(wb: &Workbook, sh: &Sheet, c: CellRef, formats: &mut DateFormats) -> Input {
    let (value, style) = match sh.cell(c).filter(|cell| shows_own_value(cell)) {
        Some(cell) => (input_value(sh, c, cell), cell.style),
        None => (sh.spill.get(&c).cloned().unwrap_or_default(), sh.style_id(c)),
    };
    if let Value::Number(n) = value
        && let Some(stamp) = formats.stamp(wb, style, n)
    {
        return Input { stamps: vec![(0, stamp)], ..Input::new(Value::Empty) };
    }
    Input::new(value)
}

fn read_block(wb: &Workbook, sh: &Sheet, r: RangeRef, formats: &mut DateFormats) -> Input {
    let (rows, cols) = (r.height() as usize, r.width() as usize);
    let mut data = vec![Value::Empty; rows.saturating_mul(cols)];
    let mut stamps = Vec::new();
    for_each_value(sh, r, &mut |c, v, style| {
        let at = (c.row - r.start.row) as usize * cols + (c.col - r.start.col) as usize;
        let stamp = if let Value::Number(n) = v { formats.stamp(wb, style, n) } else { None };
        // A request holds at most MAX_ARRAY_CELLS cells, so a place fits.
        match stamp.zip(u32::try_from(at).ok()) {
            Some((stamp, place)) => stamps.push((place, stamp)),
            None => {
                if let Some(slot) = data.get_mut(at) {
                    *slot = v;
                }
            }
        }
    });
    stamps.sort_unstable_by_key(|(place, _)| *place);
    let value = Array::new(rows, cols, data).map(Value::from).unwrap_or(Value::Error(CellError::Value));
    Input { stamps, ..Input::new(value) }
}
