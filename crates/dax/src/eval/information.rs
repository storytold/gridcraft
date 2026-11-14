//! Information functions (non-metadata).
//!
//! Value predicates (`ISBOOLEAN`, `ISSTRING`, ...), filter/scope inspection (`ISFILTERED`,
//! `HASONEVALUE`, ...), name and table helpers (`NAMEOF`, `TABLEOF`), table search (`CONTAINS`,
//! `CONTAINSROW`), the full-text helpers (`TEXTCONTAINS`, `TEXTSIMILARITY`), and the
//! connection/user functions that return fixed values because the in-memory evaluator has no
//! connection properties.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use super::*;
use crate::model::ModelTable;
use crate::value::distinct_key;

/// The upper bound on the wildcard/BM25 work a single string pair may trigger. Text values come
/// from the model, so their length is not under the evaluator's control.
const MAX_TEXT_WORK: usize = 4_000_000;

/// Dispatches the scalar information functions by name, returning `None` when this module does
/// not handle the name.
pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "CONTAINS" => contains(ev, args, ctx),
        "CONTAINSROW" => contains_row(ev, args, ctx),
        "CONTAINSSTRING" => contains_string(ev, args, ctx, false),
        "CONTAINSSTRINGEXACT" => contains_string(ev, args, ctx, true),
        // `USERNAME` is the qualified `DOMAIN\user` identity; `USERPRINCIPALNAME` is the
        // directory principal in `user@domain` form.
        "USERNAME" => Ok(Scalar::String(host_principal())),
        "USERPRINCIPALNAME" => Ok(Scalar::String(host_principal_name())),
        // Connection objects and security-object IDs are not available offline.
        "CUSTOMDATA" | "USEROBJECTID" => Ok(Scalar::String(String::new())),
        "USERCULTURE" => Ok(Scalar::String("en-US".into())),
        "EVALUATEANDLOG" => evaluate_and_log(ev, args, ctx),
        "HASONEFILTER" => has_one_filter(ev, args, ctx),
        "HASONEVALUE" => has_one_value(ev, args, ctx),
        "ISAFTER" => compare_groups(ev, args, ctx, true),
        "ISONORAFTER" => compare_groups(ev, args, ctx, false),
        "ISBOOLEAN" => type_test(ev, args, ctx, "ISBOOLEAN", |value| matches!(value, Scalar::Boolean(_))),
        "ISSTRING" => type_test(ev, args, ctx, "ISSTRING", |value| matches!(value, Scalar::String(_))),
        "ISINT64" => type_test(ev, args, ctx, "ISINT64", |value| matches!(value, Scalar::Integer(_))),
        "ISDECIMAL" => type_test(ev, args, ctx, "ISDECIMAL", |value| matches!(value, Scalar::Currency(_))),
        "ISDOUBLE" => type_test(ev, args, ctx, "ISDOUBLE", |value| matches!(value, Scalar::Decimal(_))),
        "ISDATETIME" => type_test(ev, args, ctx, "ISDATETIME", |value| matches!(value, Scalar::DateTime(_))),
        "ISNUMERIC" => type_test(ev, args, ctx, "ISNUMERIC", |value| matches!(value, Scalar::Integer(_) | Scalar::Currency(_) | Scalar::Decimal(_))),
        "ISCROSSFILTERED" => is_cross_filtered(ev, args, ctx),
        "ISEMPTY" => is_empty(ev, args, ctx),
        "ISFILTERED" => is_filtered(ev, args, ctx),
        "ISINSCOPE" => is_in_scope(ev, args, ctx),
        "ISSELECTEDMEASURE" => {
            // Calculation items are not evaluated by this engine, so no measure is selected.
            Ok(Scalar::Boolean(false))
        }
        "NAMEOF" => name_of(ev, args, ctx),
        "SELECTEDMEASURE" | "SELECTEDMEASUREFORMATSTRING" | "SELECTEDMEASURENAME" => {
            Err(EvalError::new(format!("{} requires a selected measure in a calculation item", name.to_ascii_uppercase())))
        }
        "TEXTCONTAINS" => text_contains(ev, args, ctx),
        "TEXTSIMILARITY" => text_similarity(ev, args, ctx),
        _ => return None,
    };
    Some(result)
}

/// Dispatches the table-valued information functions (`COLUMNSTATISTICS`, `TABLEOF`,
/// `EVALUATEANDLOG`), returning `None` when the name is not handled here.
pub(crate) fn call_table(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<TableValue, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "COLUMNSTATISTICS" => column_statistics(ev),
        "TABLEOF" => table_of(ev, args, ctx),
        "EVALUATEANDLOG" => match args.first() {
            Some(argument) => ev.eval_table(argument, ctx),
            None => Err(EvalError::new("EVALUATEANDLOG needs a value")),
        },
        _ => return None,
    };
    Some(result)
}

