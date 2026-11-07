//! DAX statement grammar: the 17 dax.guide statements, placement rules and printer round-trips.
//!
//! Statements covered: `EVALUATE`, `DEFINE`, `MEASURE`, `COLUMN`, `TABLE`, `VAR`, `RETURN`,
//! `ORDER BY`, `START AT`, `FUNCTION`, `MPARAMETER`, `WITH VISUAL SHAPE`, `AXIS ROWS`,
//! `AXIS COLUMNS`, `GROUP`, `TOTAL` and `DENSIFY` (https://dax.guide/statements/).

use gridcraft_dax::{
    AxisKind, Definition, ExprKind, Literal, ParameterSubtype, ParameterType, PassingMode, Query, SortDirection, StartValue, parse_expression,
    parse_query, print_expression, print_query,
};

/// Parses `src`, prints it and reparses the printed text. Asserts the tree survives unchanged.
fn round_trip(src: &str) -> Query {
    let query = parse_query(src).unwrap_or_else(|error| panic!("{src}\n{error}"));
    let printed = print_query(&query);
    let reparsed = parse_query(&printed).unwrap_or_else(|error| panic!("printed text failed to parse:\n{printed}\n{error}"));
    assert_eq!(reparsed, query, "round-trip changed the tree for:\n{printed}");
    query
}

fn error_message(src: &str) -> String {
    match parse_query(src) {
        Ok(query) => panic!("expected a query error for {src:?}, parsed {query:#?}"),
        Err(error) => error.message,
    }
}

// ---------------------------------------------------------------------------------------------
// EVALUATE
// ---------------------------------------------------------------------------------------------

#[test]
fn evaluate_keeps_the_simple_form() {
    let query = round_trip("EVALUATE SUM('Sales'[Amount])");
    assert!(query.define.is_none());
    assert_eq!(query.evaluates.len(), 1);
    assert!(query.evaluates[0].order_by.is_empty());
    assert!(query.evaluates[0].start_at.is_empty());
    assert_eq!(print_query(&query), "EVALUATE SUM('Sales'[Amount])");

    assert_eq!(print_query(&parse_query("evaluate {1, 2}").unwrap()), "EVALUATE { 1, 2 }");
}

#[test]
fn multiple_evaluate_statements_are_separated_by_keywords_only() {
    let query = round_trip("EVALUATE Sales EVALUATE Customer EVALUATE { 1 }");
    assert_eq!(query.evaluates.len(), 3);
    assert_eq!(print_query(&query), "EVALUATE Sales\nEVALUATE Customer\nEVALUATE { 1 }");
}

// ---------------------------------------------------------------------------------------------
// ORDER BY / START AT
// ---------------------------------------------------------------------------------------------

#[test]
fn order_by_takes_expressions_and_optional_directions() {
    let query = round_trip("EVALUATE ALL('Product'[Brand], 'Product'[Color]) ORDER BY 'Product'[Brand], 'Product'[Color] DESC");
    let evaluate = &query.evaluates[0];
    assert_eq!(evaluate.order_by.len(), 2);
    assert_eq!(evaluate.order_by[0].direction, SortDirection::Asc);
    assert_eq!(evaluate.order_by[1].direction, SortDirection::Desc);
    assert_eq!(
        print_query(&query),
        "EVALUATE ALL('Product'[Brand], 'Product'[Color])\nORDER BY\n    'Product'[Brand] ASC,\n    'Product'[Color] DESC"
    );
}

#[test]
fn start_at_accepts_literals_and_parameters() {
    let query =
        round_trip("EVALUATE ALL('Product'[Brand], 'Product'[Color]) ORDER BY 'Product'[Brand], 'Product'[Color] START AT \"Proseware\", @Color");
    let evaluate = &query.evaluates[0];
    assert_eq!(evaluate.start_at.len(), 2);
    assert_eq!(evaluate.start_at[0], StartValue::Literal(Literal::String("Proseware".into())));
    match &evaluate.start_at[1] {
        StartValue::Parameter(name) => assert_eq!(name.text, "Color"),
        other => panic!("expected a parameter, found {other:?}"),
    }
    assert!(print_query(&query).contains("START AT \"Proseware\", @Color"));
}

