//! Formula text conversion between the model (canonical text, no `=`) and files (`_xlfn.`
//! prefixes for newer functions).

use gridcraft_formula::Expr;
use gridcraft_model::Formula;

/// Formula text for a file.
pub fn to_file(f: &Formula) -> String {
    match f.expr() {
        Some(e) => expr_to_file(e),
        None => f.text.clone(),
    }
}

/// Expressions shared by copied formulas, converted to file form, by the expression they share
/// (and whether the file form is the expression itself, which it is unless a function needs a
/// prefix or LET/LAMBDA has parameters).
pub type FileExprs = std::collections::HashMap<usize, (std::sync::Arc<Expr>, Expr, bool)>;

fn file_form(cache: &mut FileExprs, shared: std::sync::Arc<Expr>) -> &(std::sync::Arc<Expr>, Expr, bool) {
    cache.entry(std::sync::Arc::as_ptr(&shared) as usize).or_insert_with(|| {
        let file = file_expr((*shared).clone());
        let same = file == *shared;
        (shared, file, same)
    })
}

/// [`to_file`] for many formulas: a copy of a shared formula converts the shared expression
/// once (the conversion doesn't touch references) and prints it moved to the copy's cell.
pub fn to_file_shared(f: &Formula, cache: &mut FileExprs) -> String {
    let Some((shared, (r, c))) = f.parsed() else { return f.text.clone() };
    if (r, c) == (0, 0) {
        return to_file(f);
    }
    let (_, file, _) = file_form(cache, shared);
    gridcraft_formula::print_shifted(file, r.into(), c.into())
}

/// The copy of `prev` `dr` rows and `dc` columns away, if `file_text` (a formula read from a
/// file) is that copy's text in file form: a filled column read without parsing. Usually the
/// file form is the expression's own text, which is then the copy's text as is.
pub fn copy_if_file_text(prev: &Formula, dr: i64, dc: i64, file_text: &str, cache: &mut FileExprs) -> Option<Formula> {
    // Only a formula with its expression parsed can share it (one read from text alone can't).
    prev.parsed_ref()?;
    let (shared, (r, c)) = prev.parsed()?;
    let (r, c) = (i64::from(r).checked_add(dr)?, i64::from(c).checked_add(dc)?);
    let (shared, file, same) = file_form(cache, shared);
    let candidate = gridcraft_formula::print_shifted(file, r, c);
    if candidate != file_text {
        return None;
    }
    let text = if *same { candidate } else { gridcraft_formula::print_shifted(shared, r, c) };
    prev.moved_with_text(dr, dc, text)
}

/// Arbitrary formula text (names, CF, validation) for a file. Unparseable text is kept.
pub fn text_to_file(text: &str) -> String {
    let body = text.strip_prefix('=').unwrap_or(text);
    match gridcraft_formula::parse(body) {
        Ok(e) => expr_to_file(e),
        Err(_) => body.to_string(),
    }
}

const PARAM_PREFIX: &str = "_xlpm.";

/// Adds Excel's `_xlpm.` prefix to LET/LAMBDA parameter names, at the declaration and at every use in scope.
fn prefix_params(e: Expr, scope: &[String]) -> Expr {
    let sub = |x: Box<Expr>, sc: &[String]| Box::new(prefix_params(*x, sc));
    match e {
        Expr::Name(n) if scope.iter().any(|s| s.eq_ignore_ascii_case(&n)) => Expr::Name(format!("{PARAM_PREFIX}{n}")),
        Expr::Unary(op, x) => Expr::Unary(op, sub(x, scope)),
        Expr::Paren(x) => Expr::Paren(sub(x, scope)),
        Expr::Binary(op, a, b) => Expr::Binary(op, sub(a, scope), sub(b, scope)),
        Expr::Invoke(c, args) => Expr::Invoke(sub(c, scope), args.into_iter().map(|a| prefix_params(a, scope)).collect()),
        Expr::Call(name, args) if name == "LET" || name == "LAMBDA" => {
            let last = args.len().saturating_sub(1);
            let is_let = name == "LET";
            let mut sc = scope.to_vec();
            let mut pending: Option<String> = None;
            let mut out = Vec::with_capacity(args.len());
            for (i, a) in args.into_iter().enumerate() {
                let declares = i < last && (!is_let || i % 2 == 0);
                match a {
                    Expr::Name(n) if declares => {
                        out.push(Expr::Name(format!("{PARAM_PREFIX}{n}")));
                        if is_let {
                            pending = Some(n);
                        } else {
                            sc.push(n);
                        }
                    }
                    other => {
                        out.push(prefix_params(other, &sc));
                        // A LET name is in scope only after its value expression.
                        if let Some(n) = pending.take() {
                            sc.push(n);
                        }
                    }
                }
            }
            Expr::Call(name, out)
        }
        // A LET-bound LAMBDA called by name, e.g. `f(3)`.
        Expr::Call(name, args) if scope.iter().any(|s| s.eq_ignore_ascii_case(&name)) => {
            Expr::Call(format!("{PARAM_PREFIX}{name}"), args.into_iter().map(|a| prefix_params(a, scope)).collect())
        }
        Expr::Call(name, args) => Expr::Call(name, args.into_iter().map(|a| prefix_params(a, scope)).collect()),
        other => other,
    }
}

fn expr_to_file(e: Expr) -> String {
    gridcraft_formula::print(&file_expr(e))
}

