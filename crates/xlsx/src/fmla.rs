//! Formula text conversion between the model (canonical text, no `=`) and files (`_xlfn.`
//! prefixes for newer functions, `_xlpm.` before LET/LAMBDA parameters, `_xludf.` for unresolved
//! functions). Files always use the canonical en-US syntax.

use gridcraft_formula::{BinOp, Expr, UnOp};
use gridcraft_model::Formula;

/// Marker Excel stores before LET/LAMBDA parameter names. Reading strips it (the parser does);
/// writing adds it to the parameters and their uses.
const PARAM_PREFIX: &str = "_xlpm.";

/// Formula text for a file.
pub fn to_file(f: &Formula) -> String {
    match f.expr() {
        Some(e) => expr_to_file(e),
        None => f.text.clone(),
    }
}

/// Arbitrary formula text (names, CF, validation) for a file. Unparseable text is kept.
pub fn text_to_file(text: &str) -> String {
    let body = text.strip_prefix('=').unwrap_or(text);
    match gridcraft_formula::parse(body) {
        Ok(e) => expr_to_file(e),
        Err(_) => body.to_string(),
    }
}

fn expr_to_file(e: Expr) -> String {
    let e = prefix_params(e, &mut Vec::new());
    let e = e.map(&mut |x| match x {
        Expr::Call(name, args) => Expr::Call(gridcraft_formula::catalog::file_name(&name), args),
        // Files have no `#` operator: Excel writes `A1#` as `_xlfn.ANCHORARRAY(A1)`.
        Expr::Unary(UnOp::Spill, r) => Expr::Call("_xlfn.ANCHORARRAY".into(), vec![*r]),
        other => other,
    });
    gridcraft_formula::print(&e)
}

/// The declared spelling of the innermost LET/LAMBDA parameter called `name` (ASCII
/// case-insensitive, like the evaluator's binding lookup).
fn bound<'a>(scope: &'a [String], name: &str) -> Option<&'a str> {
    scope.iter().rev().find(|s| s.eq_ignore_ascii_case(name)).map(String::as_str)
}

/// Prefixes the names bound by LET/LAMBDA, and their uses inside the scope (as values and as call
/// targets), with `_xlpm.`. `scope` holds the parameter names visible at `e`.
///
/// Left-nested chains (`1+1+…`) are unbounded in depth, so the left spine is handled with a loop;
/// only right operands and arguments recurse, bounded by the parser's nesting limit.
fn prefix_params(e: Expr, scope: &mut Vec<String>) -> Expr {
    enum Up {
        Binary(BinOp, Expr),
        Percent,
        Invoke(Vec<Expr>),
    }
    let mut ups: Vec<Up> = Vec::new();
    let mut cur = e;
    let mut node = loop {
        match cur {
            Expr::Binary(op, a, b) => {
                ups.push(Up::Binary(op, *b));
                cur = *a;
            }
            Expr::Unary(UnOp::Percent, x) => {
                ups.push(Up::Percent);
                cur = *x;
            }
            Expr::Invoke(c, args) => {
                ups.push(Up::Invoke(args));
                cur = *c;
            }
            other => break prefix_node(other, scope),
        }
    };
    while let Some(up) = ups.pop() {
        node = match up {
            Up::Binary(op, b) => {
                let b = prefix_params(b, scope);
                Expr::Binary(op, Box::new(node), Box::new(b))
            }
            Up::Percent => Expr::Unary(UnOp::Percent, Box::new(node)),
            Up::Invoke(args) => Expr::Invoke(Box::new(node), args.into_iter().map(|a| prefix_params(a, scope)).collect()),
        };
    }
    node
}

