//! Table manipulation functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! The category holds four kinds of functions:
//!
//! - table constructors and projections (`DATATABLE`, `ROW`, `ADDCOLUMNS`, `SELECTCOLUMNS`,
//!   `SUMMARIZE`, `SUMMARIZECOLUMNS`, `GROUPBY`);
//! - relational operations (`CROSSJOIN`, `UNION`, `EXCEPT`, `INTERSECT`, the natural joins and
//!   `SUBSTITUTEWITHINDEX`);
//! - companion functions that are only valid as arguments of those operations (`CURRENTGROUP`,
//!   `IGNORE`, `NONVISUAL`, `NATURALJOINUSAGE`, `ROLLUP`, `ROLLUPGROUP`,
//!   `ROLLUPADDISSUBTOTAL`, `ROLLUPISSUBTOTAL`);
//! - row selection and value helpers (`DISTINCT`, `VALUES`, `FILTERS`, `TOPN`, `TOPNSKIP`,
//!   `GENERATESERIES`, `DETAILROWS`, `TREATAS`, `SAMPLEAXISWITHLOCALMINMAX`,
//!   `TOPNPERLEVEL`).
//!
//! The companion functions are catalogued because the parser accepts them, but several do not
//! return a value on their own: they are syntax markers inside `SUMMARIZE`,
//! `SUMMARIZECOLUMNS` or `ADDMISSINGITEMS`, so their return type is [`ReturnType::Blank`].

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, variadic};

/// Available to every host, with the usual DirectQuery restrictions.
const STANDARD: FunctionFlags = FunctionFlags::new(false, false, false, true, true);

/// Iterators evaluate an expression once per row of the first table argument.
const ITERATOR: FunctionFlags = FunctionFlags::new(true, false, false, true, true);

/// No Excel version listed by dax.guide supports the function.
const ENGINE_ONLY: FunctionFlags = FunctionFlags::new(false, false, false, false, true);

