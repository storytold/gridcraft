//! Applied steps: each takes a table and returns the transformed table. Column names match
//! exactly, comparisons of text are ordinal (case-sensitive) and null sorts first, as in Power
//! Query.

use std::cmp::Ordering;
use std::collections::HashMap;

use gridcraft_core::Value;
use gridcraft_model::query::{
    AggOp, ColType, ConditionalRule, FilterCondition, FilterOp, JoinKind, Literal, Output, SortKey, SplitMode, StepOp, TextFn,
};

use super::types::{Culture, convert};
use super::{EvalCx, MAX_COLUMNS, MAX_ROWS, QResult, QTable, unique_name};

/// Applies one step.
pub fn apply(cx: &mut EvalCx<'_>, t: QTable, op: &StepOp) -> QResult<QTable> {
    let sys = cx.wb.date_system;
    match op {
        StepOp::PromoteHeaders => Ok(promote(t)),
        StepOp::DemoteHeaders => Ok(demote(t)),
        StepOp::ChangeType { columns, locale } => {
            let cul = Culture::of(locale.as_deref());
            let mut t = t;
            for ct in columns {
                let i = t.col(&ct.column)?;
                for row in t.rows.iter_mut() {
                    if let Some(v) = row.get_mut(i) {
                        *v = convert(v, ct.col_type, &cul, sys);
                    }
                }
                if let Some(ty) = t.types.get_mut(i) {
                    *ty = ct.col_type;
                }
            }
            Ok(t)
        }
        StepOp::ChooseColumns { columns } => select(t, columns),
        StepOp::RemoveColumns { columns } => {
            let idx = t.cols(columns)?;
            let keep: Vec<usize> = (0..t.columns.len()).filter(|i| !idx.contains(i)).collect();
            Ok(t.project(&keep))
        }
        StepOp::ReorderColumns { columns } => {
            let mut order = t.cols(columns)?;
            for i in 0..t.columns.len() {
                if !order.contains(&i) {
                    order.push(i);
                }
            }
            Ok(t.project(&order))
        }
        StepOp::RenameColumns { renames } => {
            let mut t = t;
            for r in renames {
                let i = t.col(&r.from)?;
                if r.to.trim().is_empty() {
                    return Err("A column name can't be empty.".into());
                }
                if t.columns.iter().enumerate().any(|(j, c)| j != i && *c == r.to) {
                    return Err(format!("The field '{}' already exists in the record.", r.to));
                }
                if let Some(c) = t.columns.get_mut(i) {
                    c.clone_from(&r.to);
                }
            }
            Ok(t)
        }
        StepOp::DuplicateColumn { column, name } => {
            let i = t.col(column)?;
            let name = if name.trim().is_empty() { format!("{column} - Copy") } else { name.clone() };
            let ty = t.types.get(i).copied().unwrap_or_default();
            add_column(t, &name, ty, |row| row.get(i).cloned().unwrap_or_default())
        }
        StepOp::KeepTopRows { count } => {
            let mut t = t;
            t.rows.truncate(clamp(*count));
            Ok(t)
        }
        StepOp::KeepBottomRows { count } => {
            let mut t = t;
            let n = t.rows.len().saturating_sub(clamp(*count));
            t.rows.drain(..n);
            Ok(t)
        }
        StepOp::KeepRange { first, count } => {
            let mut t = t;
            let start = clamp(*first).saturating_sub(1).min(t.rows.len());
            t.rows.drain(..start);
            t.rows.truncate(clamp(*count));
            Ok(t)
        }
        StepOp::RemoveTopRows { count } => {
            let mut t = t;
            let n = clamp(*count).min(t.rows.len());
            t.rows.drain(..n);
            Ok(t)
        }
        StepOp::RemoveBottomRows { count } => {
            let mut t = t;
            let n = t.rows.len().saturating_sub(clamp(*count));
            t.rows.truncate(n);
            Ok(t)
        }
        StepOp::RemoveDuplicates { columns } => {
            let idx = if columns.is_empty() { (0..t.columns.len()).collect() } else { t.cols(columns)? };
            let mut seen = std::collections::HashSet::new();
            let mut t = t;
            t.rows.retain(|row| seen.insert(idx.iter().map(|&i| key(row.get(i))).collect::<Vec<_>>()));
            Ok(t)
        }
        StepOp::RemoveBlankRows => {
            let mut t = t;
            t.rows.retain(|row| !row.iter().all(|v| matches!(v, Value::Empty) || v.as_text().is_some_and(str::is_empty)));
            Ok(t)
        }
        StepOp::RemoveErrors { columns } => {
            let idx = if columns.is_empty() { (0..t.columns.len()).collect() } else { t.cols(columns)? };
            let mut t = t;
            t.rows.retain(|row| !idx.iter().any(|&i| row.get(i).is_some_and(Value::is_error)));
            Ok(t)
        }
        StepOp::FilterRows { column, values, conditions, or } => {
            let i = t.col(column)?;
            let mut t = t;
            t.rows.retain(|row| {
                let v = row.get(i).unwrap_or(&Value::Empty);
                let in_list = values.as_ref().is_none_or(|list| list.iter().any(|l| equals(v, l.as_ref(), sys)));
                let conds = conditions.is_empty()
                    || if *or { conditions.iter().any(|c| holds(v, c, sys)) } else { conditions.iter().all(|c| holds(v, c, sys)) };
                in_list && conds
            });
            Ok(t)
        }
        StepOp::Sort { keys } => sort(t, keys),
        StepOp::ReplaceValues { columns, find, replace, whole } => {
            let idx = t.cols(columns)?;
            let mut t = t;
            for row in t.rows.iter_mut() {
                for &i in &idx {
                    if let Some(v) = row.get_mut(i) {
                        *v = replaced(v, find.as_ref(), replace.as_ref(), *whole, sys);
                    }
                }
            }
            Ok(t)
        }
        StepOp::SplitColumn { column, delimiter, mode, count } => split(t, column, delimiter, *mode, *count),
        StepOp::TransformText { columns, function } => {
            let idx = t.cols(columns)?;
            let mut t = t;
            for row in t.rows.iter_mut() {
                for &i in &idx {
                    if let Some(v) = row.get_mut(i)
                        && let Value::Text(s) = v
                    {
                        *v = Value::text(text_fn(s, *function));
                    }
                }
            }
            Ok(t)
        }
        StepOp::FillDown { columns } => Ok(fill(t, columns, false)?),
        StepOp::FillUp { columns } => Ok(fill(t, columns, true)?),
        StepOp::GroupBy { keys, aggregations } => group_by(t, keys, aggregations),
        StepOp::Unpivot { columns, attribute, value } => unpivot(t, columns, attribute, value),
        StepOp::Transpose => transpose(t),
        StepOp::MergeQueries { query, left, right, join, columns, prefix } => {
            let other = cx.evaluate_named(query)?;
            merge(t, &other, left, right, *join, columns, prefix.as_deref())
        }
        StepOp::AppendQueries { queries } => {
            let mut t = t;
            for q in queries {
                let other = cx.evaluate_named(q)?;
                t = append(t, other)?;
            }
            Ok(t)
        }
        StepOp::AddIndexColumn { name, start, step } => {
            let mut n = 0i64;
            let (start, step) = (*start, *step);
            add_column(t, name, ColType::Whole, move |_| {
                let v = start.saturating_add(step.saturating_mul(n));
                n = n.saturating_add(1);
                Value::number(v as f64)
            })
        }
        StepOp::AddConditionalColumn { name, rules, otherwise } => conditional(t, name, rules, otherwise, sys),
    }
}