/// Resolves a bound expression to a model column. Unqualified `[Column]` names are accepted when
/// exactly one model table carries that column.
fn column_ref(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<(usize, usize), EvalError> {
    match expr {
        BoundExpr::Column { table, column } => Ok((*table, *column)),
        BoundExpr::UnqualifiedColumn { name } => {
            let tables = ev.model().tables_with_column(name);
            match tables.as_slice() {
                [table] => {
                    let column = ev.model().column_index(*table, name).ok_or_else(|| EvalError::new(format!("column `[{name}]` is unknown")))?;
                    Ok((*table, column))
                }
                _ => Err(EvalError::new(format!("column `[{name}]` is ambiguous or unknown"))),
            }
        }
        _ => Err(EvalError::new("expected a column reference")),
    }
}

/// Resolves a table argument, accepting a column reference as shorthand for its table.
fn table_ref(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<usize, EvalError> {
    match expr {
        BoundExpr::Table(table) => Ok(*table),
        other => column_ref(ev, other).map(|(table, _)| table),
    }
}

/// Evaluates the first argument and applies the `ISBOOLEAN`/`ISSTRING`/... predicate named by
/// `name`.
fn type_test(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, name: &str, test: impl Fn(&Scalar) -> bool) -> Result<Scalar, EvalError> {
    let argument = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a value")))?;
    let value = ev.eval(argument, ctx)?;
    Ok(Scalar::Boolean(test(&value)))
}

/// `CONTAINS`: evaluates column/value pairs and reports whether a single table row matches all
/// of them.
fn contains(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let table_expr = args.first().ok_or_else(|| EvalError::new("CONTAINS needs a table"))?;
    let table = ev.eval_table(table_expr, ctx)?;
    let pairs = &args[1..];
    if pairs.len() < 2 || !pairs.len().is_multiple_of(2) {
        return Err(EvalError::new("CONTAINS needs column/value pairs"));
    }
    let mut conditions: Vec<(usize, Scalar)> = Vec::with_capacity(pairs.len() / 2);
    for pair in pairs.as_chunks::<2>().0 {
        let (table_index, column_index) = column_ref(ev, &pair[0])?;
        if table.lineage.is_some_and(|lineage| lineage != table_index) {
            return Err(EvalError::new("CONTAINS column belongs to a different table"));
        }
        let column_name = ev
            .model()
            .tables
            .get(table_index)
            .and_then(|model_table| model_table.columns.get(column_index))
            .map(|column| column.name.clone())
            .ok_or_else(|| EvalError::new("CONTAINS column is out of range"))?;
        // A projection keeps the lineage but not the model layout: resolve by name, always.
        let position = table.column_index(&column_name).ok_or_else(|| EvalError::new(format!("CONTAINS table has no column `{column_name}`")))?;
        let expected = ev.eval(&pair[1], ctx)?;
        conditions.push((position, expected));
    }
    // All pairs must match the same row.
    let found =
        table.rows.iter().any(|row| conditions.iter().all(|(position, expected)| row.get(*position).is_some_and(|value| value.strict_eq(expected))));
    Ok(Scalar::Boolean(found))
}

/// `CONTAINSROW`: reports whether a table row equals the supplied values in every column.
fn contains_row(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let table_expr = args.first().ok_or_else(|| EvalError::new("CONTAINSROW needs a table"))?;
    let table = ev.eval_table(table_expr, ctx)?;
    let values = args[1..].iter().map(|argument| ev.eval(argument, ctx)).collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(EvalError::new("CONTAINSROW needs at least one value"));
    }
    // DAX needs exactly one value per column; fewer would compare only a row's leading columns.
    if table.columns.len() != values.len() {
        return Err(EvalError::new("CONTAINSROW needs one value for each table column"));
    }
    let found = table.rows.iter().any(|row| row.len() >= values.len() && row.iter().zip(&values).all(|(left, right)| left.strict_eq(right)));
    Ok(Scalar::Boolean(found))
}

/// `CONTAINSSTRING`/`CONTAINSSTRINGEXACT`: searches the first text for the second, using either
/// exact substring or Excel wildcard matching.
fn contains_string(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, exact: bool) -> Result<Scalar, EvalError> {
    let within = ev.eval(args.first().ok_or_else(|| EvalError::new("CONTAINSSTRING needs a text"))?, ctx)?.to_text();
    let find = ev.eval(args.get(1).ok_or_else(|| EvalError::new("CONTAINSSTRING needs a search text"))?, ctx)?.to_text();
    if exact {
        // Case- and accent-sensitive, wildcards are plain characters.
        return Ok(Scalar::Boolean(within.contains(&find)));
    }
    Ok(Scalar::Boolean(wildcard_match(&within, &find)?))
}

#[derive(Clone, Copy)]
enum Wildcard {
    Literal(char),
    AnyOne,
    AnySequence,
}

/// Case-insensitive Excel wildcard match: `?` is one character, `*` is any sequence and `~`
/// escapes the following wildcard.
fn wildcard_match(text: &str, pattern: &str) -> Result<bool, EvalError> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let pattern = pattern.to_lowercase();
    if pattern.is_empty() {
        return Ok(true);
    }
    let mut parsed = Vec::new();
    let mut chars = pattern.chars();
    while let Some(character) = chars.next() {
        match character {
            '~' => match chars.next() {
                Some('?') => parsed.push(Wildcard::Literal('?')),
                Some('*') => parsed.push(Wildcard::Literal('*')),
                Some('~') => parsed.push(Wildcard::Literal('~')),
                Some(other) => {
                    parsed.push(Wildcard::Literal('~'));
                    parsed.push(Wildcard::Literal(other));
                }
                None => parsed.push(Wildcard::Literal('~')),
            },
            '?' => parsed.push(Wildcard::AnyOne),
            '*' => parsed.push(Wildcard::AnySequence),
            other => parsed.push(Wildcard::Literal(other)),
        }
    }
    // CONTAINSSTRING searches anywhere in the text, so the pattern is implicitly surrounded by
    // `*` on both sides.
    let mut tokens = vec![Wildcard::AnySequence];
    tokens.extend(parsed);
    tokens.push(Wildcard::AnySequence);
    if text.len().saturating_mul(tokens.len()) > MAX_TEXT_WORK {
        return Err(EvalError::new("CONTAINSSTRING pattern is too large to evaluate"));
    }
    let mut previous = vec![false; text.len() + 1];
    previous[0] = true;
    for token in &tokens {
        let mut current = vec![false; text.len() + 1];
        match *token {
            Wildcard::Literal(expected) => {
                for index in 1..=text.len() {
                    current[index] = previous[index - 1] && text[index - 1] == expected;
                }
            }
            Wildcard::AnyOne => {
                current[1..].copy_from_slice(&previous[..text.len()]);
            }
            Wildcard::AnySequence => {
                current[0] = previous[0];
                for index in 1..=text.len() {
                    current[index] = previous[index] || current[index - 1];
                }
            }
        }
        previous = current;
    }
    Ok(previous[text.len()])
}

/// `HASONEFILTER`: reports whether exactly one filter value is applied to the column.
fn has_one_filter(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("HASONEFILTER needs a column"))?)?;
    let filtered = ctx.filters.filters.get(&(table, column)).is_some_and(|values| values.len() == 1);
    Ok(Scalar::Boolean(filtered))
}

