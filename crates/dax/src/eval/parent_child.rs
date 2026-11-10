//! Parent-child functions.
//!
//! `PATH` walks a parent-child hierarchy: starting at the current row's ID it follows the parent
//! column to the top and returns the IDs joined with `|`, top first. `PATHLENGTH`, `PATHCONTAINS`
//! and `PATHITEM` are plain path-string operations handled by the core evaluator's scalar switch.
//! `PATHITEMREVERSE` is the mirror image of `PATHITEM`: it indexes the delimited path from the
//! last item, so position 1 is the current item and position 2 its immediate parent. A position
//! below 1 or past the end of the path is BLANK, exactly like an out-of-range `PATHITEM`.
#![allow(unused_imports)]

use std::sync::{Arc, Mutex};

use super::*;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    match name.to_ascii_uppercase().as_str() {
        "PATH" => Some(path(ev, name, args, ctx)),
        "PATHITEMREVERSE" => Some(item_reverse(ev, name, args, ctx)),
        _ => None,
    }
}

pub(crate) fn call_table(_ev: &Evaluator<'_>, _name: &str, _args: &[BoundExpr], _ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    None
}

/// The hierarchy separator of the parent-child functions.
const SEPARATOR: char = '|';

/// `PATH ( <ID_ColumnName>, <Parent_ColumnName> )`.
///
/// The two arguments are column references of one table. Starting from the current row's ID, the
/// parent column is followed to the top of the hierarchy and the IDs are joined with `|`, top
/// first. A blank ID is BLANK and a blank parent ends the chain. Following the catalog contract,
/// an ID containing the separator, an ID with two different parents, a parent that is not an ID,
/// and a cycle are all errors.
fn path(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (id_table, id_column) = column_argument(name, args, 0)?;
    let (parent_table, parent_column) = column_argument(name, args, 1)?;
    if id_table != parent_table {
        return Err(EvalError::new(format!("{name} needs two columns of the same table")));
    }
    let id = ev.column_value(id_table, id_column, ctx)?;
    if id.is_blank() {
        return Ok(Scalar::Blank);
    }
    if id.to_text().contains(SEPARATOR) {
        return Err(EvalError::new(format!("{name} ID `{}` contains the hierarchy separator", id.to_text())));
    }
    let table = ev.model().tables.get(id_table).ok_or_else(|| EvalError::new(format!("{name} table is out of range")))?;

    // The parent map and its validation depend only on the hierarchy columns, so one evaluation
    // builds them once: `ADDCOLUMNS(Sales, "Path", PATH(...))` would otherwise rescan the whole
    // hierarchy for every row. The cache sits in the evaluator's memo slot for this evaluation
    // and hands out `Arc` copies, so a lookup never clones the map itself.
    let cache: ParentCache =
        ev.memoized("parent_child_path", || Arc::new(Mutex::new(BTreeMap::new())), |cache| Ok::<ParentCache, EvalError>(Arc::clone(cache)))?;
    let columns = (id_table, id_column, parent_column);
    // The lookup guard is released before the miss path takes the lock again: a temporary in the
    // match scrutinee would stay alive for the whole block and deadlock on the second lock.
    let existing = {
        let guard = cache.lock().unwrap_or_else(|poison| poison.into_inner());
        guard.get(&columns).cloned()
    };
    let parents: Arc<BTreeMap<ScalarKey, ScalarKey>> = match existing {
        Some(map) => map,
        None => {
            let built = Arc::new(build_parent_map(name, table, id_column, parent_column)?);
            cache.lock().unwrap_or_else(|poison| poison.into_inner()).insert(columns, Arc::clone(&built));
            built
        }
    };

    let mut chain = vec![ScalarKey::from(&id)];
    let mut seen: BTreeSet<ScalarKey> = chain.iter().cloned().collect();
    let mut current = ScalarKey::from(&id);
    loop {
        let Some(parent) = parents.get(&current).cloned() else {
            return Err(EvalError::new(format!("{name} ID `{}` is not part of the hierarchy", current.to_scalar().to_text())));
        };
        if matches!(parent, ScalarKey::Blank) {
            break;
        }
        if !seen.insert(parent.clone()) {
            return Err(EvalError::new(format!("{name} hierarchy has a cycle at `{}`", parent.to_scalar().to_text())));
        }
        chain.push(parent.clone());
        current = parent;
    }
    chain.reverse();
    Ok(Scalar::String(chain.iter().map(|key| key.to_scalar().to_dax_text()).collect::<Vec<_>>().join(&SEPARATOR.to_string())))
}

