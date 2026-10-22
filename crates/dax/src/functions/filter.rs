//! Filter functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! This category covers table expressions that inspect or remove filter context (`ALL`,
//! `ALLSELECTED`, ...), the two functions that build a filter context (`CALCULATE`,
//! `CALCULATETABLE`), and the window/apply clauses (`INDEX`, `OFFSET`, `WINDOW`, `ORDERBY`,
//! `PARTITIONBY`, `MATCHBY`) that are only meaningful inside one of those table expressions.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, variadic};

/// Functions whose original implementation shipped in Excel's embedded engine. The argument
/// order and semantics are shared with Excel 2016 and later.
const EXCEL: FunctionFlags = FunctionFlags::new(false, false, false, true, true);

/// Engine features that Power BI uses internally for composite models and window functions.
/// Excel's embedded engine rejects them, and the DirectQuery notes still apply.
const INTERNAL: FunctionFlags = FunctionFlags::new(false, false, false, false, true);

/// `FILTER` is an iterator: it evaluates its predicate once per row.
const ITERATOR: FunctionFlags = FunctionFlags::new(true, false, false, true, true);

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "ALL",
        FunctionCategory::Filter,
        0,
        usize::MAX,
        &[optional("TableNameOrColumnName", ParamKind::Reference), optional("ColumnName", ParamKind::Column)],
        &[overload(&[]), overload(&[ParamKind::Reference]), variadic(&[ParamKind::Reference, ParamKind::Column])],
        ReturnType::Table,
        EXCEL,
        "Returns a table ignoring any filters on it, or every value of a column ignoring the \
         filters on that column. As a table expression it materialises all rows (or all distinct \
         values of one or more columns); without arguments it is only valid as a CALCULATE or \
         CALCULATETABLE modifier and removes every filter from the filter context.",
        &["ALL ( Sales )", "ALL ( Sales[Region] )", "ALL ( Sales, Sales[Region], Sales[Country] )", "ALL ( )"],
    ),
    spec(
        "ALLCROSSFILTERED",
        FunctionCategory::Filter,
        1,
        1,
        &[required("TableName", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        EXCEL,
        "Removes, as a CALCULATE modifier only, every filter on a table and on the tables that \
         cross-filter it through weak or bidirectional relationships. Unlike ALL it never returns \
         a materialised table, so it cannot be used as a table expression.",
        &["ALLCROSSFILTERED ( Sales )", "ALLCROSSFILTERED ( 'Product' )"],
    ),
    spec(
        "ALLEXCEPT",
        FunctionCategory::Filter,
        2,
        usize::MAX,
        &[required("TableName", ParamKind::Table), required("ColumnName", ParamKind::Column), optional("ColumnName", ParamKind::Column)],
        &[variadic(&[ParamKind::Table, ParamKind::Column])],
        ReturnType::Table,
        EXCEL,
        "Removes all filters from a table except those on the listed columns. As a CALCULATE \
         modifier it keeps only the named column filters; as a table expression it materialises \
         the unique combinations of the table's remaining columns.",
        &["ALLEXCEPT ( Sales, Sales[Region] )", "ALLEXCEPT ( 'Product', 'Product'[Color], 'Product'[Brand] )"],
    ),
    spec(
        "ALLNOBLANKROW",
        FunctionCategory::Filter,
        1,
        usize::MAX,
        &[required("TableNameOrColumnName", ParamKind::Reference), optional("ColumnName", ParamKind::Column)],
        &[overload(&[ParamKind::Reference]), variadic(&[ParamKind::Reference, ParamKind::Column])],
        ReturnType::Table,
        EXCEL,
        "Like ALL, removes the filters on a table or on the listed columns, but always excludes \
         the blank row that the engine adds for an invalid relationship. Real blank values stored \
         in the columns are kept in the result.",
        &["ALLNOBLANKROW ( Customer )", "ALLNOBLANKROW ( Customer[Country], Customer[City] )"],
    ),
    spec(
        "ALLSELECTED",
        FunctionCategory::Filter,
        0,
        usize::MAX,
        &[optional("TableNameOrColumnName", ParamKind::Reference), optional("ColumnName", ParamKind::Column)],
        &[overload(&[]), overload(&[ParamKind::Reference]), variadic(&[ParamKind::Reference, ParamKind::Column])],
        ReturnType::Table,
        EXCEL,
        "Restores the filters that came from outside the query while ignoring the filters applied \
         inside it, so the result reflects the outer shadow filter context. As a table expression \
         it needs at least one argument; without arguments it is only a CALCULATE modifier.",
        &["ALLSELECTED ( Sales )", "ALLSELECTED ( Sales[Region] )", "ALLSELECTED ( )"],
    ),
    spec(
        "ALLSELECTEDAPPLY",
        FunctionCategory::Filter,
        1,
        1,
        &[required("TableExpression", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        INTERNAL,
        "Marks a table expression supplied to GROUPCROSSAPPLY or GROUPCROSSAPPLYTABLE so that it \
         starts hidden from the filter context and only becomes active when ALLSELECTED runs \
         inside CALCULATE. Used internally by Power BI composite models.",
        &["ALLSELECTEDAPPLY ( VALUES ( Product[Color] ) )"],
    ),
    spec(
        "ALLSELECTEDREMOVE",
        FunctionCategory::Filter,
        1,
        usize::MAX,
        &[required("TableExpression", ParamKind::Table), optional("Column", ParamKind::Column)],
        &[overload(&[ParamKind::Table]), variadic(&[ParamKind::Table, ParamKind::Column])],
        ReturnType::Table,
        INTERNAL,
        "In GROUPCROSSAPPLY or GROUPCROSSAPPLYTABLE, hides the listed columns of a table \
         expression from a later ALLSELECTED so they no longer participate in the restored filter \
         context. With no column argument every column of the expression is hidden.",
        &["ALLSELECTEDREMOVE ( VALUES ( Product[Color] ) )", "ALLSELECTEDREMOVE ( VALUES ( Product[Color] ), Product[Color] )"],
    ),
    spec(
        "ALWAYSAPPLY",
        FunctionCategory::Filter,
        1,
        1,
        &[required("TableExpression", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        INTERNAL,
        "In GROUPCROSSAPPLY or GROUPCROSSAPPLYTABLE, forces a filter table to affect the measures \
         even when it does not change the current filter context, such as an empty table. Used \
         internally by Power BI composite models.",
        &["ALWAYSAPPLY ( FILTER ( VALUES ( Product[Color] ), FALSE ) )"],
    ),
    spec(
        "CALCULATE",
        FunctionCategory::Filter,
        1,
        usize::MAX,
        &[required("Expression", ParamKind::Expression), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression]), variadic(&[ParamKind::Expression, ParamKind::Any])],
        ReturnType::Any,
        EXCEL,
        "Evaluates an expression in a filter context modified by its filter arguments: table \
         expressions replace the filters on their columns, boolean predicates are wrapped in \
         FILTER over ALL, and context transition turns any row context into an equivalent filter \
         context. Returns the scalar result of the expression.",
        &["CALCULATE ( SUM ( Sales[Amount] ), Sales[Region] = \"East\" )", "CALCULATE ( [Sales Amount], ALL ( Product ) )"],
    ),
    spec(
        "CALCULATETABLE",
        FunctionCategory::Filter,
        1,
        usize::MAX,
        &[required("Table", ParamKind::Table), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Table]), variadic(&[ParamKind::Table, ParamKind::Any])],
        ReturnType::Table,
        EXCEL,
        "Evaluates a table expression in a filter context modified by its filter arguments, using \
         the same filter semantics and context transition as CALCULATE but returning a table \
         instead of a scalar.",
        &["CALCULATETABLE ( VALUES ( Product[Color] ), Product[Brand] = \"Contoso\" )"],
    ),
    spec(
        "DEPENDON",
        FunctionCategory::Filter,
        2,
        usize::MAX,
        &[required("TableExpression", ParamKind::Table), required("ColumnName", ParamKind::Column), optional("ColumnName", ParamKind::Column)],
        &[variadic(&[ParamKind::Table, ParamKind::Column])],
        ReturnType::Table,
        INTERNAL,
        "Makes a table expression depend on outer columns: its first columns are matched, in \
         order, against the current values of the listed outer columns, acting as an inner join \
         and hiding the matched columns from the result. Used internally by Power BI composite \
         models.",
        &["DEPENDON ( Sales, Sales[OrderDate] )", "DEPENDON ( FILTER ( Sales, Sales[Amount] > 0 ), Sales[OrderDate] )"],
    ),
    spec(
        "FILTER",
        FunctionCategory::Filter,
        2,
        2,
        &[required("Table", ParamKind::Table), required("FilterExpression", ParamKind::Logical)],
        &[overload(&[ParamKind::Table, ParamKind::Logical])],
        ReturnType::Table,
        ITERATOR,
        "Returns the rows of a table for which the predicate is TRUE. The predicate runs in a row \
         context, so it can read the current row's columns; context transition applies when it \
         calls a measure, and BLANK evaluates as FALSE. Filter context is preserved, not replaced.",
        &["FILTER ( Sales, Sales[Amount] > 0 )", "FILTER ( ALL ( Sales ), Sales[Region] = \"East\" )"],
    ),
    spec(
        "FILTERCLUSTER",
        FunctionCategory::Filter,
        4,
        usize::MAX,
        &[
            required("GroupBy_ColumnName", ParamKind::Column),
            required("FilterTable", ParamKind::Table),
            required("Separator", ParamKind::Text),
            required("TableScan", ParamKind::Table),
        ],
        &[variadic(&[ParamKind::Column, ParamKind::Any, ParamKind::Text, ParamKind::Any])],
        ReturnType::Table,
        INTERNAL,
        "Builds a correlated join table over a set of groups for GROUPCROSSAPPLY or \
         GROUPCROSSAPPLYTABLE: the group-by columns, the filter tables and the table scans are \
         combined with autoexist semantics, and the literal separator only marks where the table \
         scans begin. Used internally by Power BI composite models.",
        &["FILTERCLUSTER ( Product[Category], ALL ( Product ), \"\", ALL ( Sales ) )"],
    ),
    spec(
        "GROUPCROSSAPPLY",
        FunctionCategory::Filter,
        2,
        usize::MAX,
        &[
            required("GroupBy_ColumnName", ParamKind::Column),
            required("FilterTable", ParamKind::Table),
            optional("Name", ParamKind::Text),
            optional("Expression", ParamKind::Any),
        ],
        &[overload(&[ParamKind::Column, ParamKind::Table]), variadic(&[ParamKind::Column, ParamKind::Any, ParamKind::Any])],
        ReturnType::Table,
        INTERNAL,
        "Builds a summary table over a set of groups, cross-joining the filter tables into the \
         filter context of the group-by columns without implicit autoexist. Each name/expression \
         pair adds a column, and rows whose expressions are all blank are dropped. Used \
         internally by Power BI composite models.",
        &["GROUPCROSSAPPLY ( 'Date'[Year], ALL ( 'Date' ) )", "GROUPCROSSAPPLY ( 'Date'[Year], ALL ( 'Date' ), \"Sales\", [Sales Amount] )"],
    ),
    spec(
        "GROUPCROSSAPPLYTABLE",
        FunctionCategory::Filter,
        2,
        usize::MAX,
        &[
            required("GroupBy_ColumnName", ParamKind::Column),
            required("FilterTable", ParamKind::Table),
            optional("Name", ParamKind::Text),
            optional("TableExpression", ParamKind::Table),
        ],
        &[overload(&[ParamKind::Column, ParamKind::Table]), variadic(&[ParamKind::Column, ParamKind::Any, ParamKind::Any])],
        ReturnType::Table,
        INTERNAL,
        "Like GROUPCROSSAPPLY, builds a summary table over a set of groups, but the trailing \
         name/table-expression pairs add whole tables evaluated under the filter tables' context \
         instead of scalar expressions. Used internally by Power BI composite models.",
        &["GROUPCROSSAPPLYTABLE ( 'Date'[Year], ALL ( 'Date' ), \"Colors\", VALUES ( 'Product'[Color] ) )"],
    ),
    spec(
        "INDEX",
        FunctionCategory::Filter,
        1,
        7,
        &[
            required("Position", ParamKind::Number),
            optional("Relation", ParamKind::Table),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("PartitionBy", ParamKind::Any),
            optional("MatchBy", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Table]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Table,
        INTERNAL,
        "Returns, as a one-row table, the row at an absolute position (1-based, negative counts \
         from the end) within each partition of the relation, sorted by the ORDERBY clause. An \
         out-of-range or zero position yields an empty table; the relation defaults to \
         ALLSELECTED over the ordering and partitioning columns.",
        &[
            "INDEX ( 1, Sales, ORDERBY ( Sales[OrderDate] ) )",
            "INDEX ( -1, ALLSELECTED ( Sales ) )",
            "INDEX ( 1, Sales, ORDERBY ( Sales[OrderDate] ), KEEP, PARTITIONBY ( Sales[Region] ) )",
        ],
    ),
    spec(
        "KEEPFILTERS",
        FunctionCategory::Filter,
        1,
        1,
        &[required("Expression", ParamKind::Expression)],
        &[overload(&[ParamKind::Expression])],
        ReturnType::Any,
        EXCEL,
        "Filter modifier that makes its argument intersect the existing filter context instead of \
         replacing it, so a conflicting existing filter is kept. Applies to a CALCULATE or \
         CALCULATETABLE filter argument and also preserves the filter context during context \
         transition in an iterator.",
        &["KEEPFILTERS ( Product[Color] = \"Red\" )", "KEEPFILTERS ( VALUES ( Product[Color] ) )"],
    ),
    spec(
        "LOOKUP",
        FunctionCategory::Filter,
        3,
        usize::MAX,
        &[
            required("Result_ColumnOrExpression", ParamKind::Any),
            required("Search_Column", ParamKind::Column),
            required("Search_Value", ParamKind::Any),
            optional("Search_Column", ParamKind::Column),
            optional("Search_Value", ParamKind::Any),
            optional("AssociatedColumnsBehavior", ParamKind::Text),
        ],
        &[variadic(&[ParamKind::Any, ParamKind::Column, ParamKind::Any])],
        ReturnType::Any,
        INTERNAL,
        "Searches the cells identified by the search column/value pairs and returns a column value \
         or an expression evaluated over those cells; filters on columns not listed stay applied. \
         Only one cell may match, and the optional behaviour argument selects Explicit or \
         Inferred handling of associated columns.",
        &["LOOKUP ( [Sales Amount], 'Date'[Year], 2018 )", "LOOKUP ( [Sales Amount], 'Date'[Year], 2018, \"Inferred\" )"],
    ),
    spec(
        "LOOKUPVALUE",
        FunctionCategory::Filter,
        3,
        usize::MAX,
        &[
            required("Result_ColumnName", ParamKind::Column),
            required("Search_ColumnName", ParamKind::Column),
            required("Search_Value", ParamKind::Any),
            optional("Search_ColumnName", ParamKind::Column),
            optional("Search_Value", ParamKind::Any),
            optional("Alternate_Result", ParamKind::Any),
        ],
        &[variadic(&[ParamKind::Column, ParamKind::Column, ParamKind::Any])],
        ReturnType::Any,
        EXCEL,
        "Returns the result column value from the row where every search column/value pair \
         matches, ignoring the current filter context. With no match it returns BLANK or the \
         alternate result; identical values in several matching rows return that value, while \
         different values return the alternate result or an error.",
        &["LOOKUPVALUE ( Product[Color], Product[ProductKey], 1 )", "LOOKUPVALUE ( Product[Color], Product[ProductKey], 1, \"Unknown\" )"],
    ),
    spec(
        "LOOKUPWITHTOTALS",
        FunctionCategory::Filter,
        3,
        usize::MAX,
        &[
            required("Result_ColumnOrExpression", ParamKind::Any),
            required("Search_Column", ParamKind::Column),
            required("Search_Value", ParamKind::Any),
            optional("Search_Column", ParamKind::Column),
            optional("Search_Value", ParamKind::Any),
        ],
        &[variadic(&[ParamKind::Any, ParamKind::Column, ParamKind::Any])],
        ReturnType::Any,
        INTERNAL,
        "Searches the cells identified by the search column/value pairs, like LOOKUP, but resets \
         the non-specified columns to their grand total instead of keeping the current filters. \
         Equivalent to COLLAPSEALL around LOOKUP.",
        &["LOOKUPWITHTOTALS ( [Sales Amount], [Year], 2018 )"],
    ),
    spec(
        "MATCHBY",
        FunctionCategory::Filter,
        0,
        usize::MAX,
        &[optional("MatchBy_ColumnName", ParamKind::Column)],
        &[overload(&[]), variadic(&[ParamKind::Column])],
        ReturnType::Any,
        INTERNAL,
        "Declares the columns that identify the current row for a window function such as INDEX, \
         OFFSET or WINDOW. It is a clause, not a value: it can only appear inside a window \
         function and does not change the filter context by itself.",
        &["MATCHBY ( Sales[OrderKey] )", "MATCHBY ( )"],
    ),
    spec(
        "NONFILTER",
        FunctionCategory::Filter,
        1,
        1,
        &[required("TableExpression", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        INTERNAL,
        "In GROUPCROSSAPPLY or GROUPCROSSAPPLYTABLE, keeps a table expression permanently hidden \
         from the filter context even when its columns carry lineage, as if they were extension \
         columns. Used internally by Power BI composite models.",
        &["NONFILTER ( VALUES ( Product[Color] ) )"],
    ),
    spec(
        "OFFSET",
        FunctionCategory::Filter,
        1,
        7,
        &[
            required("Delta", ParamKind::Number),
            optional("Relation", ParamKind::Table),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("PartitionBy", ParamKind::Any),
            optional("MatchBy", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Table]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Table, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any, ParamKind::Any]),
        ],
        ReturnType::Table,
        INTERNAL,
        "Returns the row that is a number of rows before or after the current row within each \
         partition of the relation, ordered by the ORDERBY clause. The result is a one-row table; \
         a delta moving outside the partition yields an empty table.",
        &["OFFSET ( -1, Sales, ORDERBY ( Sales[OrderDate] ) )", "OFFSET ( 1, Sales )"],
    ),
    spec(
        "ORDERBY",
        FunctionCategory::Filter,
        0,
        usize::MAX,
        &[optional("OrderBy_Expression", ParamKind::Any), optional("OrderBy_Direction", ParamKind::Any)],
        &[overload(&[]), variadic(&[ParamKind::Any])],
        ReturnType::Any,
        INTERNAL,
        "Declares the sort expressions and directions (ASC or DESC, optionally with BLANKS FIRST \
         or BLANKS LAST) used by a window function such as INDEX, OFFSET or WINDOW. It is a \
         clause rather than a value and cannot be evaluated on its own.",
        &["ORDERBY ( Sales[Amount], DESC )", "ORDERBY ( Sales[OrderDate], ASC )", "ORDERBY ( )"],
    ),
    spec(
        "PARTITIONBY",
        FunctionCategory::Filter,
        0,
        usize::MAX,
        &[optional("PartitionBy_ColumnName", ParamKind::Column)],
        &[overload(&[]), variadic(&[ParamKind::Column])],
        ReturnType::Any,
        INTERNAL,
        "Declares the model columns that partition the relation of a window function such as \
         INDEX, OFFSET or WINDOW. It is a clause rather than a value, and the columns must have a \
         valid data lineage.",
        &["PARTITIONBY ( Sales[Region] )", "PARTITIONBY ( )"],
    ),
    spec(
        "REMOVEFILTERS",
        FunctionCategory::Filter,
        0,
        usize::MAX,
        &[optional("TableNameOrColumnName", ParamKind::Reference), optional("ColumnName", ParamKind::Column)],
        &[overload(&[]), overload(&[ParamKind::Reference]), variadic(&[ParamKind::Reference, ParamKind::Column])],
        ReturnType::Any,
        EXCEL,
        "Alias for ALL with the same filter-removal semantics: it clears the filters on the named \
         table or columns, or every filter when called without arguments. It is only valid as a \
         CALCULATE or CALCULATETABLE modifier and never returns a table.",
        &["REMOVEFILTERS ( Sales )", "REMOVEFILTERS ( Sales[Region] )", "REMOVEFILTERS ( )"],
    ),
    spec(
        "SELECTEDVALUE",
        FunctionCategory::Filter,
        1,
        2,
        &[required("ColumnName", ParamKind::Column), optional("AlternateResult", ParamKind::Any)],
        &[overload(&[ParamKind::Column]), overload(&[ParamKind::Column, ParamKind::Any])],
        ReturnType::Any,
        EXCEL,
        "Returns the value of a column when the filter context leaves exactly one distinct value, \
         otherwise returns the alternate result (BLANK when omitted). Equivalent to \
         IF ( HASONEVALUE ( column ), VALUES ( column ), alternate ).",
        &["SELECTEDVALUE ( Product[Color] )", "SELECTEDVALUE ( Product[Color], \"Multiple colors\" )"],
    ),
    spec(
        "SHADOWCLUSTER",
        FunctionCategory::Filter,
        1,
        1,
        &[required("TableExpression", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        INTERNAL,
        "In GROUPCROSSAPPLY or GROUPCROSSAPPLYTABLE, marks a table expression as a shadow so that \
         it is only enabled inside an ALLSELECTED context. Used internally by Power BI composite \
         models.",
        &["SHADOWCLUSTER ( VALUES ( Product[Color] ) )"],
    ),
    spec(
        "WINDOW",
        FunctionCategory::Filter,
        2,
        10,
        &[
            required("From", ParamKind::Number),
            optional("FromType", ParamKind::Any),
            required("To", ParamKind::Number),
            optional("ToType", ParamKind::Any),
            optional("Relation", ParamKind::Table),
            optional("OrderBy", ParamKind::Any),
            optional("Blanks", ParamKind::Any),
            optional("PartitionBy", ParamKind::Any),
            optional("MatchBy", ParamKind::Any),
            optional("Reset", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Any, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Any, ParamKind::Number, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Any, ParamKind::Number, ParamKind::Any, ParamKind::Table]),
            overload(&[ParamKind::Number, ParamKind::Any, ParamKind::Number, ParamKind::Any, ParamKind::Table, ParamKind::Any]),
            overload(&[ParamKind::Number, ParamKind::Any, ParamKind::Number, ParamKind::Any, ParamKind::Table, ParamKind::Any, ParamKind::Any]),
            overload(&[
                ParamKind::Number,
                ParamKind::Any,
                ParamKind::Number,
                ParamKind::Any,
                ParamKind::Table,
                ParamKind::Any,
                ParamKind::Any,
                ParamKind::Any,
            ]),
            overload(&[
                ParamKind::Number,
                ParamKind::Any,
                ParamKind::Number,
                ParamKind::Any,
                ParamKind::Table,
                ParamKind::Any,
                ParamKind::Any,
                ParamKind::Any,
                ParamKind::Any,
            ]),
            overload(&[
                ParamKind::Number,
                ParamKind::Any,
                ParamKind::Number,
                ParamKind::Any,
                ParamKind::Table,
                ParamKind::Any,
                ParamKind::Any,
                ParamKind::Any,
                ParamKind::Any,
                ParamKind::Any,
            ]),
        ],
        ReturnType::Table,
        INTERNAL,
        "Returns the rows between a start and an end boundary within each partition of the \
         relation, ordered by the ORDERBY clause. REL boundaries count rows from the current row \
         while ABS boundaries are absolute positions in the partition; the result is a table.",
        &["WINDOW ( 1, ABS, 1, ABS, Sales, ORDERBY ( Sales[OrderDate] ) )", "WINDOW ( -1, REL, 1, REL, Sales )"],
    ),
];