/// [`prefix_params`] for a node that is not part of a left spine.
fn prefix_node(e: Expr, scope: &mut Vec<String>) -> Expr {
    match e {
        Expr::Name(n) if bound(scope, &n).is_some() => Expr::Name(format!("{PARAM_PREFIX}{n}")),
        // Calling a parameter: `LET(f,LAMBDA(x,x+1),f(2))`. Checked before the built-in names, since
        // a parameter may shadow one.
        Expr::Call(name, args) if bound(scope, &name).is_some() => {
            let declared = bound(scope, &name).unwrap_or(&name).to_string();
            Expr::Call(format!("{PARAM_PREFIX}{declared}"), args.into_iter().map(|a| prefix_params(a, scope)).collect())
        }
        Expr::Call(name, args) if name.eq_ignore_ascii_case("LET") => {
            let depth = scope.len();
            let n = args.len();
            let mut out = Vec::with_capacity(n);
            let mut pending: Option<String> = None;
            for (i, a) in args.into_iter().enumerate() {
                if i + 1 < n && i % 2 == 0 {
                    // A name; it is in scope from the next value on, not in its own value.
                    match a {
                        Expr::Name(p) => {
                            out.push(Expr::Name(format!("{PARAM_PREFIX}{p}")));
                            pending = Some(p);
                        }
                        other => {
                            out.push(prefix_params(other, scope));
                            pending = None;
                        }
                    }
                } else {
                    out.push(prefix_params(a, scope));
                    if let Some(p) = pending.take() {
                        scope.push(p);
                    }
                }
            }
            scope.truncate(depth);
            Expr::Call(name, out)
        }
        Expr::Call(name, args) if name.eq_ignore_ascii_case("LAMBDA") => {
            let depth = scope.len();
            let n = args.len();
            let mut out = Vec::with_capacity(n);
            for (i, a) in args.into_iter().enumerate() {
                match a {
                    Expr::Name(p) if i + 1 < n => {
                        out.push(Expr::Name(format!("{PARAM_PREFIX}{p}")));
                        scope.push(p);
                    }
                    other => out.push(prefix_params(other, scope)),
                }
            }
            scope.truncate(depth);
            Expr::Call(name, out)
        }
        Expr::Call(name, args) => Expr::Call(name, args.into_iter().map(|a| prefix_params(a, scope)).collect()),
        Expr::Unary(op, x) => Expr::Unary(op, Box::new(prefix_params(*x, scope))),
        Expr::Paren(x) => Expr::Paren(Box::new(prefix_params(*x, scope))),
        other => other,
    }
}

