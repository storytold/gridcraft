//! Time-intelligence range functions over an in-memory model.
//!
//! The fixture date table runs from 2022-01-01 to 2025-12-31 so that previous/next year windows
//! always have a full calendar to draw from. `Date` is table 0 and `Sales` is table 1; the
//! `Sales[Date]` -> `Date[Date]` relationship is the only cross-filtering path.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, BoundExpr, Evaluator, FilterContext, Literal, ScalarKey, parse_expression};

const DATE_TABLE: usize = 0;
const DATE_COLUMN: usize = 0;
const YEAR_COLUMN: usize = 1;
const SALES_TABLE: usize = 1;
const AMOUNT_COLUMN: usize = 1;

/// Excel/OLE date serial, computed with `chrono` independently of the evaluator.
fn serial(year: i64, month: i64, day: i64) -> f64 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).expect("the Excel epoch is a valid date");
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32).expect("the fixture date is valid");
    (date - epoch).num_days() as f64
}

fn date_rows() -> Vec<Vec<Scalar>> {
    use chrono::Datelike;
    let start = chrono::NaiveDate::from_ymd_opt(2022, 1, 1).expect("the fixture start is a valid date");
    let end = chrono::NaiveDate::from_ymd_opt(2025, 12, 31).expect("the fixture end is a valid date");
    start
        .iter_days()
        .take_while(|date| *date <= end)
        .map(|date| {
            let (year, month) = (i64::from(date.year()), i64::from(date.month()));
            vec![
                Scalar::DateTime(serial(year, month, i64::from(date.day()))),
                Scalar::Integer(year),
                Scalar::Integer((month - 1) / 3 + 1),
                Scalar::Integer(month),
            ]
        })
        .collect()
}

/// (year, month, day, amount) for every sales row.
const SALES: &[(i64, i64, i64, f64)] = &[
    (2023, 1, 10, 10.0),
    (2023, 3, 5, 20.0),
    (2023, 6, 15, 30.0),
    (2023, 9, 20, 40.0),
    (2023, 12, 31, 50.0),
    (2024, 1, 5, 100.0),
    (2024, 1, 31, 200.0),
    (2024, 2, 29, 300.0),
    (2024, 3, 15, 400.0),
    (2024, 4, 1, 500.0),
    (2024, 6, 30, 600.0),
    (2024, 9, 30, 700.0),
    (2024, 11, 11, 800.0),
    (2024, 12, 25, 900.0),
    (2024, 12, 31, 1000.0),
];

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Date".into(),
                columns: vec![
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                    ModelColumn { name: "Year".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Quarter".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Month".into(), data_type: Some("Int64".into()) },
                ],
                rows: date_rows(),
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                ],
                rows: SALES
                    .iter()
                    .map(|(year, month, day, amount)| vec![Scalar::DateTime(serial(*year, *month, *day)), Scalar::Decimal(*amount)])
                    .collect(),
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "Date".into(),
            to_table: "Date".into(),
            to_column: "Date".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![Measure {
            name: "Sales Amount".into(),
            formula: "SUM(Sales[Amount])".into(),
            table: Some("Sales".into()),
            format: None,
            expr: None,
        }],
    };
    model.parse_formulas().expect("model parses");
    model
}

/// A three-day date table whose last row's date cell is BLANK: the axis must drop that row instead
/// of reading BLANK as serial 0 (1899-12-30).
fn model_with_blank_date() -> DataModel {
    DataModel {
        tables: vec![ModelTable {
            name: "Day".into(),
            columns: vec![ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) }],
            rows: vec![
                vec![Scalar::DateTime(serial(2024, 1, 1))],
                vec![Scalar::DateTime(serial(2024, 1, 2))],
                vec![Scalar::DateTime(serial(2024, 1, 3))],
                vec![Scalar::Blank],
            ],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    }
}

fn evaluate(source: &str, filters: FilterContext) -> Result<Scalar, String> {
    evaluate_in(&model(), source, filters)
}

fn evaluate_in(model: &DataModel, source: &str, filters: FilterContext) -> Result<Scalar, String> {
    let evaluator = Evaluator::new(model).map_err(|error| error.to_string())?;
    let expr = parse_expression(source).map_err(|error| error.to_string())?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_with(&bound, filters).map_err(|error| error.to_string())
}

fn eval_with(source: &str, filters: FilterContext) -> Scalar {
    evaluate(source, filters).expect("expression evaluates")
}