/// `HASONEVALUE`: reports whether the filtered column holds a single distinct value.
fn has_one_value(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("HASONEVALUE needs a column"))?)?;
    let filtered = ev.table(table, &ctx.filters);
    let distinct: BTreeSet<ScalarKey> = filtered.column(column).iter().map(distinct_key).collect();
    Ok(Scalar::Boolean(distinct.len() == 1))
}

/// Compares the `Value1`/`Value2` groups of ISAFTER/ISONORAFTER.
///
/// The optional `Order` flag can follow every pair, which makes the argument sequence ambiguous:
/// in `ISAFTER(1, 1, 0, 1)` the `0` is the first value of the second pair, not a flag. The split is
/// decided before any comparison: a flag is taken only when the arguments that follow it still form
/// complete groups, so a flag after each of two pairs (`1, 1, "DESC", 2, 3, "DESC"`) also parses.
fn compare_groups(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, strictly_after: bool) -> Result<Scalar, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::new("ISAFTER/ISONORAFTER need at least two values"));
    }
    let mut possible = vec![None; args.len() + 1];
    // `evaluated` caches the evaluated value of every flag candidate, so a computed Boolean is
    // evaluated at most once per call: the split phase and the direction read would otherwise
    // evaluate the same expression twice, and a volatile argument such as `RAND () > 0.5` could
    // change direction between the two reads.
    let mut evaluated: Vec<Option<Scalar>> = vec![None; args.len()];
    if !groups(args, 0, ev, ctx, &mut possible, &mut evaluated)? {
        return Err(EvalError::new("ISAFTER/ISONORAFTER need value pairs"));
    }
    let mut index = 0;
    while index < args.len() {
        let left = ev.eval(&args[index], ctx)?;
        let right = ev.eval(&args[index + 1], ctx)?;
        index += 2;
        let mut ascending = true;
        if index < args.len()
            && flag_candidate(ev, args, index, ctx, &mut evaluated)?
            && groups(args, index + 1, ev, ctx, &mut possible, &mut evaluated)?
        {
            // The split phase already evaluated this flag into `evaluated`, so the direction
            // read reuses the cached value and never evaluates twice.
            ascending = flag_direction(&args[index], &evaluated[index]);
            index += 1;
        }
        let ordering = ordering_with_blanks(&left, &right)?;
        let ordering = if ascending { ordering } else { ordering.reverse() };
        if ordering == Ordering::Greater {
            return Ok(Scalar::Boolean(true));
        }
        if ordering == Ordering::Less {
            return Ok(Scalar::Boolean(false));
        }
        // The pairs are equal so far: the next group decides.
    }
    Ok(Scalar::Boolean(!strictly_after))
}

/// Whether `args[start..]` splits into two-argument pairs, each optionally followed by an `Order`
/// flag. Memoised per start position, and the flag test of every argument is evaluated at most
/// once, so a long argument list cannot make the split exponential or repeat work.
fn groups(
    args: &[BoundExpr],
    start: usize,
    ev: &Evaluator<'_>,
    ctx: &mut Context,
    possible: &mut [Option<bool>],
    evaluated: &mut [Option<Scalar>],
) -> Result<bool, EvalError> {
    if start == args.len() {
        return Ok(true);
    }
    if start + 2 > args.len() {
        return Ok(false);
    }
    if let Some(known) = possible[start] {
        return Ok(known);
    }
    // A flag is preferred when the arguments after it still group, so an optional `Order` is not
    // mistaken for the first value of the next pair.
    let mut splits = false;
    if start + 2 < args.len() && flag_candidate(ev, args, start + 2, ctx, evaluated)? {
        splits = groups(args, start + 3, ev, ctx, possible, evaluated)?;
    }
    if !splits {
        splits = groups(args, start + 2, ev, ctx, possible, evaluated)?;
    }
    possible[start] = Some(splits);
    Ok(splits)
}

/// Whether the argument at `index` can be an `Order` flag, evaluated at most once per call.
///
/// The single evaluation is stored in `evaluated`, which also feeds [`flag_direction`]: the
/// direction read reuses the cached [`Scalar`] instead of evaluating the expression again.
fn flag_candidate(
    ev: &Evaluator<'_>,
    args: &[BoundExpr],
    index: usize,
    ctx: &mut Context,
    evaluated: &mut [Option<Scalar>],
) -> Result<bool, EvalError> {
    if let Some(cached) = evaluated[index].as_ref() {
        return Ok(is_order_scalar(&args[index], cached));
    }
    let value = ev.eval(&args[index], ctx)?;
    let candidate = is_order_scalar(&args[index], &value);
    evaluated[index] = Some(value);
    Ok(candidate)
}
/// Whether an already-evaluated argument reads as an `Order` flag: a Boolean, an `ASC`/`DESC`
/// keyword, or the documented `0`/`1` shorthand. A computed flag counts when it produced a
/// Boolean, matching the old `is_order_flag` behaviour; [`flag_direction`] then decides the side.
fn is_order_scalar(argument: &BoundExpr, value: &Scalar) -> bool {
    match argument {
        BoundExpr::Keyword(word) => word.eq_ignore_ascii_case("ASC") || word.eq_ignore_ascii_case("DESC"),
        BoundExpr::Literal(Literal::Boolean(_)) => matches!(value, Scalar::Boolean(_)),
        BoundExpr::Literal(Literal::Integer(_)) => matches!(value, Scalar::Integer(0) | Scalar::Integer(1)),
        BoundExpr::Literal(Literal::Decimal(_)) => matches!(value, Scalar::Decimal(number) if *number == 0.0 || *number == 1.0),
        _ => matches!(value, Scalar::Boolean(_)),
    }
}
/// The sort direction of an already-evaluated `Order` flag from its cached [`Scalar`]: `DESC`
/// is descending and unparsable values default to ascending.
fn flag_direction(argument: &BoundExpr, cached: &Option<Scalar>) -> bool {
    if let BoundExpr::Keyword(word) = argument {
        return !word.eq_ignore_ascii_case("DESC");
    }
    cached.as_ref().and_then(Scalar::to_bool).unwrap_or(true)
}

