//! Evaluator tests for the filter family: window/relation navigation, lookups and the
//! analysis-services apply/cluster forms.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, FilterContext, ScalarKey, bind_formula, parse_expression};

fn number_column(name: &str) -> ModelColumn {
    ModelColumn { name: name.into(), data_type: Some("Double".into()) }
}

fn model() -> DataModel {
    let products = ModelTable {
        name: "Products".into(),
        columns: vec![
            ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
            ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
            ModelColumn { name: "Color".into(), data_type: Some("String".into()) },
            ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
            number_column("Price"),
        ],
        rows: vec![
            vec![Scalar::Integer(1), Scalar::String("Widget".into()), Scalar::String("Red".into()), Scalar::String("A".into()), Scalar::Integer(10)],
            vec![Scalar::Integer(2), Scalar::String("Gadget".into()), Scalar::String("Blue".into()), Scalar::String("A".into()), Scalar::Integer(20)],
            vec![
                Scalar::Integer(3),
                Scalar::String("Doohickey".into()),
                Scalar::String("Red".into()),
                Scalar::String("B".into()),
                Scalar::Integer(30),
            ],
            vec![Scalar::Integer(4), Scalar::String("Gizmo".into()), Scalar::String("Green".into()), Scalar::String("B".into()), Scalar::Integer(40)],
        ],
    };
    let sales = ModelTable {
        name: "Sales".into(),
        columns: vec![
            ModelColumn { name: "OrderKey".into(), data_type: Some("Int64".into()) },
            ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
            ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
            number_column("Amount"),
            ModelColumn { name: "OrderDate".into(), data_type: Some("DateTime".into()) },
        ],
        rows: vec![
            vec![Scalar::Integer(1), Scalar::Integer(1), Scalar::String("East".into()), Scalar::Integer(100), Scalar::DateTime(45292.0)],
            vec![Scalar::Integer(2), Scalar::Integer(1), Scalar::String("West".into()), Scalar::Integer(200), Scalar::DateTime(45293.0)],
            vec![Scalar::Integer(3), Scalar::Integer(2), Scalar::String("East".into()), Scalar::Integer(300), Scalar::DateTime(45294.0)],
            vec![Scalar::Integer(4), Scalar::Integer(3), Scalar::String("West".into()), Scalar::Integer(400), Scalar::DateTime(45295.0)],
        ],
    };
    let notes = ModelTable {
        name: "Notes".into(),
        columns: vec![ModelColumn { name: "Key".into(), data_type: Some("Int64".into()) }, number_column("Score")],
        rows: vec![vec![Scalar::Integer(1), Scalar::Blank], vec![Scalar::Integer(2), Scalar::Integer(5)]],
    };
    let mut model = DataModel {
        tables: vec![products, sales, notes],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "ProductKey".into(),
            to_table: "Products".into(),
            to_column: "ProductKey".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![
            Measure { name: "Total Amount".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure { name: "Price Total".into(), formula: "SUM(Products[Price])".into(), table: Some("Products".into()), format: None, expr: None },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn evaluator(model: &DataModel) -> Evaluator<'_> {
    Evaluator::new(model).expect("evaluator")
}

fn eval(src: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = evaluator(&model);
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn eval_table(src: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = evaluator(&model);
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.to_string())
}

fn eval_east(src: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = evaluator(&model);
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    let filters = region_filter(&model, "East");
    evaluator.evaluate_with(&bound, filters).map_err(|error| error.to_string())
}

fn region_filter(model: &DataModel, region: &str) -> FilterContext {
    let mut filters = FilterContext::default();
    let table = model.table_index("Sales").expect("Sales");
    let column = model.column_index(table, "Region").expect("Region");
    filters.set(table, column, [ScalarKey::from(&Scalar::String(region.into()))]);
    filters
}

fn column(table: &TableValue, name: &str) -> Vec<Scalar> {
    table.column(table.column_index(name).expect("column exists"))
}

// ---------------------------------------------------------------------------------------------
// INDEX
// ---------------------------------------------------------------------------------------------

#[test]
fn index_returns_the_row_at_an_absolute_position() {
    let first = eval_table("INDEX(1, Sales, ORDERBY(Sales[OrderKey]))").expect("INDEX");
    assert_eq!(column(&first, "OrderKey"), vec![Scalar::Integer(1)]);
    assert_eq!(column(&first, "Amount"), vec![Scalar::Integer(100)]);

    let last = eval_table("INDEX(-1, Sales, ORDERBY(Sales[OrderKey]))").expect("INDEX");
    assert_eq!(column(&last, "OrderKey"), vec![Scalar::Integer(4)]);

    let second_last = eval_table("INDEX(-2, Sales, ORDERBY(Sales[OrderKey]))").expect("INDEX");
    assert_eq!(column(&second_last, "OrderKey"), vec![Scalar::Integer(3)]);
}

#[test]
fn index_orders_descending_and_by_expressions() {
    let highest = eval_table("INDEX(1, Sales, ORDERBY(Sales[Amount], DESC))").expect("INDEX");
    assert_eq!(column(&highest, "Amount"), vec![Scalar::Integer(400)]);

    // ORDERBY over an expression, not only a column.
    let smallest = eval_table("INDEX(1, Sales, ORDERBY(Sales[Amount] * -1))").expect("INDEX");
    assert_eq!(column(&smallest, "Amount"), vec![Scalar::Integer(400)]);
}

#[test]
fn index_out_of_range_positions_return_an_empty_table() {
    for source in ["INDEX(0, Sales, ORDERBY(Sales[OrderKey]))", "INDEX(9, Sales, ORDERBY(Sales[OrderKey]))", "INDEX(-9, Sales)"] {
        let table = eval_table(source).expect("INDEX");
        assert!(table.rows.is_empty(), "{source} must be empty");
        assert_eq!(table.columns.len(), 5, "{source} keeps the relation columns");
    }
}

#[test]
fn index_partitions_the_relation() {
    let firsts = eval_table("INDEX(1, Sales, ORDERBY(Sales[Amount]), PARTITIONBY(Sales[Region]))").expect("INDEX");
    assert_eq!(column(&firsts, "Amount"), vec![Scalar::Integer(100), Scalar::Integer(200)]);
    assert_eq!(column(&firsts, "Region"), vec![Scalar::String("East".into()), Scalar::String("West".into())]);

    let lasts = eval_table("INDEX(-1, Sales, ORDERBY(Sales[Amount]), PARTITIONBY(Sales[Region]))").expect("INDEX");
    assert_eq!(column(&lasts, "Amount"), vec![Scalar::Integer(300), Scalar::Integer(400)]);
}

#[test]
fn index_uses_the_outer_partition_filter() {
    // CALCULATE pins Region to East, so INDEX only inspects the East partition.
    let value =
        eval("CALCULATE(SUMX(INDEX(1, Sales, ORDERBY(Sales[OrderKey]), PARTITIONBY(Sales[Region])), Sales[Amount]), Sales[Region] = \"East\")")
            .expect("INDEX in CALCULATE");
    assert_eq!(value, Scalar::Decimal(100.0));
}

#[test]
fn index_orders_blanks_according_to_the_blanks_argument() {
    let default = eval_table("INDEX(1, Notes, ORDERBY(Notes[Score]))").expect("INDEX");
    assert_eq!(column(&default, "Key"), vec![Scalar::Integer(1)], "DEFAULT sorts numeric blanks just below zero");

    let last = eval_table("INDEX(1, Notes, ORDERBY(Notes[Score]), LAST)").expect("INDEX");
    assert_eq!(column(&last, "Key"), vec![Scalar::Integer(2)], "LAST moves the blank to the end");

    let first = eval_table("INDEX(-1, Notes, ORDERBY(Notes[Score]), FIRST)").expect("INDEX");
    assert_eq!(column(&first, "Key"), vec![Scalar::Integer(2)], "FIRST keeps the blank at the start");
}

#[test]
fn index_defaults_the_relation_from_the_order_clause() {
    let table = eval_table("INDEX(1, ORDERBY(Sales[OrderKey]))").expect("INDEX");
    assert_eq!(table.columns.len(), 1);
    assert_eq!(column(&table, "OrderKey"), vec![Scalar::Integer(1)]);

    let error = eval_table("INDEX(1)").expect_err("INDEX without relation or ORDERBY");
    assert!(error.contains("relation or an ORDERBY"), "{error}");
}

#[test]
fn index_rejects_non_integer_positions() {
    let error = eval_table("INDEX(1.5, Sales, ORDERBY(Sales[OrderKey]))").expect_err("fractional position");
    assert!(error.contains("whole number"), "{error}");
    let error = eval_table("INDEX(1, Sales, PRODUCTBY(Sales[OrderKey]))").expect_err("unknown clause");
    assert!(error.contains("unknown DAX function") || error.contains("PRODUCTBY"), "{error}");
}

// ---------------------------------------------------------------------------------------------
// OFFSET
// ---------------------------------------------------------------------------------------------

#[test]
fn offset_navigates_relative_to_the_current_row() {
    let table = eval_table("ADDCOLUMNS(Sales, \"Previous\", SUMX(OFFSET(-1, Sales, ORDERBY(Sales[OrderDate])), Sales[Amount]))")
        .expect("OFFSET in ADDCOLUMNS");
    assert_eq!(column(&table, "Previous"), vec![Scalar::Blank, Scalar::Decimal(100.0), Scalar::Decimal(200.0), Scalar::Decimal(300.0)]);

    let next = eval_table("ADDCOLUMNS(Sales, \"Next\", SUMX(OFFSET(1, Sales, ORDERBY(Sales[OrderDate])), Sales[Amount]))").expect("OFFSET");
    assert_eq!(column(&next, "Next"), vec![Scalar::Decimal(200.0), Scalar::Decimal(300.0), Scalar::Decimal(400.0), Scalar::Blank]);
}

#[test]
fn offset_partitions_the_relation() {
    let table = eval_table(
        "ADDCOLUMNS(Sales, \"PreviousInRegion\", SUMX(OFFSET(-1, Sales, ORDERBY(Sales[OrderDate]), PARTITIONBY(Sales[Region])), Sales[Amount]))",
    )
    .expect("OFFSET");
    assert_eq!(column(&table, "PreviousInRegion"), vec![Scalar::Blank, Scalar::Blank, Scalar::Decimal(100.0), Scalar::Decimal(200.0)]);
}

#[test]
fn offset_identifies_the_row_with_matchby() {
    let table =
        eval_table("ADDCOLUMNS(Sales, \"Previous\", SUMX(OFFSET(-1, Sales, ORDERBY(Sales[OrderKey]), MATCHBY(Sales[OrderKey])), Sales[Amount]))")
            .expect("OFFSET with MATCHBY");
    assert_eq!(column(&table, "Previous"), vec![Scalar::Blank, Scalar::Decimal(100.0), Scalar::Decimal(200.0), Scalar::Decimal(300.0)]);
}

#[test]
fn offset_outside_the_relation_needs_a_row_context() {
    let error = eval_table("OFFSET(-1, Sales, ORDERBY(Sales[OrderKey]))").expect_err("no row context");
    assert!(error.contains("row context"), "{error}");

    // An empty result is not an error: the row context exists but the delta leaves the partition.
    let table = eval_table("ADDCOLUMNS(Notes, \"Shifted\", COUNTROWS(OFFSET(5, Notes, ORDERBY(Notes[Key]))))").expect("OFFSET");
    assert_eq!(column(&table, "Shifted"), vec![Scalar::Blank, Scalar::Blank]);
}

#[test]
fn offset_reads_the_frame_of_the_relation_own_table() {
    // `OFFSET` runs inside `SUMX(Products, …)` while the window relation is `Sales`: the row
    // context stack is [Sales, Products] and the frame that identifies the current Sales row is
    // the outer one. A `Products` column that happens to hold "East" must not be mistaken for
    // `Sales[Region]`.
    let products = ModelTable {
        name: "Products".into(),
        columns: vec![
            ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
            ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
        ],
        rows: vec![vec![Scalar::Integer(1), Scalar::String("East".into())]],
    };
    let sales = ModelTable {
        name: "Sales".into(),
        columns: vec![
            ModelColumn { name: "OrderKey".into(), data_type: Some("Int64".into()) },
            ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
            number_column("Amount"),
        ],
        rows: vec![
            vec![Scalar::Integer(1), Scalar::String("East".into()), Scalar::Integer(100)],
            vec![Scalar::Integer(2), Scalar::String("West".into()), Scalar::Integer(200)],
            vec![Scalar::Integer(3), Scalar::String("East".into()), Scalar::Integer(300)],
        ],
    };
    let model = DataModel { tables: vec![products, sales], relationships: vec![], measures: vec![] };
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("ADDCOLUMNS(Sales, \"Previous\", SUMX(Products, COUNTROWS(OFFSET(-1, ALL(Sales[Region]), ORDERBY(Sales[Region])))))")
        .expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    let table = evaluator.evaluate_table(&bound).expect("nested window evaluates");
    // East is the first region, so only the West row has a previous entry; COUNTROWS of the empty
    // relation is BLANK.
    assert_eq!(column(&table, "Previous"), vec![Scalar::Blank, Scalar::Decimal(1.0), Scalar::Blank]);
}

#[test]
fn offset_requires_a_row_context_of_the_relation_table() {
    // A `Products` row does not identify a `Sales` row, so the window cannot anchor at all; the
    // old code read a `Products` column through the `Sales` model index instead.
    let error = eval_table("ADDCOLUMNS(Products, \"x\", COUNTROWS(OFFSET(-1, ALL(Sales[Region]), ORDERBY(Sales[Region]))))")
        .expect_err("Products row context does not identify a Sales row");
    assert!(error.contains("row context"), "{error}");
}

// ---------------------------------------------------------------------------------------------
// WINDOW
// ---------------------------------------------------------------------------------------------

#[test]
fn window_returns_an_absolute_range() {
    let table = eval_table("WINDOW(1, ABS, 2, ABS, Sales, ORDERBY(Sales[OrderKey]))").expect("WINDOW");
    assert_eq!(column(&table, "OrderKey"), vec![Scalar::Integer(1), Scalar::Integer(2)]);

    let tail = eval_table("WINDOW(-2, ABS, -1, ABS, Sales, ORDERBY(Sales[OrderKey]))").expect("WINDOW");
    assert_eq!(column(&tail, "OrderKey"), vec![Scalar::Integer(3), Scalar::Integer(4)]);

    let whole = eval_table("WINDOW(-10, ABS, 10, ABS, Sales, ORDERBY(Sales[OrderKey]))").expect("WINDOW");
    assert_eq!(whole.rows.len(), 4, "out-of-range boundaries are clamped");

    let reversed = eval_table("WINDOW(3, ABS, 1, ABS, Sales, ORDERBY(Sales[OrderKey]))").expect("WINDOW");
    assert!(reversed.rows.is_empty(), "a start after its end is an empty window");
}

#[test]
fn window_runs_relative_to_the_current_row() {
    let table =
        eval_table("ADDCOLUMNS(Sales, \"Running\", SUMX(WINDOW(1, ABS, 0, REL, Sales, ORDERBY(Sales[OrderKey])), Sales[Amount]))").expect("WINDOW");
    assert_eq!(column(&table, "Running"), vec![Scalar::Decimal(100.0), Scalar::Decimal(300.0), Scalar::Decimal(600.0), Scalar::Decimal(1000.0)]);
}

#[test]
fn window_partitions_the_relation() {
    let table = eval_table("WINDOW(1, ABS, 1, ABS, Sales, ORDERBY(Sales[OrderKey]), PARTITIONBY(Sales[Region]))").expect("WINDOW");
    assert_eq!(column(&table, "Amount"), vec![Scalar::Integer(100), Scalar::Integer(200)]);
}

#[test]
fn window_error_paths() {
    let error = eval_table("WINDOW(-1, REL, 1, REL, Sales, ORDERBY(Sales[OrderKey]))").expect_err("REL without row context");
    assert!(error.contains("row context"), "{error}");

    let error = eval_table("WINDOW(1, ABS, 1, WRONG, Sales)").expect_err("bad boundary type");
    assert!(error.contains("ABS or REL") || error.contains("unknown name"), "{error}");

    let error = eval_table("WINDOW(1, 2, 3, 4, Sales)").expect_err("non-keyword boundary type");
    assert!(error.contains("ABS or REL"), "{error}");
}

// ---------------------------------------------------------------------------------------------
// LOOKUP / LOOKUPVALUE / LOOKUPWITHTOTALS
// ---------------------------------------------------------------------------------------------

#[test]
fn lookupvalue_returns_one_result_per_search() {
    assert_eq!(eval("LOOKUPVALUE(Products[Name], Products[ProductKey], 2)"), Ok(Scalar::String("Gadget".into())));
    assert_eq!(eval("LOOKUPVALUE(Products[Color], Products[Category], \"B\", Products[Price], 40)"), Ok(Scalar::String("Green".into())));
    assert_eq!(eval("LOOKUPVALUE(Products[Name], Products[ProductKey], 99, \"Unknown\")"), Ok(Scalar::String("Unknown".into())));
    assert_eq!(eval("LOOKUPVALUE(Products[Name], Products[ProductKey], 99)"), Ok(Scalar::Blank));
}

#[test]
fn lookupvalue_handles_duplicates_and_errors() {
    // Both rows in category A have different names, so the value is ambiguous.
    let error = eval("LOOKUPVALUE(Products[Name], Products[Category], \"A\")").expect_err("ambiguous value");
    assert!(error.contains("multiple different values"), "{error}");
    assert_eq!(eval("LOOKUPVALUE(Products[Name], Products[Category], \"A\", \"Ambiguous\")"), Ok(Scalar::String("Ambiguous".into())));

    let error = eval("LOOKUPVALUE(Sales[Amount], Products[ProductKey], 1)").expect_err("different tables");
    assert!(error.contains("result column's table"), "{error}");
}

#[test]
fn lookupvalue_ignores_the_filter_context() {
    // The result is the unfiltered row, not the East-only aggregate.
    assert_eq!(eval_east("LOOKUPVALUE(Sales[Amount], Sales[OrderKey], 2)"), Ok(Scalar::Integer(200)));
}

#[test]
fn lookup_evaluates_measures_over_the_search_coordinates() {
    // ProductKey 1 has two sales rows, and LOOKUP aggregates them under the search filter.
    assert_eq!(eval("LOOKUP([Total Amount], Sales[ProductKey], 1)"), Ok(Scalar::Decimal(300.0)));
    assert_eq!(eval("LOOKUP([Price Total], Products[ProductKey], 2)"), Ok(Scalar::Decimal(20.0)));
    // EXPLICIT keeps the outer Region filter; the search only replaces ProductKey.
    assert_eq!(eval_east("LOOKUP([Total Amount], Sales[ProductKey], 1)"), Ok(Scalar::Decimal(100.0)));
}

#[test]
fn lookup_accepts_the_associated_columns_behaviour_and_errors_on_misses() {
    assert_eq!(eval("LOOKUP([Total Amount], Sales[ProductKey], 1, INFERRED)"), Ok(Scalar::Decimal(300.0)));
    assert_eq!(eval("LOOKUP([Total Amount], Sales[ProductKey], 1, EXPLICIT)"), Ok(Scalar::Decimal(300.0)));

    let error = eval("LOOKUP([Total Amount], Sales[ProductKey], 99)").expect_err("no match");
    assert!(error.contains("did not match any row"), "{error}");

    let error = eval("LOOKUP([Total Amount], Sales[ProductKey], 1, SOMETHING)").expect_err("unknown behaviour");
    assert!(error.contains("EXPLICIT or INFERRED") || error.contains("unknown name"), "{error}");
}

#[test]
fn lookup_by_a_related_column_and_with_totals() {
    assert_eq!(eval("LOOKUP(Products[Name], Sales[ProductKey], 3)"), Ok(Scalar::String("Doohickey".into())));
    // LOOKUPWITHTOTALS resets the non-search columns, so the outer East filter is ignored.
    assert_eq!(eval_east("LOOKUPWITHTOTALS([Total Amount], Sales[ProductKey], 1)"), Ok(Scalar::Decimal(300.0)));
}

// ---------------------------------------------------------------------------------------------
// ORDERBY / PARTITIONBY / MATCHBY clauses
// ---------------------------------------------------------------------------------------------

#[test]
fn clauses_are_not_scalars_and_bind_their_keywords() {
    let model = model();
    for clause in ["ORDERBY(Sales[Amount], DESC)", "PARTITIONBY(Sales[Region])", "MATCHBY(Sales[OrderKey])"] {
        let bound = bind_formula(&model, clause).expect("clause binds");
        let evaluator = evaluator(&model);
        let error = evaluator.evaluate(&bound).expect_err("clause is not a scalar");
        assert!(error.message.contains("clause"), "{clause}: {error}");
    }

    // Bare enum words bind; unknown identifiers still fail normally.
    assert!(bind_formula(&model, "INDEX(1, Sales, ORDERBY(Sales[Amount], DESC))").is_ok());
    assert!(bind_formula(&model, "INDEX(1, Sales, ORDERBY(Sales[Amount]), KEEP, PARTITIONBY(Sales[Region]))").is_ok());
    assert!(bind_formula(&model, "WINDOW(1, ABS, 1, ABS, Sales)").is_ok());
    assert!(bind_formula(&model, "ORDERBY(Sales[Amount], SIDEWAYS)").is_err());
    assert!(bind_formula(&model, "INDEX(1, Sales, ORDERBY(Unknown[Amount]))").is_err());
}

// ---------------------------------------------------------------------------------------------
// ALLCROSSFILTERED and the apply/cluster forms
// ---------------------------------------------------------------------------------------------

#[test]
fn allcrossfiltered_removes_the_table_filters_as_a_modifier() {
    assert_eq!(eval_east("CALCULATE([Total Amount], ALLCROSSFILTERED(Sales))"), Ok(Scalar::Decimal(1000.0)));
    let error = eval("ALLCROSSFILTERED(Sales)").expect_err("modifier only");
    assert!(error.contains("CALCULATE"), "{error}");
}

#[test]
fn apply_wrappers_return_their_table_expression() {
    let colors = eval_table("ALLSELECTEDAPPLY(VALUES(Products[Color]))").expect("ALLSELECTEDAPPLY");
    assert_eq!(colors.rows.len(), 3);
    let same = eval_table("ALLSELECTEDREMOVE(VALUES(Products[Color]))").expect("ALLSELECTEDREMOVE");
    assert_eq!(same.rows.len(), 3);
    let shadow = eval_table("SHADOWCLUSTER(VALUES(Products[Color]))").expect("SHADOWCLUSTER");
    assert_eq!(shadow.rows.len(), 3);
    let nonfilter = eval_table("NONFILTER(VALUES(Products[Color]))").expect("NONFILTER");
    assert_eq!(nonfilter.rows.len(), 3);

    let empty = eval_table("ALWAYSAPPLY(FILTER(VALUES(Products[Color]), FALSE))").expect("ALWAYSAPPLY");
    assert!(empty.rows.is_empty());
}

#[test]
fn dependon_matches_and_hides_the_leading_columns() {
    let table =
        eval_table("ADDCOLUMNS(VALUES(Products[ProductKey]), \"Match\", COUNTROWS(DEPENDON(Products, Products[ProductKey])))").expect("DEPENDON");
    assert_eq!(table.rows.len(), 4);
    assert_eq!(column(&table, "Match"), vec![Scalar::Integer(1); 4]);

    let dependent = eval_table("DEPENDON(Products, Products[ProductKey])").expect_err("no row context");
    assert!(dependent.contains("column reference") || dependent.contains("row"), "{dependent}");
}

#[test]
fn groupcrossapply_summarises_groups_with_filter_tables() {
    let table = eval_table("GROUPCROSSAPPLY(Sales[Region], ALL(Sales), \"Amount\", [Total Amount])").expect("GROUPCROSSAPPLY");
    assert_eq!(column(&table, "Region"), vec![Scalar::String("East".into()), Scalar::String("West".into())]);
    assert_eq!(column(&table, "Amount"), vec![Scalar::Decimal(400.0), Scalar::Decimal(600.0)]);

    // Rows whose expressions are all blank are omitted.
    let dropped = eval_table("GROUPCROSSAPPLY(Sales[Region], ALL(Sales), \"Nothing\", BLANK())").expect("GROUPCROSSAPPLY");
    assert!(dropped.rows.is_empty());

    // Without expressions the call simply lists the groups.
    let groups = eval_table("GROUPCROSSAPPLY(Sales[Region], ALL(Sales))").expect("GROUPCROSSAPPLY");
    assert_eq!(groups.rows.len(), 2);
}

#[test]
fn groupcrossapplytable_adds_table_expressions() {
    let table = eval_table("GROUPCROSSAPPLYTABLE(Sales[Region], ALL(Sales), \"\", VALUES(Sales[ProductKey]))").expect("GROUPCROSSAPPLYTABLE");
    // East sells products 1 and 2; West sells products 1 and 3.
    assert_eq!(
        column(&table, "Region"),
        vec![Scalar::String("East".into()), Scalar::String("East".into()), Scalar::String("West".into()), Scalar::String("West".into())]
    );
    assert_eq!(column(&table, "ProductKey"), vec![Scalar::Integer(1), Scalar::Integer(2), Scalar::Integer(1), Scalar::Integer(3)]);
}

#[test]
fn filtercluster_correlates_scans_with_groups() {
    let table = eval_table("FILTERCLUSTER(Sales[Region], ALL(Sales), \"\", VALUES(Sales[ProductKey]))").expect("FILTERCLUSTER");
    assert_eq!(table.rows.len(), 4);
    assert_eq!(
        column(&table, "Region"),
        vec![Scalar::String("East".into()), Scalar::String("East".into()), Scalar::String("West".into()), Scalar::String("West".into())]
    );

    let error = eval_table("FILTERCLUSTER(Sales[Region], ALL(Sales), ALL(Sales), VALUES(Sales[ProductKey]))").expect_err("missing separator");
    assert!(error.contains("separator"), "{error}");
}

#[test]
fn apply_functions_reject_cross_joins_above_the_row_limit() {
    // Three 101-row scans cross-join to 1,030,301 rows, above the 1,000,000-row ceiling every
    // row-generating table function enforces, so the scan is rejected instead of allocated.
    let cross_apply = "GROUPCROSSAPPLYTABLE(Sales[Region], ALL(Sales), \"\", GENERATESERIES(1, 101), \"\", \
                       GENERATESERIES(1, 101), \"\", GENERATESERIES(1, 101))";
    let filter_cluster = "FILTERCLUSTER(Sales[Region], ALL(Sales), \"\", GENERATESERIES(1, 101), \
                          GENERATESERIES(1, 101), GENERATESERIES(1, 101))";
    for (function, expression) in [("GROUPCROSSAPPLYTABLE", cross_apply), ("FILTERCLUSTER", filter_cluster)] {
        let error = eval_table(expression).expect_err("oversized cross join");
        assert!(error.contains("too large"), "{function}: {error}");
    }
}

#[test]
fn apply_functions_reject_missing_group_columns() {
    // Argument kinds are validated at bind time; a non-column group argument never reaches the
    // evaluator.
    assert!(eval_table("GROUPCROSSAPPLY(1, ALL(Sales))").is_err());
    assert!(eval_table("FILTERCLUSTER(1, ALL(Sales), \"\", VALUES(Sales[ProductKey]))").is_err());
}

#[test]
fn all_removes_the_tables_own_filters_for_table_expressions() {
    // The cell's own filter hides two of the four rows.
    assert_eq!(eval_east("COUNTROWS(Sales)").expect("COUNTROWS"), Scalar::Integer(2));
    // `ALL(Sales)` is the whole table, so an aggregate over it ignores the cell's filter.
    assert_eq!(eval_east("COUNTROWS(ALL(Sales))").expect("ALL"), Scalar::Integer(4));
    assert_eq!(eval_east("SUMX(ALL(Sales), Sales[Amount])").expect("SUMX"), Scalar::Decimal(1000.0));
    // `ALL` on a column drops that column's filter but keeps the others.
    assert_eq!(eval_east("COUNTROWS(ALL(Sales[Region]))").expect("ALL(column)"), Scalar::Integer(2));
}

#[test]
fn allexcept_keeps_only_the_named_columns_filters_in_calculate() {
    let model = model();
    let evaluator = evaluator(&model);
    let sales = model.table_index("Sales").expect("Sales");
    let region = model.column_index(sales, "Region").expect("Region");
    let amount = model.column_index(sales, "Amount").expect("Amount");
    let mut filters = FilterContext::default();
    filters.set(sales, region, [ScalarKey::from(&Scalar::String("East".into()))]);
    filters.set(sales, amount, [ScalarKey::from(&Scalar::Integer(100))]);
    let eval_with = |src: &str| {
        let expr = parse_expression(src).map_err(|error| error.to_string()).expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string()).expect("bind");
        evaluator.evaluate_with(&bound, filters.clone()).map_err(|error| error.to_string())
    };
    // Both filters together keep one row.
    assert_eq!(eval_with("COUNTROWS(Sales)").expect("COUNTROWS"), Scalar::Integer(1));
    // ALLEXCEPT keeps the region filter and drops the amount one: two East rows remain.
    assert_eq!(eval_with("CALCULATE(COUNTROWS(Sales), ALLEXCEPT(Sales, Sales[Region]))").expect("ALLEXCEPT"), Scalar::Integer(2));
}

#[test]
fn selectcolumns_projects_only_the_named_columns() {
    let table = eval_table("SELECTCOLUMNS(Sales, \"R\", Sales[Region])").expect("SELECTCOLUMNS");
    assert_eq!(table.columns.iter().map(|column| column.name.as_str()).collect::<Vec<_>>(), vec!["R"]);
    assert_eq!(
        column(&table, "R"),
        vec![Scalar::String("East".into()), Scalar::String("West".into()), Scalar::String("East".into()), Scalar::String("West".into())]
    );
    // ADDCOLUMNS still keeps the source columns and appends to them.
    let added = eval_table("ADDCOLUMNS(Sales, \"Double\", Sales[Amount] * 2)").expect("ADDCOLUMNS");
    assert_eq!(added.columns.len(), 6);
    assert_eq!(column(&added, "Double"), vec![Scalar::Integer(200), Scalar::Integer(400), Scalar::Integer(600), Scalar::Integer(800)]);
}

#[test]
fn topn_applies_each_order_pair_and_keeps_ties() {
    let ascending = eval_table("TOPN(2, Sales, Sales[Amount], ASC)").expect("TOPN asc");
    assert_eq!(column(&ascending, "Amount"), vec![Scalar::Integer(100), Scalar::Integer(200)]);
    let descending = eval_table("TOPN(2, Sales, Sales[Amount], DESC)").expect("TOPN desc");
    assert_eq!(column(&descending, "Amount"), vec![Scalar::Integer(400), Scalar::Integer(300)]);
    // The second pair breaks the ties of the first: East rows first, larger amount first.
    let two_keys = eval_table("TOPN(2, Sales, Sales[Region], ASC, Sales[Amount], DESC)").expect("TOPN two keys");
    assert_eq!(column(&two_keys, "Amount"), vec![Scalar::Integer(300), Scalar::Integer(100)]);
}
