//! Integration tests for the loan, depreciation and irregular cash-flow financial functions.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    DataModel {
        tables: vec![
            ModelTable {
                name: "CashFlows".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![Scalar::Decimal(-10000.0), Scalar::DateTime(43831.0)], // 2020-01-01
                    vec![Scalar::Decimal(2750.0), Scalar::DateTime(43922.0)],   // 2020-04-01
                    vec![Scalar::Decimal(4250.0), Scalar::DateTime(44119.0)],   // 2020-10-15
                    vec![Scalar::Decimal(3250.0), Scalar::DateTime(44197.0)],   // 2021-01-01
                ],
            },
            ModelTable {
                name: "LateCash".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![Scalar::Decimal(100.0), Scalar::DateTime(44197.0)],  // 2021-01-01
                    vec![Scalar::Decimal(-100.0), Scalar::DateTime(43831.0)], // 2020-01-01 (before anchor)
                ],
            },
        ],
        relationships: Vec::new(),
        measures: Vec::new(),
    }
}

fn eval(source: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.message.clone())?;
    let expr = parse_expression(source).map_err(|error| error.message.clone())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.message.clone())?;
    evaluator.evaluate(&bound).map_err(|error| error.message.clone())
}

fn number(source: &str) -> f64 {
    match eval(source) {
        Ok(Scalar::Decimal(value)) => value,
        Ok(Scalar::Integer(value)) => value as f64,
        Ok(other) => panic!("expected a number from `{source}`, got {other:?}"),
        Err(error) => panic!("`{source}` failed: {error}"),
    }
}

fn assert_close(actual: f64, expected: f64, label: &str) {
    let tolerance = 1e-9 * expected.abs().max(1.0);
    assert!((actual - expected).abs() <= tolerance, "{label}: expected {expected}, got {actual}");
}

fn assert_error(source: &str) {
    match eval(source) {
        Err(_) => {}
        Ok(value) => panic!("expected an error from `{source}`, got {value:?}"),
    }
}

#[test]
fn payment_and_present_future_values() {
    // Closed forms: pmt = -(r*(pv*(1+r)^n + fv)) / ((1+r*type)*((1+r)^n - 1)).
    assert_close(number("PMT(0.05/12, 60, -10000)"), 188.7123364401099, "PMT");
    assert_close(number("PMT(0.08/12, 10, -10000, 0, 1)"), 1030.1643271779772, "PMT type=1");
    assert_close(number("FV(0.06/12, 10, -200, -500, 1)"), 2581.4033740601362, "FV");
    assert_close(number("FV(0.05, 10, -100, -1000)"), 2886.683880332326, "FV arrear");
    assert_close(number("PV(0.08/12, 12*15, -1500)"), 156960.88823487246, "PV");
    assert_close(number("PV(0.06/12, 10, -200, 0, 1)"), 1955.8127840179968, "PV type=1");
    assert_close(number("NPER(0.01, -100, 1000)"), 10.588644459423241, "NPER");
    assert_close(number("NPER(0, -100, 1000)"), 10.0, "NPER zero rate");
    assert_close(number("RATE(60, -200, 8000)"), 0.014394781000913973, "RATE");
    assert_error("PMT(-1, 10, 1000)");
    assert_error("FV(-1.5, 10, -100)");
    assert_error("NPER(0.01, 100, -20000)");
    assert_error("RATE(10, 100, 1000)");
}

