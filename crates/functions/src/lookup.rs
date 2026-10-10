//! Lookup and reference functions over evaluated arrays: VLOOKUP, HLOOKUP, LOOKUP, MATCH,
//! XLOOKUP, XMATCH, INDEX, ROWS, COLUMNS, TRANSPOSE, ADDRESS.
//!
//! Reference-only behaviour (INDEX returning a reference, OFFSET, ROW…) belongs to the host.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::{Array, CellError, Value, col_to_letters, compare, round15};

use crate::criteria::{has_wildcards, lookup_equal};
use crate::util::{R, arg, array_val, as_array, has, int, num, opt_bool, opt_int, opt_num, text, text_val};
use crate::{Arg, Ctx, FnSpec};

// ---------------------------------------------------------------------------------------------
// Search helpers

/// Lookup needle: the scalar to look for. Blank looks for 0; errors propagate.
fn needle(args: &[Arg], i: usize) -> R<Value> {
    match arg(args, i)?.value.scalar() {
        Value::Error(e) => Err(e),
        Value::Empty => Ok(Value::Number(0.0)),
        v => Ok(v),
    }
}

fn same_type(a: &Value, b: &Value) -> bool {
    matches!((a, b), (Value::Number(_), Value::Number(_)) | (Value::Text(_), Value::Text(_)) | (Value::Bool(_), Value::Bool(_)))
}

/// Exact search (first match, or last when `reverse`). Text needles match wildcards when `wild`.
fn find_exact(x: &Value, hay: &[Value], wild: bool, reverse: bool) -> Option<usize> {
    let wild = wild && matches!(x, Value::Text(t) if has_wildcards(t));
    if reverse { hay.iter().rposition(|v| lookup_equal(x, v, wild)) } else { hay.iter().position(|v| lookup_equal(x, v, wild)) }
}

/// Positions in `hay` of the entries of the needle's type (what approximate searches look at).
fn positions_of_type(x: &Value, hay: &[Value]) -> Vec<usize> {
    hay.iter().enumerate().filter(|(_, v)| same_type(x, v)).map(|(i, _)| i).collect()
}

/// Binary search over the entries at `idx` (the needle's type), assuming they are sorted
/// ascending (`desc = false`) or descending. Returns the position of the last entry that is
/// `<= x` (ascending) or `>= x` (descending), like Excel's approximate match.
fn bsearch_last(x: &Value, hay: &[Value], idx: &[usize], desc: bool) -> Option<usize> {
    let (mut lo, mut hi) = (0usize, idx.len());
    // Find the first filtered position where the "passes" predicate fails.
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let v = idx.get(mid).and_then(|&i| hay.get(i))?;
        let o = compare(v, x);
        let ok = if desc { o != Ordering::Less } else { o != Ordering::Greater };
        if ok {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 { None } else { idx.get(lo - 1).copied() }
}

/// Binary search returning the first entry `>= x` (ascending) or `<= x` (descending).
fn bsearch_first(x: &Value, hay: &[Value], idx: &[usize], desc: bool) -> Option<usize> {
    let (mut lo, mut hi) = (0usize, idx.len());
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let v = idx.get(mid).and_then(|&i| hay.get(i))?;
        let o = compare(v, x);
        let before = if desc { o == Ordering::Greater } else { o == Ordering::Less };
        if before {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    idx.get(lo).copied()
}

fn column(a: &Array, c: usize) -> Vec<Value> {
    (0..a.rows).map(|r| a.get(r, c).cloned().unwrap_or(Value::Empty)).collect()
}

fn row(a: &Array, r: usize) -> Vec<Value> {
    (0..a.cols).map(|c| a.get(r, c).cloned().unwrap_or(Value::Empty)).collect()
}

fn is_vector(a: &Array) -> bool {
    a.rows == 1 || a.cols == 1
}

/// A looked-up cell value: blanks read as 0 like a reference to an empty cell.
fn cell_result(v: Value) -> Value {
    if matches!(v, Value::Empty) { Value::Number(0.0) } else { v }
}

// ---------------------------------------------------------------------------------------------
// Lookup indexes
//
// Thousands of lookups often search the same range (`VLOOKUP(x,$A$1:$B$50000,2,FALSE)` filled
// down). The host hands every formula the same shared array for such a range, so an index
// built over it once (a hash of the exact-match keys, the positions of each type for binary
// search) turns each later search into O(1) or O(log n) instead of a copy and a scan.

/// The part of an array a lookup searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Line {
    /// The whole array, which is a single row or column.
    All,
    Col(usize),
    Row(usize),
}

/// What an exact match compares, normalised so that equal keys are exactly the values
/// [`lookup_equal`] (without wildcards) calls equal: text case-folded, numbers at 15 digits.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ExactKey {
    Number(u64),
    Text(Box<str>),
    Bool(bool),
}

fn exact_key(v: &Value) -> Option<ExactKey> {
    match v {
        Value::Number(n) => {
            let r = round15(*n);
            // -0 and 0 are equal.
            Some(ExactKey::Number(if r == 0.0 { 0.0f64.to_bits() } else { r.to_bits() }))
        }
        Value::Text(t) => Some(ExactKey::Text(t.chars().flat_map(char::to_lowercase).collect::<String>().into_boxed_str())),
        Value::Bool(b) => Some(ExactKey::Bool(*b)),
        _ => None,
    }
}

