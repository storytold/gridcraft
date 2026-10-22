//! Semantic catalog of the DAX aggregation functions.
//!
//! The catalog records the documented signature of every function in
//! <https://dax.guide/functions/aggregation/> — arity, parameter kinds, overloads, return type,
//! iterator behaviour, syntax-sugar relationships, blank handling and host-compatibility flags.
//! It is static metadata: the parser already accepts these calls, and [`crate::validate`] uses
//! the records to reject statically impossible arguments.
//!
//! Documentation conflicts (for example whether `COUNTBLANK` returns 0 when rows exist but no
//! cell is blank) are recorded as [`BlankPolicy::Unspecified`] instead of silently picking one
//! source. See the crate README for the per-function notes.

/// The function family. This PR ships the aggregation family only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FunctionCategory {
    Aggregation,
}

/// What an argument position accepts.
///
/// Only [`ParamKind::Column`] and [`ParamKind::Table`] can be checked without a data model;
/// the remaining kinds are recorded for documentation and future type checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamKind {
    /// A column reference such as `Sales[Amount]` or `[Amount]`.
    Column,
    /// A table expression such as `Sales`, `FILTER ( ... )` or `{ 1, 2 }`.
    Table,
    /// A scalar expression evaluated in the iterator's row context.
    Expression,
    /// A scalar expression evaluated in the caller's context.
    Scalar,
    /// A logical (Boolean) expression.
    Logical,
    /// Any expression.
    Any,
}

impl ParamKind {
    /// Human-readable description used in validation messages.
    pub const fn description(self) -> &'static str {
        match self {
            ParamKind::Column => "a column reference",
            ParamKind::Table => "a table expression",
            ParamKind::Expression => "an expression",
            ParamKind::Scalar => "a scalar expression",
            ParamKind::Logical => "a logical expression",
            ParamKind::Any => "any expression",
        }
    }
}

/// The documented result type. `Numeric` covers decimal/currency/integer results whose exact
/// type depends on the model, and `Any` covers functions whose result type follows the inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReturnType {
    Integer,
    Numeric,
    Currency,
    Decimal,
    Any,
    Boolean,
    DateTime,
    String,
}

/// What the function returns when there is nothing to aggregate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlankPolicy {
    /// BLANK when the input has no rows or no qualifying value.
    BlankWhenEmpty,
    /// 0 when rows exist but none match, as Microsoft Learn documents for some functions. The
    /// catalog does not currently use this variant: observed engine behaviour and dax.guide
    /// disagree with that rule for every function where it was considered.
    ZeroWhenRowsButNoMatch,
    /// Sources conflict or are silent; callers must not rely on either result.
    Unspecified,
}

impl BlankPolicy {
    /// Human-readable description used by documentation and tests.
    pub const fn description(self) -> &'static str {
        match self {
            BlankPolicy::BlankWhenEmpty => "BLANK when empty",
            BlankPolicy::ZeroWhenRowsButNoMatch => "0 when rows exist but none match",
            BlankPolicy::Unspecified => "unspecified",
        }
    }
}

/// Behaviours that change how a host or optimiser treats the function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionFlags {
    /// The function is an iterator: it evaluates a second argument once per row of a table.
    pub iterator: bool,
    /// dax.guide marks the function as not recommended.
    pub not_recommended: bool,
    /// The function is available to Excel's embedded DAX engine.
    pub excel_compatible: bool,
    /// Microsoft Learn documents DirectQuery restrictions (calculated columns / RLS rules).
    pub direct_query_limited: bool,
}

impl FunctionFlags {
    pub const fn new(iterator: bool, not_recommended: bool, excel_compatible: bool, direct_query_limited: bool) -> Self {
        FunctionFlags { iterator, not_recommended, excel_compatible, direct_query_limited }
    }
}

/// One argument in a descriptive signature, including whether it may be omitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Parameter {
    pub name: &'static str,
    pub kind: ParamKind,
    pub optional: bool,
}

