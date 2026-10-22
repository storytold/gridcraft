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

/// The upper bound on the wildcard/BM25 work a single string pair may trigger. Text values come
/// from the model, so their length is not under the evaluator's control.
const MAX_TEXT_WORK: usize = 4_000_000;

pub(crate) fn call(ev: &Evaluator<'_>, name: &str, args: &[BoundExpr], ctx: &mut Context) -> Option<Result<Scalar, EvalError>> {
    let result = match name.to_ascii_uppercase().as_str() {
        "CONTAINS" => contains(ev, args, ctx),
        "CONTAINSROW" => contains_row(ev, args, ctx),
        "CONTAINSSTRING" => contains_string(ev, args, ctx, false),
        "CONTAINSSTRINGEXACT" => contains_string(ev, args, ctx, true),
        // Excel's embedded engine reports the qualified identity for both functions.
        "USERNAME" => Ok(Scalar::String(host_principal())),
        "USERPRINCIPALNAME" => Ok(Scalar::String(host_principal())),
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
        "ISSUBTOTAL" => is_subtotal(ev, args, ctx),
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

fn table_ref(ev: &Evaluator<'_>, expr: &BoundExpr) -> Result<usize, EvalError> {
    match expr {
        BoundExpr::Table(table) => Ok(*table),
        other => column_ref(ev, other).map(|(table, _)| table),
    }
}

fn type_test(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, name: &str, test: impl Fn(&Scalar) -> bool) -> Result<Scalar, EvalError> {
    let argument = args.first().ok_or_else(|| EvalError::new(format!("{name} needs a value")))?;
    let value = ev.eval(argument, ctx)?;
    Ok(Scalar::Boolean(test(&value)))
}

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
        let position = match table.lineage {
            Some(lineage) if lineage == table_index => column_index,
            _ => table.column_index(&column_name).ok_or_else(|| EvalError::new(format!("CONTAINS table has no column `{column_name}`")))?,
        };
        let expected = ev.eval(&pair[1], ctx)?;
        conditions.push((position, expected));
    }
    // All pairs must match the same row.
    let found =
        table.rows.iter().any(|row| conditions.iter().all(|(position, expected)| row.get(*position).is_some_and(|value| value.strict_eq(expected))));
    Ok(Scalar::Boolean(found))
}

fn contains_row(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let table_expr = args.first().ok_or_else(|| EvalError::new("CONTAINSROW needs a table"))?;
    let table = ev.eval_table(table_expr, ctx)?;
    let values = args[1..].iter().map(|argument| ev.eval(argument, ctx)).collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() {
        return Err(EvalError::new("CONTAINSROW needs at least one value"));
    }
    if table.columns.len() < values.len() {
        return Err(EvalError::new("CONTAINSROW has more values than the table has columns"));
    }
    let found = table.rows.iter().any(|row| row.len() >= values.len() && row.iter().zip(&values).all(|(left, right)| left.strict_eq(right)));
    Ok(Scalar::Boolean(found))
}

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

fn has_one_filter(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("HASONEFILTER needs a column"))?)?;
    let filtered = ctx.filters.filters.get(&(table, column)).is_some_and(|values| values.len() == 1);
    Ok(Scalar::Boolean(filtered))
}

fn has_one_value(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("HASONEVALUE needs a column"))?)?;
    let filtered = ev.table(table, &ctx.filters);
    let distinct: BTreeSet<ScalarKey> = filtered.column(column).iter().map(ScalarKey::from).collect();
    Ok(Scalar::Boolean(distinct.len() == 1))
}

/// Compares the `Value1`/`Value2` groups of ISAFTER/ISONORAFTER.
///
/// The optional `Order` flag can follow every pair. The evaluator identifies it by the argument
/// itself (a Boolean, or the documented `0`/`1` shorthand), matching the DAX parameter type.
fn compare_groups(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context, strictly_after: bool) -> Result<Scalar, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::new("ISAFTER/ISONORAFTER need at least two values"));
    }
    let mut index = 0;
    loop {
        if index + 1 >= args.len() {
            return Err(EvalError::new("ISAFTER/ISONORAFTER need value pairs"));
        }
        let left = ev.eval(&args[index], ctx)?;
        let right = ev.eval(&args[index + 1], ctx)?;
        index += 2;
        let ascending = match args.get(index) {
            Some(argument) if is_order_flag(ev, argument, ctx)? => {
                index += 1;
                order_flag_value(ev, argument, ctx)?
            }
            _ => true,
        };
        let ordering = ordering_with_blanks(&left, &right)?;
        let ordering = if ascending { ordering } else { ordering.reverse() };
        if ordering == Ordering::Greater {
            return Ok(Scalar::Boolean(true));
        }
        if ordering == Ordering::Less {
            return Ok(Scalar::Boolean(false));
        }
        // The pairs are equal so far: the next group decides.
        if index >= args.len() {
            return Ok(Scalar::Boolean(!strictly_after));
        }
    }
}