/// An index over one line of an array.
#[derive(Debug)]
struct LookupIndex {
    source: Arc<Array>,
    /// The searched values when they aren't the whole of `source` (a column or row of a table).
    line: Option<Vec<Value>>,
    /// First and last position of every exact-match key.
    exact: HashMap<ExactKey, (usize, usize)>,
    numbers: Vec<usize>,
    texts: Vec<usize>,
    bools: Vec<usize>,
}

impl LookupIndex {
    fn build(source: &Arc<Array>, line: Line) -> LookupIndex {
        let line = match line {
            Line::All => None,
            Line::Col(c) => Some(column(source, c)),
            Line::Row(r) => Some(row(source, r)),
        };
        let mut ix =
            LookupIndex { source: Arc::clone(source), line, exact: HashMap::new(), numbers: Vec::new(), texts: Vec::new(), bools: Vec::new() };
        let mut exact: HashMap<ExactKey, (usize, usize)> = HashMap::new();
        let (mut numbers, mut texts, mut bools) = (Vec::new(), Vec::new(), Vec::new());
        for (i, v) in ix.values().iter().enumerate() {
            match v {
                Value::Number(_) => numbers.push(i),
                Value::Text(_) => texts.push(i),
                Value::Bool(_) => bools.push(i),
                _ => {}
            }
            if let Some(k) = exact_key(v) {
                exact.entry(k).and_modify(|e| e.1 = i).or_insert((i, i));
            }
        }
        (ix.exact, ix.numbers, ix.texts, ix.bools) = (exact, numbers, texts, bools);
        ix
    }

    fn values(&self) -> &[Value] {
        self.line.as_deref().unwrap_or(&self.source.data)
    }

    fn of_type(&self, x: &Value) -> &[usize] {
        match x {
            Value::Number(_) => &self.numbers,
            Value::Text(_) => &self.texts,
            Value::Bool(_) => &self.bools,
            _ => &[],
        }
    }
}

/// Indexes built by lookups, kept by the host for one recalculation. An array gets an index the
/// second time a lookup searches the same line of it (searched once, a scan is cheaper than
/// building one), within a budget of indexed values.
#[derive(Debug, Default)]
pub struct LookupCache {
    slots: HashMap<(usize, Line), Slot>,
    indexed: usize,
}

#[derive(Debug, Default)]
struct Slot {
    searches: u32,
    /// Holds its source array, so the address the slot is keyed by can't be reused meanwhile.
    index: Option<Arc<LookupIndex>>,
}

/// Arrays shorter than this are scanned: an index wouldn't pay for itself.
const MIN_INDEXED: usize = 32;
/// Values indexed per recalculation at most (about 1 GB of keys at the worst).
const INDEX_BUDGET: usize = 1 << 25;

impl LookupCache {
    fn index(&mut self, source: &Arc<Array>, line: Line) -> Option<Arc<LookupIndex>> {
        let len = match line {
            Line::All => source.data.len(),
            Line::Col(_) => source.rows,
            Line::Row(_) => source.cols,
        };
        if len < MIN_INDEXED {
            return None;
        }
        // The address only identifies the array while the slot holds it, which it does once it
        // has an index; before that a reused address at worst builds an index one search early.
        let slot = self.slots.entry((Arc::as_ptr(source) as usize, line)).or_default();
        if let Some(ix) = &slot.index {
            return Some(Arc::clone(ix));
        }
        slot.searches = slot.searches.saturating_add(1);
        if slot.searches < 2 || self.indexed.saturating_add(len) > INDEX_BUDGET {
            return None;
        }
        self.indexed += len;
        let ix = Arc::new(LookupIndex::build(source, line));
        slot.index = Some(Arc::clone(&ix));
        Some(ix)
    }
}

/// The values a lookup searches: borrowed or copied out of the array, or an index over it.
enum Hay<'a> {
    Plain(Cow<'a, [Value]>),
    Indexed(Arc<LookupIndex>),
}

impl Hay<'_> {
    fn values(&self) -> &[Value] {
        match self {
            Hay::Plain(v) => v,
            Hay::Indexed(ix) => ix.values(),
        }
    }

    fn typed(&self, x: &Value) -> Cow<'_, [usize]> {
        match self {
            Hay::Plain(v) => Cow::Owned(positions_of_type(x, v)),
            Hay::Indexed(ix) => Cow::Borrowed(ix.of_type(x)),
        }
    }

    fn find_exact(&self, x: &Value, wild: bool, reverse: bool) -> Option<usize> {
        let wild = wild && matches!(x, Value::Text(t) if has_wildcards(t));
        match self {
            Hay::Indexed(ix) if !wild => exact_key(x).and_then(|k| ix.exact.get(&k)).map(|&(first, last)| if reverse { last } else { first }),
            _ => find_exact(x, self.values(), wild, reverse),
        }
    }

    fn bsearch_last(&self, x: &Value, desc: bool) -> Option<usize> {
        bsearch_last(x, self.values(), &self.typed(x), desc)
    }

    fn bsearch_first(&self, x: &Value, desc: bool) -> Option<usize> {
        bsearch_first(x, self.values(), &self.typed(x), desc)
    }

    fn bsearch_exact(&self, x: &Value, desc: bool) -> Option<usize> {
        let p = self.bsearch_first(x, desc)?;
        let v = self.values().get(p)?;
        if compare(v, x) == Ordering::Equal { Some(p) } else { None }
    }
}