/// Power BI-only extension functions: engine extensions that DirectQuery cannot evaluate.
const EXTENSION: FunctionFlags = FunctionFlags::new(false, false, false, false, true);

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "ADDCOLUMNS",
        FunctionCategory::TableManipulation,
        3,
        usize::MAX,
        &[required("Table", ParamKind::Table), required("Name", ParamKind::Text), required("Expression", ParamKind::Expression)],
        &[variadic(&[ParamKind::Table, ParamKind::Text, ParamKind::Expression])],
        ReturnType::Table,
        ITERATOR,
        "Returns the input table with new columns added. Each expression is evaluated in the \
         row context of the table, so the expressions are repeatable in `Name, Expression` \
         pairs. The added columns keep the data lineage of a simple column reference for \
         NATURALINNERJOIN/NATURALLEFTOUTERJOIN but not for context transition.",
        &[
            "ADDCOLUMNS ( Sales, \"Margin\", Sales[Amount] - Sales[Cost] )",
            "ADDCOLUMNS ( Product, \"Brand\", Product[Brand], \"Price\", Product[List Price] )",
        ],
    ),
    spec(
        "ADDMISSINGITEMS",
        FunctionCategory::TableManipulation,
        1,
        usize::MAX,
        &[
            optional("ShowAll_ColumnName", ParamKind::Column),
            required("Table", ParamKind::Table),
            optional("GroupBy_ColumnName", ParamKind::Any),
            optional("FilterTable", ParamKind::Table),
        ],
        &[overload(&[ParamKind::Table]), variadic(&[ParamKind::Any])],
        ReturnType::Table,
        STANDARD,
        "Adds back the rows of a SUMMARIZECOLUMNS result that were removed because every \
         measure was blank. The optional leading columns list which columns must show all \
         items; the table argument is the SUMMARIZECOLUMNS result, followed by optional \
         group-by columns (possibly ROLLUPGROUP/ROLLUPADDISSUBTOTAL calls) and filters. \
         ROLLUPISSUBTOTAL marks the subtotal columns.",
        &["ADDMISSINGITEMS ( Sales[Region], SUMMARIZECOLUMNS ( Sales[Region], \"Amount\", SUM ( Sales[Amount] ) ), Sales[Region] )"],
    ),
    spec(
        "CROSSJOIN",
        FunctionCategory::TableManipulation,
        1,
        usize::MAX,
        &[required("Table", ParamKind::Table)],
        &[variadic(&[ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Returns the Cartesian product of the table arguments. The result has the product of \
         the row counts and the sum of the column counts; column names must be different in \
         every argument or the function raises an error.",
        &["CROSSJOIN ( Product, Store )", "CROSSJOIN ( Product, Store, 'Date' )"],
    ),
    spec(
        "CURRENTGROUP",
        FunctionCategory::TableManipulation,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::Table,
        STANDARD,
        "Returns the subset of rows of the GROUPBY input table that belong to the current \
         group. It takes no arguments and is only valid as the table argument of an \
         aggregation iterator inside GROUPBY, such as SUMX or COUNTX.",
        &["GROUPBY ( Sales, Sales[Region], \"Amount\", SUMX ( CURRENTGROUP (), Sales[Amount] ) )"],
    ),
    spec(
        "DATATABLE",
        FunctionCategory::TableManipulation,
        3,
        usize::MAX,
        &[required("Name", ParamKind::Text), required("DataType", ParamKind::Text), required("Data", ParamKind::Any)],
        &[variadic(&[ParamKind::Text, ParamKind::Text, ParamKind::Any])],
        ReturnType::Table,
        STANDARD,
        "Returns a table whose columns and rows are declared inline. Column names and data type \
         names alternate, followed by a single table constructor with the data. Data type names \
         are BOOLEAN/LOGICAL, CURRENCY/DECIMAL, DATETIME, DOUBLE, INTEGER/INT64 and \
         STRING/TEXT; unlike the table constructor, DATATABLE requires literal values.",
        &[
            "DATATABLE ( \"Name\", STRING, { { \"Small\", 1 }, { \"Medium\", 2 } } )",
            "DATATABLE ( \"Name\", STRING, \"Ordinal\", INTEGER, { { \"Small\", 1 }, { \"Large\", 3 } } )",
        ],
    ),
    spec(
        "DETAILROWS",
        FunctionCategory::TableManipulation,
        1,
        1,
        &[required("Measure", ParamKind::Reference)],
        &[overload(&[ParamKind::Reference])],
        ReturnType::Table,
        STANDARD,
        "Returns the table produced by the DetailRows expression defined on a measure. When the \
         measure has no DetailRows expression, the entire table the measure belongs to is \
         returned. Context transition is not applied by the engine; wrap the call in \
         CALCULATETABLE inside a row context when transition is intended.",
        &["DETAILROWS ( [Sales Amount] )"],
    ),
    spec(
        "DISTINCT",
        FunctionCategory::TableManipulation,
        1,
        1,
        &[required("ColumnNameOrTableExpr", ParamKind::Reference)],
        &[overload(&[ParamKind::Column]), overload(&[ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Returns the distinct values of a column, or the distinct rows of a table expression. \
         Unlike VALUES, a column argument does not add the blank row generated by an invalid \
         relationship.",
        &["DISTINCT ( Sales[Region] )", "DISTINCT ( Sales )"],
    ),
    spec(
        "EXCEPT",
        FunctionCategory::TableManipulation,
        2,
        2,
        &[required("LeftTable", ParamKind::Table), required("RightTable", ParamKind::Table)],
        &[overload(&[ParamKind::Table, ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Returns the rows of the left table that do not appear in the right table. A row present \
         in both tables is removed together with its duplicates; columns are matched by position \
         with strict comparison and the result keeps the left table's column names and lineage.",
        &["EXCEPT ( Sales, SalesArchive )"],
    ),
    spec(
        "FILTERS",
        FunctionCategory::TableManipulation,
        1,
        1,
        &[required("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Column])],
        ReturnType::Table,
        STANDARD,
        "Returns the filter values applied directly to a column, as a single-column table. The \
         column argument must be a column reference; the result can include the blank row \
         generated by an invalid relationship.",
        &["FILTERS ( Store[Continent] )", "FILTERS ( 'Date'[Calendar Year] )"],
    ),
    spec(
        "GENERATE",
        FunctionCategory::TableManipulation,
        2,
        2,
        &[required("Table1", ParamKind::Table), required("Table2", ParamKind::Table)],
        &[overload(&[ParamKind::Table, ParamKind::Table])],
        ReturnType::Table,
        ITERATOR,
        "Evaluates the second table expression for every row of the first table and returns the \
         union of those results with the corresponding first-table columns. Rows of the first \
         table whose second expression is empty are dropped, unlike GENERATEALL.",
        &["GENERATE ( Product, RELATEDTABLE ( Sales ) )"],
    ),
    spec(
        "GENERATEALL",
        FunctionCategory::TableManipulation,
        2,
        2,
        &[required("Table1", ParamKind::Table), required("Table2", ParamKind::Table)],
        &[overload(&[ParamKind::Table, ParamKind::Table])],
        ReturnType::Table,
        ITERATOR,
        "Evaluates the second table expression for every row of the first table and keeps the \
         first-table row even when that expression is empty, filling the second table's columns \
         with blanks. Column names must be different in both arguments.",
        &["GENERATEALL ( Product, RELATEDTABLE ( Sales ) )"],
    ),
    spec(
        "GENERATESERIES",
        FunctionCategory::TableManipulation,
        2,
        3,
        &[required("StartValue", ParamKind::Number), required("EndValue", ParamKind::Number), optional("IncrementValue", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number]), overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Table,
        STANDARD,
        "Returns a single-column table named Value holding an arithmetic series from StartValue \
         to EndValue. IncrementValue must be positive and defaults to 1; the series stops at the \
         last value less than or equal to EndValue, and EndValue below StartValue yields an \
         empty table.",
        &["GENERATESERIES ( 1, 10, 2 )", "GENERATESERIES ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ) )"],
    ),
    spec(
        "GROUPBY",
        FunctionCategory::TableManipulation,
        1,
        usize::MAX,
        &[
            required("Table", ParamKind::Table),
            optional("GroupBy_ColumnName", ParamKind::Column),
            optional("Name", ParamKind::Text),
            optional("Expression", ParamKind::Expression),
        ],
        &[
            overload(&[ParamKind::Table]),
            overload(&[ParamKind::Table, ParamKind::Column]),
            variadic(&[ParamKind::Table, ParamKind::Column, ParamKind::Any]),
        ],
        ReturnType::Table,
        STANDARD,
        "Groups the input table by the given columns and adds expression columns evaluated per \
         group. Expressions added by GROUPBY must aggregate CURRENTGROUP() and cannot use \
         CALCULATE; use SUMMARIZE when the added columns aggregate base-table columns.",
        &["GROUPBY ( Sales, Sales[Region], \"Amount\", SUMX ( CURRENTGROUP (), Sales[Amount] ) )", "GROUPBY ( Customer, Customer[Country] )"],
    ),
    spec(
        "IGNORE",
        FunctionCategory::TableManipulation,
        1,
        1,
        &[required("Measure_Expression", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Blank,
        STANDARD,
        "Marks a SUMMARIZECOLUMNS expression so that it is excluded when deciding whether a row \
         is blank. Rows are kept as long as another expression without IGNORE returns a value; \
         the function itself returns no value.",
        &["SUMMARIZECOLUMNS ( Sales[Region], IGNORE ( [Sales Amount] ) )"],
    ),
    spec(
        "INTERSECT",
        FunctionCategory::TableManipulation,
        2,
        2,
        &[required("LeftTable", ParamKind::Table), required("RightTable", ParamKind::Table)],
        &[overload(&[ParamKind::Table, ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Returns the rows of the left table that also appear in the right table, keeping \
         duplicates from the left table. Columns are matched by position without type coercion; \
         the operation is not commutative and the result keeps the left table's column names and \
         lineage.",
        &["INTERSECT ( Sales, SalesArchive )"],
    ),
    spec(
        "NATURALINNERJOIN",
        FunctionCategory::TableManipulation,
        2,
        2,
        &[required("LeftTable", ParamKind::Table), required("RightTable", ParamKind::Table)],
        &[overload(&[ParamKind::Table, ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Joins two tables on all columns they have in common (including columns of their \
         expanded tables) and returns the matching rows. Joined columns must have the same name, \
         data type and lineage; comparison is strict, with no type coercion.",
        &["NATURALINNERJOIN ( Sales, Product )"],
    ),
    spec(
        "NATURALJOINUSAGE",
        FunctionCategory::TableManipulation,
        1,
        1,
        &[required("Expression", ParamKind::Any)],
        &[overload(&[ParamKind::Any])],
        ReturnType::Table,
        ENGINE_ONLY,
        "Adds a table as a natural join to the filter context. It is only valid as a value \
         filter of SUMMARIZECOLUMNS and is used mostly in queries sent to composite models; \
         dax.guide lists no Excel support and Microsoft documentation is not available.",
        &["SUMMARIZECOLUMNS ( Sales[Region], NATURALJOINUSAGE ( Product ) )"],
    ),
    spec(
        "NATURALLEFTOUTERJOIN",
        FunctionCategory::TableManipulation,
        2,
        2,
        &[required("LeftTable", ParamKind::Table), required("RightTable", ParamKind::Table)],
        &[overload(&[ParamKind::Table, ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Joins two tables on all columns they have in common and returns every row of the left \
         table, with blanks where the right table has no match. Joined columns must share name, \
         data type and lineage, and comparison does not coerce types.",
        &["NATURALLEFTOUTERJOIN ( Sales, Product )"],
    ),
    spec(
        "NONVISUAL",
        FunctionCategory::TableManipulation,
        1,
        1,
        &[required("Expression", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Marks a SUMMARIZECOLUMNS value filter as non-visual: the filter still restricts the \
         grouping columns but does not affect measure values, so measures that use \
         ALLSELECTED can return the non-visual totals.",
        &["SUMMARIZECOLUMNS ( 'Date'[Calendar Year], NONVISUAL ( TREATAS ( { \"CY 2008\" }, 'Date'[Calendar Year] ) ), \"Amount\", [Sales Amount] )"],
    ),
    spec(
        "ROLLUP",
        FunctionCategory::TableManipulation,
        1,
        usize::MAX,
        &[required("GroupBy_ColumnName", ParamKind::Column)],
        &[variadic(&[ParamKind::Column])],
        ReturnType::Blank,
        STANDARD,
        "Identifies the SUMMARIZE group-by columns that also produce subtotal rows. The \
         arguments must be qualified column names, not expressions, and are only valid inside \
         SUMMARIZE; the function itself returns no value.",
        &["SUMMARIZE ( Sales, ROLLUP ( Sales[Country], Sales[City] ), \"Amount\", SUM ( Sales[Amount] ) )"],
    ),
    spec(
        "ROLLUPADDISSUBTOTAL",
        FunctionCategory::TableManipulation,
        2,
        usize::MAX,
        &[
            optional("GrandtotalFilter", ParamKind::Table),
            required("GroupBy_ColumnName", ParamKind::Column),
            required("Name", ParamKind::Text),
            optional("GroupLevelFilter", ParamKind::Table),
        ],
        &[
            overload(&[ParamKind::Column, ParamKind::Text]),
            overload(&[ParamKind::Any, ParamKind::Column, ParamKind::Text]),
            variadic(&[ParamKind::Any, ParamKind::Column, ParamKind::Text, ParamKind::Any]),
        ],
        ReturnType::Blank,
        STANDARD,
        "Identifies the SUMMARIZECOLUMNS group-by columns that produce subtotals and adds a \
         Boolean column per pair indicating whether the row is a subtotal. It accepts an \
         optional grand-total filter before the pairs and an optional per-level filter after \
         each pair; it is only valid inside SUMMARIZECOLUMNS and returns no value.",
        &["SUMMARIZECOLUMNS ( ROLLUPADDISSUBTOTAL ( Sales[Country], \"IsSubtotal Country\" ), \"Amount\", SUM ( Sales[Amount] ) )"],
    ),
    spec(
        "ROLLUPGROUP",
        FunctionCategory::TableManipulation,
        1,
        usize::MAX,
        &[required("GroupBy_ColumnName", ParamKind::Column)],
        &[variadic(&[ParamKind::Column])],
        ReturnType::Blank,
        STANDARD,
        "Identifies a group of SUMMARIZE or SUMMARIZECOLUMNS columns that produce subtotals \
         together. Inside ROLLUP it suppresses the partial subtotals that ROLLUP would \
         otherwise add; the function itself returns no value.",
        &["SUMMARIZE ( Sales, ROLLUPGROUP ( Sales[Country], Sales[City] ), \"Amount\", SUM ( Sales[Amount] ) )"],
    ),
    spec(
        "ROLLUPISSUBTOTAL",
        FunctionCategory::TableManipulation,
        2,
        usize::MAX,
        &[
            optional("GrandtotalFilter", ParamKind::Table),
            required("GroupBy_ColumnName", ParamKind::Column),
            required("IsSubtotal_ColumnName", ParamKind::Text),
            optional("GroupLevelFilter", ParamKind::Table),
        ],
        &[
            overload(&[ParamKind::Column, ParamKind::Text]),
            overload(&[ParamKind::Any, ParamKind::Column, ParamKind::Text]),
            variadic(&[ParamKind::Any, ParamKind::Column, ParamKind::Text, ParamKind::Any]),
        ],
        ReturnType::Blank,
        STANDARD,
        "Pairs the subtotal columns created by ROLLUPADDISSUBTOTAL with their group-by columns \
         so that ADDMISSINGITEMS can add the missing subtotal rows. It is only valid inside \
         ADDMISSINGITEMS and returns no value.",
        &[
            "ADDMISSINGITEMS ( Sales[Country], SUMMARIZECOLUMNS ( ROLLUPADDISSUBTOTAL ( Sales[Country], \"IsSubtotal\" ), \"Amount\", SUM ( Sales[Amount] ) ), ROLLUPISSUBTOTAL ( Sales[Country], [IsSubtotal] ) )",
        ],
    ),
    spec(
        "ROW",
        FunctionCategory::TableManipulation,
        2,
        usize::MAX,
        &[required("Name", ParamKind::Text), required("Expression", ParamKind::Any)],
        &[variadic(&[ParamKind::Text, ParamKind::Any])],
        ReturnType::Table,
        STANDARD,
        "Returns a single-row table with the named columns. Arguments come in `Name, \
         Expression` pairs; ROW does not preserve the data lineage of column references used as \
         expressions.",
        &["ROW ( \"Product\", \"Bike\", \"Price\", 1500 )", "ROW ( \"Value\", 1 )"],
    ),
    spec(
        "SAMPLEAXISWITHLOCALMINMAX",
        FunctionCategory::TableManipulation,
        5,
        usize::MAX,
        &[
            required("Size", ParamKind::Number),
            required("Table", ParamKind::Table),
            required("Axis", ParamKind::Column),
            required("Measure", ParamKind::Expression),
            required("MinResolution", ParamKind::Number),
            optional("DynamicSeries", ParamKind::Column),
            optional("DynamicSeriesSelectionCriteria", ParamKind::Text),
            optional("DynamicSeriesSelectionOrder", ParamKind::Boolean),
            optional("MaxResolution", ParamKind::Number),
            optional("MaxDynamicSeries", ParamKind::Number),
            optional("MaxIterations", ParamKind::Number),
        ],
        &[variadic(&[ParamKind::Number, ParamKind::Table, ParamKind::Column, ParamKind::Any, ParamKind::Number])],
        ReturnType::Table,
        ENGINE_ONLY,
        "Returns a subset of the input table obtained by binning the numeric or date axis into \
         equal-sized bins and preserving the local minimum and maximum of each measure per \
         series. Power BI uses it to reduce the points of line charts with a continuous axis; \
         dax.guide lists no Excel support and Microsoft documentation is not available.",
        &["SAMPLEAXISWITHLOCALMINMAX ( 100, Sales, Sales[Order Date], Sales[Amount], 10 )"],
    ),
    spec(
        "SELECTCOLUMNS",
        FunctionCategory::TableManipulation,
        1,
        usize::MAX,
        &[required("Table", ParamKind::Table), optional("Name", ParamKind::Text), optional("Expression", ParamKind::Expression)],
        &[overload(&[ParamKind::Table]), variadic(&[ParamKind::Table, ParamKind::Any])],
        ReturnType::Table,
        ITERATOR,
        "Returns a table with the same number of rows as the input and one column per \
         `Name, Expression` pair, evaluated in row context. Unlike ADDCOLUMNS it starts from an \
         empty table; the name may be omitted when the expression is a simple column reference, \
         and simple references keep their data lineage.",
        &[
            "SELECTCOLUMNS ( Sales, \"Amount\", Sales[Amount] )",
            "SELECTCOLUMNS ( Product, \"Brand\", Product[Brand], \"Category\", Product[Category] )",
        ],
    ),
    spec(
        "SUBSTITUTEWITHINDEX",
        FunctionCategory::TableManipulation,
        4,
        usize::MAX,
        &[
            required("Table", ParamKind::Table),
            required("Name", ParamKind::Text),
            required("SemiJoinIndexTable", ParamKind::Table),
            required("Expression", ParamKind::Expression),
            optional("Order", ParamKind::Number),
        ],
        &[variadic(&[ParamKind::Table, ParamKind::Text, ParamKind::Table, ParamKind::Expression])],
        ReturnType::Table,
        ITERATOR,
        "Semijoins two tables and replaces the common columns with a zero-based index column. \
         The index follows the row order of the second table sorted by the supplied order \
         expressions; matching uses the same lineage rules as the natural joins and raises an \
         error when no column matches.",
        &["SUBSTITUTEWITHINDEX ( Sales, \"ProductIndex\", DISTINCT ( Sales[ProductKey] ), Sales[ProductKey] )"],
    ),
    spec(
        "SUMMARIZE",
        FunctionCategory::TableManipulation,
        1,
        usize::MAX,
        &[
            required("Table", ParamKind::Table),
            optional("GroupBy_ColumnName", ParamKind::Reference),
            optional("Name", ParamKind::Text),
            optional("Expression", ParamKind::Expression),
        ],
        &[
            overload(&[ParamKind::Table]),
            overload(&[ParamKind::Table, ParamKind::Reference]),
            variadic(&[ParamKind::Table, ParamKind::Reference, ParamKind::Any]),
        ],
        ReturnType::Table,
        STANDARD,
        "Groups the input table by the given columns and optionally adds computed columns. The \
         group-by arguments can be columns of the table or its related tables, or ROLLUP / \
         ROLLUPGROUP calls. Adding columns with SUMMARIZE is deprecated in favour of \
         SUMMARIZECOLUMNS or ADDCOLUMNS, and ROLLUP/ROLLUPGROUP columns lose their data lineage.",
        &["SUMMARIZE ( Sales, Sales[Region], \"Amount\", SUM ( Sales[Amount] ) )", "SUMMARIZE ( Sales, 'Product'[Brand], 'Date'[Calendar Year] )"],
    ),
    spec(
        "SUMMARIZECOLUMNS",
        FunctionCategory::TableManipulation,
        0,
        usize::MAX,
        &[
            optional("GroupBy_ColumnName", ParamKind::Any),
            optional("FilterTable", ParamKind::Table),
            optional("Name", ParamKind::Text),
            optional("Expression", ParamKind::Expression),
        ],
        &[overload(&[]), variadic(&[ParamKind::Any])],
        ReturnType::Table,
        STANDARD,
        "Builds a summary table from group-by columns, filters applied only to the grouping, and \
         added expression columns. A row is included only when at least one expression returns a \
         non-blank value; filters accept KEEPFILTERS, REMOVEFILTERS and NONVISUAL, and group-by \
         arguments accept ROLLUPGROUP and ROLLUPADDISSUBTOTAL. Since June 2024 it can run inside \
         context transition, with the remaining limitations documented by Microsoft.",
        &[
            "SUMMARIZECOLUMNS ( Sales[Region], \"Amount\", SUM ( Sales[Amount] ) )",
            "SUMMARIZECOLUMNS ( 'Product'[Brand], 'Date'[Calendar Year], \"Amount\", [Sales Amount] )",
            "SUMMARIZECOLUMNS ( )",
        ],
    ),
    spec(
        "TOPN",
        FunctionCategory::TableManipulation,
        2,
        usize::MAX,
        &[
            required("N_Value", ParamKind::Number),
            required("Table", ParamKind::Table),
            optional("OrderBy_Expression", ParamKind::Expression),
            optional("Order", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Number, ParamKind::Table]), variadic(&[ParamKind::Number, ParamKind::Table, ParamKind::Expression])],
        ReturnType::Table,
        ITERATOR,
        "Returns the top N rows of a table according to the order-by expressions. Order is \
         0/FALSE/DESC or 1/TRUE/ASC and defaults to descending; ties at the N-th row are all \
         returned, and N_Value of zero or less yields an empty table. The result has no \
         guaranteed sort order.",
        &["TOPN ( 10, Sales, Sales[Amount], DESC )"],
    ),
    spec(
        "TOPNPERLEVEL",
        FunctionCategory::TableManipulation,
        6,
        6,
        &[
            required("Rows", ParamKind::Number),
            required("Table", ParamKind::Table),
            required("LevelsDefinition", ParamKind::Table),
            required("NodesExpanded", ParamKind::Table),
            required("LevelsBoundaries", ParamKind::Table),
            required("RestartIndicatorColumnName", ParamKind::Text),
        ],
        &[overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Table, ParamKind::Table, ParamKind::Table, ParamKind::Text])],
        ReturnType::Table,
        EXTENSION,
        "Returns the rows to display for an expanded hierarchy, restricted to the selected nodes \
         and boundaries. It is an undocumented Power BI extension that is not part of the \
         Analysis Services engine and has no Excel or DirectQuery support.",
        &["TOPNPERLEVEL ( 10, Sales, Levels, Expanded, Boundaries, \"Restart\" )"],
    ),
    spec(
        "TOPNSKIP",
        FunctionCategory::TableManipulation,
        3,
        usize::MAX,
        &[
            required("Rows", ParamKind::Number),
            required("Skip_Rows_Value", ParamKind::Number),
            required("Table", ParamKind::Table),
            optional("OrderBy_Expression", ParamKind::Expression),
            optional("Order", ParamKind::Number),
            optional("Ties", ParamKind::Text),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Table]),
            variadic(&[ParamKind::Number, ParamKind::Number, ParamKind::Table, ParamKind::Any]),
        ],
        ReturnType::Table,
        ITERATOR,
        "Retrieves Rows rows from a table after skipping Skip rows, ordered by the order-by \
         expressions. Ties can be handled as Skip (ranking skips tied elements) or Dense (all \
         tied elements count as one); the result has no guaranteed sort order. With a non-zero \
         skip the table must reference physical columns with lineage and no context transition.",
        &["TOPNSKIP ( 10, 20, Sales, Sales[Amount], DESC )"],
    ),
    spec(
        "TREATAS",
        FunctionCategory::TableManipulation,
        2,
        usize::MAX,
        &[required("Expression", ParamKind::Table), required("ColumnName", ParamKind::Column)],
        &[variadic(&[ParamKind::Table, ParamKind::Column])],
        ReturnType::Table,
        STANDARD,
        "Applies the data lineage of the output columns to the table returned by the first \
         argument, so that its values filter the model columns as if they came from those \
         columns. The first argument must be a table expression; unused values are filtered out \
         and the result can be stored in a variable.",
        &["TREATAS ( { \"North America\" }, Sales[Region] )", "TREATAS ( { 2008, 2009 }, 'Date'[Calendar Year] )"],
    ),
    spec(
        "UNION",
        FunctionCategory::TableManipulation,
        2,
        usize::MAX,
        &[required("Table", ParamKind::Table), required("Table2", ParamKind::Table)],
        &[variadic(&[ParamKind::Table, ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "Returns all rows of all table arguments, keeping duplicates. Columns are combined by \
         position and the result takes its column names from the first table; differing data \
         types are resolved with DAX coercion rules, and lineage is kept only where all input \
         columns share it.",
        &["UNION ( Sales, SalesArchive )", "UNION ( Sales, SalesArchive, SalesForecast )"],
    ),
    spec(
        "VALUES",
        FunctionCategory::TableManipulation,
        1,
        1,
        &[required("TableNameOrColumnName", ParamKind::Reference)],
        &[overload(&[ParamKind::Column]), overload(&[ParamKind::Table])],
        ReturnType::Table,
        STANDARD,
        "With a column, returns its unique values as a single-column table, including the blank \
         row produced by an invalid relationship. With a table name it returns the table with \
         the same columns and all rows, without removing duplicates. VALUES needs a model table \
         or column, so it cannot be applied to a variable.",
        &["VALUES ( Sales[Region] )", "VALUES ( Store )"],
    ),
];
