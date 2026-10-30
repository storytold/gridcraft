//! Other functions: bit shifts, row-context helpers, window ranking, metadata tables and the
//! engine's scatter-chart sampling function.
//!
//! References: <https://learn.microsoft.com/en-us/dax/bitlshift-function-dax>,
//! <https://dax.guide/earlier/>, <https://dax.guide/rank/>, <https://dax.guide/rownumber/>,
//! <https://learn.microsoft.com/en-us/dax/info-storagetables-function-dax> and
//! <https://dax.guide/samplecartesianpointsbycover/>.
//!
//! Row-context model: [`Context::rows`] is a stack whose last entry is the innermost row.
//! `EARLIER` counts frames of the *same lineage table* outward from the innermost one, which
//! matches the documented "number of table scans": frames pushed for other tables are not scans
//! of the column's table.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::ast::Literal;

use super::support::{args, eval_all, eval_table, frame_for};
use super::*;

/// FNV-1a 64-bit offset basis and prime. `HASH` uses a fixed, version-independent algorithm so
/// the value is stable across runs and platforms (the real engine's algorithm is undocumented).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The largest magnitude that fits an `i64`, as `f64` (2^63).
const I64_MAX_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "BITLSHIFT" => bit_shift(ev, name, exprs, ctx, true),
        "BITRSHIFT" => bit_shift(ev, name, exprs, ctx, false),
        "EARLIER" => earlier(ev, name, exprs, ctx, EarlierMode::Level),
        "EARLIEST" => earlier(ev, name, exprs, ctx, EarlierMode::Outermost),
        "EXTERNALMEASURE" => external_measure(exprs),
        "HASH" => hash(ev, name, exprs, ctx),
        "ISATLEVEL" => is_at_level(exprs),
        "KEYWORDMATCH" => keyword_match(ev, name, exprs, ctx),
        "RANK" => window(ev, name, exprs, ctx, true),
        "ROWNUMBER" => window(ev, name, exprs, ctx, false),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "INFO.STORAGETABLES" => storage_tables(ev, name, exprs, ctx),
        "SAMPLECARTESIANPOINTSBYCOVER" => sample_cartesian(ev, name, exprs, ctx),
        _ => return None,
    };
    Some(result)
}

// ---------------------------------------------------------------------------------------------
// Bit shifts
// ---------------------------------------------------------------------------------------------

/// `BITLSHIFT`/`BITRSHIFT`. Shift counts are truncated to integers; a negative amount shifts the
/// other way. Shifting a 64-bit integer by 64 or more bits is undefined in DAX ("no error but
/// overflow/underflow"): we return 0, or -1 for an arithmetic right shift of a negative number.
fn bit_shift(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context, left: bool) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, exprs, ctx)?;
    let a = args(name, &values);
    let number = truncated_integer(a.number(0)?, name, 1)?;
    let shift = truncated_integer(a.number(1)?, name, 2)?;
    Ok(Scalar::Integer(shift_bits(number, shift, left)))
}

fn shift_bits(number: i64, shift: i64, left: bool) -> i64 {
    let (direction_left, magnitude) = if shift >= 0 { (left, shift.unsigned_abs()) } else { (!left, shift.unsigned_abs()) };
    if magnitude >= 64 {
        return if direction_left || number >= 0 { 0 } else { -1 };
    }
    if direction_left { ((number as u64) << magnitude) as i64 } else { number >> magnitude }
}

/// Truncates a numeric argument to an `i64`, rejecting values that cannot be represented.
fn truncated_integer(value: f64, name: &str, position: usize) -> Result<i64, EvalError> {
    if !value.is_finite() || value < -I64_MAX_EXCLUSIVE || value >= I64_MAX_EXCLUSIVE {
        return Err(EvalError::new(format!("{name} argument {position} is out of range for an integer")));
    }
    Ok(value.trunc() as i64)
}

// ---------------------------------------------------------------------------------------------
// EARLIER / EARLIEST
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum EarlierMode {
    /// EARLIER: the requested number of outer row contexts (default 1).
    Level,
    /// EARLIEST: the outermost row context of the column's table.
    Outermost,
}

