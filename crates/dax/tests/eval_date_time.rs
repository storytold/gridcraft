//! Evaluation tests for the date/time function family.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::{Scalar, TableColumn, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

/// The OLE Automation serial of a date, built with `chrono` (a normal dependency of the crate,
/// independent of the evaluator under test) from the 1899-12-30 epoch.
fn serial(year: i32, month: u32, day: u32) -> f64 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).expect("the Excel epoch is a valid date");
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day).expect("the fixture date is valid");
    (date - epoch).num_days() as f64
}

fn model() -> DataModel {
    DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Order Date".into(), data_type: Some("DateTime".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![
                    vec![Scalar::DateTime(serial(2023, 6, 15)), Scalar::Decimal(10.0)],
                    vec![Scalar::DateTime(serial(2024, 1, 15)), Scalar::Decimal(20.0)],
                    vec![Scalar::DateTime(serial(2024, 3, 10)), Scalar::Decimal(30.0)],
                ],
            },
            ModelTable {
                name: "Date".into(),
                columns: vec![ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) }],
                // No rows: CALENDARAUTO must take its bounds from the Sales date column.
                rows: Vec::new(),
            },
        ],
        relationships: Vec::new(),
        measures: Vec::new(),
    }
}

/// A model whose only column is numeric: CALENDARAUTO has nothing to scan.
fn model_without_dates() -> DataModel {
    DataModel {
        tables: vec![
            ModelTable {
                name: "Numbers".into(),
                columns: vec![ModelColumn { name: "Value".into(), data_type: Some("Int64".into()) }],
                rows: vec![vec![Scalar::Integer(1)]],
            },
            ModelTable {
                name: "Empty".into(),
                columns: vec![ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) }],
                rows: Vec::new(),
            },
        ],
        relationships: Vec::new(),
        measures: Vec::new(),
    }
}

/// A model whose date column is declared `Date` rather than `DateTime`: DAX has both date types,
/// so the fiscal calendar has to be built from either.
fn model_with_date_typed_columns() -> DataModel {
    DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Order Date".into(), data_type: Some("Date".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
            ],
            rows: vec![
                vec![Scalar::DateTime(serial(2023, 6, 15)), Scalar::Decimal(10.0)],
                vec![Scalar::DateTime(serial(2023, 3, 31)), Scalar::Decimal(20.0)],
            ],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    }
}

