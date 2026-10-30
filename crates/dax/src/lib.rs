//! The DAX language: tokenizer, syntax tree, Pratt parser, canonical printer and static types.
//!
//! This crate covers the DAX expression grammar, the function catalog for every dax.guide category
//! with static call validation, the static type surface (literal classification, inference and the documented
//! operator result tables) and the DAX query statements: `DEFINE` (`MEASURE`, `COLUMN`, `TABLE`,
//! `VAR`, `FUNCTION`, `MPARAMETER`), `EVALUATE`, `ORDER BY`, `START AT`, expression-level
//! `VAR ... RETURN`, and `WITH VISUAL SHAPE` (`AXIS`, `GROUP`, `TOTAL`, `DENSIFY`). It also
//! includes a model evaluator with semantics for all 484 catalogued functions, a columnar model
//! store, a PivotTable renderer and a reader/writer for Power Pivot measures in
//! `xl/model/item.data`. The evaluator materializes the model once into the columnar store and
//! answers filtering, aggregates, `VALUES`/`DISTINCT`, relationship joins and pivot grouping from
//! it; models that cannot be stored losslessly (ragged rows, promoted column types) and functions
//! that need a row context still read the row-based `DataModel`.
//!
//! ```
//! use gridcraft_dax::{parse_expression, print_expression};
//!
//! let expr = parse_expression("1 + 2 * 3")?;
//! assert_eq!(print_expression(&expr), "1 + 2 * 3");
//! # Ok::<(), gridcraft_dax::ParseError>(())
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod ast;
pub mod binder;
pub mod eval;
pub mod functions;
pub mod integrity;
pub mod lexer;
pub mod model;
pub mod model_file;
pub mod parser;
pub mod pivot;
pub mod pivot_table;
pub mod printer;
pub mod storage;
pub mod types;
pub mod validate;
pub mod value;

pub use ast::{
    Axis, AxisGroup, AxisKind, AxisOrderItem, BinaryOp, Call, ColumnDef, DateTimeLiteral, Define, Definition, Evaluate, Expr, ExprKind, FunctionDef,
    FunctionParam, Literal, MParameterDef, MeasureDef, Name, NameRef, NameStyle, OrderItem, ParameterSubtype, ParameterType, PassingMode, Query,
    SortDirection, Span, StartValue, TableDef, UnaryOp, VarDecl, VarDef, VisualShape,
};
pub use binder::{BindError, Binder, BoundExpr, BoundMeasure, BoundVariable, bind_formula};
pub use eval::{Evaluator, FilterContext, StorageStats};
pub use functions::{BlankPolicy, FunctionCategory, FunctionFlags, FunctionSpec, Overload, ParamKind, Parameter, ReturnType, catalog, lookup};
pub use integrity::{
    Cardinality, IntegrityCheck, IntegrityFinding, IntegrityReport, MAX_REPORTED_CYCLES, MAX_REPORTED_KEYS, MAX_REPORTED_ROWS, RelationshipCheck,
    Severity, validate_model,
};
pub use lexer::{LexError, MAX_INPUT_LEN, MAX_QUERY_LEN, Token, TokenKind, tokenize};
pub use model::{DataModel, Measure, ModelColumn, ModelError, ModelTable, Relationship, parse_measure};
pub use model_file::{ModelFileError, ModelMeasure, read_measures, write_measures};
pub use parser::{MAX_DEPTH, ParseError, parse_expression, parse_query};
pub use pivot::{PivotError, PivotField, pivot, pivot_cross, pivot_fields};
pub use printer::{print_expression, print_query};
pub use storage::{ColumnFootprint, MIN_ENCODING_SAVING, PackedCodes, SCAN_BATCH_ROWS, StorageFootprint, TableFootprint, ValueRef};
pub use validate::{ValidationError, ValidationErrorKind, validate_call, validate_expression};
pub use value::{EvalError, Scalar, ScalarKey, TableColumn, TableValue};

#[cfg(test)]
mod tests;