/// DAX ordering with BLANK before every value and an empty string after BLANK (but before other
/// text). Text compares case-insensitively.
fn ordering_with_blanks(left: &Scalar, right: &Scalar) -> Result<Ordering, EvalError> {
    match (left, right) {
        (Scalar::Blank, Scalar::Blank) => Ok(Ordering::Equal),
        (Scalar::Blank, _) => Ok(Ordering::Less),
        (_, Scalar::Blank) => Ok(Ordering::Greater),
        (Scalar::String(first), Scalar::String(second)) => Ok(first.to_lowercase().cmp(&second.to_lowercase())),
        _ => left.compare(right).ok_or_else(|| EvalError::new("ISAFTER/ISONORAFTER cannot compare these values")),
    }
}

/// `ISFILTERED`: reports whether any filter is applied to the column.
fn is_filtered(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("ISFILTERED needs a column"))?)?;
    Ok(Scalar::Boolean(ctx.filters.filters.contains_key(&(table, column))))
}

/// `ISCROSSFILTERED`: reports whether the table is filtered directly or transitively through the
/// relationship graph, mirroring the evaluator's `table_has_filter` propagation.
fn is_cross_filtered(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let target = table_ref(ev, args.first().ok_or_else(|| EvalError::new("ISCROSSFILTERED needs a table or column"))?)?;
    let filtered: BTreeSet<usize> = ctx.filters.filters.keys().map(|(table, _)| *table).collect();
    if filtered.contains(&target) {
        return Ok(Scalar::Boolean(true));
    }
    // Mirror the evaluator's own propagation instead of checking the relationships that touch the
    // target once: a filter on the `to` table restricts the `from` table, a filter on the `from`
    // table restricts the `to` table only when the relationship is bidirectional, and both rules
    // chain, so a filter two hops away (`Sales -> Product -> Category`) still cross-filters the
    // target. `table_has_filter` walks exactly that graph, applying the
    // `USERELATIONSHIP`/`CROSSFILTER` overrides in `ctx.filters.relationships`, and its visited
    // set keeps bidirectional pairs and cycles bounded. The direct-filter case is handled above,
    // so only an indirectly filtered target can reach this point as `true`.
    let mut visited = BTreeSet::new();
    Ok(Scalar::Boolean(ev.table_has_filter(target, &ctx.filters, &mut visited)))
}

/// `ISEMPTY`: evaluates a table and reports whether it has no rows.
fn is_empty(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let table = ev.eval_table(args.first().ok_or_else(|| EvalError::new("ISEMPTY needs a table"))?, ctx)?;
    Ok(Scalar::Boolean(table.rows.is_empty()))
}

/// `ISINSCOPE`: reports whether the column is a group-by column of the group being evaluated.
fn is_in_scope(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("ISINSCOPE needs a column"))?)?;
    // Only the group-by columns of the `SUMMARIZE` group being evaluated are in scope. Hierarchy
    // levels are not modelled, so every group-by column of the current group is in scope, a
    // rolled-up column is not, and outside a group (`None`) no column is in scope — a row
    // iterator's row context does not put a column in scope.
    let in_scope = ctx.grouping_scope.as_ref().is_some_and(|columns| columns.contains(&(table, column)));
    Ok(Scalar::Boolean(in_scope))
}

/// `EVALUATEANDLOG`: evaluates and returns the first argument unchanged; logging is only a trace
/// side effect.
fn evaluate_and_log(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let value = ev.eval(args.first().ok_or_else(|| EvalError::new("EVALUATEANDLOG needs a value"))?, ctx)?;
    // The log is a trace event; the optional label and row cap do not change the returned value.
    Ok(value)
}

// ---------------------------------------------------------------------------------------------
// NAMEOF
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum NameComponent {
    Table,
    Column,
    Measure,
    Calendar,
    Full,
    SelfName,
    Parent,
}

#[derive(Clone, Copy, PartialEq)]
enum NameEscape {
    Escaped,
    Unescaped,
    Minimal,
}

enum NameObject {
    Table(usize),
    Column(usize, usize),
    Measure(usize),
}

/// `NAMEOF`: resolves an object reference and returns its name for the requested component and
/// escape mode.
fn name_of(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let object = name_object(ev, args.first().ok_or_else(|| EvalError::new("NAMEOF needs a reference"))?)?;
    let component = match args.get(1) {
        None => NameComponent::Full,
        Some(argument) => {
            let value = ev.eval(argument, ctx)?;
            if value.is_blank() { NameComponent::Full } else { parse_component(&value.to_text())? }
        }
    };
    let escape = match args.get(2) {
        None => NameEscape::Escaped,
        Some(argument) => {
            let value = ev.eval(argument, ctx)?;
            if value.is_blank() { NameEscape::Escaped } else { parse_escape(&value.to_text())? }
        }
    };
    let text = name_of_text(ev, &object, component, escape)?;
    Ok(Scalar::String(text))
}

