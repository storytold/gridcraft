//! Integration tests for the time-intelligence totals and balance evaluator functions.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, ScalarKey};
use gridcraft_dax::{BoundExpr, Evaluator, FilterContext, Literal, lookup};

/// The calendar date of a whole-day Excel/OLE serial, via `chrono`.
fn date_of(value: f64) -> chrono::NaiveDate {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).expect("the Excel epoch is a valid date");
    epoch + chrono::Duration::days(value.floor() as i64)
}

/// Excel/OLE date serial for a whole day, computed with `chrono`.
fn serial(year: i64, month: i64, day: i64) -> f64 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).expect("the Excel epoch is a valid date");
    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32).expect("the fixture date is valid");
    (date - epoch).num_days() as f64
}

fn date(year: i64, month: i64, day: i64) -> Scalar {
    Scalar::DateTime(serial(year, month, day))
}

/// The fixture shared with the worksheet/measure cases: Sales facts for 2023-2024 and a
/// contiguous Date table covering both years.
fn model() -> DataModel {
    use chrono::Datelike;
    let start = date_of(serial(2023, 1, 1));
    let end = date_of(serial(2024, 12, 31));
    let date_rows: Vec<Vec<Scalar>> = start
        .iter_days()
        .take_while(|date| *date <= end)
        .map(|date| {
            // Excel's weekday numbering is 1 = Sunday, matching the evaluator's WEEKDAY default.
            let (year, month) = (i64::from(date.year()), i64::from(date.month()));
            vec![
                Scalar::DateTime(serial(year, month, i64::from(date.day()))),
                Scalar::Integer(year),
                Scalar::Integer((month - 1) / 3 + 1),
                Scalar::Integer(month),
                Scalar::Integer(i64::from(date.weekday().num_days_from_sunday()) + 1),
            ]
        })
        .collect();

    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Integer(40), date(2024, 1, 3)],
                    vec![Scalar::String("East".into()), Scalar::Integer(100), date(2024, 1, 5)],
                    vec![Scalar::String("West".into()), Scalar::Integer(250), date(2024, 1, 12)],
                    vec![Scalar::String("North".into()), Scalar::Integer(75), date(2024, 2, 3)],
                    vec![Scalar::String("South".into()), Scalar::Integer(400), date(2024, 2, 20)],
                    vec![Scalar::String("East".into()), Scalar::Blank, date(2024, 3, 15)],
                    vec![Scalar::String("West".into()), Scalar::Integer(150), date(2024, 4, 1)],
                    vec![Scalar::String("North".into()), Scalar::Integer(300), date(2024, 6, 18)],
                    vec![Scalar::String("South".into()), Scalar::Integer(50), date(2024, 7, 7)],
                    vec![Scalar::String("East".into()), Scalar::Integer(220), date(2024, 9, 9)],
                    vec![Scalar::String("West".into()), Scalar::Integer(90), date(2024, 11, 11)],
                    vec![Scalar::String("North".into()), Scalar::Integer(180), date(2024, 12, 1)],
                    vec![Scalar::String("South".into()), Scalar::Integer(310), date(2024, 12, 25)],
                    vec![Scalar::String("East".into()), Scalar::Integer(60), date(2023, 3, 10)],
                    vec![Scalar::String("West".into()), Scalar::Integer(140), date(2023, 6, 15)],
                    vec![Scalar::String("North".into()), Scalar::Integer(200), date(2023, 9, 20)],
                    vec![Scalar::String("South".into()), Scalar::Integer(80), date(2023, 12, 5)],
                ],
            },
            ModelTable {
                name: "Date".into(),
                columns: vec![
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                    ModelColumn { name: "Year".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Quarter".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Month".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Weekday".into(), data_type: Some("Int64".into()) },
                ],
                rows: date_rows,
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
        measures: vec![
            measure("Total Amount", "SUM(Sales[Amount])"),
            measure("MTD", "TOTALMTD([Total Amount], 'Date'[Date])"),
            measure("QTD", "TOTALQTD([Total Amount], 'Date'[Date])"),
            measure("WTD", "TOTALWTD([Total Amount], Date)"),
            measure("YTD", "TOTALYTD([Total Amount], 'Date'[Date])"),
            measure("YTD June", "TOTALYTD([Total Amount], 'Date'[Date], ALL(Sales), \"6/30/2024\")"),
            measure("YTD All", "TOTALYTD([Total Amount], 'Date'[Date], ALL ())"),
            measure("QTD Remove All", "TOTALQTD([Total Amount], 'Date'[Date], REMOVEFILTERS ())"),
            measure("YTD Region All", "TOTALYTD([Total Amount], 'Date'[Date], ALL([Region]))"),
            measure("YTD Over Ten", "TOTALYTD(SUM(Sales[Amount]), 'Date'[Date], Sales[Amount] > 100)"),
            measure("YTD Unqualified", "TOTALYTD([Total Amount], 'Date'[Date], [Region] = \"East\")"),
            measure("OB Month", "OPENINGBALANCEMONTH([Total Amount], 'Date'[Date])"),
            measure("CB Month", "CLOSINGBALANCEMONTH([Total Amount], 'Date'[Date])"),
            measure("OB Quarter", "OPENINGBALANCEQUARTER([Total Amount], 'Date'[Date])"),
            measure("CB Quarter", "CLOSINGBALANCEQUARTER([Total Amount], 'Date'[Date])"),
            measure("OB Week", "OPENINGBALANCEWEEK([Total Amount], Date)"),
            measure("CB Week", "CLOSINGBALANCEWEEK([Total Amount], Date)"),
            measure("OB Year", "OPENINGBALANCEYEAR([Total Amount], 'Date'[Date])"),
            measure("CB Year", "CLOSINGBALANCEYEAR([Total Amount], 'Date'[Date])"),
            measure("OB Year June", "OPENINGBALANCEYEAR([Total Amount], 'Date'[Date], ALL(Sales), \"6/30/2024\")"),
            measure("CB Year June", "CLOSINGBALANCEYEAR([Total Amount], 'Date'[Date], ALL(Sales), \"6/30/2024\")"),
            measure("First Non Blank", "FIRSTNONBLANKVALUE('Date'[Date], [Total Amount])"),
            measure("Last Non Blank", "LASTNONBLANKVALUE('Date'[Date], [Total Amount])"),
            measure("Bad Date Column", "TOTALMTD([Total Amount], Sales[Amount])"),
            measure("Bad Filter", "TOTALMTD([Total Amount], 'Date'[Date], 1)"),
            measure("Bad Year End", "TOTALYTD([Total Amount], 'Date'[Date], ALL(Sales), \"not a date\")"),
            measure("Impossible Year End", "TOTALYTD([Total Amount], 'Date'[Date], ALL(Sales), \"2/30\")"),
        ],
    };
    model.parse_formulas().expect("model formulas parse");
    model
}