fn clamp(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

// ---------------------------------------------------------------- values

/// A hashable form of a value for distinct, group and join keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Null,
    Num(u64),
    Text(String),
    Bool(bool),
    Err(u32),
}

pub fn key(v: Option<&Value>) -> Key {
    match v {
        None | Some(Value::Empty) => Key::Null,
        Some(Value::Number(n)) => Key::Num(if *n == 0.0 { 0 } else { n.to_bits() }),
        Some(Value::Text(t)) => Key::Text(t.to_string()),
        Some(Value::Bool(b)) => Key::Bool(*b),
        Some(Value::Error(e)) => Key::Err(e.code()),
        Some(Value::Array(_)) => Key::Err(0),
    }
}

/// Power Query's order: null first, then logical, numbers, text; text by code point.
pub fn compare(a: &Value, b: &Value) -> Ordering {
    let rank = |v: &Value| match v {
        Value::Empty => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::Text(_) => 3,
        Value::Error(_) | Value::Array(_) => 4,
    };
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    }
}

fn literal_value(l: Option<&Literal>) -> Value {
    match l {
        None => Value::Empty,
        Some(Literal::Bool(b)) => Value::Bool(*b),
        Some(Literal::Number(n)) => Value::number(*n),
        Some(Literal::Text(t)) => Value::text(t.as_str()),
    }
}