fn eval_error(source: &str, filters: FilterContext) -> String {
    evaluate(source, filters).expect_err("expression must fail")
}

fn evaluate_table(source: &str) -> Result<TableValue, String> {
    evaluate_table_in(&model(), source)
}

fn evaluate_table_in(model: &DataModel, source: &str) -> Result<TableValue, String> {
    let evaluator = Evaluator::new(model).map_err(|error| error.to_string())?;
    let expr = parse_expression(source).map_err(|error| error.to_string())?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.to_string())
}

fn table(source: &str) -> TableValue {
    evaluate_table(source).expect("table evaluates")
}

/// Filter context selecting the date column between two inclusive calendar days.
fn dates(from: (i64, i64, i64), to: (i64, i64, i64)) -> FilterContext {
    let mut filters = FilterContext::default();
    let mut keys = Vec::new();
    let start = serial(from.0, from.1, from.2) as i64;
    let end = serial(to.0, to.1, to.2) as i64;
    for day in start..=end {
        keys.push(ScalarKey::from(&Scalar::DateTime(day as f64)));
    }
    filters.set(DATE_TABLE, DATE_COLUMN, keys);
    filters
}

fn year(year: i64) -> FilterContext {
    let mut filters = FilterContext::default();
    filters.set(DATE_TABLE, YEAR_COLUMN, [ScalarKey::from(&Scalar::Integer(year))]);
    filters
}

/// Number of rows of a table expression evaluated under a filter context.
fn row_count(source: &str, filters: FilterContext) -> i64 {
    let count = eval_with(&format!("COUNTROWS({source})"), filters);
    match count {
        Scalar::Blank => 0,
        other => other.to_number().expect("COUNTROWS is numeric") as i64,
    }
}

/// (first, last) date of a table expression evaluated under a filter context.
fn bounds(source: &str, filters: FilterContext) -> (f64, f64) {
    let first = eval_with(&format!("MINX({source}, 'Date'[Date])"), filters.clone());
    let last = eval_with(&format!("MAXX({source}, 'Date'[Date])"), filters);
    (first.to_number().expect("first date"), last.to_number().expect("last date"))
}

#[test]
fn dateadd_shifts_the_visible_period() {
    let march = dates((2024, 3, 1), (2024, 3, 31));
    assert_eq!(row_count("DATEADD('Date'[Date], -1, \"MONTH\")", march.clone()), 29);
    assert_eq!(bounds("DATEADD('Date'[Date], -1, \"MONTH\")", march.clone()), (serial(2024, 2, 1), serial(2024, 2, 29)));
    // A partial period keeps its day span.
    assert_eq!(bounds("DATEADD('Date'[Date], 10, \"DAY\")", dates((2024, 3, 25), (2024, 3, 31))), (serial(2024, 4, 4), serial(2024, 4, 10)));
    // A selection ending on the last day of its period extends to the whole target period.
    assert_eq!(row_count("DATEADD('Date'[Date], -1, \"QUARTER\")", dates((2024, 4, 1), (2024, 6, 30))), 91);
    assert_eq!(bounds("DATEADD('Date'[Date], 1, \"MONTH\")", dates((2024, 2, 25), (2024, 2, 29))), (serial(2024, 3, 25), serial(2024, 3, 31)));
    assert_eq!(row_count("DATEADD('Date'[Date], -1, \"YEAR\")", dates((2024, 3, 1), (2024, 3, 31))), 31);

    let error = eval_error("DATEADD('Date'[Date], 1, \"DECADE\")", FilterContext::default());
    assert!(error.contains("interval"), "{error}");
    let error = eval_error("DATEADD(Sales[Amount], 1, \"DAY\")", FilterContext::default());
    assert!(error.contains("DateTime"), "{error}");
}

#[test]
fn blank_date_cells_are_not_axis_dates() {
    let model = model_with_blank_date();
    // BLANK is not serial 0, so it must not become the first visible date (1899-12-30) or an axis
    // row that the range functions can select.
    assert_eq!(evaluate_in(&model, "FIRSTDATE('Day'[Date])", FilterContext::default()).expect("first date"), Scalar::DateTime(serial(2024, 1, 1)));
    assert_eq!(evaluate_in(&model, "LASTDATE('Day'[Date])", FilterContext::default()).expect("last date"), Scalar::DateTime(serial(2024, 1, 3)));
    assert_eq!(evaluate_table_in(&model, "DATESBETWEEN('Day'[Date], DATE(1899,12,30), DATE(1899,12,30))").expect("epoch day").rows.len(), 0);
    assert_eq!(evaluate_table_in(&model, "DATESBETWEEN('Day'[Date], DATE(2024,1,1), DATE(2024,1,3))").expect("window").rows.len(), 3);
}

