//! Semantic audit regressions.
//!
//! Each test pins a documented DAX result that the evaluator used to get wrong. The expected
//! values come from the function contracts in `crates/dax/src/functions/` and from Microsoft's
//! DAX reference; the regression names say which rule is being held.
//!
//! Covered: BLANK's empty-string comparison, `CALCULATE` filters written with `IN` and its
//! removal modifiers, `IF`'s Boolean condition, and the BLANK/Boolean/error propagation rules the
//! audit depends on. The per-function cases (`TRIM`, `MOD`, `ROUNDUP`/`ROUNDDOWN`, `DATEDIFF`,
//! `TIME`, `WEEKDAY`) live with the evaluator families that implement them, under
//! `tests/eval_*.rs`.

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
    // The model engine divides by zero without an error: BLANK for 0/0 and the numerator's sign
    // of infinity otherwise (Excel's worksheet gives #DIV/0!, DAX does not).
    assert_eq!(eval("ISERROR(1/0)"), Scalar::Boolean(false));
    assert_eq!(eval("5 / 0"), Scalar::Decimal(f64::INFINITY));
    assert_eq!(eval("-5 / 0"), Scalar::Decimal(f64::NEG_INFINITY));
    assert_eq!(eval("0 / 0"), Scalar::Blank);
    assert_eq!(eval("BLANK() / BLANK()"), Scalar::Blank);
    assert_eq!(eval("5 / BLANK()"), Scalar::Decimal(f64::INFINITY));
    assert_eq!(eval("BLANK() + BLANK()"), Scalar::Blank);
    assert_eq!(eval("BLANK() - BLANK()"), Scalar::Blank);
    // Errors still stay errors unless a handler catches them.
    assert_eq!(eval("ISERROR(1)"), Scalar::Boolean(false));
    assert!(error("ERROR(\"boom\")").contains("boom"));
    assert_eq!(number("IFERROR(1/0, 42)"), f64::INFINITY);
    assert_eq!(eval("IFERROR(ERROR(\"boom\"), 42)"), Scalar::Integer(42));
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
    // East (10 + 30) plus West (20).
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
    // The filter form follows DAX equality too: the Decimal column matches an Integer literal.
    assert_eq!(eval_in_sales(r#"CALCULATE(SUM(Sales[Amount]), Sales[Amount] IN { 10 })"#), Scalar::Decimal(10.0));
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

#[test]
fn strict_equality_compares_values_and_keeps_blank_strict() {
    // `==` is strict only about BLANK; every other pair follows DAX comparison, so a number
    // compares by value and text ignores case.
    assert_eq!(eval("2 == 2.0"), Scalar::Boolean(true));
    assert_eq!(eval("1 == 2"), Scalar::Boolean(false));
    assert_eq!(eval("\"a\" == \"A\""), Scalar::Boolean(true));
    assert_eq!(eval("BLANK() == BLANK()"), Scalar::Boolean(true));
    assert_eq!(eval("BLANK() == 0"), Scalar::Boolean(false));
    assert_eq!(eval("BLANK() == \"\""), Scalar::Boolean(false));
    // `=` keeps coercing BLANK, as before.
    assert_eq!(eval("BLANK() = 0"), Scalar::Boolean(true));
}

#[test]
fn strict_equality_matches_a_decimal_column_against_an_integer_literal() {
    // A Decimal column compared with an integer literal compares numerically: `Sales[Amount]` is
    // `Scalar::Decimal`, the literal `Scalar::Integer`.
    assert_eq!(eval_in_sales("COUNTROWS(FILTER(Sales, Sales[Amount] == 10))"), Scalar::Integer(1));
    assert_eq!(eval_in_sales("COUNTROWS(FILTER(Sales, Sales[Region] == \"east\"))"), Scalar::Integer(2));
    // `strict_eq` keeps text case-sensitive but joins numeric keys across variants, so a
    // relationship whose keys are Integer on one side and Decimal on the other still matches.
    assert!(gridcraft_dax::Scalar::Integer(2).strict_eq(&Scalar::Decimal(2.0)));
    assert!(!gridcraft_dax::Scalar::Integer(2).strict_eq(&Scalar::Decimal(2.5)));
    assert!(!Scalar::String("a".into()).strict_eq(&Scalar::String("A".into())));
    // Beyond 2^53 two integers share an `f64`, and they must stay distinct keys.
    assert!(!Scalar::Integer(9_007_199_254_740_992).strict_eq(&Scalar::Integer(9_007_199_254_740_993)));
    // A large key still matches itself, in the same variant and through currency.
    assert!(Scalar::Integer(9_007_199_254_740_993).strict_eq(&Scalar::Integer(9_007_199_254_740_993)));
    assert!(Scalar::Currency(9_007_199_254_740_993).strict_eq(&Scalar::Currency(9_007_199_254_740_993)));
    // `compare` keeps the same exactness, so `IN` and `=` do not conflate large integers either.
    assert_eq!(Scalar::Integer(9_007_199_254_740_992).compare(&Scalar::Integer(9_007_199_254_740_993)), Some(std::cmp::Ordering::Less));
    assert!(Scalar::Integer(9_007_199_254_740_993).compare(&Scalar::Integer(9_007_199_254_740_993)) == Some(std::cmp::Ordering::Equal));
    // A large integer against an integral decimal stays exact instead of rounding into it.
    assert_eq!(Scalar::Integer(9_007_199_254_740_993).compare(&Scalar::Decimal(9_007_199_254_740_992.0)), Some(std::cmp::Ordering::Greater));
}

#[test]
fn calculate_applies_removal_modifiers_before_its_filters() {
    // `ALL(Sales)` clears the outer context first, whatever its position, and the explicit filters
    // then apply: both argument orders leave the Region filter in force.
    assert_eq!(eval_in_sales("CALCULATE(COUNTROWS(Sales), Sales[Region] = \"East\", ALL(Sales))"), Scalar::Integer(2));
    assert_eq!(eval_in_sales("CALCULATE(COUNTROWS(Sales), ALL(Sales), Sales[Region] = \"East\")"), Scalar::Integer(2));
    // ALL on a column clears only that column, so the Amount filter survives.
    assert_eq!(eval_in_sales("CALCULATE(COUNTROWS(Sales), Sales[Amount] = 10, ALL(Sales[Region]))"), Scalar::Integer(1));
}