/// The literal in the value's own kind where it can be (`"5"` against a number is 5, and a date
/// typed as `2026-01-31` or `1/31/2026` against a date serial is that serial).
fn literal_like(v: &Value, l: Option<&Literal>, sys: gridcraft_core::DateSystem) -> Value {
    match (v, l) {
        (Value::Number(_), Some(Literal::Text(t))) => t
            .trim()
            .parse::<f64>()
            .ok()
            .or_else(|| super::types::parse_datetime(t.trim(), &super::types::Culture::of(None), sys).map(|(n, _)| n))
            .map(Value::number)
            .unwrap_or_else(|| Value::text(t.as_str())),
        (Value::Text(_), Some(Literal::Number(n))) => Value::text(gridcraft_core::number_to_text(*n)),
        (Value::Bool(_), Some(Literal::Text(t))) if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") => {
            Value::Bool(t.eq_ignore_ascii_case("true"))
        }
        _ => literal_value(l),
    }
}

/// Equality for filters and replacements; null also matches empty text (the "(blank)" item).
fn equals(v: &Value, l: Option<&Literal>, sys: gridcraft_core::DateSystem) -> bool {
    if l.is_none() {
        return matches!(v, Value::Empty) || v.as_text().is_some_and(str::is_empty);
    }
    key(Some(v)) == key(Some(&literal_like(v, l, sys)))
}

/// Whether `v` meets a filter condition. Comparisons with null fail, as in Power Query.
pub fn holds(v: &Value, c: &FilterCondition, sys: gridcraft_core::DateSystem) -> bool {
    test(v, c.op, c.value.as_ref(), sys)
}

fn test(v: &Value, op: FilterOp, lit: Option<&Literal>, sys: gridcraft_core::DateSystem) -> bool {
    let text = |v: &Value| match v {
        Value::Text(t) => Some(t.to_string()),
        Value::Number(n) => Some(gridcraft_core::number_to_text(*n)),
        _ => None,
    };
    let lit_text = || match lit {
        Some(Literal::Text(t)) => t.clone(),
        Some(Literal::Number(n)) => gridcraft_core::number_to_text(*n),
        Some(Literal::Bool(b)) => b.to_string(),
        None => String::new(),
    };
    match op {
        FilterOp::IsNull => matches!(v, Value::Empty),
        FilterOp::IsNotNull => !matches!(v, Value::Empty),
        FilterOp::Equals => equals(v, lit, sys),
        FilterOp::NotEquals => !equals(v, lit, sys),
        FilterOp::BeginsWith => text(v).is_some_and(|t| t.starts_with(&lit_text())),
        FilterOp::NotBeginsWith => text(v).is_some_and(|t| !t.starts_with(&lit_text())),
        FilterOp::EndsWith => text(v).is_some_and(|t| t.ends_with(&lit_text())),
        FilterOp::NotEndsWith => text(v).is_some_and(|t| !t.ends_with(&lit_text())),
        FilterOp::Contains => text(v).is_some_and(|t| t.contains(&lit_text())),
        FilterOp::NotContains => text(v).is_some_and(|t| !t.contains(&lit_text())),
        FilterOp::GreaterThan | FilterOp::GreaterOrEqual | FilterOp::LessThan | FilterOp::LessOrEqual => {
            let other = literal_like(v, lit, sys);
            if matches!(v, Value::Empty | Value::Error(_)) || matches!(other, Value::Empty) {
                return false;
            }
            let same_kind =
                matches!((v, &other), (Value::Number(_), Value::Number(_)) | (Value::Text(_), Value::Text(_)) | (Value::Bool(_), Value::Bool(_)));
            if !same_kind {
                return false;
            }
            let o = compare(v, &other);
            match op {
                FilterOp::GreaterThan => o == Ordering::Greater,
                FilterOp::GreaterOrEqual => o != Ordering::Less,
                FilterOp::LessThan => o == Ordering::Less,
                _ => o != Ordering::Greater,
            }
        }
    }
}

