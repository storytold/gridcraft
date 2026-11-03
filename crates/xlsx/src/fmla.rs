//! Formula text conversion between the model (canonical text, no `=`) and files (`_xlfn.`
//! prefixes for newer functions).

use gridcraft_formula::Expr;
use gridcraft_model::Formula;

/// Formula text for a file.
pub fn to_file(f: &Formula) -> String {
    match f.expr_arc() {
        // Most formulas have nothing the file spells differently: print them without copying.
        Some(e) if !needs_file_spelling(&e) => gridcraft_formula::print(&e),
        Some(e) => expr_to_file(Expr::clone(&e)),
        None => f.text.clone(),
    }
}

/// Whether [`expr_to_file`] would change anything in `e`: a LET or LAMBDA (parameter prefixes),
/// a function with a file prefix, a spill, `@` or trim operator, or a structured reference.
fn needs_file_spelling(e: &Expr) -> bool {
    let mut found = false;
    e.walk(&mut |x| {
        found |= match x {
            Expr::Call(name, _) => name == "LET" || name == "LAMBDA" || crate::tables::file_function_name(name).is_some(),
            Expr::Unary(gridcraft_formula::UnOp::Spill | gridcraft_formula::UnOp::At | gridcraft_formula::UnOp::Trim(_), _) => true,
            Expr::Struct(_) => true,
            _ => false,
        }
    });
    found
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
        Expr::Binary(op, a, b) => {
            // A long chain such as A1+A2+… nests on its left: follow it in a loop, not one
            // level of recursion per operand.
            let mut spine = vec![(op, b)];
            let mut left = *a;
            while let Expr::Binary(op, a, b) = left {
                spine.push((op, b));
                left = *a;
            }
            let mut acc = prefix_params(left, scope);
            while let Some((op, b)) = spine.pop() {
                acc = Expr::Binary(op, Box::new(acc), sub(b, scope));
            }
            acc
        }
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
    let e = prefix_params(e, &[]);
    let e = e.map(&mut |x| match x {
        Expr::Call(name, args) => match crate::tables::file_function_name(&name) {
            Some(n) => Expr::Call(n, args),
            None => Expr::Call(name, args),
        },
        // Files have no `#` operator: Excel writes `A1#` as `_xlfn.ANCHORARRAY(A1)`, and a spill
        // reference that became `#REF!` (its cell was deleted) as plain `#REF!`.
        Expr::Unary(gridcraft_formula::UnOp::Spill, r) if matches!(*r, Expr::Error(_)) => *r,
        Expr::Unary(gridcraft_formula::UnOp::Spill, r) => Expr::Call("_xlfn.ANCHORARRAY".into(), vec![*r]),
        // Nor an `@` operator: Excel writes `@x` as `_xlfn.SINGLE(x)`.
        Expr::Unary(gridcraft_formula::UnOp::At, x) => Expr::Call("_xlfn.SINGLE".into(), vec![*x]),
        // Nor the `[@Col]` shorthand: files spell it `[[#This Row],[Col]]`, or Excel repairs
        // the file and drops the table. Carried as a name, which prints verbatim.
        Expr::Struct(st) => Expr::Name(gridcraft_formula::printer::struct_ref_file(&st)),
        // Nor trim-reference operators: written as the equivalent `TRIMRANGE(range,n,n)`, which
        // every Excel with trim references reads. (Excel's own file spelling of `A1:.E10` is
        // not publicly documented; the `_xlfn._TRO_*` call forms are accepted on read.)
        Expr::Unary(gridcraft_formula::UnOp::Trim(mode), x) => {
            let code = Expr::Number(f64::from(mode.code()));
            Expr::Call("_xlfn.TRIMRANGE".into(), vec![*x, code.clone(), code])
        }
        other => other,
    });
    gridcraft_formula::print(&e)
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

    /// Formulas without a file spelling skip the rewrite; the text must be what it would give.
    #[test]
    fn plain_formulas_print_as_the_rewrite_would() {
        for text in [
            "SUM(A1:B2)*2",
            "A1&\"x\"\"y\"",
            "Sheet2!$A$1+'My Sheet'!C:C-$3:4",
            "IF(A1>0,{1,2;3,4},-B$2%)",
            "Sheet1:Sheet3!A1",
            "INDEX(A:A,1):B5",
            "Total*2",
            "XLOOKUP(1,A:A,B:B)",
            "LET(x,1,x)",
            "LAMBDA(a,a)(1)",
            "A1#",
            "@A1:A2",
            "SUM(A1:.B3)",
            "T[@C]",
        ] {
            let f = Formula::new(text);
            assert_eq!(to_file(&f), expr_to_file(f.expr().unwrap()), "{text}");
        }
    }

    /// The fast path in [`to_file`] skips the rewrite on a list of what triggers it. Run both paths
    /// over every registered function, alone, nested and next to every operator, so a function or
    /// rewrite added later without updating [`needs_file_spelling`] fails here.
    #[test]
    fn every_registered_function_prints_as_the_rewrite_would() {
        let shapes = [
            "{}",
            "1+{}",
            "-{}%",
            "SUM({},A1:B2)",
            "IF(A1,{},Sheet2!$C$3)",
            "LET(x,{},x)",
            "LAMBDA(a,{})(1)",
            "@{}",
            "{}&A1#",
            "{}+SUM(A1:.B3)",
            "{}+T[@C]",
            "{}+T[[#All],[C]]",
            "({})*{{1,2;3,4}}",
            "{}:B5 C1:D2",
            "{}+My Sheet!A:A+Total",
        ];
        let mut checked = 0;
        for spec in gridcraft_functions::all() {
            let n = spec.min_args.max(1).min(spec.max_args).min(4);
            let args = ["A1", "B2:C3", "1", "\"x\""].get(..n).unwrap_or_default().join(",");
            let call = format!("{}({args})", spec.name);
            for shape in shapes {
                let text = shape.replace("{}", &call).replace("{{", "{").replace("}}", "}");
                let f = Formula::new(&text);
                let Some(e) = f.expr() else { continue };
                assert_eq!(to_file(&f), expr_to_file(e), "{text}");
                checked += 1;
            }
        }
        assert!(checked > gridcraft_functions::all().len() * 10, "only {checked} formulas parsed");
    }

    #[test]
    fn spill_references() {
        assert_eq!(to_file(&Formula::new("SUM(Sheet1!$D$1#)+B2#")), "SUM(_xlfn.ANCHORARRAY(Sheet1!$D$1))+_xlfn.ANCHORARRAY(B2)");
        assert_eq!(text_to_file("=Sheet1!$D$1#"), "_xlfn.ANCHORARRAY(Sheet1!$D$1)");
        assert_eq!(from_file("SUM(_xlfn.ANCHORARRAY(Sheet1!$D$1))+_xlfn.ANCHORARRAY(B2)"), "SUM(Sheet1!$D$1#)+B2#");
        // Found by the command-sequence fuzzer: deleting the anchor of `H5#` left `=#REF!` in
        // the workbook but `ANCHORARRAY(#REF!)` in the file, which read back differently.
        let deleted = Formula::from_expr(Expr::Unary(gridcraft_formula::UnOp::Spill, Box::new(Expr::Error(gridcraft_core::CellError::Ref))));
        assert_eq!(deleted.text, "#REF!");
        assert_eq!(to_file(&deleted), "#REF!");
    }

    #[test]
    fn implicit_intersection() {
        assert_eq!(to_file(&Formula::new("SUM(@A1:A3*B1:B3)")), "SUM(_xlfn.SINGLE(A1:A3)*B1:B3)");
        assert_eq!(to_file(&Formula::new("@A1:A3*2")), "_xlfn.SINGLE(A1:A3)*2");
        assert_eq!(text_to_file("=@Sheet1!$A$1:$A$3"), "_xlfn.SINGLE(Sheet1!$A$1:$A$3)");
        assert_eq!(from_file("_xlfn.SINGLE(A1:A3)*2"), "@A1:A3*2");
        assert_eq!(from_file("SUM(_xlfn.SINGLE(A1:A3)*B1:B3)"), "SUM(@A1:A3*B1:B3)");
    }

    #[test]
    fn trim_references() {
        assert_eq!(to_file(&Formula::new("SUM(A1:.E10)+SUM(Data!A.:A)")), "SUM(_xlfn.TRIMRANGE(A1:E10,2,2))+SUM(_xlfn.TRIMRANGE(Data!A:A,1,1))");
        assert_eq!(text_to_file("=ROWS(1.:.5)"), "ROWS(_xlfn.TRIMRANGE(1:5,3,3))");
        assert_eq!(from_file("SUM(_xlfn._TRO_TRAILING(A1:E10))+_xlfn._TRO_ALL($A:$B)"), "SUM(A1:.E10)+$A.:.$B");
    }

    #[test]
    fn unbracketed_this_row_column() {
        assert_eq!(from_file("Sales[[#This Row],Units]*[[#This Row],Price]"), "Sales[@Units]*[@Price]");
    }

    #[test]
    fn this_row_long_form() {
        // `[@Col]` is only Excel's display shorthand; a file holding it is repaired by Excel,
        // which drops the table.
        assert_eq!(to_file(&Formula::new("Sales[@Units]*Sales[@[Unit Price]]")), "Sales[[#This Row],[Units]]*Sales[[#This Row],[Unit Price]]");
        assert_eq!(text_to_file("=[@Units]&COUNTA(Sales[@])"), "[[#This Row],[Units]]&COUNTA(Sales[#This Row])");
        assert_eq!(text_to_file("=SUM(Sales[Units])+[#Totals]"), "SUM(Sales[Units])+[#Totals]");
        for f in ["Sales[@Units]*[@[Unit Price]]", "Sales[@]", "[@[Unit Price]]+1"] {
            let file = to_file(&Formula::new(f));
            assert!(!file.contains("[@"), "{file}");
            assert_eq!(from_file(&file), f);
        }
    }

    #[test]
    fn struct_ref_column_with_bang() {
        // A `!` inside the brackets is part of the column name, not a sheet separator.
        assert_eq!(to_file(&Formula::new("SUM(Table1[Done!])")), "SUM(Table1[Done!])");
        assert_eq!(text_to_file("=Table1[[#This Row],[a!b]]"), "Table1[[#This Row],[a!b]]");
        // `Table1[#REF!]` was found by the command-sequence fuzzer (seed 40).
        for text in ["SUM(Table1[Done!])", "Table1[#REF!]", "Table1[@[x!]]", "Sheet2!A1+Table1[[#All],[a!b]]"] {
            let f = Formula::new(text);
            assert_eq!(from_file(&to_file(&f)), f.text);
        }
    }

    #[test]
    fn prefixes() {
        assert_eq!(to_file(&Formula::new("XLOOKUP(1,A:A,B:B)+SUM(A1)")), "_xlfn.XLOOKUP(1,A:A,B:B)+SUM(A1)");
        assert_eq!(to_file(&Formula::new("FILTER(A1:A3,B1:B3)")), "_xlfn._xlws.FILTER(A1:A3,B1:B3)");
        assert_eq!(from_file("_xlfn.XLOOKUP(1,A:A,B:B)"), "XLOOKUP(1,A:A,B:B)");
        assert_eq!(text_to_file("=IFS(A1>0,1)"), "_xlfn.IFS(A1>0,1)");
        assert_eq!(to_file(&Formula::new("REGEXTEST(A1,\"x\")&ENCODEURL(B1)")), "_xlfn.REGEXTEST(A1,\"x\")&_xlfn.ENCODEURL(B1)");
        assert_eq!(to_file(&Formula::new("GROUPBY(A1:A3,B1:B3,\"SUM\")")), "_xlfn.GROUPBY(A1:A3,B1:B3,\"SUM\")");
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
