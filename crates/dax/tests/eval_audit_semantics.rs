//! Semantic audit regressions.
//!
//! Each test pins a documented DAX result that the evaluator used to get wrong. The expected
//! values come from the function contracts in `crates/dax/src/functions/` and from Microsoft's
//! DAX reference; the regression names say which rule is being held.
//!
//! Covered: `TRIM` collapsing, `MOD`'s divisor sign and zero divisor, `ROUNDUP`/`ROUNDDOWN`
//! direction for negative numbers, `DATEDIFF` intervals, `TIME` day wrapping, `WEEKDAY` return
//! types, BLANK's empty-string comparison, `CALCULATE` filters written with `IN`, `IF`'s Boolean
//! condition, and the BLANK/Boolean/error propagation rules the audit depends on.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel { tables: Vec::new(), relationships: Vec::new(), measures: Vec::new() };
    model.parse_formulas().expect("model parses");
    model
}

/// One table with a text column and a numeric column, enough for CALCULATE filter tests.
fn sales_model() -> DataModel {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
            ],
            rows: vec![
                vec![Scalar::String("East".into()), Scalar::Decimal(10.0)],
                vec![Scalar::String("West".into()), Scalar::Decimal(20.0)],
                vec![Scalar::String("East".into()), Scalar::Decimal(30.0)],
                vec![Scalar::String("North".into()), Scalar::Decimal(40.0)],
                vec![Scalar::String("South".into()), Scalar::Decimal(50.0)],
            ],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    model
}

fn eval_in_sales(source: &str) -> Scalar {
    let model = sales_model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression(source).expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    evaluator.evaluate(&bound).expect("evaluate")
}

fn eval(source: &str) -> Scalar {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression(source).expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    evaluator.evaluate(&bound).expect("evaluate")
}

fn error(source: &str) -> String {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression(source).expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    evaluator.evaluate(&bound).expect_err("evaluate must fail").message
}

fn number(source: &str) -> f64 {
    eval(source).to_number().unwrap_or_else(|| panic!("{source} is not numeric: {:?}", eval(source)))
}

#[test]
fn trim_collapses_internal_spaces_but_only_ascii_space() {
    // Microsoft Learn, TRIM: "Removes all spaces from text except for single spaces between
    // words." Only character 32 counts, as the catalog states.
    assert_eq!(eval(r#"TRIM("  DAX is          awesome !!!    ")"#), Scalar::String("DAX is awesome !!!".into()));
    assert_eq!(eval(r#"TRIM("  a  b  ")"#), Scalar::String("a b".into()));
    assert_eq!(eval(r#"TRIM("a")"#), Scalar::String("a".into()));
    assert_eq!(eval(r#"TRIM("   ")"#), Scalar::String(String::new()));
    // A tab and a non-breaking space are not spaces for TRIM.
    assert_eq!(eval("TRIM(\"\ta\t\")"), Scalar::String("\ta\t".into()));
    assert_eq!(eval("TRIM(\"a\u{00A0} b\")"), Scalar::String("a\u{00A0} b".into()));
}

#[test]
fn mod_follows_the_divisor_sign() {
    // Microsoft Learn, MOD: "The result has the same sign as the divisor."
    assert_eq!(eval("MOD(7, 3)"), Scalar::Integer(1));
    assert_eq!(eval("MOD(-3, 2)"), Scalar::Integer(1));
    assert_eq!(eval("MOD(3, -2)"), Scalar::Integer(-1));
    assert_eq!(eval("MOD(-3, -2)"), Scalar::Integer(-1));
    assert_eq!(eval("MOD(10, 5)"), Scalar::Integer(0));
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
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 3, 1), DAY)"), Scalar::Integer(60));
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 3, 1), MONTH)"), Scalar::Integer(2));
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 1), DATE(2025, 3, 1), YEAR)"), Scalar::Integer(1));
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 7, 1), QUARTER)"), Scalar::Integer(2));
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 3, 1), WEEK)"), Scalar::Integer(8));
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 1, 2), HOUR)"), Scalar::Integer(24));
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 1), DATE(2024, 1, 2), MINUTE)"), Scalar::Integer(1_440));
    // A month boundary is crossed even by a single day.
    assert_eq!(eval("DATEDIFF(DATE(2024, 1, 31), DATE(2024, 2, 1), MONTH)"), Scalar::Integer(1));
    // The sign follows Date2 - Date1.
    assert_eq!(eval("DATEDIFF(DATE(2024, 3, 1), DATE(2024, 1, 1), MONTH)"), Scalar::Integer(-2));
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
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 1))"), Scalar::Integer(2));
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 1), 1)"), Scalar::Integer(2));
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 1), 2)"), Scalar::Integer(1));
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 1), 3)"), Scalar::Integer(0));
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 1), 11)"), Scalar::Integer(1));
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 1), 12)"), Scalar::Integer(7));
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 1), 17)"), Scalar::Integer(2));
    // 2024-01-07 is a Sunday.
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 7), 2)"), Scalar::Integer(7));
    assert_eq!(eval("WEEKDAY(DATE(2024, 1, 7), 3)"), Scalar::Integer(6));
    assert!(error("WEEKDAY(DATE(2024, 1, 1), 5)").contains("WEEKDAY return type"));
}