#[test]
fn datesbetween_is_inclusive_and_accepts_blanks() {
    let between = table("DATESBETWEEN('Date'[Date], DATE(2024, 1, 15), DATE(2024, 2, 10))");
    assert_eq!(between.rows.len(), 27);
    assert_eq!(between.columns.len(), 1);
    assert_eq!(between.columns[0].name, "Date");
    assert_eq!(between.lineage, Some(DATE_TABLE));
    assert_eq!(between.rows[0][0], Scalar::DateTime(serial(2024, 1, 15)));
    assert_eq!(between.rows[26][0], Scalar::DateTime(serial(2024, 2, 10)));

    // BLANK boundaries fall back to the visible range.
    assert_eq!(row_count("DATESBETWEEN('Date'[Date], BLANK(), DATE(2024, 1, 5))", FilterContext::default()), 735);
    assert_eq!(row_count("DATESBETWEEN('Date'[Date], DATE(2024, 1, 1), BLANK())", FilterContext::default()), 731);

    assert_eq!(table("DATESBETWEEN('Date'[Date], DATE(2024, 2, 1), DATE(2024, 1, 1))").rows.len(), 0);
}

#[test]
fn datesinperiod_moves_backwards_from_the_start_date() {
    let backwards = table("DATESINPERIOD('Date'[Date], DATE(2024, 3, 15), -1, \"MONTH\")");
    assert_eq!(backwards.rows.len(), 29);
    assert_eq!(backwards.rows[0][0], Scalar::DateTime(serial(2024, 2, 16)));
    assert_eq!(backwards.rows[28][0], Scalar::DateTime(serial(2024, 3, 15)));

    let forwards = table("DATESINPERIOD('Date'[Date], DATE(2024, 3, 15), 3, \"DAY\")");
    assert_eq!(forwards.rows.len(), 3);
    assert_eq!(forwards.rows[2][0], Scalar::DateTime(serial(2024, 3, 17)));
    assert_eq!(table("DATESINPERIOD('Date'[Date], DATE(2024, 3, 15), 0, \"DAY\")").rows.len(), 0);
    assert_eq!(
        bounds("DATESINPERIOD('Date'[Date], DATE(2024, 3, 15), -2, \"WEEK\")", FilterContext::default()),
        (serial(2024, 3, 2), serial(2024, 3, 15))
    );
}

#[test]
fn to_date_windows() {
    assert_eq!(bounds("DATESMTD('Date'[Date])", dates((2024, 3, 1), (2024, 3, 20))), (serial(2024, 3, 1), serial(2024, 3, 20)));
    assert_eq!(row_count("DATESMTD('Date'[Date])", dates((2024, 3, 1), (2024, 3, 20))), 20);
    assert_eq!(row_count("DATESQTD('Date'[Date])", dates((2024, 5, 1), (2024, 5, 10))), 40);
    assert_eq!(row_count("DATESWTD('Date')", dates((2024, 3, 13), (2024, 3, 15))), 6);
    assert_eq!(row_count("DATESYTD('Date'[Date])", dates((2024, 3, 15), (2024, 3, 15))), 75);
    // A fiscal year ending June 30 starts on July 1.
    assert_eq!(row_count("DATESYTD('Date'[Date], \"6/30/2024\")", dates((2024, 8, 15), (2024, 8, 15))), 46);
}