const fn required(name: &'static str, kind: ParamKind) -> Parameter {
    Parameter { name, kind, optional: false }
}

const fn optional(name: &'static str, kind: ParamKind) -> Parameter {
    Parameter { name, kind, optional: true }
}

/// One exact arity and its parameter kinds. Overloads enumerate every accepted arity, including
/// zero-argument `COUNTROWS ()` and the optional `Variant` argument of `MAXX`/`MINX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Overload {
    pub params: &'static [ParamKind],
}

impl Overload {
    pub const fn arity(self) -> usize {
        self.params.len()
    }
}

const fn overload(params: &'static [ParamKind]) -> Overload {
    Overload { params }
}

/// A complete catalog record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionSpec {
    pub name: &'static str,
    pub category: FunctionCategory,
    pub min_args: usize,
    pub max_args: usize,
    /// Descriptive signature. Optional markers live here; [`FunctionSpec::overloads`] enumerates
    /// the concrete arity/kind combinations.
    pub params: &'static [Parameter],
    pub overloads: &'static [Overload],
    pub return_type: ReturnType,
    pub flags: FunctionFlags,
    /// The function this one is syntax sugar for, when the semantics are identical.
    pub sugar_of: Option<&'static str>,
    pub blank_policy: BlankPolicy,
    pub description: &'static str,
    pub examples: &'static [&'static str],
}

impl FunctionSpec {
    /// Whether the function evaluates an expression per row of a table.
    pub const fn is_iterator(&self) -> bool {
        self.flags.iterator
    }

    /// The overload accepting exactly `arity` arguments, if any.
    pub fn overload_for(&self, arity: usize) -> Option<&Overload> {
        self.overloads.iter().find(|overload| overload.arity() == arity)
    }
}

const fn standard(iterator: bool) -> FunctionFlags {
    FunctionFlags::new(iterator, false, true, true)
}