fn is_order_flag(ev: &Evaluator<'_>, argument: &BoundExpr, ctx: &mut Context) -> Result<bool, EvalError> {
    match argument {
        BoundExpr::Literal(Literal::Boolean(_)) => Ok(true),
        BoundExpr::Keyword(word) => Ok(word.eq_ignore_ascii_case("ASC") || word.eq_ignore_ascii_case("DESC")),
        BoundExpr::Literal(Literal::Integer(value)) => Ok(*value == 0 || *value == 1),
        BoundExpr::Literal(Literal::Decimal(value)) => Ok(*value == 0.0 || *value == 1.0),
        other => Ok(matches!(ev.eval(other, ctx)?, Scalar::Boolean(_))),
    }
}

fn order_flag_value(ev: &Evaluator<'_>, argument: &BoundExpr, ctx: &mut Context) -> Result<bool, EvalError> {
    if let BoundExpr::Keyword(word) = argument {
        return Ok(!word.eq_ignore_ascii_case("DESC"));
    }
    Ok(ev.eval(argument, ctx)?.to_bool().unwrap_or(true))
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

fn is_filtered(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("ISFILTERED needs a column"))?)?;
    Ok(Scalar::Boolean(ctx.filters.filters.contains_key(&(table, column))))
}

fn is_cross_filtered(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let target = table_ref(ev, args.first().ok_or_else(|| EvalError::new("ISCROSSFILTERED needs a table or column"))?)?;
    let filtered: BTreeSet<usize> = ctx.filters.filters.keys().map(|(table, _)| *table).collect();
    if filtered.contains(&target) {
        return Ok(Scalar::Boolean(true));
    }
    // Mirror the model's filter propagation: a filter on the `to` table restricts the `from`
    // table, and a filter on the `from` table restricts the `to` table only when the
    // relationship is bidirectional. `from` is therefore cross-filtered by its related `to`
    // table, and vice versa.
    for relationship in &ev.model().relationships {
        if relationship.inactive {
            continue;
        }
        let from = ev.model().table_index(&relationship.from_table);
        let to = ev.model().table_index(&relationship.to_table);
        let related = if from == Some(target) {
            to
        } else if to == Some(target) && relationship.bidirectional {
            from
        } else {
            None
        };
        if related.is_some_and(|other| filtered.contains(&other)) {
            return Ok(Scalar::Boolean(true));
        }
    }
    Ok(Scalar::Boolean(false))
}

fn is_empty(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let table = ev.eval_table(args.first().ok_or_else(|| EvalError::new("ISEMPTY needs a table"))?, ctx)?;
    Ok(Scalar::Boolean(table.rows.is_empty()))
}

fn is_in_scope(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("ISINSCOPE needs a column"))?)?;
    // A grouping frame over the column's table puts the column in scope. Hierarchy levels are not
    // modelled, so every column of the current grouping frame is in scope.
    let in_scope = ctx.rows.iter().any(|frame| frame.lineage == Some(table) && frame.model_columns.contains(&Some(column)));
    Ok(Scalar::Boolean(in_scope))
}

fn is_subtotal(ev: &Evaluator<'_>, args: &[BoundExpr], _ctx: &mut Context) -> Result<Scalar, EvalError> {
    let (table, column) = column_ref(ev, args.first().ok_or_else(|| EvalError::new("ISSUBTOTAL needs a column"))?)?;
    match super::table_manipulation::subtotal_columns() {
        Some(rolled_up) => Ok(Scalar::Boolean(rolled_up.contains(&(table, column)))),
        None => Err(EvalError::new("ISSUBTOTAL requires a SUMMARIZE row context")),
    }
}

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

fn parse_escape(text: &str) -> Result<NameEscape, EvalError> {
    match text.trim().to_ascii_uppercase().as_str() {
        "ESCAPED" => Ok(NameEscape::Escaped),
        "UNESCAPED" => Ok(NameEscape::Unescaped),
        "MINIMALLYESCAPED" => Ok(NameEscape::Minimal),
        other => Err(EvalError::new(format!("NAMEOF does not know escape mode `{other}`"))),
    }
}

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

fn model_table_index(ev: &Evaluator<'_>, name: &str) -> Result<usize, EvalError> {
    ev.model().table_index(name).ok_or_else(|| EvalError::new(format!("NAMEOF: unknown table `{name}`")))
}

fn model_table_name(ev: &Evaluator<'_>, table: usize) -> Result<String, EvalError> {
    ev.model().tables.get(table).map(|model_table| model_table.name.clone()).ok_or_else(|| EvalError::new("NAMEOF table is out of range"))
}

