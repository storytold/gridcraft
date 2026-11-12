//! Evaluator tests for the non-metadata information functions.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, ScalarKey, TableValue};
use gridcraft_dax::{Binder, Evaluator, FilterContext, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Color".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Price".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Cost".into(), data_type: Some("Currency".into()) },
                    ModelColumn { name: "Active".into(), data_type: Some("Boolean".into()) },
                    ModelColumn { name: "LaunchDate".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![
                        Scalar::Integer(1),
                        Scalar::String("Wireless Headphones".into()),
                        Scalar::String("Red".into()),
                        Scalar::String("Audio".into()),
                        Scalar::Decimal(20.0),
                        Scalar::Currency(150_000),
                        Scalar::Boolean(true),
                        Scalar::DateTime(45292.0),
                    ],
                    vec![
                        Scalar::Integer(2),
                        Scalar::String("Red Widget".into()),
                        Scalar::String("Red".into()),
                        Scalar::String("Audio".into()),
                        Scalar::Decimal(30.5),
                        Scalar::Currency(200_000),
                        Scalar::Boolean(true),
                        Scalar::DateTime(45300.0),
                    ],
                    vec![
                        Scalar::Integer(3),
                        Scalar::String("Blue Gadget".into()),
                        Scalar::String("Blue".into()),
                        Scalar::String("Computers".into()),
                        Scalar::Decimal(10.0),
                        Scalar::Currency(50_000),
                        Scalar::Boolean(false),
                        Scalar::DateTime(45310.0),
                    ],
                    vec![
                        Scalar::Integer(4),
                        Scalar::String("Noise Cancel Headphones".into()),
                        Scalar::String("Blue".into()),
                        Scalar::String("Audio".into()),
                        Scalar::Decimal(40.0),
                        Scalar::Currency(300_000),
                        Scalar::Boolean(true),
                        Scalar::DateTime(45320.0),
                    ],
                ],
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Decimal(10.0), Scalar::Integer(2), Scalar::Integer(1)],
                    vec![Scalar::String("West".into()), Scalar::Decimal(20.0), Scalar::Integer(3), Scalar::Integer(2)],
                    vec![Scalar::String("East".into()), Scalar::Decimal(30.0), Scalar::Integer(4), Scalar::Integer(1)],
                ],
            },
            ModelTable {
                name: "EmptyStats".into(),
                columns: vec![ModelColumn { name: "Empty Value".into(), data_type: Some("String".into()) }],
                rows: vec![],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "ProductKey".into(),
            to_table: "Product".into(),
            to_column: "ProductKey".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![
            Measure {
                name: "Total Sales".into(),
                formula: "SUM(Sales[Amount])".into(),
                table: Some("Sales".into()),
                format: Some("#,0.00".into()),
                expr: None,
            },
            Measure { name: "Total Quantity".into(), formula: "SUM(Sales[Quantity])".into(), table: Some("Sales".into()), format: None, expr: None },
        ],
    };
    model.parse_formulas().expect("fixture model parses");
    model
}

fn eval(src: &str) -> Result<Scalar, String> {
    evaluate(src, FilterContext::default())
}

fn eval_with(src: &str, build_filters: impl FnOnce(&DataModel) -> FilterContext) -> Result<Scalar, String> {
    let model = model();
    let filters = build_filters(&model);
    evaluate_with_model(&model, src, filters)
}

fn evaluate(src: &str, filters: FilterContext) -> Result<Scalar, String> {
    let model = model();
    evaluate_with_model(&model, src, filters)
}

fn evaluate_with_model(model: &DataModel, src: &str, filters: FilterContext) -> Result<Scalar, String> {
    let evaluator = Evaluator::new(model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_with(&bound, filters).map_err(|error| error.message)
}

fn eval_table(src: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.message)
}

fn number(src: &str) -> f64 {
    match eval(src) {
        Ok(value) => value.to_number().unwrap_or_else(|| panic!("`{src}` did not evaluate to a number: {value:?}")),
        Err(error) => panic!("`{src}` failed: {error}"),
    }
}

fn boolean(src: &str) -> bool {
    match eval(src) {
        Ok(Scalar::Boolean(value)) => value,
        other => panic!("`{src}` did not evaluate to a Boolean: {other:?}"),
    }
}

fn error_message(src: &str) -> String {
    match eval(src) {
        Ok(value) => panic!("`{src}` should fail, got {value:?}"),
        Err(message) => message,
    }
}

fn filter(model: &DataModel, table: &str, column: &str, values: &[Scalar]) -> FilterContext {
    let mut filters = FilterContext::default();
    let table = model.table_index(table).expect("table exists");
    let column = model.column_index(table, column).expect("column exists");
    filters.set(table, column, values.iter().map(ScalarKey::from));
    filters
}