#[test]
fn start_at_literals_include_numbers_booleans_and_datetimes() {
    let query = round_trip("EVALUATE T ORDER BY [a], [b], [c] START AT -1.5, TRUE, dt\"2023-05-19\"");
    assert_eq!(query.evaluates[0].start_at[0], StartValue::Literal(Literal::Decimal(-1.5)));
    assert_eq!(query.evaluates[0].start_at[1], StartValue::Literal(Literal::Boolean(true)));
    assert!(print_query(&query).contains("START AT -1.5, TRUE, dt\"2023-05-19\""));
}

// ---------------------------------------------------------------------------------------------
// DEFINE block and its entry kinds
// ---------------------------------------------------------------------------------------------

#[test]
fn define_measure_column_and_table() {
    let query = round_trip(
        "DEFINE MEASURE Sales[Sales Amount] = SUMX(Sales, Sales[Quantity] * Sales[Net Price]) \
         COLUMN Customer[First Letter] = LEFT(Customer[Name], 1) \
         TABLE Colors = DISTINCT('Product'[Color]) \
         EVALUATE SUMMARIZECOLUMNS(Customer[First Letter], \"Sales Amount\", [Sales Amount])",
    );
    let items = &query.define.as_ref().unwrap().items;
    assert_eq!(items.len(), 3);
    match &items[0] {
        Definition::Measure(def) => {
            assert_eq!(def.name.name.text, "Sales Amount");
            assert_eq!(def.name.table.as_ref().unwrap().text, "Sales");
        }
        other => panic!("expected a measure, found {other:?}"),
    }
    match &items[1] {
        Definition::Column(def) => assert_eq!(def.name.name.text, "First Letter"),
        other => panic!("expected a column, found {other:?}"),
    }
    match &items[2] {
        Definition::Table(def) => {
            assert_eq!(def.name.text, "Colors");
            assert!(def.visual_shape.is_none());
        }
        other => panic!("expected a table, found {other:?}"),
    }
    assert!(print_query(&query).starts_with("DEFINE\n    MEASURE Sales[Sales Amount] ="));
}

#[test]
fn define_var_is_query_scoped_and_has_no_return() {
    let query = round_trip("DEFINE VAR LastSalesDate = MAX(Sales[Order Date]) VAR Limit = 10 EVALUATE Sales");
    let items = &query.define.as_ref().unwrap().items;
    assert_eq!(items.len(), 2);
    match &items[0] {
        Definition::Var(def) => assert_eq!(def.name.text, "LastSalesDate"),
        other => panic!("expected a var, found {other:?}"),
    }
    // Definitions may reference each other in either direction, and duplicate names are allowed.
    round_trip("DEFINE VAR A = B VAR B = 1 VAR A = 2 EVALUATE A");
}

#[test]
fn define_function_takes_parameter_types_subtypes_modes_and_defaults() {
    let query = round_trip(
        "DEFINE FUNCTION SumTwoNumbers = ( a : INT64 VAL, b : SCALAR INT64 VAL = 2, c, d : TABLE, e : EXPR ) => a + b \
         EVALUATE { SumTwoNumbers(3, 2) }",
    );
    let items = &query.define.as_ref().unwrap().items;
    let Definition::Function(def) = &items[0] else { panic!("expected a function, found {:?}", items[0]) };
    assert_eq!(def.name.text, "SumTwoNumbers");
    assert_eq!(def.params.len(), 5);
    assert_eq!(def.params[0].ty, None);
    assert_eq!(def.params[0].subtype, Some(ParameterSubtype::Int64));
    assert_eq!(def.params[0].passing, Some(PassingMode::Val));
    assert_eq!(def.params[1].ty, Some(ParameterType::Scalar));
    assert_eq!(def.params[1].subtype, Some(ParameterSubtype::Int64));
    assert!(def.params[1].default.is_some());
    assert!(def.params[2].ty.is_none() && def.params[2].default.is_none());
    assert_eq!(def.params[3].ty, Some(ParameterType::Table));
    assert_eq!(def.params[4].passing, Some(PassingMode::Expr));
    assert!(print_query(&query).contains("FUNCTION SumTwoNumbers = (a : INT64 VAL, b : SCALAR INT64 VAL = 2, c, d : TABLE, e : EXPR) => a + b"));
}

#[test]
fn function_definitions_accept_empty_parameter_lists_and_dotted_names() {
    let query = round_trip("DEFINE FUNCTION My.Library.NoArgs = () => 42 EVALUATE { My.Library.NoArgs() }");
    let Definition::Function(def) = &query.define.as_ref().unwrap().items[0] else { panic!("expected a function") };
    assert_eq!(def.name.text, "My.Library.NoArgs");
    assert!(def.params.is_empty());
}

