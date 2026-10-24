//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};
use gridcraft_core::CellError;

mod french;

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Fr,
}

const SPANISH: &[(&str, &str)] = &[
    ("SUM", "SUMA"),
    ("AVERAGE", "PROMEDIO"),
    ("IF", "SI"),
    ("COUNT", "CONTAR"),
    ("SUMIF", "SUMAR.SI"),
    ("COUNTIF", "CONTAR.SI"),
    ("VLOOKUP", "BUSCARV"),
    ("TODAY", "HOY"),
    // Financial block (#1986): the loan-schedule and investment staples.
    ("NPV", "VNA"),
    ("IRR", "TIR"),
    ("PMT", "PAGO"),
    ("PV", "VA"),
    ("FV", "VF"),
    ("TRUE", "VERDADERO"),
    ("FALSE", "FALSO"),
];

/// French error spellings, following PR #2310; GridCraft's own #CIRC! stays unchanged.
const FRENCH_ERRORS: &[(CellError, &str)] = &[
    (CellError::Null, "#NUL!"),
    (CellError::Div0, "#DIV/0!"),
    (CellError::Value, "#VALEUR!"),
    (CellError::Ref, "#REF!"),
    (CellError::Name, "#NOM?"),
    (CellError::Num, "#NOMBRE!"),
    (CellError::NA, "#N/A"),
    (CellError::GettingData, "#CHARGEMENT_DONNEES"),
    (CellError::Spill, "#PROPAGATION!"),
    (CellError::Calc, "#CALC!"),
];

