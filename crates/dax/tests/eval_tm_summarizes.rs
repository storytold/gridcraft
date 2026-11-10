//! Integration tests for the table-manipulation grouping functions: `SUMMARIZE`,
//! `SUMMARIZECOLUMNS`, `GROUPBY`, `ADDMISSINGITEMS` and the row selectors.
//!
//! Model (Sales):
//!
//! | Region | Product | Amount | Quantity |
//! |--------|---------|--------|----------|
//! | East   | Widget  |     10 |        2 |
//! | West   | Widget  |     20 |        3 |
//! | East   | Gadget  |     30 |        4 |
//! | East   | Widget  |      5 |        1 |
//! | (Blank)| Gadget  |      7 |        1 |
//! | North  | Gadget  |     50 |        5 |
//!
//! Products: Widget -> Hardware, Gadget -> Devices. Amount totals by region are East 45,
//! North 50, West 20, blank 7 (grand total 122); by product Widget 35, Gadget 87.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn column(name: &str, data_type: &str) -> ModelColumn {
    ModelColumn { name: name.into(), data_type: Some(data_type.into()) }
}

fn table(name: &str, columns: Vec<ModelColumn>, rows: Vec<Vec<Scalar>>) -> ModelTable {
    ModelTable { name: name.into(), columns, rows }
}

fn text(value: &str) -> Scalar {
    Scalar::String(value.into())
}

fn int(value: i64) -> Scalar {
    Scalar::Integer(value)
}