fn earlier(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context, mode: EarlierMode) -> Result<Scalar, EvalError> {
    let Some(column_expr) = exprs.first() else {
        return Err(EvalError::new(format!("{name} needs a column reference")));
    };
    let (table, column) = resolve_column_reference(ev, column_expr, ctx, name)?;

    // The level argument is evaluated in the current context before the frames are borrowed.
    let level = match mode {
        EarlierMode::Outermost => None,
        EarlierMode::Level => Some(match exprs.get(1) {
            Some(expr) => {
                let value = ev.eval(expr, ctx)?;
                let number = value.to_number().ok_or_else(|| EvalError::new(format!("{name} argument 2 must be numeric")))?;
                let level = truncated_integer(number, name, 2)?;
                if level < 1 {
                    return Err(EvalError::new(format!("{name} argument 2 must be at least 1")));
                }
                level
            }
            None => 1,
        }),
    };

    // Frames of this column's table, ordered outermost to innermost. The innermost one is the
    // current row context; one step further out is level 1.
    let frames: Vec<&RowFrame> = ctx.rows.iter().filter(|frame| frame.lineage == Some(table)).collect();
    let target = match level {
        Some(level) => {
            let Ok(level) = usize::try_from(level) else {
                return Err(EvalError::new(format!("{name} argument 2 is out of range")));
            };
            if frames.len() <= level {
                return Err(no_outer_row_context(name, ev, table, column));
            }
            frames.len() - 1 - level
        }
        None => {
            if frames.len() < 2 {
                return Err(no_outer_row_context(name, ev, table, column));
            }
            0
        }
    };
    let frame = frames.get(target).ok_or_else(|| no_outer_row_context(name, ev, table, column))?;
    frame.get(column).cloned().ok_or_else(|| EvalError::new(format!("{name} cannot read the column in the outer row context")))
}

fn no_outer_row_context(name: &str, ev: &Evaluator<'_>, table: usize, column: usize) -> EvalError {
    let table_name = ev.model().tables.get(table).map(|model| model.name.as_str()).unwrap_or("<table>");
    let column_name = ev.model().tables.get(table).and_then(|model| model.columns.get(column)).map(|model| model.name.as_str()).unwrap_or("<column>");
    EvalError::new(format!("{name} requires an outer row context for {table_name}[{column_name}]"))
}

/// Resolves the column reference of `EARLIER`, `EARLIEST` and `MATCHBY`.
fn resolve_column_reference(ev: &Evaluator<'_>, expr: &BoundExpr, ctx: &Context, name: &str) -> Result<(usize, usize), EvalError> {
    match expr {
        BoundExpr::Column { table, column } => Ok((*table, *column)),
        BoundExpr::UnqualifiedColumn { name: column_name } => {
            for frame in ctx.rows.iter().rev() {
                if let Some(table) = frame.lineage
                    && let Some(column) = ev.model().column_index(table, column_name)
                {
                    return Ok((table, column));
                }
            }
            let tables = ev.model().tables_with_column(column_name);
            if tables.len() == 1 {
                let table = tables[0];
                let column = ev
                    .model()
                    .column_index(table, column_name)
                    .ok_or_else(|| EvalError::new(format!("{name} cannot resolve column `[{column_name}]`")))?;
                return Ok((table, column));
            }
            Err(EvalError::new(format!("{name} could not resolve column `[{column_name}]`")))
        }
        _ => Err(EvalError::new(format!("{name} expects a column reference"))),
    }
}