impl FormulaLocale {
    pub fn list_separator(self) -> char {
        if self != Self::En { ';' } else { ',' }
    }
    pub fn decimal_separator(self) -> char {
        if self != Self::En { ',' } else { '.' }
    }
    pub fn array_column_separator(self) -> char {
        if self != Self::En { '\\' } else { ',' }
    }
    /// Returns canonical-to-localized function names for this formula locale.
    pub fn aliases(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::Fr => french::FRENCH,
        }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        self.aliases().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        self.aliases().iter().find(|(_, local)| local.eq_ignore_ascii_case(localized)).map(|(en, _)| *en).unwrap_or(localized)
    }
    /// Localized error literals; unlisted errors keep their canonical spelling.
    pub(crate) fn errors(self) -> &'static [(CellError, &'static str)] {
        match self {
            Self::Fr => FRENCH_ERRORS,
            Self::En | Self::Es => &[],
        }
    }
    /// The error spelling used when displaying a formula in this language.
    pub fn error_name(self, error: CellError) -> &'static str {
        self.errors().iter().find(|(e, _)| *e == error).map(|(_, name)| *name).unwrap_or(error.as_str())
    }
    /// Reads a localized or canonical error value, case-insensitively.
    pub fn parse_error(self, text: &str) -> Option<CellError> {
        self.errors().iter().find(|(_, name)| name.eq_ignore_ascii_case(text)).map(|(e, _)| *e).or_else(|| CellError::parse(text))
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale != FormulaLocale::En {
        normalize(&mut expr, &known_name, &mut Vec::new(), 0, false, locale)?;
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
    normalize(&mut expr, &known_name, &mut Vec::new(), 0, true, locale)?;
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
            Tok::Error(e) => out.push_str(locale.error_name(e)),
            Tok::Number(_) => out.push_str(&text.replace('.', &locale.decimal_separator().to_string())),
            Tok::Comma => out.push(if array_depth > 0 { locale.array_column_separator() } else { locale.list_separator() }),
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    match locale.canonical_name(name).to_ascii_uppercase().as_str() {
        "TRUE" => Some(true),
        "FALSE" => Some(false),
        _ => None,
    }
}

fn normalize(
    expr: &mut Expr,
    known_name: &impl Fn(&str) -> bool,
    bindings: &mut Vec<String>,
    depth: usize,
    display: bool,
    locale: FormulaLocale,
) -> Result<(), ParseError> {
    if depth > parser::MAX_DEPTH {
        return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
    }
    let bound = |name: &str, bindings: &[String]| bindings.iter().rev().any(|n| crate::same_name(n, name)) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let alias = locale.function_name(if *value { "TRUE" } else { "FALSE" });
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !bound(name, bindings) => {
            if !display && let Some(value) = boolean_alias(name, locale) {
                *expr = Expr::Bool(value);
            } else if locale == FormulaLocale::Fr {
                // GROUPBY/PIVOTBY and LAMBDA helpers also accept functions by name.
                let alias = if display { locale.function_name(name) } else { locale.canonical_name(name) };
                if !display || !bound(alias, bindings) {
                    *name = alias.to_string();
                }
            }
        }
        Expr::Call(name, args) if name == "LET" && args.len() >= 3 && !args.len().is_multiple_of(2) => {
            let saved = bindings.len();
            if let Some((body, pairs)) = args.split_last_mut() {
                for pair in pairs.chunks_mut(2) {
                    if let [declaration, value] = pair {
                        // The new name is not in scope in its own value expression.
                        normalize(value, known_name, bindings, depth + 1, display, locale)?;
                        if let Expr::Name(name) = declaration {
                            bindings.push(name.clone());
                        }
                    }
                }
                normalize(body, known_name, bindings, depth + 1, display, locale)?;
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
                normalize(body, known_name, bindings, depth + 1, display, locale)?;
            }
            bindings.truncate(saved);
        }
        Expr::Call(name, args) => {
            let alias = if display { locale.function_name(name) } else { locale.canonical_name(name) };
            if !bound(name, bindings) && (!display || !bound(alias, bindings)) {
                *name = alias.to_string();
            }
            for arg in args {
                normalize(arg, known_name, bindings, depth + 1, display, locale)?;
            }
        }
        Expr::Invoke(callee, args) => {
            normalize(callee, known_name, bindings, depth + 1, display, locale)?;
            for arg in args {
                normalize(arg, known_name, bindings, depth + 1, display, locale)?;
            }
        }
        Expr::Unary(_, child) | Expr::Paren(child) => normalize(child, known_name, bindings, depth + 1, display, locale)?,
        Expr::Binary(..) => {
            // Follow the left spine of a flat chain (A1+A2+…) in a loop: it is not nesting.
            // Each call leaves `bindings` as it found them, so the order does not matter.
            let mut node = expr;
            while let Expr::Binary(_, left, right) = node {
                normalize(right, known_name, bindings, depth + 1, display, locale)?;
                node = left;
            }
            normalize(node, known_name, bindings, depth, display, locale)?;
        }
        Expr::Array(rows) if display => {
            for row in rows {
                for child in row {
                    normalize(child, known_name, bindings, depth + 1, display, locale)?;
                }
            }
        }
        // Array elements have already been parsed as literals. Sheet/structured references
        // contain user-defined labels and must not be translated.
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod french_tests {
    use super::*;

    #[test]
    fn every_french_name_is_unique_and_round_trips() {
        let mut canonical = std::collections::HashSet::new();
        let mut localized = std::collections::HashSet::new();
        for &(en, fr) in french::FRENCH {
            assert!(canonical.insert(en), "duplicate canonical name: {en}");
            assert!(localized.insert(fr), "ambiguous French name: {fr}");
            assert_eq!(FormulaLocale::Fr.function_name(en), fr);
            assert_eq!(FormulaLocale::Fr.canonical_name(&fr.to_ascii_lowercase()), en);
            let expr = crate::parse(&format!("{en}(1.5,\"a,b;c\")")).unwrap();
            let shown = print_input(&expr, FormulaLocale::Fr, |_| false).unwrap();
            assert_eq!(shown, format!("{fr}(1,5;\"a,b;c\")"));
            assert_eq!(crate::print(&parse_input(&shown, FormulaLocale::Fr, |_| false).unwrap()), crate::print(&expr), "{en}");
        }
        assert_eq!(canonical.len(), 514);
    }
}
