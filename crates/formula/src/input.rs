//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};
use gridcraft_core::CellError;

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Pl,
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

mod pl;

impl FormulaLocale {
    /// `(English, localized)` function names; functions not listed keep their English name.
    pub fn aliases(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::Pl => pl::FUNCTIONS,
        }
    }
    /// Localized error values, shown and accepted (Spanish keeps the English spellings).
    fn error_names(self) -> &'static [(CellError, &'static str)] {
        if self == Self::Pl { pl::ERRORS } else { &[] }
    }
    fn table_items(self) -> &'static [(&'static str, &'static str)] {
        if self == Self::Pl { pl::TABLE_ITEMS } else { &[] }
    }
    pub fn error_name(self, error: CellError) -> &'static str {
        self.error_names().iter().find(|(e, _)| *e == error).map(|(_, local)| *local).unwrap_or(error.as_str())
    }
    /// The error literal `src` starts with in this locale and its length in chars. English
    /// spellings stay accepted, as English function names do.
    pub fn error_prefix(self, src: &str) -> Option<(CellError, usize)> {
        let aliases = if self == Self::Pl { pl::ERROR_ALIASES } else { &[] };
        let localized = aliases.iter().chain(self.error_names()).map(|(e, t)| (*e, *t));
        localized.chain(CellError::ALL.iter().map(|e| (*e, e.as_str()))).find_map(|(e, t)| {
            let len = t.chars().count();
            let head: String = src.chars().take(len).collect();
            crate::same_name(&head, t).then_some((e, len))
        })
    }
    /// Converts the inside of a structured reference (`#Dane`, `[#Nagłówki];[Kol]`) to canonical
    /// syntax (`display` false) or to this locale: the `#` specifiers and the separator between
    /// bracketed parts. Column names are left alone.
    pub fn table_reference(self, body: &str, display: bool) -> String {
        let items = self.table_items();
        if items.is_empty() {
            return body.to_string();
        }
        let swap = |item: &str| -> Option<&'static str> {
            let item = item.trim();
            items
                .iter()
                .find(|(en, local)| crate::same_name(item, if display { en } else { local }))
                .map(|(en, local)| if display { *local } else { *en })
        };
        if let Some(whole) = swap(body) {
            return whole.to_string();
        }
        let (from, to) = if display { (',', self.list_separator()) } else { (self.list_separator(), ',') };
        // A bare column holding the local separator (`[Kwota;netto]`) gets brackets, or entering
        // it again would split it in two.
        if display && !body.contains('[') && body.contains(to) {
            return match body.strip_prefix('@') {
                Some(column) => format!("@[{column}]"),
                None => format!("[{body}]"),
            };
        }
        let mut out = String::with_capacity(body.len());
        let mut part = String::new();
        let mut depth = 0usize;
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            match c {
                // `'` escapes the next character in column names.
                '\'' => {
                    let target = if depth > 0 { &mut part } else { &mut out };
                    target.push(c);
                    if let Some(n) = chars.next() {
                        target.push(n);
                    }
                }
                '[' => {
                    depth += 1;
                    if depth == 1 {
                        part.clear();
                    } else {
                        part.push(c);
                    }
                }
                ']' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        out.push('[');
                        out.push_str(swap(&part).unwrap_or(&part));
                        out.push(']');
                    } else {
                        part.push(c);
                    }
                }
                _ if depth > 0 => part.push(c),
                c if c == from => out.push(to),
                c => out.push(c),
            }
        }
        if depth > 0 {
            // Unbalanced: the parser reports it on the original text.
            return body.to_string();
        }
        out
    }
    /// The localized spellings of TRUE and FALSE.
    fn boolean_names(self) -> (&'static str, &'static str) {
        match self {
            Self::En => ("TRUE", "FALSE"),
            Self::Es => ("VERDADERO", "FALSO"),
            Self::Pl => ("PRAWDA", "FAŁSZ"),
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
        self.aliases().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        self.aliases().iter().find(|(_, local)| crate::same_name(local, localized)).map(|(en, _)| *en).unwrap_or(localized)
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
            Tok::Error(e) => out.push_str(locale.error_name(e)),
            Tok::Struct(body) => {
                out.push('[');
                out.push_str(&locale.table_reference(&body, true));
                out.push(']');
            }
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    let (truth, falsity) = locale.boolean_names();
    if locale == FormulaLocale::En {
        None
    } else if crate::same_name(name, truth) {
        Some(true)
    } else if crate::same_name(name, falsity) {
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
            let (truth, falsity) = locale.boolean_names();
            let alias = if *value { truth } else { falsity };
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