fn eval_in(model: &DataModel, src: &str) -> Result<Scalar, String> {
    let evaluator = Evaluator::new(model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn eval_table_in(model: &DataModel, src: &str) -> Result<TableValue, String> {
    let evaluator = Evaluator::new(model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.to_string())
}

fn eval(src: &str) -> Result<Scalar, String> {
    eval_in(&model(), src)
}

fn eval_table(src: &str) -> Result<TableValue, String> {
    eval_table_in(&model(), src)
}

fn value(src: &str) -> Scalar {
    eval(src).unwrap_or_else(|error| panic!("{src}: {error}"))
}

fn assert_close(actual: Scalar, expected: f64, label: &str) {
    match actual {
        Scalar::Decimal(value) if (value - expected).abs() <= 1e-12 => {}
        other => panic!("{label}: expected {expected}, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Text conversion
// ---------------------------------------------------------------------------------------------

#[test]
fn datevalue_reads_the_common_text_dates() {
    assert_eq!(value("DATEVALUE(\"2024-01-15\")"), Scalar::DateTime(45_306.0));
    assert_eq!(value("DATEVALUE(\"2024/1/15\")"), Scalar::DateTime(45_306.0));
    // The documented en-US form, month first.
    assert_eq!(value("DATEVALUE(\"1/8/2009\")"), Scalar::DateTime(39_821.0));
    // A time portion is ignored: DATEVALUE returns the date.
    assert_eq!(value("DATEVALUE(\"2024-01-15 16:48:23\")"), Scalar::DateTime(45_306.0));
    // A 12-hour clock suffix is accepted in both date orders.
    let expected = Scalar::DateTime(serial(2024, 1, 15));
    assert_eq!(value("DATEVALUE(\"2024-01-15 4:48 PM\")"), expected);
    assert_eq!(value("DATEVALUE(\"1/15/2024 4:48 PM\")"), expected);
    assert_eq!(value("DATEVALUE(DATE(2024,1,15))"), Scalar::DateTime(45_306.0));
    assert_eq!(value("DATEVALUE(\"2024-02-29\")"), Scalar::DateTime(45_351.0));
}

#[test]
fn datevalue_rejects_invalid_text() {
    assert!(eval("DATEVALUE(\"not a date\")").is_err());
    // Impossible days and month-first days above 12 are errors, not serial rollover.
    assert!(eval("DATEVALUE(\"2024-02-30\")").is_err());
    assert!(eval("DATEVALUE(\"31/12/2024\")").is_err());
    assert!(eval("DATEVALUE(\"2024-13-01\")").is_err());
    assert!(eval("DATEVALUE(\"\")").is_err());
}

#[test]
fn timevalue_reads_the_common_text_times() {
    let clock = (16.0 * 3600.0 + 48.0 * 60.0 + 23.0) / 86_400.0;
    assert_eq!(value("TIMEVALUE(\"16:48:23\")"), Scalar::DateTime(clock));
    assert_eq!(value("TIMEVALUE(\"4:48:23 PM\")"), Scalar::DateTime(clock));
    assert_eq!(value("TIMEVALUE(\"2024-01-15 16:48:23\")"), Scalar::DateTime(clock));
    assert_eq!(value("TIMEVALUE(\"12:00 AM\")"), Scalar::DateTime(0.0));
    assert_eq!(value("TIMEVALUE(\"12:30 PM\")"), Scalar::DateTime((12.0 * 3600.0 + 30.0 * 60.0) / 86_400.0));
    assert_eq!(value("TIMEVALUE(\"6:00\")"), Scalar::DateTime(0.25));
    assert_eq!(value("TIMEVALUE(\"2024-01-15\")"), Scalar::DateTime(0.0));
}

#[test]
fn timevalue_rejects_invalid_text() {
    assert!(eval("TIMEVALUE(\"25:00\")").is_err());
    assert!(eval("TIMEVALUE(\"12:61\")").is_err());
    assert!(eval("TIMEVALUE(\"lunch\")").is_err());
    assert!(eval("TIMEVALUE(\"13:00 PM\")").is_err());
}

// ---------------------------------------------------------------------------------------------
// Scalar date parts
// ---------------------------------------------------------------------------------------------

#[test]
fn quarter_maps_month_to_calendar_quarter() {
    assert_eq!(value("QUARTER(DATE(2024,1,15))"), Scalar::Integer(1));
    assert_eq!(value("QUARTER(DATE(2024,3,31))"), Scalar::Integer(1));
    assert_eq!(value("QUARTER(DATE(2024,4,1))"), Scalar::Integer(2));
    assert_eq!(value("QUARTER(DATE(2024,7,1))"), Scalar::Integer(3));
    assert_eq!(value("QUARTER(DATE(2024,10,5))"), Scalar::Integer(4));
    assert_eq!(value("QUARTER(DATE(2024,12,31))"), Scalar::Integer(4));
    assert_eq!(value("QUARTER(\"2024-05-15\")"), Scalar::Integer(2));
}

#[test]
fn weeknum_matches_excel_week_systems() {
    // System 1: the week containing January 1 is week 1, weeks start on Sunday by default.
    assert_eq!(value("WEEKNUM(DATE(2024,1,1))"), Scalar::Integer(1));
    assert_eq!(value("WEEKNUM(DATE(2024,1,15))"), Scalar::Integer(3));
    assert_eq!(value("WEEKNUM(DATE(2024,1,15), 2)"), Scalar::Integer(3));
    assert_eq!(value("WEEKNUM(DATE(2024,1,15), 2)"), value("WEEKNUM(DATE(2024,1,15), 11)"));
    // Monday start: Sunday 2024-01-07 still belongs to week 1.
    assert_eq!(value("WEEKNUM(DATE(2024,1,7), 2)"), Scalar::Integer(1));
    // Sunday start: the same Sunday begins week 2.
    assert_eq!(value("WEEKNUM(DATE(2024,1,7))"), Scalar::Integer(2));
    assert_eq!(value("WEEKNUM(DATE(2024,1,8), 2)"), Scalar::Integer(2));
    // A year can end in week 53 when January 1 falls late in the week.
    assert_eq!(value("WEEKNUM(DATE(2023,12,31))"), Scalar::Integer(53));
    assert_eq!(value("WEEKNUM(\"2024-01-15\")"), Scalar::Integer(3));
}

#[test]
fn weeknum_iso_weeks_cross_year_boundaries() {
    assert_eq!(value("WEEKNUM(DATE(2024,1,15), 21)"), Scalar::Integer(3));
    // 2021-01-01 (Friday) belongs to ISO week 53 of 2020.
    assert_eq!(value("WEEKNUM(DATE(2021,1,1), 21)"), Scalar::Integer(53));
    // 2018-12-31 (Monday) and 2024-12-30 (Monday) are week 1 of the next year.
    assert_eq!(value("WEEKNUM(DATE(2018,12,31), 21)"), Scalar::Integer(1));
    assert_eq!(value("WEEKNUM(DATE(2024,12,30), 21)"), Scalar::Integer(1));
    assert_eq!(value("WEEKNUM(DATE(2024,12,29), 21)"), Scalar::Integer(52));
    assert_eq!(value("WEEKNUM(DATE(2020,12,31), 21)"), Scalar::Integer(53));
    assert!(eval("WEEKNUM(DATE(2024,1,1), 3)").is_err());
    assert!(eval("WEEKNUM(DATE(2024,1,1), 10)").is_err());
}

// ---------------------------------------------------------------------------------------------
// YEARFRAC
// ---------------------------------------------------------------------------------------------

#[test]
fn yearfrac_supports_every_basis() {
    let start = "DATE(2024,1,1)";
    let end = "DATE(2024,7,1)";
    // Leap year 2024: Jan 1 to Jul 1 is 182 actual days.
    assert_close(value(&format!("YEARFRAC({start}, {end})")), 0.5, "basis 0 default");
    assert_close(value(&format!("YEARFRAC({start}, {end}, 0)")), 180.0 / 360.0, "basis 0");
    assert_close(value(&format!("YEARFRAC({start}, {end}, 1)")), 182.0 / 366.0, "basis 1");
    assert_close(value(&format!("YEARFRAC({start}, {end}, 2)")), 182.0 / 360.0, "basis 2");
    assert_close(value(&format!("YEARFRAC({start}, {end}, 3)")), 182.0 / 365.0, "basis 3");
    assert_close(value(&format!("YEARFRAC({start}, {end}, 4)")), 180.0 / 360.0, "basis 4");
}

#[test]
fn yearfrac_handles_calendar_boundaries_and_reversed_ranges() {
    // A whole leap year is 1 for the 30/360 convention and 365/366 for actual/actual.
    assert_close(value("YEARFRAC(DATE(2024,1,1), DATE(2024,12,31), 0)"), 1.0, "30/360 whole year");
    assert_close(value("YEARFRAC(DATE(2024,1,1), DATE(2024,12,31), 1)"), 365.0 / 366.0, "actual/actual whole year");
    // Excel's US 30/360 February rule: the end of February counts as day 30 but March 31 keeps 31.
    assert_close(value("YEARFRAC(DATE(2019,2,28), DATE(2019,3,31), 0)"), 31.0 / 360.0, "February rule");
    // Two end-of-February dates collapse to 30/30, which is a full year.
    assert_close(value("YEARFRAC(DATE(2011,2,28), DATE(2012,2,29), 0)"), 1.0, "February to February");
    // actual/actual uses 366 when the period contains a February 29.
    assert_close(value("YEARFRAC(DATE(2011,3,1), DATE(2012,2,29), 1)"), 365.0 / 366.0, "leap day between");
    // Multi-year actual/actual divides by the average calendar year length.
    assert_close(value("YEARFRAC(DATE(2000,1,1), DATE(2003,1,1), 1)"), 1096.0 / (1461.0 / 4.0), "average year");
    // Excel ignores the argument order: the reversed pair returns the positive fraction.
    assert_close(value("YEARFRAC(DATE(2024,7,1), DATE(2024,1,1), 2)"), 182.0 / 360.0, "reversed");
    assert_close(value("YEARFRAC(\"2024-01-01\", \"2024-07-01\", 2)"), 182.0 / 360.0, "text dates");
    // dax.guide's documented example: 2011-01-01 .. 2011-12-15.
    assert_close(value("YEARFRAC(DATE(2011,1,1), DATE(2011,12,15), 0)"), 344.0 / 360.0, "dax.guide 30/360");
    assert_close(value("YEARFRAC(DATE(2011,1,1), DATE(2011,12,15), 1)"), 348.0 / 365.0, "dax.guide actual/actual");
}

#[test]
fn yearfrac_rejects_unknown_bases() {
    assert!(eval("YEARFRAC(DATE(2024,1,1), DATE(2024,12,31), 5)").is_err());
    assert!(eval("YEARFRAC(DATE(2024,1,1), DATE(2024,12,31), -1)").is_err());
}

// ---------------------------------------------------------------------------------------------
// NETWORKDAYS
// ---------------------------------------------------------------------------------------------

#[test]
fn networkdays_counts_working_days_with_weekends_and_holidays() {
    // January 2024 has 31 days, starting on a Monday, with 8 weekend days.
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31))"), Scalar::Integer(23));
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 1)"), Scalar::Integer(23));
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 1, { DATE(2024,1,15) })"), Scalar::Integer(22));
    // A holiday already on a weekend does not count twice.
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 1, { DATE(2024,1,6) })"), Scalar::Integer(23));
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 1, { DATE(2024,1,1), DATE(2024,1,15) })"), Scalar::Integer(21));
    // Weekend code 2 is Sunday/Monday, 11 is Sunday only.
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 2)"), Scalar::Integer(22));
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 11)"), Scalar::Integer(27));
    // A reversed range returns the negative count.
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,31), DATE(2024,1,1))"), Scalar::Integer(-23));
    // A single working day and a single weekend day.
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,2), DATE(2024,1,2))"), Scalar::Integer(1));
    assert_eq!(value("NETWORKDAYS(DATE(2024,1,6), DATE(2024,1,6))"), Scalar::Integer(0));
}

