//! Name binding for DAX expressions and measures.
//!
//! The binder resolves table names, `Table[Column]` references, measures and expression-level
//! `VAR` names against a [`DataModel`]. It validates catalogued function calls with the same
//! arity/kind rules as [`crate::validate`], and builds the measure dependency graph.
//!
//! It is deliberately static: row context and filter context are evaluator concerns. A bracketed
//! column that is not qualified by a table is left as [`BoundExpr::UnqualifiedColumn`] so the
//! evaluator can resolve it from the current row context.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{BinaryOp, Literal, NameRef, UnaryOp};
use crate::model::DataModel;
use crate::{Call, Expr, ExprKind, FunctionSpec, lookup, parse_expression, validate_call};

/// A name-resolution error with the source span.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{message} at bytes {}..{}", span.start, span.end)]
pub struct BindError {
    pub message: String,
    pub span: crate::Span,
}

impl BindError {
    fn new(message: impl Into<String>, span: crate::Span) -> Self {
        BindError { message: message.into(), span }
    }
}

/// The bound form of an expression.
#[derive(Clone, Debug, PartialEq)]
pub enum BoundExpr {
    Literal(Literal),
    /// A resolved `Table[Column]` reference.
    Column {
        table: usize,
        column: usize,
    },
    /// `[Column]` without a table qualifier: resolved from the evaluator's row context.
    UnqualifiedColumn {
        name: String,
    },
    /// A model measure reference.
    Measure(usize),
    /// A table reference.
    Table(usize),
    /// A catalogued function call with bound arguments.
    Call {
        spec: &'static FunctionSpec,
        args: Vec<BoundExpr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<BoundExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<BoundExpr>,
        right: Box<BoundExpr>,
    },
    /// `{ value, ... }`.
    TableConstructor(Vec<BoundExpr>),
    /// `( value, ... )`.
    Tuple(Vec<BoundExpr>),
    /// A variable reference.
    ///
    /// `index` is the variable's absolute slot in the cumulative stack of every enclosing
    /// [`BoundExpr::VarReturn`] block — outer blocks first, each block appending its own
    /// declarations in order — not its position inside the innermost `vars` list. The evaluator
    /// pushes each block's values onto the same stack, so the index stays valid across nesting.
    Variable {
        index: usize,
        name: String,
    },
    /// A function-specific keyword argument (`DAY` in `DATEDIFF`, a type name in `DATATABLE`).
    Keyword(String),
    /// `VAR name = expr ... RETURN body`.
    VarReturn {
        vars: Vec<BoundVariable>,
        body: Box<BoundExpr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoundVariable {
    pub name: String,
    pub expr: BoundExpr,
}

/// A bound measure with its direct measure dependencies.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundMeasure {
    pub name: String,
    pub expr: BoundExpr,
    pub dependencies: BTreeSet<String>,
}

/// Binds expressions against one model.
pub struct Binder<'a> {
    model: &'a DataModel,
}

impl<'a> Binder<'a> {
    pub fn new(model: &'a DataModel) -> Self {
        Binder { model }
    }