/// Resolves a bound expression to the table, column or measure that `NAMEOF` describes.
fn name_object(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<NameObject, EvalError> {
    match expr {
        BoundExpr::Table(table) => Ok(NameObject::Table(*table)),
        BoundExpr::Column { table, column } => Ok(NameObject::Column(*table, *column)),
        BoundExpr::Measure(measure) => Ok(NameObject::Measure(*measure)),
        BoundExpr::UnqualifiedColumn { .. } => {
            let (table, column) = column_ref(ev, expr)?;
            Ok(NameObject::Column(table, column))
        }
        _ => Err(EvalError::new("NAMEOF needs a table, column or measure reference")),
    }
}

/// Parses the optional `NAMEOF` component argument (`TABLE`, `COLUMN`, `FULL`, ...).
fn parse_component(text: &str) -> Result<NameComponent, EvalError> {
    match text.trim().to_ascii_uppercase().as_str() {
        "TABLE" => Ok(NameComponent::Table),
        "COLUMN" => Ok(NameComponent::Column),
        "MEASURE" => Ok(NameComponent::Measure),
        "CALENDAR" => Ok(NameComponent::Calendar),
        "FULL" => Ok(NameComponent::Full),
        "SELF" => Ok(NameComponent::SelfName),
        "PARENT" => Ok(NameComponent::Parent),
        other => Err(EvalError::new(format!("NAMEOF does not know component `{other}`"))),
    }
}

/// Parses the optional `NAMEOF` escape argument (`ESCAPED`, `UNESCAPED`, `MINIMALLYESCAPED`).
fn parse_escape(text: &str) -> Result<NameEscape, EvalError> {
    match text.trim().to_ascii_uppercase().as_str() {
        "ESCAPED" => Ok(NameEscape::Escaped),
        "UNESCAPED" => Ok(NameEscape::Unescaped),
        "MINIMALLYESCAPED" => Ok(NameEscape::Minimal),
        other => Err(EvalError::new(format!("NAMEOF does not know escape mode `{other}`"))),
    }
}

/// Formats the name of a `NAMEOF` object for the given component and escape mode.
fn name_of_text(ev: &Evaluator<'_>, object: &NameObject, component: NameComponent, escape: NameEscape) -> Result<String, EvalError> {
    if component == NameComponent::Calendar {
        return Err(EvalError::new("NAMEOF: the model has no calendar objects"));
    }
    let (table_name, child) = match object {
        NameObject::Table(table) => {
            let name = model_table_name(ev, *table)?;
            (Some(name), None)
        }
        NameObject::Column(table, column) => {
            let table_name = model_table_name(ev, *table)?;
            let column_name = ev
                .model()
                .tables
                .get(*table)
                .and_then(|model_table| model_table.columns.get(*column))
                .map(|column| column.name.clone())
                .ok_or_else(|| EvalError::new("NAMEOF column is out of range"))?;
            (Some(table_name), Some(column_name))
        }
        NameObject::Measure(measure) => {
            let measure = ev.model().measures.get(*measure).ok_or_else(|| EvalError::new("NAMEOF measure is out of range"))?;
            let table_name = match &measure.table {
                Some(name) => Some(model_table_name(ev, model_table_index(ev, name)?)?),
                None => None,
            };
            (table_name, Some(measure.name.clone()))
        }
    };

    match component {
        NameComponent::Full => {
            if escape == NameEscape::Unescaped && table_name.is_some() && child.is_some() {
                return Err(EvalError::new("NAMEOF cannot return a fully qualified name UNESCAPED"));
            }
            match (&table_name, &child) {
                (Some(table_name), Some(child)) => {
                    // In a qualified FULL name the child component is always delimited, even in
                    // MINIMALLYESCAPED mode: `Product[Name]`, never `ProductName`.
                    Ok(format!("{}{}", escape_table(table_name, escape), escape_qualified_child(child, escape)))
                }
                (Some(table_name), None) => Ok(escape_table(table_name, escape)),
                (None, Some(child)) => Ok(escape_child(child, escape)),
                (None, None) => Err(EvalError::new("NAMEOF has no name to return")),
            }
        }
        NameComponent::Table => table_name.map(|name| escape_table(&name, escape)).ok_or_else(|| EvalError::new("NAMEOF: this object has no table")),
        NameComponent::Parent => {
            if child.is_none() {
                return Err(EvalError::new("NAMEOF: a table has no parent"));
            }
            table_name.map(|name| escape_table(&name, escape)).ok_or_else(|| EvalError::new("NAMEOF: this object has no parent table"))
        }
        NameComponent::Column => match (&child, object) {
            (Some(child), NameObject::Column(..)) => Ok(escape_child(child, escape)),
            _ => Err(EvalError::new("NAMEOF: this object is not a column")),
        },
        NameComponent::Measure => match (&child, object) {
            (Some(child), NameObject::Measure(_)) => Ok(escape_child(child, escape)),
            _ => Err(EvalError::new("NAMEOF: this object is not a measure")),
        },
        NameComponent::SelfName => match (&table_name, &child) {
            (_, Some(child)) => Ok(escape_child(child, escape)),
            (Some(table_name), None) => Ok(escape_table(table_name, escape)),
            (None, None) => Err(EvalError::new("NAMEOF has no name to return")),
        },
        NameComponent::Calendar => Err(EvalError::new("NAMEOF: the model has no calendar objects")),
    }
}

/// Looks up a model table index by name for the `NAMEOF` helpers.
fn model_table_index(ev: &Evaluator<'_>, name: &str) -> Result<usize, EvalError> {
    ev.model().table_index(name).ok_or_else(|| EvalError::new(format!("NAMEOF: unknown table `{name}`")))
}

/// Returns a model table's name by index for the `NAMEOF` helpers.
fn model_table_name(ev: &Evaluator<'_>, table: usize) -> Result<String, EvalError> {
    ev.model().tables.get(table).map(|model_table| model_table.name.clone()).ok_or_else(|| EvalError::new("NAMEOF table is out of range"))
}

/// Formats a table name as `'Quoted'` when the escape mode asks for delimiters.
fn escape_table(name: &str, escape: NameEscape) -> String {
    match escape {
        NameEscape::Unescaped => name.to_string(),
        NameEscape::Minimal if is_simple_name(name) => name.to_string(),
        NameEscape::Escaped | NameEscape::Minimal => format!("'{}'", name.replace('\'', "''")),
    }
}

/// Formats a column or measure name as `[Bracketed]` when the escape mode asks for delimiters.
fn escape_child(name: &str, escape: NameEscape) -> String {
    match escape {
        NameEscape::Unescaped => name.to_string(),
        NameEscape::Minimal if is_simple_name(name) => name.to_string(),
        NameEscape::Escaped | NameEscape::Minimal => format!("[{}]", name.replace(']', "]]")),
    }
}

/// A child component of a qualified name is always delimited when escaping is requested.
fn escape_qualified_child(name: &str, escape: NameEscape) -> String {
    match escape {
        NameEscape::Unescaped => name.to_string(),
        NameEscape::Escaped | NameEscape::Minimal => format!("[{}]", name.replace(']', "]]")),
    }
}

/// Reports whether a name is a plain identifier that needs no quoting under minimal escaping.
fn is_simple_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else { return false };
    (first.is_alphabetic() || first == '_') && characters.all(|character| character.is_alphanumeric() || character == '_')
}