#[test]
fn column_statistics_reports_every_column() {
    let table = eval_table("COLUMNSTATISTICS()").expect("column statistics");
    let names: Vec<&str> = table.columns.iter().map(|column| column.name.as_str()).collect();
    assert_eq!(names, ["Table Name", "Column Name", "Min", "Max", "Cardinality", "Max Length"]);
    // Product has 8 columns, Sales has 4 and EmptyStats has 1.
    assert_eq!(table.rows.len(), 13);

    let row = table
        .rows
        .iter()
        .find(|row| row[0] == Scalar::String("Product".into()) && row[1] == Scalar::String("Name".into()))
        .expect("Product[Name] row");
    assert_eq!(row[2], Scalar::String("Blue Gadget".into()));
    assert_eq!(row[3], Scalar::String("Wireless Headphones".into()));
    assert_eq!(row[4], Scalar::Integer(4));
    assert_eq!(row[5], Scalar::Integer(23));

    let price = table
        .rows
        .iter()
        .find(|row| row[0] == Scalar::String("Product".into()) && row[1] == Scalar::String("Price".into()))
        .expect("Product[Price] row");
    assert_eq!(price[2], Scalar::Decimal(10.0));
    assert_eq!(price[3], Scalar::Decimal(40.0));
    assert_eq!(price[4], Scalar::Integer(4));
    assert_eq!(price[5], Scalar::Blank);

    let empty = table.rows.iter().find(|row| row[0] == Scalar::String("EmptyStats".into())).expect("EmptyStats row");
    assert_eq!(empty[2], Scalar::Blank);
    assert_eq!(empty[3], Scalar::Blank);
    assert_eq!(empty[4], Scalar::Integer(0));
    assert_eq!(empty[5], Scalar::Blank);
}

#[test]
fn contains_and_containsrow() {
    assert!(boolean("CONTAINS(Product, Product[Color], \"Red\")"));
    assert!(boolean("CONTAINS(Product, Product[Color], \"Red\", Product[Category], \"Audio\")"));
    assert!(!boolean("CONTAINS(Product, Product[Color], \"Red\", Product[Category], \"Computers\")"));
    // Strict equality: BLANK does not match the number 0.
    assert!(!boolean("CONTAINS(Product, Product[Color], BLANK())"));
    assert!(boolean("EVALUATEANDLOG(CONTAINS(Product, Product[ProductKey], 3))"));
    assert!(error_message("CONTAINS(Product, Product[Color], \"Red\", Product[Name])").contains("column/value pairs"));

    assert!(boolean("CONTAINSROW({1, 2}, 2)"));
    assert!(!boolean("CONTAINSROW({1, 2}, 3)"));
    // One value per table column: `Sales` is (Region, Amount, Quantity, ProductKey).
    assert!(boolean("CONTAINSROW(Sales, \"East\", 10.0, 2, 1)"));
    assert!(!boolean("CONTAINSROW(Sales, \"West\", 10.0, 2, 1)"));
    // A value count that is not the column count is an error, never a prefix match.
    assert!(error_message("CONTAINSROW(Sales, \"East\")").contains("one value"));
    assert!(error_message("CONTAINSROW({1}, 1, 2)").contains("one value"));
}

#[test]
fn contains_string_variants() {
    assert!(boolean("CONTAINSSTRING(\"Hello World\", \"ell\")"));
    assert!(boolean("CONTAINSSTRING(\"Hello World\", \"WORLD\")"));
    assert!(!boolean("CONTAINSSTRING(\"Hello World\", \"xyz\")"));
    // Excel wildcards: `?` one character, `*` any run, `~` escapes a wildcard.
    assert!(boolean("CONTAINSSTRING(\"Hello World\", \"W?rld\")"));
    assert!(boolean("CONTAINSSTRING(\"Hello World\", \"H*d\")"));
    assert!(boolean("CONTAINSSTRING(\"a*b\", \"~*\")"));
    assert!(!boolean("CONTAINSSTRING(\"abc\", \"~*\")"));
    assert!(boolean("CONTAINSSTRING(BLANK(), \"\")"));

    assert!(boolean("CONTAINSSTRINGEXACT(\"Hello World\", \"World\")"));
    assert!(!boolean("CONTAINSSTRINGEXACT(\"Hello World\", \"world\")"));
    // Exact matching does not interpret wildcards.
    assert!(boolean("CONTAINSSTRINGEXACT(\"a*b\", \"*\")"));
    assert!(!boolean("CONTAINSSTRINGEXACT(\"Hello\", \"Ell\")"));
}

#[test]
fn user_and_connection_functions_return_defaults() {
    assert_eq!(eval("CUSTOMDATA()"), Ok(Scalar::String(String::new())));
    assert_eq!(eval("USERCULTURE()"), Ok(Scalar::String("en-US".into())));
    assert_eq!(eval("USEROBJECTID()"), Ok(Scalar::String(String::new())));
    // USERNAME is the `DOMAIN\user` identity; USERPRINCIPALNAME is the `user@domain` directory
    // principal. On a test host without a user both are empty.
    let host_user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let username = match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() && !host_user.is_empty() => format!("{domain}\\{host_user}"),
        _ => host_user.clone(),
    };
    let principal = match (
        std::env::var("USERDNSDOMAIN").ok().filter(|domain| !domain.is_empty()),
        std::env::var("USERDOMAIN").ok().filter(|domain| !domain.is_empty()),
        host_user.as_str(),
    ) {
        (_, _, "") => host_user,
        (Some(domain), _, user) | (None, Some(domain), user) => format!("{user}@{}", domain.to_ascii_lowercase()),
        (None, None, user) => user.to_string(),
    };
    assert_eq!(eval("USERNAME()"), Ok(Scalar::String(username)));
    assert_eq!(eval("USERPRINCIPALNAME()"), Ok(Scalar::String(principal)));
    // The principal name is the `user@domain` form, never the `DOMAIN\user` form.
    assert!(!matches!(eval("USERPRINCIPALNAME()"), Ok(Scalar::String(value)) if value.contains('\\')));
}