#[test]
fn period_boundaries() {
    assert_eq!(eval_with("FIRSTDATE('Date'[Date])", dates((2024, 3, 5), (2024, 3, 20))), Scalar::DateTime(serial(2024, 3, 5)));
    assert_eq!(eval_with("LASTDATE('Date'[Date])", dates((2024, 3, 5), (2024, 3, 20))), Scalar::DateTime(serial(2024, 3, 20)));
    let first = table("FIRSTDATE('Date'[Date])");
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.lineage, Some(DATE_TABLE));

    assert_eq!(eval_with("STARTOFMONTH('Date'[Date])", dates((2024, 3, 20), (2024, 3, 20))), Scalar::DateTime(serial(2024, 3, 1)));
    assert_eq!(eval_with("ENDOFMONTH('Date'[Date])", dates((2024, 2, 1), (2024, 2, 10))), Scalar::DateTime(serial(2024, 2, 29)));
    assert_eq!(eval_with("STARTOFQUARTER('Date'[Date])", dates((2024, 5, 20), (2024, 5, 20))), Scalar::DateTime(serial(2024, 4, 1)));
    assert_eq!(eval_with("ENDOFQUARTER('Date'[Date])", dates((2024, 5, 20), (2024, 5, 20))), Scalar::DateTime(serial(2024, 6, 30)));
    assert_eq!(eval_with("STARTOFWEEK('Date')", dates((2024, 3, 13), (2024, 3, 13))), Scalar::DateTime(serial(2024, 3, 10)));
    assert_eq!(eval_with("ENDOFWEEK('Date')", dates((2024, 3, 13), (2024, 3, 13))), Scalar::DateTime(serial(2024, 3, 16)));
    assert_eq!(eval_with("STARTOFYEAR('Date'[Date])", dates((2024, 3, 20), (2024, 3, 20))), Scalar::DateTime(serial(2024, 1, 1)));
    assert_eq!(eval_with("ENDOFYEAR('Date'[Date])", dates((2024, 3, 20), (2024, 3, 20))), Scalar::DateTime(serial(2024, 12, 31)));
    assert_eq!(eval_with("STARTOFYEAR('Date'[Date], \"6/30/2024\")", dates((2024, 3, 20), (2024, 3, 20))), Scalar::DateTime(serial(2023, 7, 1)));
    assert_eq!(eval_with("ENDOFYEAR('Date'[Date], \"6/30/2024\")", dates((2024, 8, 15), (2024, 8, 15))), Scalar::DateTime(serial(2025, 6, 30)));
}

#[test]
fn adjacent_periods() {
    assert_eq!(eval_with("NEXTDAY('Date'[Date])", dates((2024, 3, 15), (2024, 3, 15))), Scalar::DateTime(serial(2024, 3, 16)));
    assert_eq!(eval_with("PREVIOUSDAY('Date'[Date])", dates((2024, 3, 15), (2024, 3, 15))), Scalar::DateTime(serial(2024, 3, 14)));

    assert_eq!(bounds("NEXTMONTH('Date'[Date])", dates((2024, 3, 1), (2024, 3, 10))), (serial(2024, 4, 1), serial(2024, 4, 30)));
    assert_eq!(bounds("PREVIOUSMONTH('Date'[Date])", dates((2024, 3, 1), (2024, 3, 10))), (serial(2024, 2, 1), serial(2024, 2, 29)));
    assert_eq!(row_count("NEXTQUARTER('Date'[Date])", dates((2024, 2, 1), (2024, 2, 28))), 91);
    assert_eq!(row_count("PREVIOUSQUARTER('Date'[Date])", dates((2024, 5, 1), (2024, 5, 31))), 91);
    assert_eq!(bounds("NEXTWEEK('Date')", dates((2024, 3, 13), (2024, 3, 13))), (serial(2024, 3, 17), serial(2024, 3, 23)));
    assert_eq!(bounds("PREVIOUSWEEK('Date')", dates((2024, 3, 13), (2024, 3, 13))), (serial(2024, 3, 3), serial(2024, 3, 9)));
    assert_eq!(bounds("NEXTYEAR('Date'[Date])", dates((2024, 3, 1), (2024, 3, 31))), (serial(2025, 1, 1), serial(2025, 12, 31)));
    assert_eq!(row_count("NEXTYEAR('Date'[Date])", dates((2024, 3, 1), (2024, 3, 31))), 365);
    assert_eq!(bounds("PREVIOUSYEAR('Date'[Date])", dates((2024, 1, 10), (2024, 1, 20))), (serial(2023, 1, 1), serial(2023, 12, 31)));

    // A partial period is expanded to the full parallel period.
    assert_eq!(row_count("PARALLELPERIOD('Date'[Date], -1, \"MONTH\")", dates((2024, 3, 15), (2024, 3, 20))), 29);
    assert_eq!(row_count("PARALLELPERIOD('Date'[Date], 0, \"YEAR\")", dates((2024, 3, 15), (2024, 3, 20))), 366);
    assert_eq!(row_count("SAMEPERIODLASTYEAR('Date'[Date])", dates((2024, 3, 1), (2024, 3, 31))), 31);
    assert_eq!(bounds("SAMEPERIODLASTYEAR('Date'[Date])", dates((2024, 3, 1), (2024, 3, 31))), (serial(2023, 3, 1), serial(2023, 3, 31)));
}