fn replaced(v: &Value, find: Option<&Literal>, replace: Option<&Literal>, whole: bool, sys: gridcraft_core::DateSystem) -> Value {
    match (v, find) {
        (Value::Text(s), Some(Literal::Text(f))) if !whole && !f.is_empty() => {
            let r = match replace {
                Some(Literal::Text(r)) => r.clone(),
                Some(Literal::Number(n)) => gridcraft_core::number_to_text(*n),
                Some(Literal::Bool(b)) => b.to_string(),
                None => String::new(),
            };
            if s.contains(f.as_str()) {
                let out = s.replace(f.as_str(), &r);
                Value::text(out.chars().take(1_000_000).collect::<String>())
            } else {
                v.clone()
            }
        }
        _ if equals(v, find, sys) => literal_value(replace),
        _ => v.clone(),
    }
}

fn text_fn(s: &str, f: TextFn) -> String {
    match f {
        TextFn::Lower => s.to_lowercase(),
        TextFn::Upper => s.to_uppercase(),
        TextFn::Trim => s.trim().to_string(),
        TextFn::Clean => s.chars().filter(|c| !c.is_control()).collect(),
        TextFn::Proper => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for c in s.chars() {
                if c.is_alphabetic() {
                    if start {
                        out.extend(c.to_uppercase());
                    } else {
                        out.extend(c.to_lowercase());
                    }
                    start = false;
                } else {
                    out.push(c);
                    start = !c.is_numeric() && c != '\'';
                }
            }
            out
        }
    }
}

// ---------------------------------------------------------------- columns

fn select(t: QTable, columns: &[String]) -> QResult<QTable> {
    let idx = t.cols(columns)?;
    Ok(t.project(&idx))
}

/// Adds a column at the end, its values from `f` per row.
fn add_column(mut t: QTable, name: &str, ty: ColType, mut f: impl FnMut(&[Value]) -> Value) -> QResult<QTable> {
    if t.columns.len() >= MAX_COLUMNS {
        return Err("The table has too many columns.".into());
    }
    if name.trim().is_empty() {
        return Err("A column name can't be empty.".into());
    }
    if t.columns.iter().any(|c| c == name) {
        return Err(format!("The field '{name}' already exists in the record."));
    }
    for row in t.rows.iter_mut() {
        let v = f(row);
        row.push(v);
    }
    t.columns.push(name.to_string());
    t.types.push(ty);
    Ok(t)
}

fn promote(t: QTable) -> QTable {
    let mut rows = t.rows.into_iter();
    let Some(first) = rows.next() else {
        return QTable { columns: t.columns, types: vec![ColType::Any; t.types.len()], rows: vec![] };
    };
    let mut columns: Vec<String> = first
        .iter()
        .enumerate()
        .map(|(i, v)| match v {
            Value::Empty => format!("Column{}", i + 1),
            Value::Text(s) if s.is_empty() => format!("Column{}", i + 1),
            v => v.display(),
        })
        .collect();
    super::unique_names(&mut columns);
    let width = columns.len();
    QTable { columns, types: vec![ColType::Any; width], rows: rows.collect() }
}