/// The values to search in `line` of `a` (the array `v` holds), through the host's index cache
/// when `v` is a shared array the host keeps one for.
fn hay<'a>(c: &mut dyn Ctx, v: &Value, a: &'a Array, line: Line) -> Hay<'a> {
    if let Value::Array(source) = v
        && let Some(ix) = c.lookup_cache().and_then(|cache| cache.index(source, line))
    {
        return Hay::Indexed(ix);
    }
    Hay::Plain(match line {
        Line::All => Cow::Borrowed(&a.data),
        Line::Col(col) => Cow::Owned(column(a, col)),
        Line::Row(r) => Cow::Owned(row(a, r)),
    })
}

// ---------------------------------------------------------------------------------------------
// VLOOKUP / HLOOKUP / LOOKUP / MATCH

fn vh_lookup(args: &[Arg], vertical: bool, c: &mut dyn Ctx) -> R<Value> {
    let x = needle(args, 0)?;
    let tv = &arg(args, 1)?.value;
    if let Value::Error(e) = tv {
        return Err(*e);
    }
    let table = as_array(tv);
    let idx = num(args, 2)?;
    if idx < 1.0 {
        return Err(CellError::Value);
    }
    let idx = idx.trunc() as usize - 1;
    let approx = opt_bool(args, 3, true)?;
    let len = if vertical { table.cols } else { table.rows };
    if idx >= len {
        return Err(CellError::Ref);
    }
    let keys = hay(c, tv, &table, if vertical { Line::Col(0) } else { Line::Row(0) });
    let pos = if approx { keys.bsearch_last(&x, false) } else { keys.find_exact(&x, true, false) };
    let pos = pos.ok_or(CellError::NA)?;
    let v = if vertical { table.get(pos, idx) } else { table.get(idx, pos) };
    Ok(cell_result(v.cloned().unwrap_or(Value::Empty)))
}

fn vlookup(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    vh_lookup(a, true, c)
}

fn hlookup(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    vh_lookup(a, false, c)
}

fn lookup(args: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let x = needle(args, 0)?;
    let lv = &arg(args, 1)?.value;
    if let Value::Error(e) = lv {
        return Err(*e);
    }
    let la = as_array(lv);
    if args.len() >= 3 {
        let keys = hay(c, lv, &la, if is_vector(&la) { Line::All } else { Line::Col(0) });
        let pos = keys.bsearch_last(&x, false).ok_or(CellError::NA)?;
        let rv = &arg(args, 2)?.value;
        if let Value::Error(e) = rv {
            return Err(*e);
        }
        let ra = as_array(rv);
        let v = if is_vector(&ra) {
            ra.data.get(pos).cloned()
        } else if ra.rows >= keys.values().len() {
            ra.get(pos, 0).cloned()
        } else {
            None
        };
        return v.map(cell_result).ok_or(CellError::NA);
    }
    // Array form: search the first row of a wide array, else the first column; return the
    // matching entry from the last row / column.
    if la.cols > la.rows {
        let pos = hay(c, lv, &la, Line::Row(0)).bsearch_last(&x, false).ok_or(CellError::NA)?;
        Ok(cell_result(la.get(la.rows - 1, pos).cloned().unwrap_or(Value::Empty)))
    } else {
        let pos = hay(c, lv, &la, Line::Col(0)).bsearch_last(&x, false).ok_or(CellError::NA)?;
        Ok(cell_result(la.get(pos, la.cols - 1).cloned().unwrap_or(Value::Empty)))
    }
}

fn match_fn(args: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let x = needle(args, 0)?;
    let av = &arg(args, 1)?.value;
    if let Value::Error(e) = av {
        return Err(*e);
    }
    let a = as_array(av);
    if !is_vector(&a) {
        return Err(CellError::NA);
    }
    let hay = hay(c, av, &a, Line::All);
    let mt = opt_num(args, 2, 1.0)?;
    let pos = if mt == 0.0 {
        hay.find_exact(&x, true, false)
    } else if mt > 0.0 {
        hay.bsearch_last(&x, false)
    } else {
        hay.bsearch_last(&x, true)
    };
    pos.map(|p| Value::Number((p + 1) as f64)).ok_or(CellError::NA)
}

// ---------------------------------------------------------------------------------------------
// XLOOKUP / XMATCH