#[test]
fn mparameter_accepts_scalar_and_table_values() {
    let query = round_trip(
        "DEFINE MPARAMETER DateParameter = DATE(2022, 5, 23) \
         MPARAMETER 'Country Parameter' = \"Canada\" \
         MPARAMETER BrandParameter = { \"Contoso\", \"Fabrikam\" } \
         EVALUATE SUMMARIZECOLUMNS('Date'[Date], \"Sales\", [Sales Amount])",
    );
    let items = &query.define.as_ref().unwrap().items;
    assert_eq!(items.len(), 3);
    match &items[1] {
        Definition::MParameter(def) => assert_eq!(def.name.text, "Country Parameter"),
        other => panic!("expected an M parameter, found {other:?}"),
    }
    match &items[2] {
        Definition::MParameter(def) => assert!(matches!(def.expr.kind, ExprKind::Table(_))),
        other => panic!("expected an M parameter, found {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// VAR / RETURN (expression level)
// ---------------------------------------------------------------------------------------------

#[test]
fn expression_var_return_parses_and_prints() {
    let expr = parse_expression("VAR Sales = SUM(Sales[Amount]) VAR Target = 100 RETURN DIVIDE(Sales, Target)").unwrap();
    match &expr.kind {
        ExprKind::VarReturn { vars, .. } => {
            assert_eq!(vars.len(), 2);
            assert_eq!(vars[0].name.text, "Sales");
        }
        other => panic!("expected a var-return, found {other:?}"),
    }
    let printed = print_expression(&expr);
    assert_eq!(printed, "VAR Sales = SUM(Sales[Amount]) VAR Target = 100 RETURN DIVIDE(Sales, Target)");
    assert_eq!(parse_expression(&printed).unwrap(), expr);
}

#[test]
fn expression_var_return_nests_and_accepts_parentheses() {
    let flat = parse_expression("VAR x = 1 VAR y = VAR z = x + 1 RETURN z RETURN y").unwrap();
    let printed = print_expression(&flat);
    assert_eq!(parse_expression(&printed).unwrap(), flat);

    // Parentheses around a VAR/RETURN block are grouping, as in Power BI-generated COLUMN code.
    let parenthesized = parse_expression("(VAR n = 1 RETURN n) + 1").unwrap();
    assert!(matches!(parenthesized.kind, ExprKind::Binary { .. }));
    assert_eq!(print_expression(&parenthesized), "(VAR n = 1 RETURN n) + 1");

    // As a call argument, no parentheses are needed for the block itself.
    let argument = parse_expression("FILTER(T, VAR x = 1 RETURN x)").unwrap();
    assert_eq!(print_expression(&argument), "FILTER(T, VAR x = 1 RETURN x)");
    assert_eq!(parse_expression(&print_expression(&argument)).unwrap(), argument);
}

#[test]
fn define_column_accepts_a_parenthesized_var_return() {
    round_trip(
        "DEFINE COLUMN Customer[First Letter] = (VAR name = Customer[Name] RETURN LEFT(name, 1)) \
         EVALUATE SUMMARIZECOLUMNS(Customer[First Letter])",
    );
}

#[test]
fn return_without_var_and_var_without_return_are_rejected() {
    assert!(error_message("EVALUATE RETURN 1").contains("RETURN without a preceding VAR"));
    assert!(error_message("EVALUATE VAR x = 1").contains("must be followed by RETURN"));
    assert!(error_message("DEFINE MEASURE Sales[M] = RETURN 1 EVALUATE Sales").contains("RETURN without a preceding VAR"));
    assert!(parse_expression("RETURN 1").is_err());
    assert!(parse_expression("VAR x = 1").is_err());
}

// ---------------------------------------------------------------------------------------------
// WITH VISUAL SHAPE: AXIS ROWS / AXIS COLUMNS / GROUP / TOTAL / DENSIFY
// ---------------------------------------------------------------------------------------------

const SHAPE_QUERY: &str = "DEFINE TABLE '__DS0VisualCalcs' = __DS0VisualCalcsInput \
    WITH VISUAL SHAPE \
    AXIS ROWS \
        GROUP [Brand] TOTAL [IsGrandTotalRowTotal] \
        GROUP [Category], [Category_Code] TOTAL [IsDM1Total] \
        ORDER BY [Brand] ASC, [Category_Code] ASC \
    AXIS COLUMNS \
        GROUP [Year] TOTAL [IsGrandTotalColumnTotal] \
        GROUP [Month], [Month_Number] TOTAL [IsDM6Total] \
        ORDER BY [Year] ASC, [Month_Number] ASC, [Month] ASC \
    DENSIFY \"IsDensifiedRow\" \
    EVALUATE '__DS0VisualCalcs'";

#[test]
fn visual_shape_with_both_axes_groups_totals_order_and_densify() {
    let query = round_trip(SHAPE_QUERY);
    let Definition::Table(table) = &query.define.as_ref().unwrap().items[0] else { panic!("expected a table") };
    let shape = table.visual_shape.as_ref().unwrap();
    assert_eq!(shape.axes.len(), 2);
    assert_eq!(shape.axes[0].kind, AxisKind::Rows);
    assert_eq!(shape.axes[1].kind, AxisKind::Columns);
    assert_eq!(shape.axes[0].groups.len(), 2);
    assert_eq!(shape.axes[0].groups[1].columns.len(), 2);
    assert_eq!(shape.axes[1].groups.len(), 2);
    assert_eq!(shape.axes[0].order_by.len(), 2);
    assert_eq!(shape.axes[1].order_by.len(), 3);
    assert_eq!(shape.densify.as_deref(), Some("IsDensifiedRow"));

    let printed = print_query(&query);
    assert!(printed.contains("    TABLE '__DS0VisualCalcs' = __DS0VisualCalcsInput WITH VISUAL SHAPE"));
    assert!(printed.contains("\n        AXIS ROWS\n            GROUP [Brand] TOTAL [IsGrandTotalRowTotal]"));
    assert!(printed.contains("\n        AXIS COLUMNS"));
    assert!(printed.ends_with("\n        DENSIFY \"IsDensifiedRow\"\nEVALUATE '__DS0VisualCalcs'"));
}

#[test]
fn visual_shape_accepts_one_axis_and_omitted_densify() {
    let query = round_trip("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS COLUMNS GROUP [Year] TOTAL [IsTotal] ORDER BY [Year] EVALUATE T");
    let Definition::Table(table) = &query.define.as_ref().unwrap().items[0] else { panic!("expected a table") };
    let shape = table.visual_shape.as_ref().unwrap();
    assert_eq!(shape.axes.len(), 1);
    assert!(shape.densify.is_none());
    // A direction may be omitted on an axis; it defaults to ASC and prints explicitly.
    assert_eq!(shape.axes[0].order_by[0].direction, SortDirection::Asc);
    assert!(print_query(&query).contains("                [Year] ASC"));
}

// ---------------------------------------------------------------------------------------------
// Whole query
// ---------------------------------------------------------------------------------------------

#[test]
fn full_query_round_trips_and_keeps_every_clause() {
    let source = "DEFINE \
        MEASURE Sales[Sales Amount] = SUMX ( Sales, Sales[Quantity] * Sales[Net Price] ) \
        MEASURE Sales[Margin] = [Sales Amount] - [Total Cost] \
        COLUMN Customer[First Letter] = LEFT ( Customer[Name], 1 ) \
        TABLE Colors = DISTINCT ( 'Product'[Color] ) \
        VAR MinimumAmount = 2000000 \
        VAR MaxAmount = VAR x = 8000000 RETURN x \
        FUNCTION SumTwoNumbers = ( a : INT64 VAL, b : INT64 VAL = 0 ) => a + b \
        MPARAMETER 'Country Parameter' = \"Canada\" \
        MPARAMETER BrandParameter = { \"Contoso\", \"Fabrikam\" } \
        EVALUATE SUMMARIZECOLUMNS ( Customer[CountryRegion], \"Sales Amount\", [Sales Amount] ) \
            ORDER BY [CountryRegion] ASC, [Sales Amount] DESC \
            START AT \"US\", 1000 \
        EVALUATE Colors";
    let query = round_trip(source);
    let items = &query.define.as_ref().unwrap().items;
    assert_eq!(items.len(), 9);
    assert_eq!(query.evaluates.len(), 2);
    assert_eq!(query.evaluates[0].order_by.len(), 2);
    assert_eq!(query.evaluates[0].start_at.len(), 2);
    assert!(query.evaluates[1].order_by.is_empty());

    let printed = print_query(&query);
    assert_eq!(
        printed,
        "DEFINE\n\
         \x20   MEASURE Sales[Sales Amount] = SUMX(Sales, Sales[Quantity] * Sales[Net Price])\n\
         \x20   MEASURE Sales[Margin] = [Sales Amount] - [Total Cost]\n\
         \x20   COLUMN Customer[First Letter] = LEFT(Customer[Name], 1)\n\
         \x20   TABLE Colors = DISTINCT('Product'[Color])\n\
         \x20   VAR MinimumAmount = 2000000\n\
         \x20   VAR MaxAmount = VAR x = 8000000 RETURN x\n\
         \x20   FUNCTION SumTwoNumbers = (a : INT64 VAL, b : INT64 VAL = 0) => a + b\n\
         \x20   MPARAMETER 'Country Parameter' = \"Canada\"\n\
         \x20   MPARAMETER BrandParameter = { \"Contoso\", \"Fabrikam\" }\n\
         EVALUATE SUMMARIZECOLUMNS(Customer[CountryRegion], \"Sales Amount\", [Sales Amount])\n\
         ORDER BY\n\
         \x20   [CountryRegion] ASC,\n\
         \x20   [Sales Amount] DESC\n\
         START AT \"US\", 1000\n\
         EVALUATE Colors"
    );
}

// ---------------------------------------------------------------------------------------------
// Lexical behaviour
// ---------------------------------------------------------------------------------------------

#[test]
fn statement_keywords_are_case_insensitive() {
    let query = round_trip(
        "define measure Sales[Amount] = 1 var X = 2 function F = (a : int64 val = 2) => a + X \
         mparameter P = 1 table T = X with visual shape axis rows group [a] total [t] order by [a] desc densify \"D\" \
         evaluate { F(1) } order by [x] asc start at @p",
    );
    assert_eq!(query.define.as_ref().unwrap().items.len(), 5);
    assert_eq!(query.evaluates[0].order_by[0].direction, SortDirection::Asc);
    // Canonical output writes keywords in uppercase.
    let printed = print_query(&query);
    assert!(printed.starts_with("DEFINE\n    MEASURE Sales[Amount] = 1\n    VAR X = 2"));
    assert!(printed.contains("AXIS ROWS"));
    assert!(printed.contains("DENSIFY \"D\""));
}

#[test]
fn quoted_and_bracketed_names_keep_their_style() {
    let query = round_trip(
        "DEFINE MEASURE 'Sales Table'[It]]s Amount] = 1 TABLE 'Unbought products' = X \
         EVALUATE 'Unbought products' ORDER BY 'Sales Table'[It]]s Amount]",
    );
    let items = &query.define.as_ref().unwrap().items;
    let Definition::Measure(measure) = &items[0] else { panic!("expected a measure") };
    assert_eq!(measure.name.table.as_ref().unwrap().text, "Sales Table");
    assert_eq!(measure.name.name.text, "It]s Amount");
    let printed = print_query(&query);
    assert!(printed.contains("MEASURE 'Sales Table'[It]]s Amount] = 1"));
    assert!(printed.contains("TABLE 'Unbought products' = X"));
    assert!(printed.contains("EVALUATE 'Unbought products'"));
}

#[test]
fn comments_and_whitespace_do_not_separate_statements() {
    let query = round_trip(
        "/* DEFINE */ DEFINE // one measure\n MEASURE Sales[M] = 1 -- another\n MEASURE Sales[N] = 2\n\
         EVALUATE\nSales\n// before evaluate\nEVALUATE {1};",
    );
    assert_eq!(query.define.as_ref().unwrap().items.len(), 2);
    assert_eq!(query.evaluates.len(), 2);
}

#[test]
fn optional_trailing_semicolon_is_accepted() {
    let query = round_trip("EVALUATE Sales;");
    assert_eq!(print_query(&query), "EVALUATE Sales");
    assert!(parse_query("EVALUATE Sales;;").is_ok());
}

// ---------------------------------------------------------------------------------------------
// Invalid placements
// ---------------------------------------------------------------------------------------------

#[test]
fn define_requires_a_following_evaluate() {
    assert!(error_message("DEFINE MEASURE Sales[M] = 1").contains("at least one `EVALUATE`"));
    assert!(error_message("DEFINE VAR x = 1").contains("at least one `EVALUATE`"));
    assert!(error_message("DEFINE").contains("DEFINE needs at least one definition"));
    assert!(error_message("DEFINE EVALUATE Sales").contains("DEFINE needs at least one definition"));
}

#[test]
fn definitions_are_only_allowed_inside_define_and_before_evaluate() {
    assert!(error_message("EVALUATE MEASURE Sales[M] = 1").contains("unexpected"));
    assert!(error_message("EVALUATE T DEFINE MEASURE Sales[M] = 1").contains("unexpected `DEFINE`"));
    assert!(error_message("MEASURE Sales[M] = 1 EVALUATE Sales").contains("at least one `EVALUATE`"));
    assert!(error_message("DEFINE MEASURE Sales[M] = 1 DEFINE MEASURE Sales[N] = 2 EVALUATE Sales").contains("DEFINE entry"));
}

#[test]
fn start_at_requires_order_by_and_at_most_one_value_per_item() {
    assert!(error_message("EVALUATE Sales START AT 1").contains("START AT requires a preceding ORDER BY"));
    assert!(error_message("EVALUATE Sales ORDER BY [a] START AT 1, 2").contains("START AT has 2 value(s)"));
    // START AT values are literals or @parameters, not expressions.
    assert!(parse_query("EVALUATE Sales ORDER BY [a] START AT 1 + 1").is_err());
    assert!(parse_query("EVALUATE Sales ORDER BY [a] START AT [b]").is_err());
}

#[test]
fn order_by_and_start_at_are_not_axes_and_vice_versa() {
    assert!(error_message("EVALUATE Sales ORDER BY [a] ORDER BY [b]").contains("unexpected"));
    assert!(
        error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] ORDER BY [a] START AT 1 EVALUATE T")
            .contains("DEFINE entry")
    );
    assert!(
        error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] ORDER BY [a] ORDER BY [b] EVALUATE T")
            .contains("DEFINE entry")
    );
}

#[test]
fn visual_shape_requires_table_and_valid_axis_clauses() {
    assert!(error_message("EVALUATE X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] ORDER BY [a]").contains("unexpected `WITH`"));
    assert!(error_message("DEFINE VAR T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] ORDER BY [a] EVALUATE T").contains("DEFINE entry"));
    assert!(
        error_message(
            "DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] ORDER BY [a] AXIS ROWS GROUP [b] TOTAL [u] ORDER BY [b] EVALUATE T"
        )
        .contains("duplicate AXIS ROWS")
    );
    assert!(error_message("DEFINE TABLE T = X WITH VISUAL SHAPE DENSIFY \"d\" EVALUATE T").contains("at least one AXIS"));
    assert!(
        error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWPAGES GROUP [a] TOTAL [t] ORDER BY [a] EVALUATE T")
            .contains("expected `ROWS` or `COLUMNS`")
    );
    assert!(error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] ORDER BY [a] EVALUATE T").contains("TOTAL"));
    assert!(error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] EVALUATE T").contains("ORDER BY"));
    assert!(error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS ORDER BY [a] EVALUATE T").contains("GROUP"));
}

#[test]
fn densify_is_last_and_takes_a_string() {
    assert!(error_message("DEFINE TABLE T = X DENSIFY \"d\" EVALUATE T").contains("DEFINE entry"));
    assert!(error_message("EVALUATE DENSIFY \"d\"").contains("unexpected"));
    assert!(
        error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] ORDER BY [a] DENSIFY \"d\" DENSIFY \"e\" EVALUATE T")
            .contains("DEFINE entry")
    );
    assert!(
        error_message("DEFINE TABLE T = X WITH VISUAL SHAPE AXIS ROWS GROUP [a] TOTAL [t] ORDER BY [a] DENSIFY [d] EVALUATE T")
            .contains("double-quoted")
    );
}

#[test]
fn variable_names_cannot_be_delimited_and_parameter_types_are_checked() {
    assert!(error_message("DEFINE VAR 'x' = 1 EVALUATE T").contains("cannot be quoted"));
    assert!(error_message("DEFINE FUNCTION F = (a : FOO) => a EVALUATE T").contains("unknown function parameter type"));
    assert!(error_message("DEFINE FUNCTION F = (a : TABLE INT64) => a EVALUATE T").contains("only SCALAR"));
    assert!(error_message("DEFINE MEASURE T = 1 EVALUATE T").contains("bracketed measure name"));
}