    pub fn model(&self) -> &'a DataModel {
        self.model
    }

    /// Binds an expression with no variables in scope.
    pub fn bind_expression(&self, expr: &Expr) -> Result<BoundExpr, BindError> {
        self.bind_with_vars(expr, &mut Vec::new())
    }

    /// Parses and binds a measure formula. The model's measures must have been prepared with
    /// [`DataModel::parse_formulas`].
    pub fn bind_measure(&self, name: &str) -> Result<BoundMeasure, BindError> {
        let index = self.model.measure_index(name).ok_or_else(|| BindError::new(format!("unknown measure `{name}`"), crate::Span::empty()))?;
        let measure = &self.model.measures[index];
        if measure.expr.is_none() {
            return Err(BindError::new(
                format!("measure `{}` has no parsed formula; call DataModel::parse_formulas", measure.name),
                crate::Span::empty(),
            ));
        }
        let Some(formula) = measure.expr.as_ref() else {
            return Err(BindError::new(
                format!("measure `{}` has no parsed formula; call DataModel::parse_formulas", measure.name),
                crate::Span::empty(),
            ));
        };
        let expr = self.bind_expression(formula)?;
        let mut dependencies = BTreeSet::new();
        collect_measure_names(self.model, &expr, &mut dependencies);
        Ok(BoundMeasure { name: measure.name.clone(), expr, dependencies })
    }

    /// Builds the direct measure dependency graph and rejects cycles.
    pub fn dependency_graph(&self) -> Result<BTreeMap<String, BTreeSet<String>>, BindError> {
        let mut graph = BTreeMap::new();
        for measure in &self.model.measures {
            let bound = self.bind_measure(&measure.name)?;
            graph.insert(bound.name, bound.dependencies);
        }
        let mut state: BTreeMap<String, u8> = BTreeMap::new();
        for name in graph.keys() {
            self.visit(name, &graph, &mut state)?;
        }
        Ok(graph)
    }

    fn visit(&self, name: &str, graph: &BTreeMap<String, BTreeSet<String>>, state: &mut BTreeMap<String, u8>) -> Result<(), BindError> {
        // Iterative: a measure chain comes from the model JSON, so its length must not grow the
        // call stack (a long chain would otherwise abort the process instead of returning an error).
        if state.get(name).copied() == Some(2) {
            return Ok(());
        }
        let empty = BTreeSet::new();
        state.insert(name.to_string(), 1);
        let mut stack: Vec<(&str, std::collections::btree_set::Iter<'_, String>)> = vec![(name, graph.get(name).unwrap_or(&empty).iter())];
        while let Some((current, dependencies)) = stack.last_mut() {
            match dependencies.next() {
                Some(dependency) => match state.get(dependency.as_str()).copied() {
                    Some(2) => {}
                    Some(1) => {
                        return Err(BindError::new(format!("circular measure dependency involving `{dependency}`"), crate::Span::empty()));
                    }
                    _ => {
                        state.insert(dependency.clone(), 1);
                        stack.push((dependency.as_str(), graph.get(dependency).unwrap_or(&empty).iter()));
                    }
                },
                None => {
                    state.insert((*current).to_string(), 2);
                    stack.pop();
                }
            }
        }
        Ok(())
    }

    fn bind_with_vars(&self, expr: &Expr, vars: &mut Vec<String>) -> Result<BoundExpr, BindError> {
        match &expr.kind {
            ExprKind::Literal(literal) => Ok(BoundExpr::Literal(literal.clone())),
            ExprKind::Name(name) => self.bind_name(name, expr.span, vars),
            ExprKind::Call(call) => self.bind_call(call, expr.span, vars),
            ExprKind::Unary { op, expr: inner } => Ok(BoundExpr::Unary { op: *op, expr: Box::new(self.bind_with_vars(inner, vars)?) }),
            ExprKind::Binary { op, left, right } => Ok(BoundExpr::Binary {
                op: *op,
                left: Box::new(self.bind_with_vars(left, vars)?),
                right: Box::new(self.bind_with_vars(right, vars)?),
            }),
            ExprKind::Table(items) => {
                Ok(BoundExpr::TableConstructor(items.iter().map(|item| self.bind_with_vars(item, vars)).collect::<Result<_, _>>()?))
            }
            ExprKind::Tuple(items) => Ok(BoundExpr::Tuple(items.iter().map(|item| self.bind_with_vars(item, vars)).collect::<Result<_, _>>()?)),
            ExprKind::VarReturn { vars: declarations, body } => {
                let mut bound = Vec::with_capacity(declarations.len());
                let mut names = vars.clone();
                for declaration in declarations {
                    let value = self.bind_with_vars(&declaration.expr, &mut names)?;
                    names.push(declaration.name.text.clone());
                    bound.push(BoundVariable { name: declaration.name.text.clone(), expr: value });
                }
                let body = self.bind_with_vars(body, &mut names)?;
                Ok(BoundExpr::VarReturn { vars: bound, body: Box::new(body) })
            }
        }
    }

    fn bind_name(&self, name: &NameRef, span: crate::Span, vars: &[String]) -> Result<BoundExpr, BindError> {
        if let Some(table) = &name.table {
            let table_index = self.model.table_index(&table.text).ok_or_else(|| BindError::new(format!("unknown table `{}`", table.text), span))?;
            if let Some(column_index) = self.model.column_index(table_index, &name.name.text) {
                return Ok(BoundExpr::Column { table: table_index, column: column_index });
            }
            // `Sales[Total Sales]` names a measure homed on the qualifying table; DAX allows the
            // table qualifier on a measure reference and only rejects it for another table.
            if let Some(index) = self.model.measure_index(&name.name.text)
                && let Some(home) = self.model.measures[index].table.as_deref()
            {
                if self.model.table_index(home) == Some(table_index) {
                    return Ok(BoundExpr::Measure(index));
                }
                return Err(BindError::new(format!("measure `{}` belongs to table `{}`, not `{}`", name.name.text, home, table.text), span));
            }
            return Err(BindError::new(format!("unknown column `{}` in table `{}`", name.name.text, table.text), span));
        }

        // A bracketed name is always a model object reference (`[Measure]`, `[Column]`), never a
        // variable: DAX requires bare identifiers for variables, so `VAR Total = 1 ... [Total]`
        // refers to the measure, not the variable.
        if name.name.style == crate::NameStyle::Bracketed {
            if let Some(index) = self.model.measure_index(&name.name.text) {
                return Ok(BoundExpr::Measure(index));
            }
            // A bracketed name may be a row-context column; the evaluator resolves it.
            return Ok(BoundExpr::UnqualifiedColumn { name: name.name.text.clone() });
        }

        if let Some(index) = vars.iter().rposition(|var| var.eq_ignore_ascii_case(&name.name.text)) {
            return Ok(BoundExpr::Variable { index, name: name.name.text.clone() });
        }

        if let Some(index) = self.model.table_index(&name.name.text) {
            return Ok(BoundExpr::Table(index));
        }
        if let Some(index) = self.model.measure_index(&name.name.text) {
            return Ok(BoundExpr::Measure(index));
        }
        Err(BindError::new(format!("unknown name `{}`", name.name.text), span))
    }

    fn bind_call(&self, call: &Call, span: crate::Span, vars: &mut Vec<String>) -> Result<BoundExpr, BindError> {
        let Some(spec) = lookup(&call.name.text) else {
            return Err(BindError::new(format!("unknown DAX function `{}`", call.name.text), span));
        };
        if let Some(error) = validate_call(call).into_iter().next() {
            return Err(BindError::new(error.message, span));
        }
        let mut args = Vec::with_capacity(call.args.len());
        for (index, arg) in call.args.iter().enumerate() {
            // Documented enum parameters (intervals, ties, blanks, text type names) are written
            // as bare names. The parser cannot tell them from table/measure names, so each
            // documented slot is matched by function and position below; anything else goes
            // through normal name resolution.
            let keyword = match spec.name {
                // DATATABLE's type words (`DATATABLE ( "Id", INTEGER, ... )`) and CONVERT's data
                // type (`CONVERT ( "123", INTEGER )`) are documented enumerations the catalog
                // types as `Text`/`Any`.
                "DATATABLE" if index % 2 == 1 => keyword_name(arg),
                "CONVERT" if index == 1 => keyword_name(arg),
                // The interval and calendar-flag slots are documented enumerations the catalog
                // types as `Text`: `DATEADD` takes an interval plus optional extension and
                // truncation flags, `DATESINPERIOD` an interval plus an end behaviour,
                // `PARALLELPERIOD` an interval.
                "DATEADD" if index == 2 => clause_keyword(arg, &["DAY", "MONTH", "QUARTER", "YEAR"]),
                "DATEADD" if index == 3 || index == 4 => clause_keyword(arg, &["EXTENDING", "ANCHORED", "PRECISE"]),
                "DATESINPERIOD" if index == 3 => clause_keyword(arg, &["DAY", "WEEK", "MONTH", "QUARTER", "YEAR"]),
                "DATESINPERIOD" if index == 4 => clause_keyword(arg, &["PRECISE", "ENDALIGNED"]),
                "PARALLELPERIOD" if index == 2 => clause_keyword(arg, &["MONTH", "QUARTER", "YEAR"]),
                // RANKX's Ties argument is documented as `Dense`/`Skip` but catalogued as a
                // Scalar; only the documented words bind as keywords so a typo stays an
                // unknown name instead of reaching the evaluator as a keyword.
                "RANKX" if index == 4 => clause_keyword(arg, &["SKIP", "DENSE"]),
                // RANKX's Order argument is catalogued as a Scalar too, but it is `ASC`/`DESC`.
                "RANKX" if index == 3 => clause_keyword(arg, &["ASC", "DESC"]),
                // DATEDIFF's interval is a documented enumeration the catalog types as `Any`.
                "DATEDIFF" if index == 2 => clause_keyword(arg, &["SECOND", "MINUTE", "HOUR", "DAY", "WEEK", "MONTH", "QUARTER", "YEAR"]),
                // TOPN's `(expression, Order)` pairs: ASC/DESC sit at every odd index from 3.
                "TOPN" if index >= 3 && index % 2 == 1 => clause_keyword(arg, &["ASC", "DESC"]),
                "CROSSFILTER" if index == 2 => {
                    clause_keyword(arg, &["NONE", "ONE", "ONEWAY", "BOTH", "TWOWAY", "ONEWAY_LEFTFILTERSRIGHT", "ONEWAY_RIGHTFILTERSLEFT"])
                }
                // RANK/ROWNUMBER's Reset argument is `NONE`, `LOWESTPARENT` or `HIGHESTPARENT`,
                // catalogued as `Any` so that a relation expression still binds there.
                // RANK's Ties and Blanks arguments are documented enumerations the catalog types
                // as `Text`: `RANK ( DENSE, ... )`, `RANK ( ..., DEFAULT, ... )`.
                "RANK" if index == 0 => clause_keyword(arg, &["DENSE", "SKIP"]),
                "RANK" if index == 3 => clause_keyword(arg, &["DEFAULT", "FIRST", "LAST"]),
                // ROWNUMBER's Blanks argument takes the same three ordering words.
                "ROWNUMBER" if index == 2 => clause_keyword(arg, &["DEFAULT", "FIRST", "LAST"]),
                // RANK/ROWNUMBER's Reset argument is `NONE`, `LOWESTPARENT` or `HIGHESTPARENT`,
                // catalogued as `Any` so that a relation expression still binds there.
                "RANK" if index == 6 => clause_keyword(arg, &["NONE", "LOWESTPARENT", "HIGHESTPARENT"]),
                // The documented bare ASC/DESC slots: RANKX and RANK.EQ take Order at index 3/2,
                // TOPNSKIP/CONCATENATEX at index 4 and SAMPLE at index 3.
                "RANK.EQ" if index == 2 => clause_keyword(arg, &["ASC", "DESC"]),
                // SAMPLE and TOPNSKIP repeat `(orderBy, Order)` pairs, so every order word sits at
                // an even offset from the first one: 3, 5, 7, ... for SAMPLE and 4, 6, 8, ... for
                // TOPNSKIP.
                "TOPNSKIP" if index >= 4 && (index - 4) % 2 == 0 => clause_keyword(arg, &["ASC", "DESC"]),
                "SAMPLE" if index >= 3 && (index - 3) % 2 == 0 => clause_keyword(arg, &["ASC", "DESC"]),
                // CONCATENATEX takes `(orderBy, Order)` pairs from index 4 on, so every even
                // offset from 4 is an order word.
                "CONCATENATEX" if index >= 4 && (index - 4) % 2 == 0 => clause_keyword(arg, &["ASC", "DESC"]),
                "ROWNUMBER" if index == 5 => clause_keyword(arg, &["NONE", "LOWESTPARENT", "HIGHESTPARENT"]),
                // Window/relation clauses take bare enum words: `ORDERBY(x, DESC)`,
                // `WINDOW(1, ABS, 1, ABS, ...)`, `INDEX(..., KEEP, PARTITIONBY(...))`, and the
                // LOOKUP associated-columns behaviour (`INFERRED`).
                "ORDERBY" if index % 2 == 1 => clause_keyword(arg, &["ASC", "DESC"]),
                "INDEX" | "OFFSET" if index >= 2 => clause_keyword(arg, WINDOW_KEYWORDS),
                // FromType/ToType are optional, so the blanks/reset slots move with the layout;
                // `window_table` identifies each slot by its value.
                "WINDOW" if index >= 1 => clause_keyword(arg, WINDOW_KEYWORDS),
                "LOOKUP" | "LOOKUPWITHTOTALS" if index % 2 == 1 && index + 1 == call.args.len() => {
                    clause_keyword(arg, &["EXPLICIT", "INFERRED", "DEFAULT"])
                }
                // PATHITEM's optional Type is documented as `TEXT`/`INTEGER` but catalogued as
                // `Any` so that the numeric `0`/`1` spellings still bind.
                "PATHITEM" | "PATHITEMREVERSE" if index == 2 => clause_keyword(arg, &["TEXT", "INTEGER"]),
                // Every other Text position (LEFT's text, FORMAT's pattern, INFO restriction
                // names) takes a value, not an enumeration: a bare word there resolves as a
                // name, so an undefined word reports "unknown name" instead of silently
                // becoming text. Table, measure and variable names still bind normally.
                _ => visual_keyword_name(spec.name, index, arg),
            };
            if let Some(keyword) = keyword {
                args.push(BoundExpr::Keyword(keyword));
            } else {
                args.push(self.bind_with_vars(arg, vars)?);
            }
        }
        Ok(BoundExpr::Call { spec, args })
    }
}