/// `TABLEOF`: returns the unfiltered table that contains the referenced table, column or measure.
fn table_of(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<TableValue, EvalError> {
    let argument = args.first().ok_or_else(|| EvalError::new("TABLEOF needs a reference"))?;
    // Ignore arguments: TABLEOF returns the containing table, not a filtered view.
    let _ = ctx;
    let table = match argument {
        BoundExpr::Table(table) => *table,
        BoundExpr::Measure(measure) => {
            let measure = ev.model().measures.get(*measure).ok_or_else(|| EvalError::new("TABLEOF measure is out of range"))?;
            match &measure.table {
                Some(name) => ev.model().table_index(name).ok_or_else(|| EvalError::new(format!("TABLEOF: unknown table `{name}`")))?,
                None => return Err(EvalError::new("TABLEOF: the measure has no home table")),
            }
        }
        other => column_ref(ev, other)?.0,
    };
    Ok(ev.table(table, &FilterContext::default()))
}

// ---------------------------------------------------------------------------------------------
// COLUMNSTATISTICS
// ---------------------------------------------------------------------------------------------

/// `COLUMNSTATISTICS`: builds a table of per-column minimum, maximum, cardinality and maximum
/// string length.
fn column_statistics(ev: &Evaluator<'_>) -> Result<TableValue, EvalError> {
    let columns = ["Table Name", "Column Name", "Min", "Max", "Cardinality", "Max Length"].map(|name| TableColumn { name: name.into() });
    let mut rows = Vec::new();
    for model_table in &ev.model().tables {
        for (index, column) in model_table.columns.iter().enumerate() {
            let values: Vec<&Scalar> = model_table.rows.iter().filter_map(|row| row.get(index)).collect();
            let mut minimum: Option<&Scalar> = None;
            let mut maximum: Option<&Scalar> = None;
            let mut cardinality = BTreeSet::new();
            let mut max_length: Option<usize> = None;
            for value in values.iter().copied() {
                cardinality.insert(distinct_key(value));
                if minimum.is_none_or(|current| statistic_order(value, current) == Some(Ordering::Less)) {
                    minimum = Some(value);
                }
                if maximum.is_none_or(|current| statistic_order(value, current) == Some(Ordering::Greater)) {
                    maximum = Some(value);
                }
                if let Scalar::String(text) = value {
                    let length = text.chars().count();
                    max_length = Some(max_length.map_or(length, |current| current.max(length)));
                }
            }
            rows.push(vec![
                Scalar::String(model_table.name.clone()),
                Scalar::String(column.name.clone()),
                minimum.cloned().unwrap_or(Scalar::Blank),
                maximum.cloned().unwrap_or(Scalar::Blank),
                Scalar::Integer(cardinality.len() as i64),
                max_length.map_or(Scalar::Blank, |length| Scalar::Integer(length as i64)),
            ]);
        }
    }
    Ok(TableValue::new(columns.to_vec(), rows))
}

/// Statistics ordering: BLANK sorts before every value; text compares case-insensitively.
fn statistic_order(left: &Scalar, right: &Scalar) -> Option<Ordering> {
    match (left, right) {
        (Scalar::Blank, Scalar::Blank) => Some(Ordering::Equal),
        (Scalar::Blank, _) => Some(Ordering::Less),
        (_, Scalar::Blank) => Some(Ordering::Greater),
        (Scalar::String(first), Scalar::String(second)) => Some(first.to_lowercase().cmp(&second.to_lowercase())),
        _ => left.compare(right),
    }
}

// ---------------------------------------------------------------------------------------------
// TEXTCONTAINS / TEXTSIMILARITY
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchMode {
    Text,
    Fuzzy,
    Phrase,
}

/// Reads the optional match-mode argument of `TEXTCONTAINS`/`TEXTSIMILARITY`, defaulting to text
/// matching when absent or blank.
fn match_mode(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, index: usize) -> Result<MatchMode, EvalError> {
    let Some(argument) = args.get(index) else { return Ok(MatchMode::Text) };
    let value = ev.eval(argument, ctx)?;
    if value.is_blank() {
        return Ok(MatchMode::Text);
    }
    match value.to_text().trim().to_ascii_uppercase().as_str() {
        "TEXTMATCHING" => Ok(MatchMode::Text),
        "FUZZYMATCHING" => Ok(MatchMode::Fuzzy),
        "PHRASEMATCHING" => Ok(MatchMode::Phrase),
        other => Err(EvalError::new(format!("unknown full-text match mode `{other}`"))),
    }
}

/// Lowercase alphanumeric tokens; stemming and stop-word removal are language features of a
/// full-text index and are not modelled here.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric()).filter(|token| !token.is_empty()).map(|token| token.to_lowercase()).collect()
}

/// Reports whether a token matches a query term under the given mode, exactly or within the
/// fuzzy edit-distance threshold.
fn token_matches(token: &str, term: &str, mode: MatchMode) -> bool {
    match mode {
        MatchMode::Text | MatchMode::Phrase => token == term,
        MatchMode::Fuzzy => fuzzy_distance(token, term) <= fuzzy_threshold(term.chars().count()),
    }
}

/// Allowed edit distance for fuzzy matching: two edits from five characters, one from three, and
/// exact matching below that.
fn fuzzy_threshold(length: usize) -> usize {
    if length >= 5 {
        2
    } else if length >= 3 {
        1
    } else {
        0
    }
}

/// Edit distance, bounded: tokens longer than 256 characters are compared only when identical so
/// a hostile value cannot force a quadratic allocation.
fn fuzzy_distance(left: &str, right: &str) -> usize {
    if left == right {
        return 0;
    }
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.len() > 256 || right.len() > 256 {
        return usize::MAX;
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (row, left_character) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_character) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_character != right_character);
            current[column + 1] = substitution.min(previous[column + 1] + 1).min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// Reports whether the query tokens occur as a contiguous phrase within the document tokens.