fn column_reference(expr: &BoundExpr) -> Option<(usize, usize)> {
    match expr {
        BoundExpr::Column { table, column } => Some((*table, *column)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// EXTERNALMEASURE
// ---------------------------------------------------------------------------------------------

/// A remote measure cannot be resolved by the in-memory evaluator; the documented behavior for
/// an unresolved external reference is BLANK.
fn external_measure(_exprs: &[BoundExpr]) -> Result<Scalar, EvalError> {
    Ok(Scalar::Blank)
}

// ---------------------------------------------------------------------------------------------
// HASH
// ---------------------------------------------------------------------------------------------

/// `HASH` computes a FNV-1a 64-bit hash over a type-tagged encoding of every argument. The real
/// engine's algorithm is undocumented; a stable 64-bit hash is the documented contract.
fn hash(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    if exprs.is_empty() {
        return Err(EvalError::new(format!("{name} needs at least one argument")));
    }
    let values = eval_all(ev, exprs, ctx)?;
    let mut state = FNV_OFFSET;
    for value in &values {
        hash_value(&mut state, value);
        // Delimit arguments so `HASH("ab", "c")` and `HASH("a", "bc")` differ.
        fnv_byte(&mut state, 0xff);
    }
    Ok(Scalar::Integer(state as i64))
}

fn fnv_byte(state: &mut u64, byte: u8) {
    *state ^= u64::from(byte);
    *state = state.wrapping_mul(FNV_PRIME);
}

fn fnv_bytes(state: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        fnv_byte(state, *byte);
    }
}

fn fnv_u64(state: &mut u64, value: u64) {
    fnv_bytes(state, &value.to_le_bytes());
}

fn hash_value(state: &mut u64, value: &Scalar) {
    match value {
        Scalar::Blank => fnv_byte(state, 0),
        Scalar::Boolean(flag) => {
            fnv_byte(state, 1);
            fnv_byte(state, u8::from(*flag));
        }
        Scalar::Integer(number) => {
            fnv_byte(state, 2);
            fnv_u64(state, *number as u64);
        }
        Scalar::Currency(number) => {
            fnv_byte(state, 3);
            fnv_u64(state, *number as u64);
        }
        Scalar::Decimal(number) => {
            fnv_byte(state, 4);
            let bits = if *number == 0.0 { 0.0f64.to_bits() } else { number.to_bits() };
            fnv_u64(state, bits);
        }
        Scalar::DateTime(serial) => {
            fnv_byte(state, 5);
            fnv_u64(state, if *serial == 0.0 { 0.0f64.to_bits() } else { serial.to_bits() });
        }
        Scalar::String(text) => {
            fnv_byte(state, 6);
            fnv_u64(state, text.len() as u64);
            fnv_bytes(state, text.as_bytes());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// ISATLEVEL
// ---------------------------------------------------------------------------------------------

/// Visual levels are not modelled by the in-memory evaluator, so a column is never reported as
/// being at the current grid level. The column argument is validated but not evaluated: it only
/// names the column, exactly as in the engine.
fn is_at_level(exprs: &[BoundExpr]) -> Result<Scalar, EvalError> {
    let Some(expr) = exprs.first() else {
        return Err(EvalError::new("ISATLEVEL needs a column reference"));
    };
    if !matches!(expr, BoundExpr::Column { .. } | BoundExpr::UnqualifiedColumn { .. }) {
        return Err(EvalError::new("ISATLEVEL expects a column reference"));
    }
    Ok(Scalar::Boolean(false))
}

// ---------------------------------------------------------------------------------------------
// KEYWORDMATCH
// ---------------------------------------------------------------------------------------------

/// `KEYWORDMATCH(MatchExpression, Text)`: the match expression is a case-insensitive pattern
/// with Excel wildcards (`*` any run, `?` one character) that must occur inside the text.
/// The function was never released ("only appeared in some Excel versions") so there is no
/// authoritative specification; a blank match expression never matches.
fn keyword_match(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = eval_all(ev, exprs, ctx)?;
    let a = args(name, &values);
    let pattern = a.text(0)?;
    let text = a.text(1)?;
    if pattern.is_empty() || text.is_empty() {
        return Ok(Scalar::Boolean(false));
    }
    Ok(Scalar::Boolean(keyword_pattern_matches(&pattern, &text)))
}

/// Implicitly surrounds the pattern with `*` so it matches anywhere in the text.
fn keyword_pattern_matches(pattern: &str, text: &str) -> bool {
    let mut chars: Vec<char> = Vec::with_capacity(pattern.chars().count() + 2);
    chars.push('*');
    chars.extend(pattern.to_lowercase().chars());
    chars.push('*');
    wildcard_match(&chars, &text.to_lowercase().chars().collect::<Vec<_>>())
}

/// Greedy wildcard matcher with single-star backtracking: linear in practice, never recursive.
fn wildcard_match(pattern: &[char], text: &[char]) -> bool {
    let (mut pattern_index, mut text_index) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while text_index < text.len() {
        if pattern_index < pattern.len() && (pattern[pattern_index] == '?' || pattern[pattern_index] == text[text_index]) {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some((pattern_index, text_index));
            pattern_index += 1;
        } else if let Some((star_pattern, star_text)) = star {
            pattern_index = star_pattern + 1;
            text_index = star_text + 1;
            star = Some((star_pattern, star_text + 1));
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

// ---------------------------------------------------------------------------------------------
// RANK / ROWNUMBER
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlanksOrder {
    /// DAX DEFAULT: numeric blanks sort between zero and negative values; string blanks sort
    /// before every string.
    Default,
    First,
    Last,
}

#[derive(Clone, Copy)]
enum KeySource<'a> {
    /// An `ORDERBY` expression, evaluated in the relation's row context.
    Expr(&'a BoundExpr),
    /// A relation column used when no `ORDERBY` clause is supplied.
    Column(usize),
}

#[derive(Clone, Copy)]
struct OrderKey<'a> {
    source: KeySource<'a>,
    descending: bool,
    blanks: Option<BlanksOrder>,
}

struct WindowSpec<'a> {
    dense: bool,
    ties_seen: bool,
    relation: Option<&'a BoundExpr>,
    /// A bare name found where the relation belongs. The binder binds RANK's first argument as
    /// a keyword (the catalog marks that position as text), so `RANK(Sales, ORDERBY(...))`
    /// without a ties mode arrives as `Keyword("Sales")` instead of a table reference.
    relation_keyword: Option<String>,
    order: Vec<OrderKey<'a>>,
    blanks: BlanksOrder,
    partition: Vec<&'a BoundExpr>,
    match_by: Vec<&'a BoundExpr>,
}

struct WindowRow {
    index: usize,
    keys: Vec<Scalar>,
    partition: Vec<Scalar>,
}

/// Parses the keyword-style arguments of `RANK`/`ROWNUMBER`. `ORDERBY`, `PARTITIONBY` and
/// `MATCHBY` are recognized structurally, so they never need to be evaluated as tables.
///
/// The binder turns documented bare enum parameters (`DENSE`, `SKIP`, `DEFAULT`, `FIRST`,
/// `LAST`, ...) into [`BoundExpr::Keyword`] when the catalog marks the position as text. Quoted
/// strings are accepted too, which keeps older expressions working. `ORDERBY`'s positions are
/// all `Any`, so per-key `ASC`/`DESC`/`BLANKS` come through as plain names (see
/// [`keyword_text`]).
fn parse_window<'a>(name: &str, exprs: &'a [BoundExpr], rank: bool) -> Result<WindowSpec<'a>, EvalError> {
    let mut spec = WindowSpec {
        dense: false,
        ties_seen: false,
        relation: None,
        relation_keyword: None,
        order: Vec::new(),
        blanks: BlanksOrder::Default,
        partition: Vec::new(),
        match_by: Vec::new(),
    };
    for expr in exprs {
        if let Some(text) = keyword_text(expr) {
            let keyword = text.trim().to_ascii_uppercase();
            if rank && !spec.ties_seen && matches!(keyword.as_str(), "DENSE" | "SKIP") {
                spec.dense = keyword == "DENSE";
                spec.ties_seen = true;
            } else if matches!(keyword.as_str(), "DEFAULT" | "FIRST" | "LAST") {
                spec.blanks = match keyword.as_str() {
                    "FIRST" => BlanksOrder::First,
                    "LAST" => BlanksOrder::Last,
                    _ => BlanksOrder::Default,
                };
            } else if matches!(keyword.as_str(), "NONE" | "LOWESTPARENT" | "HIGHESTPARENT") {
                // Reset: no visual parent hierarchy exists, so it is accepted and ignored.
            } else if spec.relation.is_none() && spec.relation_keyword.is_none() {
                // Resolved against the model's table names while evaluating (see `window`).
                spec.relation_keyword = Some(text.clone());
            } else {
                return Err(EvalError::new(format!("{name} unexpected keyword `{text}`")));
            }
            continue;
        }
        match expr {
            BoundExpr::Call { spec: call_spec, args: inner } if call_spec.name.eq_ignore_ascii_case("ORDERBY") => {
                parse_order_by(inner, &mut spec.order)?;
            }
            BoundExpr::Call { spec: call_spec, args: inner } if call_spec.name.eq_ignore_ascii_case("PARTITIONBY") => {
                spec.partition.extend(inner.iter());
            }
            BoundExpr::Call { spec: call_spec, args: inner } if call_spec.name.eq_ignore_ascii_case("MATCHBY") => {
                spec.match_by.extend(inner.iter());
            }
            _ => {
                if spec.relation.is_none() {
                    spec.relation = Some(expr);
                }
                // Later positional arguments (Reset) do not affect an in-memory table.
            }
        }
    }
    Ok(spec)
}

/// A keyword argument, written either as a bare name (bound by the catalog's `Text` parameter
/// rule) or as a quoted string.
fn keyword_text(expr: &BoundExpr) -> Option<String> {
    match expr {
        BoundExpr::Keyword(name) => Some(name.clone()),
        BoundExpr::Literal(Literal::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn parse_order_by<'a>(inner: &'a [BoundExpr], order: &mut Vec<OrderKey<'a>>) -> Result<(), EvalError> {
    for expr in inner {
        if let Some(text) = keyword_text(expr) {
            let keyword = text.trim().to_ascii_uppercase();
            match keyword.as_str() {
                "ASC" | "DESC" => {
                    let key = order.last_mut().ok_or_else(|| EvalError::new("ORDERBY needs an expression before a sort direction"))?;
                    key.descending = keyword == "DESC";
                    continue;
                }
                "DEFAULT" | "FIRST" | "LAST" => {
                    let key = order.last_mut().ok_or_else(|| EvalError::new("ORDERBY needs an expression before a blanks option"))?;
                    key.blanks = Some(match keyword.as_str() {
                        "FIRST" => BlanksOrder::First,
                        "LAST" => BlanksOrder::Last,
                        _ => BlanksOrder::Default,
                    });
                    continue;
                }
                _ => {}
            }
        }
        order.push(OrderKey { source: KeySource::Expr(expr), descending: false, blanks: None });
    }
    Ok(())
}

fn window(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context, rank: bool) -> Result<Scalar, EvalError> {
    let spec = parse_window(name, exprs, rank)?;

    // The relation: an explicit table expression, or the table of the first ordering column.
    let table: TableValue;
    let relation_table: usize;
    if let Some(relation) = spec.relation {
        table = eval_table(ev, relation, ctx)?;
        relation_table = table.lineage.ok_or_else(|| EvalError::new(format!("{name} needs a relation table with model lineage")))?;
    } else if let Some(keyword) = spec.relation_keyword.as_deref() {
        relation_table =
            ev.model().table_index(keyword).ok_or_else(|| EvalError::new(format!("{name} expected a relation table, found `{keyword}`")))?;
        table = ev.table(relation_table, &ctx.filters);
    } else {
        let (candidate, _) = spec
            .order
            .iter()
            .find_map(|key| match key.source {
                KeySource::Expr(expr) => column_reference(expr),
                KeySource::Column(_) => None,
            })
            .ok_or_else(|| EvalError::new(format!("{name} needs a relation or an ORDERBY clause")))?;
        if ev.model().tables.get(candidate).is_none() {
            return Err(EvalError::new(format!("{name} could not resolve the relation table")));
        }
        relation_table = candidate;
        table = ev.table(relation_table, &ctx.filters);
    }

    let mut order = spec.order;
    if order.is_empty() {
        order = default_order_keys(ev, &table, relation_table, &spec.partition);
    }

    // Evaluate the key and partition values in each relation row's context.
    let mut rows: Vec<WindowRow> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let index = rows.len();
        ctx.rows.push(frame_for(ev, &table, row));
        let keys = evaluate_keys(ev, &order, ctx);
        let partition = evaluate_keys(
            ev,
            &spec.partition.iter().map(|expr| OrderKey { source: KeySource::Expr(expr), descending: false, blanks: None }).collect::<Vec<_>>(),
            ctx,
        );
        ctx.rows.pop();
        rows.push(WindowRow { index, keys: keys?, partition: partition? });
    }

    let current = current_row_index(ev, &table, relation_table, &spec.match_by, ctx)?;

    // Partition first, then the ordering keys; stable so equal rows keep the relation's order.
    rows.sort_by(|left, right| {
        compare_partition(&left.partition, &right.partition).then_with(|| compare_keys(&order, spec.blanks, &left.keys, &right.keys))
    });

    let mut partition_start = 0usize;
    let mut dense_rank = 0i64;
    let mut skip_rank = 0i64;
    let mut previous_keys: Option<&[Scalar]> = None;
    let mut previous_partition: Option<&[Scalar]> = None;
    for (position, row) in rows.iter().enumerate() {
        let new_partition = previous_partition.is_none_or(|partition| compare_partition(partition, &row.partition) != Ordering::Equal);
        if new_partition {
            partition_start = position;
            dense_rank = 0;
            skip_rank = 0;
            previous_keys = None;
            previous_partition = Some(&row.partition);
        }
        let same_group = !new_partition && previous_keys.is_some_and(|keys| compare_keys(&order, spec.blanks, keys, &row.keys) == Ordering::Equal);
        if !same_group {
            dense_rank += 1;
            skip_rank = (position - partition_start) as i64 + 1;
        }
        if row.index == current {
            let value = if rank { if spec.dense { dense_rank } else { skip_rank } } else { (position - partition_start) as i64 + 1 };
            return Ok(Scalar::Integer(value));
        }
        previous_keys = Some(&row.keys);
    }
    Err(EvalError::new(format!("{name} could not locate the current row in the relation")))
}

/// When `ORDERBY` is omitted, the relation is ordered by every model column that is not part of
/// the partition (dax.guide documents this default for the `Relation` argument).
fn default_order_keys<'a>(ev: &Evaluator<'_>, table: &TableValue, relation_table: usize, partition: &[&'a BoundExpr]) -> Vec<OrderKey<'a>> {
    let mut excluded = BTreeSet::new();
    for expr in partition {
        if let Some((table_index, column)) = column_reference(expr)
            && table_index == relation_table
        {
            excluded.insert(column);
        }
    }
    let mut seen = BTreeSet::new();
    let mut keys = Vec::new();
    for column in &table.columns {
        if let Some(index) = ev.model().column_index(relation_table, &column.name)
            && !excluded.contains(&index)
            && seen.insert(index)
        {
            keys.push(OrderKey { source: KeySource::Column(index), descending: false, blanks: None });
        }
    }
    keys
}

fn evaluate_keys(ev: &Evaluator<'_>, keys: &[OrderKey<'_>], ctx: &mut Context) -> Result<Vec<Scalar>, EvalError> {
    let mut values = Vec::with_capacity(keys.len());
    for key in keys {
        let value = match key.source {
            KeySource::Expr(expr) => ev.eval(expr, ctx)?,
            KeySource::Column(column) => ctx
                .rows
                .last()
                .and_then(|frame| frame.get(column))
                .cloned()
                .ok_or_else(|| EvalError::new("window ordering column is not in the current row"))?,
        };
        values.push(value);
    }
    Ok(values)
}

/// Identifies the current row by the innermost frame of the relation's table (or by a
/// single-value filter context when there is no row context).
fn current_row_index(
    ev: &Evaluator<'_>,
    table: &TableValue,
    relation_table: usize,
    match_by: &[&BoundExpr],
    ctx: &Context,
) -> Result<usize, EvalError> {
    let frame = ctx.rows.iter().rev().find(|frame| frame.lineage == Some(relation_table));
    let mut wanted: Vec<(usize, Scalar)> = Vec::new();
    if let Some(frame) = frame {
        if match_by.is_empty() {
            for (position, column) in frame.model_columns.iter().enumerate() {
                if let (Some(column), Some(value)) = (column, frame.values.get(position)) {
                    wanted.push((*column, value.clone()));
                }
            }
        } else {
            for expr in match_by {
                let (_, column) = resolve_column_reference(ev, expr, ctx, "MATCHBY")?;
                let value = frame.get(column).cloned().ok_or_else(|| EvalError::new("MATCHBY column is not part of the current row"))?;
                wanted.push((column, value));
            }
        }
    } else {
        for (&(table_index, column), values) in &ctx.filters.filters {
            if table_index != relation_table || values.len() != 1 {
                continue;
            }
            if let Some(value) = values.iter().next() {
                wanted.push((column, value.to_scalar()));
            }
        }
    }
    if wanted.is_empty() {
        return Err(EvalError::new("RANK/ROWNUMBER requires a row context or a single-value filter context"));
    }
    table
        .rows
        .iter()
        .position(|row| wanted.iter().all(|(column, value)| row.get(*column).is_some_and(|candidate| values_match(candidate, value))))
        .ok_or_else(|| EvalError::new("RANK/ROWNUMBER could not match the current row in the relation"))
}

fn compare_values(left: &Scalar, right: &Scalar, descending: bool, blanks: BlanksOrder) -> Ordering {
    match (left.is_blank(), right.is_blank()) {
        (true, true) => Ordering::Equal,
        (true, false) => match blanks {
            BlanksOrder::First => Ordering::Less,
            BlanksOrder::Last => Ordering::Greater,
            BlanksOrder::Default => {
                let ordering = compare_non_blank(left, right);
                if descending { ordering.reverse() } else { ordering }
            }
        },
        (false, true) => match blanks {
            BlanksOrder::First => Ordering::Greater,
            BlanksOrder::Last => Ordering::Less,
            BlanksOrder::Default => {
                let ordering = compare_non_blank(left, right);
                if descending { ordering.reverse() } else { ordering }
            }
        },
        (false, false) => {
            let ordering = compare_non_blank(left, right);
            if descending { ordering.reverse() } else { ordering }
        }
    }
}

fn compare_non_blank(left: &Scalar, right: &Scalar) -> Ordering {
    left.compare(right).unwrap_or_else(|| left.to_text().to_lowercase().cmp(&right.to_text().to_lowercase()))
}

fn compare_keys(keys: &[OrderKey<'_>], default_blanks: BlanksOrder, left: &[Scalar], right: &[Scalar]) -> Ordering {
    for (index, key) in keys.iter().enumerate() {
        let (Some(a), Some(b)) = (left.get(index), right.get(index)) else {
            continue;
        };
        let ordering = compare_values(a, b, key.descending, key.blanks.unwrap_or(default_blanks));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn compare_partition(left: &[Scalar], right: &[Scalar]) -> Ordering {
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = compare_values(a, b, false, BlanksOrder::Default);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn values_match(left: &Scalar, right: &Scalar) -> bool {
    match left.compare(right) {
        Some(ordering) => ordering == Ordering::Equal,
        None => left.strict_eq(right),
    }
}

// ---------------------------------------------------------------------------------------------
// INFO.STORAGETABLES
// ---------------------------------------------------------------------------------------------

/// The documented columns of `INFO.STORAGETABLES` (the `DISCOVER_STORAGE_TABLES` view). The
/// in-memory evaluator has no storage layer, so the table carries the schema with no rows.
pub(crate) const STORAGE_TABLES_COLUMNS: [&str; 9] = [
    "Database_Name",
    "Cube_Name",
    "Measure_Group_Name",
    "Dimension_Name",
    "Table_ID",
    "Table_Partitions_Count",
    "Hint_Table_Type",
    "Rows_Count",
    "RIViolation_Count",
];

fn storage_tables(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if !exprs.len().is_multiple_of(2) {
        return Err(EvalError::new(format!("{name} restrictions must be name/value pairs")));
    }
    for pair in exprs.chunks(2) {
        let restriction = ev.eval(&pair[0], ctx)?;
        if !matches!(restriction, Scalar::String(_) | Scalar::Blank) {
            return Err(EvalError::new(format!("{name} restriction names must be text")));
        }
        // Restriction values are evaluated for their side effects and errors; there are no rows
        // to restrict in an empty storage table.
        ev.eval(&pair[1], ctx)?;
    }
    let columns = STORAGE_TABLES_COLUMNS.iter().map(|name| TableColumn { name: (*name).to_string() }).collect();
    Ok(TableValue::new(columns, Vec::new()))
}

// ---------------------------------------------------------------------------------------------
// SAMPLECARTESIANPOINTSBYCOVER
// ---------------------------------------------------------------------------------------------

/// Scatter-chart sampling: plot each row at `(XAxis, YAxis)`, optionally sized by `Radius`, and
/// remove points that land on a grid cell already covered by a kept point. The grid has about
/// `sqrt(Size)` cells per axis, so at most `Size` points survive and the work is bounded by the
/// relation's row count.
fn sample_cartesian(ev: &Evaluator<'_>, name: &str, exprs: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    if exprs.len() < 4 {
        return Err(EvalError::new(format!("{name} needs Size, Table, XAxis and YAxis")));
    }
    let size = positive_size(ev, name, &exprs[0], ctx)?;
    let table = eval_table(ev, &exprs[1], ctx)?;
    let max_min_ratio = match exprs.get(5) {
        Some(expr) => Some(positive_number(ev, name, expr, 6, ctx)?),
        None => None,
    };
    let max_blank_ratio = match exprs.get(6) {
        Some(expr) => Some(positive_number(ev, name, expr, 7, ctx)?),
        None => None,
    };

    let mut points: Vec<(usize, f64, f64, Option<f64>)> = Vec::new();
    for (index, row) in table.rows.iter().enumerate() {
        ctx.rows.push(frame_for(ev, &table, row));
        let x = ev.eval(&exprs[2], ctx);
        let y = ev.eval(&exprs[3], ctx);
        let radius = match exprs.get(4) {
            Some(expr) => ev.eval(expr, ctx).map(|value| value.to_number().filter(|number| number.is_finite() && *number > 0.0)),
            None => Ok(None),
        };
        ctx.rows.pop();
        let (Some(x), Some(y), radius) = (coordinate(&x?), coordinate(&y?), radius?) else {
            continue;
        };
        points.push((index, x, y, radius));
    }
    if points.is_empty() {
        return Ok(TableValue::new(table.columns, Vec::new()).with_lineage_if(table.lineage));
    }

    let x_min = points.iter().map(|point| point.1).fold(f64::INFINITY, f64::min);
    let x_max = points.iter().map(|point| point.1).fold(f64::NEG_INFINITY, f64::max);
    let y_min = points.iter().map(|point| point.2).fold(f64::INFINITY, f64::min);
    let y_max = points.iter().map(|point| point.2).fold(f64::NEG_INFINITY, f64::max);
    let cells = (size as f64).sqrt().ceil().clamp(1.0, 1024.0) as usize;
    let max_radius = points.iter().filter_map(|point| point.3).fold(0.0, f64::max);
    let min_radius = max_min_ratio.filter(|_| max_radius > 0.0).map(|ratio| max_radius / ratio).unwrap_or(0.0);
    let blank_radius = max_blank_ratio.filter(|_| max_radius > 0.0).map(|ratio| max_radius / ratio).unwrap_or(0.0);
    let (width, height) = (x_max - x_min, y_max - y_min);

    let mut occupied: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut kept: Vec<Vec<Scalar>> = Vec::new();
    for (index, x, y, radius) in &points {
        if kept.len() >= size {
            break;
        }
        let radius = radius.map(|value| value.max(min_radius)).unwrap_or(blank_radius);
        let span = cell_span(radius, width, height, cells);
        let cell_x = grid_cell(*x, x_min, x_max, cells);
        let cell_y = grid_cell(*y, y_min, y_max, cells);
        let x_range = cell_x.saturating_sub(span)..=cell_x.saturating_add(span).min(cells - 1);
        let y_range = cell_y.saturating_sub(span)..=cell_y.saturating_add(span).min(cells - 1);
        if y_range.clone().any(|row| x_range.clone().any(|column| occupied.contains(&(column, row)))) {
            continue;
        }
        for row in y_range {
            for column in x_range.clone() {
                occupied.insert((column, row));
            }
        }
        if let Some(source) = table.rows.get(*index) {
            kept.push(source.clone());
        }
    }
    Ok(TableValue::new(table.columns, kept).with_lineage_if(table.lineage))
}

fn coordinate(value: &Scalar) -> Option<f64> {
    value.to_number().filter(|number| number.is_finite())
}

fn positive_size(ev: &Evaluator<'_>, name: &str, expr: &BoundExpr, ctx: &mut Context) -> Result<usize, EvalError> {
    let value = ev.eval(expr, ctx)?;
    let number = value.to_number().ok_or_else(|| EvalError::new(format!("{name} Size must be numeric")))?;
    if !number.is_finite() || number < 1.0 {
        return Err(EvalError::new(format!("{name} Size must be a positive number")));
    }
    // Values above this cannot produce a useful sample and would only invite huge loops.
    Ok(number.trunc().min(1_000_000.0) as usize)
}

fn positive_number(ev: &Evaluator<'_>, name: &str, expr: &BoundExpr, position: usize, ctx: &mut Context) -> Result<f64, EvalError> {
    let value = ev.eval(expr, ctx)?;
    let number = value.to_number().ok_or_else(|| EvalError::new(format!("{name} argument {position} must be numeric")))?;
    if !number.is_finite() || number <= 0.0 {
        return Err(EvalError::new(format!("{name} argument {position} must be a positive number")));
    }
    Ok(number)
}

fn grid_cell(value: f64, min: f64, max: f64, cells: usize) -> usize {
    if !value.is_finite() || !max.is_finite() || !min.is_finite() || max <= min {
        return 0;
    }
    let ratio = ((value - min) / (max - min)).clamp(0.0, 1.0);
    let cell = (ratio * cells as f64).floor();
    if !cell.is_finite() {
        return 0;
    }
    (cell as usize).min(cells.saturating_sub(1))
}

/// Number of neighbouring cells a point's radius covers, capped so a single point can never
/// touch more than an 9x9 neighbourhood.
fn cell_span(radius: f64, width: f64, height: f64, cells: usize) -> usize {
    let extent = width.max(height);
    if !radius.is_finite() || radius <= 0.0 || !extent.is_finite() || extent <= 0.0 {
        return 0;
    }
    let cell_size = extent / cells.max(1) as f64;
    if !cell_size.is_finite() || cell_size <= 0.0 {
        return 0;
    }
    (radius / cell_size).ceil().clamp(0.0, 4.0) as usize
}