/// Core XMATCH search: 0-based position in `hay`.
fn xsearch(x: &Value, hay: &Hay<'_>, match_mode: i64, search_mode: i64) -> R<Option<usize>> {
    if !matches!(match_mode, -1..=2) || !matches!(search_mode, -2 | -1 | 1 | 2) {
        return Err(CellError::Value);
    }
    if search_mode.abs() == 2 {
        let desc = search_mode == -2;
        if match_mode == 2 {
            // Wildcards are not supported with binary search: Excel falls back to exact.
            return Ok(hay.bsearch_exact(x, desc));
        }
        return Ok(match match_mode {
            0 => hay.bsearch_exact(x, desc),
            // Next smaller: last <= x (ascending) / first <= x (descending).
            -1 => {
                if desc {
                    hay.bsearch_first(x, true)
                } else {
                    hay.bsearch_last(x, false)
                }
            }
            _ => {
                if desc {
                    hay.bsearch_last(x, true)
                } else {
                    hay.bsearch_first(x, false)
                }
            }
        });
    }
    let reverse = search_mode == -1;
    if let Some(p) = hay.find_exact(x, match_mode == 2, reverse) {
        return Ok(Some(p));
    }
    if match_mode == 0 || match_mode == 2 {
        return Ok(None);
    }
    // Best candidate: the largest value below (mode -1) or smallest above (mode 1).
    let values = hay.values();
    let order: Box<dyn Iterator<Item = usize>> = if reverse { Box::new((0..values.len()).rev()) } else { Box::new(0..values.len()) };
    let mut best: Option<usize> = None;
    for i in order {
        let Some(v) = values.get(i) else { continue };
        if !same_type(x, v) {
            continue;
        }
        let o = compare(v, x);
        let candidate = if match_mode == -1 { o == Ordering::Less } else { o == Ordering::Greater };
        if !candidate {
            continue;
        }
        let better = match best.and_then(|b| values.get(b)) {
            None => true,
            Some(bv) => {
                let c = compare(v, bv);
                if match_mode == -1 { c == Ordering::Greater } else { c == Ordering::Less }
            }
        };
        if better {
            best = Some(i);
        }
    }
    Ok(best)
}

fn xlookup(args: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let x = needle(args, 0)?;
    let lv = &arg(args, 1)?.value;
    if let Value::Error(e) = lv {
        return Err(*e);
    }
    let rv = &arg(args, 2)?.value;
    if let Value::Error(e) = rv {
        return Err(*e);
    }
    let la = as_array(lv);
    let ra = as_array(rv);
    if !is_vector(&la) {
        return Err(CellError::Value);
    }
    // Orientation: a column lookup returns rows of the return array; a row lookup returns columns.
    let vertical = if la.rows == 1 && la.cols == 1 { ra.rows == 1 || ra.cols != 1 } else { la.cols == 1 };
    if vertical && ra.rows != la.rows || !vertical && ra.cols != la.cols {
        return Err(CellError::Value);
    }
    let mm = if has(args, 4) { opt_int(args, 4, 0)? } else { 0 };
    let sm = if has(args, 5) { opt_int(args, 5, 1)? } else { 1 };
    let hay = hay(c, lv, &la, Line::All);
    match xsearch(&x, &hay, mm, sm)? {
        Some(p) => {
            if vertical {
                let r = row(&ra, p);
                if r.len() == 1 {
                    return Ok(cell_result(r.into_iter().next().unwrap_or(Value::Empty)));
                }
                array_val(1, r.len(), r)
            } else {
                let c = column(&ra, p);
                if c.len() == 1 {
                    return Ok(cell_result(c.into_iter().next().unwrap_or(Value::Empty)));
                }
                array_val(c.len(), 1, c)
            }
        }
        None => {
            if has(args, 3) {
                Ok(arg(args, 3)?.value.clone())
            } else {
                Err(CellError::NA)
            }
        }
    }
}

fn xmatch(args: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let x = needle(args, 0)?;
    let lv = &arg(args, 1)?.value;
    if let Value::Error(e) = lv {
        return Err(*e);
    }
    let la = as_array(lv);
    if !is_vector(&la) {
        return Err(CellError::Value);
    }
    let mm = if has(args, 2) { int(args, 2)? } else { 0 };
    let sm = if has(args, 3) { int(args, 3)? } else { 1 };
    let hay = hay(c, lv, &la, Line::All);
    xsearch(&x, &hay, mm, sm)?.map(|p| Value::Number((p + 1) as f64)).ok_or(CellError::NA)
}

// ---------------------------------------------------------------------------------------------
// INDEX / ROWS / COLUMNS / TRANSPOSE / ADDRESS

fn index(args: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let av = &arg(args, 0)?.value;
    if let Value::Error(e) = av {
        return Err(*e);
    }
    let a = as_array(av);
    let r = if has(args, 1) { num(args, 1)? } else { 0.0 };
    let c = if has(args, 2) { num(args, 2)? } else { 0.0 };
    if has(args, 3) && num(args, 3)?.trunc() != 1.0 {
        return Err(CellError::Ref);
    }
    if r < 0.0 || c < 0.0 {
        return Err(CellError::Value);
    }
    let (mut r, mut c) = (r.trunc() as usize, c.trunc() as usize);
    // A single row with only one index: the index selects the column.
    if a.rows == 1 && args.len() == 2 && a.cols > 1 {
        c = r;
        r = 1;
    }
    if r > a.rows || c > a.cols {
        return Err(CellError::Ref);
    }
    match (r, c) {
        (0, 0) => Ok(Value::Array(std::sync::Arc::new(a.into_owned()))),
        (0, c) => {
            let v = column(&a, c - 1);
            if v.len() == 1 {
                return Ok(v.into_iter().next().unwrap_or(Value::Empty));
            }
            array_val(v.len(), 1, v)
        }
        (r, 0) => {
            let v = row(&a, r - 1);
            if v.len() == 1 {
                return Ok(v.into_iter().next().unwrap_or(Value::Empty));
            }
            array_val(1, v.len(), v)
        }
        (r, c) => a.get(r - 1, c - 1).cloned().ok_or(CellError::Ref),
    }
}