#[test]
fn evaluate_and_log_returns_its_argument() {
    assert_eq!(eval("EVALUATEANDLOG(42)"), Ok(Scalar::Integer(42)));
    assert_eq!(eval("EVALUATEANDLOG(7, \"label\", 5)"), Ok(Scalar::Integer(7)));
    let table = eval_table("EVALUATEANDLOG(Sales)").expect("logged table");
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.columns.len(), 4);
    assert!(eval("EVALUATEANDLOG()").is_err());
}

#[test]
fn filter_context_predicates() {
    assert!(!boolean("HASONEVALUE(Product[Color])"));
    assert!(!boolean("HASONEVALUE(Product[Category])"));

    let single_category =
        eval_with("HASONEVALUE(Product[Category])", |model| filter(model, "Product", "Category", &[Scalar::String("Audio".into())]))
            .expect("HASONEVALUE");
    assert_eq!(single_category, Scalar::Boolean(true));

    let two_colors = eval_with("HASONEVALUE(Product[Color])", |model| {
        filter(model, "Product", "Color", &[Scalar::String("Red".into()), Scalar::String("Blue".into())])
    })
    .expect("HASONEVALUE with two values");
    assert_eq!(two_colors, Scalar::Boolean(false));

    let single_color_filter =
        eval_with("HASONEFILTER(Product[Color])", |model| filter(model, "Product", "Color", &[Scalar::String("Red".into())])).expect("HASONEFILTER");
    assert_eq!(single_color_filter, Scalar::Boolean(true));

    let propagated = eval_with("HASONEFILTER(Product[Color])", |model| filter(model, "Product", "Category", &[Scalar::String("Computers".into())]))
        .expect("HASONEFILTER ignores propagated filters");
    assert_eq!(propagated, Scalar::Boolean(false));

    let two_value_filter = eval_with("HASONEFILTER(Product[Color])", |model| {
        filter(model, "Product", "Color", &[Scalar::String("Red".into()), Scalar::String("Blue".into())])
    })
    .expect("HASONEFILTER with two values");
    assert_eq!(two_value_filter, Scalar::Boolean(false));

    let empty_column = eval("HASONEVALUE(EmptyStats[Empty Value])").expect("empty column");
    assert_eq!(empty_column, Scalar::Boolean(false));

    assert!(!boolean("ISFILTERED(Product[Color])"));
    let filtered =
        eval_with("ISFILTERED(Product[Color])", |model| filter(model, "Product", "Color", &[Scalar::String("Red".into())])).expect("ISFILTERED");
    assert_eq!(filtered, Scalar::Boolean(true));

    let unrelated = eval_with("ISFILTERED(Product[Name])", |model| filter(model, "Product", "Color", &[Scalar::String("Red".into())]))
        .expect("ISFILTERED other column");
    assert_eq!(unrelated, Scalar::Boolean(false));
}

#[test]
fn cross_filtered_follows_relationship_propagation() {
    assert!(!boolean("ISCROSSFILTERED(Sales)"));
    assert!(!boolean("ISCROSSFILTERED(Product)"));

    let from_dimension = |model: &DataModel| filter(model, "Product", "Category", &[Scalar::String("Audio".into())]);
    assert_eq!(eval_with("ISCROSSFILTERED(Sales)", from_dimension).expect("sales"), Scalar::Boolean(true));
    assert_eq!(eval_with("ISCROSSFILTERED(Sales[Amount])", from_dimension).expect("sales column"), Scalar::Boolean(true));
    assert_eq!(eval_with("ISCROSSFILTERED(Product)", from_dimension).expect("product"), Scalar::Boolean(true));

    // Filtering the many-side fact table does not propagate to the one-side dimension.
    let from_fact = |model: &DataModel| filter(model, "Sales", "Region", &[Scalar::String("East".into())]);
    assert_eq!(eval_with("ISCROSSFILTERED(Product)", from_fact).expect("product"), Scalar::Boolean(false));
    assert_eq!(eval_with("ISCROSSFILTERED(Sales)", from_fact).expect("sales"), Scalar::Boolean(true));
}

/// `Sales -> Product -> Category`, plus an inactive `Product -> AltCategory` link so
/// `USERELATIONSHIP` can switch a hop on.
fn snowflake_model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Category".into(),
                columns: vec![
                    ModelColumn { name: "CategoryKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                ],
                rows: vec![vec![Scalar::Integer(1), Scalar::String("Audio".into())], vec![Scalar::Integer(2), Scalar::String("Computers".into())]],
            },
            ModelTable {
                name: "AltCategory".into(),
                columns: vec![
                    ModelColumn { name: "CategoryKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                ],
                rows: vec![vec![Scalar::Integer(1), Scalar::String("Audio".into())], vec![Scalar::Integer(2), Scalar::String("Computers".into())]],
            },
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "CategoryKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "AltCategoryKey".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::Integer(1), Scalar::Integer(1)],
                    vec![Scalar::Integer(2), Scalar::Integer(1), Scalar::Integer(2)],
                    vec![Scalar::Integer(3), Scalar::Integer(2), Scalar::Integer(1)],
                ],
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::Decimal(10.0)],
                    vec![Scalar::Integer(2), Scalar::Decimal(20.0)],
                    vec![Scalar::Integer(3), Scalar::Decimal(30.0)],
                ],
            },
        ],
        relationships: vec![
            Relationship {
                from_table: "Sales".into(),
                from_column: "ProductKey".into(),
                to_table: "Product".into(),
                to_column: "ProductKey".into(),
                bidirectional: false,
                inactive: false,
            },
            Relationship {
                from_table: "Product".into(),
                from_column: "CategoryKey".into(),
                to_table: "Category".into(),
                to_column: "CategoryKey".into(),
                bidirectional: false,
                inactive: false,
            },
            Relationship {
                from_table: "Product".into(),
                from_column: "AltCategoryKey".into(),
                to_table: "AltCategory".into(),
                to_column: "CategoryKey".into(),
                bidirectional: false,
                inactive: true,
            },
        ],
        measures: vec![],
    };
    model.parse_formulas().expect("fixture model parses");
    model
}

