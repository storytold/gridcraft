//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::input_de::{GERMAN, GERMAN_ERRORS};
use crate::{Expr, ParseError, parser};
use gridcraft_core::CellError;

/// Formula-entry syntax selected by the interface language. Interface languages without a
/// translation table use the canonical English syntax.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    De,
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

impl FormulaLocale {
    pub fn list_separator(self) -> char {
        match self {
            Self::En => ',',
            Self::Es | Self::De => ';',
        }
    }
    pub fn decimal_separator(self) -> char {
        match self {
            Self::En => '.',
            Self::Es | Self::De => ',',
        }
    }
    pub fn array_column_separator(self) -> char {
        match self {
            Self::En => ',',
            Self::Es => '\\',
            Self::De => '.',
        }
    }
    /// Canonical → localized function names; English has none.
    fn names(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::De => GERMAN,
        }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        self.names().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        self.names().iter().find(|(_, local)| local.eq_ignore_ascii_case(localized)).map(|(en, _)| *en).unwrap_or(localized)
    }
    /// The error value as this language writes it (`#WERT!`); untranslated errors are canonical.
    pub fn error_name(self, error: CellError) -> &'static str {
        let names: &[(CellError, &str)] = match self {
            Self::De => GERMAN_ERRORS,
            Self::En | Self::Es => &[],
        };
        names.iter().find(|(e, _)| *e == error).map(|(_, name)| *name).unwrap_or(error.as_str())
    }
    /// The localized TRUE or FALSE, taken from the function table (TRUE() and FALSE() share them).
    fn boolean_name(self, value: bool) -> &'static str {
        let canonical = if value { "TRUE" } else { "FALSE" };
        self.names().iter().find(|(en, _)| *en == canonical).map(|(_, local)| *local).unwrap_or(canonical)
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale != FormulaLocale::En {
        normalize(&mut expr, locale, &known_name, &mut Vec::new(), 0, false)?;
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
    normalize(&mut expr, locale, &known_name, &mut Vec::new(), 0, true)?;
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
            Tok::Error(error) => out.push_str(locale.error_name(error)),
            Tok::Comma => out.push(if array_depth > 0 { locale.array_column_separator() } else { locale.list_separator() }),
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    if locale == FormulaLocale::En {
        None
    } else if name.eq_ignore_ascii_case(locale.boolean_name(true)) {
        Some(true)
    } else if name.eq_ignore_ascii_case(locale.boolean_name(false)) {
        Some(false)
    } else {
        None
    }
}

fn normalize(
    expr: &mut Expr,
    locale: FormulaLocale,
    known_name: &impl Fn(&str) -> bool,
    bindings: &mut Vec<String>,
    depth: usize,
    display: bool,
) -> Result<(), ParseError> {
    if depth > parser::MAX_DEPTH {
        return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
    }
    let bound = |name: &str, bindings: &[String]| bindings.iter().rev().any(|n| crate::same_name(n, name)) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let alias = locale.boolean_name(*value);
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !display && !bound(name, bindings) => {
            if let Some(value) = boolean_alias(name, locale) {
                *expr = Expr::Bool(value);
            }
        }
        Expr::Call(name, args) if name == "LET" && args.len() >= 3 && !args.len().is_multiple_of(2) => {
            let saved = bindings.len();
            if let Some((body, pairs)) = args.split_last_mut() {
                for pair in pairs.chunks_mut(2) {
                    if let [declaration, value] = pair {
                        // The new name is not in scope in its own value expression.
                        normalize(value, locale, known_name, bindings, depth + 1, display)?;
                        if let Expr::Name(name) = declaration {
                            bindings.push(name.clone());
                        }
                    }
                }
                normalize(body, locale, known_name, bindings, depth + 1, display)?;
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
                normalize(body, locale, known_name, bindings, depth + 1, display)?;
            }
            bindings.truncate(saved);
        }
        Expr::Call(name, args) => {
            let alias = if display { locale.function_name(name) } else { locale.canonical_name(name) };
            if !bound(name, bindings) && (!display || !bound(alias, bindings)) {
                *name = alias.to_string();
            }
            for arg in args {
                normalize(arg, locale, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Invoke(callee, args) => {
            normalize(callee, locale, known_name, bindings, depth + 1, display)?;
            for arg in args {
                normalize(arg, locale, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Unary(_, child) | Expr::Paren(child) => normalize(child, locale, known_name, bindings, depth + 1, display)?,
        Expr::Binary(..) => {
            // Follow the left spine of a flat chain (A1+A2+…) in a loop: it is not nesting.
            // Each call leaves `bindings` as it found them, so the order does not matter.
            let mut node = expr;
            while let Expr::Binary(_, left, right) = node {
                normalize(right, locale, known_name, bindings, depth + 1, display)?;
                node = left;
            }
            normalize(node, locale, known_name, bindings, depth, display)?;
        }
        Expr::Array(rows) if display => {
            for row in rows {
                for child in row {
                    normalize(child, locale, known_name, bindings, depth + 1, display)?;
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
mod tests {
    use super::*;

    #[test]
    fn every_german_name_round_trips() {
        for (en, de) in GERMAN {
            let canonical = format!("{en}(1)");
            let parsed = parse_input(&format!("{de}(1)"), FormulaLocale::De, |_| false).unwrap();
            assert_eq!(crate::print(&parsed), canonical, "{de}");
            assert_eq!(print_input(&parsed, FormulaLocale::De, |_| false).unwrap(), format!("{de}(1)"), "{en}");
        }
    }
}
