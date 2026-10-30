//! Integration tests for the time-intelligence totals and balance evaluator functions.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, ScalarKey};
use gridcraft_dax::{BoundExpr, Evaluator, FilterContext, Literal, lookup};

/// Excel/OLE date serial for a whole day (the same algorithm the evaluator uses).
fn serial(year: i64, month: i64, day: i64) -> f64 {
    let mut y = year;
    let mut m = month;
    y += (m - 1).div_euclid(12);
    m = (m - 1).rem_euclid(12) + 1;
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468 + 25569) as f64
}

fn parts(value: f64) -> (i64, i64, i64) {
    let days = value.floor() as i64;
    let z = days - 25569 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (y + i64::from(m <= 2), m, d)
}

fn date(year: i64, month: i64, day: i64) -> Scalar {
    Scalar::DateTime(serial(year, month, day))
}

/// The fixture shared with the worksheet/measure cases: Sales facts for 2023-2024 and a
/// contiguous Date table covering both years.
fn model() -> DataModel {
    let mut date_rows = Vec::new();
    let mut day = serial(2023, 1, 1);
    let end = serial(2024, 12, 31);
    while day <= end {
        let (year, month, _) = parts(day);
        let weekday = (day.floor() as i64).rem_euclid(7);
        let weekday = if weekday == 0 { 7 } else { weekday };
        date_rows.push(vec![
            Scalar::DateTime(day),
            Scalar::Integer(year),
            Scalar::Integer((month - 1) / 3 + 1),
            Scalar::Integer(month),
            Scalar::Integer(weekday),
        ]);
        day += 1.0;
    }

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

    // Without a fact on the boundary date Excel returns BLANK, not a scan backwards.
    let march = filters(&facts, &[("Date", "Year", Scalar::Integer(2024)), ("Date", "Month", Scalar::Integer(3))]);
    assert_eq!(eval_measure(&facts, "CB Month", march), Ok(Scalar::Decimal(800.0)));

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