/// All aggregation functions, in the order listed by dax.guide.
pub static CATALOG: [FunctionSpec; 22] = [
    FunctionSpec {
        name: "APPROXIMATEDISTINCTCOUNT",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Integer,
        flags: FunctionFlags::new(false, false, false, true),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Approximates the number of distinct values in a column. DirectQuery models \
                      push the aggregation to the source; Import models use HyperLogLog with a \
                      small error rate. The argument must be a column reference.",
        examples: &["APPROXIMATEDISTINCTCOUNT ( Sales[ProductKey] )"],
    },
    FunctionSpec {
        name: "AVERAGE",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Numeric,
        flags: standard(false),
        sugar_of: Some("AVERAGEX"),
        blank_policy: BlankPolicy::Unspecified,
        description: "Arithmetic mean of a numeric column, ignoring blanks and including zero. \
                      Syntax sugar for AVERAGEX. dax.guide returns BLANK when no value qualifies, \
                      while Microsoft Learn documents 0 when rows exist; the catalog keeps the \
                      conflict explicit.",
        examples: &["AVERAGE ( Sales[Amount] )", "AVERAGE ( Sales[Quantity] )"],
    },
    FunctionSpec {
        name: "AVERAGEA",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Numeric,
        flags: FunctionFlags::new(false, true, true, true),
        sugar_of: None,
        blank_policy: BlankPolicy::Unspecified,
        description: "Averages a column, counting TRUE as 1, FALSE as 0 and any text as 0. \
                      Internally evaluated with AVERAGEX but not pure sugar because of those \
                      coercions; dax.guide marks it not recommended. Sources disagree on the \
                      empty/all-blank result.",
        examples: &["AVERAGEA ( Customer[LargeFamily] )", "AVERAGEA ( Sales[Flag] )"],
    },
    FunctionSpec {
        name: "AVERAGEX",
        category: FunctionCategory::Aggregation,
        min_args: 2,
        max_args: 2,
        params: &[required("Table", ParamKind::Table), required("Expression", ParamKind::Expression)],
        overloads: &[overload(&[ParamKind::Table, ParamKind::Expression])],
        return_type: ReturnType::Numeric,
        flags: standard(true),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Evaluates the expression once per row of the table and returns the mean of \
                      the non-blank results, including zeros. The expression runs in row context, \
                      so a measure reference triggers context transition.",
        examples: &["AVERAGEX ( Sales, Sales[Quantity] * Sales[Net Price] )", "AVERAGEX ( Customer, [Sales Amount] )"],
    },
    FunctionSpec {
        name: "COUNT",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Integer,
        flags: standard(false),
        sugar_of: Some("COUNTX"),
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Counts non-blank numbers, dates and strings in a column; empty strings count \
                      and booleans do not. Syntax sugar for COUNTX.",
        examples: &["COUNT ( Customer[Customer Name] )", "COUNT ( Sales[Text] )"],
    },
    FunctionSpec {
        name: "COUNTA",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Integer,
        flags: standard(false),
        sugar_of: Some("COUNTAX"),
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Counts non-blank values in a column, including booleans (as values, not as \
                      1/0). Syntax sugar for COUNTAX.",
        examples: &["COUNTA ( Customer[Customer Name] )", "COUNTA ( Sales[Flag] )"],
    },
    FunctionSpec {
        name: "COUNTAX",
        category: FunctionCategory::Aggregation,
        min_args: 2,
        max_args: 2,
        params: &[required("Table", ParamKind::Table), required("Expression", ParamKind::Expression)],
        overloads: &[overload(&[ParamKind::Table, ParamKind::Expression])],
        return_type: ReturnType::Integer,
        flags: standard(true),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Evaluates the expression per row and counts non-blank results, including \
                      booleans.",
        examples: &["COUNTAX ( Customer, Customer[LargeFamily] )"],
    },
    FunctionSpec {
        name: "COUNTBLANK",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Integer,
        flags: standard(false),
        sugar_of: None,
        blank_policy: BlankPolicy::Unspecified,
        description: "Counts blanks in a column of any type; the empty string counts as blank and \
                      zero does not. dax.guide says the function never returns 0, while Microsoft \
                      Learn documents 0 when rows exist but no value is blank.",
        examples: &["COUNTBLANK ( Customer[Customer Name] )", "COUNTBLANK ( Sales[Text] )"],
    },
    FunctionSpec {
        name: "COUNTROWS",
        category: FunctionCategory::Aggregation,
        min_args: 0,
        max_args: 1,
        params: &[optional("Table", ParamKind::Table)],
        overloads: &[overload(&[]), overload(&[ParamKind::Table])],
        return_type: ReturnType::Integer,
        flags: standard(false),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Counts the rows of a table expression. With no argument it counts the home \
                      table of the measure or calculated column. Excel's embedded engine accepts \
                      the one-argument form only.",
        examples: &["COUNTROWS ( Sales )", "COUNTROWS ( DISTINCT ( Sales[Region] ) )", "COUNTROWS ( )"],
    },
    FunctionSpec {
        name: "COUNTX",
        category: FunctionCategory::Aggregation,
        min_args: 2,
        max_args: 2,
        params: &[required("Table", ParamKind::Table), required("Expression", ParamKind::Expression)],
        overloads: &[overload(&[ParamKind::Table, ParamKind::Expression])],
        return_type: ReturnType::Integer,
        flags: standard(true),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Evaluates the expression per row and counts non-blank results of type number, \
                      date or string; booleans are not counted (use COUNTAX).",
        examples: &["COUNTX ( Customer, DIVIDE ( Customer[Total Children], Customer[Cars Owned] ) )", "COUNTX ( Sales, Sales[Quantity] )"],
    },
    FunctionSpec {
        name: "DISTINCTCOUNT",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Integer,
        flags: standard(false),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Counts distinct values in a column of any type. BLANK is one distinct value; \
                      totals are not additive. Equivalent to COUNTROWS ( DISTINCT ( table[column] ) ).",
        examples: &["DISTINCTCOUNT ( Store[Area Manager] )", "DISTINCTCOUNT ( Sales[Region] )"],
    },
    FunctionSpec {
        name: "DISTINCTCOUNTNOBLANK",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Integer,
        flags: FunctionFlags::new(false, false, false, true),
        sugar_of: Some("DISTINCTCOUNT"),
        blank_policy: BlankPolicy::Unspecified,
        description: "Counts distinct values in a column, excluding BLANK. Syntax sugar for \
                      DISTINCTCOUNT with an ISBLANK filter. Not available to Excel's embedded engine.",
        examples: &["DISTINCTCOUNTNOBLANK ( Store[Area Manager] )"],
    },
    FunctionSpec {
        name: "MAX",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 2,
        params: &[required("ColumnNameOrScalar1", ParamKind::Any), optional("Scalar2", ParamKind::Scalar)],
        overloads: &[overload(&[ParamKind::Column]), overload(&[ParamKind::Scalar, ParamKind::Scalar])],
        return_type: ReturnType::Any,
        flags: standard(false),
        sugar_of: Some("MAXX"),
        blank_policy: BlankPolicy::Unspecified,
        description: "With one argument, the largest non-blank value of a column (internally \
                      MAXX); text compares alphabetically and logical values are unsupported. With \
                      two arguments, the larger scalar, where BLANK is significant per dax.guide \
                      but treated as zero by Microsoft Learn.",
        examples: &["MAX ( Sales[Net Price] )", "MAX ( 10000, [Sales Amount] )", "MAX ( 3, 5 )"],
    },
    FunctionSpec {
        name: "MAXA",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Any,
        flags: standard(false),
        sugar_of: None,
        blank_policy: BlankPolicy::Unspecified,
        description: "Largest value of a column, counting TRUE as 1 and FALSE as 0 instead of \
                      ignoring logical values. Internally MAXX. Sources disagree on the empty \
                      result; text is not rankable.",
        examples: &["MAXA ( Customer[LargeFamily] )", "MAXA ( Sales[Flag] )"],
    },
    FunctionSpec {
        name: "MAXX",
        category: FunctionCategory::Aggregation,
        min_args: 2,
        max_args: 3,
        params: &[required("Table", ParamKind::Table), required("Expression", ParamKind::Expression), optional("Variant", ParamKind::Logical)],
        overloads: &[overload(&[ParamKind::Table, ParamKind::Expression]), overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Logical])],
        return_type: ReturnType::Any,
        flags: standard(true),
        sugar_of: None,
        blank_policy: BlankPolicy::Unspecified,
        description: "Evaluates the expression per row and returns the largest value, skipping \
                      blanks and ignoring logical values. Mixed-type expressions consider numbers \
                      only unless Variant is TRUE, which compares per ORDER BY DESC.",
        examples: &["MAXX ( Customer, 1 * Customer[LargeFamily] )", "MAXX ( Sales, Sales[Amount], TRUE )"],
    },
    FunctionSpec {
        name: "MIN",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 2,
        params: &[required("ColumnNameOrScalar1", ParamKind::Any), optional("Scalar2", ParamKind::Scalar)],
        overloads: &[overload(&[ParamKind::Column]), overload(&[ParamKind::Scalar, ParamKind::Scalar])],
        return_type: ReturnType::Any,
        flags: standard(false),
        sugar_of: Some("MINX"),
        blank_policy: BlankPolicy::Unspecified,
        description: "With one argument, the smallest non-blank value of a column (internally \
                      MINX); text compares alphabetically and logical values are unsupported. With \
                      two arguments, the smaller scalar, where BLANK is significant per dax.guide \
                      but treated as zero by Microsoft Learn.",
        examples: &["MIN ( Sales[Net Price] )", "MIN ( 5000000, [Sales Amount] )", "MIN ( 3, 5 )"],
    },
    FunctionSpec {
        name: "MINA",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Any,
        flags: standard(false),
        sugar_of: None,
        blank_policy: BlankPolicy::Unspecified,
        description: "Smallest value of a column, counting TRUE as 1 and FALSE as 0 instead of \
                      ignoring logical values. Internally MINX. Sources disagree on the empty \
                      result.",
        examples: &["MINA ( Customer[LargeFamily] )", "MINA ( Sales[Flag] )"],
    },
    FunctionSpec {
        name: "MINX",
        category: FunctionCategory::Aggregation,
        min_args: 2,
        max_args: 3,
        params: &[required("Table", ParamKind::Table), required("Expression", ParamKind::Expression), optional("Variant", ParamKind::Logical)],
        overloads: &[overload(&[ParamKind::Table, ParamKind::Expression]), overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Logical])],
        return_type: ReturnType::Any,
        flags: standard(true),
        sugar_of: None,
        blank_policy: BlankPolicy::Unspecified,
        description: "Evaluates the expression per row and returns the smallest value, skipping \
                      blanks and ignoring logical values. Mixed-type expressions consider numbers \
                      only unless Variant is TRUE, which compares per ORDER BY ASC.",
        examples: &["MINX ( Customer, 1 * Customer[NoCars] )", "MINX ( Sales, Sales[Amount], FALSE )"],
    },
    FunctionSpec {
        name: "PRODUCT",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Decimal,
        flags: standard(false),
        sugar_of: Some("PRODUCTX"),
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Multiplies the numbers in a column, ignoring blanks, logical values and text. \
                      Syntax sugar for PRODUCTX.",
        examples: &["PRODUCT ( Sales[Quantity] )"],
    },
    FunctionSpec {
        name: "PRODUCTX",
        category: FunctionCategory::Aggregation,
        min_args: 2,
        max_args: 2,
        params: &[required("Table", ParamKind::Table), required("Expression", ParamKind::Expression)],
        overloads: &[overload(&[ParamKind::Table, ParamKind::Expression])],
        return_type: ReturnType::Decimal,
        flags: standard(true),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Evaluates the expression per row and multiplies the non-blank numeric \
                      results; blanks, logical values and text are ignored, unlike the `*` \
                      operator.",
        examples: &["PRODUCTX ( Rates, 1 + Rates[InflationRate] )", "PRODUCTX ( Sales, Sales[Quantity] )"],
    },
    FunctionSpec {
        name: "SUM",
        category: FunctionCategory::Aggregation,
        min_args: 1,
        max_args: 1,
        params: &[required("ColumnName", ParamKind::Column)],
        overloads: &[overload(&[ParamKind::Column])],
        return_type: ReturnType::Numeric,
        flags: standard(false),
        sugar_of: Some("SUMX"),
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Adds the numbers in a column, ignoring blanks, logical values and text. \
                      Syntax sugar for SUMX. dax.guide says the result type follows the column, \
                      while Microsoft Learn documents a decimal number.",
        examples: &["SUM ( Sales[Amount] )", "SUM ( Sales[Quantity] )"],
    },
    FunctionSpec {
        name: "SUMX",
        category: FunctionCategory::Aggregation,
        min_args: 2,
        max_args: 2,
        params: &[required("Table", ParamKind::Table), required("Expression", ParamKind::Expression)],
        overloads: &[overload(&[ParamKind::Table, ParamKind::Expression])],
        return_type: ReturnType::Numeric,
        flags: standard(true),
        sugar_of: None,
        blank_policy: BlankPolicy::BlankWhenEmpty,
        description: "Evaluates the expression once per row of the table and sums the numeric \
                      results; blanks, logical values and text are ignored. The expression runs \
                      in row context.",
        examples: &[
            "SUMX ( Sales, Sales[Quantity] * Sales[Net Price] )",
            "SUMX ( FILTER ( InternetSales, InternetSales[SalesTerritoryID] = 5 ), [Freight] )",
        ],
    },
];

/// Every aggregation function, in dax.guide order.
pub fn catalog() -> &'static [FunctionSpec] {
    &CATALOG
}

/// Looks a function up by name, ignoring ASCII case.
pub fn lookup(name: &str) -> Option<&'static FunctionSpec> {
    CATALOG.iter().find(|spec| spec.name.eq_ignore_ascii_case(name))
}
