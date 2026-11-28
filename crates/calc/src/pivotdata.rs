//! GETPIVOTDATA: a value from a PivotTable report, found by its value field and the row and
//! column items that locate it (Excel's documented semantics). Each PivotTable's value cells are
//! indexed in `PivotTable::data_index` (set on refresh and saved with the report; built on first
//! use only for a pivot loaded without one, and only when it matches the stored report); the
//! result is read from the report cell the sheet shows.

use gridcraft_core::{CellError, CellRef, Value, number_to_text};
use gridcraft_formula::Expr;
use gridcraft_model::{PIVOT_INDEXER, PivotDataCell, PivotIndexError, PivotItemRef, PivotLookup, PivotTable};
use std::borrow::Cow;

use crate::eval::{Ev, Evaluator};

/// Excel allows 126 field/item pairs.
const MAX_PAIRS: usize = 126;

fn same_text(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn is_blank_query(v: &Value) -> bool {
    matches!(v, Value::Empty) || matches!(v, Value::Text(t) if t.is_empty())
}

#[derive(Clone, Copy)]
enum PairMatch {
    /// Strict identity match:
    /// - blank query (`Value::Empty` / `""`) matching raw `Value::Empty`,
    /// - numeric query `n` matching `Value::Number(x)` with `n == x`,
    /// - boolean query `b` matching `Value::Bool(b)`,
    /// - non-blank text/other query matching a non-empty item's label.
    Exact,
    /// Relative `1e-12` numeric tolerance (no absolute floor); stores `abs_delta = (n - x).abs()`.
    Tolerant(f64),
    /// Fallback match: formatted label hit for numbers, raw numeric text against formatted
    /// numeric items, or literal `"(blank)"` text matching `Value::Empty` when no literal text
    /// item exists.
    Fallback,
}

impl PairMatch {
    fn is_better_than(self, other: Self) -> bool {
        match (self, other) {
            (Self::Exact, Self::Exact) | (Self::Fallback, _) => false,
            (Self::Exact, _) | (Self::Tolerant(_), Self::Fallback) => true,
            (Self::Tolerant(_), Self::Exact) => false,
            (Self::Tolerant(a), Self::Tolerant(b)) => a.total_cmp(&b).is_lt(),
        }
    }
}

#[derive(Clone, Copy)]
struct MatchScore {
    fallbacks: usize,
    tolerants: usize,
    max_delta: f64,
    sum_delta: f64,
}

impl MatchScore {
    const EXACT: Self = Self { fallbacks: 0, tolerants: 0, max_delta: 0.0, sum_delta: 0.0 };

    fn is_exact(self) -> bool {
        self.fallbacks == 0 && self.tolerants == 0
    }

    fn is_better_than(self, other: Self) -> bool {
        self.fallbacks
            .cmp(&other.fallbacks)
            .then_with(|| self.tolerants.cmp(&other.tolerants))
            .then_with(|| self.max_delta.total_cmp(&other.max_delta))
            .then_with(|| self.sum_delta.total_cmp(&other.sum_delta))
            .is_lt()
    }
}

/// Scores a single pivot item against a query value, returning the lowest matching tier.
/// Blank queries (`Value::Empty` or `""`) match only genuinely blank items (`Value::Empty`),
/// never literal `"(blank)"` text items.
fn pair_tier(item: &PivotItemRef, v: &Value) -> Option<PairMatch> {
    if is_blank_query(v) {
        return matches!(item.value, Value::Empty).then_some(PairMatch::Exact);
    }
    if let Value::Number(n) = v {
        if let Value::Number(x) = &item.value {
            if n == x {
                return Some(PairMatch::Exact);
            }
            let delta = (n - x).abs();
            if delta <= 1e-12 * n.abs().max(x.abs()) {
                return Some(PairMatch::Tolerant(delta));
            }
        }
        return (!matches!(item.value, Value::Empty) && v.to_text().is_ok_and(|t| same_text(&t, &item.label))).then_some(PairMatch::Fallback);
    }
    if let Value::Bool(b) = v {
        return (item.value == Value::Bool(*b)).then_some(PairMatch::Exact);
    }
    if matches!(item.value, Value::Empty) {
        return v.to_text().is_ok_and(|t| same_text(&t, &item.label)).then_some(PairMatch::Fallback);
    }
    if v.to_text().is_ok_and(|t| same_text(&t, &item.label)) {
        return Some(PairMatch::Exact);
    }
    if let Value::Text(t) = v
        && let Value::Number(x) = &item.value
        && same_text(t, &number_to_text(*x))
    {
        return Some(PairMatch::Fallback);
    }
    None
}

/// Scores `cell` against `(data_field, pairs)`. Returns `None` if the data field does not match,
/// item counts differ, or any query pair fails to match.
fn score_cell(cell: &PivotDataCell, data_field: &str, pairs: &[(String, Value)]) -> Option<MatchScore> {
    if !(same_text(&cell.data_field, data_field) || same_text(&cell.source_col, data_field)) || cell.items.len() != pairs.len() {
        return None;
    }
    let mut score = MatchScore::EXACT;
    for (f, v) in pairs {
        let mut best_pair: Option<PairMatch> = None;
        for it in &cell.items {
            if same_text(&it.field, f)
                && let Some(m) = pair_tier(it, v)
                && best_pair.is_none_or(|cur| m.is_better_than(cur))
            {
                best_pair = Some(m);
            }
        }
        match best_pair? {
            PairMatch::Exact => {}
            PairMatch::Tolerant(delta) => {
                score.tolerants += 1;
                score.max_delta = score.max_delta.max(delta);
                score.sum_delta += delta;
            }
            PairMatch::Fallback => {
                score.fallbacks += 1;
            }
        }
    }
    Some(score)
}

/// Resolves `field` against `pt`'s axis fields (`rows` then `columns`), returning the matched
/// axis-field index and its canonical `source_col`. Direct `source_col` matches take precedence
/// over custom `caption` aliases (preferring an unrenamed field if the same `source_col` appears
/// multiple times, e.g. date-grouped levels).
fn resolve_axis_field<'a>(pt: &'a PivotTable, field: &str) -> Option<(usize, &'a str)> {
    let mut best_source: Option<(usize, &'a str)> = None;
    for (idx, af) in pt.rows.iter().chain(pt.columns.iter()).enumerate() {
        if same_text(&af.source_col, field) {
            if af.caption.as_deref().is_none_or(|c| same_text(c, field)) {
                return Some((idx, af.source_col.as_str()));
            }
            best_source.get_or_insert((idx, af.source_col.as_str()));
        }
    }
    if best_source.is_some() {
        return best_source;
    }
    pt.rows
        .iter()
        .chain(pt.columns.iter())
        .enumerate()
        .find(|(_, af)| af.caption.as_deref().is_some_and(|c| same_text(c, field)))
        .map(|(idx, af)| (idx, af.source_col.as_str()))
}

/// The value cell whose value field and items best match those asked for.
///
/// 1. O(1) hash fast path (for all queries, including blank queries via `"(blank)"`): if the hash
///    hit matches every pair strictly in `Exact` (checking `Value::Empty` identity for blank
///    queries and strict `n == x` for numbers), return immediately.
/// 2. Single-pass scored fallback scan: score every matching candidate per pair (`Exact` <
///    `Tolerant(|n - x|)` < `Fallback`), ranking lexicographically by fewest `Fallback` pairs,
///    fewest `Tolerant` pairs, smallest `(max_delta, sum_delta)`, and first cell on exact tie.
///    Any all-`Exact` candidate short-circuits immediately.
fn find<'a>(cells: &'a PivotLookup, data_field: &str, pairs: &[(String, Value)]) -> Option<&'a PivotDataCell> {
    let labels: Option<Vec<(&str, Cow<'_, str>, bool)>> = pairs
        .iter()
        .map(|(f, v)| {
            if is_blank_query(v) {
                Some((f.as_str(), Cow::Borrowed("(blank)"), false))
            } else {
                v.to_text().ok().map(|t| (f.as_str(), Cow::Owned(t), matches!(v, Value::Bool(_))))
            }
        })
        .collect();
    let hit = labels.as_ref().and_then(|labels| {
        let refs: Vec<(&str, &str, bool)> = labels.iter().map(|(f, t, b)| (*f, t.as_ref(), *b)).collect();
        cells.find(data_field, &refs)
    });
    if let Some(c) = hit
        && score_cell(c, data_field, pairs).is_some_and(MatchScore::is_exact)
    {
        return Some(c);
    }
    if hit.is_none()
        && pairs.iter().all(|(_, v)| {
            matches!(
                v,
                Value::Text(t)
                    if !t.is_empty()
                        && !t.trim().eq_ignore_ascii_case("true")
                        && !t.trim().eq_ignore_ascii_case("false")
                        && t.trim().parse::<f64>().is_err()
            )
        })
    {
        return None;
    }
    let mut best: Option<(&PivotDataCell, MatchScore)> = None;
    for cell in cells.cells()? {
        let Some(score) = score_cell(cell, data_field, pairs) else { continue };
        if score.is_exact() {
            return Some(cell);
        }
        if best.is_none_or(|(_, b)| score.is_better_than(b)) {
            best = Some((cell, score));
        }
    }
    best.map(|(cell, _)| cell)
}