#[test]
fn cross_filtered_follows_the_whole_chain_and_relationship_overrides() {
    let model = snowflake_model();
    let with = |filters: FilterContext, src: &str| evaluate_with_model(&model, src, filters);

    assert_eq!(with(FilterContext::default(), "ISCROSSFILTERED(Sales)").expect("unfiltered sales"), Scalar::Boolean(false));
    assert_eq!(with(FilterContext::default(), "ISCROSSFILTERED(Product)").expect("unfiltered product"), Scalar::Boolean(false));

    // Two hops away: `Category -> Product -> Sales`. A single-hop check misses this.
    let category = filter(&model, "Category", "Name", &[Scalar::String("Audio".into())]);
    assert_eq!(with(category.clone(), "ISCROSSFILTERED(Sales)").expect("sales"), Scalar::Boolean(true));
    assert_eq!(with(category.clone(), "ISCROSSFILTERED(Product)").expect("product"), Scalar::Boolean(true));
    assert_eq!(with(category.clone(), "ISCROSSFILTERED(Category)").expect("category"), Scalar::Boolean(true));
    // The chain only runs towards the many side; the inactive branch stays out of it.
    assert_eq!(with(category.clone(), "ISCROSSFILTERED(AltCategory)").expect("alt category"), Scalar::Boolean(false));

    // A filter on the fact table does not climb the one-way chain towards `Category`.
    let sales = filter(&model, "Sales", "Amount", &[Scalar::Decimal(10.0)]);
    assert_eq!(with(sales.clone(), "ISCROSSFILTERED(Category)").expect("category"), Scalar::Boolean(false));
    assert_eq!(with(sales.clone(), "ISCROSSFILTERED(Sales)").expect("sales"), Scalar::Boolean(true));

    // An inactive relationship is ignored until `USERELATIONSHIP` activates it; the override in
    // the filter context, not the model's `inactive` flag, must decide.
    let alt_category = filter(&model, "AltCategory", "Name", &[Scalar::String("Audio".into())]);
    assert_eq!(with(alt_category.clone(), "ISCROSSFILTERED(Sales)").expect("inactive link"), Scalar::Boolean(false));
    let activation = "CALCULATE(ISCROSSFILTERED(Sales), USERELATIONSHIP(Product[AltCategoryKey], AltCategory[CategoryKey]))";
    assert_eq!(with(alt_category.clone(), activation).expect("activated link"), Scalar::Boolean(true));
    // Activating the other pair leaves the `Category` chain alone.
    assert_eq!(with(alt_category, "ISCROSSFILTERED(Category)").expect("category"), Scalar::Boolean(false));

    // `CROSSFILTER` widens the fact-side filter back to the dimension for the surrounding
    // CALCULATE only.
    let widened = "CALCULATE(ISCROSSFILTERED(Product), CROSSFILTER(Sales[ProductKey], Product[ProductKey], BOTH))";
    assert_eq!(with(sales, widened).expect("bidirectional override"), Scalar::Boolean(true));
}

#[test]
fn scope_and_subtotal_predicates() {
    // Outside a `SUMMARIZE` group no column is in scope, and a row iterator's row context does
    // not put one there either: scope is the group-by set of the group being evaluated.
    assert!(!boolean("ISINSCOPE(Product[Color])"));
    assert_eq!(number("SUMX(Product, IF(ISINSCOPE(Product[Color]), 1, 0))"), 0.0);
    assert_eq!(number("SUMX(Sales, IF(ISINSCOPE(Product[Color]), 1, 0))"), 0.0);
}

#[test]
fn is_empty_checks_result_rows() {
    assert!(!boolean("ISEMPTY(Sales)"));
    assert!(boolean("ISEMPTY(FILTER(Sales, Sales[Amount] > 1000))"));
    assert!(boolean("ISEMPTY(EmptyStats)"));
    assert!(boolean("ISEMPTY({})"));
}