fn measure(name: &str, formula: &str) -> Measure {
    Measure { name: name.into(), formula: formula.into(), table: Some("Sales".into()), format: None, expr: None }
}

fn filters(model: &DataModel, pairs: &[(&str, &str, Scalar)]) -> FilterContext {
    let mut context = FilterContext::default();
    for (table, column, value) in pairs {
        let table_index = model.table_index(table).expect("table exists");
        let column_index = model.column_index(table_index, column).expect("column exists");
        context.set(table_index, column_index, [ScalarKey::from(value)]);
    }
    context
}

fn date_filter(model: &DataModel, dates: &[Scalar]) -> FilterContext {
    let table = model.table_index("Date").expect("Date table");
    let column = model.column_index(table, "Date").expect("Date column");
    let mut context = FilterContext::default();
    context.set(table, column, dates.iter().map(ScalarKey::from));
    context
}

fn eval_measure(model: &DataModel, name: &str, filters: FilterContext) -> Result<Scalar, String> {
    let evaluator = Evaluator::new(model).map_err(|error| error.to_string())?;
    evaluator.evaluate_measure(name, filters).map_err(|error| format!("{name}: {error}"))
}

fn assert_number(result: Result<Scalar, String>, expected: f64) {
    match result {
        Ok(value) => {
            let actual = value.to_number().expect("numeric result");
            assert!((actual - expected).abs() < 1e-9, "expected {expected}, got {actual} ({value:?})");
        }
        Err(error) => panic!("unexpected error: {error}"),
    }
}

fn february_filter(model: &DataModel) -> FilterContext {
    filters(model, &[("Date", "Year", Scalar::Integer(2024)), ("Date", "Month", Scalar::Integer(2))])
}

