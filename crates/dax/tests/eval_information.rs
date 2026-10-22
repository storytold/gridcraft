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
    assert!(boolean("CONTAINSROW(Sales, \"East\", 10.0)"));
    assert!(boolean("CONTAINSROW(Sales, \"East\")"));
    assert!(!boolean("CONTAINSROW(Sales, \"West\", 10.0)"));
    assert!(error_message("CONTAINSROW({1}, 1, 2)").contains("more values"));
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
    // USERNAME/USERPRINCIPALNAME report the host identity; on a test host without a user they
    // are empty, otherwise they mirror the operating system's user and domain.
    let host_user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    let principal = match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() && !host_user.is_empty() => format!("{domain}\\{host_user}"),
        _ => host_user,
    };
    assert_eq!(eval("USERNAME()"), Ok(Scalar::String(principal.clone())));
    assert_eq!(eval("USERPRINCIPALNAME()"), Ok(Scalar::String(principal)));
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

#[test]
fn scope_and_subtotal_predicates() {
    assert!(!boolean("ISINSCOPE(Product[Color])"));
    assert_eq!(number("SUMX(Product, IF(ISINSCOPE(Product[Color]), 1, 0))"), 4.0);
    // A frame over a different table does not put Product columns in scope.
    assert_eq!(number("SUMX(Sales, IF(ISINSCOPE(Product[Color]), 1, 0))"), 0.0);

    assert!(error_message("ISSUBTOTAL(Product[Color])").contains("SUMMARIZE"));
    assert!(error_message("SUMX(Product, ISSUBTOTAL(Product[Color]))").contains("SUMMARIZE"));
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