fn phrase_matches(tokens: &[String], query: &[String]) -> bool {
    if query.is_empty() || query.len() > tokens.len() {
        return false;
    }
    tokens.windows(query.len()).any(|window| window.iter().zip(query).all(|(left, right)| left == right))
}

/// `TEXTCONTAINS`: tokenizes both texts and searches for a query term or phrase according to the
/// match mode.
fn text_contains(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let mode = match_mode(ev, args, ctx, 2)?;
    let within = ev.eval(args.first().ok_or_else(|| EvalError::new("TEXTCONTAINS needs a column"))?, ctx)?.to_text();
    let search = ev.eval(args.get(1).ok_or_else(|| EvalError::new("TEXTCONTAINS needs a search text"))?, ctx)?.to_text();
    let tokens = tokenize(&within);
    let query = tokenize(&search);
    if query.is_empty() {
        return Ok(Scalar::Boolean(false));
    }
    let matched = match mode {
        MatchMode::Text | MatchMode::Fuzzy => query.iter().any(|term| tokens.iter().any(|token| token_matches(token, term, mode))),
        MatchMode::Phrase => phrase_matches(&tokens, &query),
    };
    Ok(Scalar::Boolean(matched))
}

/// `TEXTSIMILARITY`: scores the searched text against the column's corpus and returns the BM25
/// relevance value.
fn text_similarity(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let argument = args.first().ok_or_else(|| EvalError::new("TEXTSIMILARITY needs a column"))?;
    let within = ev.eval(argument, ctx)?.to_text();
    let search = ev.eval(args.get(1).ok_or_else(|| EvalError::new("TEXTSIMILARITY needs a search text"))?, ctx)?.to_text();
    let mode = match_mode(ev, args, ctx, 2)?;
    let query = tokenize(&search);
    if query.is_empty() {
        return Ok(Scalar::Decimal(0.0));
    }
    let stats = match similarity_stats(ev, argument, &search, &query, mode)? {
        Some(stats) => stats,
        // Without a model column to rank against (a scalar argument, or a table with no rows)
        // the searched text is its own corpus, as the single-document BM25 form scores it.
        None => single_document_stats(&tokenize(&within), &query, mode),
    };
    Ok(Scalar::Decimal(similarity_score(&stats, &tokenize(&within), &query, mode)))
}

/// The corpus statistics BM25 needs, computed once per column and query so a row iterator
/// (`MAXX(Product, TEXTSIMILARITY(Product[Name], "red"))`) does not rebuild them for every row:
/// the document count, the average document length and, for each query term, how many documents
/// contain it.
#[derive(Clone, PartialEq)]
struct SimilarityStats {
    /// Number of documents in the corpus; at least one, so the IDF term stays finite.
    document_count: f64,
    /// Mean token count per document; at least one, so the length normalization never divides by
    /// zero.
    average_length: f64,
    /// Matching-document count per query term, in `query` order.
    matches: Vec<f64>,
}

/// The cache key of [`SimilarityStats`]: the column, the query and the match mode. The memo itself
/// lives on the [`Evaluator`] (via its `memoized` slot), so the key needs no model identity and can
/// never outlive the evaluation or the model borrow.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SimilarityKey {
    table: usize,
    column: usize,
    search: String,
    mode: MatchMode,
}

/// BM25 statistics for the columns and queries one evaluation has scored, keyed by
/// [`SimilarityKey`]. `TEXTSIMILARITY` runs once per row inside a row iterator, so the corpus is
/// built on the first call and reused for the rest of the evaluation; two queries in the same
/// evaluation keep separate entries. The memo is dropped with the evaluator.
type SimilarityMemo = std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<SimilarityKey, SimilarityStats>>>;

/// The tag this module's slot uses in [`Evaluator::memoized`].
const SIMILARITY_MEMO_TAG: &str = "textsimilarity";

/// The characters the column's values take once converted to text, counted without allocating the
/// column: a `String` contributes its own length and every other variant is measured by writing it
/// into a counting sink with the same format `Scalar::to_text` uses, so the work budget matches the
/// conversion it guards instead of guessing at a bound.
fn corpus_characters(model_table: &ModelTable, column: usize) -> usize {
    model_table.rows.iter().filter_map(|row| row.get(column)).map(scalar_text_length).sum()
}

/// A [`std::fmt::Write`] sink that counts characters instead of storing them.
struct CharacterCounter(usize);

impl std::fmt::Write for CharacterCounter {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0 += text.chars().count();
        Ok(())
    }
}

/// `Scalar::to_text().chars().count()` without the `String` in between.
fn scalar_text_length(value: &Scalar) -> usize {
    use std::fmt::Write as _;
    let mut counter = CharacterCounter(0);
    // Writing into the counter cannot fail, so the result is ignored.
    let _ = match value {
        Scalar::Blank => Ok(()),
        Scalar::Boolean(true) => counter.write_str("TRUE"),
        Scalar::Boolean(false) => counter.write_str("FALSE"),
        Scalar::Integer(number) => write!(counter, "{number}"),
        Scalar::Currency(minor) => write!(counter, "{:.4}", *minor as f64 / 10_000.0),
        Scalar::Decimal(number) => {
            if number.fract() == 0.0 {
                write!(counter, "{number:.1}")
            } else {
                write!(counter, "{number}")
            }
        }
        Scalar::DateTime(serial) => write!(counter, "{serial}"),
        Scalar::String(text) => {
            counter.0 += text.chars().count();
            Ok(())
        }
    };
    counter.0
}