fn escape_table(name: &str, escape: NameEscape) -> String {
    match escape {
        NameEscape::Unescaped => name.to_string(),
        NameEscape::Minimal if is_simple_name(name) => name.to_string(),
        NameEscape::Escaped | NameEscape::Minimal => format!("'{}'", name.replace('\'', "''")),
    }
}

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

fn is_simple_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else { return false };
    (first.is_alphabetic() || first == '_') && characters.all(|character| character.is_alphanumeric() || character == '_')
}

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
                cardinality.insert(ScalarKey::from(value));
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

#[derive(Clone, Copy, PartialEq)]
enum MatchMode {
    Text,
    Fuzzy,
    Phrase,
}

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

fn token_matches(token: &str, term: &str, mode: MatchMode) -> bool {
    match mode {
        MatchMode::Text | MatchMode::Phrase => token == term,
        MatchMode::Fuzzy => fuzzy_distance(token, term) <= fuzzy_threshold(term.chars().count()),
    }
}

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

fn phrase_matches(tokens: &[String], query: &[String]) -> bool {
    if query.is_empty() || query.len() > tokens.len() {
        return false;
    }
    tokens.windows(query.len()).any(|window| window.iter().zip(query).all(|(left, right)| left == right))
}

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

fn text_similarity(ev: &Evaluator<'_>, args: &[BoundExpr], ctx: &mut Context) -> Result<Scalar, EvalError> {
    let argument = args.first().ok_or_else(|| EvalError::new("TEXTSIMILARITY needs a column"))?;
    let within = ev.eval(argument, ctx)?.to_text();
    let search = ev.eval(args.get(1).ok_or_else(|| EvalError::new("TEXTSIMILARITY needs a search text"))?, ctx)?.to_text();
    let mode = match_mode(ev, args, ctx, 2)?;
    let corpus = column_corpus(ev, argument);
    // A model column can be large; when scoring every value against the query would exceed the
    // text work budget, fall back to the current row's own value so the call stays bounded.
    let work = corpus.iter().map(|value| value.chars().count()).sum::<usize>().saturating_mul(search.chars().count().max(1));
    let corpus = if corpus.is_empty() || work > MAX_TEXT_WORK { vec![within.clone()] } else { corpus };
    Ok(Scalar::Decimal(similarity_score(&corpus, &within, &search, mode)))
}

/// The indexed corpus: every value of the model column, independent of the filter context.
fn column_corpus(ev: &Evaluator<'_>, expr: &BoundExpr) -> Vec<String> {
    let Ok((table, column)) = column_ref(ev, expr) else { return Vec::new() };
    let Some(model_table) = ev.model().tables.get(table) else { return Vec::new() };
    model_table.rows.iter().filter_map(|row| row.get(column)).map(|value| value.to_text()).collect()
}

/// BM25 relevance over the column's values (k1 = 1.2, b = 0.75). The score is an opaque ranking
/// value, so only its sign and relative order are meaningful.
fn similarity_score(corpus: &[String], text: &str, search: &str, mode: MatchMode) -> f64 {
    let documents: Vec<Vec<String>> =
        if corpus.is_empty() { vec![tokenize(text)] } else { corpus.iter().map(|document| tokenize(document)).collect() };
    let target = tokenize(text);
    let query = tokenize(search);
    if query.is_empty() {
        return 0.0;
    }
    if mode == MatchMode::Phrase && !phrase_matches(&target, &query) {
        return 0.0;
    }
    let document_count = documents.len().max(1) as f64;
    let total_length: usize = documents.iter().map(Vec::len).sum();
    let average_length = (total_length as f64 / documents.len().max(1) as f64).max(1.0);
    let document_length = target.len() as f64;
    let (k1, b) = (1.2, 0.75);
    let mut score = 0.0;
    for term in &query {
        let term_frequency = target.iter().filter(|token| token_matches(token, term, mode)).count();
        if term_frequency == 0 {
            continue;
        }
        let matching_documents = documents.iter().filter(|document| document.iter().any(|token| token_matches(token, term, mode))).count() as f64;
        let inverse_frequency = (1.0 + (document_count - matching_documents + 0.5) / (matching_documents + 0.5)).ln();
        let denominator = term_frequency as f64 + k1 * (1.0 - b + b * document_length / average_length);
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

/// `USERPRINCIPALNAME`: Excel's embedded engine reports `DOMAIN\user`; on a host without a
/// domain the plain user name is the best available identity.
fn host_principal() -> String {
    let user = host_user();
    match (std::env::var("USERDOMAIN"), user.as_str()) {
        (Ok(domain), user) if !domain.is_empty() && !user.is_empty() => format!("{domain}\\{user}"),
        (_, user) => user.to_string(),
    }
}
