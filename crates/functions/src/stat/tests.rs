use super::{MAX_REGRESSORS, aggregate_values, aggregate_values_k, check_regression_size};
use crate::util::testutil::*;
use gridcraft_core::{CellError, Value};

const D7: f64 = 1e-6;

fn c(xs: &[f64]) -> crate::Arg {
    av(col(xs))
}

// The two middle values are finite and their midpoint is representable even when
// adding them first would overflow. Exercise both callers of the shared helper.
#[test]
fn median_finite_midpoint_does_not_overflow() {
    for (values, expected) in [
        (vec![1e308, 1e308], 1e308),
        (vec![-1e308, -1e308], -1e308),
        (vec![1.6e308, 1e308], 1.3e308),
        (vec![-1e308, -1.6e308], -1.3e308),
        (vec![f64::MAX, f64::MAX], f64::MAX),
        (vec![-f64::MAX, -f64::MAX], -f64::MAX),
        (vec![-f64::MAX, f64::MAX], 0.0),
        (vec![1.7e308, -1.7e308, 1e308, 1.6e308], 1.3e308),
    ] {
        close(ev("MEDIAN", values.iter().copied().map(n).collect()), expected);
        close(ev("MEDIAN", vec![rf(col(&values))]), expected);
        close(aggregate_values(12, &values.iter().copied().map(nv).collect::<Vec<_>>()), expected);
    }
}

#[test]
fn median_midpoint_keeps_small_values_and_argument_semantics() {
    // Halving each operand unconditionally can lose the smallest numbers. Use exact
    // equality here: the usual aggregate-test tolerance would accept zero.
    for value in [f64::MIN_POSITIVE, -f64::MIN_POSITIVE, f64::from_bits(1), -f64::from_bits(1)] {
        assert_eq!(ev("MEDIAN", vec![n(value), n(value)]), nv(value));
        assert_eq!(aggregate_values(12, &[nv(value), nv(value)]), nv(value));
    }
    close(ev("MEDIAN", vec![c(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])]), 3.5);
    close(ev("MEDIAN", vec![c(&[1e308, -1e308, 3.0])]), 3.0);
    close(ev("MEDIAN", vec![n(1e308)]), 1e308);
    close(ev("MEDIAN", vec![t("4"), b(true)]), 2.5);
    close(ev("MEDIAN", vec![rf(arr(vec![vec![nv(1.0), tv("ignored"), Value::Empty, Value::Bool(true), nv(6.0)]]))]), 3.5);
    is_err(ev("MEDIAN", vec![t("invalid")]), CellError::Value);
    is_err(ev("MEDIAN", vec![e(CellError::NA), n(1e308)]), CellError::NA);
    is_err(ev("MEDIAN", vec![rf(arr(vec![vec![tv("ignored"), Value::Empty]]))]), CellError::Num);
    is_err(aggregate_values(12, &[]), CellError::Num);
    is_err(aggregate_values(12, &[nv(1e308), Value::Error(CellError::Div0)]), CellError::Div0);
}

#[test]
fn averages_and_counts() {
    close(ev("AVERAGE", vec![n(1.0), n(2.0), n(6.0)]), 3.0);
    is_err(ev("AVERAGE", vec![rf(arr(vec![vec![tv("a")]]))]), CellError::Div0);
    let mixed = arr(vec![vec![nv(1.0), tv("x"), Value::Bool(true), Value::Empty, nv(3.0)]]);
    close(ev("AVERAGE", vec![rf(mixed.clone())]), 2.0);
    close(ev("AVERAGEA", vec![rf(mixed.clone())]), 1.25);
    close(ev("AVERAGE", vec![t("4"), b(true)]), 2.5);
    is_err(ev("AVERAGE", vec![t("abc")]), CellError::Value);
    is_err(ev("AVERAGE", vec![rf(arr(vec![vec![nv(1.0), Value::Error(CellError::NA)]]))]), CellError::NA);
    close(ev("COUNT", vec![rf(mixed.clone()), n(4.0), t("5"), t("x")]), 4.0);
    close(ev("COUNT", vec![e(CellError::Div0)]), 0.0);
    close(ev("COUNTA", vec![rf(mixed.clone())]), 4.0);
    close(ev("COUNTA", vec![n(1.0), t("")]), 2.0);
    let blanks = arr(vec![vec![Value::Empty, tv(""), nv(0.0), tv(" ")]]);
    close(ev("COUNTBLANK", vec![rf(blanks)]), 2.0);
    close(ev("MEDIAN", vec![c(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])]), 3.5);
    close(ev("MEDIAN", vec![c(&[3.0, 1.0, 2.0])]), 2.0);
    is_err(ev("MEDIAN", vec![rf(arr(vec![vec![tv("a")]]))]), CellError::Num);
    close(ev("MODE.SNGL", vec![c(&[5.6, 4.0, 4.0, 3.0, 2.0, 4.0])]), 4.0);
    close(ev("MODE", vec![c(&[1.0, 2.0, 2.0, 1.0])]), 1.0);
    is_err(ev("MODE", vec![c(&[1.0, 2.0, 3.0])]), CellError::NA);
    assert_eq!(rows_of(&ev("MODE.MULT", vec![c(&[1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 2.0, 3.0])])), vec![vec![nv(2.0)], vec![nv(3.0)]]);
}