/// The corpus statistics for `expr` under `search`/`mode`, or `None` when `expr` does not name a
/// column with rows. The statistics are memoised on the evaluator, so a row iterator builds the
/// corpus once per column and query, and the work budget is checked before any value is converted
/// to text — a column that cannot be scored fails instead of being copied row by row.
fn similarity_stats(
    ev: &Evaluator<'_>,
    expr: &BoundExpr,
    search: &str,
    query: &[String],
    mode: MatchMode,
) -> Result<Option<SimilarityStats>, EvalError> {
    let Ok((table, column)) = column_ref(ev, expr) else { return Ok(None) };
    let Some(model_table) = ev.model().tables.get(table) else { return Ok(None) };
    if model_table.rows.is_empty() {
        return Ok(None);
    }
    let key = SimilarityKey { table, column, search: search.to_string(), mode };
    let memo = ev.memoized(SIMILARITY_MEMO_TAG, SimilarityMemo::default, std::sync::Arc::clone);
    let mut memo = memo.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(stats) = memo.get(&key) {
        return Ok(Some(stats.clone()));
    }
    let characters = corpus_characters(model_table, column);
    if characters.saturating_mul(search.chars().count().max(1)) > MAX_TEXT_WORK {
        return Err(EvalError::new("TEXTSIMILARITY: the column is too large to score within the text work budget"));
    }
    let values: Vec<String> = model_table.rows.iter().filter_map(|row| row.get(column)).map(|value| value.to_text()).collect();
    if values.is_empty() {
        return Ok(None);
    }
    let documents: Vec<Vec<String>> = values.iter().map(|value| tokenize(value)).collect();
    let document_count = documents.len() as f64;
    let total_length: usize = documents.iter().map(Vec::len).sum();
    let average_length = (total_length as f64 / document_count).max(1.0);
    let matches: Vec<f64> = query
        .iter()
        .map(|term| documents.iter().filter(|document| document.iter().any(|token| token_matches(token, term, mode))).count() as f64)
        .collect();
    let stats = SimilarityStats { document_count, average_length, matches };
    memo.insert(key, stats.clone());
    Ok(Some(stats))
}
/// The statistics of a one-document corpus made of the searched text itself, used when there is no
/// model column to rank against.
fn single_document_stats(target: &[String], query: &[String], mode: MatchMode) -> SimilarityStats {
    let matches = query.iter().map(|term| f64::from(u8::from(target.iter().any(|token| token_matches(token, term, mode))))).collect();
    SimilarityStats { document_count: 1.0, average_length: (target.len() as f64).max(1.0), matches }
}

/// BM25 relevance (k1 = 1.2, b = 0.75) of `target` against `stats`. The score is an opaque ranking
/// value, so only its sign and relative order are meaningful.
fn similarity_score(stats: &SimilarityStats, target: &[String], query: &[String], mode: MatchMode) -> f64 {
    if mode == MatchMode::Phrase && !phrase_matches(target, query) {
        return 0.0;
    }
    let (k1, b) = (1.2, 0.75);
    let document_length = target.len() as f64;
    let mut score = 0.0;
    for (term, matching_documents) in query.iter().zip(&stats.matches) {
        let term_frequency = target.iter().filter(|token| token_matches(token, term, mode)).count();
        if term_frequency == 0 {
            continue;
        }
        let inverse_frequency = (1.0 + (stats.document_count - matching_documents + 0.5) / (matching_documents + 0.5)).ln();
        let denominator = term_frequency as f64 + k1 * (1.0 - b + b * document_length / stats.average_length);
        if denominator > 0.0 {
            score += inverse_frequency * (term_frequency as f64 * (k1 + 1.0)) / denominator;
        }
    }
    if score.is_finite() { score.max(0.0) } else { 0.0 }
}
/// The operating-system user name (`USERNAME`). Empty when the host does not expose one.
fn host_user() -> String {
    std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default()
}

/// `USERNAME`: the qualified `DOMAIN\user` identity; on a host without a domain the plain user
/// name is the best available identity.
fn host_principal() -> String {
    let user = host_user();
    match (std::env::var("USERDOMAIN"), user.as_str()) {
        (Ok(domain), user) if !domain.is_empty() && !user.is_empty() => format!("{domain}\\{user}"),
        (_, user) => user.to_string(),
    }
}

/// `USERPRINCIPALNAME`: the directory principal name in `user@domain` form. The host's DNS
/// domain is preferred over its NetBIOS domain; without one, the plain user name is the best
/// available identity.
fn host_principal_name() -> String {
    let user = host_user();
    if user.is_empty() {
        return user;
    }
    let domain = std::env::var("USERDNSDOMAIN")
        .ok()
        .filter(|domain| !domain.is_empty())
        .or_else(|| std::env::var("USERDOMAIN").ok().filter(|domain| !domain.is_empty()));
    match domain {
        Some(domain) => format!("{user}@{}", domain.to_ascii_lowercase()),
        None => user,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DataModel, ModelColumn, ModelTable};

    fn model() -> DataModel {
        DataModel {
            tables: vec![ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Color".into(), data_type: Some("String".into()) },
                ],
                rows: vec![vec![Scalar::String("Widget".into()), Scalar::String("Red".into())]],
            }],
            relationships: vec![],
            measures: vec![],
        }
    }

    /// `ISINSCOPE(Product[column])` with an explicit grouping scope.
    fn in_scope(columns: Option<Vec<(usize, usize)>>, column: &str) -> bool {
        let model = model();
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr = crate::parse_expression(&format!("ISINSCOPE(Product[{column}])")).expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        let BoundExpr::Call { args, .. } = bound else { panic!("ISINSCOPE is not a call") };
        let mut ctx = Context { filters: FilterContext::default(), rows: Vec::new(), vars: Vec::new(), grouping_scope: columns };
        match is_in_scope(&evaluator, &args, &mut ctx).expect("in scope") {
            Scalar::Boolean(value) => value,
            other => panic!("ISINSCOPE answered {other:?}"),
        }
    }

    #[test]
    fn in_scope_reads_the_grouping_scope() {
        // Product is table 0, `Name` column 0 and `Color` column 1.
        assert!(in_scope(Some(vec![(0, 1)]), "Color"), "the group-by column is in scope");
        assert!(!in_scope(Some(vec![(0, 1)]), "Name"), "another column of the same group is not");
        assert!(!in_scope(None, "Color"), "outside a group no column is in scope");
    }
}