fn keyword_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Name(name) if name.table.is_none() && name.name.style == crate::NameStyle::Bare => Some(name.name.text.clone()),
        _ => None,
    }
}

/// Bare identifiers that visual-calculation functions accept where their syntax documents an
/// axis, an ORDERBY flag, a blank-ordering enumeration or a reset clause. These words are not
/// names in the model, so they must become [`BoundExpr::Keyword`] instead of failing resolution.
fn visual_keyword_name(function: &str, index: usize, arg: &Expr) -> Option<String> {
    let word = keyword_name(arg)?;
    is_visual_keyword(function, index, &word).then_some(word)
}

/// Whether `word` is a documented keyword at the 0-based `index` of a visual-calculation call.
///
/// The positions mirror the Microsoft syntax for each function, e.g. `FIRST ( <column>[, <axis>]
/// [, <orderBy>][, <blanks>][, reset] )` and `RANGE ( <step>[, <includeCurrent>][, <axis>]
/// [, <orderBy>][, <blanks>][, <reset>] )`. `COLLAPSE`/`EXPAND` also have navigation-only forms
/// whose first argument is the axis (`COLLAPSE ( <axis>[, N] )`).
fn is_visual_keyword(function: &str, index: usize, word: &str) -> bool {
    let axis = |positions: &[usize]| positions.contains(&index) && matches!(word.to_ascii_uppercase().as_str(), "ROWS" | "COLUMNS");
    let order_by = |positions: &[usize]| positions.contains(&index) && word.eq_ignore_ascii_case("KEEP");
    let blanks = |positions: &[usize]| positions.contains(&index) && matches!(word.to_ascii_uppercase().as_str(), "DEFAULT" | "FIRST" | "LAST");
    let reset =
        |positions: &[usize]| positions.contains(&index) && matches!(word.to_ascii_uppercase().as_str(), "NONE" | "LOWESTPARENT" | "HIGHESTPARENT");
    match function {
        "COLLAPSE" | "EXPAND" | "COLLAPSEALL" | "EXPANDALL" => axis(&[0, 1]),
        "FIRST" | "LAST" => axis(&[1]) || order_by(&[2]) || blanks(&[3]) || reset(&[4]),
        "PREVIOUS" | "NEXT" => axis(&[2]) || order_by(&[3]) || blanks(&[4]) || reset(&[5]),
        "MOVINGAVERAGE" => axis(&[3]) || order_by(&[4]) || blanks(&[5]) || reset(&[6]),
        "RUNNINGSUM" => axis(&[1]) || order_by(&[2]) || blanks(&[3]) || reset(&[4]),
        "RANGE" => axis(&[2]) || order_by(&[3]) || blanks(&[4]) || reset(&[5]),
        _ => false,
    }
}

