use crate::util::testutil::{TestCtx, n};
use gridcraft_core::{DateSystem, Value, date::serial_from_ymd};

fn check(name: &str, sys: DateSystem, settlement: (i64, i64, i64), maturity: (i64, i64, i64), tail: &[f64], expected: f64) {
    let date = |(y, m, d)| n(serial_from_ymd(sys, y, m, d).unwrap());
    let args: Vec<_> = [date(settlement), date(maturity)].into_iter().chain(tail.iter().copied().map(n)).collect();
    let result = crate::call(crate::lookup(name).unwrap(), &args, &mut TestCtx { sys, ..TestCtx::default() });
    let Value::Number(actual) = result else { panic!("{name} {tail:?}: expected {expected}, got {result:?}") };
    if expected == 0.0 {
        assert!(actual.abs() < 1e-12, "{name}: {actual} != 0");
    } else {
        assert!((actual / expected - 1.0).abs() < 1e-10, "{name}: {actual} != {expected}");
    }
}

#[test]
fn zero_coupon_yield_does_not_evaluate_zero_over_underflowed_power() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("YIELD", sys, (2025, 1, 1), (2079, 1, 1), &[0.0, 100.0, 100.0, 1.0, 4.0], 0.0);
        check("YIELD", sys, (2025, 1, 1), (2039, 1, 1), &[0.0, 100.0, 100.0, 4.0, 4.0], 0.0);
    }
}

#[test]
fn bond_price_scales_accrued_interest_before_multiplication() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("PRICE", sys, (2025, 1, 1), (2025, 4, 1), &[1e305, 0.04, 100.0, 1.0, 4.0], 2.400990099009901e306);
    }
}

#[test]
fn bond_duration_cancels_large_cash_flow_scale() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("DURATION", sys, (2025, 1, 1), (2025, 4, 1), &[1e307, 0.04, 1.0, 4.0], 0.25);
        check("MDURATION", sys, (2025, 1, 1), (2025, 4, 1), &[1e307, 0.04, 1.0, 4.0], 0.25 / 1.04);
        check("DURATION", sys, (2025, 1, 1), (2027, 1, 1), &[1e306, 0.0, 1.0, 4.0], 1.5);
    }
}

#[test]
fn final_range_and_negative_domain_guards_are_retained() {
    use crate::util::testutil::{ev, is_err};
    use gridcraft_core::CellError;
    let s = n(serial_from_ymd(DateSystem::D1900, 2025, 1, 1).unwrap());
    let m = n(serial_from_ymd(DateSystem::D1900, 2027, 1, 1).unwrap());
    is_err(ev("PRICE", vec![s.clone(), m.clone(), n(9e307), n(0.0), n(100.0), n(1.0), n(4.0)]), CellError::Num);
    // The separate negative-yield policy is unchanged by widening the positive search.
    is_err(ev("YIELD", vec![s.clone(), m.clone(), n(0.0), n(100.0), n(1e-20), n(1.0), n(4.0)]), CellError::Num);
    is_err(ev("PRICE", vec![s, m, n(0.05), n(-0.01), n(100.0), n(1.0), n(4.0)]), CellError::Num);
}

#[test]
fn bond_duration_cancels_tiny_present_value_scale() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("DURATION", sys, (2025, 1, 1), (2125, 1, 1), &[0.0, 1e100, 1.0, 4.0], 100.0);
        check("MDURATION", sys, (2025, 1, 1), (2125, 1, 1), &[0.0, 1e100, 1.0, 4.0], 1e-98);
    }
}

#[test]
fn bond_yield_search_covers_representable_positive_roots() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("YIELD", sys, (2025, 1, 1), (2027, 1, 1), &[0.0, 1e-40, 100.0, 1.0, 4.0], 1e21);
        check("YIELD", sys, (2025, 1, 1), (2027, 1, 1), &[0.0, 1e-100, 100.0, 1.0, 4.0], 1e51);
    }
}

#[test]
fn bond_price_keeps_representable_redemption_when_power_overflows() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("PRICE", sys, (2025, 1, 1), (2027, 1, 1), &[0.0, 2e154, 1e307, 1.0, 4.0], 0.025);
        check("PRICE", sys, (2025, 1, 1), (2027, 1, 1), &[0.0, 1e160, 1e300, 1.0, 4.0], 1e-20);
    }
}

#[test]
fn single_coupon_yield_scales_days_before_multiplication() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("YIELD", sys, (2025, 1, 1), (2026, 1, 1), &[0.0, 1e-300, 1e6, 1.0, 4.0], 1e306);
        check("YIELD", sys, (2025, 1, 1), (2026, 1, 1), &[1e306, 100.0, 100.0, 1.0, 4.0], 1e306);
    }
}

#[test]
fn bond_coupon_construction_keeps_frequency_and_discount_scaling() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        check("PRICE", sys, (2025, 1, 1), (2025, 4, 1), &[2e306, 0.04, 100.0, 4.0, 4.0], 4.950495049504951e307);
        check("PRICE", sys, (2025, 1, 1), (2026, 1, 1), &[1e307, 100.0, 100.0, 1.0, 4.0], 9.900990099009901e306);
    }
}