/// Formula text read from a file → model text (strips prefixes when it parses).
pub fn from_file(text: &str) -> String {
    let body = text.strip_prefix('=').unwrap_or(text);
    match gridcraft_formula::parse(body) {
        Ok(e) => gridcraft_formula::print(&e),
        Err(_) => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_formula::parse_local;
    use gridcraft_locale::{Dialect, language, region};

    fn pt_br() -> Dialect<'static> {
        Dialect { names: language("pt-BR").expect("pt-BR language"), regional: region("pt-BR").expect("pt-BR region") }
    }

    /// A formula typed in pt-BR, as the file stores it.
    fn local_to_file(src: &str) -> String {
        to_file(&Formula::from_expr(parse_local(src, &pt_br()).expect("parses")))
    }

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
        // Future functions stored without a prefix.
        assert_eq!(to_file(&Formula::new("NETWORKDAYS.INTL(A1,B1)")), "NETWORKDAYS.INTL(A1,B1)");
        assert_eq!(from_file("_xlfn.NETWORKDAYS.INTL(A1,B1)"), "NETWORKDAYS.INTL(A1,B1)");
    }

    #[test]
    fn unknown_future_functions_keep_their_prefix() {
        for f in ["_xlfn.FORECAST.ETS(A1,B1:B9,C1:C9)", "_xlfn.SINGLE(A1)+1", "_xlfn._xlws.PY(A1)"] {
            assert_eq!(from_file(f), f, "reading {f}");
            assert_eq!(text_to_file(f), f, "writing {f}");
        }
    }

    #[test]
    fn unresolved_functions_keep_xludf() {
        assert_eq!(from_file("_xludf.SUM(1,2)"), "_xludf.SUM(1,2)");
        assert_eq!(text_to_file("_xludf.SUM(1,2)"), "_xludf.SUM(1,2)");
        assert_eq!(from_file("_xludf.MYFN(1)"), "_xludf.MYFN(1)");
        assert_eq!(text_to_file("_xludf.MYFN(1)"), "_xludf.MYFN(1)");
    }

    #[test]
    fn local_formulas_are_written_canonically() {
        assert_eq!(local_to_file("SE(A1>1,5;SOMA(B1:B3);0)"), "IF(A1>1.5,SUM(B1:B3),0)");
        assert_eq!(local_to_file("PROCV(A2;Tabela1[#Tudo];2;FALSO)"), "VLOOKUP(A2,Tabela1[#All],2,FALSE)");
        assert_eq!(local_to_file("{1\\2;3\\4}"), "{1,2;3,4}");
        assert_eq!(local_to_file("SUM(1;2)"), "_xludf.SUM(1,2)");
        assert_eq!(local_to_file("SOMA(1,5;2)"), "SUM(1.5,2)");
        assert_eq!(
            local_to_file("SOMA(A1#;CONT.VALORES(Tabela1[Col];B2#))"),
            "SUM(_xlfn.ANCHORARRAY(A1),COUNTA(Tabela1[Col],_xlfn.ANCHORARRAY(B2)))"
        );
    }

    #[test]
    fn param_markers() {
        assert_eq!(text_to_file("LAMBDA(x,x+1)"), "_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1)");
        assert_eq!(text_to_file("LET(a,1,a+1)"), "_xlfn.LET(_xlpm.a,1,_xlpm.a+1)");
        assert_eq!(text_to_file("LET(a,1,b,a+1,a+b)"), "_xlfn.LET(_xlpm.a,1,_xlpm.b,_xlpm.a+1,_xlpm.a+_xlpm.b)");
        // A name is in scope after its own value only; outside the scope it is a plain name.
        assert_eq!(text_to_file("LET(a,a,a)+a"), "_xlfn.LET(_xlpm.a,a,_xlpm.a)+a");
        assert_eq!(text_to_file("LAMBDA(x,y,x*y)(2,3)"), "_xlfn.LAMBDA(_xlpm.x,_xlpm.y,_xlpm.x*_xlpm.y)(2,3)");
        assert_eq!(text_to_file("MAP(A1:A3,LAMBDA(v,v*2))"), "_xlfn.MAP(A1:A3,_xlfn.LAMBDA(_xlpm.v,_xlpm.v*2))");
        assert_eq!(text_to_file("LET(x,1,LAMBDA(x,x)(x))"), "_xlfn.LET(_xlpm.x,1,_xlfn.LAMBDA(_xlpm.x,_xlpm.x)(_xlpm.x))");
        // Reading removes the markers.
        assert_eq!(from_file("_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1)"), "LAMBDA(x,x+1)");
        assert_eq!(from_file("_xlfn.LET(_xlpm.a,1,_xlpm.a+1)"), "LET(a,1,a+1)");
        assert_eq!(from_file(&text_to_file("LET(a,1,b,a+1,a+b)")), "LET(a,1,b,a+1,a+b)");
    }

    #[test]
    fn calls_through_parameters_are_marked() {
        let written = "_xlfn.LET(_xlpm.fn,_xlfn.LAMBDA(_xlpm.x,_xlpm.x+1),_xlpm.fn(2))";
        assert_eq!(text_to_file("LET(fn,LAMBDA(x,x+1),fn(2))"), written);
        // Reading drops the marker (call targets are upper-cased like every function name) and
        // writing it again is stable.
        assert_eq!(from_file(written), "LET(fn,LAMBDA(x,x+1),FN(2))");
        assert_eq!(text_to_file(&from_file(written)), written);
        // A parameter that shadows a built-in is called as the parameter inside its scope only.
        let shadow = "_xlfn.LET(_xlpm.sum,_xlfn.LAMBDA(_xlpm.x,_xlpm.x),_xlpm.sum(1))+SUM(1)";
        assert_eq!(text_to_file("LET(sum,LAMBDA(x,x),sum(1))+SUM(1)"), shadow);
        assert_eq!(text_to_file(&from_file(shadow)), shadow);
        // Outside the scope the same name is an ordinary call.
        assert_eq!(text_to_file("LET(f,1,f)+F(2)"), "_xlfn.LET(_xlpm.f,1,_xlpm.f)+F(2)");
    }

    #[test]
    fn deep_left_nested_chains_do_not_overflow_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                for src in [format!("1{}", "+1".repeat(5000)), format!("A1{}", "%".repeat(5000)), format!("LAMBDA(x,x){}", "(1)".repeat(3000))] {
                    let written = text_to_file(&src);
                    assert_eq!(written, src.replacen("LAMBDA(x,x)", "_xlfn.LAMBDA(_xlpm.x,_xlpm.x)", 1));
                    assert_eq!(from_file(&written), src);
                    assert_eq!(to_file(&Formula::new(&src)), written);
                }
            })
            .expect("spawn")
            .join()
            .expect("no stack overflow");
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
        // The marker keeps the parameter's declared spelling, not the parser's upper-cased call name.
        assert_eq!(to_file(&Formula::new("LET(f,LAMBDA(x,x*2),f(3))")), "_xlfn.LET(_xlpm.f,_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2),_xlpm.f(3))");
        assert_eq!(from_file("_xlfn.LET(_xlpm.f,_xlfn.LAMBDA(_xlpm.x,_xlpm.x*2),_xlpm.f(3))"), "LET(f,LAMBDA(x,x*2),F(3))");
    }
}