#[test]
fn networkdays_rejects_bad_weekend_codes() {
    assert!(eval("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 0)").is_err());
    assert!(eval("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 8)").is_err());
    assert!(eval("NETWORKDAYS(DATE(2024,1,1), DATE(2024,1,31), 18)").is_err());
}

// ---------------------------------------------------------------------------------------------
// CALENDAR / CALENDARAUTO
// ---------------------------------------------------------------------------------------------

#[test]
fn calendar_builds_the_inclusive_date_column() {
    let table = eval_table("CALENDAR(DATE(2024,2,27), DATE(2024,3,1))").expect("calendar table");
    assert_eq!(table.columns, vec![TableColumn { name: "Date".into() }]);
    // 2024 is a leap year, so February 29 is included.
    assert_eq!(
        table.rows,
        vec![
            vec![Scalar::DateTime(serial(2024, 2, 27))],
            vec![Scalar::DateTime(serial(2024, 2, 28))],
            vec![Scalar::DateTime(serial(2024, 2, 29))],
            vec![Scalar::DateTime(serial(2024, 3, 1))],
        ]
    );
    assert_eq!(eval_table("CALENDAR(DATE(2024,1,1), DATE(2024,1,31))").expect("january").rows.len(), 31);
    assert_eq!(value("COUNTROWS(CALENDAR(DATE(2024,1,1), DATE(2024,1,31)))"), Scalar::Integer(31));
    // Table functions are not scalars.
    assert!(eval("CALENDAR(DATE(2024,1,1), DATE(2024,1,2))").is_err());
}

