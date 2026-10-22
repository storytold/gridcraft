//! Evaluator tests for the securities financial functions (coupons, prices and yields).
//!
//! Expected values come from the published Microsoft reference results for the Excel/DAX
//! equivalents (learn.microsoft.com/en-us/dax/<function>-function-dax) or are derived by hand
//! from the documented formulas.

use gridcraft_dax::model::DataModel;
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    DataModel { tables: Vec::new(), relationships: Vec::new(), measures: Vec::new() }
}

fn eval(src: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn value(src: &str) -> f64 {
    match eval(src).unwrap_or_else(|error| panic!("{src}: {error}")) {
        Scalar::Decimal(value) => value,
        Scalar::Integer(value) => value as f64,
        Scalar::Currency(value) => value as f64 / 10_000.0,
        Scalar::DateTime(value) => value,
        other => panic!("{src}: expected a number, got {other:?}"),
    }
}

fn date(src: &str) -> f64 {
    match eval(src).unwrap_or_else(|error| panic!("{src}: {error}")) {
        Scalar::DateTime(value) => value,
        other => panic!("{src}: expected a date, got {other:?}"),
    }
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    let scale = expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() / scale <= tolerance,
        "expected {expected} but got {actual} (relative error {})",
        (actual - expected).abs() / scale
    );
}

fn fails(src: &str) {
    assert!(eval(src).is_err(), "{src} should fail");
}

// --- coupon schedule -------------------------------------------------------------------------

#[test]
fn coupncd_and_couppcd_follow_the_maturity_anchored_schedule() {
    assert_eq!(date("COUPNCD(DATE(2011,1,25), DATE(2011,11,15), 2, 1)"), date("DATE(2011,5,15)"));
    assert_eq!(date("COUPPCD(DATE(2011,1,25), DATE(2011,11,15), 2, 1)"), date("DATE(2010,11,15)"));
    // Quarterly coupons step back three months at a time.
    assert_eq!(date("COUPNCD(DATE(2011,1,25), DATE(2011,11,15), 4)"), date("DATE(2011,2,15)"));
    // A settlement exactly on a coupon date keeps that date as the previous coupon.
    assert_eq!(date("COUPPCD(DATE(2011,5,15), DATE(2011,11,15), 2)"), date("DATE(2011,5,15)"));
    assert_eq!(date("COUPNCD(DATE(2011,5,15), DATE(2011,11,15), 2)"), date("DATE(2011,11,15)"));
    // Month-end maturity clamps the derived coupon day (Aug 31 minus six months is Feb 29).
    assert_eq!(date("COUPPCD(DATE(2011,5,30), DATE(2011,8,31), 2)"), date("DATE(2011,2,28)"));
}

#[test]
fn coupon_counts_and_period_lengths() {
    assert_eq!(value("COUPNUM(DATE(2007,1,25), DATE(2008,11,15), 2)"), 4.0);
    assert_eq!(value("COUPNUM(DATE(2011,1,25), DATE(2011,11,15), 2)"), 2.0);
    assert_eq!(value("COUPNUM(DATE(2011,5,15), DATE(2011,11,15), 2)"), 1.0);
    // Actual/actual uses the real length of the coupon period, the 30/360 bases use 360/freq.
    assert_eq!(value("COUPDAYS(DATE(2011,1,25), DATE(2011,11,15), 2, 1)"), 181.0);
    assert_eq!(value("COUPDAYS(DATE(2011,1,25), DATE(2011,11,15), 2, 0)"), 180.0);
    assert_eq!(value("COUPDAYS(DATE(2011,1,25), DATE(2011,11,15), 4, 2)"), 90.0);
    assert_eq!(value("COUPDAYS(DATE(2011,1,25), DATE(2011,11,15), 2, 3)"), 182.5);
}