fn rows_fn(args: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    match &arg(args, 0)?.value {
        Value::Array(a) => Ok(Value::Number(a.rows as f64)),
        Value::Error(e) => Err(*e),
        _ => Ok(Value::Number(1.0)),
    }
}

fn columns_fn(args: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    match &arg(args, 0)?.value {
        Value::Array(a) => Ok(Value::Number(a.cols as f64)),
        Value::Error(e) => Err(*e),
        _ => Ok(Value::Number(1.0)),
    }
}

fn transpose(args: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    match &arg(args, 0)?.value {
        Value::Array(a) => Ok(Value::from(a.transpose())),
        v => Ok(v.clone()),
    }
}

fn quote_sheet(s: &str) -> String {
    let plain =
        !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') && !s.chars().next().is_some_and(|c| c.is_ascii_digit());
    if plain { s.to_string() } else { format!("'{}'", s.replace('\'', "''")) }
}

fn address(args: &[Arg], _c: &mut dyn Ctx) -> R<Value> {
    let r = num(args, 0)?.trunc();
    let c = num(args, 1)?.trunc();
    let abs = if has(args, 2) { int(args, 2)? } else { 1 };
    let a1 = if has(args, 3) { opt_bool(args, 3, true)? } else { true };
    if !(1.0..=1_048_576.0).contains(&r) || !(1.0..=16_384.0).contains(&c) || !(1..=4).contains(&abs) {
        return Err(CellError::Value);
    }
    let (ri, ci) = (r as u32, c as u32);
    let (row_abs, col_abs) = match abs {
        1 => (true, true),
        2 => (true, false),
        3 => (false, true),
        _ => (false, false),
    };
    let cell = if a1 {
        format!("{}{}{}{}", if col_abs { "$" } else { "" }, col_to_letters(ci - 1), if row_abs { "$" } else { "" }, ri)
    } else {
        let rp = if row_abs { format!("R{ri}") } else { format!("R[{ri}]") };
        let cp = if col_abs { format!("C{ci}") } else { format!("C[{ci}]") };
        format!("{rp}{cp}")
    };
    if has(args, 4) {
        let sheet = text(args, 4)?;
        if sheet.is_empty() {
            return text_val(format!("!{cell}"));
        }
        return text_val(format!("{}!{cell}", quote_sheet(&sheet)));
    }
    text_val(cell)
}

const XL: &[bool] = &[true, false, false, false, false, false];
const VL: &[bool] = &[true, false, true, true];
const ML: &[bool] = &[true, false, true, true];
const IDX: &[bool] = &[false, true, true, true];