#[test]
fn calendar_rejects_reversed_bounds() {
    assert!(eval_table("CALENDAR(DATE(2024,2,1), DATE(2024,1,1))").is_err());
}

#[test]
fn calendar_rejects_oversized_spans() {
    // The whole in-range span is 2,958,466 days, above the 1,000,000-row generation ceiling, so
    // it must fail instead of allocating the table.
    let error = eval_table("CALENDAR(DATE(1899,12,30), DATE(9999,12,31))").expect_err("oversized calendar");
    assert!(error.contains("too large"), "{error}");
    // 1900-01-01 is serial 2, so exactly 1,000,000 days end on 4637-11-27; the cap is inclusive
    // there and one day further fails.
    let largest = eval_table("CALENDAR(DATE(1900,1,1), DATE(4637,11,27))").expect("largest calendar");
    assert_eq!(largest.rows.len(), 1_000_000);
    assert_eq!(largest.rows.last().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(4637, 11, 27))));
    let error = eval_table("CALENDAR(DATE(1900,1,1), DATE(4637,11,28))").expect_err("one day over the cap");
    assert!(error.contains("too large"), "{error}");
}

#[test]
fn calendarauto_covers_whole_fiscal_years() {
    // Sales spans 2023-06-15 .. 2024-03-10 and Date spans 2023-01-01 .. 2024-12-31.
    let table = eval_table("CALENDARAUTO()").expect("calendar auto");
    assert_eq!(table.columns, vec![TableColumn { name: "Date".into() }]);
    assert_eq!(table.rows.first().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2023, 1, 1))));
    assert_eq!(table.rows.last().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2024, 12, 31))));
    assert_eq!(table.rows.len(), 731);
    assert_eq!(value("COUNTROWS(CALENDARAUTO())"), Scalar::Integer(731));

    // Fiscal years ending in March: 2023-04-01 .. 2024-03-31, including leap day 2024-02-29.
    let fiscal = eval_table("CALENDARAUTO(3)").expect("fiscal calendar");
    assert_eq!(fiscal.rows.first().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2023, 4, 1))));
    assert_eq!(fiscal.rows.last().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2024, 3, 31))));
    assert_eq!(fiscal.rows.len(), 366);
    assert!(fiscal.rows.iter().any(|row| row[0] == Scalar::DateTime(serial(2024, 2, 29))));
}