fn demote(t: QTable) -> QTable {
    let width = t.columns.len();
    let mut rows = Vec::with_capacity(t.rows.len() + 1);
    rows.push(t.columns.iter().map(|c| Value::text(c.as_str())).collect());
    rows.extend(t.rows.into_iter().take(MAX_ROWS.saturating_sub(1)));
    QTable { columns: (1..=width).map(|i| format!("Column{i}")).collect(), types: vec![ColType::Any; width], rows }
}

fn sort(t: QTable, keys: &[SortKey]) -> QResult<QTable> {
    let idx: Vec<(usize, bool)> = keys.iter().map(|k| t.col(&k.column).map(|i| (i, k.descending))).collect::<QResult<_>>()?;
    let mut t = t;
    t.rows.sort_by(|a, b| {
        for &(i, desc) in &idx {
            let o = compare(a.get(i).unwrap_or(&Value::Empty), b.get(i).unwrap_or(&Value::Empty));
            let o = if desc { o.reverse() } else { o };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
    Ok(t)
}

fn split(t: QTable, column: &str, delimiter: &str, mode: SplitMode, count: Option<u32>) -> QResult<QTable> {
    if delimiter.is_empty() {
        return Err("Enter a delimiter to split by.".into());
    }
    let i = t.col(column)?;
    let parts_of = |v: &Value| -> Vec<Value> {
        let s = match v {
            Value::Empty => return vec![],
            Value::Text(s) => s.to_string(),
            other => other.display(),
        };
        let pieces: Vec<String> = match mode {
            SplitMode::Each => s.split(delimiter).map(str::to_string).collect(),
            SplitMode::LeftMost => match s.split_once(delimiter) {
                Some((a, b)) => vec![a.to_string(), b.to_string()],
                None => vec![s],
            },
            SplitMode::RightMost => match s.rsplit_once(delimiter) {
                Some((a, b)) => vec![a.to_string(), b.to_string()],
                None => vec![s],
            },
        };
        pieces.into_iter().map(Value::text).collect()
    };
    let room = MAX_COLUMNS.saturating_sub(t.columns.len()).saturating_add(1);
    let n = match (mode, count) {
        (SplitMode::Each, Some(c)) => (c as usize).clamp(1, room),
        (SplitMode::Each, None) => {
            t.rows.iter().map(|r| r.get(i).map(|v| parts_of(v).len()).unwrap_or(0)).max().unwrap_or(1).clamp(1, room.min(1024))
        }
        _ => 2usize.min(room),
    };
    let mut t = t;
    let names: Vec<String> = (1..=n).map(|k| unique_name(&t.columns, &format!("{column}.{k}"))).collect();
    for row in t.rows.iter_mut() {
        let mut parts = row.get(i).map(parts_of).unwrap_or_default();
        if mode == SplitMode::Each && parts.len() > n {
            // The rest stays together in the last column.
            let tail: Vec<String> = parts.drain(n - 1..).map(|v| v.display()).collect();
            parts.push(Value::text(tail.join(delimiter)));
        }
        parts.resize(n, Value::Empty);
        row.splice(i..=i, parts);
    }
    t.columns.splice(i..=i, names);
    t.types.splice(i..=i, std::iter::repeat_n(ColType::Text, n));
    Ok(t)
}

fn fill(t: QTable, columns: &[String], up: bool) -> QResult<QTable> {
    let idx = t.cols(columns)?;
    let mut t = t;
    for &i in &idx {
        let mut last = Value::Empty;
        let n = t.rows.len();
        for k in 0..n {
            let r = if up { n - 1 - k } else { k };
            if let Some(v) = t.rows.get_mut(r).and_then(|row| row.get_mut(i)) {
                if matches!(v, Value::Empty) {
                    *v = last.clone();
                } else {
                    last = v.clone();
                }
            }
        }
    }
    Ok(t)
}

fn group_by(t: QTable, keys: &[String], aggs: &[gridcraft_model::query::Aggregation]) -> QResult<QTable> {
    let kidx = t.cols(keys)?;
    let aidx: Vec<Option<usize>> = aggs
        .iter()
        .map(|a| match (&a.column, a.op) {
            (_, AggOp::Count) => Ok(None),
            (Some(c), _) => t.col(c).map(Some),
            (None, _) => Err(format!("The aggregation '{}' needs a column.", a.name)),
        })
        .collect::<QResult<_>>()?;
    let mut order: Vec<Vec<Key>> = Vec::new();
    let mut groups: HashMap<Vec<Key>, (Vec<Value>, Vec<usize>)> = HashMap::new();
    for (r, row) in t.rows.iter().enumerate() {
        let k: Vec<Key> = kidx.iter().map(|&i| key(row.get(i))).collect();
        match groups.get_mut(&k) {
            Some(g) => g.1.push(r),
            None => {
                order.push(k.clone());
                groups.insert(k, (kidx.iter().map(|&i| row.get(i).cloned().unwrap_or_default()).collect(), vec![r]));
            }
        }
    }
    let mut columns: Vec<String> = keys.to_vec();
    let mut types: Vec<ColType> = kidx.iter().map(|&i| t.types.get(i).copied().unwrap_or_default()).collect();
    for a in aggs {
        if a.name.trim().is_empty() || columns.contains(&a.name) {
            return Err(format!("The new column name '{}' is empty or already used.", a.name));
        }
        columns.push(a.name.clone());
        types.push(match a.op {
            AggOp::Count | AggOp::CountDistinct => ColType::Whole,
            _ => ColType::Number,
        });
    }
    let mut out = QTable { columns, types, rows: Vec::with_capacity(order.len()) };
    for k in order {
        let Some((key_vals, members)) = groups.remove(&k) else { continue };
        let mut row = key_vals;
        for (a, col) in aggs.iter().zip(&aidx) {
            let vals = || members.iter().filter_map(|&r| col.and_then(|c| t.rows.get(r).and_then(|row| row.get(c))));
            let nums = || vals().filter_map(Value::as_f64).collect::<Vec<f64>>();
            row.push(match a.op {
                AggOp::Count => Value::number(members.len() as f64),
                AggOp::CountDistinct => {
                    let set: std::collections::HashSet<Key> = vals().map(|v| key(Some(v))).collect();
                    Value::number(set.len() as f64)
                }
                AggOp::Sum => Value::number(nums().iter().sum()),
                AggOp::Average => {
                    let n = nums();
                    if n.is_empty() { Value::Empty } else { Value::number(n.iter().sum::<f64>() / n.len() as f64) }
                }
                AggOp::Median => {
                    let mut n = nums();
                    n.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
                    match n.len() {
                        0 => Value::Empty,
                        l if l % 2 == 1 => n.get(l / 2).map(|x| Value::number(*x)).unwrap_or_default(),
                        l => match (n.get(l / 2 - 1), n.get(l / 2)) {
                            (Some(a), Some(b)) => Value::number((a + b) / 2.0),
                            _ => Value::Empty,
                        },
                    }
                }
                AggOp::Min => vals().filter(|v| !matches!(v, Value::Empty)).min_by(|a, b| compare(a, b)).cloned().unwrap_or_default(),
                AggOp::Max => vals().filter(|v| !matches!(v, Value::Empty)).max_by(|a, b| compare(a, b)).cloned().unwrap_or_default(),
            });
        }
        out.rows.push(row);
    }
    // Min/Max keep the column's own type.
    for (j, (a, col)) in aggs.iter().zip(&aidx).enumerate() {
        if matches!(a.op, AggOp::Min | AggOp::Max)
            && let (Some(c), Some(ty)) = (col, out.types.get_mut(kidx.len() + j))
        {
            *ty = t.types.get(*c).copied().unwrap_or_default();
        }
    }
    Ok(out)
}

fn unpivot(t: QTable, columns: &[String], attribute: &str, value: &str) -> QResult<QTable> {
    let idx = t.cols(columns)?;
    let keep: Vec<usize> = (0..t.columns.len()).filter(|i| !idx.contains(i)).collect();
    let mut names: Vec<String> = keep.iter().filter_map(|&i| t.columns.get(i).cloned()).collect();
    if names.iter().any(|n| n == attribute || n == value) || attribute == value {
        return Err("The attribute and value column names must be new and different.".into());
    }
    names.push(attribute.to_string());
    names.push(value.to_string());
    let mut types: Vec<ColType> = keep.iter().map(|&i| t.types.get(i).copied().unwrap_or_default()).collect();
    types.push(ColType::Text);
    types.push(ColType::Any);
    let mut out = QTable { columns: names, types, rows: vec![] };
    'rows: for row in &t.rows {
        for &i in &idx {
            let v = row.get(i).cloned().unwrap_or_default();
            if matches!(v, Value::Empty) {
                continue;
            }
            if out.rows.len() >= MAX_ROWS {
                break 'rows;
            }
            let mut r: Vec<Value> = keep.iter().map(|&k| row.get(k).cloned().unwrap_or_default()).collect();
            r.push(Value::text(t.columns.get(i).map(String::as_str).unwrap_or("")));
            r.push(v);
            out.rows.push(r);
        }
    }
    Ok(out)
}

fn transpose(t: QTable) -> QResult<QTable> {
    if t.rows.len() > MAX_COLUMNS {
        return Err(format!("Transposing would give more than {MAX_COLUMNS} columns."));
    }
    let width = t.rows.len();
    let mut out = QTable::with_columns((1..=width).map(|i| format!("Column{i}")).collect());
    for c in 0..t.columns.len() {
        out.push_row(t.rows.iter().map(|r| r.get(c).cloned().unwrap_or_default()).collect());
    }
    Ok(out)
}

fn merge(
    left: QTable,
    right: &QTable,
    lkeys: &[String],
    rkeys: &[String],
    join: JoinKind,
    columns: &[String],
    prefix: Option<&str>,
) -> QResult<QTable> {
    if lkeys.is_empty() || lkeys.len() != rkeys.len() {
        return Err("Select the same number of matching columns in both tables.".into());
    }
    let li = left.cols(lkeys)?;
    let ri = right.cols(rkeys)?;
    let expand: Vec<usize> = if columns.is_empty() { (0..right.columns.len()).filter(|i| !ri.contains(i)).collect() } else { right.cols(columns)? };
    let mut index: HashMap<Vec<Key>, Vec<usize>> = HashMap::new();
    for (r, row) in right.rows.iter().enumerate() {
        index.entry(ri.iter().map(|&i| key(row.get(i))).collect()).or_default().push(r);
    }
    let lw = left.columns.len();
    let mut names = left.columns.clone();
    for &i in &expand {
        let base = right.columns.get(i).cloned().unwrap_or_default();
        let name = match prefix {
            Some(p) if !p.is_empty() => format!("{p}.{base}"),
            _ => base,
        };
        names.push(unique_name(&names, &name));
    }
    if names.len() > MAX_COLUMNS {
        return Err("The merged table has too many columns.".into());
    }
    let mut types = left.types.clone();
    types.extend(expand.iter().map(|&i| right.types.get(i).copied().unwrap_or_default()));
    let right_part = |r: Option<usize>| -> Vec<Value> {
        expand.iter().map(|&i| r.and_then(|r| right.rows.get(r)).and_then(|row| row.get(i)).cloned().unwrap_or_default()).collect()
    };
    let mut rows = Vec::new();
    let mut matched_right = vec![false; right.rows.len()];
    let keeps_left_unmatched = matches!(join, JoinKind::LeftOuter | JoinKind::FullOuter | JoinKind::LeftAnti);
    for row in &left.rows {
        let k: Vec<Key> = li.iter().map(|&i| key(row.get(i))).collect();
        let hits = index.get(&k);
        match hits {
            Some(hits) if !hits.is_empty() => {
                for &r in hits {
                    if let Some(m) = matched_right.get_mut(r) {
                        *m = true;
                    }
                    if matches!(join, JoinKind::LeftOuter | JoinKind::FullOuter | JoinKind::Inner | JoinKind::RightOuter) {
                        if rows.len() >= MAX_ROWS {
                            return Err("The merge gives more rows than a worksheet holds.".into());
                        }
                        let mut out = row.clone();
                        out.extend(right_part(Some(r)));
                        rows.push(out);
                    }
                }
            }
            _ if keeps_left_unmatched => {
                let mut out = row.clone();
                out.extend(right_part(None));
                rows.push(out);
            }
            _ => {}
        }
    }
    if matches!(join, JoinKind::RightOuter | JoinKind::FullOuter | JoinKind::RightAnti) {
        for (r, m) in matched_right.iter().enumerate() {
            if !m && rows.len() < MAX_ROWS {
                let mut out = vec![Value::Empty; lw];
                out.extend(right_part(Some(r)));
                rows.push(out);
            }
        }
    }
    Ok(QTable { columns: names, types, rows })
}

fn append(mut t: QTable, other: QTable) -> QResult<QTable> {
    let map: Vec<usize> = other
        .columns
        .iter()
        .enumerate()
        .map(|(j, c)| match t.columns.iter().position(|x| x == c) {
            Some(i) => {
                if t.types.get(i) != other.types.get(j)
                    && let Some(ty) = t.types.get_mut(i)
                {
                    *ty = ColType::Any;
                }
                i
            }
            None => {
                t.columns.push(c.clone());
                t.types.push(other.types.get(j).copied().unwrap_or_default());
                t.columns.len() - 1
            }
        })
        .collect();
    if t.columns.len() > MAX_COLUMNS {
        return Err("The appended table has too many columns.".into());
    }
    let width = t.columns.len();
    for row in t.rows.iter_mut() {
        row.resize(width, Value::Empty);
    }
    for row in other.rows {
        if t.rows.len() >= MAX_ROWS {
            return Err("The appended table has more rows than a worksheet holds.".into());
        }
        let mut out = vec![Value::Empty; width];
        for (v, &i) in row.into_iter().zip(&map) {
            if let Some(slot) = out.get_mut(i) {
                *slot = v;
            }
        }
        t.rows.push(out);
    }
    Ok(t)
}

fn conditional(t: QTable, name: &str, rules: &[ConditionalRule], otherwise: &Output, sys: gridcraft_core::DateSystem) -> QResult<QTable> {
    let ridx: Vec<usize> = rules.iter().map(|r| t.col(&r.column)).collect::<QResult<_>>()?;
    let out_col = |o: &Output| -> QResult<Option<usize>> { o.column.as_deref().map(|c| t.col(c)).transpose() };
    let outs: Vec<Option<usize>> = rules.iter().map(|r| out_col(&r.output)).collect::<QResult<_>>()?;
    let else_col = out_col(otherwise)?;
    let value_of = |row: &[Value], o: &Output, col: Option<usize>| -> Value {
        match col {
            Some(c) => row.get(c).cloned().unwrap_or_default(),
            None => literal_value(o.value.as_ref()),
        }
    };
    add_column(t, name, ColType::Any, |row| {
        for ((r, &i), col) in rules.iter().zip(&ridx).zip(&outs) {
            if test(row.get(i).unwrap_or(&Value::Empty), r.op, r.value.as_ref(), sys) {
                return value_of(row, &r.output, *col);
            }
        }
        value_of(row, otherwise, else_col)
    })
}