pub(crate) fn specs() -> Vec<FnSpec> {
    use crate::util::{A, S};
    vec![
        f!(
            "VLOOKUP",
            3,
            4,
            Lookup,
            VL,
            "VLOOKUP(lookup_value, table_array, col_index_num, [range_lookup])",
            "Finds a value in the first column of a table and returns the value in the same row of another column.",
            vlookup
        ),
        f!(
            "HLOOKUP",
            3,
            4,
            Lookup,
            VL,
            "HLOOKUP(lookup_value, table_array, row_index_num, [range_lookup])",
            "Finds a value in the first row of a table and returns the value in the same column of another row.",
            hlookup
        ),
        f!(
            "LOOKUP",
            2,
            3,
            Lookup,
            &[true, false, false],
            "LOOKUP(lookup_value, lookup_vector, [result_vector])",
            "Approximate lookup in a sorted vector or the first row/column of an array.",
            lookup
        ),
        f!(
            "MATCH",
            2,
            3,
            Lookup,
            &[true, false, true],
            "MATCH(lookup_value, lookup_array, [match_type])",
            "Returns the 1-based position of a value in a row or column.",
            match_fn
        ),
        f!(
            "XLOOKUP",
            3,
            6,
            Lookup,
            XL,
            "XLOOKUP(lookup_value, lookup_array, return_array, [if_not_found], [match_mode], [search_mode])",
            "Searches a row or column and returns the matching item, row or column from another array.",
            xlookup
        ),
        f!(
            "XMATCH",
            2,
            4,
            Lookup,
            ML,
            "XMATCH(lookup_value, lookup_array, [match_mode], [search_mode])",
            "Returns the position of a value in a row or column with flexible match and search modes.",
            xmatch
        ),
        f!(
            "INDEX",
            2,
            4,
            Lookup,
            IDX,
            "INDEX(array, row_num, [column_num], [area_num])",
            "Returns the element, row or column of an array at the given position.",
            index
        ),
        f!("ROWS", 1, 1, Lookup, A, "ROWS(array)", "Counts the rows in an array or range.", rows_fn),
        f!("COLUMNS", 1, 1, Lookup, A, "COLUMNS(array)", "Counts the columns in an array or range.", columns_fn),
        f!("TRANSPOSE", 1, 1, Lookup, A, "TRANSPOSE(array)", "Swaps the rows and columns of an array.", transpose),
        f!(
            "ADDRESS",
            2,
            5,
            Lookup,
            S,
            "ADDRESS(row_num, column_num, [abs_num], [a1], [sheet_text])",
            "Builds a cell address as text from row and column numbers.",
            address
        ),
    ]
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellError, Value};

    use crate::util::testutil::*;

    fn table() -> Value {
        arr(vec![
            vec![nv(1.0), tv("one"), nv(10.0)],
            vec![nv(2.0), tv("two"), nv(20.0)],
            vec![nv(3.0), tv("three"), nv(30.0)],
            vec![nv(5.0), tv("five"), Value::Empty],
        ])
    }

    fn names() -> Value {
        arr(vec![vec![tv("Apple"), nv(1.0)], vec![tv("Banana"), nv(2.0)], vec![tv("Cherry"), nv(3.0)]])
    }

    #[test]
    fn vlookup() {
        is_text(ev("VLOOKUP", vec![n(2.0), rf(table()), n(2.0)]), "two");
        is_text(ev("VLOOKUP", vec![n(4.0), rf(table()), n(2.0)]), "three");
        is_text(ev("VLOOKUP", vec![n(4.0), rf(table()), n(2.0), b(true)]), "three");
        is_text(ev("VLOOKUP", vec![n(99.0), rf(table()), n(2.0)]), "five");
        is_err(ev("VLOOKUP", vec![n(0.5), rf(table()), n(2.0)]), CellError::NA);
        is_err(ev("VLOOKUP", vec![n(4.0), rf(table()), n(2.0), b(false)]), CellError::NA);
        is_err(ev("VLOOKUP", vec![n(4.0), rf(table()), n(2.0), empty()]), CellError::NA);
        close(ev("VLOOKUP", vec![n(3.0), rf(table()), n(3.0), b(false)]), 30.0);
        close(ev("VLOOKUP", vec![n(5.0), rf(table()), n(3.0), b(false)]), 0.0);
        is_err(ev("VLOOKUP", vec![n(3.0), rf(table()), n(4.0)]), CellError::Ref);
        is_err(ev("VLOOKUP", vec![n(3.0), rf(table()), n(0.0)]), CellError::Value);
        close(ev("VLOOKUP", vec![t("b*"), rf(names()), n(2.0), b(false)]), 2.0);
        close(ev("VLOOKUP", vec![t("cherry"), rf(names()), n(2.0), b(false)]), 3.0);
        close(ev("VLOOKUP", vec![t("?pple"), rf(names()), n(2.0), b(false)]), 1.0);
        close(ev("VLOOKUP", vec![t("Bz"), rf(names()), n(2.0)]), 2.0);
        is_err(ev("VLOOKUP", vec![e(CellError::Div0), rf(names()), n(2.0)]), CellError::Div0);
        // Lifting over the lookup value.
        let r = ev("VLOOKUP", vec![av(col(&[1.0, 3.0])), rf(table()), n(3.0), b(false)]);
        assert_eq!(rows_of(&r), vec![vec![nv(10.0)], vec![nv(30.0)]]);
    }

    #[test]
    fn hlookup() {
        let t2 = arr(vec![vec![nv(1.0), nv(2.0), nv(3.0)], vec![tv("a"), tv("b"), tv("c")]]);
        is_text(ev("HLOOKUP", vec![n(2.0), rf(t2.clone()), n(2.0), b(false)]), "b");
        is_text(ev("HLOOKUP", vec![n(2.5), rf(t2.clone()), n(2.0)]), "b");
        is_err(ev("HLOOKUP", vec![n(2.0), rf(t2), n(3.0)]), CellError::Ref);
    }

    #[test]
    fn lookup_fn() {
        close(ev("LOOKUP", vec![n(4.0), av(row(&[1.0, 3.0, 5.0])), av(row(&[10.0, 30.0, 50.0]))]), 30.0);
        close(ev("LOOKUP", vec![n(5.0), av(row(&[1.0, 3.0, 5.0])), av(col(&[10.0, 30.0, 50.0]))]), 50.0);
        is_err(ev("LOOKUP", vec![n(0.0), av(row(&[1.0, 3.0, 5.0]))]), CellError::NA);
        close(ev("LOOKUP", vec![n(2.5), rf(table())]), 20.0);
        let wide = arr(vec![vec![nv(1.0), nv(2.0), nv(3.0)], vec![tv("a"), tv("b"), tv("c")]]);
        is_text(ev("LOOKUP", vec![n(2.0), av(wide)]), "b");
        // Text mixed in is skipped.
        let mixed = arr(vec![vec![nv(1.0), tv("x"), nv(3.0), tv("y")]]);
        close(ev("LOOKUP", vec![n(9.0), av(mixed.clone()), av(row(&[1.0, 2.0, 3.0, 4.0]))]), 3.0);
        close(ev("LOOKUP", vec![t("zz"), av(mixed), av(row(&[1.0, 2.0, 3.0, 4.0]))]), 4.0);
    }

    #[test]
    fn match_fn() {
        let v = col(&[10.0, 20.0, 30.0, 40.0]);
        close(ev("MATCH", vec![n(30.0), rf(v.clone()), n(0.0)]), 3.0);
        close(ev("MATCH", vec![n(35.0), rf(v.clone())]), 3.0);
        close(ev("MATCH", vec![n(35.0), rf(v.clone()), n(1.0)]), 3.0);
        close(ev("MATCH", vec![n(100.0), rf(v.clone())]), 4.0);
        is_err(ev("MATCH", vec![n(5.0), rf(v.clone())]), CellError::NA);
        is_err(ev("MATCH", vec![n(35.0), rf(v), n(0.0)]), CellError::NA);
        let d = col(&[40.0, 30.0, 20.0, 10.0]);
        close(ev("MATCH", vec![n(25.0), rf(d.clone()), n(-1.0)]), 2.0);
        close(ev("MATCH", vec![n(40.0), rf(d.clone()), n(-1.0)]), 1.0);
        is_err(ev("MATCH", vec![n(50.0), rf(d), n(-1.0)]), CellError::NA);
        let s = arr(vec![vec![tv("apple"), tv("banana"), tv("cherry")]]);
        close(ev("MATCH", vec![t("B*"), av(s.clone()), n(0.0)]), 2.0);
        close(ev("MATCH", vec![t("CHERRY"), av(s.clone()), n(0.0)]), 3.0);
        close(ev("MATCH", vec![t("c"), av(s.clone())]), 2.0);
        is_err(ev("MATCH", vec![n(1.0), rf(table())]), CellError::NA);
        close(ev("MATCH", vec![b(true), av(arr(vec![vec![nv(1.0), Value::Bool(true)]])), n(0.0)]), 2.0);
    }

    #[test]
    fn xlookup() {
        let keys = col(&[1.0, 2.0, 3.0, 5.0]);
        let vals = col(&[10.0, 20.0, 30.0, 50.0]);
        close(ev("XLOOKUP", vec![n(3.0), rf(keys.clone()), rf(vals.clone())]), 30.0);
        is_err(ev("XLOOKUP", vec![n(4.0), rf(keys.clone()), rf(vals.clone())]), CellError::NA);
        is_text(ev("XLOOKUP", vec![n(4.0), rf(keys.clone()), rf(vals.clone()), t("none")]), "none");
        close(ev("XLOOKUP", vec![n(4.0), rf(keys.clone()), rf(vals.clone()), empty(), n(-1.0)]), 30.0);
        close(ev("XLOOKUP", vec![n(4.0), rf(keys.clone()), rf(vals.clone()), empty(), n(1.0)]), 50.0);
        is_err(ev("XLOOKUP", vec![n(6.0), rf(keys.clone()), rf(vals.clone()), empty(), n(1.0)]), CellError::NA);
        close(ev("XLOOKUP", vec![n(4.0), rf(keys.clone()), rf(vals.clone()), empty(), n(-1.0), n(2.0)]), 30.0);
        close(ev("XLOOKUP", vec![n(4.0), rf(keys.clone()), rf(vals.clone()), empty(), n(1.0), n(2.0)]), 50.0);
        close(ev("XLOOKUP", vec![n(5.0), rf(keys.clone()), rf(vals.clone()), empty(), n(0.0), n(2.0)]), 50.0);
        let dup = col(&[1.0, 2.0, 1.0]);
        let dv = col(&[10.0, 20.0, 30.0]);
        close(ev("XLOOKUP", vec![n(1.0), rf(dup.clone()), rf(dv.clone())]), 10.0);
        close(ev("XLOOKUP", vec![n(1.0), rf(dup), rf(dv), empty(), n(0.0), n(-1.0)]), 30.0);
        // Descending binary search.
        let desc = col(&[50.0, 30.0, 20.0, 10.0]);
        close(ev("XLOOKUP", vec![n(25.0), rf(desc.clone()), rf(col(&[1.0, 2.0, 3.0, 4.0])), empty(), n(-1.0), n(-2.0)]), 3.0);
        close(ev("XLOOKUP", vec![n(25.0), rf(desc), rf(col(&[1.0, 2.0, 3.0, 4.0])), empty(), n(1.0), n(-2.0)]), 2.0);
        // Wildcards only in mode 2.
        let names_col = arr(vec![vec![tv("Apple")], vec![tv("Banana")]]);
        is_err(ev("XLOOKUP", vec![t("b*"), rf(names_col.clone()), rf(col(&[1.0, 2.0]))]), CellError::NA);
        close(ev("XLOOKUP", vec![t("b*"), rf(names_col), rf(col(&[1.0, 2.0])), empty(), n(2.0)]), 2.0);
        // Returning a whole row.
        let r = ev("XLOOKUP", vec![n(2.0), rf(col(&[1.0, 2.0, 3.0])), rf(table_rows3())]);
        assert_eq!(rows_of(&r), vec![vec![nv(2.0), tv("two"), nv(20.0)]]);
        // Horizontal lookup returning a column.
        let hk = row(&[1.0, 2.0]);
        let hv = arr(vec![vec![tv("a"), tv("b")], vec![tv("c"), tv("d")]]);
        assert_eq!(rows_of(&ev("XLOOKUP", vec![n(2.0), av(hk.clone()), av(hv)])), vec![vec![tv("b")], vec![tv("d")]]);
        is_err(ev("XLOOKUP", vec![n(2.0), av(hk), av(col(&[1.0, 2.0, 3.0]))]), CellError::Value);
        is_err(ev("XLOOKUP", vec![n(2.0), rf(keys.clone()), rf(vals.clone()), empty(), n(3.0)]), CellError::Value);
        is_err(ev("XLOOKUP", vec![n(2.0), rf(table()), rf(vals)]), CellError::Value);
    }

    fn table_rows3() -> Value {
        arr(vec![vec![nv(1.0), tv("one"), nv(10.0)], vec![nv(2.0), tv("two"), nv(20.0)], vec![nv(3.0), tv("three"), nv(30.0)]])
    }

    #[test]
    fn xmatch() {
        let v = col(&[10.0, 20.0, 30.0]);
        close(ev("XMATCH", vec![n(20.0), rf(v.clone())]), 2.0);
        is_err(ev("XMATCH", vec![n(25.0), rf(v.clone())]), CellError::NA);
        close(ev("XMATCH", vec![n(25.0), rf(v.clone()), n(1.0)]), 3.0);
        close(ev("XMATCH", vec![n(25.0), rf(v.clone()), n(-1.0)]), 2.0);
        close(ev("XMATCH", vec![n(30.0), rf(v.clone()), n(0.0), n(2.0)]), 3.0);
        close(ev("XMATCH", vec![t("a?c"), av(arr(vec![vec![tv("xyz"), tv("abc")]])), n(2.0)]), 2.0);
        let r = ev("XMATCH", vec![av(row(&[30.0, 10.0])), rf(v)]);
        assert_eq!(rows_of(&r), vec![vec![nv(3.0), nv(1.0)]]);
    }

    #[test]
    fn index() {
        let t3 = table_rows3();
        is_text(ev("INDEX", vec![rf(t3.clone()), n(2.0), n(2.0)]), "two");
        close(ev("INDEX", vec![rf(t3.clone()), n(3.0), n(3.0)]), 30.0);
        assert_eq!(rows_of(&ev("INDEX", vec![rf(t3.clone()), n(0.0), n(3.0)])), vec![vec![nv(10.0)], vec![nv(20.0)], vec![nv(30.0)]]);
        assert_eq!(rows_of(&ev("INDEX", vec![rf(t3.clone()), n(1.0), n(0.0)])), vec![vec![nv(1.0), tv("one"), nv(10.0)]]);
        assert_eq!(rows_of(&ev("INDEX", vec![rf(t3.clone()), n(1.0)])), vec![vec![nv(1.0), tv("one"), nv(10.0)]]);
        is_err(ev("INDEX", vec![rf(t3.clone()), n(4.0), n(1.0)]), CellError::Ref);
        is_err(ev("INDEX", vec![rf(t3.clone()), n(1.0), n(4.0)]), CellError::Ref);
        is_err(ev("INDEX", vec![rf(t3), n(-1.0), n(1.0)]), CellError::Value);
        close(ev("INDEX", vec![av(row(&[5.0, 6.0, 7.0])), n(2.0)]), 6.0);
        close(ev("INDEX", vec![av(col(&[5.0, 6.0, 7.0])), n(3.0)]), 7.0);
        close(ev("INDEX", vec![n(9.0), n(1.0), n(1.0)]), 9.0);
        let r = ev("INDEX", vec![av(col(&[5.0, 6.0, 7.0])), av(row(&[3.0, 1.0]))]);
        assert_eq!(rows_of(&r), vec![vec![nv(7.0), nv(5.0)]]);
    }

    #[test]
    fn shape_and_address() {
        close(ev("ROWS", vec![rf(table())]), 4.0);
        close(ev("COLUMNS", vec![rf(table())]), 3.0);
        close(ev("ROWS", vec![n(1.0)]), 1.0);
        let tr = ev("TRANSPOSE", vec![av(row(&[1.0, 2.0, 3.0]))]);
        assert_eq!(rows_of(&tr), vec![vec![nv(1.0)], vec![nv(2.0)], vec![nv(3.0)]]);
        is_text(ev("ADDRESS", vec![n(1.0), n(1.0)]), "$A$1");
        is_text(ev("ADDRESS", vec![n(2.0), n(3.0), n(2.0)]), "C$2");
        is_text(ev("ADDRESS", vec![n(2.0), n(3.0), n(3.0)]), "$C2");
        is_text(ev("ADDRESS", vec![n(2.0), n(28.0), n(4.0)]), "AB2");
        is_text(ev("ADDRESS", vec![n(2.0), n(3.0), n(1.0), b(false)]), "R2C3");
        is_text(ev("ADDRESS", vec![n(2.0), n(3.0), n(4.0), b(false)]), "R[2]C[3]");
        is_text(ev("ADDRESS", vec![n(1.0), n(1.0), n(1.0), b(true), t("Sheet1")]), "Sheet1!$A$1");
        is_text(ev("ADDRESS", vec![n(1.0), n(1.0), n(1.0), b(true), t("My Sheet")]), "'My Sheet'!$A$1");
        is_text(ev("ADDRESS", vec![n(1048576.0), n(16384.0)]), "$XFD$1048576");
        is_err(ev("ADDRESS", vec![n(0.0), n(1.0)]), CellError::Value);
        is_err(ev("ADDRESS", vec![n(1.0), n(16385.0)]), CellError::Value);
        is_err(ev("ADDRESS", vec![n(1.0), n(1.0), n(5.0)]), CellError::Value);
    }
}