#[test]
fn calendarauto_reads_date_typed_columns() {
    // 2023-03-31 .. 2023-06-15, in a column declared `Date`: the whole 2023 calendar year.
    let table = eval_table_in(&model_with_date_typed_columns(), "CALENDARAUTO()").expect("calendar auto over Date columns");
    assert_eq!(table.rows.first().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2023, 1, 1))));
    assert_eq!(table.rows.last().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2023, 12, 31))));
    assert_eq!(table.rows.len(), 365);
    // A fiscal year ending in March: two fiscal years, since June 2023 falls in FY2024.
    let fiscal = eval_table_in(&model_with_date_typed_columns(), "CALENDARAUTO(3)").expect("fiscal calendar over Date columns");
    assert_eq!(fiscal.rows.first().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2022, 4, 1))));
    assert_eq!(fiscal.rows.last().map(|row| row[0].clone()), Some(Scalar::DateTime(serial(2024, 3, 31))));
}

#[test]
fn calendarauto_validates_arguments_and_model() {
    assert!(eval_table("CALENDARAUTO(0)").is_err());
    assert!(eval_table("CALENDARAUTO(13)").is_err());
    assert!(eval_table_in(&model_without_dates(), "CALENDARAUTO()").is_err());
    // Table functions are not scalars.
    assert!(eval("CALENDARAUTO()").is_err());
}

// ---------------------------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------------------------

#[test]
// The test runs natively and needs an independent clock reference for the date serials.
#[allow(clippy::disallowed_methods)]
fn clock_functions_track_the_system_clock() {
    let clock = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() / 86_400.0 + 25_569.0)
            .expect("system clock")
    };
    let serial_of = |src: &str| match value(src) {
        Scalar::DateTime(serial) => serial,
        other => panic!("{src} produced {other:?}"),
    };
    // Each function is a separate evaluation; retry if a local or UTC day boundary falls inside
    // the sampling window, so the floor comparisons cannot race midnight.
    let mut attempts = 0;
    let (today, now, utc_today, utc_now) = loop {
        let before = clock();
        let (today, now) = (serial_of("TODAY()"), serial_of("NOW()"));
        let (utc_today, utc_now) = (serial_of("UTCTODAY()"), serial_of("UTCNOW()"));
        let after = clock();
        if before.floor() == after.floor() && today == now.floor() && utc_today == utc_now.floor() {
            break (today, now, utc_today, utc_now);
        }
        attempts += 1;
        assert!(attempts < 5, "clock functions disagree: TODAY {today}, NOW {now}, UTCTODAY {utc_today}, UTCNOW {utc_now}");
    };
    let expected = clock();
    assert!((utc_now - expected).abs() < 1.0 / 86_400.0, "UTCNOW follows the system clock");
    assert!((0.0..1.0).contains(&now.fract()));
    // The host supplies one wall clock, so the local and UTC variants never differ by a day.
    assert!((now - utc_now).abs() < 1.0);
    assert!((today - utc_today).abs() <= 1.0);
    // Volatile functions take no arguments.
    assert!(eval("NOW(1)").is_err());
    assert!(eval("TODAY(1)").is_err());
}