#[test]
fn type_predicates_are_strict() {
    assert!(boolean("ISBOOLEAN(TRUE)"));
    assert!(!boolean("ISBOOLEAN(1)"));
    assert!(!boolean("ISBOOLEAN(BLANK())"));
    assert!(boolean("ISSTRING(\"text\")"));
    assert!(!boolean("ISSTRING(1)"));
    assert!(!boolean("ISSTRING(BLANK())"));
    assert!(boolean("ISINT64(1)"));
    assert!(!boolean("ISINT64(1.5)"));
    assert!(!boolean("ISDECIMAL(1.5)"));
    assert!(boolean("ISDOUBLE(1.5)"));
    assert!(!boolean("ISDOUBLE(1)"));
    assert!(boolean("ISDATETIME(DATE(2024, 1, 1))"));
    assert!(!boolean("ISDATETIME(1)"));
    assert!(boolean("ISNUMERIC(1)"));
    assert!(boolean("ISNUMERIC(1.5)"));
    assert!(!boolean("ISNUMERIC(\"1\")"));
    assert!(!boolean("ISNUMERIC(TRUE)"));
    assert!(!boolean("ISNUMERIC(DATE(2024, 1, 1))"));

    // Column values in a row context keep their storage types.
    assert_eq!(number("SUMX(Product, IF(ISDECIMAL(Product[Cost]), 1, 0))"), 4.0);
    assert_eq!(number("SUMX(Product, IF(ISDOUBLE(Product[Price]), 1, 0))"), 4.0);
    assert_eq!(number("SUMX(Product, IF(ISDOUBLE(Product[Cost]), 1, 0))"), 0.0);
    assert_eq!(number("SUMX(Product, IF(ISINT64(Product[ProductKey]), 1, 0))"), 4.0);
    assert_eq!(number("SUMX(Product, IF(ISNUMERIC(Product[Cost]), 1, 0))"), 4.0);
    assert_eq!(number("SUMX(Product, IF(ISNUMERIC(Product[Active]), 1, 0))"), 0.0);
    assert_eq!(number("SUMX(Product, IF(ISDATETIME(Product[LaunchDate]), 1, 0))"), 4.0);
    assert_eq!(number("SUMX(Product, IF(ISSTRING(Product[Name]), 1, 0))"), 4.0);
    assert_eq!(number("SUMX(Product, IF(ISBOOLEAN(Product[Active]), 1, 0))"), 4.0);
}

#[test]
fn isafter_and_isonorafter_compare_groups() {
    assert!(boolean("ISAFTER(2, 1)"));
    assert!(!boolean("ISAFTER(1, 1)"));
    assert!(!boolean("ISAFTER(1, 2)"));
    assert!(boolean("ISAFTER(1, 2, FALSE)"));
    assert!(boolean("ISAFTER(1, 2, 0)"));
    assert!(boolean("ISAFTER(\"b\", \"A\")"));
    // The first differing pair decides; equal pairs fall through to the next group.
    assert!(boolean("ISAFTER(1, 1, TRUE, 5, 4)"));
    assert!(!boolean("ISAFTER(1, 1, TRUE, 4, 5)"));
    // BLANK sorts before every value, including the empty string.
    assert!(boolean("ISAFTER(\"\", BLANK())"));
    assert!(!boolean("ISAFTER(BLANK(), \"\")"));
    assert!(error_message("ISAFTER(\"abc\", 1)").contains("cannot compare"));

    assert!(boolean("ISONORAFTER(2, 1)"));
    assert!(boolean("ISONORAFTER(1, 1)"));
    assert!(!boolean("ISONORAFTER(0, 1)"));
    assert!(boolean("ISONORAFTER(1, 2, FALSE)"));
    assert!(!boolean("ISONORAFTER(1, 1, TRUE, 4, 5)"));
    assert!(boolean("ISONORAFTER(1, 1, TRUE, 5, 4)"));
    assert!(!boolean("ISONORAFTER(BLANK(), \"\")"));
    assert!(eval("ISONORAFTER(1)").is_err());
}

#[test]
fn selected_measure_functions_require_a_calculation_item() {
    assert!(!boolean("ISSELECTEDMEASURE([Total Sales])"));
    assert!(!boolean("ISSELECTEDMEASURE([Total Sales], [Total Quantity])"));
    assert!(error_message("SELECTEDMEASURE()").contains("selected measure"));
    assert!(error_message("SELECTEDMEASURENAME()").contains("selected measure"));
    assert!(error_message("SELECTEDMEASUREFORMATSTRING()").contains("selected measure"));
}

#[test]
fn nameof_formats_and_escapes_names() {
    assert_eq!(eval("NAMEOF(Sales[Amount])"), Ok(Scalar::String("'Sales'[Amount]".into())));
    assert_eq!(eval("NAMEOF([Total Sales])"), Ok(Scalar::String("'Sales'[Total Sales]".into())));
    assert_eq!(eval("NAMEOF(Sales)"), Ok(Scalar::String("'Sales'".into())));
    assert_eq!(eval("NAMEOF(Sales[Amount], \"TABLE\")"), Ok(Scalar::String("'Sales'".into())));
    assert_eq!(eval("NAMEOF([Total Sales], \"PARENT\")"), Ok(Scalar::String("'Sales'".into())));
    assert_eq!(eval("NAMEOF(Product[Name], \"SELF\")"), Ok(Scalar::String("[Name]".into())));
    assert_eq!(eval("NAMEOF(Sales[Amount], \"COLUMN\")"), Ok(Scalar::String("[Amount]".into())));
    assert_eq!(eval("NAMEOF([Total Sales], \"MEASURE\")"), Ok(Scalar::String("[Total Sales]".into())));
    assert_eq!(eval("NAMEOF(Sales, \"FULL\", \"UNESCAPED\")"), Ok(Scalar::String("Sales".into())));
    assert_eq!(eval("NAMEOF(Product[Name], \"SELF\", \"UNESCAPED\")"), Ok(Scalar::String("Name".into())));
    assert_eq!(eval("NAMEOF(Product[Name], \"FULL\", \"MINIMALLYESCAPED\")"), Ok(Scalar::String("Product[Name]".into())));
    assert_eq!(eval("NAMEOF([Total Sales], \"SELF\", \"MINIMALLYESCAPED\")"), Ok(Scalar::String("[Total Sales]".into())));

    assert!(error_message("NAMEOF(Sales[Amount], \"FULL\", \"UNESCAPED\")").contains("fully qualified"));
    assert!(error_message("NAMEOF(Sales, \"PARENT\")").contains("no parent"));
    assert!(error_message("NAMEOF(Sales[Amount], \"CALENDAR\")").contains("no calendar"));
    assert!(error_message("NAMEOF(Sales[Amount], \"BOGUS\")").contains("does not know component"));
    assert!(error_message("NAMEOF(1)").contains("table, column or measure"));
}

