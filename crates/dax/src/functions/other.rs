//! Other functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category collects the bit-shift operators, the row-context helpers, the window ranking
//! functions, the metadata table function and the remaining special functions that dax.guide
//! lists under "Other". `INFO.STORAGETABLES` keeps the exact dotted spelling and shares the
//! optional restriction-pair syntax of the other `INFO.*` functions.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, variadic};

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "BITLSHIFT",
        FunctionCategory::Other,
        2,
        2,
        &[required("Number", ParamKind::Number), required("ShiftAmount", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the integer in the first argument shifted left by the number of bits in the \
         second argument; a negative shift amount shifts right. The result follows two's \
         complement integer semantics and can overflow or underflow.",
        &["BITLSHIFT ( 4, 2 )", "BITLSHIFT ( Sales[Quantity], 1 )"],
    ),
    spec(
        "BITRSHIFT",
        FunctionCategory::Other,
        2,
        2,
        &[required("Number", ParamKind::Number), required("ShiftAmount", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Integer,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the integer in the first argument shifted right by the number of bits in the \
         second argument; a negative shift amount shifts left. The result follows two's \
         complement integer semantics and can overflow or underflow.",
        &["BITRSHIFT ( 8, 2 )", "BITRSHIFT ( Sales[Quantity], 1 )"],
    ),
    spec(
        "BLANK",
        FunctionCategory::Other,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Blank,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the BLANK value. BLANK has no data type and is converted automatically when \
         compared with other values; use the `==` operator or ISBLANK to test for it.",
        &["BLANK ()"],
    ),
    spec(
        "EARLIER",
        FunctionCategory::Other,
        1,
        2,
        &[required("ColumnName", ParamKind::Column), optional("Number", ParamKind::Number)],
        &[overload(&[ParamKind::Column]), overload(&[ParamKind::Column, ParamKind::Number])],
        ReturnType::Any,
        FunctionFlags::new(false, false, true, true, true),
        "Returns the value of a column in an outer row context, `Number` evaluation passes back \
         (default 1). Fails when no outer row context exists. dax.guide marks it not recommended: \
         a variable captured before the inner row context is easier to read and maintain.",
        &["EARLIER ( Customer[Yearly Income] )", "EARLIER ( Customer[Yearly Income], 1 )"],
    ),
    spec(
        "EARLIEST",
        FunctionCategory::Other,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Any,
        FunctionFlags::new(false, false, true, true, true),
        "Returns the value of a column in the outermost row context, equivalent to EARLIER with \
         the largest possible scan count. dax.guide marks it not recommended in favour of storing \
         the value in a variable.",
        &["EARLIEST ( Customer[Yearly Income] )"],
    ),
    spec(
        "ERROR",
        FunctionCategory::Other,
        1,
        1,
        &[required("ErrorText", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::Variant,
        FunctionFlags::new(false, false, false, true, true),
        "Stops evaluation and raises a user-specified error with the supplied message. It can be \
         used anywhere a scalar is expected, but has no data type of its own, so wrap it in \
         CONVERT to give the surrounding expression a concrete type.",
        &["ERROR ( \"Missing inflation rate\" )"],
    ),
    spec(
        "EXTERNALMEASURE",
        FunctionCategory::Other,
        1,
        3,
        &[required("MeasureName", ParamKind::Text), optional("Type", ParamKind::Text), optional("Datasource", ParamKind::Text)],
        &[
            overload(&[ParamKind::Text]),
            overload(&[ParamKind::Text, ParamKind::Text]),
            overload(&[ParamKind::Text, ParamKind::Text, ParamKind::Text]),
        ],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, false, true),
        "Invokes a measure defined in a remote model. The optional type name \
         (BOOLEAN/LOGICAL, CURRENCY/DECIMAL, DATETIME, DOUBLE, INTEGER/INT64, STRING/TEXT, \
         VARIANT) declares the remote result type and the optional data source names the \
         external model. Power BI generates these calls for composite models.",
        &["EXTERNALMEASURE ( \"Remote Sales\", \"DOUBLE\", \"RemoteModel\" )"],
    ),
    spec(
        "HASH",
        FunctionCategory::Other,
        1,
        usize::MAX,
        &[required("Expression", ParamKind::Any)],
        &[variadic(&[ParamKind::Any])],
        ReturnType::Integer,
        FunctionFlags::new(false, false, false, false, false),
        "Computes a hash over a variable number of input expressions and returns an integer. The \
         function appears in the engine metadata but is not documented by Microsoft, so its \
         exact hash algorithm and stability across versions are unspecified.",
        &["HASH ( Sales[Amount] )", "HASH ( Sales[OrderNumber], Sales[LineNumber] )"],
    ),
    spec(
        "INFO.STORAGETABLES",
        FunctionCategory::Other,
        0,
        usize::MAX,
        &[optional("RestrictionName", ParamKind::Text), optional("RestrictionValue", ParamKind::Any)],
        &[overload(&[]), overload(&[ParamKind::Text]), variadic(&[ParamKind::Text, ParamKind::Any])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, false),
        "Returns statistics about the in-memory storage tables of the model, with one row per \
         table and columns such as DATABASE_NAME, TABLE_ID, TABLE_PARTITIONS_COUNT and \
         ROWS_COUNT. It corresponds to the DISCOVER_STORAGE_TABLES dynamic management view, \
         accepts optional restriction name/value pairs, and, like every INFO function, cannot be \
         used in calculated tables or calculated columns.",
        &["INFO.STORAGETABLES ()", "INFO.STORAGETABLES ( \"DATABASE_NAME\", \"Sales\" )"],
    ),
    spec(
        "ISATLEVEL",
        FunctionCategory::Other,
        1,
        1,
        &[required("Column", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, false, false),
        "Reports whether the given column is present at the current level of the visual \
         calculation data grid. Use it to apply different formatting or logic per hierarchy level.",
        &["ISATLEVEL ( 'Date'[Year] )", "ISATLEVEL ( Product[Category] )"],
    ),
    spec(
        "ISCURRENCY",
        FunctionCategory::Other,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, false, true),
        "Checks whether a value is a fixed-precision decimal number (currency) and returns TRUE \
         or FALSE. It is an alias of ISDECIMAL.",
        &["ISCURRENCY ( Sales[Amount] )", "ISCURRENCY ( 1.5 )"],
    ),
    spec(
        "ISINTEGER",
        FunctionCategory::Other,
        1,
        1,
        &[required("Value", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, false, true),
        "Checks whether a value is an integer and returns TRUE or FALSE. It is an alias of \
         ISINT64.",
        &["ISINTEGER ( Sales[Quantity] )", "ISINTEGER ( 2.5 )"],
    ),
    spec(
        "KEYWORDMATCH",
        FunctionCategory::Other,
        2,
        2,
        &[required("MatchExpression", ParamKind::Text), required("Text", ParamKind::Text)],
        &[overload(&[ParamKind::Text, ParamKind::Text])],
        ReturnType::Boolean,
        FunctionFlags::new(false, false, false, true, false),
        "Returns TRUE when the match expression matches the supplied text. dax.guide lists it as \
         deprecated: it was never released and only appeared in some Excel versions.",
        &["KEYWORDMATCH ( \"bike\", \"Mountain Bike\" )"],
    ),
    spec(
        "RANK",
        FunctionCategory::Other,
        0,
        7,
        &[
            optional("Ties", ParamKind::Text),
            optional("Relation", ParamKind::Table),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Text),
            optional("PartitionBy", ParamKind::Any),
            optional("MatchBy", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[]),
            overload(&[ParamKind::Text]),
            overload(&[ParamKind::Text, ParamKind::Table]),
            overload(&[ParamKind::Text, ParamKind::Table, ParamKind::Any]),
            overload(&[ParamKind::Text, ParamKind::Table, ParamKind::Any, ParamKind::Text]),
            overload(&[ParamKind::Text, ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any]),
            overload(&[ParamKind::Text, ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Text, ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Integer,
        FunctionFlags::new(false, false, false, false, true),
        "Returns the rank of the current row inside its partition, ordered by an ORDERBY clause. \
         The optional Ties argument selects DENSE or SKIP (SKIP is the default), Blanks controls \
         where blank ordering values sort, PARTITIONBY splits the table, MATCHBY identifies the \
         current row and Reset restarts the numbering. Relation defaults to ALLSELECTED of the \
         ordering and partitioning columns, and either Relation or ORDERBY must be supplied.",
        &["RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ) )", "RANK ( SKIP, Sales, ORDERBY ( Sales[Amount], DESC ) )"],
    ),
    spec(
        "ROWNUMBER",
        FunctionCategory::Other,
        0,
        6,
        &[
            optional("Relation", ParamKind::Table),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Text),
            optional("PartitionBy", ParamKind::Any),
            optional("MatchBy", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[]),
            overload(&[ParamKind::Table]),
            overload(&[ParamKind::Table, ParamKind::Any]),
            overload(&[ParamKind::Table, ParamKind::Any, ParamKind::Text]),
            overload(&[ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any]),
            overload(&[ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Table, ParamKind::Any, ParamKind::Text, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Integer,
        FunctionFlags::new(false, false, false, false, true),
        "Returns the unique row number of the current row inside its partition, ordered by an \
         ORDERBY clause. Unlike RANK, ties receive different numbers. The optional Blanks, \
         PARTITIONBY, MATCHBY and Reset arguments behave as they do for RANK; either Relation or \
         ORDERBY must be supplied.",
        &["ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ) )", "ROWNUMBER ( Sales, ORDERBY ( Sales[OrderDate] ), LAST )"],
    ),
    spec(
        "SAMPLECARTESIANPOINTSBYCOVER",
        FunctionCategory::Other,
        4,
        7,
        &[
            required("Size", ParamKind::Number),
            required("Table", ParamKind::Table),
            required("XAxis", ParamKind::Column),
            required("YAxis", ParamKind::Column),
            optional("Radius", ParamKind::Column),
            optional("MaxMinRatio", ParamKind::Number),
            optional("MaxBlankRatio", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Column, ParamKind::Column]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Column, ParamKind::Column, ParamKind::Column]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Column, ParamKind::Column, ParamKind::Column, ParamKind::Number]),
            overload(&[
                ParamKind::Number,
                ParamKind::Table,
                ParamKind::Column,
                ParamKind::Column,
                ParamKind::Column,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, false),
        "Returns a sample of at most `Size` rows from a table, chosen by plotting each row as a \
         point on the X and Y coordinates and then dropping overlapping points. The optional \
         Radius column controls point size, MaxMinRatio limits the ratio between the largest and \
         smallest drawn points, and MaxBlankRatio limits the ratio between the largest point and \
         those with a blank radius. The function is used by the engine's scatter chart sampling \
         and is not documented by Microsoft.",
        &[
            "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[Longitude], Sales[Latitude] )",
            "SAMPLECARTESIANPOINTSBYCOVER ( 1000, Sales, Sales[X], Sales[Y], Sales[Radius], 4, 0.1 )",
        ],
    ),
];
