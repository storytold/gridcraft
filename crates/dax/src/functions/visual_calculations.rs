//! Visual calculation functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! All functions in this category are Power BI visual calculation functions: they can only be
//! used in the visual calculation editor and are rejected in measures, calculated columns and
//! calculated tables. The `axis` parameter is defined by the VISUAL SHAPE query syntax emitted
//! by the visual calculation feature, so `ROWS` and `COLUMNS` appear in examples as bare names.

use super::{FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, variadic};

/// Power BI visual calculations are not available to Excel's embedded engine, and the
/// DirectQuery restrictions listed for measures and calculated columns do not apply because
/// the functions cannot be used there.
const VISUAL: FunctionFlags = FunctionFlags::new(false, false, false, false, false);

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "COLLAPSE",
        super::FunctionCategory::VisualCalculations,
        1,
        usize::MAX,
        &[
            required("Expression", ParamKind::Any),
            optional("Axis", ParamKind::Any),
            optional("Column", ParamKind::Column),
            optional("N", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Any]), variadic(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        VISUAL,
        "Retrieves a context with the detail levels above the current one removed, or returns its \
         first argument evaluated in that coarser context. Navigation-only calls (an axis or a \
         list of columns) can be used inside CALCULATE. Available only in Power BI visual \
         calculations; `N` defaults to one level.",
        &[
            "COLLAPSE ( [Sales Amount], ROWS )",
            "COLLAPSE ( [Sales Amount], ROWS, 2 )",
            "COLLAPSE ( [Sales Amount], [Month] )",
            "COLLAPSE ( ROWS, 1 )",
        ],
    ),
    spec(
        "COLLAPSEALL",
        super::FunctionCategory::VisualCalculations,
        1,
        2,
        &[optional("Expression", ParamKind::Any), required("Axis", ParamKind::Any)],
        &[overload(&[ParamKind::Any]), overload(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        VISUAL,
        "Removes every detail level along an axis, returning the expression evaluated at the \
         highest level of that axis. The inverse of EXPANDALL. Available only in Power BI visual \
         calculations.",
        &["COLLAPSEALL ( [Sales Amount], ROWS )", "COLLAPSEALL ( ROWS )", "COLLAPSEALL ( [Sales Amount], COLUMNS )"],
    ),
    spec(
        "EXPAND",
        super::FunctionCategory::VisualCalculations,
        1,
        usize::MAX,
        &[
            required("Expression", ParamKind::Any),
            optional("Axis", ParamKind::Any),
            optional("Column", ParamKind::Column),
            optional("N", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Any]), variadic(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        VISUAL,
        "Retrieves a context with additional detail levels compared to the current one, or \
         returns its first argument evaluated in that finer context. Navigation-only calls (an \
         axis or a list of columns) can be used inside CALCULATE. Available only in Power BI \
         visual calculations; `N` defaults to one level.",
        &["EXPAND ( [Sales Amount], ROWS )", "EXPAND ( [Sales Amount], ROWS, 2 )", "EXPAND ( [Sales Amount], [Month] )"],
    ),
    spec(
        "EXPANDALL",
        super::FunctionCategory::VisualCalculations,
        1,
        2,
        &[optional("Expression", ParamKind::Any), required("Axis", ParamKind::Any)],
        &[overload(&[ParamKind::Any]), overload(&[ParamKind::Any, ParamKind::Any])],
        ReturnType::Any,
        VISUAL,
        "Adds every detail level along an axis, returning the expression evaluated at the lowest \
         level of that axis. The inverse of COLLAPSEALL. Available only in Power BI visual \
         calculations.",
        &["EXPANDALL ( [Sales Amount], ROWS )", "EXPANDALL ( ROWS )", "EXPANDALL ( [Sales Amount], COLUMNS )"],
    ),
    spec(
        "FIRST",
        super::FunctionCategory::VisualCalculations,
        1,
        5,
        &[
            required("Column", ParamKind::Column),
            optional("Axis", ParamKind::Any),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Column]),
            overload(&[ParamKind::Column, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Any,
        VISUAL,
        "Returns the value of a column in the first row of an axis or partition. OrderBy columns \
         define the sort order; Blanks accepts DEFAULT, FIRST or LAST and Reset accepts NONE, \
         LOWESTPARENT, HIGHESTPARENT or a level number. Available only in Power BI visual \
         calculations.",
        &["FIRST ( [Sales Amount] )", "FIRST ( [Sales Amount], ROWS )", "FIRST ( [Sales Amount], ROWS, [Month], DEFAULT, NONE )"],
    ),
    spec(
        "LAST",
        super::FunctionCategory::VisualCalculations,
        1,
        5,
        &[
            required("Column", ParamKind::Column),
            optional("Axis", ParamKind::Any),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Column]),
            overload(&[ParamKind::Column, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Any,
        VISUAL,
        "Returns the value of a column in the last row of an axis or partition. OrderBy columns \
         define the sort order; Blanks accepts DEFAULT, FIRST or LAST and Reset accepts NONE, \
         LOWESTPARENT, HIGHESTPARENT or a level number. Available only in Power BI visual \
         calculations.",
        &["LAST ( [Sales Amount] )", "LAST ( [Sales Amount], ROWS )", "LAST ( [Sales Amount], ROWS, [Month], FIRST, LOWESTPARENT )"],
    ),
    spec(
        "MOVINGAVERAGE",
        super::FunctionCategory::VisualCalculations,
        2,
        7,
        &[
            required("Column", ParamKind::Column),
            required("WindowSize", ParamKind::Number),
            optional("IncludeCurrent", ParamKind::Boolean),
            optional("Axis", ParamKind::Any),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Column, ParamKind::Number]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Boolean]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Boolean, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Boolean, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Boolean, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Boolean, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Numeric,
        VISUAL,
        "Calculates the average of the values in a sliding window of WindowSize rows along an \
         axis. IncludeCurrent defaults to TRUE; when FALSE the window holds the previous \
         WindowSize rows. OrderBy, Blanks and Reset behave as in FIRST. Available only in Power \
         BI visual calculations.",
        &[
            "MOVINGAVERAGE ( [Sales Amount], 6 )",
            "MOVINGAVERAGE ( [Sales Amount], 3, FALSE )",
            "MOVINGAVERAGE ( [Sales Amount], 6, TRUE, ROWS )",
            "MOVINGAVERAGE ( [Sales Amount], 12, TRUE, ROWS, [Month], DEFAULT, NONE )",
        ],
    ),
    spec(
        "NEXT",
        super::FunctionCategory::VisualCalculations,
        1,
        6,
        &[
            required("Column", ParamKind::Column),
            optional("Steps", ParamKind::Number),
            optional("Axis", ParamKind::Any),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Column]),
            overload(&[ParamKind::Column, ParamKind::Number]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Any,
        VISUAL,
        "Returns the value of a column the given number of rows after the current row on an axis. \
         Steps defaults to 1 and must be at least 1. OrderBy, Blanks and Reset behave as in \
         FIRST. Available only in Power BI visual calculations.",
        &["NEXT ( [Sales Amount] )", "NEXT ( [Sales Amount], 1, ROWS )", "NEXT ( [Sales Amount], 2, ROWS, [Month], LAST, HIGHESTPARENT )"],
    ),
    spec(
        "PREVIOUS",
        super::FunctionCategory::VisualCalculations,
        1,
        6,
        &[
            required("Column", ParamKind::Column),
            optional("Steps", ParamKind::Number),
            optional("Axis", ParamKind::Any),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Column]),
            overload(&[ParamKind::Column, ParamKind::Number]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Number, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Any,
        VISUAL,
        "Returns the value of a column the given number of rows before the current row on an \
         axis. Steps defaults to 1 and is negated internally, so it must be at least 1. OrderBy, \
         Blanks and Reset behave as in FIRST. Available only in Power BI visual calculations.",
        &[
            "PREVIOUS ( [Sales Amount] )",
            "PREVIOUS ( [Sales Amount], 1, ROWS )",
            "PREVIOUS ( [Sales Amount], 2, ROWS, [Month], DEFAULT, LOWESTPARENT )",
        ],
    ),
    spec(
        "RANGE",
        super::FunctionCategory::VisualCalculations,
        1,
        6,
        &[
            required("Step", ParamKind::Number),
            optional("IncludeCurrent", ParamKind::Boolean),
            optional("Axis", ParamKind::Any),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Boolean]),
            overload(&[ParamKind::Number, ParamKind::Boolean, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Boolean, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Boolean, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Boolean, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Table,
        VISUAL,
        "Returns the interval of rows within an axis that starts Step rows before (negative Step) \
         or after (non-negative Step) the current row, optionally including the current row. The \
         result is normally passed to CALCULATE. Available only in Power BI visual calculations.",
        &["RANGE ( 6 )", "RANGE ( -5, TRUE, ROWS )", "RANGE ( 12, FALSE, ROWS, [Month], DEFAULT, NONE )"],
    ),
    spec(
        "RUNNINGSUM",
        super::FunctionCategory::VisualCalculations,
        1,
        5,
        &[
            required("Column", ParamKind::Column),
            optional("Axis", ParamKind::Any),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Column]),
            overload(&[ParamKind::Column, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Column, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Numeric,
        VISUAL,
        "Calculates the cumulative sum of a column from the start of the axis or partition up to \
         the current row. OrderBy, Blanks and Reset behave as in FIRST. Available only in Power \
         BI visual calculations.",
        &["RUNNINGSUM ( [Sales Amount] )", "RUNNINGSUM ( [Sales Amount], ROWS )", "RUNNINGSUM ( [Sales Amount], ROWS, [Month], DEFAULT, NONE )"],
    ),
];