#[test]
fn coupon_day_counts_around_settlement() {
    // Nov 15 2010 to Jan 25 2011 is 71 actual days.
    assert_eq!(value("COUPDAYBS(DATE(2011,1,25), DATE(2011,11,15), 2, 1)"), 71.0);
    assert_eq!(value("COUPDAYSNC(DATE(2011,1,25), DATE(2011,11,15), 2, 1)"), 110.0);
    // 30/360 counts 30 days per month: Nov 15 to Feb 15 is 90 days.
    assert_eq!(value("COUPDAYBS(DATE(2008,2,15), DATE(2017,11,15), 2, 0)"), 90.0);
    assert_eq!(value("COUPDAYSNC(DATE(2008,2,15), DATE(2017,11,15), 2, 0)"), 90.0);
    assert_eq!(value("COUPDAYBS(DATE(2011,5,15), DATE(2011,11,15), 2, 2)"), 0.0);
    // February is a normal 30-day month for 30/360 coupon counts: Feb 15 to Mar 15 is 30 days.
    assert_eq!(value("COUPDAYBS(DATE(2008,3,15), DATE(2008,8,15), 2, 0)"), 30.0);
    assert_eq!(value("COUPDAYSNC(DATE(2008,3,15), DATE(2008,8,15), 2, 0)"), 150.0);
    assert_eq!(value("COUPDAYBS(DATE(2008,3,15), DATE(2008,8,15), 2, 4)"), 30.0);
}

#[test]
fn coupon_functions_validate_arguments() {
    fails("COUPNUM(DATE(2011,11,15), DATE(2011,11,15), 2)");
    fails("COUPNUM(DATE(2012,1,1), DATE(2011,11,15), 2)");
    fails("COUPDAYS(DATE(2011,1,25), DATE(2011,11,15), 3)");
    fails("COUPDAYS(DATE(2011,1,25), DATE(2011,11,15), 2, 5)");
    fails("COUPDAYS(DATE(2011,1,25), DATE(2011,11,15), 2, -1)");
    fails("COUPNCD(DATE(2011,1,25), DATE(2011,11,15), 0)");
}

// --- accrued interest ------------------------------------------------------------------------

#[test]
fn accrint_sums_quasi_coupon_periods() {
    // Issue 2007-03-01, first interest 2008-08-31, settlement 2008-05-01, semiannual, 30/360.
    // Whole quasi periods accrue one coupon each; the stub from issue accrues 180/180 and the
    // current period 61/180, so 50 * (1 + 1 + 61/180) = 116.9444...
    close(value("ACCRINT(DATE(2007,3,1), DATE(2008,8,31), DATE(2008,5,1), 0.1, 1000, 2, 0)"), 116.944444444444, 1e-12);
    // The short-stub example accrues 60/180 of a 50 coupon.
    close(value("ACCRINT(DATE(2008,3,1), DATE(2008,8,31), DATE(2008,5,1), 0.1, 1000, 2, 0)"), 50.0 * 60.0 / 180.0, 1e-12);
    // calc_method FALSE skips the first (issue) quasi period.
    close(value("ACCRINT(DATE(2007,3,1), DATE(2008,8,31), DATE(2008,5,1), 0.1, 1000, 2, 0, FALSE)"), 66.9444444444445, 1e-12);
    fails("ACCRINT(DATE(2008,5,1), DATE(2008,8,31), DATE(2008,5,1), 0.1, 1000, 2, 0)");
    fails("ACCRINT(DATE(2008,3,1), DATE(2008,8,31), DATE(2008,5,1), 0, 1000, 2, 0)");
    fails("ACCRINT(DATE(2008,3,1), DATE(2008,8,31), DATE(2008,5,1), 0.1, 0, 2, 0)");
    fails("ACCRINT(DATE(2008,3,1), DATE(2008,8,31), DATE(2008,5,1), 0.1, 1000, 3)");
}

#[test]
fn accrintm_uses_the_year_fraction_to_maturity() {
    // 75 actual days over 365 on a 1000 par at 10%.
    close(value("ACCRINTM(DATE(2008,4,1), DATE(2008,6,15), 0.1, 1000, 3)"), 20.5479452054795, 1e-12);
    close(value("ACCRINTM(DATE(2008,4,1), DATE(2008,6,15), 0.1, 1000, 2)"), 1000.0 * 0.1 * 75.0 / 360.0, 1e-12);
    fails("ACCRINTM(DATE(2008,6,15), DATE(2008,4,1), 0.1, 1000)");
    fails("ACCRINTM(DATE(2008,4,1), DATE(2008,6,15), 0.1, 1000, 7)");
}