#[test]
fn tableof_returns_the_containing_table() {
    for source in ["TABLEOF(Sales[Amount])", "TABLEOF(Sales)", "TABLEOF([Total Sales])"] {
        let table = eval_table(source).expect(source);
        assert_eq!(table.rows.len(), 3, "{source}");
        assert_eq!(table.columns.len(), 4, "{source}");
    }
    // Unqualified column names resolve when a single table carries them.
    let empty = eval_table("TABLEOF([Empty Value])").expect("TABLEOF empty column");
    assert_eq!(empty.rows.len(), 0);
    assert_eq!(empty.columns.len(), 1);
}

#[test]
fn text_contains_match_modes() {
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"wireless bluetooth\")))"), 1.0);
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"WIRELESS\")))"), 1.0);
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"zzz\")))"), 0.0);
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"\")))"), 0.0);
    // Typo-tolerant matching accepts an edit distance of 1-2 for longer tokens.
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"headphone\", \"FUZZYMATCHING\")))"), 2.0);
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"hedphone\", \"FUZZYMATCHING\")))"), 2.0);
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"headphone\")))"), 0.0);
    // Phrase matching requires adjacent tokens in the specified order.
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"noise cancel\", \"PHRASEMATCHING\")))"), 1.0);
    assert_eq!(number("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"cancel noise\", \"PHRASEMATCHING\")))"), 0.0);
    assert!(error_message("COUNTROWS(FILTER(Product, TEXTCONTAINS(Product[Name], \"x\", \"BOGUS\")))").contains("match mode"));
}

#[test]
fn text_similarity_scores_matches() {
    let red = number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"red\"))");
    assert!(red > 0.0, "red score {red}");
    assert_eq!(number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"zzz\"))"), 0.0);
    assert!((number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"RED\"))") - red).abs() < 1e-12);

    let both = number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"red widget\"))");
    assert!(both > red, "a two-term match ({both}) should outrank one term ({red})");

    assert!(number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"hedphone\", \"FUZZYMATCHING\"))") > 0.0);
    assert_eq!(number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"hedphone\"))"), 0.0);
    assert!(number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"noise cancel\", \"PHRASEMATCHING\"))") > 0.0);
    assert_eq!(number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"cancel noise\", \"PHRASEMATCHING\"))"), 0.0);
    let blank = number("MAXX(Product, TEXTSIMILARITY(Product[Name], BLANK()))");
    assert_eq!(blank, 0.0);
}

#[test]
fn text_similarity_scores_each_row_against_the_shared_corpus() {
    // `MAXX` evaluates `TEXTSIMILARITY` once per row, and every one of those calls ranks against
    // the same column corpus (4 documents, 9 tokens, `red` in one document). The statistics are
    // memoised, so the row that matches must score exactly as it does on its own.
    let score = number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"red\"))");
    // k1 = 1.2, b = 0.75: idf = ln(1 + (4 - 1 + 0.5) / (1 + 0.5)); the target has two tokens and
    // the corpus averages 9 / 4 = 2.25, so the denominator is 1 + 1.2 * (0.25 + 0.75 * 2 / 2.25).
    assert!((score - 1.261_304_842_627_171_4).abs() < 1e-9, "unexpected score {score}");
    // The same row scored through its own filter context gets the same value.
    let scoped = evaluate("TEXTSIMILARITY(Product[Name], \"red\")", filter(&model(), "Product", "Name", &[Scalar::String("Red Widget".into())]))
        .expect("scoped score");
    assert_eq!(scoped, Scalar::Decimal(score));
}

#[test]
fn text_similarity_reports_an_oversized_column_instead_of_rescoring() {
    // Beyond the text work budget the function must fail: substituting a cheaper corpus would
    // silently change every score for the same column.
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Notes".into(),
            columns: vec![ModelColumn { name: "Text".into(), data_type: Some("String".into()) }],
            rows: (0..1500).map(|index| vec![Scalar::String(format!("note {index} {}", "x".repeat(1000)))]).collect(),
        }],
        relationships: vec![],
        measures: vec![],
    };
    model.parse_formulas().expect("fixture model parses");
    let error = evaluate_with_model(&model, "MAXX(Notes, TEXTSIMILARITY(Notes[Text], \"note\"))", FilterContext::default())
        .expect_err("a column beyond the work budget");
    assert!(error.contains("too large"), "unexpected error: {error}");
}
/// A one-column model whose rows are the given texts.
fn corpus_model(documents: [&str; 3]) -> DataModel {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "NoteLines".into(),
            columns: vec![ModelColumn { name: "Text".into(), data_type: Some("String".into()) }],
            rows: documents.iter().map(|document| vec![Scalar::String((*document).into())]).collect(),
        }],
        relationships: vec![],
        measures: vec![],
    };
    model.parse_formulas().expect("fixture model parses");
    model
}