/// The parent map of one hierarchy, validated once per evaluation: one parent per ID, no ID with
/// two different parents, and every parent itself an ID of the hierarchy.
type ParentCache = Arc<Mutex<BTreeMap<(usize, usize, usize), Arc<BTreeMap<ScalarKey, ScalarKey>>>>>;

fn build_parent_map(
    name: &str,
    table: &crate::model::ModelTable,
    id_column: usize,
    parent_column: usize,
) -> Result<BTreeMap<ScalarKey, ScalarKey>, EvalError> {
    // One parent per ID. The same ID with the same parent twice is Excel's common-parent case; a
    // second, different parent makes the hierarchy ambiguous.
    let mut parents: BTreeMap<ScalarKey, ScalarKey> = BTreeMap::new();
    for row in &table.rows {
        let Some(key) = row.get(id_column) else { continue };
        if key.is_blank() {
            continue;
        }
        if key.to_text().contains(SEPARATOR) {
            return Err(EvalError::new(format!("{name} ID `{}` contains the hierarchy separator", key.to_text())));
        }
        let parent = ScalarKey::from(row.get(parent_column).unwrap_or(&Scalar::Blank));
        let key = ScalarKey::from(key);
        match parents.get(&key) {
            Some(existing) if *existing != parent => {
                return Err(EvalError::new(format!("{name} ID `{}` has more than one parent", key.to_scalar().to_text())));
            }
            Some(_) => {}
            None => {
                parents.insert(key, parent);
            }
        }
    }
    for parent in parents.values() {
        if !matches!(parent, ScalarKey::Blank) && !parents.contains_key(parent) {
            return Err(EvalError::new(format!("{name} parent `{}` is not an ID of the hierarchy", parent.to_scalar().to_text())));
        }
    }
    Ok(parents)
}

/// The table and column a `PATH` argument names; anything else is an error, as the catalog says.
fn column_argument(name: &str, args: &[BoundExpr], index: usize) -> Result<(usize, usize), EvalError> {
    match args.get(index) {
        Some(BoundExpr::Column { table, column }) => Ok((*table, *column)),
        _ => Err(EvalError::new(format!("{name} arguments must be column references"))),
    }
}

/// `PATHITEMREVERSE ( <Path>, <Position> [, <Type> ] )`.
///
/// The path is split on the hierarchy separator (`|`) and the item is taken at
/// `len - position`. `Type` selects the result: `TEXT` (or 0), the default, returns the item as
/// a string; `INTEGER` (or 1) converts it to an integer and errors when the item is not one.
fn item_reverse(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let values = super::support::eval_all(ev, args, ctx)?;
    let args = super::support::args(name, &values);
    let path = args.text(0)?;
    let position = args.number(1)?;
    let integer = match args.optional(2) {
        None => false,
        Some(Scalar::Blank) => false,
        Some(Scalar::String(text)) if text.eq_ignore_ascii_case("TEXT") => false,
        Some(Scalar::String(text)) if text.eq_ignore_ascii_case("INTEGER") => true,
        Some(value) => match value.to_number() {
            Some(0.0) => false,
            Some(1.0) => true,
            _ => return Err(EvalError::new(format!("{name} Type must be TEXT (0) or INTEGER (1)"))),
        },
    };

    // A path always has at least one item, so only positions below 1 or past the last item are
    // out of range. Non-finite positions cannot index anything.
    if !position.is_finite() || position < 1.0 {
        return Ok(Scalar::Blank);
    }
    let parts: Vec<&str> = path.split('|').collect();
    let position = position.trunc() as usize;
    let Some(item) = parts.len().checked_sub(position).and_then(|index| parts.get(index)) else {
        return Ok(Scalar::Blank);
    };

    if integer {
        item.trim().parse::<i64>().map(Scalar::Integer).map_err(|_| EvalError::new(format!("{name} item `{item}` is not an integer")))
    } else {
        Ok(Scalar::String((*item).to_string()))
    }
}