#[test]
fn interest_and_principal_schedules() {
    assert_close(number("IPMT(0.1/12, 1, 36, 8000)"), -66.66666666666667, "IPMT first");
    assert_close(number("IPMT(0.1/12, 2, 36, 8000)"), -65.07107640929972, "IPMT second");
    assert_close(number("PPMT(0.1/12, 2, 36, 8000)"), -193.0664211414009, "PPMT second");
    assert_close(number("IPMT(0.1/12, 1, 36, 8000, 0, 1)"), 0.0, "IPMT advance first");
    assert_close(number("CUMPRINC(0.09/12, 360, 125000, 13, 24, 0)"), -934.1071234208848, "CUMPRINC");
    assert_error("IPMT(0.1/12, 0, 36, 8000)");
    assert_error("IPMT(0.1/12, 37, 36, 8000)");
    assert_error("CUMIPMT(0.09/12, 360, 125000, 0, 24, 0)");
    assert_error("CUMIPMT(0.09/12, 360, 125000, 13, 361, 0)");
    assert_error("CUMPRINC(0, 360, 125000, 13, 24, 0)");
}

#[test]
fn cumulative_schedule_sums_match_per_period_interest() {
    let expected_interest: f64 = (13..=24).map(|period| number(&format!("IPMT(0.09/12, {period}, 360, 125000)"))).sum();
    assert_close(number("CUMIPMT(0.09/12, 360, 125000, 13, 24, 0)"), expected_interest, "CUMIPMT sum");
    assert_close(number("CUMIPMT(0.09/12, 360, 125000, 13, 24, 0)"), -11135.232130750835, "CUMIPMT closed form");

    let expected_principal: f64 = (1..=4).map(|period| number(&format!("PPMT(0.1/12, {period}, 36, 8000, 0, 1)"))).sum();
    assert_close(number("CUMPRINC(0.1/12, 36, 8000, 1, 4, 1)"), expected_principal, "CUMPRINC advance sum");
}

#[test]
fn simple_interest_and_rates() {
    assert_close(number("ISPMT(0.1/12, 1, 36, 8000000)"), -64814.81481481482, "ISPMT");
    assert_close(number("EFFECT(0.0525, 4)"), 0.05354266737075819, "EFFECT");
    assert_close(number("NOMINAL(0.1, 4)"), 0.09645475633778045, "NOMINAL");
    assert_close(number("PDURATION(0.025, 2000, 2200)"), 3.859866162622648, "PDURATION");
    assert_close(number("RRI(96, 10000, 11000)"), 0.0009933073762913303, "RRI");
    assert_close(number("RRI(10, 1000, 0)"), -1.0, "RRI to zero");
    assert_error("EFFECT(0.1, 0)");
    assert_error("NOMINAL(0, 4)");
    assert_error("PDURATION(0, 2000, 2200)");
    assert_error("RRI(0, 1000, 1100)");
    assert_error("RRI(10, 0, 1100)");
}

#[test]
fn fractional_dollar_prices() {
    assert_close(number("DOLLARDE(1.02, 16)"), 1.125, "DOLLARDE 16ths");
    assert_close(number("DOLLARDE(1.1, 32)"), 1.3125, "DOLLARDE 32nds");
    assert_close(number("DOLLARDE(-1.02, 16)"), -1.125, "DOLLARDE negative");
    assert_close(number("DOLLARFR(1.125, 16)"), 1.02, "DOLLARFR 16ths");
    assert_close(number("DOLLARFR(1.125, 32)"), 1.04, "DOLLARFR 32nds");
    assert_error("DOLLARDE(1.02, 0)");
    assert_error("DOLLARFR(1.125, -2)");
}

#[test]
fn straight_line_and_sum_of_years_depreciation() {
    assert_close(number("SLN(30000, 7500, 10)"), 2250.0, "SLN");
    assert_close(number("SYD(30000, 7500, 10, 1)"), 4090.909090909091, "SYD first");
    assert_close(number("SYD(30000, 7500, 10, 10)"), 409.09090909090907, "SYD last");
    assert_error("SLN(30000, 7500, 0)");
    assert_error("SYD(30000, 7500, 10, 11)");
}

