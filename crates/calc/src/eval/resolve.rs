//! What a name in a formula refers to, decided in one place.
//!
//! The evaluator reads names through [`resolve_name`] and [`is_function_call`], and
//! [`uses`] applies the same rules to a whole expression without evaluating it, keeping LET and
//! LAMBDA bindings as the evaluator does. An edit that must not change what any formula means
//! (renaming a defined name) resolves every use before and after and compares.

use std::rc::Rc;

use gridcraft_formula::Expr;
use gridcraft_model::Workbook;

/// What a name resolves to, in the evaluator's order: a LET or LAMBDA variable, then a defined
/// name (a `Sheet!Name` qualifier picks the sheet; otherwise the sheet's own name before the
/// workbook's), then a table's data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target<L> {
    /// A LET or LAMBDA variable, as the caller identifies it.
    Local(L),
    /// A defined name: its index in [`Workbook::names`].
    Defined(usize),
    /// A table's data body: (sheet, table) indexes.
    Table(usize, usize),
    /// Nothing of that name (`#NAME?`), or a qualifier naming no sheet.
    Unresolved,
}

/// Resolves the name `n` read on `sheet`. `local` answers whether a LET or LAMBDA variable of
/// that name is in scope (and which), and is asked first.
pub fn resolve_name<L>(wb: &Workbook, sheet: usize, n: &str, local: impl FnOnce(&str) -> Option<L>) -> Target<L> {
    if let Some(l) = local(n) {
        return Target::Local(l);
    }
    // The parser stores the sheet unquoted (`' Sales '` keeps its spaces), split at the last `!`
    // (`'a!b'!Name`).
    let (scope_sheet, bare) = match n.rsplit_once('!') {
        Some((s, b)) => match wb.sheet_index(s) {
            Some(i) => (i, b),
            None => return Target::Unresolved,
        },
        None => (sheet, n),
    };
    if let Some(i) = wb.name_index(bare, scope_sheet) {
        return Target::Defined(i);
    }
    // A table name alone means its data body (a table without one is no reference).
    if let Some((si, ti)) = wb.table(bare)
        && wb.sheet(si).and_then(|s| s.tables.get(ti)).and_then(|t| t.data_range()).is_some()
    {
        return Target::Table(si, ti);
    }
    Target::Unresolved
}

/// Whether a call `name(…)` calls a function rather than a name: the functions the evaluator
/// handles itself, by their exact upper-case name as the parser gives it, and worksheet
/// functions. A call to anything else calls what [`resolve_name`] finds (a LAMBDA).
pub fn is_function_call(name: &str) -> bool {
    super::special::is_special(name) || gridcraft_functions::lookup(name).is_some()
}

/// What one use of a name in an expression resolves to: see [`uses`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Use {
    /// A LET or LAMBDA variable: the binding's number, counted in [`visit`] order.
    Local(usize),
    /// A defined name: its index in [`Workbook::names`].
    Defined(usize),
    /// A table's data body.
    Table(usize, usize),
    /// A call to a function.
    Function,
    /// Nothing.
    Unresolved,
}