#[test]
fn text_similarity_keeps_each_model_s_corpus() {
    // The two corpora have the same shape, the same first row and the same last row; only the
    // middle document differs. Statistics memoised on anything weaker than the model itself would
    // score the second corpus with the first one's document frequencies.
    let dense = corpus_model(["red alpha", "red beta", "same tail"]);
    let sparse = corpus_model(["red alpha", "blue beta", "same tail"]);
    let score =
        |model: &DataModel| match evaluate_with_model(model, "MAXX(NoteLines, TEXTSIMILARITY(NoteLines[Text], \"red\"))", FilterContext::default()) {
            Ok(Scalar::Decimal(value)) => value,
            other => panic!("unexpected result: {other:?}"),
        };
    let dense_score = score(&dense);
    let sparse_score = score(&sparse);
    // `red` is in two of the three documents above and in one of them below, so it is rarer and
    // scores higher.
    assert!(dense_score > 0.0, "dense score {dense_score}");
    assert!(sparse_score > dense_score, "sparse {sparse_score} should outrank dense {dense_score}");
}
#[test]
fn text_similarity_sees_an_edited_corpus() {
    // The model address and row count stay the same when a caller edits a text cell, so the memo
    // must be invalidated by the corpus fingerprint rather than by those two facts alone.
    let mut model = corpus_model(["red alpha", "blue beta", "same tail"]);
    let score =
        |model: &DataModel| match evaluate_with_model(model, "MAXX(NoteLines, TEXTSIMILARITY(NoteLines[Text], \"red\"))", FilterContext::default()) {
            Ok(Scalar::Decimal(value)) => value,
            other => panic!("unexpected result: {other:?}"),
        };
    let rare = score(&model);
    model.tables[0].rows[1][0] = Scalar::String("red beta".into());
    // `red` now occurs in two of the three documents, so it is less distinctive and scores lower.
    let common = score(&model);
    assert!(rare > 0.0, "rare score {rare}");
    assert!(common < rare, "edited corpus {common} should score below {rare}");
}
#[test]
fn text_similarity_does_not_alias_between_interleaved_evaluators() {
    // Two models scored alternately on one thread: the memo must never answer one corpus's
    // statistics for the other, in either order and across repeated evaluations.
    let dense = corpus_model(["red alpha", "red beta", "same tail"]);
    let sparse = corpus_model(["red alpha", "blue beta", "same tail"]);
    let score =
        |model: &DataModel| match evaluate_with_model(model, "MAXX(NoteLines, TEXTSIMILARITY(NoteLines[Text], \"red\"))", FilterContext::default()) {
            Ok(Scalar::Decimal(value)) => value,
            other => panic!("unexpected result: {other:?}"),
        };
    let dense_first = score(&dense);
    let sparse_first = score(&sparse);
    assert!(sparse_first > dense_first, "sparse {sparse_first} should outrank dense {dense_first}");
    for _ in 0..3 {
        assert_eq!(score(&dense), dense_first, "dense score changed between evaluations");
        assert_eq!(score(&sparse), sparse_first, "sparse score changed between evaluations");
    }
}

#[test]
fn text_similarity_error_leaves_no_stale_statistics() {
    // A call that fails on the work budget must not leave statistics behind for the next call.
    let mut oversized = DataModel {
        tables: vec![ModelTable {
            name: "Notes".into(),
            columns: vec![ModelColumn { name: "Text".into(), data_type: Some("String".into()) }],
            rows: (0..1500).map(|index| vec![Scalar::String(format!("note {index} {}", "x".repeat(1000)))]).collect(),
        }],
        relationships: vec![],
        measures: vec![],
    };
    oversized.parse_formulas().expect("fixture model parses");
    let error = evaluate_with_model(&oversized, "MAXX(Notes, TEXTSIMILARITY(Notes[Text], \"note\"))", FilterContext::default())
        .expect_err("a column beyond the work budget");
    assert!(error.contains("too large"), "unexpected error: {error}");
    // The normal corpus still scores exactly as it does on its own.
    let model = corpus_model(["red alpha", "blue beta", "same tail"]);
    let score = match evaluate_with_model(&model, "MAXX(NoteLines, TEXTSIMILARITY(NoteLines[Text], \"red\"))", FilterContext::default()) {
        Ok(Scalar::Decimal(value)) => value,
        other => panic!("unexpected result: {other:?}"),
    };
    let sparse = corpus_model(["red alpha", "blue beta", "same tail"]);
    let expected = match evaluate_with_model(&sparse, "MAXX(NoteLines, TEXTSIMILARITY(NoteLines[Text], \"red\"))", FilterContext::default()) {
        Ok(Scalar::Decimal(value)) => value,
        other => panic!("unexpected result: {other:?}"),
    };
    assert_eq!(score, expected);
}
#[test]
fn text_similarity_counts_numeric_text_exactly() {
    // 70,000 one-digit integers are 70,000 characters of text, not twenty each: a per-variant bound
    // that assumed twenty would reject this column although its text is tiny. The query is three
    // characters, so the old bound would have asked for 4.2 M characters.
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Counts".into(),
            columns: vec![ModelColumn { name: "Value".into(), data_type: Some("Int64".into()) }],
            rows: (0..70_000).map(|_| vec![Scalar::Integer(1)]).collect(),
        }],
        relationships: vec![],
        measures: vec![],
    };
    model.parse_formulas().expect("fixture model parses");
    let score = evaluate_with_model(&model, "MAXX(Counts, TEXTSIMILARITY(Counts[Value], \"one\"))", FilterContext::default());
    assert!(matches!(score, Ok(Scalar::Decimal(_))), "unexpected result: {score:?}");
}