/// MODE argument semantics in Excel 365, on the inputs of FormulaEvalTestData (row "Mode"):
/// text is skipped, typed or referenced, but a single-cell reference to a blank cell is
/// `#VALUE!`; the first error from the left wins. The values cached in POI's file come from an
/// older Excel and say `#VALUE!` for `MODE(C9,E16,"1",B8)`, `MODE(B8,"1",2,3)` and
/// `MODE("1",1,#DIV/0!,2,3)`; Excel 365 gives `#N/A`, `#N/A` and `#DIV/0!`.
#[test]
fn mode_scalar_arguments() {
    let data = arr(vec![vec![tv(""), nv(0.0), nv(1.0)], vec![tv("x"), nv(1.0), nv(1.001)], vec![tv("y"), nv(9e9), nv(2.3456)]]);
    let cases: Vec<(Vec<crate::Arg>, Result<f64, CellError>)> = vec![
        (vec![n(1.0), n(1.0), n(2.0), n(3.0)], Ok(1.0)),
        (vec![rf(data)], Ok(1.0)),
        (vec![rf(arr(vec![vec![nv(-1.1), Value::Empty, tv("1.1")]])), n(-1.1)], Ok(-1.1)),
        // MODE(C9,E16,"1",B8): E16 holds the text "1.1".
        (vec![rf(nv(1.1)), rf(tv("1.1")), t("1"), rf(nv(1.0))], Err(CellError::NA)),
        (vec![rf(nv(1.0)), t("1"), n(2.0), n(3.0)], Err(CellError::NA)),
        (vec![t("1"), n(1.0), e(CellError::Div0), n(2.0), n(3.0)], Err(CellError::Div0)),
        // MODE(B9,2,D7) with D7 blank.
        (vec![rf(nv(2.0)), n(2.0), rf(Value::Empty)], Err(CellError::Value)),
        (vec![n(1.0), n(1.0), e(CellError::Div0), t("1")], Err(CellError::Div0)),
    ];
    for name in ["MODE", "MODE.SNGL"] {
        for (args, want) in &cases {
            match want {
                Ok(x) => close(ev(name, args.clone()), *x),
                Err(err) => is_err(ev(name, args.clone()), *err),
            }
        }
    }
    is_err(ev("MODE.MULT", vec![rf(nv(1.0)), t("1"), rf(Value::Empty)]), CellError::Value);
}

/// Text and logical values typed as arguments are skipped, not coerced (Excel 365, #736).
#[test]
fn mode_skips_typed_text_and_logicals() {
    let cases: Vec<(Vec<crate::Arg>, Result<f64, CellError>)> = vec![
        (vec![n(1.0), n(1.0), t("x")], Ok(1.0)),
        (vec![n(1.0), n(2.0), t("x")], Err(CellError::NA)),
        (vec![n(1.0), t("1"), n(2.0), n(3.0)], Err(CellError::NA)),
        (vec![t("1"), n(1.0), n(2.0), n(3.0)], Err(CellError::NA)),
        (vec![t("1"), n(1.0), e(CellError::Div0), n(2.0), n(3.0)], Err(CellError::Div0)),
        (vec![n(1.0), b(true), b(true), n(2.0)], Err(CellError::NA)),
        (vec![n(1.0), n(1.0), b(true)], Ok(1.0)),
    ];
    for name in ["MODE", "MODE.SNGL"] {
        for (args, want) in &cases {
            match want {
                Ok(x) => close(ev(name, args.clone()), *x),
                Err(err) => is_err(ev(name, args.clone()), *err),
            }
        }
    }
    assert_eq!(rows_of(&ev("MODE.MULT", vec![n(1.0), t("1"), n(1.0), t("x")])), vec![vec![nv(1.0)]]);
    is_err(ev("MODE.MULT", vec![n(1.0), b(true), b(true), n(2.0)]), CellError::NA);
}

#[test]
fn spread() {
    let d = [1345.0, 1301.0, 1368.0, 1322.0, 1310.0, 1370.0, 1318.0, 1350.0, 1303.0, 1299.0];
    close_tol(ev("STDEV.S", vec![c(&d)]), 27.46391572, 1e-9);
    close_tol(ev("STDEV", vec![c(&d)]), 27.46391572, 1e-9);
    close_tol(ev("STDEV.P", vec![c(&d)]), 26.05455814, 1e-9);
    close_tol(ev("STDEVP", vec![c(&d)]), 26.05455814, 1e-9);
    close_tol(ev("VAR.S", vec![c(&d)]), 754.2666667, 1e-9);
    close(ev("VAR.P", vec![c(&d)]), 678.84);
    close(ev("VARP", vec![c(&d)]), 678.84);
    is_err(ev("STDEV.S", vec![n(1.0)]), CellError::Div0);
    close(ev("STDEV.P", vec![n(1.0)]), 0.0);
    let mixed = arr(vec![vec![nv(2.0), Value::Bool(true), tv("x")]]);
    close(ev("VARA", vec![rf(mixed.clone())]), 1.0);
    close(ev("VARPA", vec![rf(mixed.clone())]), 2.0 / 3.0);
    close(ev("STDEVA", vec![rf(mixed.clone())]), 1.0);
    close(ev("STDEVPA", vec![rf(mixed)]), (2.0f64 / 3.0).sqrt());
    let s = [4.0, 5.0, 6.0, 7.0, 5.0, 4.0, 3.0];
    close(ev("DEVSQ", vec![c(&[4.0, 5.0, 8.0, 7.0, 11.0, 4.0, 3.0])]), 48.0);
    close_tol(ev("AVEDEV", vec![c(&s)]), 1.020408163, 1e-9);
    let g = [4.0, 5.0, 8.0, 7.0, 11.0, 4.0, 3.0];
    close_tol(ev("GEOMEAN", vec![c(&g)]), 5.476986969, 1e-9);
    close_tol(ev("HARMEAN", vec![c(&g)]), 5.028375962, 1e-9);
    is_err(ev("GEOMEAN", vec![c(&[1.0, 0.0])]), CellError::Num);
    close_tol(ev("TRIMMEAN", vec![c(&[4.0, 5.0, 6.0, 7.0, 2.0, 3.0, 4.0, 5.0, 1.0, 2.0, 3.0]), n(0.2)]), 3.777777778, 1e-9);
    is_err(ev("TRIMMEAN", vec![c(&[1.0]), n(1.0)]), CellError::Num);
    let k = [3.0, 4.0, 5.0, 2.0, 3.0, 4.0, 5.0, 6.0, 4.0, 7.0];
    close_tol(ev("KURT", vec![c(&k)]), -0.151799637, 1e-8);
    close_tol(ev("SKEW", vec![c(&k)]), 0.359543071, 1e-8);
    close_tol(ev("SKEW.P", vec![c(&k)]), 0.303193339, 1e-8);
    is_err(ev("KURT", vec![c(&[1.0, 2.0, 3.0])]), CellError::Div0);
    close_tol(ev("STANDARDIZE", vec![n(42.0), n(40.0), n(1.5)]), 1.333333333, 1e-9);
    is_err(ev("STANDARDIZE", vec![n(42.0), n(40.0), n(0.0)]), CellError::Num);
}