#[test]
fn period_to_date_totals() {
    let model = model();
    let february = february_filter(&model);
    // February 2024: 2024-02-03 (75) + 2024-02-20 (400); January 2024: 40 + 100 + 250.
    assert_number(eval_measure(&model, "MTD", february.clone()), 475.0);
    assert_number(eval_measure(&model, "QTD", february.clone()), 865.0);
    assert_number(eval_measure(&model, "YTD", february.clone()), 865.0);
    // Fiscal year ending June 30: H2 2023 (200 + 80) plus the 2024 YTD window.
    assert_number(eval_measure(&model, "YTD June", february), 1145.0);

    let january = date_filter(&model, &[date(2024, 1, 5)]);
    assert_number(eval_measure(&model, "MTD", january.clone()), 140.0);
    assert_number(eval_measure(&model, "QTD", january.clone()), 140.0);
    assert_number(eval_measure(&model, "YTD", january.clone()), 140.0);
    // 2024-01-05 is a Friday: the DAX week starts on Sunday 2023-12-31.
    assert_number(eval_measure(&model, "WTD", january), 140.0);

    let second_friday = date_filter(&model, &[date(2024, 1, 12)]);
    assert_number(eval_measure(&model, "WTD", second_friday), 250.0);

    // The last visible date anchors the window: Feb 28 only, not the whole month.
    let partial = date_filter(&model, &[date(2024, 1, 5), date(2024, 2, 28)]);
    assert_number(eval_measure(&model, "MTD", partial.clone()), 475.0);
    assert_number(eval_measure(&model, "QTD", partial.clone()), 865.0);
    // No sale in the week of 2024-02-28 (Sunday 25th to Saturday 2nd), so the sum is blank.
    assert_eq!(eval_measure(&model, "WTD", partial), Ok(Scalar::Blank));
}

#[test]
fn opening_and_closing_balances() {
    // Boundary dates need facts: add one sale on every date the balance functions select.
    let mut facts = model();
    let boundaries = [
        (2023, 6, 30, 900),
        (2023, 12, 30, 450),
        (2023, 12, 31, 500),
        (2024, 1, 6, 550),
        (2024, 1, 13, 650),
        (2024, 1, 31, 600),
        (2024, 2, 29, 700),
        (2024, 3, 31, 800),
        (2024, 6, 30, 950),
        (2024, 12, 31, 1000),
    ];
    let sales = &mut facts.tables[0].rows;
    for (year, month, day, amount) in boundaries {
        sales.push(vec![Scalar::String("Boundary".into()), Scalar::Integer(amount), date(year, month, day)]);
    }

    let february = february_filter(&facts);
    // EXCEL: CLOSINGBALANCEMONTH = CALCULATE(expr, ENDOFMONTH(dates)), OPENING = previous day.
    assert_number(eval_measure(&facts, "OB Month", february.clone()), 600.0);
    assert_number(eval_measure(&facts, "CB Month", february.clone()), 700.0);
    assert_number(eval_measure(&facts, "OB Quarter", february.clone()), 500.0);
    assert_number(eval_measure(&facts, "CB Quarter", february.clone()), 800.0);
    assert_number(eval_measure(&facts, "OB Year", february.clone()), 500.0);
    assert_number(eval_measure(&facts, "CB Year", february.clone()), 1000.0);
    // Fiscal year July 2023 - June 2024: opening is 2023-06-30, closing is 2024-06-30.
    assert_number(eval_measure(&facts, "OB Year June", february.clone()), 900.0);
    assert_number(eval_measure(&facts, "CB Year June", february), 950.0);

    let january = date_filter(&facts, &[date(2024, 1, 5)]);
    assert_number(eval_measure(&facts, "OB Month", january.clone()), 500.0);
    assert_number(eval_measure(&facts, "CB Month", january.clone()), 600.0);
    // The DAX week starts on Sunday 2023-12-31, so the opening date is 2023-12-30.
    assert_number(eval_measure(&facts, "OB Week", january.clone()), 450.0);
    assert_number(eval_measure(&facts, "CB Week", january), 550.0);

    let second_week = date_filter(&facts, &[date(2024, 1, 12)]);
    assert_number(eval_measure(&facts, "OB Week", second_week.clone()), 550.0);
    assert_number(eval_measure(&facts, "CB Week", second_week), 650.0);

    // March 31 carries a boundary fact, so the closing balance is that day's value.
    let march = filters(&facts, &[("Date", "Year", Scalar::Integer(2024)), ("Date", "Month", Scalar::Integer(3))]);
    assert_eq!(eval_measure(&facts, "CB Month", march), Ok(Scalar::Decimal(800.0)));

    // Without a fact on the boundary date Excel returns BLANK, not a scan backwards.
    let sparse = model();
    let february = february_filter(&sparse);
    assert_eq!(eval_measure(&sparse, "OB Month", february.clone()), Ok(Scalar::Blank));
    assert_eq!(eval_measure(&sparse, "CB Month", february), Ok(Scalar::Blank));
}