#[test]
fn blank_boolean_and_error_propagation() {
    // BLANK coerces to 0 in arithmetic and comparisons, but multiplying by BLANK stays BLANK.
    assert_eq!(number("BLANK() + 1"), 1.0);
    assert_eq!(eval("BLANK() * 5"), Scalar::Blank);
    assert_eq!(eval("BLANK() = 0"), Scalar::Boolean(true));
    assert_eq!(eval("BLANK() == 0"), Scalar::Boolean(false));
    assert_eq!(eval("BLANK() & \"x\""), Scalar::String("x".into()));
    assert_eq!(eval("ISBLANK(BLANK())"), Scalar::Boolean(true));
    // Booleans are 1/0 in arithmetic.
    assert_eq!(number("TRUE() + 1"), 2.0);
    // Errors stay errors unless a handler catches them.
    assert_eq!(eval("ISERROR(1/0)"), Scalar::Boolean(true));
    assert_eq!(eval("ISERROR(1)"), Scalar::Boolean(false));
    assert_eq!(eval("IFERROR(1/0, 42)"), Scalar::Integer(42));
    assert_eq!(eval("DIVIDE(1, 0)"), Scalar::Blank);
    assert_eq!(eval("DIVIDE(1, 0, 99)"), Scalar::Integer(99));
    assert_eq!(number("DIVIDE(1, 2)"), 0.5);
}

#[test]
fn blank_compares_as_empty_text_and_as_zero() {
    // Excel's DAX: BLANK converts to an empty string against text and to 0 against a number.
    // `BLANK () = ""` is TRUE in Excel (observed through the Power Pivot model connection).
    assert_eq!(eval("BLANK() = \"\""), Scalar::Boolean(true));
    assert_eq!(eval("\"\" = BLANK()"), Scalar::Boolean(true));
    assert_eq!(eval("BLANK() <> \"\""), Scalar::Boolean(false));
    assert_eq!(eval("BLANK() = \"x\""), Scalar::Boolean(false));
    assert_eq!(eval("\"x\" = BLANK()"), Scalar::Boolean(false));
    assert_eq!(eval("BLANK() = 0"), Scalar::Boolean(true));
    assert_eq!(eval("BLANK() = BLANK()"), Scalar::Boolean(true));
    // BLANK converts to FALSE against a logical value, so the comparison is TRUE. This case was
    // not covered by the Excel sample (BLANK has no worksheet equivalent); it follows the same
    // convert-to-the-other-operand rule as the two above.
    assert_eq!(eval("BLANK() = FALSE()"), Scalar::Boolean(true));
}

#[test]
fn calculate_filter_accepts_an_in_list() {
    // Excel returns 1010 for the equivalent fixture query.
    assert_eq!(eval_in_sales(r#"CALCULATE(SUM(Sales[Amount]), Sales[Region] IN { "East", "West" })"#), Scalar::Decimal(60.0));
    // A list of one.
    assert_eq!(eval_in_sales(r#"CALCULATE(SUM(Sales[Amount]), Sales[Region] IN { "North" })"#), Scalar::Decimal(40.0));
    // Duplicates in the list do not double-count rows.
    assert_eq!(eval_in_sales(r#"CALCULATE(SUM(Sales[Amount]), Sales[Region] IN { "East", "East", "East" })"#), Scalar::Decimal(40.0));
    // A value that is not in the column filters everything out.
    assert_eq!(eval_in_sales(r#"CALCULATE(SUM(Sales[Amount]), Sales[Region] IN { "Nowhere" })"#), Scalar::Blank);
    // IN over a single-column table expression.
    assert_eq!(
        eval_in_sales(r#"CALCULATE(SUM(Sales[Amount]), Sales[Region] IN FILTER(VALUES(Sales[Region]), Sales[Region] <> "East"))"#),
        Scalar::Decimal(110.0)
    );
    // The scalar IN operator keeps working.
    assert_eq!(eval("1 IN { 1, 2 }"), Scalar::Boolean(true));
    assert_eq!(eval("3 IN { 1, 2 }"), Scalar::Boolean(false));
}

#[test]
fn if_requires_a_boolean_condition() {
    // Excel rejects a condition it cannot read as logical; it does not take the false branch.
    assert!(error(r#"IF("x", 1, 2)"#).contains("IF argument 1 must be Boolean"));
    assert!(error(r#"IF("x", 1, 2)"#).contains("String"));
    // The documented conditions still work: TRUE, FALSE and BLANK (which converts to FALSE).
    assert_eq!(eval("IF(TRUE(), 1, 2)"), Scalar::Integer(1));
    assert_eq!(eval("IF(FALSE(), 1, 2)"), Scalar::Integer(2));
    assert_eq!(eval("IF(BLANK(), 1, 2)"), Scalar::Integer(2));
    assert_eq!(eval("IF(1 > 0, 1, 2)"), Scalar::Integer(1));
}
