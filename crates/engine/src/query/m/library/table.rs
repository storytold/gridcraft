//! `Table.*`: the table functions the Power Query Editor writes (Change Type, Remove Columns,
//! Filter Rows, Sort, Group By, Merge and Expand, Pivot and Unpivot, Split, Add Column…), and
//! building tables (`#table`, `Table.FromRows`…).

use std::collections::HashMap;
use std::rc::Rc;

use gridcraft_model::query::ColType;

use super::super::eval::{Ev, is_limit};
use super::super::value::{Key, MError, MFunc, MRecord, MResult, MTable, MType, MValue, check_cells, key, sort_order};
use super::{Args, cant, convert, names_of, show, text};
use crate::query::MAX_COLUMNS;

pub fn call(ev: &mut Ev<'_, '_>, name: &str, a: &Args<'_>) -> MResult<Option<MValue>> {
    let r = match name {
        "#table" => from_rows(ev, a.at(1), a.at(0))?,
        "Table.FromRows" => from_rows(ev, a.at(0), a.at(1))?,
        "Table.FromRecords" => from_records(ev, a)?,
        "Table.FromList" => from_list(ev, a)?,
        "Table.FromColumns" => from_columns(a)?,
        "Table.PromoteHeaders" => promote_headers(ev, a)?,
        "Table.DemoteHeaders" => demote_headers(a)?,
        "Table.TransformColumnTypes" => transform_column_types(ev, a)?,
        "Table.SelectColumns" => select_columns(a)?,
        "Table.RemoveColumns" => remove_columns(a)?,
        "Table.RenameColumns" => rename_columns(a)?,
        "Table.ReorderColumns" => reorder_columns(a)?,
        "Table.DuplicateColumn" => duplicate_column(ev, a)?,
        "Table.FirstN" => {
            let t = a.table(0)?;
            let n = count_or_condition(ev, &t, a.at(1), false)?;
            with_rows(&t, t.rows.iter().take(n).cloned().collect())
        }
        "Table.LastN" => {
            let t = a.table(0)?;
            let n = count_or_condition(ev, &t, a.at(1), true)?;
            with_rows(&t, t.rows.iter().skip(t.rows.len().saturating_sub(n)).cloned().collect())
        }
        "Table.Skip" | "Table.RemoveFirstN" => {
            let t = a.table(0)?;
            let n = if a.has(1) { count_or_condition(ev, &t, a.at(1), false)? } else { 1 };
            with_rows(&t, t.rows.iter().skip(n).cloned().collect())
        }
        "Table.RemoveLastN" => {
            let t = a.table(0)?;
            let n = if a.has(1) { count_or_condition(ev, &t, a.at(1), true)? } else { 1 };
            with_rows(&t, t.rows.iter().take(t.rows.len().saturating_sub(n)).cloned().collect())
        }
        "Table.Range" => {
            let t = a.table(0)?;
            let offset = a.count(1)?;
            let count = if a.has(2) { a.count(2)? } else { usize::MAX };
            with_rows(&t, t.rows.iter().skip(offset).take(count).cloned().collect())
        }
        "Table.RemoveRows" => {
            let t = a.table(0)?;
            let offset = a.count(1)?;
            let count = if a.has(2) { a.count(2)? } else { 1 };
            let end = offset.saturating_add(count);
            with_rows(&t, t.rows.iter().enumerate().filter(|(i, _)| *i < offset || *i >= end).map(|(_, r)| r.clone()).collect())
        }
        "Table.AlternateRows" => {
            let t = a.table(0)?;
            let (offset, skip, take) = (a.count(1)?, a.count(2)?, a.count(3)?);
            let cycle = skip.saturating_add(take).max(1);
            let rows = t.rows.iter().enumerate().filter(|(i, _)| *i < offset || (i - offset) % cycle >= skip).map(|(_, r)| r.clone()).collect();
            with_rows(&t, rows)
        }
        "Table.ReverseRows" => {
            let t = a.table(0)?;
            with_rows(&t, t.rows.iter().rev().cloned().collect())
        }
        "Table.Distinct" => distinct(ev, a)?,
        "Table.RemoveRowsWithErrors" => rows_with_errors(ev, a, false)?,
        "Table.SelectRowsWithErrors" => rows_with_errors(ev, a, true)?,
        "Table.SelectRows" => select_rows(ev, a)?,
        "Table.Sort" => sort(ev, a)?,
        "Table.ReplaceValue" => replace_value(ev, a)?,
        "Table.ReplaceErrorValues" => replace_error_values(a)?,
        "Table.SplitColumn" => split_column(ev, a)?,
        "Table.TransformColumns" => transform_columns(ev, a)?,
        "Table.TransformColumnNames" => {
            let t = a.table(0)?;
            let f = a.func(1)?;
            let mut names = Vec::with_capacity(t.width());
            for n in t.names.iter() {
                match ev.call1(&f, MValue::Text(n.clone()))? {
                    MValue::Text(s) => names.push(s),
                    other => return Err(cant(&other, "Text")),
                }
            }
            check_unique(&names)?;
            MValue::table(MTable { names: names.into(), types: t.types.clone(), rows: t.rows.clone() })
        }
        "Table.FillDown" => fill(a, false)?,
        "Table.FillUp" => fill(a, true)?,
        "Table.Group" => group(ev, a)?,
        "Table.UnpivotOtherColumns" => unpivot(ev, a, true)?,
        "Table.Unpivot" => unpivot(ev, a, false)?,
        "Table.Pivot" => pivot(ev, a)?,
        "Table.Transpose" => transpose(a)?,
        "Table.NestedJoin" => nested_join(ev, a)?,
        "Table.ExpandTableColumn" => expand(ev, a, Expand::Table)?,
        "Table.ExpandRecordColumn" => expand(ev, a, Expand::Record)?,
        "Table.ExpandListColumn" => expand(ev, a, Expand::List)?,
        "Table.Combine" => {
            let l = a.list(0)?;
            let mut r = combine(ev, &l)?;
            if a.has(1)
                && let MValue::Table(t) = &r
            {
                let names = a.names(1)?;
                r = MValue::table(project(t, &names, MissingField::UseNull)?);
            }
            r
        }
        "Table.AddIndexColumn" => {
            let t = a.table(0)?;
            let name = a.text(1)?;
            let start = if a.has(2) { a.num(2)? } else { 0.0 };
            let step = if a.has(3) { a.num(3)? } else { 1.0 };
            let ty = col_type_arg(a.at(4)).unwrap_or(ColType::Whole);
            let mut i = 0.0f64;
            add_column(&t, &name, ty, |_| {
                let v = MValue::Number(start + i * step);
                i += 1.0;
                Ok(v)
            })?
        }
        "Table.AddColumn" => {
            let t = a.table(0)?;
            let name = a.text(1)?;
            let f = a.func(2)?;
            let ty = col_type_arg(a.at(3)).unwrap_or(ColType::Any);
            let names = t.names.clone();
            add_column(&t, &name, ty, |row| held(ev.call1(&f, MValue::record(MRecord { names: names.clone(), values: row.to_vec() }))))?
        }
        "Table.CombineColumns" => combine_columns(ev, a)?,
        "Table.ColumnNames" => MValue::list(a.table(0)?.names.iter().map(|n| MValue::Text(n.clone())).collect()),
        "Table.RowCount" => MValue::Number(a.table(0)?.rows.len() as f64),
        "Table.ColumnCount" => MValue::Number(a.table(0)?.width() as f64),
        "Table.Column" => {
            let t = a.table(0)?;
            let c = t.col(&a.text(1)?)?;
            MValue::list(t.rows.iter().map(|r| r.get(c).cloned().unwrap_or(MValue::Null)).collect())
        }
        "Table.ToRows" => MValue::list(a.table(0)?.rows.iter().map(|r| MValue::list(r.clone())).collect()),
        "Table.ToRecords" => {
            let t = a.table(0)?;
            MValue::list(t.rows.iter().map(|r| MValue::record(t.row_record(r))).collect())
        }
        "Table.First" | "Table.Last" => {
            let t = a.table(0)?;
            let row = if name == "Table.First" { t.rows.first() } else { t.rows.last() };
            match row {
                Some(r) => MValue::record(t.row_record(r)),
                None => a.at(1).clone(),
            }
        }
        "Table.IsEmpty" => MValue::Logical(a.table(0)?.rows.is_empty()),
        "Table.Buffer" => MValue::Table(a.table(0)?),
        _ => return Ok(None),
    };
    Ok(Some(r))
}