#[test]
fn first_and_last_non_blank_values() {
    let model = model();
    let february = february_filter(&model);
    assert_number(eval_measure(&model, "First Non Blank", february.clone()), 75.0);
    assert_number(eval_measure(&model, "Last Non Blank", february.clone()), 400.0);

    let january = date_filter(&model, &[date(2024, 1, 5)]);
    assert_number(eval_measure(&model, "First Non Blank", january.clone()), 100.0);
    assert_number(eval_measure(&model, "Last Non Blank", january), 100.0);

    let march = filters(&model, &[("Date", "Year", Scalar::Integer(2024)), ("Date", "Month", Scalar::Integer(3))]);
    assert_eq!(eval_measure(&model, "First Non Blank", march.clone()), Ok(Scalar::Blank));
    assert_eq!(eval_measure(&model, "Last Non Blank", march), Ok(Scalar::Blank));
}

#[test]
fn all_clears_the_outer_filter_context() {
    let model = model();
    // February 2024 restricted to the East region: the outer filter sits on the fact table, the
    // date filters only anchor the window.
    let east_february = filters(
        &model,
        &[("Sales", "Region", Scalar::String("East".into())), ("Date", "Year", Scalar::Integer(2024)), ("Date", "Month", Scalar::Integer(2))],
    );
    // Without a filter argument the region filter still applies: East only, January + February.
    assert_number(eval_measure(&model, "YTD", east_february.clone()), 140.0);
    assert_number(eval_measure(&model, "QTD", east_february.clone()), 140.0);

    // `ALL ()` clears the whole filter context, so every region is visible again over the same
    // year-to-date window (40 + 100 + 250 + 75 + 400 = 865).
    assert_number(eval_measure(&model, "YTD All", east_february.clone()), 865.0);
    // `REMOVEFILTERS ()` is the same modifier.
    assert_number(eval_measure(&model, "QTD Remove All", east_february.clone()), 865.0);
    // An unqualified column argument clears just that column: `[Region]` resolves to Sales[Region].
    assert_number(eval_measure(&model, "YTD Region All", east_february), 865.0);
}

#[test]
fn empty_window_and_invalid_arguments() {
    let model = model();

    // A date filter outside the model's date range leaves nothing visible.
    let outside = date_filter(&model, &[date(2030, 1, 1)]);
    assert_eq!(eval_measure(&model, "MTD", outside.clone()), Ok(Scalar::Blank));
    assert_eq!(eval_measure(&model, "CB Month", outside.clone()), Ok(Scalar::Blank));
    assert_eq!(eval_measure(&model, "First Non Blank", outside), Ok(Scalar::Blank));

    let error = eval_measure(&model, "Bad Date Column", FilterContext::default()).expect_err("amount is not a date");
    assert!(error.contains("not a date column"), "{error}");

    let error = eval_measure(&model, "Bad Filter", FilterContext::default()).expect_err("invalid filter");
    assert!(error.contains("table expression"), "{error}");

    let error = eval_measure(&model, "Bad Year End", FilterContext::default()).expect_err("invalid year end");
    assert!(error.contains("YearEndDate"), "{error}");
    // The range functions share the parser, so an impossible day is rejected here too.
    let error = eval_measure(&model, "Impossible Year End", FilterContext::default()).expect_err("2/30 is not a date");
    assert!(error.contains("YearEndDate"), "{error}");

    let evaluator = Evaluator::new(&model).expect("evaluator");
    let spec = lookup("TOTALMTD").expect("catalog entry");
    let error = evaluator.evaluate(&BoundExpr::Call { spec, args: Vec::new() }).expect_err("arity");
    assert!(error.message.contains("between 2 and 3"), "{}", error.message);

    let spec = lookup("FIRSTNONBLANKVALUE").expect("catalog entry");
    let measure_index = model.measure_index("Total Amount").expect("measure");
    let expr = BoundExpr::Call { spec, args: vec![BoundExpr::Literal(Literal::Integer(1)), BoundExpr::Measure(measure_index)] };
    let error = evaluator.evaluate(&expr).expect_err("non-column argument");
    assert!(error.message.contains("column reference"), "{}", error.message);
}