#[test]
fn edate_and_eomonth_survive_extreme_month_offsets() {
    // `months` is arbitrary input: the month arithmetic must saturate instead of overflowing.
    let model = model();
    for (formula, name) in [("EDATE(DATE(2024,1,15), 1E18)", "EDATE"), ("EOMONTH(DATE(2024,1,15), -1E18)", "EOMONTH")] {
        match eval_in(&model, formula) {
            // Out-of-range serials are reported, but the month arithmetic itself must not overflow.
            Ok(value) => assert!(matches!(value, Scalar::DateTime(serial) if serial.is_finite() || serial.is_nan()), "{name}: {value:?}"),
            Err(error) => assert!(!error.is_empty(), "{name} reported an empty error"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Relocated regressions
//
// These pin documented results that the evaluator used to get wrong; they moved here from
// `eval_audit_semantics.rs` and `eval_text.rs` because their functions live in this family.
// ---------------------------------------------------------------------------------------------

/// The single numeric value of an expression; panics when it is not numeric.
fn number(src: &str) -> f64 {
    let evaluated = value(src);
    evaluated.to_number().unwrap_or_else(|| panic!("{src} is not numeric: {evaluated:?}"))
}

/// The message of the error an expression raises.
fn error(src: &str) -> String {
    match eval(src) {
        Err(message) => message,
        Ok(evaluated) => panic!("{src}: expected an error, got {evaluated:?}"),
    }
}

/// The text form of an expression, as produced by `& ""`.
fn text(src: &str) -> String {
    match value(src) {
        Scalar::String(text) => text,
        other => panic!("{src} should return text, got {other:?}"),
    }
}

#[test]
fn mod_follows_the_divisor_sign() {
    // Microsoft Learn, MOD: "The result has the same sign as the divisor."
    assert_eq!(value("MOD(7, 3)"), Scalar::Integer(1));
    assert_eq!(value("MOD(-3, 2)"), Scalar::Integer(1));
    assert_eq!(value("MOD(3, -2)"), Scalar::Integer(-1));
    assert_eq!(value("MOD(-3, -2)"), Scalar::Integer(-1));
    assert_eq!(value("MOD(10, 5)"), Scalar::Integer(0));
    // A fractional operand keeps the Decimal result.
    assert_eq!(number("MOD(5.5, 2)"), 1.5);
    assert_eq!(number("MOD(-5.5, 2)"), 0.5);
    // A zero divisor is a division-by-zero error, not BLANK.
    assert!(error("MOD(1, 0)").contains("MOD divisor must not be zero"));
}

#[test]
fn roundup_and_rounddown_follow_the_sign() {
    // Microsoft Learn: ROUNDDOWN rounds toward zero, ROUNDUP away from zero.
    assert_eq!(number("ROUNDDOWN(-2.9, 0)"), -2.0);
    assert_eq!(number("ROUNDDOWN(2.9, 0)"), 2.0);
    assert_eq!(number("ROUNDDOWN(-2.345, 2)"), -2.34);
    assert_eq!(number("ROUNDUP(-2.1, 0)"), -3.0);
    assert_eq!(number("ROUNDUP(2.1, 0)"), 3.0);
    assert_eq!(number("ROUNDUP(2.341, 2)"), 2.35);
    // ROUND still rounds half away from zero.
    assert_eq!(number("ROUND(2.5, 0)"), 3.0);
    assert_eq!(number("ROUND(-2.5, 0)"), -3.0);
}

#[test]
fn datediff_counts_interval_boundaries() {
    // The catalog: DATEDIFF counts the Interval boundaries crossed between two dates.
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 3, 1), DAY)"), Scalar::Integer(60));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 3, 1), MONTH)"), Scalar::Integer(2));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2025, 3, 1), YEAR)"), Scalar::Integer(1));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 7, 1), QUARTER)"), Scalar::Integer(2));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 3, 1), WEEK)"), Scalar::Integer(8));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 1, 2), HOUR)"), Scalar::Integer(24));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 1, 2), MINUTE)"), Scalar::Integer(1_440));
    // A month boundary is crossed even by a single day.
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 31), DATE(2024, 2, 1), MONTH)"), Scalar::Integer(1));
    // The sign follows Date2 - Date1.
    assert_eq!(value("DATEDIFF(DATE(2024, 3, 1), DATE(2024, 1, 1), MONTH)"), Scalar::Integer(-2));
    // A text argument keeps the time of day, exactly like a DateTime one: the sub-day intervals
    // must not be read as midnight on both sides.
    assert_eq!(value("DATEDIFF(\"2024-01-01 10:00:00\", \"2024-01-01 12:00:00\", HOUR)"), Scalar::Integer(2));
    assert_eq!(value("DATEDIFF(\"2024-01-01 10:00:00\", \"2024-01-01 12:00:00\", MINUTE)"), Scalar::Integer(120));
    assert_eq!(value("DATEDIFF(\"2024-01-01 10:00:00\", \"2024-01-01 12:00:00\", SECOND)"), Scalar::Integer(7_200));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1) + 10/24, \"2024-01-01 12:00:00\", HOUR)"), Scalar::Integer(2));
    assert!(error("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 3, 1), FORTNIGHT)").contains("DATEDIFF interval"));
}