impl Evaluator<'_> {
    pub(crate) fn getpivotdata(&mut self, args: &[Expr]) -> Ev {
        match self.pivot_value(args) {
            Ok(v) => Ev::V(v),
            Err(e) => Ev::V(Value::Error(e)),
        }
    }

    fn scalar_arg(&mut self, e: &Expr) -> Result<Value, CellError> {
        match self.value(e).scalar() {
            Value::Error(e) => Err(e),
            v => Ok(v),
        }
    }

    fn pivot_value(&mut self, args: &[Expr]) -> Result<Value, CellError> {
        let (Some(field_expr), Some(table_expr)) = (args.first(), args.get(1)) else { return Err(CellError::Value) };
        let rest = args.get(2..).unwrap_or_default();
        if rest.len() % 2 != 0 || rest.len() / 2 > MAX_PAIRS {
            return Err(CellError::Ref);
        }
        let data_field = self.scalar_arg(field_expr)?.to_text()?;
        let area = match self.eval(table_expr) {
            Ev::R(areas) => areas.first().copied().ok_or(CellError::Ref)?,
            Ev::V(Value::Error(e)) => return Err(e),
            _ => return Err(CellError::Ref),
        };
        let mut pairs: Vec<(String, Value)> = Vec::with_capacity(rest.len() / 2);
        for pair in rest.chunks(2) {
            let (Some(f), Some(i)) = (pair.first(), pair.get(1)) else { return Err(CellError::Ref) };
            let field = self.scalar_arg(f)?.to_text()?;
            let item = self.scalar_arg(i)?;
            if pairs.iter().any(|(g, _)| same_text(g, &field)) {
                return Err(CellError::Ref);
            }
            pairs.push((field, item));
        }
        let wb = self.host.workbook();
        let sheet = wb.sheet(area.sheet).ok_or(CellError::Ref)?;
        let pt = sheet.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.intersects(&area.range))).ok_or(CellError::Ref)?;
        // A report filter field may be named when the report shows exactly that one item.
        // Resolve axis field captions to their canonical source field names and reject duplicate
        // references to the same axis field (e.g. canonical `"Region"` + custom caption `"Area"`).
        let mut axis_pairs = Vec::with_capacity(pairs.len());
        let mut canonical_pairs = Vec::with_capacity(pairs.len());
        let mut seen_axis_indices: Vec<usize> = Vec::with_capacity(pairs.len());
        let mut mapped = false;
        for (field, item) in pairs {
            if let Some((axis_idx, canonical)) = resolve_axis_field(pt, &field) {
                if seen_axis_indices.contains(&axis_idx) {
                    return Err(CellError::Ref);
                }
                seen_axis_indices.push(axis_idx);
                if !same_text(canonical, &field) {
                    mapped = true;
                }
                canonical_pairs.push((canonical.to_string(), item.clone()));
                axis_pairs.push((field, item));
                continue;
            }
            if let Some(filter) = pt.filters.iter().find(|f| same_text(&f.source_col, &field)) {
                let shown = match filter.selected.as_deref() {
                    Some([one]) => {
                        if is_blank_query(&item) {
                            same_text(one, "(blank)")
                        } else {
                            item.to_text().is_ok_and(|t| same_text(&t, one))
                        }
                    }
                    _ => false,
                };
                if !shown {
                    return Err(CellError::Ref);
                }
                continue;
            }
            // A grouped/renamed axis field present in a pre-built OOXML index may not appear in
            // `pt.rows`/`pt.columns`; let the index lookup decide whether it exists.
            if pt.data_index.cells().is_some() {
                canonical_pairs.push((field.clone(), item.clone()));
                axis_pairs.push((field, item));
                continue;
            }
            return Err(CellError::Ref);
        }
        // The report's value cells. A pivot loaded without them builds them once; a stored report
        // that no longer matches its source gets no index at all, so the lookup is #REF! until a
        // refresh — never an index cached from a failed build, because the shared lookup keeps
        // the first one set and undo or reopen could then never rebuild it.
        if pt.data_index.cells().is_none() {
            let build = PIVOT_INDEXER.get().ok_or(CellError::Ref)?;
            match build(wb, area.sheet, pt) {
                Ok(cells) => {
                    pt.data_index.set_cells(cells);
                }
                Err(PivotIndexError::Stale | PivotIndexError::Other(_)) => {}
            }
        }
        let canonical_unique = (0..canonical_pairs.len()).all(|i| !canonical_pairs[..i].iter().any(|(g, _)| same_text(g, &canonical_pairs[i].0)));
        let cell = (canonical_unique.then(|| find(&pt.data_index, &data_field, &canonical_pairs)).flatten())
            .or_else(|| mapped.then(|| find(&pt.data_index, &data_field, &axis_pairs)).flatten())
            .ok_or(CellError::Ref)?;
        // The value is the one the sheet shows, so the formula and the report cell always agree.
        let report = pt.last_range.ok_or(CellError::Ref)?;
        let at = CellRef::new(report.start.row.saturating_add(cell.row), report.start.col.saturating_add(cell.col));
        if !report.contains(at) {
            return Err(CellError::Ref);
        }
        Ok(sheet.value(at))
    }
}