fn money(value: f64) -> Scalar {
    Scalar::Decimal(value)
}

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            table(
                "Sales",
                vec![column("Region", "String"), column("Product", "String"), column("Amount", "Double"), column("Quantity", "Int64")],
                vec![
                    vec![text("East"), text("Widget"), money(10.0), int(2)],
                    vec![text("West"), text("Widget"), money(20.0), int(3)],
                    vec![text("East"), text("Gadget"), money(30.0), int(4)],
                    vec![text("East"), text("Widget"), money(5.0), int(1)],
                    vec![Scalar::Blank, text("Gadget"), money(7.0), int(1)],
                    vec![text("North"), text("Gadget"), money(50.0), int(5)],
                ],
            ),
            table(
                "Products",
                vec![column("Product", "String"), column("Category", "String")],
                vec![vec![text("Widget"), text("Hardware")], vec![text("Gadget"), text("Devices")]],
            ),
            table(
                "Items",
                vec![column("Item", "String"), column("Rank", "Int64")],
                vec![vec![text("A"), int(1)], vec![text("B"), int(2)], vec![text("C"), int(3)], vec![text("D"), int(4)]],
            ),
            table(
                "Levels",
                vec![
                    column("LevelNumber", "Int64"),
                    column("TotalColumnName", "String"),
                    column("ColumnName", "String"),
                    column("FlagOrder", "Int64"),
                ],
                vec![vec![int(1), text(""), text("Item"), int(1)], vec![int(1), text(""), text("Rank"), int(0)]],
            ),
            table(
                "Nodes",
                vec![column("Unknown", "Int64"), column("LevelNumber", "Int64"), column("ColumnName", "String"), column("ItemSelected", "String")],
                vec![
                    vec![int(0), int(1), text("Item"), text("A")],
                    vec![int(0), int(1), text("Item"), text("B")],
                    vec![int(0), int(1), text("Item"), text("C")],
                ],
            ),
            table(
                "Bounds",
                vec![
                    column("Unknown", "Int64"),
                    column("LevelNumber", "Int64"),
                    column("ColumnName", "String"),
                    column("BoundaryItemValue", "String"),
                    column("BoundaryType", "Int64"),
                ],
                vec![vec![int(0), int(1), text("Item"), text("A"), int(2)], vec![int(0), int(1), text("Item"), text("C"), int(1)]],
            ),
            table(
                "BoundsStart",
                vec![
                    column("Unknown", "Int64"),
                    column("LevelNumber", "Int64"),
                    column("ColumnName", "String"),
                    column("BoundaryItemValue", "String"),
                    column("BoundaryType", "Int64"),
                ],
                vec![vec![int(0), int(1), text("Item"), text("B"), int(2)], vec![int(0), int(1), text("Item"), text("D"), int(1)]],
            ),
            table(
                "Scores",
                vec![column("Team", "String"), column("Score", "Int64")],
                vec![vec![text("A"), int(10)], vec![text("B"), int(10)], vec![text("C"), int(8)], vec![text("D"), int(5)]],
            ),
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "Product".into(),
            to_table: "Products".into(),
            to_column: "Product".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![
            Measure { name: "Total Sales".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure { name: "Total Quantity".into(), formula: "SUM(Sales[Quantity])".into(), table: Some("Sales".into()), format: None, expr: None },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn eval(source: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(source).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn eval_table(source: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(source).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.to_string())
}

fn header(table: &TableValue) -> Vec<String> {
    table.columns.iter().map(|column| column.name.clone()).collect()
}

fn row(table: &TableValue, index: usize) -> Vec<Scalar> {
    table.rows.get(index).cloned().unwrap_or_else(|| panic!("row {index} is missing in {:?}", table.rows))
}

fn assert_columns(table: &TableValue, expected: &[&str]) {
    assert_eq!(header(table), expected.iter().map(|name| (*name).to_string()).collect::<Vec<_>>(), "column names/order");
}

fn error(source: &str) -> String {
    match eval_table(source) {
        Ok(table) => panic!("{source} should fail, returned {table:?}"),
        Err(message) => message,
    }
}

fn scalar_error(source: &str) -> String {
    match eval(source) {
        Ok(value) => panic!("{source} should fail, returned {value:?}"),
        Err(message) => message,
    }
}

#[test]
fn summarize_groups_by_columns_and_evaluates_expressions() {
    let table = eval_table("SUMMARIZE(Sales, Sales[Region], \"Amount\", SUM(Sales[Amount]))").expect("summarize");
    assert_columns(&table, &["Region", "Amount"]);
    assert_eq!(
        table.rows,
        vec![vec![text("East"), money(45.0)], vec![text("North"), money(50.0)], vec![text("West"), money(20.0)], vec![Scalar::Blank, money(7.0)],]
    );
}

#[test]
fn summarize_without_group_columns_returns_distinct_rows() {
    let table = eval_table("SUMMARIZE(Sales)").expect("summarize");
    assert_columns(&table, &["Region", "Product", "Amount", "Quantity"]);
    assert_eq!(table.rows.len(), 6);
    assert_eq!(table.lineage, Some(0));
}

#[test]
fn summarize_rollup_adds_subtotals_and_grand_total() {
    let table = eval_table("SUMMARIZE(Sales, ROLLUP(Sales[Region], Sales[Product]), \"Amount\", SUM(Sales[Amount]))").expect("rollup");
    assert_columns(&table, &["Region", "Product", "Amount"]);
    assert_eq!(
        table.rows,
        vec![
            vec![text("East"), text("Gadget"), money(30.0)],
            vec![text("East"), text("Widget"), money(15.0)],
            vec![text("East"), Scalar::Blank, money(45.0)],
            vec![text("North"), text("Gadget"), money(50.0)],
            vec![text("North"), Scalar::Blank, money(50.0)],
            vec![text("West"), text("Widget"), money(20.0)],
            vec![text("West"), Scalar::Blank, money(20.0)],
            vec![Scalar::Blank, text("Gadget"), money(7.0)],
            vec![Scalar::Blank, Scalar::Blank, money(7.0)],
            vec![Scalar::Blank, Scalar::Blank, money(122.0)],
        ]
    );
}

#[test]
fn summarize_rollupgroup_selects_its_columns_without_subtotals() {
    // The catalog validates ROLLUP arguments as plain column references, so a ROLLUPGROUP is
    // only reachable as a group-by argument of its own; it then just contributes its columns.
    let table = eval_table("SUMMARIZE(Sales, ROLLUPGROUP(Sales[Region], Sales[Product]), \"Amount\", SUM(Sales[Amount]))").expect("rollupgroup");
    assert_columns(&table, &["Region", "Product", "Amount"]);
    assert_eq!(table.rows.len(), 5, "no subtotal rows without ROLLUP");
    assert_eq!(table.rows[0], vec![text("East"), text("Gadget"), money(30.0)]);
}

#[test]
fn summarize_accepts_related_table_columns() {
    let table = eval_table("SUMMARIZE(Sales, Products[Category], \"Amount\", SUM(Sales[Amount]))").expect("related grouping");
    assert_columns(&table, &["Category", "Amount"]);
    assert_eq!(table.rows, vec![vec![text("Devices"), money(87.0)], vec![text("Hardware"), money(35.0)]]);
}

#[test]
fn summarize_related_columns_share_one_relationship_lookup() {
    // Both group columns hop through Sales -> Products for every source row; repeated and
    // out-of-order keys must still land in their single (Category, Product) group.
    let table = eval_table("SUMMARIZE(Sales, Products[Category], Products[Product], \"Amount\", SUM(Sales[Amount]))").expect("related columns");
    assert_columns(&table, &["Category", "Product", "Amount"]);
    assert_eq!(table.rows, vec![vec![text("Devices"), text("Gadget"), money(87.0)], vec![text("Hardware"), text("Widget"), money(35.0)]]);
}

#[test]
fn summarize_rejects_malformed_argument_lists() {
    assert!(error("SUMMARIZE(Sales, Sales[Region], \"Amount\")").contains("needs an expression"));
    assert!(error("SUMMARIZE(Sales, \"Amount\", SUM(Sales[Amount]), Sales[Region])").contains("must come before"));
    assert!(error("SUMMARIZE(Sales, ROLLUP(Sales[Region]), ROLLUP(Sales[Product]))").contains("single ROLLUP"));
    assert!(error("SUMMARIZE(Sales, 1)").contains("must be a column reference"));
}

#[test]
fn summarize_columns_cross_joins_groups_and_drops_blank_rows() {
    let table = eval_table("SUMMARIZECOLUMNS(Sales[Product], Sales[Region], \"Amount\", SUM(Sales[Amount]))").expect("summarize columns");
    assert_columns(&table, &["Product", "Region", "Amount"]);
    assert_eq!(
        table.rows,
        vec![
            vec![text("Gadget"), text("East"), money(30.0)],
            vec![text("Gadget"), text("North"), money(50.0)],
            vec![text("Gadget"), Scalar::Blank, money(7.0)],
            vec![text("Widget"), text("East"), money(15.0)],
            vec![text("Widget"), text("West"), money(20.0)],
        ],
        "combinations with a blank measure are dropped"
    );
}

#[test]
fn summarize_columns_requires_a_group_column() {
    assert!(error("SUMMARIZECOLUMNS()").contains("at least one group-by column"));
    assert!(error("SUMMARIZECOLUMNS(Sales[Region], Sales[Region])").contains("more than once"));
    assert!(error("SUMMARIZECOLUMNS(Sales[Region], \"Amount\")").contains("needs an expression"));
}

#[test]
fn summarize_columns_ignore_keeps_rows_only_for_other_expressions() {
    // A conditional expression is blank outside its own product group, so the blank-row test
    // decides which product rows survive.
    let plain = eval_table(
        "SUMMARIZECOLUMNS(Sales[Product], \"Widget\", IF(Sales[Product] = \"Widget\", SUM(Sales[Amount])), \
         \"Gadget\", IF(Sales[Product] = \"Gadget\", SUM(Sales[Amount])))",
    )
    .expect("plain");
    assert_eq!(
        plain.rows,
        vec![vec![text("Gadget"), Scalar::Blank, money(87.0)], vec![text("Widget"), money(35.0), Scalar::Blank],],
        "both products have a non-blank measure"
    );

    let ignored = eval_table(
        "SUMMARIZECOLUMNS(Sales[Product], \"Widget\", IF(Sales[Product] = \"Widget\", SUM(Sales[Amount])), \
         \"Gadget\", IGNORE(IF(Sales[Product] = \"Gadget\", SUM(Sales[Amount]))))",
    )
    .expect("ignored");
    assert_columns(&ignored, &["Product", "Widget", "Gadget"]);
    assert_eq!(ignored.rows, vec![vec![text("Widget"), money(35.0), Scalar::Blank]], "IGNORE does not keep the Gadget row");
}

#[test]
fn summarize_columns_nonvisual_filter_does_not_restrict_measures() {
    let filter = "FILTER(VALUES(Sales[Product]), Sales[Product] = \"Widget\")";
    let visual = eval_table(&format!("SUMMARIZECOLUMNS(Sales[Region], {filter}, \"Amount\", SUM(Sales[Amount]))")).expect("visual");
    assert_eq!(visual.rows, vec![vec![text("East"), money(15.0)], vec![text("West"), money(20.0)]]);

    let non_visual = eval_table(&format!("SUMMARIZECOLUMNS(Sales[Region], NONVISUAL({filter}), \"Amount\", SUM(Sales[Amount]))")).expect("nonvisual");
    assert_eq!(non_visual.rows, vec![vec![text("East"), money(45.0)], vec![text("West"), money(20.0)]]);
}

#[test]
fn summarize_columns_table_filter_restricts_the_group_column_domain() {
    let table =
        eval_table("SUMMARIZECOLUMNS(Sales[Region], FILTER(VALUES(Sales[Product]), Sales[Product] = \"Widget\"), \"Amount\", SUM(Sales[Amount]))")
            .expect("filtered");
    assert_columns(&table, &["Region", "Amount"]);
    assert_eq!(table.rows, vec![vec![text("East"), money(15.0)], vec![text("West"), money(20.0)]]);
}

#[test]
fn summarize_columns_filter_matches_with_dax_equality_semantics() {
    // `Amount` is stored as Decimal; the integer literal must still select the 10.0 rows.
    let numeric = eval_table("SUMMARIZECOLUMNS(Sales[Region], Sales[Amount] = 10, \"Amount\", SUM(Sales[Amount]))").expect("numeric filter");
    assert_columns(&numeric, &["Region", "Amount"]);
    assert_eq!(numeric.rows, vec![vec![text("East"), money(10.0)]]);

    // Text compares ignoring case: `"east"` selects the stored `"East"` value.
    let textual = eval_table("SUMMARIZECOLUMNS(Sales[Region], Sales[Region] = \"east\", \"Amount\", SUM(Sales[Amount]))").expect("text filter");
    assert_columns(&textual, &["Region", "Amount"]);
    assert_eq!(textual.rows, vec![vec![text("East"), money(45.0)]]);
}

#[test]
fn summarize_columns_accepts_treatas_and_multi_filter_forms() {
    let table =
        eval_table("SUMMARIZECOLUMNS(Sales[Region], TREATAS({\"East\", \"West\"}, Sales[Region]), \"Amount\", SUM(Sales[Amount]))").expect("treatas");
    assert_eq!(table.rows, vec![vec![text("East"), money(45.0)], vec![text("West"), money(20.0)]]);

    let non_visual = eval_table("SUMMARIZECOLUMNS(Sales[Region], NONVISUAL(TREATAS({\"East\"}, Sales[Region])), \"Amount\", SUM(Sales[Amount]))")
        .expect("nonvisual treatas");
    assert_eq!(non_visual.rows, vec![vec![text("East"), money(45.0)]]);
}

#[test]
fn summarize_columns_rollup_adds_subtotal_rows_with_flags() {
    let table = eval_table("SUMMARIZECOLUMNS(ROLLUPADDISSUBTOTAL(Sales[Region], \"RegionTotal\"), \"Amount\", SUM(Sales[Amount]))")
        .expect("rollupaddissubtotal");
    assert_columns(&table, &["Region", "RegionTotal", "Amount"]);
    assert_eq!(
        table.rows,
        vec![
            vec![text("East"), Scalar::Boolean(false), money(45.0)],
            vec![text("North"), Scalar::Boolean(false), money(50.0)],
            vec![text("West"), Scalar::Boolean(false), money(20.0)],
            vec![Scalar::Blank, Scalar::Boolean(false), money(7.0)],
            vec![Scalar::Blank, Scalar::Boolean(true), money(122.0)],
        ]
    );
}

#[test]
fn rollup_grand_total_filter_applies_only_to_the_grand_total() {
    // `grandtotalFilter` restricts the grand-total selection; the detail rows keep every region.
    let table = eval_table(
        "SUMMARIZECOLUMNS(ROLLUPADDISSUBTOTAL(Sales[Region] = \"East\", Sales[Region], \"RegionTotal\"), \
         \"Amount\", SUM(Sales[Amount]))",
    )
    .expect("grand total filter");
    assert_columns(&table, &["Region", "RegionTotal", "Amount"]);
    assert_eq!(
        table.rows,
        vec![
            vec![text("East"), Scalar::Boolean(false), money(45.0)],
            vec![text("North"), Scalar::Boolean(false), money(50.0)],
            vec![text("West"), Scalar::Boolean(false), money(20.0)],
            vec![Scalar::Blank, Scalar::Boolean(false), money(7.0)],
            vec![Scalar::Blank, Scalar::Boolean(true), money(45.0)],
        ],
        "only the East row feeds the grand total"
    );
}

#[test]
fn rollup_level_filter_applies_only_to_that_level() {
    // The filter after Region's name restricts the per-region subtotal rows to East; the detail
    // rows and the grand total are unfiltered.
    let table = eval_table(
        "SUMMARIZECOLUMNS(ROLLUPADDISSUBTOTAL(Sales[Region], \"RegionTotal\", Sales[Region] = \"East\", \
         Sales[Product], \"ProductTotal\"), \"Amount\", SUM(Sales[Amount]))",
    )
    .expect("region level filter");
    assert_columns(&table, &["Region", "Product", "RegionTotal", "ProductTotal", "Amount"]);
    assert_eq!(
        table.rows.iter().filter(|row| row[2] == Scalar::Boolean(false) && row[3] == Scalar::Boolean(true)).count(),
        1,
        "only the East region subtotal survives its own level filter"
    );
    assert!(table.rows.iter().any(|row| {
        row[0] == text("East")
            && row[1] == Scalar::Blank
            && row[2] == Scalar::Boolean(false)
            && row[3] == Scalar::Boolean(true)
            && row[4] == money(45.0)
    }));
    assert!(
        table.rows.iter().any(|row| {
            row[0] == Scalar::Blank
                && row[1] == Scalar::Blank
                && row[2] == Scalar::Boolean(true)
                && row[3] == Scalar::Boolean(true)
                && row[4] == money(122.0)
        }),
        "the grand total still sees every region"
    );
    assert!(table.rows.iter().any(|row| row[0] == text("East") && row[1] == text("Gadget") && row[4] == money(30.0)), "details are unfiltered");

    // The filter after the last level's name restricts the detail rows instead: only Widget
    // details remain, while every region still gets its subtotal.
    let detail = eval_table(
        "SUMMARIZECOLUMNS(ROLLUPADDISSUBTOTAL(Sales[Region], \"RegionTotal\", Sales[Product], \"ProductTotal\", \
         Sales[Product] = \"Widget\"), \"Amount\", SUM(Sales[Amount]))",
    )
    .expect("product level filter");
    assert!(detail.rows.iter().any(|row| row[1] == text("Widget") && row[4] == money(15.0)));
    assert!(detail.rows.iter().any(|row| row[1] == text("Widget") && row[4] == money(20.0)));
    assert!(
        !detail.rows.iter().any(|row| row[3] == Scalar::Boolean(false) && row[1] != text("Widget")),
        "detail rows outside the product filter are gone"
    );
    assert_eq!(
        detail.rows.iter().filter(|row| row[2] == Scalar::Boolean(false) && row[3] == Scalar::Boolean(true)).count(),
        4,
        "every region keeps its subtotal"
    );
}

#[test]
fn summarize_columns_rollup_multiple_levels_and_dimensions() {
    // The catalog's variadic overload only accepts more than one (column, name) pair when a
    // leading table filter is present, so the no-op grand-total filter is spelled out here.
    let table = eval_table(
        "SUMMARIZECOLUMNS(ROLLUPADDISSUBTOTAL(VALUES(Sales[Region]), Sales[Region], \"RegionTotal\", Sales[Product], \"ProductTotal\"), \
         \"Amount\", SUM(Sales[Amount]))",
    )
    .expect("two levels");
    assert_columns(&table, &["Region", "Product", "RegionTotal", "ProductTotal", "Amount"]);
    assert_eq!(table.rows.len(), 10);
    assert_eq!(
        row(&table, 2),
        vec![text("East"), Scalar::Blank, Scalar::Boolean(false), Scalar::Boolean(true), money(45.0)],
        "per-region subtotal: only the product level is rolled up"
    );
    assert_eq!(row(&table, 9), vec![Scalar::Blank, Scalar::Blank, Scalar::Boolean(true), Scalar::Boolean(true), money(122.0)], "grand total");

    let independent = eval_table(
        "SUMMARIZECOLUMNS(ROLLUPADDISSUBTOTAL(Sales[Region], \"RegionTotal\"), ROLLUPADDISSUBTOTAL(Sales[Product], \"ProductTotal\"), \
         \"Amount\", SUM(Sales[Amount]))",
    )
    .expect("two dimensions");
    assert_columns(&independent, &["Region", "Product", "RegionTotal", "ProductTotal", "Amount"]);
    assert_eq!(independent.rows.len(), 12, "detail rows plus both partial subtotals and the grand total");
    assert!(
        independent.rows.iter().any(|row| row[3] == Scalar::Boolean(true) && row[2] == Scalar::Boolean(false)),
        "product subtotals keep the region level"
    );
    assert!(independent.rows.iter().any(|row| row[2] == Scalar::Boolean(true) && row[3] == Scalar::Boolean(false)));
    assert_eq!(row(&independent, 11), vec![Scalar::Blank, Scalar::Blank, Scalar::Boolean(true), Scalar::Boolean(true), money(122.0)]);
}

#[test]
fn rollup_subtotals_group_the_details_once_per_depth() {
    // 60 customers x 60 products: 3,600 detail groups, 60 customer subtotals and one grand total.
    // The subtotal pass must bucket the details instead of scanning them once per prefix
    // (CodeRabbit, #3412).
    let rows: Vec<Vec<Scalar>> = (0..60)
        .flat_map(|customer| (0..60).map(move |product| vec![text(&format!("C{customer}")), text(&format!("P{product}")), money(1.0)]))
        .collect();
    let mut model = DataModel {
        tables: vec![table("Sales", vec![column("Customer", "String"), column("Product", "String"), column("Amount", "Double")], rows)],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("SUMMARIZE(Sales, ROLLUP(Sales[Customer], Sales[Product]), \"Amount\", SUM(Sales[Amount]))").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    let table = evaluator.evaluate_table(&bound).expect("summarize");
    assert_eq!(table.rows.len(), 60 + 60 * 60 + 1, "customer subtotals, details and the grand total");
}

#[test]
fn summarize_columns_rollupgroup_contributes_its_columns() {
    let table = eval_table("SUMMARIZECOLUMNS(ROLLUPGROUP(Sales[Region], Sales[Product]), \"Amount\", SUM(Sales[Amount]))").expect("rollupgroup");
    assert_columns(&table, &["Region", "Product", "Amount"]);
    assert_eq!(table.rows.len(), 5, "one row per non-blank region/product combination");
    assert_eq!(table.rows[0], vec![text("East"), text("Gadget"), money(30.0)]);
}

#[test]
fn groupby_aggregates_the_current_group() {
    let table = eval_table("GROUPBY(Sales, Sales[Region], \"Amount\", SUMX(CURRENTGROUP(), Sales[Amount]))").expect("groupby");
    assert_columns(&table, &["Region", "Amount"]);
    assert_eq!(
        table.rows,
        vec![vec![text("East"), money(45.0)], vec![text("North"), money(50.0)], vec![text("West"), money(20.0)], vec![Scalar::Blank, money(7.0)],]
    );

    let counted = eval_table("GROUPBY(Sales, Sales[Product], \"Rows\", COUNTROWS(CURRENTGROUP()))").expect("countrows");
    assert_eq!(counted.rows, vec![vec![text("Gadget"), int(3)], vec![text("Widget"), int(3)]]);

    let distinct = eval_table("GROUPBY(Sales)").expect("distinct rows");
    assert_eq!(distinct.rows.len(), 6, "GROUPBY without group columns returns the distinct rows");
}

#[test]
fn summarize_columns_performs_context_transition() {
    // Inside the row context of Sales each row filters all its columns, so only its own product
    // remains in a nested SUMMARIZECOLUMNS. Without context transition every row would see two.
    assert_eq!(eval("MAXX(Sales, COUNTROWS(SUMMARIZECOLUMNS(Sales[Product])))"), Ok(money(1.0)));
}

#[test]
fn currentgroup_is_only_valid_inside_groupby() {
    assert!(error("CURRENTGROUP()").contains("only valid inside GROUPBY"));
    assert!(scalar_error("GROUPBY(Sales, Sales[Region])").contains("returns a table"));
    assert!(
        error("GROUPBY(Sales, ROLLUP(Sales[Region]))").contains("column reference"),
        "the catalog rejects ROLLUP as a GROUPBY column before evaluation"
    );
}

#[test]
fn addmissingitems_adds_rows_for_missing_combinations() {
    let table = eval_table(
        "ADDMISSINGITEMS(Sales[Region], SUMMARIZECOLUMNS(Sales[Region], \"Amount\", \
         CALCULATE(SUM(Sales[Amount]), Sales[Product] = \"Widget\")), Sales[Region])",
    )
    .expect("addmissingitems");
    assert_columns(&table, &["Region", "Amount"]);
    assert_eq!(
        table.rows,
        vec![
            vec![text("East"), money(15.0)],
            vec![text("West"), money(20.0)],
            vec![text("North"), Scalar::Blank],
            vec![Scalar::Blank, Scalar::Blank],
        ]
    );
}

#[test]
fn addmissingitems_marks_rollup_subtotal_columns() {
    let table = eval_table(
        "ADDMISSINGITEMS(Sales[Region], SUMMARIZECOLUMNS(ROLLUPADDISSUBTOTAL(Sales[Region], \"IsRegionTotal\"), \"Amount\", \
         CALCULATE(SUM(Sales[Amount]), Sales[Product] = \"Widget\")), ROLLUPISSUBTOTAL(Sales[Region], [IsRegionTotal]))",
    )
    .expect("addmissingitems subtotals");
    assert_columns(&table, &["Region", "IsRegionTotal", "Amount"]);
    assert_eq!(
        table.rows,
        vec![
            vec![text("East"), Scalar::Boolean(false), money(15.0)],
            vec![text("West"), Scalar::Boolean(false), money(20.0)],
            vec![Scalar::Blank, Scalar::Boolean(true), money(35.0)],
            vec![text("North"), Scalar::Boolean(false), Scalar::Blank],
        ]
    );
}

#[test]
fn ignore_and_nonvisual_pass_their_argument_through() {
    let plain = eval_table("VALUES(Sales[Region])").expect("values");
    let ignored = eval_table("IGNORE(VALUES(Sales[Region]))").expect("ignore");
    assert_eq!(ignored, plain);
    let non_visual = eval_table("NONVISUAL(Sales)").expect("nonvisual");
    assert_eq!(non_visual.rows.len(), 6);
    assert_eq!(eval("IGNORE(1 + 2)"), Ok(int(3)));
}

#[test]
fn rollup_companions_are_rejected_outside_their_host() {
    assert!(error("ROLLUP(Sales[Region])").contains("only valid as a SUMMARIZE"));
    assert!(error("ROLLUPGROUP(Sales[Region])").contains("only valid inside ROLLUP"));
    assert!(scalar_error("ROLLUPADDISSUBTOTAL(Sales[Region], \"X\")").contains("only valid inside SUMMARIZECOLUMNS"));
    assert!(scalar_error("ROLLUPISSUBTOTAL(Sales[Region], [Total])").contains("only valid inside ADDMISSINGITEMS"));
    assert!(scalar_error("SUMMARIZECOLUMNS(Sales[Region])").contains("returns a table"));
}

#[test]
fn topnskip_skips_then_takes_ordered_rows() {
    let table = eval_table("TOPNSKIP(2, 1, Sales, Sales[Amount], 0)").expect("topnskip");
    assert_columns(&table, &["Region", "Product", "Amount", "Quantity"]);
    assert_eq!(
        table.rows.iter().map(|row| row[2].clone()).collect::<Vec<_>>(),
        vec![money(30.0), money(20.0)],
        "descending by Amount: 50 is skipped, then 30 and 20"
    );

    let ascending = eval_table("TOPNSKIP(2, 1, Sales, Sales[Amount], 1)").expect("ascending");
    assert_eq!(ascending.rows.iter().map(|row| row[2].clone()).collect::<Vec<_>>(), vec![money(7.0), money(10.0)]);
}

#[test]
fn topnskip_returns_exactly_the_requested_rows() {
    let empty = eval_table("TOPNSKIP(0, 0, Scores, Scores[Score], 0)").expect("zero rows");
    assert!(empty.rows.is_empty());
    assert_eq!(header(&empty), vec!["Team".to_string(), "Score".to_string()]);

    // Unlike TOPN, TOPNSKIP has no ties parameter: equal keys do not expand the result.
    let first = eval_table("TOPNSKIP(1, 0, Scores, Scores[Score], 0)").expect("first row");
    assert_eq!(first.rows.len(), 1);

    let skip = eval_table("TOPNSKIP(1, 2, Scores, Scores[Score], 0)").expect("skip rows");
    assert_eq!(skip.rows, vec![vec![text("C"), int(8)]]);
}

#[test]
fn topnskip_requires_lineage_when_skipping() {
    assert!(error("TOPNSKIP(1, 1, SUMMARIZE(Sales, Sales[Region]), Sales[Region], 0)").contains("lineage"));
}

#[test]
fn topnperlevel_returns_rows_per_level_with_restart_indicators() {
    let table = eval_table("TOPNPERLEVEL(2, Items, Levels, Nodes, Bounds, \"Restart\")").expect("topnperlevel");
    assert_columns(&table, &["Item", "Rank", "Restart"]);
    assert_eq!(table.rows, vec![vec![text("A"), int(1), Scalar::Blank], vec![text("B"), int(2), int(2)]]);

    let started = eval_table("TOPNPERLEVEL(5, Items, Levels, Nodes, BoundsStart, \"Restart\")").expect("bounded start");
    assert_eq!(started.rows, vec![vec![text("B"), int(2), int(1)], vec![text("C"), int(3), Scalar::Blank]]);
}

#[test]
fn topnperlevel_validates_its_arguments() {
    assert!(scalar_error("TOPNPERLEVEL(2, Items, Levels, Nodes, Bounds)").contains("expects 6"));
    assert!(error("TOPNPERLEVEL(2, Items, Levels, Nodes, Bounds, \"Rank\")").contains("already exists"));
    assert!(error("TOPNPERLEVEL(2, Items, Items, Nodes, Bounds, \"Restart\")").contains("level number"));
}

#[test]
fn summarize_evaluates_expressions_over_the_table_arguments_rows() {
    // `FILTER` narrows the table argument; East's 10 and 30 rows survive (Amount > 5 removes only
    // the 5), so East sums 10 + 30 = 40. Without the table rows in the group context the removed
    // row would be counted too.
    let table = eval_table("SUMMARIZE(FILTER(Sales, Sales[Amount] > 5), Sales[Region], \"Amount\", SUM(Sales[Amount]))").expect("summarize");
    assert_eq!(
        table.rows,
        vec![vec![text("East"), money(40.0)], vec![text("North"), money(50.0)], vec![text("West"), money(20.0)], vec![Scalar::Blank, money(7.0)]]
    );
}

#[test]
fn summarize_columns_without_expressions_keeps_only_existing_combinations() {
    // East never sells Hardware? Products map Widget -> Hardware; East sells Widget and Gadget, so
    // East × Hardware exists, West × Devices does not: only real Region/Product pairs survive.
    let table = eval_table("SUMMARIZECOLUMNS(Sales[Region], Sales[Product])").expect("summarize columns");
    assert_columns(&table, &["Region", "Product"]);
    assert!(!table.rows.iter().any(|row| row == &vec![text("West"), text("Gadget")]), "no invented pair: {:?}", table.rows);
    assert!(table.rows.iter().any(|row| row == &vec![text("East"), text("Widget")]));
    assert_eq!(table.rows.len(), 5, "one row per existing pair: {:?}", table.rows);
}

#[test]
fn summarize_accepts_projected_table_arguments_that_keep_lineage() {
    // `VALUES` keeps the source lineage while dropping duplicate rows, so the group owns fewer
    // rows than the lineage has under the group's filters. That is not a leak: every lineage row
    // the filters admit still carries a region the group owns.
    let table = eval_table("SUMMARIZE(VALUES(Sales[Region]), Sales[Region], \"T\", SUM(Sales[Amount]))").expect("summarize values");
    assert_columns(&table, &["Region", "T"]);
    assert_eq!(
        table.rows,
        vec![vec![text("East"), money(45.0)], vec![text("North"), money(50.0)], vec![text("West"), money(20.0)], vec![Scalar::Blank, money(7.0)]]
    );
}

#[test]
fn groupby_accepts_projected_table_arguments_that_keep_lineage() {
    // `DISTINCT`/`VALUES` deduplicate, so `COUNTROWS(CURRENTGROUP())` is 1 per group; the guard
    // must compare identities rather than counts and let the evaluation through.
    let table = eval_table("GROUPBY(VALUES(Sales[Region]), Sales[Region], \"Rows\", COUNTROWS(CURRENTGROUP()))").expect("groupby values");
    assert_columns(&table, &["Region", "Rows"]);
    assert_eq!(table.rows.len(), 4);
    assert!(table.rows.iter().all(|row| row[1] == int(1)), "one source row per region: {:?}", table.rows);
}

#[test]
fn topnskip_orders_mixed_type_keys_without_losing_stability() {
    // A key column that mixes text, numbers and a blank has pairs `Scalar::compare` cannot order.
    // A total order keeps the sort stable and makes the ascending result the exact reverse of the
    // descending one. NaN is avoided here only because `Scalar` equality is not reflexive for it.
    let mut model = DataModel {
        tables: vec![table(
            "Mix",
            vec![column("Key", "Variant")],
            vec![vec![text("b")], vec![int(5)], vec![int(10)], vec![Scalar::Blank], vec![text("A")], vec![money(-2.5)]],
        )],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let run = |source: &str| -> Vec<Vec<Scalar>> {
        let expression = parse_expression(source).expect("parse");
        let bound = Binder::new(&model).bind_expression(&expression).expect("bind");
        evaluator.evaluate_table(&bound).expect(source).rows
    };
    let descending = run("TOPNSKIP(6, 0, Mix, Mix[Key], 0)");
    let ascending = run("TOPNSKIP(6, 0, Mix, Mix[Key], 1)");
    assert_eq!(descending.len(), 6);
    let mut reversed = ascending.clone();
    reversed.reverse();
    assert_eq!(reversed, descending, "ascending {ascending:?} is not the reverse of descending {descending:?}");
}

#[test]
fn summarize_rejects_cross_column_table_filters_that_leak_rows() {
    // The predicate compares two columns, so one value set per column cannot rebuild the group:
    // East keeps (Amount 5, Cost 3) and (9, 8), while the removed (5, 8) matches the Amount set
    // {5, 9} and the Cost set {3, 8}. Aggregating that row would return 19 instead of 14, so the
    // evaluation has to fail rather than answer with rows the filter removed.
    let mut model = DataModel {
        tables: vec![table(
            "Sales",
            vec![column("Region", "String"), column("Amount", "Double"), column("Cost", "Double")],
            vec![vec![text("East"), money(5.0), money(3.0)], vec![text("East"), money(9.0), money(8.0)], vec![text("East"), money(5.0), money(8.0)]],
        )],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expression =
        parse_expression("SUMMARIZE(FILTER(Sales, Sales[Amount] > Sales[Cost]), Sales[Region], \"T\", SUM(Sales[Amount]))").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expression).expect("bind");
    let message = match evaluator.evaluate_table(&bound) {
        Ok(table) => panic!("cross-column filter should fail, returned {table:?}"),
        Err(error) => error.to_string(),
    };
    assert!(message.contains("SUMMARIZE"), "error names the operation: {message}");
    assert!(message.contains("table argument"), "error names the table argument: {message}");
}

#[test]
fn summarize_detects_a_leak_when_a_group_filter_replaces_a_query_filter() {
    // The query filter keeps only East rows and the table argument carries every East row, so a
    // comparison against the query filters alone sees no leak. The West group then replaces the
    // Region filter, and its per-column sets admit the West (5, 8) row the predicate removed:
    // aggregating it would return 19 instead of 14. The check must run despite the query filter.
    let mut model = DataModel {
        tables: vec![table(
            "Sales",
            vec![column("Region", "String"), column("Amount", "Double"), column("Cost", "Double")],
            vec![
                vec![text("East"), money(5.0), money(3.0)],
                vec![text("East"), money(9.0), money(8.0)],
                vec![text("West"), money(5.0), money(3.0)],
                vec![text("West"), money(9.0), money(8.0)],
                vec![text("West"), money(5.0), money(8.0)],
            ],
        )],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expression = parse_expression(
        "CALCULATETABLE(SUMMARIZE(FILTER(ALL(Sales), Sales[Amount] > Sales[Cost]), Sales[Region], \"T\", SUM(Sales[Amount])), Sales[Region] = \"East\")",
    )
    .expect("parse");
    let bound = Binder::new(&model).bind_expression(&expression).expect("bind");
    let message = match evaluator.evaluate_table(&bound) {
        Ok(table) => panic!("the West group's replaced filter admits the removed row; returned {table:?}"),
        Err(error) => error.to_string(),
    };
    assert!(message.contains("SUMMARIZE"), "error names the operation: {message}");
    assert!(message.contains("table argument"), "error names the table argument: {message}");
}

#[test]
fn issue_subtotal_marks_only_the_rolled_up_rows() {
    let table = eval_table("SUMMARIZE(Sales, ROLLUP(Sales[Region]), \"IsSubtotal\", ISSUBTOTAL(Sales[Region]))").expect("issue subtotal");
    assert_columns(&table, &["Region", "IsSubtotal"]);
    assert_eq!(table.rows.len(), 5, "four regions plus the grand total");
    // Detail rows are FALSE; the blank-region grand total is the only subtotal row.
    let flags: Vec<&Scalar> = table.rows.iter().map(|row| &row[1]).collect();
    assert_eq!(flags.iter().filter(|flag| ***flag == Scalar::Boolean(true)).count(), 1);
    assert_eq!(table.rows.last().expect("grand total")[1], Scalar::Boolean(true));
}
