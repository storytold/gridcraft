//! Logical functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category holds three kinds of functions:
//!
//! - Boolean combinators (`AND`, `OR`, `NOT`) and the constants `TRUE`/`FALSE`, all of which
//!   return a Boolean;
//! - bitwise integer operations (`BITAND`, `BITOR`, `BITXOR`), which are engine-only: dax.guide
//!   does not list them for any Excel version;
//! - scalar selectors (`IF`, `IF.EAGER`, `IFERROR`, `SWITCH`, `COALESCE`), which return a value
//!   whose type follows the selected branch and are therefore catalogued as
//!   [`ReturnType::Any`].

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, variadic};

/// Available to every host, with the usual DirectQuery restrictions.
const STANDARD: FunctionFlags = FunctionFlags::new(false, false, false, true, true);

/// Engine-only functions: no Excel version listed by dax.guide supports them.
const ENGINE_ONLY: FunctionFlags = FunctionFlags::new(false, false, false, false, true);

/// Functions dax.guide flags as not recommended (`IFERROR`) or that are documented as an
/// optimisation-only escape hatch (`IF.EAGER`).
const DISCOURAGED: FunctionFlags = FunctionFlags::new(false, false, true, true, true);

/// `IF.EAGER` is engine-only and discouraged except for specific optimisations.
const EAGER: FunctionFlags = FunctionFlags::new(false, false, true, false, true);

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "AND",
        FunctionCategory::Logical,
        2,
        2,
        &[required("Logical1", ParamKind::Logical), required("Logical2", ParamKind::Logical)],
        &[overload(&[ParamKind::Logical, ParamKind::Logical])],
        ReturnType::Boolean,
        STANDARD,
        "Returns TRUE when both logical arguments are TRUE, and FALSE otherwise. The function \
         accepts exactly two arguments; use the `&&` operator to combine three or more conditions \
         without nesting. Like the operator, it does not short-circuit when evaluated eagerly.",
        &["AND ( Sales[Amount] > 0, Sales[Quantity] > 0 )", "AND ( TRUE, FALSE )"],
    ),
    spec(
        "BITAND",
        FunctionCategory::Logical,
        2,
        2,
        &[required("Number1", ParamKind::Number), required("Number2", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        ENGINE_ONLY,
        "Returns the bitwise AND of two integers. Both arguments are converted to integers; the \
         result is a signed 64-bit integer.",
        &["BITAND ( 5, 3 )", "BITAND ( 13, 25 )"],
    ),
    spec(
        "BITOR",
        FunctionCategory::Logical,
        2,
        2,
        &[required("Number1", ParamKind::Number), required("Number2", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        ENGINE_ONLY,
        "Returns the bitwise OR of two integers. Both arguments are converted to integers; the \
         result is a signed 64-bit integer.",
        &["BITOR ( 5, 3 )", "BITOR ( 13, 25 )"],
    ),
    spec(
        "BITXOR",
        FunctionCategory::Logical,
        2,
        2,
        &[required("Number1", ParamKind::Number), required("Number2", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        ENGINE_ONLY,
        "Returns the bitwise XOR of two integers. Both arguments are converted to integers; the \
         result is a signed 64-bit integer.",
        &["BITXOR ( 5, 3 )", "BITXOR ( 13, 25 )"],
    ),
    spec(
        "COALESCE",
        FunctionCategory::Logical,
        2,
        usize::MAX,
        &[required("Value1", ParamKind::Any), required("Value2", ParamKind::Any)],
        &[variadic(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        ENGINE_ONLY,
        "Returns the first argument that does not evaluate to BLANK; when every argument is \
         blank the result is BLANK. At least two arguments are required, and later arguments are \
         not evaluated once a non-blank value is found. Unlike a chain of IF/ISBLANK checks, the \
         result type follows the first non-blank argument.",
        &[
            "COALESCE ( Sales[Discount], 0 )",
            "COALESCE ( Store[Area Manager], \"** Not Assigned **\" )",
            "COALESCE ( [Sales Amount], [Budget Amount], 0 )",
        ],
    ),
    spec(
        "FALSE",
        FunctionCategory::Logical,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Boolean,
        STANDARD,
        "Returns the logical value FALSE. `FALSE` without parentheses is the same constant; FALSE \
         is converted to 0 when a numeric result is required.",
        &["FALSE ()", "IF ( Sales[Amount] > 0, TRUE (), FALSE () )"],
    ),
    spec(
        "IF",
        FunctionCategory::Logical,
        2,
        3,
        &[required("LogicalTest", ParamKind::Logical), required("ResultIfTrue", ParamKind::Any), optional("ResultIfFalse", ParamKind::Any)],
        &[overload(&[ParamKind::Logical, ParamKind::Any]), overload(&[ParamKind::Logical, ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        STANDARD,
        "Evaluates the logical test and returns `ResultIfTrue` when it is TRUE and \
         `ResultIfFalse` otherwise. When the third argument is omitted a FALSE test returns \
         BLANK. Only the selected branch is evaluated in the regular (strict) execution plan, and \
         the result type follows the selected branch or the common type of both branches.",
        &["IF ( Sales[Amount] > 0, 1, 0 )", "IF ( [Sales Amount] > 1000, \"Large\", \"Small\" )", "IF ( Sales[Quantity] > 0, Sales[Amount] )"],
    ),
    spec(
        "IF.EAGER",
        FunctionCategory::Logical,
        2,
        3,
        &[required("LogicalTest", ParamKind::Logical), required("ResultIfTrue", ParamKind::Any), optional("ResultIfFalse", ParamKind::Any)],
        &[overload(&[ParamKind::Logical, ParamKind::Any]), overload(&[ParamKind::Logical, ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        EAGER,
        "Same conditional semantics as IF, but forces an eager execution plan in which both \
         branch expressions are evaluated before the test chooses one. It exists only for query \
         optimisation and should be used only after measuring a clear advantage over IF; the \
         result type is Variant when the branches differ.",
        &["IF.EAGER ( Sales[Amount] > 0, 1, 0 )", "IF.EAGER ( [Sales Amount] > 1000, \"Large\", \"Small\" )"],
    ),
    spec(
        "IFERROR",
        FunctionCategory::Logical,
        2,
        2,
        &[required("Value", ParamKind::Any), required("ValueIfError", ParamKind::Any)],
        &[overload(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        DISCOURAGED,
        "Returns `ValueIfError` when the first argument raises an error and the value of the \
         first argument otherwise; it is the faster equivalent of \
         `IF ( ISERROR ( Value ), ValueIfError, Value )`. Neither IFERROR nor ISERROR catches \
         every runtime error, because block-mode execution can bypass the check, so both are \
         discouraged.",
        &["IFERROR ( 1 / 0, 0 )", "IFERROR ( VALUE ( Sales[Text] ), BLANK () )"],
    ),
    spec(
        "NOT",
        FunctionCategory::Logical,
        1,
        1,
        &[required("Logical", ParamKind::Logical)],
        &[overload(&[ParamKind::Logical])],
        ReturnType::Boolean,
        STANDARD,
        "Returns the opposite Boolean value: TRUE for a FALSE argument and FALSE for a TRUE \
         argument. NOT is an operator exposed through function syntax.",
        &["NOT ( Sales[Amount] > 0 )", "NOT ( TRUE )"],
    ),
    spec(
        "OR",
        FunctionCategory::Logical,
        2,
        2,
        &[required("Logical1", ParamKind::Logical), required("Logical2", ParamKind::Logical)],
        &[overload(&[ParamKind::Logical, ParamKind::Logical])],
        ReturnType::Boolean,
        STANDARD,
        "Returns TRUE when either logical argument is TRUE, and FALSE when both are FALSE. The \
         function accepts exactly two arguments; use the `||` operator to combine three or more \
         conditions without nesting.",
        &["OR ( Sales[Amount] > 0, Sales[Quantity] > 0 )", "OR ( TRUE, FALSE )"],
    ),
    spec(
        "SWITCH",
        FunctionCategory::Logical,
        3,
        usize::MAX,
        &[
            required("Expression", ParamKind::Any),
            required("Value", ParamKind::Any),
            required("Result", ParamKind::Any),
            optional("Else", ParamKind::Any),
        ],
        &[variadic(&[ParamKind::Any, ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        STANDARD,
        "Compares the expression with each value in turn and returns the result paired with the \
         first match; when nothing matches it returns the optional else argument, or BLANK when \
         that argument is omitted. Values and results come in pairs, so the call needs an odd \
         number of arguments when there is no else and an even number when there is one. All \
         results and the else value must have the same data type. Using TRUE as the expression \
         turns SWITCH into a cleaner alternative to cascading IF calls.",
        &[
            "SWITCH ( [X], 1, \"a\", 2, \"b\", \"other\" )",
            "SWITCH ( Sales[Region], \"North\", 1, \"South\", 2, 0 )",
            "SWITCH ( TRUE (), Sales[Amount] > 1000, \"Large\", Sales[Amount] > 100, \"Medium\", \"Small\" )",
        ],
    ),
    spec(
        "TRUE",
        FunctionCategory::Logical,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Boolean,
        STANDARD,
        "Returns the logical value TRUE. `TRUE` without parentheses is the same constant; TRUE is \
         converted to 1 when a numeric result is required.",
        &["TRUE ()", "IF ( Sales[Amount] > 0, TRUE (), FALSE () )"],
    ),
];
