//! Arithmetic that follows Excel's bits rather than IEEE's: the snap to zero.

/// 2^-49: the snap threshold is this fraction of the left operand's binade.
const SNAP: f64 = 1.0 / 562_949_953_421_312.0;

/// The power of two at the bottom of `x`'s binade (`x` with its mantissa cleared); 0 for zero,
/// subnormal and non-finite values.
fn binade(x: f64) -> f64 {
    if x.is_finite() { f64::from_bits(x.to_bits() & 0x7FF0_0000_0000_0000) } else { 0.0 }
}

/// `x + y` as Excel computes it at the root of a formula or as the last addition of a `SUM`:
/// when the operands have opposite signs and `|x + y| < 2^(e-49)`, `e` the binary exponent of
/// the LEFT operand `x`, the result is exactly 0. A subtraction passes `-y`.
///
/// Excel does this only at those places, not at every add or subtract (see
/// `EXCEL-BEHAVIOUR.md` of the Spreadsheet Accuracy Suite, "The snap to zero").
pub fn snap_add(x: f64, y: f64) -> f64 {
    let s = x + y;
    let opposite = (x > 0.0 && y < 0.0) || (x < 0.0 && y > 0.0);
    if opposite && s.abs() < binade(x) * SNAP { 0.0 } else { s }
}

/// The sum of `values` left to right, each addition rounded, with the LAST addition snapped.
/// This is `SUM`'s arithmetic: `SUM(a, -b)` is 0 for a near-cancellation, but `SUM(a, -b, 0)` keeps
/// the residue because its last addition is residue + 0.
pub fn sum_snapped(values: &[f64]) -> f64 {
    let Some((last, head)) = values.split_last() else { return 0.0 };
    snap_add(head.iter().fold(0.0, |acc, v| acc + v), *last)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ULP1: f64 = f64::EPSILON;

    #[test]
    fn snaps_below_two_to_the_e_minus_49_of_the_left_operand() {
        assert_eq!(snap_add(1.0, -(1.0 + ULP1)), 0.0);
        assert_eq!(snap_add(1.0, -(1.0 + 7.0 * ULP1)), 0.0);
        // Exactly 2^-49 (8 ulps of 1.0) does not snap.
        assert_eq!(snap_add(1.0, -(1.0 + 8.0 * ULP1)), -8.0 * ULP1);
        // The left operand's binade decides (Excel observed, request 4, excel-probe-4.00044 to 00046):
        // 0.4999999999999998 - 0.5000000000000002 keeps -4.44e-16 (threshold 2^-51 is not beaten),
        // the other way round it is 0 (threshold 2^-50).
        assert_eq!(snap_add(0.4999999999999998, -0.5000000000000002), -4.440892098500626e-16);
        assert_eq!(snap_add(0.5000000000000002, -0.4999999999999998), 0.0);
    }

    #[test]
    fn never_snaps_same_sign_zero_or_hostile_operands() {
        assert_eq!(snap_add(1e-20, 2e-20), 1e-20 + 2e-20);
        assert_eq!(snap_add(3.0, 0.0), 3.0);
        assert_eq!(snap_add(0.0, -1e-300), -1e-300);
        // A subnormal left operand has no binade to measure against: it never snaps.
        assert_eq!(snap_add(5e-324, -1e-323), -5e-324);
        assert!(snap_add(f64::INFINITY, -f64::INFINITY).is_nan());
        assert_eq!(snap_add(-1.0, 1.0 + ULP1), 0.0);
    }

    #[test]
    fn sum_snaps_only_its_last_addition() {
        assert_eq!(sum_snapped(&[]), 0.0);
        assert_eq!(sum_snapped(&[1.0, -(1.0 + ULP1)]), 0.0);
        assert_eq!(sum_snapped(&[1.0, -(1.0 + ULP1), 0.0]), 1.0 - (1.0 + ULP1));
        assert_eq!(sum_snapped(&[3.0, 1e16, -1e16, 0.0]), 4.0);
        // Left to right, each addition rounded: ten 0.1s do not make 1.
        assert_eq!(sum_snapped(&[0.1; 10]), 0.999_999_999_999_999_9);
    }
}