#[test]
fn nonblank_scan_uses_the_value_expression() {
    assert_eq!(eval_with("FIRSTNONBLANK('Date'[Date], [Sales Amount])", year(2023)), Scalar::DateTime(serial(2023, 1, 10)));
    assert_eq!(eval_with("LASTNONBLANK('Date'[Date], [Sales Amount])", year(2023)), Scalar::DateTime(serial(2023, 12, 31)));
    assert_eq!(row_count("FIRSTNONBLANK('Date'[Date], [Sales Amount])", year(2023)), 1);
    // An aggregate expression works too: it is evaluated in the date row's context.
    assert_eq!(
        eval_with("LASTNONBLANK('Date'[Date], SUM(Sales[Amount]))", dates((2024, 1, 1), (2024, 6, 30))),
        Scalar::DateTime(serial(2024, 6, 30))
    );
    // No sales at all in 2025: nothing is non-blank.
    assert_eq!(eval_with("FIRSTNONBLANK('Date'[Date], [Sales Amount])", year(2025)), Scalar::Blank);
    assert_eq!(row_count("LASTNONBLANK('Date'[Date], [Sales Amount])", year(2025)), 0);
}

#[test]
fn range_tables_drive_calculate() {
    let march = dates((2024, 3, 1), (2024, 3, 31));
    assert_eq!(eval_with("CALCULATE(SUM(Sales[Amount]), DATESMTD('Date'[Date]))", march.clone()), Scalar::Decimal(400.0));
    assert_eq!(eval_with("CALCULATE([Sales Amount], SAMEPERIODLASTYEAR('Date'[Date]))", march), Scalar::Decimal(20.0));
    assert_eq!(eval_with("CALCULATE([Sales Amount], PREVIOUSYEAR('Date'[Date]))", dates((2024, 1, 10), (2024, 1, 20))), Scalar::Decimal(150.0));
    assert_eq!(eval_with("CALCULATE(SUM(Sales[Amount]), ENDOFMONTH('Date'[Date]))", dates((2024, 2, 1), (2024, 2, 10))), Scalar::Decimal(300.0));
    assert_eq!(eval_with("CALCULATE(COUNTROWS('Date'), FIRSTDATE('Date'[Date]))", dates((2024, 3, 1), (2024, 3, 31))), Scalar::Integer(1));
    // Rolling twelve months ending December 31, 2024.
    assert_eq!(
        eval_with("CALCULATE(SUM(Sales[Amount]), DATESINPERIOD('Date'[Date], DATE(2024, 12, 31), -12, \"MONTH\"))", FilterContext::default()),
        Scalar::Decimal(5500.0)
    );
    assert_eq!(
        eval_with("CALCULATE(SUM(Sales[Amount]), DATESBETWEEN('Date'[Date], DATE(2024, 1, 1), DATE(2024, 3, 31)))", FilterContext::default()),
        Scalar::Decimal(1000.0)
    );
}

#[test]
fn multi_row_results_are_not_scalars() {
    let error = eval_error("DATESMTD('Date'[Date])", dates((2024, 3, 1), (2024, 3, 31)));
    assert!(error.contains("rows"), "{error}");
    let error = eval_error("NEXTMONTH('Date'[Date])", dates((2024, 3, 1), (2024, 3, 31)));
    assert!(error.contains("rows"), "{error}");
}

#[test]
fn single_date_results_are_scalars() {
    let filters = dates((2024, 3, 15), (2024, 3, 15));
    assert_eq!(eval_with("STARTOFMONTH('Date'[Date])", filters.clone()), Scalar::DateTime(serial(2024, 3, 1)));
    assert_eq!(eval_with("ENDOFMONTH('Date'[Date])", filters.clone()), Scalar::DateTime(serial(2024, 3, 31)));
    assert_eq!(eval_with("NEXTDAY('Date'[Date])", filters), Scalar::DateTime(serial(2024, 3, 16)));
}