#[test]
fn comparison_and_unqualified_column_filters_are_applied() {
    // `TOTALYTD(expr, dates, filter)` is CALCULATE sugar: a comparison and an unqualified column
    // are filter arguments too, not table expressions.
    let model = model();
    let february = date_filter(&model, &[Scalar::DateTime(serial(2024, 2, 1)), Scalar::DateTime(serial(2024, 2, 29))]);
    let plain = eval_measure(&model, "YTD", february.clone()).expect("YTD");
    let filtered = eval_measure(&model, "YTD Over Ten", february).expect("YTD Over Ten");
    // Exact totals, so a filter argument the evaluator ignored could not pass: every 2024
    // January-February amount is above 10 but only two are above 100.
    assert_eq!(plain.to_number(), Some(865.0));
    assert_eq!(filtered.to_number(), Some(650.0));
    let east = eval_measure(&model, "YTD Unqualified", FilterContext::default()).expect("YTD Unqualified");
    assert_eq!(east.to_number(), Some(360.0));
}

#[test]
fn related_dimension_filters_reach_the_visible_date_range() {
    // Snowflaked calendar: `Date` is many-to-one against a `Year` dimension on its one side.
    // Filtering `Year` propagates to `Date`, so the dates visible to the range functions narrow
    // to the filtered year instead of spanning the whole model calendar.
    let mut model = model();
    model.tables.push(ModelTable {
        name: "Year".into(),
        columns: vec![ModelColumn { name: "Year".into(), data_type: Some("Int64".into()) }],
        rows: vec![vec![Scalar::Integer(2023)], vec![Scalar::Integer(2024)]],
    });
    model.relationships.push(Relationship {
        from_table: "Date".into(),
        from_column: "Year".into(),
        to_table: "Year".into(),
        to_column: "Year".into(),
        bidirectional: false,
        inactive: false,
    });
    model.measures.push(measure("Last Date", "LASTDATE('Date'[Date])"));
    model.measures.push(measure("First Date", "FIRSTDATE('Date'[Date])"));
    model.measures.push(measure("Dates YTD Rows", "COUNTROWS(DATESYTD('Date'[Date]))"));
    model.parse_formulas().expect("model formulas parse");

    let y2023 = filters(&model, &[("Year", "Year", Scalar::Integer(2023))]);
    // The axis ends on the last date of the year the relationship selects, not on the model's
    // overall last date (2024-12-31).
    assert_eq!(eval_measure(&model, "Last Date", y2023.clone()), Ok(date(2023, 12, 31)));
    assert_eq!(eval_measure(&model, "First Date", y2023.clone()), Ok(date(2023, 1, 1)));
    // 2023 is not a leap year: 365 days in the year-to-date window, not the 366 of 2024.
    assert_number(eval_measure(&model, "Dates YTD Rows", y2023.clone()), 365.0);
    // The year-to-date total is 2023's own facts: 60 + 140 + 200 + 80.
    assert_number(eval_measure(&model, "YTD", y2023.clone()), 480.0);

    let y2024 = filters(&model, &[("Year", "Year", Scalar::Integer(2024))]);
    assert_eq!(eval_measure(&model, "Last Date", y2024.clone()), Ok(date(2024, 12, 31)));
    assert_number(eval_measure(&model, "Dates YTD Rows", y2024.clone()), 366.0);
    // 2024's facts: 40 + 100 + 250 + 75 + 400 + 150 + 300 + 50 + 220 + 90 + 180 + 310.
    assert_number(eval_measure(&model, "YTD", y2024), 2165.0);
}

