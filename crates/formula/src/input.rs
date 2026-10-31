//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::spanish::SPANISH;
use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
}

/// Upper-case English name -> Spanish name, and upper-case Spanish name -> English name.
/// Keys are upper-cased in full Unicode, so `año` finds `AÑO`.
fn spanish_maps() -> &'static (HashMap<String, &'static str>, HashMap<String, &'static str>) {
    static MAPS: OnceLock<(HashMap<String, &'static str>, HashMap<String, &'static str>)> = OnceLock::new();
    MAPS.get_or_init(|| {
        let to_es = SPANISH.iter().map(|&(en, es)| (en.to_uppercase(), es)).collect();
        let to_en = SPANISH.iter().map(|&(en, es)| (es.to_uppercase(), en)).collect();
        (to_es, to_en)
    })
}

/// Every (English, Spanish) function-name pair. Names Spanish Excel keeps in English are listed
/// with themselves.
pub fn spanish_function_names() -> &'static [(&'static str, &'static str)] {
    SPANISH
}

impl FormulaLocale {
    pub fn list_separator(self) -> char {
        if self == Self::Es { ';' } else { ',' }
    }
    pub fn decimal_separator(self) -> char {
        if self == Self::Es { ',' } else { '.' }
    }
    pub fn array_column_separator(self) -> char {
        if self == Self::Es { '\\' } else { ',' }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        if self == Self::Es { spanish_maps().0.get(&canonical.to_uppercase()).copied().unwrap_or(canonical) } else { canonical }
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        if self == Self::Es { spanish_maps().1.get(&localized.to_uppercase()).copied().unwrap_or(localized) } else { localized }
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale == FormulaLocale::Es {
        normalize(&mut expr, &known_name, &mut Vec::new(), 0, false)?;
    }
    Ok(expr)
}

/// Prints canonical expressions for editing. Name collisions retain the canonical spelling
/// so displaying then committing a formula never changes a defined-name reference.
pub fn print_input(expr: &Expr, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<String, ParseError> {
    // Check before cloning/printing. A flat operator chain such as A1+A2+… nests on its left
    // without nesting in the text: cloning, printing and `normalize` follow that left spine in
    // a loop, so it does not count (Excel accepts 4,096 operands, #916). Other nesting does.
    let mut pending = vec![(expr, 0usize)];
    while let Some((node, depth)) = pending.pop() {
        if depth > parser::MAX_DEPTH {
            return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
        }
        match node {
            Expr::Unary(_, child) | Expr::Paren(child) => pending.push((child, depth + 1)),
            Expr::Binary(_, left, right) => {
                pending.push((left, depth));
                pending.push((right, depth + 1));
            }
            Expr::Call(_, args) => pending.extend(args.iter().map(|e| (e, depth + 1))),
            Expr::Invoke(callee, args) => {
                pending.push((callee, depth + 1));
                pending.extend(args.iter().map(|e| (e, depth + 1)));
            }
            Expr::Array(rows) => pending.extend(rows.iter().flatten().map(|e| (e, depth + 1))),
            _ => {}
        }
    }
    if locale == FormulaLocale::En {
        return Ok(crate::print(expr));
    }
    let mut expr = expr.clone();
    normalize(&mut expr, &known_name, &mut Vec::new(), 0, true)?;
    let canonical = crate::print(&expr);
    let tokens = crate::lexer::tokenize(&canonical).map_err(|e| ParseError { msg: e.msg, pos: e.pos })?;
    let mut out = String::new();
    let mut array_depth = 0usize;
    for token in tokens {
        use crate::lexer::Tok;
        let text = canonical.get(token.start..token.end).unwrap_or("");
        match token.tok {
            Tok::LBrace => {
                array_depth += 1;
                out.push_str(text);
            }
            Tok::RBrace => {
                array_depth = array_depth.saturating_sub(1);
                out.push_str(text);
            }
            Tok::Number(_) => out.push_str(&text.replace('.', &locale.decimal_separator().to_string())),
            Tok::Comma => out.push(if array_depth > 0 { locale.array_column_separator() } else { locale.list_separator() }),
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

pub(crate) fn boolean_alias(name: &str) -> Option<bool> {
    if name.eq_ignore_ascii_case("VERDADERO") {
        Some(true)
    } else if name.eq_ignore_ascii_case("FALSO") {
        Some(false)
    } else {
        None
    }
}

/// The Spanish (`display`) or English spelling of a function name that no binding or defined
/// name shadows. Shown in Spanish, a function whose Spanish name a binding shadows keeps its
/// English name; when that English name is the Spanish name of another function (`DECIMAL`,
/// which is FIXED in Spanish), no spelling would read back as the same function, so the
/// formula cannot be shown in Spanish.
fn translate_function(name: &str, display: bool, bound: &impl Fn(&str) -> bool) -> Result<String, ParseError> {
    let es = FormulaLocale::Es;
    if !display {
        return Ok(es.canonical_name(name).to_string());
    }
    let alias = es.function_name(name);
    if !bound(alias) {
        return Ok(alias.to_string());
    }
    if !crate::same_name(es.canonical_name(name), name) {
        return Err(ParseError { msg: format!("{name} cannot be shown in Spanish while {alias} names a local value"), pos: 0 });
    }
    Ok(name.to_string())
}

/// Whether argument `index` of `count` passed to the built-in `function` (English name) is a
/// function, which may be given by name: the LAMBDA of the helpers and the aggregation of
/// GROUPBY and PIVOTBY, as the evaluator reads them. Every other argument is a value, so a
/// name there stays a name (`SUMA(RAIZ;1)` keeps the name `RAIZ`).
fn takes_function(function: &str, index: usize, count: usize) -> bool {
    let position = match function.to_ascii_uppercase().as_str() {
        "MAP" => count.checked_sub(1),
        "BYROW" | "BYCOL" => Some(1),
        "REDUCE" | "SCAN" | "MAKEARRAY" | "GROUPBY" => Some(2),
        "PIVOTBY" => Some(3),
        _ => None,
    };
    position == Some(index)
}

/// A function given by name where a function is expected, as in `MAP({1;4};RAIZ)`, is
/// translated like a call. Booleans, bound names and names that are no function's are left alone.
fn function_argument(arg: &mut Expr, display: bool, bound: &impl Fn(&str) -> bool) -> Result<(), ParseError> {
    let Expr::Name(name) = arg else { return Ok(()) };
    if bound(name) || boolean_alias(name).is_some() {
        return Ok(());
    }
    let es = FormulaLocale::Es;
    let other = if display { es.function_name(name) } else { es.canonical_name(name) };
    if !crate::same_name(other, name) {
        *name = translate_function(name, display, bound)?;
    }
    Ok(())
}

fn normalize(expr: &mut Expr, known_name: &impl Fn(&str) -> bool, bindings: &mut Vec<String>, depth: usize, display: bool) -> Result<(), ParseError> {
    if depth > parser::MAX_DEPTH {
        return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
    }
    let bound = |name: &str, bindings: &[String]| bindings.iter().rev().any(|n| crate::same_name(n, name)) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let alias = if *value { "VERDADERO" } else { "FALSO" };
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !display && !bound(name, bindings) => {
            if let Some(value) = boolean_alias(name) {
                *expr = Expr::Bool(value);
            }
        }
        Expr::Call(name, args) if name == "LET" && args.len() >= 3 && !args.len().is_multiple_of(2) => {
            let saved = bindings.len();
            if let Some((body, pairs)) = args.split_last_mut() {
                for pair in pairs.chunks_mut(2) {
                    if let [declaration, value] = pair {
                        // The new name is not in scope in its own value expression.
                        normalize(value, known_name, bindings, depth + 1, display)?;
                        if let Expr::Name(name) = declaration {
                            bindings.push(name.clone());
                        }
                    }
                }
                normalize(body, known_name, bindings, depth + 1, display)?;
            }
            bindings.truncate(saved);
        }
        Expr::Call(name, args) if name == "LAMBDA" => {
            let saved = bindings.len();
            if let Some((body, params)) = args.split_last_mut() {
                for param in params {
                    if let Expr::Name(name) = param {
                        bindings.push(name.clone());
                    }
                }
                normalize(body, known_name, bindings, depth + 1, display)?;
            }
            bindings.truncate(saved);
        }
        Expr::Call(name, args) => {
            let mut canonical = name.clone();
            if !bound(name, bindings) {
                *name = translate_function(name, display, &|n| bound(n, bindings))?;
                if !display {
                    canonical.clone_from(name);
                }
            }
            let count = args.len();
            for (i, arg) in args.iter_mut().enumerate() {
                if takes_function(&canonical, i, count) {
                    function_argument(arg, display, &|n| bound(n, bindings))?;
                }
                normalize(arg, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Invoke(callee, args) => {
            normalize(callee, known_name, bindings, depth + 1, display)?;
            for arg in args {
                normalize(arg, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Unary(_, child) | Expr::Paren(child) => normalize(child, known_name, bindings, depth + 1, display)?,
        Expr::Binary(..) => {
            // Follow the left spine of a flat chain (A1+A2+…) in a loop: it is not nesting.
            // Each call leaves `bindings` as it found them, so the order does not matter.
            let mut node = expr;
            while let Expr::Binary(_, left, right) = node {
                normalize(right, known_name, bindings, depth + 1, display)?;
                node = left;
            }
            normalize(node, known_name, bindings, depth, display)?;
        }
        Expr::Array(rows) if display => {
            for row in rows {
                for child in row {
                    normalize(child, known_name, bindings, depth + 1, display)?;
                }
            }
        }
        // Array elements have already been parsed as literals. Sheet/structured references
        // contain user-defined labels and must not be translated.
        _ => {}
    }
    Ok(())
}