#[test]
fn empty_filter_context_yields_empty_windows() {
    // An empty date-column filter leaves nothing visible.
    let mut filters = FilterContext::default();
    filters.set(DATE_TABLE, DATE_COLUMN, Vec::<ScalarKey>::new());
    assert_eq!(row_count("DATESMTD('Date'[Date])", filters.clone()), 0);
    assert_eq!(eval_with("FIRSTDATE('Date'[Date])", filters.clone()), Scalar::Blank);
    assert_eq!(eval_with("LASTDATE('Date'[Date])", filters), Scalar::Blank);
}

#[test]
fn fact_filters_do_not_narrow_the_date_axis() {
    let mut filters = FilterContext::default();
    filters.set(SALES_TABLE, AMOUNT_COLUMN, [ScalarKey::from(&Scalar::Decimal(400.0))]);
    // Only one sale matches, but the date table itself is unfiltered.
    assert_eq!(eval_with("FIRSTDATE('Date'[Date])", filters.clone()), Scalar::DateTime(serial(2022, 1, 1)));
    assert_eq!(row_count("DATESMTD('Date'[Date])", filters), 31);
}

#[test]
fn interval_keywords_reach_the_evaluator_as_keywords() {
    // The binder turns a bare interval keyword (DATEADD(..., MONTH)) into `BoundExpr::Keyword`;
    // build that shape directly so the evaluator path is covered independently of the binder.
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let dateadd = BoundExpr::Call {
        spec: gridcraft_dax::lookup("DATEADD").expect("DATEADD is catalogued"),
        args: vec![
            BoundExpr::Column { table: DATE_TABLE, column: DATE_COLUMN },
            BoundExpr::Literal(Literal::Integer(-1)),
            BoundExpr::Keyword("MONTH".into()),
        ],
    };
    let countrows = BoundExpr::Call { spec: gridcraft_dax::lookup("COUNTROWS").expect("COUNTROWS is catalogued"), args: vec![dateadd] };
    let count = evaluator.evaluate_with(&countrows, dates((2024, 3, 1), (2024, 3, 31))).expect("count");
    assert_eq!(count, Scalar::Integer(29));
}

#[test]
fn week_functions_accept_a_table_reference() {
    let filters = dates((2024, 3, 13), (2024, 3, 13));
    assert_eq!(row_count("DATESWTD('Date')", filters.clone()), 4);
    assert_eq!(eval_with("STARTOFWEEK('Date')", filters), Scalar::DateTime(serial(2024, 3, 10)));
    let error = eval_error("DATESWTD('Date'[Date])", FilterContext::default());
    assert!(error.contains("table"), "{error}");
}

#[test]
fn dateadd_and_sameperiodlastyear_keep_the_leap_day_of_a_full_month() {
    // A full February shifted back a year reaches 29 February 2024, not 28.
    let february = dates((2025, 2, 1), (2025, 2, 28));
    assert_eq!(bounds("DATEADD('Date'[Date], -1, \"YEAR\")", february.clone()), (serial(2024, 2, 1), serial(2024, 2, 29)));
    assert_eq!(bounds("SAMEPERIODLASTYEAR('Date'[Date])", february), (serial(2024, 2, 1), serial(2024, 2, 29)));
    // A selection ending mid-month keeps its day span.
    assert_eq!(bounds("DATEADD('Date'[Date], -1, \"YEAR\")", dates((2025, 2, 1), (2025, 2, 15))), (serial(2024, 2, 1), serial(2024, 2, 15)));
}

#[test]
fn dateadd_does_not_extend_a_selection_ending_on_the_penultimate_day() {
    // 29 April is its month's penultimate day, but the selection does not contain 30 April, so
    // the shifted window keeps its day span instead of reaching 31 March.
    let selection = dates((2024, 4, 1), (2024, 4, 29));
    assert_eq!(bounds("DATEADD('Date'[Date], -1, \"MONTH\")", selection.clone()), (serial(2024, 3, 1), serial(2024, 3, 29)));
    assert_eq!(row_count("DATEADD('Date'[Date], -1, \"MONTH\")", selection), 29);
}

#[test]
fn dateadd_does_not_extend_a_selection_ending_on_day_30_of_a_31_day_month() {
    // 30 July is one day before its month end, but 31 July is not visible, so the shifted window
    // stops on 30 August instead of extending to 31 August.
    let selection = dates((2024, 7, 1), (2024, 7, 30));
    assert_eq!(bounds("DATEADD('Date'[Date], 1, \"MONTH\")", selection.clone()), (serial(2024, 8, 1), serial(2024, 8, 30)));
    assert_eq!(row_count("DATEADD('Date'[Date], 1, \"MONTH\")", selection), 30);
}