#[test]
fn text_similarity_rejects_a_column_whose_text_exceeds_the_budget() {
    // A large decimal formats to hundreds of characters, so a column of them must be rejected even
    // though the value count alone looks harmless.
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Wide".into(),
            columns: vec![ModelColumn { name: "Value".into(), data_type: Some("Double".into()) }],
            rows: (0..20_000).map(|_| vec![Scalar::Decimal(1e308)]).collect(),
        }],
        relationships: vec![],
        measures: vec![],
    };
    model.parse_formulas().expect("fixture model parses");
    let error = evaluate_with_model(&model, "MAXX(Wide, TEXTSIMILARITY(Wide[Value], \"1\"))", FilterContext::default())
        .expect_err("a column whose text exceeds the budget");
    assert!(error.contains("too large"), "unexpected error: {error}");
}

#[test]
fn text_similarity_keeps_two_queries_apart_in_one_evaluation() {
    // Both queries run inside one evaluation, so they hit the same memo: each must get the
    // statistics for its own terms.
    let combined = number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"red\")) + MAXX(Product, TEXTSIMILARITY(Product[Name], \"widget\"))");
    let red = number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"red\"))");
    let widget = number("MAXX(Product, TEXTSIMILARITY(Product[Name], \"widget\"))");
    assert!(red > 0.0 && widget > 0.0);
    assert!((combined - (red + widget)).abs() < 1e-9, "combined {combined} should be {red} + {widget}");
}
#[test]
fn distinct_values_fold_text_case() {
    // `"East"` and `"east"` are one distinct value under the model collation, so `HASONEVALUE`
    // agrees with `VALUES`/`DISTINCTCOUNT` and `COLUMNSTATISTICS` reports the same cardinality.
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Regions".into(),
            columns: vec![ModelColumn { name: "Name".into(), data_type: Some("String".into()) }],
            rows: vec![vec![Scalar::String("East".into())], vec![Scalar::String("east".into())]],
        }],
        relationships: vec![],
        measures: vec![],
    };
    model.parse_formulas().expect("fixture model parses");
    let one_value = evaluate_with_model(&model, "HASONEVALUE(Regions[Name])", FilterContext::default()).expect("has one value");
    assert_eq!(one_value, Scalar::Boolean(true));
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("COLUMNSTATISTICS()").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    let table = evaluator.evaluate_table(&bound).expect("column statistics");
    let row = table
        .rows
        .iter()
        .find(|row| row[0] == Scalar::String("Regions".into()) && row[1] == Scalar::String("Name".into()))
        .expect("Regions[Name] row");
    assert_eq!(row[4], Scalar::Integer(1), "Cardinality must fold text case");
}
#[test]
fn isafter_keeps_value_pairs_that_look_like_order_flags() {
    // `0` and `1` are the documented `Order` shorthands, but here they are the second pair's
    // values: the first pair is equal, so the second one decides.
    assert!(!boolean("ISAFTER(1, 1, 0, 1)"), "0 < 1, so the pair is not after");
    assert!(boolean("ISAFTER(1, 1, 2, 1)"), "2 > 1, so the pair is after");
    // ISONORAFTER also includes equality, and the pair here is strictly before.
    assert!(!boolean("ISONORAFTER(1, 1, 0, 1)"));
    assert!(boolean("ISONORAFTER(1, 1, 1, 1)"));
    assert!(boolean("ISONORAFTER(1, 1, 2, 1)"));
}

#[test]
fn isafter_accepts_a_flag_after_every_pair() {
    // A flag after each pair must still split into pairs, not into a pair plus a dangling value.
    assert!(boolean("ISAFTER(1, 1, TRUE, 2, 3, FALSE)"));
    assert!(boolean("ISAFTER(1, 1, TRUE, 3, 2)"));
    assert!(!boolean("ISAFTER(1, 1, TRUE, 2, 3)"));
    assert!(boolean("ISONORAFTER(1, 1, TRUE, 2, 2)"));
}

#[test]
fn isafter_rejects_an_odd_argument_count() {
    assert!(eval("ISAFTER(1)").is_err());
    // `ISAFTER(1, 1, 0)` is a pair plus the `0` shorthand flag, so only a leftover value is an
    // error: here the `3` after the second pair is neither a flag nor paired.
    assert!(eval("ISONORAFTER(1, 1, 2, 2, 3)").is_err());
}

#[test]
fn isafter_evaluates_a_computed_flag_once() {
    // A computed `Order` flag must keep the direction it had when the argument list was split:
    // the flag is evaluated once and the cached value decides both the split and the sort.
    // `TRUE` is ascending, `FALSE` is descending (as in `ISAFTER(1, 2, FALSE)` above).
    assert!(!boolean("ISAFTER(1, 2, IF(1 > 0, TRUE, FALSE))"));
    assert!(boolean("ISAFTER(2, 1, IF(1 > 0, TRUE, FALSE))"));
    assert!(boolean("ISAFTER(1, 2, IF(1 > 0, FALSE, TRUE))"));
    assert!(!boolean("ISAFTER(2, 1, IF(1 > 0, FALSE, TRUE))"));
    assert!(boolean("ISONORAFTER(1, 1, IF(1 > 0, TRUE, FALSE))"));
    assert!(boolean("ISONORAFTER(1, 1, IF(1 > 0, FALSE, TRUE))"));
}

#[test]
fn contains_row_needs_one_value_per_column() {
    // `Sales` has four columns: a short value list is an error, not a leading-prefix match.
    assert!(eval("CONTAINSROW(Sales, \"East\")").is_err());
    assert!(eval("CONTAINSROW(Sales, \"East\", 10.0, 2, 1)").is_ok());
    assert!(!boolean("CONTAINSROW(Sales, \"East\", 20.0, 2, 1)"));
}