#[test]
fn time_wraps_inside_one_day() {
    // The catalog: "Overflowing arguments wrap inside the same day instead of raising an error
    // (TIME ( 10, 90, 0 ) is 11:30 and TIME ( 50, 0, 0 ) is 02:00)".
    assert_eq!(number("TIME(10, 90, 0)"), 11.5 / 24.0);
    assert_eq!(number("TIME(50, 0, 0)"), 2.0 / 24.0);
    assert_eq!(number("TIME(25, 0, 0)"), 1.0 / 24.0);
    assert_eq!(number("TIME(0, 0, 0)"), 0.0);
    assert_eq!(number("TIME(1, 2, 3)"), (3_723.0) / 86_400.0);
    assert!(error("TIME(-1, 0, 0)").contains("TIME argument 1"));
    assert!(error("TIME(32768, 0, 0)").contains("TIME argument 1"));
}

#[test]
fn weekday_honours_the_return_type() {
    // 2024-01-01 is a Monday. Return type 1 numbers Sunday 1, 2 numbers Monday 1, 3 numbers
    // Monday 0, and 11-17 start the week on Monday through Sunday.
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 1))"), Scalar::Integer(2));
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 1), 1)"), Scalar::Integer(2));
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 1), 2)"), Scalar::Integer(1));
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 1), 3)"), Scalar::Integer(0));
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 1), 11)"), Scalar::Integer(1));
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 1), 12)"), Scalar::Integer(7));
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 1), 17)"), Scalar::Integer(2));
    // 2024-01-07 is a Sunday.
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 7), 2)"), Scalar::Integer(7));
    assert_eq!(value("WEEKDAY(DATE(2024, 1, 7), 3)"), Scalar::Integer(6));
    assert!(error("WEEKDAY(DATE(2024, 1, 1), 5)").contains("WEEKDAY return type"));
}

#[test]
fn datediff_counts_boundaries_for_sub_day_and_week_intervals() {
    // 10:59:59 to 11:00:01 crosses the 11:00 hour boundary even though two seconds passed.
    assert_eq!(value("DATEDIFF(TIME(10, 59, 59), TIME(11, 0, 1), HOUR)"), Scalar::Integer(1));
    assert_eq!(value("DATEDIFF(TIME(10, 59, 58), TIME(10, 59, 59), SECOND)"), Scalar::Integer(1));
    assert_eq!(value("DATEDIFF(TIME(10, 59, 59), TIME(11, 0, 0), MINUTE)"), Scalar::Integer(1));
    // Exactly one elapsed hour is still one boundary; 59 minutes is none.
    assert_eq!(value("DATEDIFF(TIME(10, 0, 0), TIME(11, 0, 0), HOUR)"), Scalar::Integer(1));
    assert_eq!(value("DATEDIFF(TIME(10, 0, 0), TIME(10, 59, 59), HOUR)"), Scalar::Integer(0));
    // Weeks are Sunday-based: 2024-01-01 is a Monday, so the week boundary is the Sunday before it.
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 1, 6), WEEK)"), Scalar::Integer(0));
    assert_eq!(value("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 1, 7), WEEK)"), Scalar::Integer(1));
    // The sign follows Date2 - Date1.
    assert_eq!(value("DATEDIFF(TIME(11, 0, 1), TIME(10, 59, 59), HOUR)"), Scalar::Integer(-1));
}

#[test]
fn currency_and_large_numbers_concatenate_in_dax_form() {
    assert_eq!(text("CURRENCY(10) & \"\""), "10");
    assert_eq!(text("CURRENCY(10.5) & \"\""), "10.5");
    assert_eq!(text("CONVERT(1, CURRENCY) & \"\""), "1");
    // Very large whole numbers use Excel's exponent text with the explicit sign.
    assert_eq!(text("1e15 & \"\""), "1E+15");
    // DAX prints 15 significant digits, so a floating-point residue does not surface.
    assert_eq!(text("0.1 + 0.2 & \"\""), "0.3");
}