#[test]
fn dateadd_does_not_expand_a_single_month_end_date_to_the_target_period() {
    // Only 30 April 2024 is visible: the date shifts to 30 May and stays a single date instead of
    // expanding to the end of May (or the whole target period).
    let selection = dates((2024, 4, 30), (2024, 4, 30));
    assert_eq!(bounds("DATEADD('Date'[Date], 1, \"MONTH\")", selection.clone()), (serial(2024, 5, 30), serial(2024, 5, 30)));
    assert_eq!(row_count("DATEADD('Date'[Date], 1, \"MONTH\")", selection), 1);
}

#[test]
fn same_day_timestamps_stay_in_the_date_axis() {
    // Two timestamps sharing a day are distinct cells of the column, and the range tables key
    // their filter on those exact values, so both rows have to survive the axis.
    let model = DataModel {
        tables: vec![ModelTable {
            name: "Log".into(),
            columns: vec![ModelColumn { name: "Stamp".into(), data_type: Some("DateTime".into()) }],
            rows: vec![
                vec![Scalar::DateTime(serial(2024, 1, 1))],
                vec![Scalar::DateTime(serial(2024, 1, 2) + 9.0 / 24.0)],
                vec![Scalar::DateTime(serial(2024, 1, 2) + 14.0 / 24.0)],
            ],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    let table = evaluate_table_in(&model, "DATESBETWEEN('Log'[Stamp], DATE(2024,1,2), DATE(2024,1,2))").expect("window");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][0], Scalar::DateTime(serial(2024, 1, 2) + 9.0 / 24.0));
    assert_eq!(table.rows[1][0], Scalar::DateTime(serial(2024, 1, 2) + 14.0 / 24.0));
    // The other days are untouched by the value-level deduplication.
    let whole = evaluate_table_in(&model, "DATESBETWEEN('Log'[Stamp], DATE(2024,1,1), DATE(2024,1,2))").expect("whole window");
    assert_eq!(whole.rows.len(), 3);
}

#[test]
fn single_date_functions_pick_one_cell_of_a_day() {
    // A day can hold several timestamps; `FIRSTDATE`/`LASTDATE` are scalar conveniences and must
    // return one exact cell rather than every cell of the day.
    let model = DataModel {
        tables: vec![ModelTable {
            name: "Log".into(),
            columns: vec![ModelColumn { name: "Stamp".into(), data_type: Some("DateTime".into()) }],
            rows: vec![
                vec![Scalar::DateTime(serial(2024, 1, 2) + 9.0 / 24.0)],
                vec![Scalar::DateTime(serial(2024, 1, 2) + 14.0 / 24.0)],
                vec![Scalar::DateTime(serial(2024, 1, 3) + 10.0 / 24.0)],
            ],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    assert_eq!(
        evaluate_in(&model, "LASTDATE('Log'[Stamp])", FilterContext::default()).expect("last date"),
        Scalar::DateTime(serial(2024, 1, 3) + 10.0 / 24.0)
    );

    // Filtered to the first day, which holds two timestamps: `FIRSTDATE` still returns one value.
    let mut filters = FilterContext::default();
    filters.set(
        0,
        0,
        [Scalar::DateTime(serial(2024, 1, 2) + 9.0 / 24.0), Scalar::DateTime(serial(2024, 1, 2) + 14.0 / 24.0)]
            .into_iter()
            .map(|value| ScalarKey::from(&value)),
    );
    assert_eq!(evaluate_in(&model, "FIRSTDATE('Log'[Stamp])", filters).expect("first date"), Scalar::DateTime(serial(2024, 1, 2) + 9.0 / 24.0));
}

#[test]
fn single_date_functions_stay_inside_the_visible_cells() {
    // A filter that selects one timestamp of a two-timestamp day: `FIRSTDATE`/`LASTDATE` must pick
    // from the selected cell, not from the other timestamp the same day holds.
    let model = DataModel {
        tables: vec![ModelTable {
            name: "Log".into(),
            columns: vec![ModelColumn { name: "Stamp".into(), data_type: Some("DateTime".into()) }],
            rows: vec![vec![Scalar::DateTime(serial(2024, 1, 2) + 9.0 / 24.0)], vec![Scalar::DateTime(serial(2024, 1, 2) + 14.0 / 24.0)]],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    let early = Scalar::DateTime(serial(2024, 1, 2) + 9.0 / 24.0);
    let late = Scalar::DateTime(serial(2024, 1, 2) + 14.0 / 24.0);
    let mut filters = FilterContext::default();
    filters.set(0, 0, [ScalarKey::from(&late)]);

    // 14:00 is the only visible cell: the last date is it, even though 09:00 exists in the column.
    assert_eq!(evaluate_in(&model, "LASTDATE('Log'[Stamp])", filters.clone()).expect("last date"), late);
    assert_eq!(evaluate_in(&model, "FIRSTDATE('Log'[Stamp])", filters.clone()).expect("first date"), late);

    let mut early_filters = FilterContext::default();
    early_filters.set(0, 0, [ScalarKey::from(&early)]);
    assert_eq!(evaluate_in(&model, "LASTDATE('Log'[Stamp])", early_filters.clone()).expect("last date"), early);
    assert_eq!(evaluate_in(&model, "FIRSTDATE('Log'[Stamp])", early_filters).expect("first date"), early);
}

#[test]
fn nonblank_candidates_match_mixed_case_text_rows() {
    // The candidate filter is written through `FilterContext::set`, which folds text the same way
    // the filter's own comparison does: a candidate `"A"` has to select the `"A"` and `"a"` rows.
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "T".into(),
            columns: vec![ModelColumn { name: "Text".into(), data_type: Some("String".into()) }],
            rows: vec![vec![Scalar::String("A".into())], vec![Scalar::String("a".into())], vec![Scalar::String("B".into())]],
        }],
        relationships: Vec::new(),
        measures: vec![gridcraft_dax::model::Measure {
            name: "Rows".into(),
            formula: "COUNTROWS('T')".into(),
            table: Some("T".into()),
            format: None,
            expr: None,
        }],
    };
    model.parse_formulas().expect("model formulas parse");
    // Only the folded candidate filter leaves rows for the expression; an unfolded key would match
    // nothing and make every candidate blank.
    assert_eq!(
        evaluate_in(&model, "FIRSTNONBLANK('T'[Text], IF([Rows] = 0, BLANK(), 1))", FilterContext::default()).expect("first non blank"),
        Scalar::String("A".into())
    );
}

#[test]
fn period_ends_pick_the_latest_timestamp_of_a_day() {
    // A day may hold several timestamps; the axis orders them by their full serial, so `ENDOFMONTH`
    // returns the latest one even when the model stores it before or after its siblings.
    let model = DataModel {
        tables: vec![ModelTable {
            name: "Log".into(),
            columns: vec![ModelColumn { name: "Stamp".into(), data_type: Some("DateTime".into()) }],
            rows: vec![vec![Scalar::DateTime(serial(2024, 3, 31) + 14.0 / 24.0)], vec![Scalar::DateTime(serial(2024, 3, 31) + 9.0 / 24.0)]],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    let mut filters = FilterContext::default();
    filters.set(
        0,
        0,
        [Scalar::DateTime(serial(2024, 3, 31) + 14.0 / 24.0), Scalar::DateTime(serial(2024, 3, 31) + 9.0 / 24.0)]
            .into_iter()
            .map(|value| ScalarKey::from(&value)),
    );
    assert_eq!(
        evaluate_in(&model, "ENDOFMONTH('Log'[Stamp])", filters.clone()).expect("end of month"),
        Scalar::DateTime(serial(2024, 3, 31) + 14.0 / 24.0)
    );
    assert_eq!(
        evaluate_in(&model, "STARTOFMONTH('Log'[Stamp])", filters).expect("start of month"),
        Scalar::DateTime(serial(2024, 3, 31) + 9.0 / 24.0)
    );
}

#[test]
fn an_impossible_year_end_date_is_rejected_by_the_range_functions() {
    // The same validation the total functions use: `"2/30"` is not a date, while the documented
    // `"6/30/2024"` and `"6/30"` forms are.
    let model = model();
    let count = |argument: &str| evaluate_in(&model, &format!("COUNTROWS(DATESYTD('Date'[Date], \"{argument}\"))"), FilterContext::default());
    assert!(count("2/30").is_err());
    // An empty component is not a date either.
    assert!(count("6/-30").is_err());
    assert!(count("6/").is_err());
    assert!(count("6/30").is_ok());
    assert!(count("6/30/2024").is_ok());
    assert!(count("2/29").is_ok());
}
