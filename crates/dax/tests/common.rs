#![allow(dead_code)]
//! Shared helpers for the per-category catalog integration tests.
//!
//! The `functions_*` suites all drive the same flow — parse a DAX snippet,
//! look a function up in the catalog, and assert validation outcomes — so the
//! helpers live here once instead of being copied into every suite.

use gridcraft_dax::functions::{FunctionSpec, ParamKind, lookup};
use gridcraft_dax::{Expr, ExprKind, ValidationError, ValidationErrorKind, parse_expression, validate_call, validate_expression};

/// Parses `src`, panicking with the parse error when it is not valid DAX.
pub fn parse(src: &str) -> Expr {
    match parse_expression(src) {
        Ok(expr) => expr,
        Err(error) => panic!("{src:?} did not parse: {error}"),
    }
}

/// Looks `name` up in the catalog, panicking when it is missing.
pub fn spec(name: &str) -> &'static FunctionSpec {
    match lookup(name) {
        Some(spec) => spec,
        None => panic!("{name} is missing from the catalog"),
    }
}

/// Validation errors for a whole expression.
pub fn expression_errors(src: &str) -> Vec<ValidationError> {
    validate_expression(&parse(src))
}

/// Validation errors for the call at the root of `src`.
pub fn call_errors(src: &str) -> Vec<ValidationError> {
    let expr = parse(src);
    match &expr.kind {
        ExprKind::Call(call) => validate_call(call),
        other => panic!("{src:?} is not a call: {other:?}"),
    }
}

/// Asserts `src` validates cleanly.
pub fn assert_valid(src: &str) {
    let errors = expression_errors(src);
    assert!(errors.is_empty(), "{src:?} should validate, got {errors:?}");
}

/// Asserts `src` is rejected with `kind`, returning all errors for further checks.
pub fn assert_invalid_kind(src: &str, kind: ValidationErrorKind) -> Vec<ValidationError> {
    let errors = expression_errors(src);
    assert!(!errors.is_empty(), "{src:?} should be rejected");
    assert!(errors.iter().any(|error| error.kind == kind), "{src:?} should report {kind:?}, got {errors:?}");
    errors
}

/// The overload accepting exactly `arity` arguments, if any.
pub fn overload_kinds(spec: &FunctionSpec, arity: usize) -> Option<Vec<ParamKind>> {
    spec.overload_for(arity).map(|overload| overload.params.to_vec())
}
