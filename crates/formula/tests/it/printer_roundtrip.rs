use gridcraft_formula::{Expr, parse, print};

#[test]
fn number_near_f64_max_prints_exactly() {
    // Typed formulas refuse such literals, but a number from another source must still print as a finite value.
    for n in [f64::MAX, -f64::MAX] {
        let printed = print(&Expr::Number(n));
        assert_eq!(printed.parse::<f64>().ok(), Some(n), "{printed}");
    }
}

#[test]
fn accepted_number_literals_at_the_limits_round_trip() {
    for src in ["=9.999999999999996E+307", "=2.2250738585072014E-308"] {
        let e = parse(src).unwrap();
        let printed = print(&e);
        let back = parse(&printed).unwrap_or_else(|err| panic!("{printed}: {err:?}"));
        assert_eq!(e, back, "{src} printed as {printed}");
    }
}