/// Fixtures for the two fact-table cases: a model whose date column is a fact column, so there is
/// no date dimension whose filters the window may replace.
fn fact_date_model(mut measures: Vec<Measure>, rows: Vec<Vec<Scalar>>, amount_type: &str) -> DataModel {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some(amount_type.into()) },
                ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
            ],
            rows,
        }],
        relationships: Vec::new(),
        measures: std::mem::take(&mut measures),
    };
    model.parse_formulas().expect("model formulas parse");
    model
}

#[test]
fn fact_date_totals_keep_the_other_fact_filters() {
    // The date column is a fact column, so the window replaces only that column's filter: the
    // Region slice has to keep applying, as it would to `CALCULATE ( SUM (…), DATESYTD (…) )`.
    let model = fact_date_model(
        vec![measure("YTD Fact", "TOTALYTD(SUM(Sales[Amount]), Sales[Date])"), measure("MTD Fact", "TOTALMTD(SUM(Sales[Amount]), Sales[Date])")],
        vec![
            vec![Scalar::String("East".into()), Scalar::Integer(10), date(2024, 1, 10)],
            vec![Scalar::String("East".into()), Scalar::Integer(20), date(2024, 3, 10)],
            vec![Scalar::String("West".into()), Scalar::Integer(100), date(2024, 1, 20)],
        ],
        "Int64",
    );
    let east = filters(&model, &[("Sales", "Region", Scalar::String("East".into()))]);
    // East's own year-to-date facts: 10 + 20. Dropping the Region filter would return 130.
    assert_number(eval_measure(&model, "YTD Fact", east.clone()), 30.0);
    assert_number(eval_measure(&model, "MTD Fact", east.clone()), 20.0);

    // A filter on the fact date column itself is replaced by the window, not intersected with it:
    // anchoring on 2024-03-10 makes the year-to-date window run from 2024-01-01, so both East rows
    // count (30). Keeping and intersecting the filter would return only 2024-03-10's 20.
    let east_march = filters(&model, &[("Sales", "Region", Scalar::String("East".into())), ("Sales", "Date", date(2024, 3, 10))]);
    assert_number(eval_measure(&model, "YTD Fact", east_march), 30.0);
}

#[test]
fn fact_date_totals_match_cells_that_carry_a_time_of_day() {
    // DateTime cells may store a time of day, and a day may hold more than one timestamp; every
    // cell of the period still has to count, not just the first one found for its day.
    let model = fact_date_model(
        vec![
            measure("CB Month", "CLOSINGBALANCEMONTH(SUM(Sales[Amount]), Sales[Date])"),
            measure("OB Month", "OPENINGBALANCEMONTH(SUM(Sales[Amount]), Sales[Date])"),
            measure("YTD Fact", "TOTALYTD(SUM(Sales[Amount]), Sales[Date])"),
        ],
        vec![
            vec![Scalar::String("East".into()), Scalar::Integer(5), Scalar::DateTime(serial(2024, 1, 1))],
            vec![Scalar::String("East".into()), Scalar::Integer(7), Scalar::DateTime(serial(2024, 2, 29) + 9.5 / 24.0)],
            vec![Scalar::String("East".into()), Scalar::Integer(3), Scalar::DateTime(serial(2024, 3, 31) + 9.5 / 24.0)],
            vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::DateTime(serial(2024, 3, 31) + 14.0 / 24.0)],
        ],
        "Int64",
    );
    // March's closing balance covers both 2024-03-31 timestamps, not the earlier February date.
    assert_number(eval_measure(&model, "CB Month", FilterContext::default()), 13.0);
    // March's opening balance is the day before its start: 2024-02-29 09:30.
    assert_number(eval_measure(&model, "OB Month", FilterContext::default()), 7.0);
    // The year-to-date window keeps every cell: 5 + 7 + 3 + 10, including both March 31 rows.
    assert_number(eval_measure(&model, "YTD Fact", FilterContext::default()), 25.0);
}

