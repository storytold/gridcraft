//! Financial intermediates with binary64 precision and a separate binary exponent.
//!
//! A coupon, discount power or present value may be outside binary64 even when its final
//! price or weighted ratio fits. Keep its scale until that final conversion. This extends
//! range, not precision; it does not repair ill-conditioned subtraction of nearby amounts.

use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Sub};

#[derive(Clone, Copy, Debug)]
pub(super) struct Scaled {
    mantissa: f64,
    exponent: i32,
}

impl From<f64> for Scaled {
    fn from(value: f64) -> Self {
        if value == 0.0 || !value.is_finite() {
            return Self { mantissa: value, exponent: 0 };
        }
        // Normalize subnormals before extracting their exponent. No intermediate rounds.
        let (value, adjustment) = if value.abs() < f64::MIN_POSITIVE { (value * 2.0_f64.powi(52), -52) } else { (value, 0) };
        let bits = value.to_bits();
        Self {
            mantissa: f64::from_bits((bits & ((1_u64 << 63) | ((1_u64 << 52) - 1))) | (1023_u64 << 52)),
            exponent: ((bits >> 52) & 0x7ff) as i32 - 1023 + adjustment,
        }
    }
}

impl Scaled {
    fn normalized(mantissa: f64, exponent: i32) -> Self {
        let value = Self::from(mantissa);
        Self { exponent: value.exponent.saturating_add(exponent), ..value }
    }

    pub(super) fn exp(log_value: f64) -> Self {
        let value = log_value.exp();
        if value.is_normal() {
            return Self::from(value);
        }
        let power = log_value * std::f64::consts::LOG2_E;
        if !power.is_finite() {
            return Self::from(value);
        }
        let whole = power.floor();
        Self::normalized((power - whole).exp2(), whole as i32)
    }

    pub(super) fn value(self) -> f64 {
        if self.mantissa == 0.0 || !self.mantissa.is_finite() {
            return self.mantissa;
        }
        if self.exponent < -1022 {
            // Construct a subnormal only at the final rounding, including half-ULP cases.
            (self.mantissa * f64::MIN_POSITIVE) * 2.0_f64.powi(self.exponent.saturating_add(1022))
        } else {
            self.mantissa * 2.0_f64.powi(self.exponent)
        }
    }

    pub(super) fn compare(self, other: Self) -> Ordering {
        let difference = (self - other).mantissa;
        if difference == 0.0 { Ordering::Equal } else { difference.total_cmp(&0.0) }
    }
}

impl Add for Scaled {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        if self.mantissa == 0.0 {
            return rhs;
        }
        if rhs.mantissa == 0.0 {
            return self;
        }
        let (large, small) = if self.exponent >= rhs.exponent { (self, rhs) } else { (rhs, self) };
        Self::normalized(large.mantissa + small.mantissa * 2.0_f64.powi(small.exponent.saturating_sub(large.exponent)), large.exponent)
    }
}

impl Sub for Scaled {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self + Self { mantissa: -rhs.mantissa, ..rhs }
    }
}

impl Mul for Scaled {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::normalized(self.mantissa * rhs.mantissa, self.exponent.saturating_add(rhs.exponent))
    }
}

impl Div for Scaled {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self::normalized(self.mantissa / rhs.mantissa, self.exponent.saturating_sub(rhs.exponent))
    }
}

#[cfg(test)]
mod tests {
    use super::Scaled;

    #[test]
    fn final_conversion_preserves_extreme_values_after_scale_cancellation() {
        for value in [f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1), -f64::MAX] {
            let wide = Scaled::from(value) * Scaled::from(f64::MAX);
            assert_eq!((wide / Scaled::from(f64::MAX)).value(), value);
        }
        let large_sum = Scaled::from(9e307) + Scaled::from(9e307);
        assert_eq!((large_sum / Scaled::from(2.0)).value(), 9e307);
    }

    #[test]
    fn discounted_values_can_cancel_their_exponents_without_underflow() {
        let ratio = Scaled::exp(-1000.0) / Scaled::exp(-1000.0);
        assert_eq!(ratio.value(), 1.0);
        let weighted = Scaled::exp(-1000.0) * Scaled::from(100.0);
        assert!(((weighted / Scaled::exp(-1000.0)).value() / 100.0 - 1.0).abs() < 1e-15);
    }
}