// --- discount securities ---------------------------------------------------------------------

#[test]
fn discount_and_interest_rates() {
    close(value("DISC(DATE(2018,7,1), DATE(2048,1,1), 97.975, 100, 1)"), 0.000686384169121348, 1e-9);
    close(value("INTRATE(DATE(2008,2,15), DATE(2008,5,15), 1000000, 1014420, 2)"), 0.05768, 1e-9);
    close(value("RECEIVED(DATE(2008,2,15), DATE(2008,5,15), 1000000, 0.0575, 2)"), 1014584.6544071, 1e-12);
    close(value("PRICEDISC(DATE(2008,2,16), DATE(2008,3,1), 0.0525, 100, 2)"), 99.7958333333333, 1e-12);
    close(value("YIELDDISC(DATE(2008,2,16), DATE(2008,3,1), 99.795, 100, 2)"), 0.0528225719868583, 1e-9);
    fails("DISC(DATE(2008,3,1), DATE(2008,2,16), 97.975, 100, 2)");
    fails("DISC(DATE(2008,2,16), DATE(2008,3,1), 0, 100, 2)");
    fails("INTRATE(DATE(2008,2,15), DATE(2008,5,15), 0, 1014420, 2)");
    fails("RECEIVED(DATE(2008,2,15), DATE(2008,5,15), 1000000, 0, 2)");
    fails("PRICEDISC(DATE(2008,2,16), DATE(2008,3,1), 0.0525, 100, 9)");
    fails("YIELDDISC(DATE(2008,2,16), DATE(2008,3,1), 0, 100, 2)");
}

#[test]
fn price_and_yield_at_maturity() {
    // The semiannual coupon is 3.05, DIM/B = 154/360, A/B = 96/360, DSM/B = 58/360 on 30/360.
    close(value("PRICEMAT(DATE(2008,2,15), DATE(2008,4,13), DATE(2007,11,11), 0.061, 0.061, 0)"), 99.9844988755569, 1e-12);
    close(value("YIELDMAT(DATE(2008,3,15), DATE(2008,11,3), DATE(2007,11,8), 0.0625, 100.0123, 0)"), 0.0609543336915387, 1e-12);
    fails("PRICEMAT(DATE(2008,2,15), DATE(2008,2,15), DATE(2007,11,11), 0.061, 0.061, 0)");
    fails("PRICEMAT(DATE(2008,2,15), DATE(2008,4,13), DATE(2008,2,15), 0.061, 0.061, 0)");
    fails("PRICEMAT(DATE(2008,2,15), DATE(2008,4,13), DATE(2007,11,11), -0.061, 0.061, 0)");
    fails("YIELDMAT(DATE(2008,3,15), DATE(2008,11,3), DATE(2007,11,8), 0.0625, 0, 0)");
    fails("YIELDMAT(DATE(2007,11,8), DATE(2008,11,3), DATE(2007,11,8), 0.0625, 100.0123, 0)");
}

#[test]
fn treasury_bill_functions() {
    close(value("TBILLPRICE(DATE(2008,3,31), DATE(2008,6,1), 0.09)"), 98.45, 1e-12);
    close(value("TBILLYIELD(DATE(2008,3,31), DATE(2008,6,1), 98.45)"), 0.0914169629253426, 1e-9);
    close(value("TBILLEQ(DATE(2008,3,31), DATE(2008,6,1), 0.0914)"), 0.094151493565943, 1e-9);
    fails("TBILLPRICE(DATE(2008,6,1), DATE(2008,3,31), 0.09)");
    fails("TBILLPRICE(DATE(2008,3,31), DATE(2008,6,1), 0)");
    fails("TBILLPRICE(DATE(2008,3,31), DATE(2009,6,1), 0.09)");
    fails("TBILLYIELD(DATE(2008,3,31), DATE(2008,6,1), 0)");
    fails("TBILLEQ(DATE(2008,3,31), DATE(2008,6,1), -0.09)");
}