// ---------------------------------------------------------------- helpers

/// A cell value from evaluating a row's expression: an error stays in the cell (limits end the
/// evaluation).
pub fn held(r: MResult<MValue>) -> MResult<MValue> {
    match r {
        Ok(v) => Ok(v),
        Err(e) if is_limit(&e) => Err(e),
        Err(e) => Ok(MValue::error(e)),
    }
}

fn with_rows(t: &MTable, rows: Vec<Vec<MValue>>) -> MValue {
    MValue::table(MTable { names: t.names.clone(), types: t.types.clone(), rows })
}

pub fn column_names(n: usize) -> Vec<Rc<str>> {
    (1..=n).map(|i| Rc::from(format!("Column{i}"))).collect()
}

fn check_unique(names: &[Rc<str>]) -> MResult<()> {
    for (i, n) in names.iter().enumerate() {
        if names.iter().take(i).any(|m| m == n) {
            return Err(MError::expr(format!("The field '{n}' already exists in the record.")));
        }
    }
    if names.len() > MAX_COLUMNS {
        return Err(MError::expr(format!("A table can have at most {MAX_COLUMNS} columns.")));
    }
    Ok(())
}

fn col_type_arg(v: &MValue) -> Option<ColType> {
    match v {
        MValue::Type(t) => Some(t.col_type()),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MissingField {
    Error,
    Ignore,
    UseNull,
}

fn missing(v: &MValue) -> MResult<MissingField> {
    match v {
        MValue::Null => Ok(MissingField::Error),
        MValue::Number(n) if *n == 1.0 => Ok(MissingField::Ignore),
        MValue::Number(n) if *n == 2.0 => Ok(MissingField::UseNull),
        MValue::Number(n) if *n == 0.0 => Ok(MissingField::Error),
        other => Err(cant(other, "MissingField")),
    }
}

/// The columns `names` of `t`, in that order.
fn project(t: &MTable, names: &[Rc<str>], mode: MissingField) -> MResult<MTable> {
    let mut idx: Vec<Option<usize>> = Vec::with_capacity(names.len());
    let mut kept: Vec<Rc<str>> = Vec::with_capacity(names.len());
    for n in names {
        match t.names.iter().position(|c| c == n) {
            Some(i) => {
                idx.push(Some(i));
                kept.push(n.clone());
            }
            None => match mode {
                MissingField::Error => return Err(MError::expr(format!("The column '{n}' of the table wasn't found."))),
                MissingField::Ignore => {}
                MissingField::UseNull => {
                    idx.push(None);
                    kept.push(n.clone());
                }
            },
        }
    }
    check_unique(&kept)?;
    check_cells(t.rows.len(), kept.len())?;
    let types = idx.iter().map(|i| i.and_then(|i| t.types.get(i).copied()).unwrap_or_default()).collect();
    let rows = t.rows.iter().map(|r| idx.iter().map(|i| i.and_then(|i| r.get(i).cloned()).unwrap_or(MValue::Null)).collect()).collect();
    Ok(MTable { names: kept.into(), types, rows })
}

fn add_column(t: &MTable, name: &Rc<str>, ty: ColType, mut f: impl FnMut(&[MValue]) -> MResult<MValue>) -> MResult<MValue> {
    if t.names.iter().any(|n| n == name) {
        return Err(MError::expr(format!("The field '{name}' already exists in the record.")));
    }
    if t.width() >= MAX_COLUMNS {
        return Err(MError::expr(format!("A table can have at most {MAX_COLUMNS} columns.")));
    }
    let mut names: Vec<Rc<str>> = t.names.to_vec();
    names.push(name.clone());
    let mut types = t.types.clone();
    types.push(ty);
    check_cells(t.rows.len(), names.len())?;
    let mut rows = Vec::with_capacity(t.rows.len());
    for r in &t.rows {
        let v = f(r)?;
        let mut row = r.clone();
        row.push(v);
        rows.push(row);
    }
    Ok(MValue::table(MTable { names: names.into(), types, rows }))
}

/// A row count, or the number of rows (from the start or the end) meeting a condition.
fn count_or_condition(ev: &mut Ev<'_, '_>, t: &MTable, v: &MValue, from_end: bool) -> MResult<usize> {
    match v {
        MValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Ok(if *n > usize::MAX as f64 { usize::MAX } else { *n as usize }),
        MValue::Function(_) => {
            let mut n = 0;
            let rows: Box<dyn Iterator<Item = &Vec<MValue>>> = if from_end { Box::new(t.rows.iter().rev()) } else { Box::new(t.rows.iter()) };
            for r in rows {
                match ev.call1(v, MValue::record(t.row_record(r)))? {
                    MValue::Logical(true) => n += 1,
                    _ => break,
                }
            }
            Ok(n)
        }
        other => Err(cant(other, "Number")),
    }
}

/// The keys of `cols` in a row (values worked out; text lower-cased when `fold`).
fn row_key(ev: &mut Ev<'_, '_>, row: &[MValue], cols: &[usize], fold: &[bool]) -> MResult<Vec<Key>> {
    let mut out = Vec::with_capacity(cols.len());
    for (j, c) in cols.iter().enumerate() {
        let v = ev.force(row.get(*c).cloned().unwrap_or(MValue::Null))?;
        let v = match v {
            MValue::Text(t) if fold.get(j).copied().unwrap_or(false) => MValue::text(t.to_lowercase()),
            v => v,
        };
        out.push(key(&v));
    }
    Ok(out)
}

fn is_ignore_case(v: &MValue) -> bool {
    matches!(v, MValue::Function(f) if matches!(&**f, MFunc::Builtin { name, .. } if &**name == "Comparer.OrdinalIgnoreCase"))
}

/// Column criteria: a name, a list of names, `{name, comparer}` or a list of those; with
/// whether each compares text ignoring case.
fn criteria(t: &MTable, v: &MValue) -> MResult<(Vec<usize>, Vec<bool>)> {
    let mut cols = Vec::new();
    let mut fold = Vec::new();
    let mut add = |name: &Rc<str>, f: bool| -> MResult<()> {
        cols.push(t.col(name)?);
        fold.push(f);
        Ok(())
    };
    match v {
        MValue::Null => return Ok(((0..t.width()).collect(), vec![false; t.width()])),
        MValue::Text(n) => add(n, false)?,
        MValue::List(l) => {
            if let (Some(MValue::Text(n)), Some(c @ MValue::Function(_)), 2) = (l.first(), l.get(1), l.len()) {
                add(n, is_ignore_case(c))?;
            } else {
                for item in l.iter() {
                    match item {
                        MValue::Text(n) => add(n, false)?,
                        MValue::List(p) => match p.first() {
                            Some(MValue::Text(n)) => add(n, p.get(1).is_some_and(is_ignore_case))?,
                            _ => return Err(cant(item, "Text")),
                        },
                        other => return Err(cant(other, "Text")),
                    }
                }
            }
        }
        other => return Err(cant(other, "List")),
    }
    Ok((cols, fold))
}

/// `{name, x}` or a list of those, as pairs.
fn pairs(v: &MValue) -> MResult<Vec<Vec<MValue>>> {
    let MValue::List(l) = v else { return Err(cant(v, "List")) };
    if matches!(l.first(), Some(MValue::Text(_))) {
        return Ok(vec![l.to_vec()]);
    }
    l.iter()
        .map(|x| match x {
            MValue::List(p) => Ok(p.to_vec()),
            other => Err(cant(other, "List")),
        })
        .collect()
}

fn pair_name(p: &[MValue]) -> MResult<Rc<str>> {
    match p.first() {
        Some(MValue::Text(t)) => Ok(t.clone()),
        Some(other) => Err(cant(other, "Text")),
        None => Err(MError::expr("An empty list was given where a column name was expected.")),
    }
}

// ---------------------------------------------------------------- building tables

enum Columns {
    Default,
    Names(Vec<Rc<str>>),
    Count(usize),
    Typed(Vec<(Rc<str>, MType)>),
}

fn columns_spec(v: &MValue) -> MResult<Columns> {
    Ok(match v {
        MValue::Null => Columns::Default,
        MValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 => {
            if *n > MAX_COLUMNS as f64 {
                return Err(MError::expr(format!("A table can have at most {MAX_COLUMNS} columns.")));
            }
            Columns::Count(*n as usize)
        }
        MValue::Type(t) if t.kind == "table" || t.kind == "record" => Columns::Typed(t.fields.clone()),
        MValue::List(_) => Columns::Names(names_of(v)?),
        other => return Err(cant(other, "List")),
    })
}

fn table_of(spec: Columns, width: usize) -> MResult<MTable> {
    let t = match spec {
        Columns::Default => MTable::new(column_names(width.min(MAX_COLUMNS))),
        Columns::Count(n) => MTable::new(column_names(n)),
        Columns::Names(names) => {
            check_unique(&names)?;
            MTable::new(names)
        }
        Columns::Typed(fields) => {
            let names: Vec<Rc<str>> = fields.iter().map(|(n, _)| n.clone()).collect();
            check_unique(&names)?;
            let mut t = MTable::new(names);
            t.types = fields.iter().map(|(_, ty)| ty.col_type()).collect();
            t
        }
    };
    Ok(t)
}

fn from_rows(ev: &mut Ev<'_, '_>, rows: &MValue, columns: &MValue) -> MResult<MValue> {
    let MValue::List(rows) = rows else { return Err(cant(rows, "List")) };
    let mut lists = Vec::with_capacity(rows.len());
    for r in rows.iter() {
        match ev.force(r.clone())? {
            MValue::List(l) => lists.push(l),
            other => return Err(cant(&other, "List")),
        }
    }
    let width = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut t = table_of(columns_spec(columns)?, width)?;
    for l in lists {
        if l.len() > t.width() {
            return Err(MError::expr(format!("{} keys were specified, but {} values were provided.", t.width(), l.len())));
        }
        t.push(l.to_vec())?;
    }
    Ok(MValue::table(t))
}

fn from_records(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let l = a.list(0)?;
    let mut recs = Vec::with_capacity(l.len());
    for r in l.iter() {
        match ev.force(r.clone())? {
            MValue::Record(r) => recs.push(r),
            other => return Err(cant(&other, "Record")),
        }
    }
    let mut t = match columns_spec(a.at(1))? {
        Columns::Default | Columns::Count(_) => {
            let mut names: Vec<Rc<str>> = Vec::new();
            for r in &recs {
                for n in r.names.iter() {
                    if !names.contains(n) {
                        if names.len() >= MAX_COLUMNS {
                            return Err(MError::expr(format!("A table can have at most {MAX_COLUMNS} columns.")));
                        }
                        names.push(n.clone());
                    }
                }
            }
            MTable::new(names)
        }
        spec => table_of(spec, 0)?,
    };
    let names = t.names.clone();
    for r in recs {
        t.push(names.iter().map(|n| r.get(n).cloned().unwrap_or(MValue::Null)).collect())?;
    }
    Ok(MValue::table(t))
}

fn from_list(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let l = a.list(0)?;
    let splitter = a.at(1).clone();
    let mut rows = Vec::with_capacity(l.len());
    for item in l.iter() {
        let item = ev.force(item.clone())?;
        let parts = match &splitter {
            MValue::Null => match &item {
                MValue::Text(t) => t.split(',').map(MValue::text).collect(),
                other => vec![other.clone()],
            },
            f => match ev.call1(f, item)? {
                MValue::List(p) => p.to_vec(),
                other => vec![other],
            },
        };
        rows.push(parts);
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut t = table_of(columns_spec(a.at(2))?, width)?;
    let default = a.at(3).clone();
    let extra_error = matches!(a.at(4), MValue::Number(n) if *n == 1.0);
    for mut r in rows {
        if r.len() > t.width() && extra_error {
            return Err(MError::expr(format!("There were more values than the {} columns.", t.width())));
        }
        while r.len() < t.width() {
            r.push(default.clone());
        }
        t.push(r)?;
    }
    Ok(MValue::table(t))
}

fn from_columns(a: &Args<'_>) -> MResult<MValue> {
    let l = a.list(0)?;
    let mut cols = Vec::with_capacity(l.len());
    for c in l.iter() {
        match c {
            MValue::List(x) => cols.push(x.clone()),
            other => return Err(cant(other, "List")),
        }
    }
    let mut t = table_of(columns_spec(a.at(1))?, cols.len())?;
    let height = cols.iter().map(|c| c.len()).max().unwrap_or(0);
    for i in 0..height {
        t.push(cols.iter().map(|c| c.get(i).cloned().unwrap_or(MValue::Null)).collect())?;
    }
    Ok(MValue::table(t))
}

// ---------------------------------------------------------------- columns

fn promote_headers(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let all = a.opt_record(1)?.is_some_and(|r| matches!(r.get("PromoteAllScalars"), Some(MValue::Logical(true))));
    let Some(first) = t.rows.first() else { return Ok(MValue::Table(t)) };
    let mut names: Vec<String> = Vec::with_capacity(t.width());
    for (i, v) in first.iter().enumerate() {
        let v = ev.force(v.clone()).unwrap_or(MValue::Null);
        let name = match &v {
            MValue::Text(s) if !s.is_empty() => Some(s.to_string()),
            MValue::Number(n) => Some(convert::number_text(*n)),
            MValue::Null | MValue::Text(_) => None,
            other if all => convert::text_of(other, None),
            _ => None,
        };
        names.push(name.unwrap_or_else(|| t.names.get(i).map(|n| n.to_string()).unwrap_or_default()));
    }
    header_names(&mut names);
    let names: Vec<Rc<str>> = names.into_iter().map(Rc::from).collect();
    let n = names.len();
    Ok(MValue::table(MTable { names: names.into(), types: vec![ColType::Any; n], rows: t.rows.iter().skip(1).cloned().collect() }))
}

/// Makes promoted header names unique as Power Query does: one counter over all repeats, left to
/// right (`Total`, `Total_1`, `Name`, `Name_2`), skipping names already taken.
fn header_names(names: &mut [String]) {
    let taken: std::collections::HashSet<String> = names.iter().cloned().collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut k = 0usize;
    for n in names.iter_mut() {
        if seen.contains(n.as_str()) {
            loop {
                k += 1;
                let candidate = format!("{n}_{k}");
                if !taken.contains(&candidate) && !seen.contains(&candidate) {
                    *n = candidate;
                    break;
                }
            }
        }
        seen.insert(n.clone());
    }
}

fn demote_headers(a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let mut out = MTable::new(column_names(t.width()));
    out.push(t.names.iter().map(|n| MValue::Text(n.clone())).collect())?;
    for r in &t.rows {
        out.push(r.clone())?;
    }
    Ok(MValue::table(out))
}

/// `Using Locale`'s culture: a text, or a record with `Culture`.
fn culture_arg(v: &MValue) -> Option<String> {
    match v {
        MValue::Text(t) => Some(t.to_string()),
        MValue::Record(r) => match r.get("Culture") {
            Some(MValue::Text(t)) => Some(t.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn transform_column_types(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let culture = culture_arg(a.at(2));
    let mut out = (*t).clone();
    for p in pairs(a.at(1))? {
        let name = pair_name(&p)?;
        let ty = match p.get(1) {
            Some(MValue::Type(ty)) => ty.clone(),
            Some(other) => return Err(cant(other, "Type")),
            None => return Err(MError::expr(format!("No type was given for the column '{name}'."))),
        };
        let c = out.col(&name)?;
        ev.tick(out.rows.len() as u64)?;
        for row in out.rows.iter_mut() {
            if let Some(cell) = row.get_mut(c) {
                let v = std::mem::replace(cell, MValue::Null);
                *cell = match ev.force(v) {
                    Ok(v) => convert::convert(&v, &ty, culture.as_deref(), ev.sys),
                    Err(e) if is_limit(&e) => return Err(e),
                    Err(e) => MValue::error(e),
                };
            }
        }
        if let Some(slot) = out.types.get_mut(c) {
            *slot = ty.col_type();
        }
    }
    Ok(MValue::table(out))
}

fn select_columns(a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    Ok(MValue::table(project(&t, &a.names(1)?, missing(a.at(2))?)?))
}

fn remove_columns(a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let drop = a.names(1)?;
    if missing(a.at(2))? == MissingField::Error
        && let Some(n) = drop.iter().find(|n| !t.names.contains(n))
    {
        return Err(MError::expr(format!("The column '{n}' of the table wasn't found.")));
    }
    let keep: Vec<Rc<str>> = t.names.iter().filter(|n| !drop.contains(n)).cloned().collect();
    Ok(MValue::table(project(&t, &keep, MissingField::Error)?))
}

fn rename_columns(a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let mode = missing(a.at(2))?;
    let mut names: Vec<Rc<str>> = t.names.to_vec();
    for p in pairs(a.at(1))? {
        let from = pair_name(&p)?;
        let to = match p.get(1) {
            Some(MValue::Text(s)) => s.clone(),
            Some(other) => return Err(cant(other, "Text")),
            None => return Err(MError::expr(format!("No new name was given for the column '{from}'."))),
        };
        match t.names.iter().position(|n| *n == from) {
            Some(i) => {
                if let Some(slot) = names.get_mut(i) {
                    *slot = to;
                }
            }
            None if mode == MissingField::Error => return Err(MError::expr(format!("The column '{from}' of the table wasn't found."))),
            None => {}
        }
    }
    check_unique(&names)?;
    Ok(MValue::table(MTable { names: names.into(), types: t.types.clone(), rows: t.rows.clone() }))
}

fn reorder_columns(a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let mode = missing(a.at(2))?;
    let mut order = Vec::new();
    for n in a.names(1)? {
        match t.names.iter().position(|c| *c == n) {
            Some(i) => order.push(i),
            None if mode == MissingField::Error => return Err(MError::expr(format!("The column '{n}' of the table wasn't found."))),
            None => {}
        }
    }
    // The listed columns take the places the listed columns had, in the listed order.
    let mut slots: Vec<usize> = order.clone();
    slots.sort_unstable();
    let mut idx: Vec<usize> = (0..t.width()).collect();
    for (slot, col) in slots.iter().zip(order.iter()) {
        if let Some(x) = idx.get_mut(*slot) {
            *x = *col;
        }
    }
    let names: Vec<Rc<str>> = idx.iter().filter_map(|i| t.names.get(*i).cloned()).collect();
    check_unique(&names)?;
    let types = idx.iter().map(|i| t.types.get(*i).copied().unwrap_or_default()).collect();
    let rows = t.rows.iter().map(|r| idx.iter().map(|i| r.get(*i).cloned().unwrap_or(MValue::Null)).collect()).collect();
    Ok(MValue::table(MTable { names: names.into(), types, rows }))
}

fn duplicate_column(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let c = t.col(&a.text(1)?)?;
    let name = a.text(2)?;
    let ty = col_type_arg(a.at(3)).unwrap_or_else(|| t.types.get(c).copied().unwrap_or_default());
    ev.tick(t.rows.len() as u64)?;
    add_column(&t, &name, ty, |r| Ok(r.get(c).cloned().unwrap_or(MValue::Null)))
}

// ---------------------------------------------------------------- rows

fn distinct(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let (cols, fold) = criteria(&t, a.at(1))?;
    let mut seen = std::collections::HashSet::new();
    let mut rows = Vec::new();
    for r in &t.rows {
        if seen.insert(row_key(ev, r, &cols, &fold)?) {
            rows.push(r.clone());
        }
    }
    Ok(with_rows(&t, rows))
}

fn rows_with_errors(ev: &mut Ev<'_, '_>, a: &Args<'_>, keep_errors: bool) -> MResult<MValue> {
    let t = a.table(0)?;
    let cols: Vec<usize> = if a.has(1) { a.names(1)?.iter().map(|n| t.col(n)).collect::<MResult<_>>()? } else { (0..t.width()).collect() };
    let mut rows = Vec::new();
    for r in &t.rows {
        let mut bad = false;
        for c in &cols {
            let v = r.get(*c).cloned().unwrap_or(MValue::Null);
            if matches!(v, MValue::Error(_) | MValue::Lazy(_)) {
                match ev.force(v) {
                    Err(e) if is_limit(&e) => return Err(e),
                    Err(_) => bad = true,
                    Ok(_) => {}
                }
            }
        }
        if bad == keep_errors {
            rows.push(r.clone());
        }
    }
    Ok(with_rows(&t, rows))
}

fn select_rows(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let f = a.func(1)?;
    let mut rows = Vec::new();
    for r in &t.rows {
        match ev.call1(&f, MValue::record(t.row_record(r)))? {
            MValue::Logical(true) => rows.push(r.clone()),
            MValue::Logical(false) | MValue::Null => {}
            other => return Err(MError::expr(format!("We cannot convert the value {} to type Logical.", show(&other)))),
        }
    }
    Ok(with_rows(&t, rows))
}

enum SortBy {
    Col(usize),
    Key(MValue),
}

fn sort(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let crit = a.at(1);
    let mut keys: Vec<(SortBy, bool)> = Vec::new();
    let one = |v: &MValue, desc: bool, keys: &mut Vec<(SortBy, bool)>| -> MResult<()> {
        match v {
            MValue::Text(n) => keys.push((SortBy::Col(t.col(n)?), desc)),
            MValue::Function(f) => {
                if let MFunc::Closure { def, .. } = &**f
                    && def.params.len() == 2
                {
                    return Err(MError::expr("Sorting with a comparer function isn't supported by GridCraft yet."));
                }
                keys.push((SortBy::Key(v.clone()), desc));
            }
            other => return Err(cant(other, "Text")),
        }
        Ok(())
    };
    let order_of = |v: Option<&MValue>| matches!(v, Some(MValue::Number(n)) if *n == 1.0);
    match crit {
        MValue::List(l) if matches!((l.first(), l.get(1), l.len()), (Some(MValue::Text(_) | MValue::Function(_)), Some(MValue::Number(_)), 2)) => {
            if let Some(first) = l.first() {
                one(first, order_of(l.get(1)), &mut keys)?;
            }
        }
        MValue::List(l) => {
            for item in l.iter() {
                match item {
                    MValue::List(p) => match p.first() {
                        Some(first) => one(first, order_of(p.get(1)), &mut keys)?,
                        None => return Err(cant(item, "Text")),
                    },
                    other => one(other, false, &mut keys)?,
                }
            }
        }
        other => one(other, false, &mut keys)?,
    }
    // Sort keys worked out once per row.
    let mut decorated: Vec<(Vec<MValue>, usize)> = Vec::with_capacity(t.rows.len());
    for (i, r) in t.rows.iter().enumerate() {
        let mut ks = Vec::with_capacity(keys.len());
        for (k, _) in &keys {
            ks.push(match k {
                SortBy::Col(c) => ev.force(r.get(*c).cloned().unwrap_or(MValue::Null))?,
                SortBy::Key(f) => ev.call1(f, MValue::record(t.row_record(r)))?,
            });
        }
        decorated.push((ks, i));
    }
    ev.tick(decorated.len() as u64 * 20)?;
    decorated.sort_by(|(x, _), (y, _)| {
        for ((a, b), (_, desc)) in x.iter().zip(y.iter()).zip(keys.iter()) {
            let o = sort_order(a, b);
            let o = if *desc { o.reverse() } else { o };
            if o.is_ne() {
                return o;
            }
        }
        std::cmp::Ordering::Equal
    });
    Ok(with_rows(&t, decorated.into_iter().filter_map(|(_, i)| t.rows.get(i).cloned()).collect()))
}

fn builtin_name(f: &MValue) -> Option<&str> {
    match f {
        MValue::Function(f) => match &**f {
            MFunc::Builtin { name, bound } if bound.is_empty() => Some(name),
            _ => None,
        },
        _ => None,
    }
}

fn replace_value(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let (old, new, replacer) = (a.at(1).clone(), a.at(2).clone(), a.func(3)?);
    let cols: Vec<usize> = a.names(4)?.iter().map(|n| t.col(n)).collect::<MResult<_>>()?;
    let by_row = matches!(old, MValue::Function(_)) || matches!(new, MValue::Function(_));
    let mut out = (*t).clone();
    for r in out.rows.iter_mut() {
        let (o, n) = if by_row {
            let rec = MValue::record(MRecord { names: t.names.clone(), values: r.clone() });
            let o = if matches!(old, MValue::Function(_)) { ev.call1(&old, rec.clone())? } else { old.clone() };
            let n = if matches!(new, MValue::Function(_)) { ev.call1(&new, rec)? } else { new.clone() };
            (o, n)
        } else {
            (old.clone(), new.clone())
        };
        for c in &cols {
            let Some(cell) = r.get_mut(*c) else { continue };
            if matches!(cell, MValue::Error(_)) {
                continue;
            }
            let v = std::mem::replace(cell, MValue::Null);
            *cell = match builtin_name(&replacer) {
                Some("Replacer.ReplaceValue") => held(ev.force(v).map(|v| if super::super::value::equal(&v, &o) { n.clone() } else { v }))?,
                Some("Replacer.ReplaceText") => held(ev.force(v).and_then(|v| text::replace_text(&v, &o, &n)))?,
                _ => {
                    let v = ev.force(v)?;
                    held(ev.call(&replacer, vec![v, o.clone(), n.clone()]))?
                }
            };
        }
    }
    Ok(MValue::table(out))
}

fn replace_error_values(a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let mut out = (*t).clone();
    for p in pairs(a.at(1))? {
        let c = out.col(&pair_name(&p)?)?;
        let v = p.get(1).cloned().unwrap_or(MValue::Null);
        for r in out.rows.iter_mut() {
            if let Some(cell) = r.get_mut(c)
                && matches!(cell, MValue::Error(_))
            {
                *cell = v.clone();
            }
        }
    }
    Ok(MValue::table(out))
}

fn split_column(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let source = a.text(1)?;
    let c = t.col(&source)?;
    let splitter = a.func(2)?;
    let default = a.at(4).clone();
    let mut parts: Vec<Vec<MValue>> = Vec::with_capacity(t.rows.len());
    for r in &t.rows {
        let v = r.get(c).cloned().unwrap_or(MValue::Null);
        let p = match ev.force(v) {
            Ok(v) => match ev.call1(&splitter, v)? {
                MValue::List(l) => l.to_vec(),
                other => vec![other],
            },
            Err(e) if is_limit(&e) => return Err(e),
            Err(e) => vec![MValue::error(e)],
        };
        parts.push(p);
    }
    let new_names: Vec<Rc<str>> = match a.at(3) {
        MValue::Null => {
            let n = parts.iter().map(Vec::len).max().unwrap_or(1).clamp(1, MAX_COLUMNS);
            (1..=n).map(|i| Rc::from(format!("{source}.{i}"))).collect()
        }
        MValue::Number(n) if *n >= 1.0 && n.fract() == 0.0 && *n <= MAX_COLUMNS as f64 => {
            (1..=*n as usize).map(|i| Rc::from(format!("{source}.{i}"))).collect()
        }
        v @ MValue::List(_) => names_of(v)?,
        other => return Err(cant(other, "List")),
    };
    let mut names: Vec<Rc<str>> = Vec::with_capacity(t.width() + new_names.len());
    let mut types = Vec::with_capacity(names.capacity());
    for (i, n) in t.names.iter().enumerate() {
        if i == c {
            names.extend(new_names.iter().cloned());
            types.extend(std::iter::repeat_n(ColType::Any, new_names.len()));
        } else {
            names.push(n.clone());
            types.push(t.types.get(i).copied().unwrap_or_default());
        }
    }
    check_unique(&names)?;
    check_cells(t.rows.len(), names.len())?;
    let mut rows = Vec::with_capacity(t.rows.len());
    for (r, p) in t.rows.iter().zip(parts) {
        let mut row = Vec::with_capacity(names.len());
        for (i, v) in r.iter().enumerate() {
            if i == c {
                for k in 0..new_names.len() {
                    row.push(p.get(k).cloned().unwrap_or_else(|| default.clone()));
                }
            } else {
                row.push(v.clone());
            }
        }
        rows.push(row);
    }
    Ok(MValue::table(MTable { names: names.into(), types, rows }))
}

fn transform_columns(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let mode = missing(a.at(3))?;
    let mut ops: Vec<(usize, MValue, Option<ColType>)> = Vec::new();
    for p in pairs(a.at(1))? {
        let name = pair_name(&p)?;
        let f = match p.get(1) {
            Some(f @ MValue::Function(_)) => f.clone(),
            Some(other) => return Err(cant(other, "Function")),
            None => return Err(MError::expr(format!("No function was given for the column '{name}'."))),
        };
        match t.names.iter().position(|n| *n == name) {
            Some(c) => ops.push((c, f, p.get(2).and_then(col_type_arg))),
            None if mode == MissingField::Error => return Err(MError::expr(format!("The column '{name}' of the table wasn't found."))),
            None => {}
        }
    }
    if let MValue::Function(_) = a.at(2) {
        for c in 0..t.width() {
            if !ops.iter().any(|(x, _, _)| *x == c) {
                ops.push((c, a.at(2).clone(), None));
            }
        }
    }
    let mut out = (*t).clone();
    for (c, f, ty) in ops {
        for r in out.rows.iter_mut() {
            if let Some(cell) = r.get_mut(c) {
                if matches!(cell, MValue::Error(_)) {
                    continue;
                }
                let v = std::mem::replace(cell, MValue::Null);
                *cell = match ev.force(v) {
                    Ok(v) => held(ev.call1(&f, v))?,
                    Err(e) if is_limit(&e) => return Err(e),
                    Err(e) => MValue::error(e),
                };
            }
        }
        if let Some(slot) = out.types.get_mut(c) {
            *slot = ty.unwrap_or(ColType::Any);
        }
    }
    Ok(MValue::table(out))
}

fn fill(a: &Args<'_>, up: bool) -> MResult<MValue> {
    let t = a.table(0)?;
    let cols: Vec<usize> = a.names(1)?.iter().map(|n| t.col(n)).collect::<MResult<_>>()?;
    let mut out = (*t).clone();
    for c in cols {
        let mut last = MValue::Null;
        let mut fill_one = |r: &mut Vec<MValue>| {
            if let Some(cell) = r.get_mut(c) {
                if cell.is_null() {
                    *cell = last.clone();
                } else {
                    last = cell.clone();
                }
            }
        };
        if up {
            out.rows.iter_mut().rev().for_each(&mut fill_one);
        } else {
            out.rows.iter_mut().for_each(&mut fill_one);
        }
    }
    Ok(MValue::table(out))
}

// ---------------------------------------------------------------- grouping and reshaping

fn group(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let key_names = a.names(1)?;
    let key_cols: Vec<usize> = key_names.iter().map(|n| t.col(n)).collect::<MResult<_>>()?;
    let fold = vec![is_ignore_case(a.at(4)); key_cols.len()];
    let local = matches!(a.at(3), MValue::Number(n) if *n == 0.0);
    let mut aggs: Vec<(Rc<str>, MValue, ColType)> = Vec::new();
    let agg_list = match a.at(2) {
        MValue::List(l) if l.is_empty() => vec![],
        v => pairs(v)?,
    };
    for p in agg_list {
        let name = pair_name(&p)?;
        let f = match p.get(1) {
            Some(f @ MValue::Function(_)) => f.clone(),
            Some(other) => return Err(cant(other, "Function")),
            None => return Err(MError::expr(format!("No aggregation was given for the column '{name}'."))),
        };
        aggs.push((name, f, p.get(2).and_then(col_type_arg).unwrap_or(ColType::Any)));
    }
    // Groups in order of first appearance (or runs of equal keys for GroupKind.Local).
    let mut groups: Vec<(Vec<Key>, Vec<usize>)> = Vec::new();
    let mut index: HashMap<Vec<Key>, usize> = HashMap::new();
    for (i, r) in t.rows.iter().enumerate() {
        let k = row_key(ev, r, &key_cols, &fold)?;
        if local {
            match groups.last_mut() {
                Some((lk, rows)) if *lk == k => rows.push(i),
                _ => groups.push((k, vec![i])),
            }
        } else {
            match index.get(&k) {
                Some(g) => {
                    if let Some((_, rows)) = groups.get_mut(*g) {
                        rows.push(i);
                    }
                }
                None => {
                    index.insert(k.clone(), groups.len());
                    groups.push((k, vec![i]));
                }
            }
        }
    }
    let mut names: Vec<Rc<str>> = key_names.clone();
    names.extend(aggs.iter().map(|(n, _, _)| n.clone()));
    check_unique(&names)?;
    let mut types: Vec<ColType> = key_cols.iter().map(|c| t.types.get(*c).copied().unwrap_or_default()).collect();
    types.extend(aggs.iter().map(|(_, _, ty)| *ty));
    check_cells(groups.len(), names.len())?;
    let mut out = MTable { names: names.into(), types, rows: Vec::with_capacity(groups.len()) };
    for (_, rows) in groups {
        let Some(first) = rows.first().and_then(|i| t.rows.get(*i)) else { continue };
        let mut row: Vec<MValue> = key_cols.iter().map(|c| first.get(*c).cloned().unwrap_or(MValue::Null)).collect();
        let sub =
            Rc::new(MTable { names: t.names.clone(), types: t.types.clone(), rows: rows.iter().filter_map(|i| t.rows.get(*i).cloned()).collect() });
        for (_, f, _) in &aggs {
            row.push(held(ev.call1(f, MValue::Table(sub.clone())))?);
        }
        out.push(row)?;
    }
    Ok(MValue::table(out))
}

fn unpivot(ev: &mut Ev<'_, '_>, a: &Args<'_>, other_columns: bool) -> MResult<MValue> {
    let t = a.table(0)?;
    let listed = a.names(1)?;
    for n in &listed {
        t.col(n)?;
    }
    let (attr, value) = (a.text(2)?, a.text(3)?);
    let unpivoted: Vec<usize> = (0..t.width()).filter(|i| t.names.get(*i).is_some_and(|n| listed.contains(n) != other_columns)).collect();
    let kept: Vec<usize> = (0..t.width()).filter(|i| !unpivoted.contains(i)).collect();
    let mut names: Vec<Rc<str>> = kept.iter().filter_map(|i| t.names.get(*i).cloned()).collect();
    names.push(attr);
    names.push(value);
    check_unique(&names)?;
    let mut types: Vec<ColType> = kept.iter().map(|i| t.types.get(*i).copied().unwrap_or_default()).collect();
    types.push(ColType::Text);
    let first_type = unpivoted.first().and_then(|i| t.types.get(*i).copied()).unwrap_or_default();
    types.push(if unpivoted.iter().all(|i| t.types.get(*i).copied().unwrap_or_default() == first_type) { first_type } else { ColType::Any });
    let mut out = MTable { names: names.into(), types, rows: vec![] };
    ev.tick((t.rows.len() * unpivoted.len().max(1)) as u64)?;
    for r in &t.rows {
        for u in &unpivoted {
            let v = r.get(*u).cloned().unwrap_or(MValue::Null);
            if v.is_null() {
                continue;
            }
            let mut row: Vec<MValue> = kept.iter().map(|k| r.get(*k).cloned().unwrap_or(MValue::Null)).collect();
            row.push(t.names.get(*u).map(|n| MValue::Text(n.clone())).unwrap_or(MValue::Null));
            row.push(v);
            out.push(row)?;
        }
    }
    Ok(MValue::table(out))
}

fn pivot(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let values = a.list(1)?;
    let (ac, vc) = (t.col(&a.text(2)?)?, t.col(&a.text(3)?)?);
    let agg = if a.has(4) { Some(a.func(4)?) } else { None };
    let others: Vec<usize> = (0..t.width()).filter(|i| *i != ac && *i != vc).collect();
    let mut pv_keys = Vec::with_capacity(values.len());
    let mut names: Vec<Rc<str>> = others.iter().filter_map(|i| t.names.get(*i).cloned()).collect();
    for v in values.iter() {
        let v = ev.force(v.clone())?;
        pv_keys.push(key(&v));
        names.push(Rc::from(convert::text_of(&v, None).unwrap_or_default()));
    }
    check_unique(&names)?;
    let mut types: Vec<ColType> = others.iter().map(|i| t.types.get(*i).copied().unwrap_or_default()).collect();
    types.extend(std::iter::repeat_n(if agg.is_some() { ColType::Any } else { t.types.get(vc).copied().unwrap_or_default() }, values.len()));
    let no_fold = vec![false; others.len()];
    let mut groups: Vec<(Vec<MValue>, Vec<Vec<MValue>>)> = Vec::new();
    let mut index: HashMap<Vec<Key>, usize> = HashMap::new();
    for r in &t.rows {
        let k = row_key(ev, r, &others, &no_fold)?;
        let g = match index.get(&k) {
            Some(g) => *g,
            None => {
                // Each new group reserves a cell per pivot value: refuse before allocating it.
                check_cells(groups.len() + 1, names.len())?;
                index.insert(k, groups.len());
                groups.push((others.iter().map(|c| r.get(*c).cloned().unwrap_or(MValue::Null)).collect(), vec![vec![]; values.len()]));
                groups.len() - 1
            }
        };
        let attr = key(&ev.force(r.get(ac).cloned().unwrap_or(MValue::Null))?);
        if let Some(p) = pv_keys.iter().position(|k| *k == attr)
            && let Some(cell) = groups.get_mut(g).and_then(|(_, cells)| cells.get_mut(p))
        {
            cell.push(r.get(vc).cloned().unwrap_or(MValue::Null));
        }
    }
    // Rows come out in the order of the other columns' values, as Power Query's pivot gives them.
    groups.sort_by(|(a, _), (b, _)| a.iter().zip(b.iter()).map(|(x, y)| sort_order(x, y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal));
    check_cells(groups.len(), names.len())?;
    let mut out = MTable { names: names.into(), types, rows: Vec::with_capacity(groups.len()) };
    for (mut row, cells) in groups {
        for c in cells {
            row.push(match &agg {
                Some(f) => held(ev.call1(f, MValue::list(c)))?,
                None => match c.len() {
                    0 => MValue::Null,
                    1 => c.into_iter().next().unwrap_or(MValue::Null),
                    _ => MValue::error(MError::expr("There were too many elements in the enumeration to complete the operation.")),
                },
            });
        }
        out.push(row)?;
    }
    Ok(MValue::table(out))
}

fn transpose(a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    if t.rows.len() > MAX_COLUMNS {
        return Err(MError::expr(format!(
            "A table can have at most {MAX_COLUMNS} columns, so a table with {} rows can't be transposed.",
            t.rows.len()
        )));
    }
    let mut out = table_of(columns_spec(a.at(1))?, t.rows.len())?;
    for c in 0..t.width() {
        out.push(t.rows.iter().map(|r| r.get(c).cloned().unwrap_or(MValue::Null)).collect())?;
    }
    Ok(MValue::table(out))
}

fn nested_join(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let (t1, t2) = (a.table(0)?, a.table(2)?);
    let k1: Vec<usize> = a.names(1)?.iter().map(|n| t1.col(n)).collect::<MResult<_>>()?;
    let k2: Vec<usize> = a.names(3)?.iter().map(|n| t2.col(n)).collect::<MResult<_>>()?;
    if k1.len() != k2.len() {
        return Err(MError::expr("The tables are joined on different numbers of key columns."));
    }
    let new = a.text(4)?;
    let kind = a.int(5, 1)?;
    if !(0..=5).contains(&kind) {
        return Err(cant(a.at(5), "JoinKind"));
    }
    let fold = vec![false; k1.len()];
    let mut right: HashMap<Vec<Key>, Vec<usize>> = HashMap::new();
    let mut right_keys = Vec::with_capacity(t2.rows.len());
    for (i, r) in t2.rows.iter().enumerate() {
        let k = row_key(ev, r, &k2, &fold)?;
        right.entry(k.clone()).or_default().push(i);
        right_keys.push(k);
    }
    let nested = |rows: &[usize]| {
        MValue::table(MTable {
            names: t2.names.clone(),
            types: t2.types.clone(),
            rows: rows.iter().filter_map(|i| t2.rows.get(*i).cloned()).collect(),
        })
    };
    let mut names: Vec<Rc<str>> = t1.names.to_vec();
    names.push(new);
    check_unique(&names)?;
    let mut types = t1.types.clone();
    types.push(ColType::Any);
    let mut out = MTable { names: names.into(), types, rows: vec![] };
    let mut matched = vec![false; t2.rows.len()];
    for r in &t1.rows {
        let k = row_key(ev, r, &k1, &fold)?;
        let hits: &[usize] = right.get(&k).map(Vec::as_slice).unwrap_or(&[]);
        for h in hits {
            if let Some(m) = matched.get_mut(*h) {
                *m = true;
            }
        }
        let keep = match kind {
            0 | 2 => !hits.is_empty(),
            1 | 3 => true,
            4 => hits.is_empty(),
            _ => false,
        };
        if keep {
            let mut row = r.clone();
            row.push(nested(hits));
            out.push(row)?;
        }
    }
    if matches!(kind, 2 | 3 | 5) {
        // Right rows without a match: one row per key, left columns null.
        let mut order: Vec<Vec<Key>> = Vec::new();
        let mut by_key: HashMap<Vec<Key>, Vec<usize>> = HashMap::new();
        for (i, k) in right_keys.into_iter().enumerate() {
            if matched.get(i).copied().unwrap_or(true) {
                continue;
            }
            if !by_key.contains_key(&k) {
                order.push(k.clone());
            }
            by_key.entry(k).or_default().push(i);
        }
        for k in order {
            let rows = by_key.get(&k).map(Vec::as_slice).unwrap_or(&[]);
            let mut row = vec![MValue::Null; t1.width()];
            row.push(nested(rows));
            out.push(row)?;
        }
    }
    Ok(MValue::table(out))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expand {
    Table,
    Record,
    List,
}

fn expand(ev: &mut Ev<'_, '_>, a: &Args<'_>, kind: Expand) -> MResult<MValue> {
    let t = a.table(0)?;
    let c = t.col(&a.text(1)?)?;
    let (fields, new_names): (Vec<Rc<str>>, Vec<Rc<str>>) = if kind == Expand::List {
        let n = t.names.get(c).cloned().unwrap_or_else(|| "Column".into());
        (vec![n.clone()], vec![n])
    } else {
        let f = a.names(2)?;
        let n = if a.has(3) { a.names(3)? } else { f.clone() };
        if n.len() != f.len() {
            return Err(MError::expr("The number of new column names doesn't match the number of columns to expand."));
        }
        (f, n)
    };
    let mut names: Vec<Rc<str>> = Vec::with_capacity(t.width() + new_names.len());
    for (i, n) in t.names.iter().enumerate() {
        if i == c {
            names.extend(new_names.iter().cloned());
        } else {
            names.push(n.clone());
        }
    }
    check_unique(&names)?;
    // Column types from the first nested table.
    let mut new_types = vec![ColType::Any; new_names.len()];
    if kind == Expand::Table
        && let Some(MValue::Table(nt)) = t.rows.iter().filter_map(|r| r.get(c)).find(|v| matches!(v, MValue::Table(_)))
    {
        for (slot, f) in new_types.iter_mut().zip(&fields) {
            *slot = nt.names.iter().position(|n| n == f).and_then(|i| nt.types.get(i).copied()).unwrap_or_default();
        }
    }
    let mut types = Vec::with_capacity(names.len());
    for (i, ty) in t.types.iter().enumerate() {
        if i == c {
            types.extend(new_types.iter().copied());
        } else {
            types.push(*ty);
        }
    }
    let mut out = MTable { names: names.into(), types, rows: vec![] };
    let nulls = vec![MValue::Null; fields.len()];
    for r in &t.rows {
        let v = match ev.force(r.get(c).cloned().unwrap_or(MValue::Null)) {
            Ok(v) => v,
            Err(e) if is_limit(&e) => return Err(e),
            Err(e) => MValue::error(e),
        };
        let parts: Vec<Vec<MValue>> = match (&v, kind) {
            (MValue::Null, _) => vec![nulls.clone()],
            (MValue::Error(_), _) => vec![vec![v.clone(); fields.len()]],
            (MValue::Table(nt), Expand::Table) => {
                let idx: Vec<Option<usize>> = fields.iter().map(|f| nt.names.iter().position(|n| n == f)).collect();
                if nt.rows.is_empty() {
                    vec![nulls.clone()]
                } else {
                    nt.rows.iter().map(|nr| idx.iter().map(|i| i.and_then(|i| nr.get(i).cloned()).unwrap_or(MValue::Null)).collect()).collect()
                }
            }
            (MValue::Record(rec), Expand::Record) => vec![fields.iter().map(|f| rec.get(f).cloned().unwrap_or(MValue::Null)).collect()],
            (MValue::List(l), Expand::List) => {
                if l.is_empty() {
                    vec![vec![MValue::Null]]
                } else {
                    l.iter().map(|x| vec![x.clone()]).collect()
                }
            }
            (other, Expand::Table) => return Err(cant(other, "Table")),
            (other, Expand::Record) => return Err(cant(other, "Record")),
            (other, Expand::List) => return Err(cant(other, "List")),
        };
        for p in parts {
            let mut row = Vec::with_capacity(out.width());
            for (i, x) in r.iter().enumerate() {
                if i == c {
                    row.extend(p.iter().cloned());
                } else {
                    row.push(x.clone());
                }
            }
            out.push(row)?;
        }
    }
    Ok(MValue::table(out))
}

/// Appends tables, matching columns by name (a column missing from a table is null there).
pub fn combine(ev: &mut Ev<'_, '_>, tables: &[MValue]) -> MResult<MValue> {
    let mut ts = Vec::with_capacity(tables.len());
    for t in tables {
        match ev.force(t.clone())? {
            MValue::Table(t) => ts.push(t),
            other => return Err(cant(&other, "Table")),
        }
    }
    let mut names: Vec<Rc<str>> = Vec::new();
    let mut types: Vec<ColType> = Vec::new();
    for t in &ts {
        for (n, ty) in t.names.iter().zip(t.types.iter()) {
            match names.iter().position(|m| m == n) {
                Some(i) => {
                    if let Some(slot) = types.get_mut(i)
                        && *slot != *ty
                    {
                        *slot = ColType::Any;
                    }
                }
                None => {
                    names.push(n.clone());
                    types.push(*ty);
                }
            }
        }
    }
    check_unique(&names)?;
    let mut out = MTable { names: names.clone().into(), types, rows: vec![] };
    for t in &ts {
        let idx: Vec<Option<usize>> = names.iter().map(|n| t.names.iter().position(|m| m == n)).collect();
        for r in &t.rows {
            out.push(idx.iter().map(|i| i.and_then(|i| r.get(i).cloned()).unwrap_or(MValue::Null)).collect())?;
        }
    }
    Ok(MValue::table(out))
}

fn combine_columns(ev: &mut Ev<'_, '_>, a: &Args<'_>) -> MResult<MValue> {
    let t = a.table(0)?;
    let cols: Vec<usize> = a.names(1)?.iter().map(|n| t.col(n)).collect::<MResult<_>>()?;
    let combiner = a.func(2)?;
    let new = a.text(3)?;
    let at = cols.iter().min().copied().unwrap_or(0);
    let mut names: Vec<Rc<str>> = Vec::new();
    let mut types = Vec::new();
    for (i, n) in t.names.iter().enumerate() {
        if i == at {
            names.push(new.clone());
            types.push(ColType::Any);
        }
        if !cols.contains(&i) {
            names.push(n.clone());
            types.push(t.types.get(i).copied().unwrap_or_default());
        }
    }
    if cols.is_empty() {
        names.push(new.clone());
        types.push(ColType::Any);
    }
    check_unique(&names)?;
    let mut out = MTable { names: names.into(), types, rows: Vec::with_capacity(t.rows.len()) };
    for r in &t.rows {
        let mut parts = Vec::with_capacity(cols.len());
        let mut err = None;
        for c in &cols {
            match ev.force(r.get(*c).cloned().unwrap_or(MValue::Null)) {
                Ok(v) => parts.push(v),
                Err(e) if is_limit(&e) => return Err(e),
                Err(e) => err = Some(e),
            }
        }
        let combined = match err {
            Some(e) => MValue::error(e),
            None => held(ev.call1(&combiner, MValue::list(parts)))?,
        };
        let mut row = Vec::with_capacity(out.width());
        for (i, v) in r.iter().enumerate() {
            if i == at {
                row.push(combined.clone());
            }
            if !cols.contains(&i) {
                row.push(v.clone());
            }
        }
        if cols.is_empty() {
            row.push(combined);
        }
        out.push(row)?;
    }
    Ok(MValue::table(out))
}
