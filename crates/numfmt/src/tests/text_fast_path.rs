//! The TEXT fast path for the one-digit code `0` against the renderer.

use super::*;

#[test]
fn text_through_the_one_digit_code_says_what_the_renderer_says() {
    // `TEXT(n, "0")` on an integer skips the renderer; both must agree, edges included.
    let fmt = NumberFormat::parse("0");
    let mut values: Vec<f64> = (-100_000..=100_000).map(f64::from).collect();
    for e in 0..=16 {
        let p = 10f64.powi(e);
        values.extend([p - 1.0, p, p + 1.0, -(p - 1.0), -p, -(p + 1.0)]);
    }
    values.extend([999_999_999_999_999.0, -999_999_999_999_999.0, 0.0, -0.0, 2f64.powi(53)]);
    // Fractions round through the renderer; the fast path must leave them to it.
    values.extend([0.5, 1.5, 2.5, -0.5, -1.5, 0.4999, 1e14 + 0.5, 123_456.789]);
    for n in values {
        let (f, overflow) = fmt.format_number(n, S);
        let rendered = if overflow { Err(CellError::Value) } else { Ok(f.text) };
        assert_eq!(text_function(&Value::Number(n), "0", S), rendered, "TEXT({n}, \"0\")");
    }
}