#[test]
fn min_max_order() {
    close(ev("MIN", vec![c(&[3.0, -1.0, 2.0]), n(0.0)]), -1.0);
    close(ev("MAX", vec![c(&[3.0, -1.0, 2.0]), n(10.0)]), 10.0);
    close(ev("MIN", vec![]), 0.0);
    close(ev("MAX", vec![rf(arr(vec![vec![tv("a")]]))]), 0.0);
    close(ev("MAXA", vec![rf(arr(vec![vec![nv(-1.0), Value::Bool(true)]]))]), 1.0);
    close(ev("MINA", vec![rf(arr(vec![vec![nv(1.0), tv("x")]]))]), 0.0);
    let d = c(&[3.0, 5.0, 3.0, 5.0, 4.0, 4.0, 2.0, 4.0, 6.0, 7.0]);
    close(ev("LARGE", vec![d.clone(), n(3.0)]), 5.0);
    close(ev("SMALL", vec![d.clone(), n(4.0)]), 4.0);
    is_err(ev("LARGE", vec![d.clone(), n(0.0)]), CellError::Num);
    is_err(ev("SMALL", vec![d.clone(), n(11.0)]), CellError::Num);
    assert_eq!(rows_of(&ev("LARGE", vec![d, av(row(&[1.0, 2.0]))])), vec![vec![nv(7.0), nv(6.0)]]);
    let r = c(&[89.0, 88.0, 92.0, 101.0, 94.0, 97.0, 95.0]);
    close(ev("RANK.AVG", vec![n(94.0), r.clone()]), 4.0);
    close(ev("RANK.EQ", vec![n(101.0), r.clone()]), 1.0);
    is_err(ev("RANK", vec![n(1.0), r]), CellError::NA);
    let r2 = c(&[7.0, 3.5, 3.5, 1.0, 2.0]);
    close(ev("RANK.EQ", vec![n(7.0), r2.clone(), n(1.0)]), 5.0);
    close(ev("RANK.EQ", vec![n(3.5), r2.clone(), n(1.0)]), 3.0);
    close(ev("RANK.AVG", vec![n(3.5), r2, n(1.0)]), 3.5);
}

#[test]
fn percentiles() {
    close(ev("PERCENTILE.INC", vec![c(&[1.0, 3.0, 2.0, 4.0]), n(0.3)]), 1.9);
    close(ev("PERCENTILE", vec![c(&[1.0, 3.0, 2.0, 4.0]), n(1.0)]), 4.0);
    is_err(ev("PERCENTILE.INC", vec![c(&[1.0]), n(1.5)]), CellError::Num);
    let e9 = c(&[1.0, 2.0, 3.0, 6.0, 6.0, 6.0, 7.0, 8.0, 9.0]);
    close(ev("PERCENTILE.EXC", vec![e9.clone(), n(0.25)]), 2.5);
    is_err(ev("PERCENTILE.EXC", vec![e9.clone(), n(0.0)]), CellError::Num);
    is_err(ev("PERCENTILE.EXC", vec![e9.clone(), n(0.01)]), CellError::Num);
    close(ev("QUARTILE.INC", vec![c(&[1.0, 2.0, 4.0, 7.0, 8.0, 9.0, 10.0, 12.0]), n(1.0)]), 3.5);
    close(ev("QUARTILE", vec![c(&[1.0, 2.0, 4.0, 7.0, 8.0, 9.0, 10.0, 12.0]), n(4.0)]), 12.0);
    let q = c(&[6.0, 7.0, 15.0, 36.0, 39.0, 40.0, 41.0, 42.0, 43.0, 47.0, 49.0]);
    close(ev("QUARTILE.EXC", vec![q.clone(), n(1.0)]), 15.0);
    close(ev("QUARTILE.EXC", vec![q.clone(), n(3.0)]), 43.0);
    is_err(ev("QUARTILE.EXC", vec![q, n(4.0)]), CellError::Num);
    let p = c(&[13.0, 12.0, 11.0, 8.0, 4.0, 3.0, 2.0, 1.0, 1.0, 1.0]);
    close(ev("PERCENTRANK.INC", vec![p.clone(), n(2.0)]), 0.333);
    close(ev("PERCENTRANK.INC", vec![p.clone(), n(4.0)]), 0.555);
    close(ev("PERCENTRANK.INC", vec![p.clone(), n(8.0)]), 0.666);
    close(ev("PERCENTRANK", vec![p.clone(), n(5.0)]), 0.583);
    close(ev("PERCENTRANK.INC", vec![p.clone(), n(5.0), n(1.0)]), 0.5);
    is_err(ev("PERCENTRANK.INC", vec![p.clone(), n(0.5)]), CellError::NA);
    is_err(ev("PERCENTRANK.INC", vec![p, n(5.0), n(0.0)]), CellError::Num);
    close(ev("PERCENTRANK.EXC", vec![e9.clone(), n(7.0)]), 0.7);
    close(ev("PERCENTRANK.EXC", vec![e9.clone(), n(5.43)]), 0.381);
    close(ev("PERCENTRANK.EXC", vec![e9, n(5.43), n(1.0)]), 0.3);
    let t = c(&[1.0, 2.0, 3.0]);
    close(ev("PERCENTRANK", vec![t.clone(), n(2.0), n(400.0)]), 0.5);
    close(ev("PERCENTRANK.INC", vec![t.clone(), n(2.0), n(400.0)]), 0.5);
    close(ev("PERCENTRANK.INC", vec![t.clone(), n(1.0), n(400.0)]), 0.0);
    close(ev("PERCENTRANK.EXC", vec![t, n(2.0), n(400.0)]), 0.5);
}

