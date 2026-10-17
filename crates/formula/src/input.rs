//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    It,
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
    ("TRUE", "VERDADERO"),
    ("FALSE", "FALSO"),
];

// Italian Excel names. Functions spelled the same in Italian (MAX, MIN, INT…) need no entry.
const ITALIAN: &[(&str, &str)] = &[
    ("SUM", "SOMMA"),
    ("AVERAGE", "MEDIA"),
    ("IF", "SE"),
    ("COUNT", "CONTA.NUMERI"),
    ("COUNTA", "CONTA.VALORI"),
    ("COUNTBLANK", "CONTA.VUOTE"),
    ("SUMIF", "SOMMA.SE"),
    ("SUMIFS", "SOMMA.PIÙ.SE"),
    ("COUNTIF", "CONTA.SE"),
    ("COUNTIFS", "CONTA.PIÙ.SE"),
    ("AVERAGEIF", "MEDIA.SE"),
    ("IFERROR", "SE.ERRORE"),
    ("IFS", "PIÙ.SE"),
    ("AND", "E"),
    ("OR", "O"),
    ("NOT", "NON"),
    ("VLOOKUP", "CERCA.VERT"),
    ("HLOOKUP", "CERCA.ORIZZ"),
    ("XLOOKUP", "CERCA.X"),
    ("INDEX", "INDICE"),
    ("MATCH", "CONFRONTA"),
    ("ROUND", "ARROTONDA"),
    ("ROUNDUP", "ARROTONDA.PER.ECC"),
    ("ROUNDDOWN", "ARROTONDA.PER.DIF"),
    ("ABS", "ASS"),
    ("MOD", "RESTO"),
    ("POWER", "POTENZA"),
    ("SQRT", "RADQ"),
    ("PRODUCT", "PRODOTTO"),
    ("SUMPRODUCT", "MATR.SOMMA.PRODOTTO"),
    ("CONCATENATE", "CONCATENA"),
    ("LEFT", "SINISTRA"),
    ("RIGHT", "DESTRA"),
    ("MID", "STRINGA.ESTRAI"),
    ("LEN", "LUNGHEZZA"),
    ("UPPER", "MAIUSC"),
    ("LOWER", "MINUSC"),
    ("TRIM", "ANNULLA.SPAZI"),
    ("TEXT", "TESTO"),
    ("VALUE", "VALORE"),
    ("TODAY", "OGGI"),
    ("NOW", "ADESSO"),
    ("DATE", "DATA"),
    ("YEAR", "ANNO"),
    ("MONTH", "MESE"),
    ("DAY", "GIORNO"),
    ("TRUE", "VERO"),
    ("FALSE", "FALSO"),
];

/// Case-insensitive name comparison that also folds non-ASCII letters (PIÙ / più).
fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || (!a.is_ascii() && a.to_uppercase() == b.to_uppercase())
}

impl FormulaLocale {
    /// Canonical English → localized function names; empty for English.
    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::It => ITALIAN,
        }
    }
    pub fn list_separator(self) -> char {
        if self == Self::En { ',' } else { ';' }
    }
    pub fn decimal_separator(self) -> char {
        if self == Self::En { '.' } else { ',' }
    }
    pub fn array_column_separator(self) -> char {
        if self == Self::En { ',' } else { '\\' }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        self.table().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        self.table().iter().find(|(_, local)| same_name(local, localized)).map(|(en, _)| *en).unwrap_or(localized)
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale != FormulaLocale::En {
        normalize(locale, &mut expr, &known_name, &mut Vec::new(), 0, false)?;
    }
    Ok(expr)
}

/// Prints canonical expressions for editing. Name collisions retain the canonical spelling
/// so displaying then committing a formula never changes a defined-name reference.
pub fn print_input(expr: &Expr, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<String, ParseError> {
    // Check before cloning/printing: a flat operator chain can produce a deep AST even
    // though the parser itself did not recurse deeply.
    let mut pending = vec![(expr, 0usize)];
    while let Some((node, depth)) = pending.pop() {
        if depth > parser::MAX_DEPTH {
            return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
        }
        match node {
            Expr::Unary(_, child) | Expr::Paren(child) => pending.push((child, depth + 1)),
            Expr::Binary(_, left, right) => {
                pending.push((left, depth + 1));
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
    normalize(locale, &mut expr, &known_name, &mut Vec::new(), 0, true)?;
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

pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    if locale == FormulaLocale::En {
        None
    } else if same_name(locale.function_name("TRUE"), name) {
        Some(true)
    } else if same_name(locale.function_name("FALSE"), name) {
        Some(false)
    } else {
        None
    }
}

fn normalize(
    locale: FormulaLocale,
    expr: &mut Expr,
    known_name: &impl Fn(&str) -> bool,
    bindings: &mut Vec<String>,
    depth: usize,
    display: bool,
) -> Result<(), ParseError> {
    if depth > parser::MAX_DEPTH {
        return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
    }
    let bound = |name: &str, bindings: &[String]| bindings.iter().rev().any(|n| n.eq_ignore_ascii_case(name)) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let alias = locale.function_name(if *value { "TRUE" } else { "FALSE" });
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
                        normalize(locale, value, known_name, bindings, depth + 1, display)?;
                        if let Expr::Name(name) = declaration {
                            bindings.push(name.clone());
                        }
                    }
                }
                normalize(locale, body, known_name, bindings, depth + 1, display)?;
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
                normalize(locale, body, known_name, bindings, depth + 1, display)?;
            }
            bindings.truncate(saved);
        }
        Expr::Call(name, args) => {
            let alias = if display { locale.function_name(name) } else { locale.canonical_name(name) };
            if !bound(name, bindings) && (!display || !bound(alias, bindings)) {
                *name = alias.to_string();
            }
            for arg in args {
                normalize(locale, arg, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Invoke(callee, args) => {
            normalize(locale, callee, known_name, bindings, depth + 1, display)?;
            for arg in args {
                normalize(locale, arg, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Unary(_, child) | Expr::Paren(child) => normalize(locale, child, known_name, bindings, depth + 1, display)?,
        Expr::Binary(_, left, right) => {
            normalize(locale, left, known_name, bindings, depth + 1, display)?;
            normalize(locale, right, known_name, bindings, depth + 1, display)?;
        }
        Expr::Array(rows) if display => {
            for row in rows {
                for child in row {
                    normalize(locale, child, known_name, bindings, depth + 1, display)?;
                }
            }
        }
        // Array elements have already been parsed as literals. Sheet/structured references
        // contain user-defined labels and must not be translated.
        _ => {}
    }
    Ok(())
}