/// The expression as files write it (references untouched).
fn file_expr(e: Expr) -> Expr {
    let e = prefix_params(e, &[]);
    e.map(&mut |x| match x {
        Expr::Call(name, args) => match crate::tables::file_function_name(&name) {
            Some(n) => Expr::Call(n, args),
            None => Expr::Call(name, args),
        },
        // Files have no `#` operator: Excel writes `A1#` as `_xlfn.ANCHORARRAY(A1)`.
        Expr::Unary(gridcraft_formula::UnOp::Spill, r) => Expr::Call("_xlfn.ANCHORARRAY".into(), vec![*r]),
        other => other,
    })
}

/// `_xlpm.x` → `x` (any case, as the parser upper-cases function names).
fn strip_param_prefix(n: String) -> String {
    match (n.get(..PARAM_PREFIX.len()), n.get(PARAM_PREFIX.len()..)) {
        (Some(p), Some(rest)) if p.eq_ignore_ascii_case(PARAM_PREFIX) => rest.to_string(),
        _ => n,
    }
}

/// Formula text read from a file → model text (strips prefixes when it parses).
pub fn from_file(text: &str) -> String {
    let body = text.strip_prefix('=').unwrap_or(text);
    match gridcraft_formula::parse(body) {
        Ok(e) => gridcraft_formula::print(&e.map(&mut |x| match x {
            Expr::Name(n) => Expr::Name(strip_param_prefix(n)),
            Expr::Call(n, args) => Expr::Call(strip_param_prefix(n), args),
            other => other,
        })),
        Err(_) => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spill_references() {
        assert_eq!(to_file(&Formula::new("SUM(Sheet1!$D$1#)+B2#")), "SUM(_xlfn.ANCHORARRAY(Sheet1!$D$1))+_xlfn.ANCHORARRAY(B2)");
        assert_eq!(text_to_file("=Sheet1!$D$1#"), "_xlfn.ANCHORARRAY(Sheet1!$D$1)");
        assert_eq!(from_file("SUM(_xlfn.ANCHORARRAY(Sheet1!$D$1))+_xlfn.ANCHORARRAY(B2)"), "SUM(Sheet1!$D$1#)+B2#");
    }

    #[test]
    fn prefixes() {
        assert_eq!(to_file(&Formula::new("XLOOKUP(1,A:A,B:B)+SUM(A1)")), "_xlfn.XLOOKUP(1,A:A,B:B)+SUM(A1)");
        assert_eq!(to_file(&Formula::new("FILTER(A1:A3,B1:B3)")), "_xlfn._xlws.FILTER(A1:A3,B1:B3)");
        assert_eq!(from_file("_xlfn.XLOOKUP(1,A:A,B:B)"), "XLOOKUP(1,A:A,B:B)");
        assert_eq!(text_to_file("=IFS(A1>0,1)"), "_xlfn.IFS(A1>0,1)");
    }

    #[test]
    fn let_lambda_params() {
        assert_eq!(to_file(&Formula::new("LET(x,1,x+1)")), "_xlfn.LET(_xlpm.x,1,_xlpm.x+1)");
        assert_eq!(to_file(&Formula::new("LET(x,A1*2,y,x+1,x*y)")), "_xlfn.LET(_xlpm.x,A1*2,_xlpm.y,_xlpm.x+1,_xlpm.x*_xlpm.y)");
        assert_eq!(to_file(&Formula::new("LAMBDA(a,b,a^2+b)(3,4)")), "_xlfn.LAMBDA(_xlpm.a,_xlpm.b,_xlpm.a^2+_xlpm.b)(3,4)");
        // A name outside the scope stays a plain defined name.
        assert_eq!(to_file(&Formula::new("LET(x,1,x)+x")), "_xlfn.LET(_xlpm.x,1,_xlpm.x)+x");
        assert_eq!(from_file("_xlfn.LET(_xlpm.x,1,_xlpm.x+1)"), "LET(x,1,x+1)");
        assert_eq!(from_file("_xlfn.LAMBDA(_xlpm.a,_xlpm.a*2)(3)"), "LAMBDA(a,a*2)(3)");
        // A LET-bound LAMBDA called by name.
        assert_eq!(to_file(&Formula::new("LET(f,LAMBDA(x,x*2),f(3))")), "_xlfn.LET(_xlpm.f,_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2),_xlpm.F(3))");
        assert_eq!(from_file("_xlfn.LET(_xlpm.f,_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2),_xlpm.f(3))"), "LET(f,LAMBDA(x,x*2),F(3))");
    }
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    #[test]
    fn shared_copies_write_the_same_text_as_their_own_formulas() {
        let formulas = ["XLOOKUP(A2,$A$1:$A$9,B1:B9)", "LET(x,A2,LAMBDA(y,y+x)(B1))", "SUM(A2#)+SEQUENCE(A1)", "MAX(A1:A2)+_xlfn.CONCAT(C1)"];
        let mut cache = FileExprs::default();
        for f in formulas {
            let origin = Formula::new(f);
            for (dr, dc) in [(1, 0), (5, 2), (-1, 0), (0, -1)] {
                // What a copy wrote before copies shared expressions: its own moved expression.
                let copy = origin.moved(dr, dc);
                let own = Formula::from_expr(gridcraft_formula::adjust::shift_relative(gridcraft_formula::parse(f).unwrap(), dr, dc));
                assert_eq!(copy.text, own.text);
                assert_eq!(to_file_shared(&copy, &mut cache), to_file(&own), "{f} by ({dr}, {dc})");
            }
        }
    }
}