/// A use of a name that [`visit`] hands out: a name, or the name a call calls.
pub enum Site<'e> {
    Name(&'e mut String),
    Call(&'e mut String),
}

/// The variables in scope: the newest first, each with its binding's number.
struct Env {
    name: String,
    binding: usize,
    outer: Option<Rc<Env>>,
}

fn find_local(mut env: Option<&Rc<Env>>, n: &str) -> Option<usize> {
    while let Some(e) = env {
        if gridcraft_formula::same_name(&e.name, n) {
            return Some(e.binding);
        }
        env = e.outer.as_ref();
    }
    None
}

fn bind(env: &Option<Rc<Env>>, name: &str, binding: usize) -> Option<Rc<Env>> {
    Some(Rc::new(Env { name: name.to_string(), binding, outer: env.clone() }))
}

/// Visits every use of a name in `e` in a fixed order, with the binding number of the LET or
/// LAMBDA variable it reads, if any: `f(site, local)`. LET and LAMBDA variables are bound as the
/// evaluator binds them: a LET name from the value after it on, a LAMBDA's parameters in its
/// body. The names that declare them are not uses. Two expressions of the same shape are visited
/// in the same order, so the uses of a formula before and after a rename line up.
pub fn visit(e: &mut Expr, f: &mut dyn FnMut(Site<'_>, Option<usize>)) {
    let mut bindings = 0usize;
    // An explicit stack: a long operator chain must not recurse once per operand.
    let mut pending: Vec<(&mut Expr, Option<Rc<Env>>)> = vec![(e, None)];
    while let Some((x, env)) = pending.pop() {
        match x {
            Expr::Name(n) => {
                let local = find_local(env.as_ref(), n);
                f(Site::Name(n), local);
            }
            Expr::Call(name, args) => {
                let local = if is_function_call(name) { None } else { find_local(env.as_ref(), name) };
                let kind = name.as_str();
                // The evaluator matches LET and LAMBDA by their exact names.
                let is_let = kind == "LET" && args.len() >= 3 && args.len() % 2 == 1;
                let is_lambda = kind == "LAMBDA" && !args.is_empty();
                f(Site::Call(name), local);
                // The arguments in order, each with the variables it reads; pushed reversed so
                // they are visited left to right.
                let mut children: Vec<(&mut Expr, Option<Rc<Env>>)> = Vec::with_capacity(args.len());
                if is_let {
                    // LET(name1, value1, …, body): each value is read with the names before it.
                    let mut scope = env;
                    let last = args.len() - 1;
                    let mut it = args.iter_mut();
                    for _ in 0..last / 2 {
                        let (Some(n), Some(v)) = (it.next(), it.next()) else { break };
                        // Not a name: the evaluator stops with #VALUE!; read it as an expression.
                        let bound = match n {
                            Expr::Name(bound) => Some(bound.clone()),
                            other => {
                                children.push((other, scope.clone()));
                                None
                            }
                        };
                        children.push((v, scope.clone()));
                        if let Some(bound) = bound {
                            scope = bind(&scope, &bound, bindings);
                            bindings += 1;
                        }
                    }
                    if let Some(body) = it.next() {
                        children.push((body, scope));
                    }
                } else if is_lambda {
                    // LAMBDA(params…, body): the parameters are bound in the body.
                    let last = args.len() - 1;
                    let mut scope = env.clone();
                    let mut it = args.iter_mut();
                    for _ in 0..last {
                        let Some(p) = it.next() else { break };
                        match p {
                            Expr::Name(param) => {
                                scope = bind(&scope, param.trim_start_matches('[').trim_end_matches(']'), bindings);
                                bindings += 1;
                            }
                            other => children.push((other, env.clone())),
                        }
                    }
                    if let Some(body) = it.next() {
                        children.push((body, scope));
                    }
                } else {
                    children.extend(args.iter_mut().map(|a| (a, env.clone())));
                }
                pending.extend(children.into_iter().rev());
            }
            Expr::Array(rows) => pending.extend(rows.iter_mut().flatten().rev().map(|a| (a, env.clone()))),
            Expr::Unary(_, a) | Expr::Paren(a) => pending.push((a, env)),
            Expr::Binary(_, a, b) => {
                pending.push((b, env.clone()));
                pending.push((a, env));
            }
            Expr::Invoke(callee, args) => {
                pending.extend(args.iter_mut().rev().map(|a| (a, env.clone())));
                pending.push((callee, env));
            }
            Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Ref(_) | Expr::Struct(_) | Expr::Missing => {}
        }
    }
}

/// What every use of a name in `e`, read on `sheet`, resolves to, in [`visit`] order.
pub fn uses(wb: &Workbook, sheet: usize, e: &Expr) -> Vec<Use> {
    let mut e = e.clone();
    let mut out = Vec::new();
    visit(&mut e, &mut |site, local| {
        let target = match site {
            Site::Name(n) => resolve_name(wb, sheet, n, |_| local),
            Site::Call(n) if is_function_call(n) => {
                out.push(Use::Function);
                return;
            }
            Site::Call(n) => resolve_name(wb, sheet, n, |_| local),
        };
        out.push(match target {
            Target::Local(b) => Use::Local(b),
            Target::Defined(i) => Use::Defined(i),
            Target::Table(s, t) => Use::Table(s, t),
            Target::Unresolved => Use::Unresolved,
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_model::DefinedName;

    fn wb() -> Workbook {
        let mut wb = Workbook::new();
        wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Sheet2")));
        let name = |n: &str, scope| DefinedName { name: n.into(), scope, formula: "1".into(), comment: String::new(), hidden: false };
        wb.names.push(name("Old", None));
        wb.names.push(name("Old", Some(1)));
        wb
    }

    fn at(src: &str, sheet: usize) -> Vec<Use> {
        let Ok(e) = gridcraft_formula::parse(src) else { return vec![] };
        uses(&wb(), sheet, &e)
    }

    #[test]
    fn a_sheet_reads_its_own_name_first() {
        assert_eq!(at("Old+1", 0), vec![Use::Defined(0)]);
        assert_eq!(at("Old+1", 1), vec![Use::Defined(1)]);
        assert_eq!(at("Sheet2!Old", 0), vec![Use::Defined(1)]);
        assert_eq!(at("Nope!Old", 0), vec![Use::Unresolved]);
    }

    #[test]
    fn let_and_lambda_bind_as_the_evaluator_does() {
        // The value is read before its own name binds; the body reads the variable.
        assert_eq!(at("LET(Old,Old,Old)", 0), vec![Use::Function, Use::Defined(0), Use::Local(0)]);
        assert_eq!(at("LAMBDA(old,old+Old)", 0), vec![Use::Function, Use::Local(0), Use::Local(0)]);
        // A function is called before a variable of the same name.
        assert_eq!(at("LET(sum,1,SUM(sum))", 0), vec![Use::Function, Use::Function, Use::Local(0)]);
    }

    #[test]
    fn the_evaluators_own_functions_are_functions() {
        for f in ["IF", "LET", "LAMBDA", "ROW", "OFFSET", "XLOOKUP", "GROUPBY", "NOW", "SUM"] {
            assert!(is_function_call(f), "{f}");
        }
        assert!(!is_function_call("Old"));
    }

    #[test]
    fn a_long_chain_is_visited_without_recursing() {
        // As long as a formula may be (8,192 characters).
        let src = vec!["Old"; 2_000].join("+");
        assert_eq!(at(&src, 0).len(), 2_000);
    }
}