// --- coupon bonds ----------------------------------------------------------------------------

#[test]
fn price_of_a_semiannual_bond() {
    close(value("PRICE(DATE(2008,2,15), DATE(2017,11,15), 0.0575, 0.065, 100, 2, 0)"), 94.6343616213221, 1e-9);
    // A par bond bought exactly on a coupon date prices at its redemption value:
    // 3/1.03 + 103/1.03^2 = 100.
    close(value("PRICE(DATE(2016,11,15), DATE(2017,11,15), 0.06, 0.06, 100, 2, 0)"), 100.0, 1e-12);
    // One coupon left: 30/360 gives A = 120 days (May 15 to Sep 15) and DSR = E - A = 60 days,
    // so the price uses simple interest over one third of a coupon period.
    let single = (100.0 * 0.08 / 2.0 + 100.0) / (1.0 + 0.06 / 2.0 * (60.0 / 180.0)) - 100.0 * 0.08 / 2.0 * (120.0 / 180.0);
    close(value("PRICE(DATE(2011,9,15), DATE(2011,11,15), 0.08, 0.06, 100, 2, 0)"), single, 1e-12);
    fails("PRICE(DATE(2011,11,15), DATE(2011,11,15), 0.08, 0.06, 100, 2, 0)");
    fails("PRICE(DATE(2011,9,15), DATE(2011,11,15), -0.08, 0.06, 100, 2, 0)");
    fails("PRICE(DATE(2011,9,15), DATE(2011,11,15), 0.08, -0.06, 100, 2, 0)");
    fails("PRICE(DATE(2011,9,15), DATE(2011,11,15), 0.08, 0.06, 0, 2, 0)");
    fails("PRICE(DATE(2011,9,15), DATE(2011,11,15), 0.08, 0.06, 100, 3, 0)");
    fails("PRICE(DATE(2011,9,15), DATE(2011,11,15), 0.08, 0.06, 100, 2, 5)");
}

#[test]
fn yield_inverts_price() {
    close(value("YIELD(DATE(2008,2,15), DATE(2016,11,15), 0.0575, 95.04287, 100, 2, 0)"), 0.0650000068807314, 1e-9);
    // PRICE and YIELD are inverse functions for a multi-coupon bond.
    let price = value("PRICE(DATE(2018,1,15), DATE(2028,1,15), 0.05, 0.07, 100, 2, 1)");
    let source = format!("YIELD(DATE(2018,1,15), DATE(2028,1,15), 0.05, {price}, 100, 2, 1)");
    close(value(&source), 0.07, 1e-10);
    fails("YIELD(DATE(2011,11,15), DATE(2011,11,15), 0.08, 95.0, 100, 2, 0)");
    fails("YIELD(DATE(2011,9,15), DATE(2011,11,15), -0.08, 95.0, 100, 2, 0)");
    fails("YIELD(DATE(2011,9,15), DATE(2011,11,15), 0.08, 0, 100, 2, 0)");
    fails("YIELD(DATE(2011,9,15), DATE(2011,11,15), 0.08, 95.0, 0, 2, 0)");
    fails("YIELD(DATE(2011,9,15), DATE(2011,11,15), 0.08, 95.0, 100, 2, -2)");
}

