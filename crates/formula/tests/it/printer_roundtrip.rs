use gridcraft_formula::{Expr, print};

#[test]
fn number_near_f64_max_prints_exactly() {
    // Typed formulas refuse such literals, but a number from another source must still print as a finite value.
    for n in [f64::MAX, -f64::MAX] {
        let printed = print(&Expr::Number(n));
        assert_eq!(printed.parse::<f64>().ok(), Some(n), "{printed}");
    }
}
