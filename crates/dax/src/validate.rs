//! Static validation of aggregation-function calls.
//!
//! The parser accepts any `NAME ( args )`; this module applies the catalog in
//! [`crate::functions`] to reject what can be decided without a data model:
//!
//! - argument counts against the documented arities (including `COUNTROWS ()` and the optional
//!   `Variant` of `MAXX`/`MINX`);
//! - column-only parameters that are not a bracketed name reference (`SUM ( 1 )`);
//! - table parameters that are an obvious scalar literal or scalar operator expression
//!   (`COUNTROWS ( 1 )`). Dynamic table expressions such as `FILTER ( ... )`, `DISTINCT ( ... )`,
//!   variables and table constructors are accepted.
//!
//! Unknown functions are ignored, so the validator can run over expressions that use the rest of
//! the DAX language.

use std::fmt;

use crate::ast::{Call, Expr, ExprKind, NameStyle, Span};
use crate::functions::{FunctionSpec, Overload, ParamKind, lookup};

/// The category of a validation failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValidationErrorKind {
    /// Wrong number of arguments for every documented overload.
    Arity,
    /// The argument at the reported span cannot satisfy the parameter kind.
    ArgumentKind,
}

impl ValidationErrorKind {
    pub const fn description(self) -> &'static str {
        match self {
            ValidationErrorKind::Arity => "wrong number of arguments",
            ValidationErrorKind::ArgumentKind => "wrong argument kind",
        }
    }
}

/// One error found by [`validate_call`] or [`validate_expression`].
///
/// `function` is the catalog spelling, `span` is the offending argument for kind errors and the
/// whole call for arity errors. [`validate_call`] has no span for the call itself, so arity
/// errors it returns carry an empty span; [`validate_expression`] fills the expression span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub function: &'static str,
    pub kind: ValidationErrorKind,
    pub message: String,
    pub span: Span,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at bytes {}..{}", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for ValidationError {}

/// Validates one call against the aggregation catalog.
///
/// Returns every error the call contains, or an empty vector for a valid call and for a function
/// that is not in the catalog. Because [`Call`] does not carry its own span, arity errors use
/// [`Span::empty`]; use [`validate_expression`] when spans matter.
pub fn validate_call(call: &Call) -> Vec<ValidationError> {
    validate_call_at(call, Span::empty())
}

/// Walks the expression tree and reports every invalid call to a known aggregation function.
///
/// Unknown functions are ignored but their arguments are still visited, so a bad `SUM` nested in
/// `CALCULATE (...)` is reported. Results are in depth-first source order.
pub fn validate_expression(expr: &Expr) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    expr.walk(&mut |node| {
        if let ExprKind::Call(call) = &node.kind {
            errors.extend(validate_call_at(call, node.span));
        }
    });
    errors
}

fn validate_call_at(call: &Call, call_span: Span) -> Vec<ValidationError> {
    let Some(spec) = lookup(&call.name.text) else {
        return Vec::new();
    };
    let arity = call.args.len();
    if arity < spec.min_args || arity > spec.max_args {
        return vec![arity_error(spec, arity, call_span)];
    }
    let matching: Vec<&Overload> = spec.overloads.iter().filter(|overload| overload.arity() == arity).collect();
    if matching.is_empty() {
        return vec![arity_error(spec, arity, call_span)];
    }
    let mut errors = Vec::new();
    for (index, argument) in call.args.iter().enumerate() {
        if matching.iter().any(|overload| overload_accepts(overload, index, argument)) {
            continue;
        }
        let expected = expected_kinds(&matching, index);
        errors.push(ValidationError {
            function: spec.name,
            kind: ValidationErrorKind::ArgumentKind,
            message: format!("{} argument {} must be {expected}, found {}", spec.name, index + 1, describe_argument(argument)),
            span: argument.span,
        });
    }
    errors
}

fn arity_error(spec: &FunctionSpec, arity: usize, span: Span) -> ValidationError {
    let expected = if spec.min_args == spec.max_args { format!("{}", spec.min_args) } else { format!("{} to {}", spec.min_args, spec.max_args) };
    ValidationError {
        function: spec.name,
        kind: ValidationErrorKind::Arity,
        message: format!("{} expects {expected} argument(s), found {arity}", spec.name),
        span,
    }
}

fn overload_accepts(overload: &Overload, index: usize, argument: &Expr) -> bool {
    match overload.params.get(index) {
        Some(kind) => accepts(*kind, argument),
        None => true,
    }
}

fn expected_kinds(overloads: &[&Overload], index: usize) -> String {
    let mut expected: Vec<&'static str> = Vec::new();
    for overload in overloads {
        let Some(kind) = overload.params.get(index) else {
            continue;
        };
        let description = kind.description();
        if !expected.contains(&description) {
            expected.push(description);
        }
    }
    expected.join(" or ")
}

/// The only argument checks that are decidable without a model.
fn accepts(kind: ParamKind, argument: &Expr) -> bool {
    match kind {
        ParamKind::Column => is_column_reference(argument),
        ParamKind::Table => !is_obvious_scalar(argument) && !is_column_reference(argument),
        ParamKind::Expression | ParamKind::Scalar | ParamKind::Logical | ParamKind::Any => true,
    }
}

fn is_column_reference(expr: &Expr) -> bool {
    matches!(
        &expr.kind,
        ExprKind::Name(name_ref) if name_ref.name.style == NameStyle::Bracketed
    )
}

/// Literals and operator expressions are certainly scalars, never tables. Names, calls, table
/// constructors and row constructors may be dynamic table expressions, so they are not rejected.
fn is_obvious_scalar(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Literal(_) | ExprKind::Unary { .. } | ExprKind::Binary { .. })
}

fn describe_argument(expr: &Expr) -> &'static str {
    match &expr.kind {
        ExprKind::Literal(_) => "a literal",
        ExprKind::Name(_) => "a name reference",
        ExprKind::Call(_) => "a function call",
        ExprKind::Unary { .. } | ExprKind::Binary { .. } => "an operator expression",
        ExprKind::Table(_) => "a table constructor",
        ExprKind::Tuple(_) => "a row constructor",
        ExprKind::VarReturn { .. } => "a VAR/RETURN expression",
    }
}
