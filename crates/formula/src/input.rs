//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    /// Brazilian Portuguese: `SOMA`, `PROCV`, `SE`, decimal commas and `;` arguments.
    PtBr,
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

/// Brazilian Portuguese names, as the Portuguese (Brazil) edition of the target app spells them.
/// Accented names (`MÉDIA`, `CONT.NÚM`, `ÍNDICE`) also match unaccented-case input such as
/// `média`, through [`crate::same_name`].
const PORTUGUESE_BR: &[(&str, &str)] = &[
    ("SUM", "SOMA"),
    ("AVERAGE", "MÉDIA"),
    ("IF", "SE"),
    ("COUNT", "CONT.NÚM"),
    ("COUNTA", "CONT.VALORES"),
    ("SUMIF", "SOMASE"),
    ("SUMIFS", "SOMASES"),
    ("COUNTIF", "CONT.SE"),
    ("COUNTIFS", "CONT.SES"),
    ("SUMPRODUCT", "SOMARPRODUTO"),
    ("VLOOKUP", "PROCV"),
    ("HLOOKUP", "PROCH"),
    ("XLOOKUP", "PROCX"),
    ("INDEX", "ÍNDICE"),
    ("MATCH", "CORRESP"),
    ("ROUND", "ARRED"),
    ("ROUNDUP", "ARREDONDAR.PARA.CIMA"),
    ("ROUNDDOWN", "ARREDONDAR.PARA.BAIXO"),
    ("TRUNC", "TRUNCAR"),
    ("MAX", "MÁXIMO"),
    ("MIN", "MÍNIMO"),
    ("AND", "E"),
    ("OR", "OU"),
    ("NOT", "NÃO"),
    ("IFERROR", "SEERRO"),
    ("CONCATENATE", "CONCATENAR"),
    ("TODAY", "HOJE"),
    ("NOW", "AGORA"),
    // Financial block, as in Spanish (#1986).
    ("NPV", "VPL"),
    ("IRR", "TIR"),
    ("PMT", "PGTO"),
    ("PV", "VP"),
    ("FV", "VF"),
    ("TRUE", "VERDADEIRO"),
    ("FALSE", "FALSO"),
];

impl FormulaLocale {
    /// Canonical English name and localized name of every translated function and boolean.
    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::PtBr => PORTUGUESE_BR,
        }
    }
    /// Separates function arguments and reference unions: `,` in English, `;` otherwise.
    pub fn list_separator(self) -> char {
        if self == Self::En { ',' } else { ';' }
    }
    /// The decimal mark of number literals: `.` in English, `,` otherwise.
    pub fn decimal_separator(self) -> char {
        if self == Self::En { '.' } else { ',' }
    }
    /// Separates columns inside an array constant: `,` in English, `\` otherwise (rows use `;`).
    pub fn array_column_separator(self) -> char {
        if self == Self::En { ',' } else { '\\' }
    }
    /// The localized name of a canonical English function, or `canonical` when untranslated.
    pub fn function_name(self, canonical: &str) -> &str {
        self.table().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    /// The canonical English name of a localized function (any case), or `localized` unchanged.
    pub fn canonical_name(self, localized: &str) -> &str {
        self.table().iter().find(|(_, local)| crate::same_name(local, localized)).map(|(en, _)| *en).unwrap_or(localized)
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
            Tok::Comma => out.push(if array_depth > 0 { locale.array_column_separator() } else { locale.list_separator() }),
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

/// `VERDADERO` / `VERDADEIRO` -> `true`: the localized spelling of a boolean literal.
pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    // Only the localized spelling: an English `TRUE` reaching here (`TRUE[...]`) stays a name.
    match locale.table().iter().find(|(_, local)| crate::same_name(local, name)).map(|(en, _)| *en) {
        Some("TRUE") => Some(true),
        Some("FALSE") => Some(false),
        _ => None,
    }
}

/// Rewrites function names and boolean literals between the canonical and `locale` spellings:
/// to canonical on input, to localized for `display`. Defined names and LET/LAMBDA bindings win.
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