/// The enum words accepted by the window/relation clauses. Matching is case-insensitive, and an
/// identifier outside this set is left to normal name binding so typos still report an error.
const WINDOW_KEYWORDS: &[&str] = &["ASC", "DESC", "ABS", "REL", "DEFAULT", "FIRST", "LAST", "KEEP", "NONE", "LOWESTPARENT", "HIGHESTPARENT"];

fn clause_keyword(expr: &Expr, allowed: &[&str]) -> Option<String> {
    let word = keyword_name(expr)?;
    allowed.iter().any(|candidate| word.eq_ignore_ascii_case(candidate)).then_some(word)
}

fn collect_measure_names(model: &DataModel, expr: &BoundExpr, out: &mut BTreeSet<String>) {
    match expr {
        BoundExpr::Measure(index) => {
            if let Some(measure) = model.measures.get(*index) {
                out.insert(measure.name.clone());
            }
        }
        BoundExpr::Call { args, .. } | BoundExpr::TableConstructor(args) | BoundExpr::Tuple(args) => {
            for arg in args {
                collect_measure_names(model, arg, out);
            }
        }
        BoundExpr::Unary { expr, .. } => collect_measure_names(model, expr, out),
        BoundExpr::Binary { left, right, .. } => {
            collect_measure_names(model, left, out);
            collect_measure_names(model, right, out);
        }
        BoundExpr::VarReturn { vars, body } => {
            for var in vars {
                collect_measure_names(model, &var.expr, out);
            }
            collect_measure_names(model, body, out);
        }
        BoundExpr::Literal(_)
        | BoundExpr::Column { .. }
        | BoundExpr::UnqualifiedColumn { .. }
        | BoundExpr::Table(_)
        | BoundExpr::Variable { .. }
        | BoundExpr::Keyword(_) => {}
    }
}

/// Parses and binds one measure formula against a model, without requiring a full model parse.
pub fn bind_formula(model: &DataModel, formula: &str) -> Result<BoundExpr, BindError> {
    let expr = parse_expression(formula).map_err(|error| BindError::new(error.message, error.span))?;
    Binder::new(model).bind_expression(&expr)
}