#[test]
fn balance_filters_intersect_the_boundary_day() {
    // The boundary day holds two timestamps; a filter argument that keeps only one of them has to
    // intersect with the boundary keys, exactly like `TOTAL*`, instead of being dropped.
    let mut model = fact_date_model(
        vec![measure("CB Month", "CLOSINGBALANCEMONTH(SUM(Sales[Amount]), Sales[Date])")],
        vec![
            vec![Scalar::String("East".into()), Scalar::Integer(3), Scalar::DateTime(serial(2024, 3, 31) + 9.5 / 24.0)],
            vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::DateTime(serial(2024, 3, 31) + 14.0 / 24.0)],
        ],
        "Int64",
    );
    model.measures.push(measure("CB Month Filtered", "CLOSINGBALANCEMONTH(SUM(Sales[Amount]), Sales[Date], Sales[Date] > DATE(2024,3,31) + 10/24)"));
    model.parse_formulas().expect("model formulas parse");

    // Without a filter both same-day cells are the boundary.
    assert_number(eval_measure(&model, "CB Month", FilterContext::default()), 13.0);
    // With a filter that keeps only the 14:00 cell the boundary is one cell: 10, not 13.
    assert_number(eval_measure(&model, "CB Month Filtered", FilterContext::default()), 10.0);
}

#[test]
fn a_calendar_table_argument_uses_its_typed_date_column() {
    // `TOTALWTD(…, Calendar)` must resolve the calendar's date column exactly like `DATESWTD`:
    // the declared date type wins over an earlier untyped column.
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Calendar".into(),
                columns: vec![
                    ModelColumn { name: "Label".into(), data_type: None },
                    ModelColumn { name: "Day".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![vec![Scalar::String("first".into()), date(2024, 1, 1)], vec![Scalar::String("second".into()), date(2024, 1, 2)]],
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![[10.0, serial(2024, 1, 1)], [20.0, serial(2024, 1, 2)]]
                    .into_iter()
                    .map(|[amount, serial]| vec![Scalar::Decimal(amount), Scalar::DateTime(serial)])
                    .collect(),
            },
        ],
        relationships: Vec::new(),
        measures: vec![measure("Week To Date", "TOTALWTD(SUM(Sales[Amount]), Calendar)")],
    };
    model.parse_formulas().expect("model formulas parse");

    // 2024-01-02 is a Tuesday, so the week starts on Sunday 2023-12-31: both calendar days are in
    // the window and the sum is 30. Binding the untyped `Label` column would return BLANK.
    assert_number(eval_measure(&model, "Week To Date", FilterContext::default()), 30.0);
}

#[test]
fn a_calendar_with_one_untyped_column_is_still_a_calendar() {
    // A calendar whose only date column is untyped keeps working: the unambiguous untyped column
    // is the fallback once no typed date column and no `Date` column exist.
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Calendar".into(),
                columns: vec![ModelColumn { name: "Day".into(), data_type: None }],
                rows: vec![vec![date(2024, 1, 1)], vec![date(2024, 1, 2)]],
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![[10.0, serial(2024, 1, 1)], [20.0, serial(2024, 1, 2)]]
                    .into_iter()
                    .map(|[amount, serial]| vec![Scalar::Decimal(amount), Scalar::DateTime(serial)])
                    .collect(),
            },
        ],
        relationships: Vec::new(),
        measures: vec![measure("Week To Date", "TOTALWTD(SUM(Sales[Amount]), Calendar)")],
    };
    model.parse_formulas().expect("model formulas parse");
    assert_number(eval_measure(&model, "Week To Date", FilterContext::default()), 30.0);
}

#[test]
fn a_calendar_date_column_declared_as_a_non_date_type_is_rejected() {
    // An integer `Date` column (YYYYMMDD keys) is not a date axis: report the model, do not read
    // its numbers as OLE serials.
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Calendar".into(),
                columns: vec![ModelColumn { name: "Date".into(), data_type: Some("Int64".into()) }],
                rows: vec![vec![Scalar::Integer(20_240_101)]],
            },
            ModelTable {
                name: "Sales".into(),
                columns: vec![ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) }],
                rows: vec![vec![Scalar::Decimal(10.0)]],
            },
        ],
        relationships: Vec::new(),
        measures: vec![measure("Week To Date", "TOTALWTD(SUM(Sales[Amount]), Calendar)")],
    };
    model.parse_formulas().expect("model formulas parse");
    let error = eval_measure(&model, "Week To Date", FilterContext::default()).expect_err("an Int64 Date column is not a date axis");
    assert!(error.contains("has no date column"), "{error}");
}