#[test]
fn declining_balance_depreciation() {
    assert_close(number("DB(1000000, 100000, 6, 1, 7)"), 186083.33333333334, "DB first");
    assert_close(number("DB(1000000, 100000, 6, 7, 7)"), 15845.098473848071, "DB final partial");
    assert_close(number("DDB(2400, 300, 10, 1)"), 480.0, "DDB first");
    assert_close(number("DDB(2400, 300, 10, 2)"), 384.0, "DDB second");
    assert_close(number("VDB(2400, 300, 10, 0, 1)"), 480.0, "VDB first year");
    assert_close(number("VDB(2400, 300, 10, 0, 0.875, 1.5)"), 315.0, "VDB partial with factor");
    assert_close(number("VDB(2400, 300, 10, 0, 10)"), 2100.0, "VDB full life");
    assert_close(number("VDB(2400, 300, 10, 6, 10)"), 329.1456000000003, "VDB partial range");
    assert_close(number("VDB(2400, 300, 10, 0.5, 1.5)"), 432.0, "VDB fractional bounds");
    assert_error("DB(1000000, 100000, 6, 9)");
    assert_error("DDB(2400, 300, 10, 11)");
    assert_error("VDB(2400, 300, 10, 0, 11)");
}

#[test]
fn french_accounting_depreciation() {
    assert_close(number("AMORDEGRC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 1, 0.15, 1)"), 776.0, "AMORDEGRC period 1");
    assert_close(number("AMORDEGRC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 0, 0.15, 1)"), 330.0, "AMORDEGRC period 0");
    assert_close(number("AMORDEGRC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 2, 0.15, 1)"), 485.0, "AMORDEGRC period 2");
    assert_close(number("AMORLINC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 0, 0.15, 1)"), 131.8032786885246, "AMORLINC prorated");
    assert_close(number("AMORLINC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 1, 0.15, 1)"), 360.0, "AMORLINC full period");
    assert_close(number("AMORLINC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 0, 0.15, 0)"), 132.0, "AMORLINC 30/360");
    assert_error("AMORDEGRC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 0, 0.22, 0)");
    assert_error("AMORLINC(2400, DATE(2008,12,31), DATE(2008,8,19), 300, 0, 0.15, 0)");
    assert_error("AMORLINC(2400, DATE(2008,8,19), DATE(2008,12,31), 300, 0, 0.15, 2)");
}

#[test]
fn amordegrc_huge_period_terminates() {
    // Once the rounded depreciation reaches zero the loop must stop rather than run `period`
    // times; a user-supplied 10^15 period used to hang the evaluator.
    let value = number("AMORDEGRC(2400, DATE(2008,8,19), DATE(2008,12,31), 0, 1000000000000000, 0.15, 1)");
    assert_eq!(value, 0.0);
}

#[test]
fn irregular_cash_flows() {
    assert_close(number("XNPV(CashFlows, CashFlows[Amount], CashFlows[Date], 0.09)"), -356.8911834998921, "XNPV");
    assert_close(number("XIRR(CashFlows, CashFlows[Amount], CashFlows[Date])"), 0.035365618646134966, "XIRR");
    assert_close(
        number("XIRR(FILTER(CashFlows, CashFlows[Amount] < 0 || CashFlows[Amount] >= 2750), CashFlows[Amount], CashFlows[Date])"),
        0.035365618646134966,
        "XIRR filtered",
    );
    assert_error("XIRR(FILTER(CashFlows, CashFlows[Amount] > 100000), CashFlows[Amount], CashFlows[Date])");
    assert_error("XIRR(LateCash, LateCash[Amount], LateCash[Date])");
    assert_error("XIRR(FILTER(CashFlows, CashFlows[Amount] > 0), CashFlows[Amount], CashFlows[Date])");
    assert_error("XNPV(CashFlows, CashFlows[Amount], CashFlows[Date], -1)");
    assert_error("XNPV(FILTER(CashFlows, CashFlows[Amount] > 100000), CashFlows[Amount], CashFlows[Date], 0.09)");
    assert_eq!(
        number("XIRR(FILTER(CashFlows, CashFlows[Amount] > 0), CashFlows[Amount], CashFlows[Date], 0.1, -99)"),
        -99.0,
        "XIRR alternate result"
    );
}