#[test]
fn duration_and_modified_duration() {
    close(value("DURATION(DATE(2018,7,1), DATE(2048,1,1), 0.08, 0.09, 2, 1)"), 10.9191452815919, 1e-9);
    close(value("MDURATION(DATE(2008,1,1), DATE(2016,1,1), 0.08, 0.09, 2, 1)"), 5.73566981391884, 1e-9);
    // Modified duration is Macaulay duration divided by (1 + yield / frequency).
    let macaulay = value("DURATION(DATE(2008,1,1), DATE(2016,1,1), 0.08, 0.09, 2, 1)");
    close(value("MDURATION(DATE(2008,1,1), DATE(2016,1,1), 0.08, 0.09, 2, 1)"), macaulay / (1.0 + 0.09 / 2.0), 1e-12);
    // A zero coupon bond's Macaulay duration equals its time to maturity in years.
    close(value("DURATION(DATE(2008,1,1), DATE(2018,1,1), 0, 0.05, 2, 0)"), 10.0, 1e-9);
    fails("DURATION(DATE(2018,1,1), DATE(2018,1,1), 0.08, 0.09, 2, 1)");
    fails("DURATION(DATE(2018,1,1), DATE(2028,1,1), -0.08, 0.09, 2, 1)");
    fails("MDURATION(DATE(2018,1,1), DATE(2028,1,1), 0.08, -0.09, 2, 1)");
    fails("MDURATION(DATE(2018,1,1), DATE(2028,1,1), 0.08, 0.09, 2, 6)");
}

// --- odd periods -----------------------------------------------------------------------------

#[test]
fn odd_last_period_price_and_yield() {
    close(value("ODDLPRICE(DATE(2008,2,7), DATE(2008,6,15), DATE(2007,10,15), 0.0375, 0.0405, 100, 2, 0)"), 99.8782860147213, 1e-9);
    close(value("ODDLYIELD(DATE(2008,4,20), DATE(2008,6,15), DATE(2007,12,24), 0.0375, 99.875, 100, 2, 0)"), 0.0451922356291692, 1e-9);
    // The two functions invert each other for the same cash flows.
    let price = value("ODDLPRICE(DATE(2008,2,7), DATE(2008,6,15), DATE(2007,10,15), 0.0375, 0.0405, 100, 2, 0)");
    let source = format!("ODDLYIELD(DATE(2008,2,7), DATE(2008,6,15), DATE(2007,10,15), 0.0375, {price}, 100, 2, 0)");
    close(value(&source), 0.0405, 1e-10);
    fails("ODDLPRICE(DATE(2008,6,15), DATE(2008,6,15), DATE(2007,10,15), 0.0375, 0.0405, 100, 2, 0)");
    fails("ODDLPRICE(DATE(2008,2,7), DATE(2008,6,15), DATE(2008,5,15), 0.0375, 0.0405, 100, 2, 0)");
    fails("ODDLPRICE(DATE(2008,2,7), DATE(2008,6,15), DATE(2007,10,15), -0.0375, 0.0405, 100, 2, 0)");
    fails("ODDLYIELD(DATE(2008,4,20), DATE(2008,6,15), DATE(2007,12,24), 0.0375, 0, 100, 2, 0)");
    fails("ODDLYIELD(DATE(2008,4,20), DATE(2008,6,15), DATE(2007,12,24), 0.0375, 99.875, 100, 4, 9)");
}

#[test]
fn odd_first_period_price_and_yield() {
    close(value("ODDFPRICE(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0785, 0.0625, 100, 2, 1)"), 113.597717474079, 1e-9);
    close(value("ODDFYIELD(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0575, 84.5, 100, 2, 0)"), 0.0772455415972989, 1e-9);
    // The short first stub accrues from the issue date, so a 30/360 hand calculation: the odd
    // coupon is 137/181 of a coupon and the accrued interest is 27/181.
    let price = value("ODDFPRICE(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0785, 0.0625, 100, 2, 1)");
    let source = format!("ODDFYIELD(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0785, {price}, 100, 2, 1)");
    close(value(&source), 0.0625, 1e-10);
    fails("ODDFPRICE(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0785, 0.0625, 100, 2, 7)");
    fails("ODDFPRICE(DATE(2008,11,11), DATE(2009,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0785, 0.0625, 100, 2, 1)");
    fails("ODDFPRICE(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,11,11), DATE(2009,3,1), 0.0785, 0.0625, 100, 2, 1)");
    fails("ODDFYIELD(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0575, 0, 100, 2, 0)");
    fails("ODDFYIELD(DATE(2008,11,11), DATE(2021,3,1), DATE(2008,10,15), DATE(2009,3,1), 0.0575, 84.5, 100, 3, 0)");
}