#[test]
fn pairs_and_regression() {
    close_tol(ev("CORREL", vec![c(&[3.0, 2.0, 4.0, 5.0, 6.0]), c(&[9.0, 7.0, 12.0, 15.0, 17.0])]), 0.997054486, 1e-9);
    close_tol(ev("PEARSON", vec![c(&[9.0, 7.0, 5.0, 3.0, 1.0]), c(&[10.0, 6.0, 1.0, 5.0, 3.0])]), 0.699379, D7);
    is_err(ev("CORREL", vec![c(&[1.0, 2.0]), c(&[1.0, 2.0, 3.0])]), CellError::NA);
    is_err(ev("CORREL", vec![c(&[1.0, 1.0]), c(&[1.0, 2.0])]), CellError::Div0);
    let y = c(&[2.0, 3.0, 9.0, 1.0, 8.0, 7.0, 5.0]);
    let x = c(&[6.0, 5.0, 11.0, 7.0, 5.0, 4.0, 4.0]);
    close_tol(ev("SLOPE", vec![y.clone(), x.clone()]), 0.305555556, 1e-8);
    close_tol(ev("RSQ", vec![y.clone(), x.clone()]), 0.057950192, 1e-8);
    close_tol(ev("STEYX", vec![y.clone(), x.clone()]), 3.305718950, 1e-8);
    close_tol(ev("INTERCEPT", vec![c(&[2.0, 3.0, 9.0, 1.0, 8.0]), c(&[6.0, 5.0, 11.0, 7.0, 5.0])]), 0.048387097, 1e-8);
    close_tol(ev("FORECAST", vec![n(30.0), c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 10.607253, D7);
    close_tol(ev("FORECAST.LINEAR", vec![n(30.0), c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 10.607253, D7);
    close(ev("COVARIANCE.P", vec![c(&[3.0, 2.0, 4.0, 5.0, 6.0]), c(&[9.0, 7.0, 12.0, 15.0, 17.0])]), 5.2);
    close(ev("COVAR", vec![c(&[3.0, 2.0, 4.0, 5.0, 6.0]), c(&[9.0, 7.0, 12.0, 15.0, 17.0])]), 5.2);
    close(ev("COVARIANCE.S", vec![c(&[2.0, 4.0, 8.0]), c(&[5.0, 11.0, 12.0])]), 29.0 / 3.0);
    // Pairs with text are skipped.
    let xs = arr(vec![vec![nv(1.0)], vec![tv("x")], vec![nv(3.0)]]);
    close(ev("SLOPE", vec![c(&[2.0, 100.0, 6.0]), rf(xs)]), 2.0);
}

#[test]
fn linest_family() {
    let r = rows_of(&ev("LINEST", vec![c(&[1.0, 9.0, 5.0, 7.0]), c(&[0.0, 4.0, 2.0, 3.0])]));
    assert_eq!(r.len(), 1);
    close(r[0][0].clone(), 2.0);
    close(r[0][1].clone(), 1.0);
    // Statistics for a simple fit: y = 1,2,4 on x = 1,2,3.
    let s = rows_of(&ev("LINEST", vec![c(&[1.0, 2.0, 4.0]), c(&[1.0, 2.0, 3.0]), b(true), b(true)]));
    assert_eq!((s.len(), s[0].len()), (5, 2));
    close(s[0][0].clone(), 1.5);
    close(s[0][1].clone(), -2.0 / 3.0);
    close(s[2][0].clone(), 0.9642857142857143); // r²
    close(s[3][1].clone(), 1.0); // df
    close(s[4][0].clone(), 4.5); // ssreg
    close(s[4][1].clone(), 1.0 / 6.0); // ssresid
    close(s[2][1].clone(), (1.0f64 / 6.0).sqrt()); // sey
    close(s[3][0].clone(), 27.0); // F
    close(s[1][0].clone(), (1.0f64 / 6.0 / 2.0).sqrt()); // se slope
    // No constant.
    let z = rows_of(&ev("LINEST", vec![c(&[2.0, 4.0, 6.0]), c(&[1.0, 2.0, 3.0]), b(false)]));
    close(z[0][0].clone(), 2.0);
    close(z[0][1].clone(), 0.0);
    // Two predictors: y = 1 + 2a + 3b.
    let xs = arr(vec![vec![nv(1.0), nv(0.0)], vec![nv(0.0), nv(1.0)], vec![nv(1.0), nv(1.0)], vec![nv(2.0), nv(1.0)], vec![nv(3.0), nv(5.0)]]);
    let ys: Vec<f64> = [(1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (3.0, 5.0)].iter().map(|(a, b)| 1.0 + 2.0 * a + 3.0 * b).collect();
    let m = rows_of(&ev("LINEST", vec![c(&ys), av(xs)]));
    close(m[0][0].clone(), 3.0);
    close(m[0][1].clone(), 2.0);
    close(m[0][2].clone(), 1.0);
    let gy = [33100.0, 47300.0, 69000.0, 102000.0, 150000.0, 220000.0];
    let gx = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0];
    let l = rows_of(&ev("LOGEST", vec![c(&gy), c(&gx)]));
    close_tol(l[0][0].clone(), 1.463275628, 1e-9);
    close_tol(l[0][1].clone(), 495.3047702, 1e-9);
    let g = rows_of(&ev("GROWTH", vec![c(&gy), c(&gx), c(&[17.0, 18.0])]));
    close_tol(g[0][0].clone(), 320196.7184, 1e-9);
    close_tol(g[1][0].clone(), 468536.0539, 1e-9);
    let ty = [133890.0, 135000.0, 135790.0, 137300.0, 138130.0, 139100.0, 139900.0, 141120.0, 141890.0, 143230.0, 144000.0, 145290.0];
    let tx: Vec<f64> = (1..=12).map(f64::from).collect();
    let t1 = ev("TREND", vec![c(&ty), c(&tx), n(13.0)]);
    close_tol(Value::Number(rows_of(&t1)[0][0].as_f64().unwrap().round()), 146172.0, 1e-9);
    let fitted = rows_of(&ev("TREND", vec![c(&[1.0, 9.0, 5.0, 7.0]), c(&[0.0, 4.0, 2.0, 3.0])]));
    close(fitted[1][0].clone(), 9.0);
    is_err(ev("LOGEST", vec![c(&[1.0, -2.0])]), CellError::Num);
    is_err(ev("LINEST", vec![c(&[1.0, 2.0]), c(&[1.0, 2.0, 3.0])]), CellError::Ref);
}

// #2177: one predictor row of 16,384 columns built three 16,384² matrices (6 GB) and an O(k³)
// inversion; a wider TOROW aborted the process. It is now refused before allocating.
#[test]
fn linest_family_caps_regressor_count() {
    let wide = |k: usize| av(arr(vec![(0..k).map(|i| nv(i as f64)).collect()]));
    let tall = |k: usize| av(arr((0..k).map(|i| vec![nv(i as f64), nv(1.0)]).collect()));
    for f in ["LINEST", "LOGEST", "TREND", "GROWTH"] {
        // y is one cell, so every column of x is a predictor.
        is_err(ev(f, vec![n(1.0), wide(16_384)]), CellError::Value);
        // y is a row, so every row of x is a predictor.
        is_err(ev(f, vec![av(arr(vec![vec![nv(1.0), nv(2.0)]])), tall(16_384)]), CellError::Value);
    }
    is_err(ev("LINEST", vec![n(1.0), wide(MAX_REGRESSORS + 1)]), CellError::Value);
    // At the cap the fit still runs (one observation: coefficients are zero, not an error).
    assert_eq!(rows_of(&ev("LINEST", vec![n(1.0), wide(MAX_REGRESSORS)]))[0].len(), MAX_REGRESSORS + 1);
    // The rows × k² work bound.
    assert_eq!(check_regression_size(1_000, MAX_REGRESSORS), Ok(()));
    assert_eq!(check_regression_size(1_000_000, 100), Err(CellError::Num));
    assert_eq!(check_regression_size(usize::MAX, 2), Err(CellError::Num));
    assert_eq!(check_regression_size(10_000_000, 1), Ok(()));
}

#[test]
fn frequency_prob() {
    let f = ev("FREQUENCY", vec![c(&[79.0, 85.0, 78.0, 85.0, 50.0, 81.0, 95.0, 88.0, 97.0]), c(&[70.0, 79.0, 89.0])]);
    assert_eq!(rows_of(&f), vec![vec![nv(1.0)], vec![nv(2.0)], vec![nv(4.0)], vec![nv(2.0)]]);
    let x = c(&[0.0, 1.0, 2.0, 3.0]);
    let p = c(&[0.2, 0.3, 0.1, 0.4]);
    close(ev("PROB", vec![x.clone(), p.clone(), n(2.0)]), 0.1);
    close(ev("PROB", vec![x.clone(), p, n(1.0), n(3.0)]), 0.8);
    is_err(ev("PROB", vec![x.clone(), c(&[0.5, 0.5, 0.5, 0.5]), n(1.0)]), CellError::Num);
    // Excel: any probability <= 0 is #NUM!, even when it sums to one and lies outside the limits.
    is_err(ev("PROB", vec![x.clone(), c(&[0.5, 0.0, 0.25, 0.25]), n(0.0), n(3.0)]), CellError::Num);
    is_err(ev("PROB", vec![x, c(&[0.5, 0.0, 0.25, 0.25]), n(2.0), n(3.0)]), CellError::Num);
}

#[test]
fn normal() {
    close_tol(ev("NORM.DIST", vec![n(42.0), n(40.0), n(1.5), b(true)]), 0.9087888, D7);
    close_tol(ev("NORM.DIST", vec![n(42.0), n(40.0), n(1.5), b(false)]), 0.10934005, D7);
    close_tol(ev("NORMDIST", vec![n(42.0), n(40.0), n(1.5), b(true)]), 0.9087888, D7);
    is_err(ev("NORM.DIST", vec![n(42.0), n(40.0), n(0.0), b(true)]), CellError::Num);
    close_tol(ev("NORM.INV", vec![n(0.908789), n(40.0), n(1.5)]), 42.000002, D7);
    close_tol(ev("NORMINV", vec![n(0.908789), n(40.0), n(1.5)]), 42.000002, D7);
    is_err(ev("NORM.INV", vec![n(1.0), n(0.0), n(1.0)]), CellError::Num);
    close_tol(ev("NORM.S.DIST", vec![n(1.333333), b(true)]), 0.908788726, 1e-8);
    close_tol(ev("NORM.S.DIST", vec![n(0.0), b(false)]), 0.398942280, 1e-8);
    close_tol(ev("NORMSDIST", vec![n(1.333333)]), 0.908788726, 1e-8);
    close_tol(ev("NORM.S.INV", vec![n(0.908789)]), 1.3333347, D7);
    close_tol(ev("NORMSINV", vec![n(0.975)]), 1.959963985, 1e-9);
    close_tol(ev("PHI", vec![n(0.75)]), 0.301137432, 1e-8);
    close_tol(ev("GAUSS", vec![n(2.0)]), 0.477249868, 1e-8);
    close_tol(ev("FISHER", vec![n(0.75)]), 0.972955075, 1e-8);
    close_tol(ev("FISHERINV", vec![n(0.972955)]), 0.75, D7);
    is_err(ev("FISHER", vec![n(1.0)]), CellError::Num);
    close_tol(ev("CONFIDENCE.NORM", vec![n(0.05), n(2.5), n(50.0)]), 0.692951912, 1e-8);
    close_tol(ev("CONFIDENCE", vec![n(0.05), n(2.5), n(50.0)]), 0.692951912, 1e-8);
    close_tol(ev("CONFIDENCE.T", vec![n(0.05), n(1.0), n(50.0)]), 0.284196855, 1e-8);
    is_err(ev("CONFIDENCE.T", vec![n(0.05), n(1.0), n(1.0)]), CellError::Div0);
    let z = c(&[3.0, 6.0, 7.0, 8.0, 6.0, 5.0, 4.0, 2.0, 1.0, 9.0]);
    close_tol(ev("Z.TEST", vec![z.clone(), n(4.0)]), 0.090574, 1e-5);
    close_tol(ev("ZTEST", vec![z, n(6.0)]), 0.863043, 1e-5);
    close_tol(ev("LOGNORM.DIST", vec![n(4.0), n(3.5), n(1.2), b(true)]), 0.0390836, D7);
    close_tol(ev("LOGNORM.DIST", vec![n(4.0), n(3.5), n(1.2), b(false)]), 0.0176176, D7);
    close_tol(ev("LOGNORMDIST", vec![n(4.0), n(3.5), n(1.2)]), 0.0390836, D7);
    close_tol(ev("LOGNORM.INV", vec![n(0.039084), n(3.5), n(1.2)]), 4.0000252, D7);
    close_tol(ev("LOGINV", vec![n(0.039084), n(3.5), n(1.2)]), 4.0000252, D7);
}

#[test]
fn t_chi_f() {
    close_tol(ev("T.DIST", vec![n(60.0), n(1.0), b(true)]), 0.99469533, D7);
    close_tol(ev("T.DIST", vec![n(8.0), n(3.0), b(false)]), 0.00073691, 1e-5);
    close_tol(ev("T.DIST.2T", vec![n(1.959999998), n(60.0)]), 0.054644930, D7);
    close_tol(ev("T.DIST.RT", vec![n(1.959999998), n(60.0)]), 0.027322465, D7);
    close_tol(ev("TDIST", vec![n(1.959999998), n(60.0), n(2.0)]), 0.054644930, D7);
    is_err(ev("TDIST", vec![n(-1.0), n(60.0), n(2.0)]), CellError::Num);
    is_err(ev("T.DIST", vec![n(1.0), n(0.0), b(true)]), CellError::Num);
    close_tol(ev("T.INV", vec![n(0.75), n(2.0)]), 0.8164966, D7);
    close_tol(ev("T.INV", vec![n(0.25), n(2.0)]), -0.8164966, D7);
    close_tol(ev("T.INV.2T", vec![n(0.546449), n(60.0)]), 0.606533, D7);
    close_tol(ev("T.INV.2T", vec![n(0.05), n(10.0)]), 2.228138852, 1e-9);
    close_tol(ev("TINV", vec![n(0.05), n(10.0)]), 2.228138852, 1e-9);
    let a1 = c(&[3.0, 4.0, 5.0, 8.0, 9.0, 1.0, 2.0, 4.0, 5.0]);
    let a2 = c(&[6.0, 19.0, 3.0, 2.0, 14.0, 4.0, 5.0, 17.0, 1.0]);
    close_tol(ev("T.TEST", vec![a1.clone(), a2.clone(), n(2.0), n(1.0)]), 0.196016, 1e-5);
    close_tol(ev("TTEST", vec![a1.clone(), a2.clone(), n(2.0), n(1.0)]), 0.196016, 1e-5);
    close_tol(ev("T.TEST", vec![a1.clone(), a2.clone(), n(2.0), n(2.0)]), 0.191995887, 1e-6);
    close_tol(ev("T.TEST", vec![a1.clone(), a2.clone(), n(1.0), n(3.0)]), 0.1012, 1e-3);
    is_err(ev("T.TEST", vec![a1, a2, n(3.0), n(1.0)]), CellError::Num);
    close_tol(ev("CHISQ.DIST", vec![n(0.5), n(1.0), b(true)]), 0.52049988, D7);
    close_tol(ev("CHISQ.DIST", vec![n(2.0), n(3.0), b(false)]), 0.20755375, D7);
    close_tol(ev("CHISQ.DIST.RT", vec![n(18.307), n(10.0)]), 0.0500006, 1e-5);
    close_tol(ev("CHIDIST", vec![n(18.307), n(10.0)]), 0.0500006, 1e-5);
    close_tol(ev("CHISQ.INV", vec![n(0.93), n(1.0)]), 3.283020287, 1e-8);
    close_tol(ev("CHISQ.INV", vec![n(0.6), n(2.0)]), 1.832581464, 1e-8);
    close_tol(ev("CHISQ.INV.RT", vec![n(0.050001), n(10.0)]), 18.30697, D7);
    close_tol(ev("CHIINV", vec![n(0.050001), n(10.0)]), 18.30697, D7);
    is_err(ev("CHISQ.DIST", vec![n(-1.0), n(1.0), b(true)]), CellError::Num);
    let act = arr(vec![vec![nv(58.0), nv(35.0)], vec![nv(11.0), nv(25.0)], vec![nv(10.0), nv(23.0)]]);
    let exp = arr(vec![vec![nv(45.35), nv(47.65)], vec![nv(17.56), nv(18.44)], vec![nv(16.09), nv(16.91)]]);
    close_tol(ev("CHISQ.TEST", vec![av(act.clone()), av(exp.clone())]), 0.0003082, 1e-3);
    close_tol(ev("CHITEST", vec![av(act), av(exp)]), 0.0003082, 1e-3);
    close_tol(ev("F.DIST", vec![n(15.2069), n(6.0), n(4.0), b(true)]), 0.99, 1e-6);
    close_tol(ev("F.DIST", vec![n(15.2069), n(6.0), n(4.0), b(false)]), 0.0012238, 1e-5);
    close_tol(ev("F.DIST.RT", vec![n(15.2069), n(6.0), n(4.0)]), 0.01, 1e-6);
    close_tol(ev("FDIST", vec![n(15.2069), n(6.0), n(4.0)]), 0.01, 1e-6);
    close_tol(ev("F.INV", vec![n(0.01), n(6.0), n(4.0)]), 0.10930991, D7);
    close_tol(ev("F.INV.RT", vec![n(0.01), n(6.0), n(4.0)]), 15.20686, D7);
    close_tol(ev("FINV", vec![n(0.01), n(6.0), n(4.0)]), 15.20686, D7);
    close_tol(ev("F.TEST", vec![c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 0.64831785, D7);
    close_tol(ev("FTEST", vec![c(&[6.0, 7.0, 9.0, 15.0, 21.0]), c(&[20.0, 28.0, 31.0, 38.0, 40.0])]), 0.64831785, D7);
    is_err(ev("F.DIST", vec![n(1.0), n(0.0), n(4.0), b(true)]), CellError::Num);
}

#[test]
fn binom_inv_huge_trials_returns() {
    // Above 2^53 neighbouring f64 values are more than 1 apart; the bisection must still end.
    let cases: [(&str, f64, f64, f64); 3] = [("BINOM.INV", 1e17, 0.5, 0.5), ("CRITBINOM", 1e300, 1.0, 0.5), ("BINOM.INV", 1e300, 0.5, 0.5)];
    for (name, trials, p, alpha) in cases {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = ev(name, vec![n(trials), n(p), n(alpha)]);
            let _ = tx.send(r);
        });
        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Value::Number(_)) | Ok(Value::Error(CellError::Num)) => {}
            Ok(other) => panic!("{name}({trials},{p},{alpha}) returned {other:?}"),
            Err(_) => panic!("{name}({trials},{p},{alpha}) did not return within 5 s"),
        }
    }
}

#[test]
fn discrete() {
    close_tol(ev("BINOM.DIST", vec![n(6.0), n(10.0), n(0.5), b(false)]), 0.205078125, 1e-12);
    close_tol(ev("BINOM.DIST", vec![n(6.0), n(10.0), n(0.5), b(true)]), 0.828125, 1e-12);
    close_tol(ev("BINOMDIST", vec![n(6.0), n(10.0), n(0.5), b(false)]), 0.205078125, 1e-12);
    is_err(ev("BINOM.DIST", vec![n(11.0), n(10.0), n(0.5), b(false)]), CellError::Num);
    close_tol(ev("BINOM.DIST.RANGE", vec![n(60.0), n(0.75), n(48.0)]), 0.083974967, 1e-6);
    close_tol(ev("BINOM.DIST.RANGE", vec![n(60.0), n(0.75), n(45.0), n(50.0)]), 0.523629793, 1e-6);
    close(ev("BINOM.INV", vec![n(6.0), n(0.5), n(0.75)]), 4.0);
    close(ev("CRITBINOM", vec![n(6.0), n(0.5), n(0.75)]), 4.0);
    close(ev("BINOM.INV", vec![n(1e9), n(0.5), n(0.5)]), 5e8);
    close_tol(ev("POISSON.DIST", vec![n(2.0), n(5.0), b(true)]), 0.124652, 1e-5);
    close_tol(ev("POISSON.DIST", vec![n(2.0), n(5.0), b(false)]), 0.084224, 1e-5);
    close_tol(ev("POISSON", vec![n(2.0), n(5.0), b(false)]), 0.084224, 1e-5);
    is_err(ev("POISSON.DIST", vec![n(-1.0), n(5.0), b(false)]), CellError::Num);
    close_tol(ev("HYPGEOM.DIST", vec![n(1.0), n(4.0), n(8.0), n(20.0), b(true)]), 0.4654, 1e-4);
    close_tol(ev("HYPGEOM.DIST", vec![n(1.0), n(4.0), n(8.0), n(20.0), b(false)]), 0.3633, 1e-4);
    close_tol(ev("HYPGEOMDIST", vec![n(1.0), n(4.0), n(8.0), n(20.0)]), 0.363261094, 1e-8);
    is_err(ev("HYPGEOMDIST", vec![n(5.0), n(4.0), n(8.0), n(20.0)]), CellError::Num);
    close_tol(ev("NEGBINOM.DIST", vec![n(10.0), n(5.0), n(0.25), b(true)]), 0.3135141, D7);
    close_tol(ev("NEGBINOM.DIST", vec![n(10.0), n(5.0), n(0.25), b(false)]), 0.0550487, D7);
    close_tol(ev("NEGBINOMDIST", vec![n(10.0), n(5.0), n(0.25)]), 0.0550487, D7);
}

#[test]
fn continuous() {
    close_tol(ev("EXPON.DIST", vec![n(0.2), n(10.0), b(true)]), 0.86466472, D7);
    close_tol(ev("EXPON.DIST", vec![n(0.2), n(10.0), b(false)]), 1.35335283, D7);
    close_tol(ev("EXPONDIST", vec![n(0.2), n(10.0), b(true)]), 0.86466472, D7);
    is_err(ev("EXPON.DIST", vec![n(0.2), n(0.0), b(true)]), CellError::Num);
    close_tol(ev("GAMMA.DIST", vec![n(10.00001131), n(9.0), n(2.0), b(false)]), 0.032639, 1e-5);
    close_tol(ev("GAMMA.DIST", vec![n(10.00001131), n(9.0), n(2.0), b(true)]), 0.068094, 1e-5);
    close_tol(ev("GAMMADIST", vec![n(10.00001131), n(9.0), n(2.0), b(true)]), 0.068094, 1e-5);
    close_tol(ev("GAMMA.INV", vec![n(0.068094), n(9.0), n(2.0)]), 10.0000112, 1e-6);
    close_tol(ev("GAMMAINV", vec![n(0.068094), n(9.0), n(2.0)]), 10.0000112, 1e-6);
    close_tol(ev("GAMMALN", vec![n(4.0)]), 1.791759469, 1e-9);
    close_tol(ev("GAMMALN.PRECISE", vec![n(4.5)]), 2.453736571, 1e-9);
    is_err(ev("GAMMALN", vec![n(0.0)]), CellError::Num);
    close_tol(ev("GAMMA", vec![n(2.5)]), 1.329340388, 1e-9);
    close_tol(ev("GAMMA", vec![n(-0.75)]), -4.834146544, 1e-8);
    is_err(ev("GAMMA", vec![n(-1.0)]), CellError::Num);
    is_err(ev("GAMMA", vec![n(0.0)]), CellError::Num);
    close_tol(ev("BETA.DIST", vec![n(2.0), n(8.0), n(10.0), b(true), n(1.0), n(3.0)]), 0.6854706, D7);
    close_tol(ev("BETA.DIST", vec![n(2.0), n(8.0), n(10.0), b(false), n(1.0), n(3.0)]), 1.4837646, D7);
    close_tol(ev("BETADIST", vec![n(2.0), n(8.0), n(10.0), n(1.0), n(3.0)]), 0.6854706, D7);
    close_tol(ev("BETA.INV", vec![n(0.685470581), n(8.0), n(10.0), n(1.0), n(3.0)]), 2.0, D7);
    close_tol(ev("BETAINV", vec![n(0.685470581), n(8.0), n(10.0), n(1.0), n(3.0)]), 2.0, D7);
    is_err(ev("BETA.DIST", vec![n(4.0), n(8.0), n(10.0), b(true), n(1.0), n(3.0)]), CellError::Num);
    close_tol(ev("WEIBULL.DIST", vec![n(105.0), n(20.0), n(100.0), b(true)]), 0.929581, 1e-5);
    close_tol(ev("WEIBULL.DIST", vec![n(105.0), n(20.0), n(100.0), b(false)]), 0.035589, 1e-4);
    close_tol(ev("WEIBULL", vec![n(105.0), n(20.0), n(100.0), b(true)]), 0.929581, 1e-5);
    // Lifting over an array of x values.
    let r = rows_of(&ev("NORM.S.DIST", vec![av(row(&[0.0, 1.0])), b(true)]));
    close(r[0][0].clone(), 0.5);
    close_tol(r[0][1].clone(), 0.841344746, 1e-8);
}

#[test]
fn aggregate_helper() {
    let vals = vec![nv(1.0), nv(2.0), tv("x"), Value::Empty, Value::Bool(true), nv(6.0)];
    close(aggregate_values(1, &vals), 3.0);
    close(aggregate_values(2, &vals), 3.0);
    close(aggregate_values(3, &vals), 5.0);
    close(aggregate_values(4, &vals), 6.0);
    close(aggregate_values(105, &vals), 1.0);
    close(aggregate_values(6, &vals), 12.0);
    close(aggregate_values(9, &vals), 9.0);
    close(aggregate_values(109, &vals), 9.0);
    close(aggregate_values(11, &vals), 14.0 / 3.0);
    close(aggregate_values(10, &vals), 7.0);
    close(aggregate_values(12, &vals), 2.0);
    is_err(aggregate_values(13, &vals), CellError::NA);
    is_err(aggregate_values(14, &vals), CellError::Value);
    is_err(aggregate_values(99, &vals), CellError::Value);
    is_err(aggregate_values(9, &[nv(1.0), Value::Error(CellError::Div0)]), CellError::Div0);
    close(aggregate_values_k(14, &vals, &nv(1.0)), 6.0);
    close(aggregate_values_k(15, &vals, &nv(2.0)), 2.0);
    close(aggregate_values_k(16, &vals, &nv(0.5)), 2.0);
    close(aggregate_values_k(17, &vals, &nv(4.0)), 6.0);
    close(aggregate_values_k(18, &vals, &nv(0.5)), 2.0);
    close(aggregate_values_k(19, &vals, &nv(2.0)), 2.0);
    close(aggregate_values_k(9, &vals, &nv(2.0)), 9.0);
    is_err(aggregate_values_k(14, &vals, &nv(10.0)), CellError::Num);
}
