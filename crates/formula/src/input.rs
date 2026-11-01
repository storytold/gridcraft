//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use gridcraft_core::CellError;
use std::collections::HashMap;
use std::sync::OnceLock;

mod en;
mod es;
mod ru;

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Ru,
}

/// Per-locale data consumed by one parser/printer and one UI boundary.
struct LocaleData {
    functions: &'static [(&'static str, &'static str)],
    errors: &'static [(CellError, &'static str)],
    separators: Separators,
    fold_alias: fn(&str) -> String,
    aliases: OnceLock<HashMap<String, &'static str>>,
}

#[derive(Clone, Copy)]
struct Separators {
    decimal: char,
    list: char,
    array_column: char,
    array_row: char,
}

const ENGLISH_SEPARATORS: Separators = Separators { decimal: '.', list: ',', array_column: ',', array_row: ';' };
const COMMA_DECIMAL_SEPARATORS: Separators = Separators { decimal: ',', list: ';', array_column: '\\', array_row: ';' };

/// Preserve a token until alias lookup if canonical ASCII casing changes its locale identity.
fn normalize_function_case(name: &str, fold: fn(&str) -> String) -> String {
    let upper = name.to_ascii_uppercase();
    if upper == name || fold(name) == fold(&upper) { parser::normalize_function_name(name) } else { name.to_string() }
}

impl FormulaLocale {
    /// Returns the static syntax tables and alias cache for this formula locale.
    fn data(self) -> &'static LocaleData {
        match self {
            Self::En => &en::DATA,
            Self::Es => &es::DATA,
            Self::Ru => &ru::DATA,
        }
    }

    /// Separates function arguments and reference unions.
    pub fn list_separator(self) -> char {
        self.data().separators.list
    }
    /// Separates the integer and fractional parts of numeric literals.
    pub fn decimal_separator(self) -> char {
        self.data().separators.decimal
    }
    /// Separates columns inside array constants.
    pub fn array_column_separator(self) -> char {
        self.data().separators.array_column
    }
    /// Separates rows inside array constants.
    pub fn array_row_separator(self) -> char {
        self.data().separators.array_row
    }

    /// Whether a character separates either rows or columns in a localized array constant.
    pub(crate) fn is_array_separator(self, character: char) -> bool {
        character == self.array_column_separator() || character == self.array_row_separator()
    }

    /// Case folding for formula aliases only; workbook names keep their own identity.
    pub fn fold_alias(self, name: &str) -> String {
        (self.data().fold_alias)(name)
    }

    /// Normalizes a function token without erasing locale-specific case distinctions.
    pub fn normalize_function_name(self, name: &str) -> String {
        normalize_function_case(name, self.data().fold_alias)
    }

    /// Preferred local spelling of a built-in; unmapped names keep their canonical spelling.
    pub fn function_name(self, canonical: &str) -> &str {
        self.data().functions.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map_or(canonical, |(_, local)| *local)
    }

    /// Resolves a local alias; unmapped names remain unchanged.
    pub fn canonical_name(self, localized: &str) -> &str {
        let data = self.data();
        if data.functions.is_empty() {
            return localized;
        }
        data.aliases
            .get_or_init(|| data.functions.iter().map(|(en, local)| (self.fold_alias(local), *en)).collect())
            .get(&self.fold_alias(localized))
            .copied()
            .unwrap_or(localized)
    }

    /// Localized error spelling at the editor boundary. Files keep CellError's canonical text.
    pub fn error_name(self, error: CellError) -> &'static str {
        self.data().errors.iter().find(|(value, _)| *value == error).map_or(error.as_str(), |(_, text)| *text)
    }

    /// Both a locale's spelling and the canonical spelling identify the same error value.
    pub fn parse_error(self, text: &str) -> Option<CellError> {
        CellError::parse(text).or_else(|| {
            let folded = self.fold_alias(text);
            self.data().errors.iter().find(|(_, spelling)| self.fold_alias(spelling) == folded).map(|(error, _)| *error)
        })
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
            Tok::Error(error) => out.push_str(locale.error_name(error)),
            Tok::Semicolon if array_depth > 0 => out.push(locale.array_row_separator()),
            Tok::Number(_) => out.push_str(&text.replace('.', &locale.decimal_separator().to_string())),
            Tok::Comma => out.push(if array_depth > 0 { locale.array_column_separator() } else { locale.list_separator() }),
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

/// Resolves a localized Boolean alias; English literals are handled by the canonical parser.
pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    if locale == FormulaLocale::En {
        return None;
    }
    match locale.canonical_name(name) {
        "TRUE" => Some(true),
        "FALSE" => Some(false),
        _ => None,
    }
}

/// Rewrites aliases between canonical and display forms while preserving workbook and local bindings.
/// Bounds nested expressions and walks flat binary chains iteratively; display selects the direction.
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
                *name = if display { alias.to_string() } else { parser::normalize_function_name(alias) };
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
mod case_rule_tests {
    use super::normalize_function_case;

    /// Verifies token normalization preserves distinctions required by a locale-specific case fold.
    #[test]
    fn locale_case_rules_survive_token_normalization() {
        // A data-only example of a case rule where ASCII upper-casing loses information.
        /// Models dotted-I alias folding to exercise a case distinction lost by ASCII uppercasing.
        fn dotted_i_fold(name: &str) -> String {
            name.chars().flat_map(|c| if c == 'i' { 'İ'.to_uppercase() } else { c.to_uppercase() }).collect()
        }
        assert_eq!(normalize_function_case("indis", dotted_i_fold), "indis");
        assert_eq!(normalize_function_case("sum", dotted_i_fold), "SUM");
        assert_eq!(normalize_function_case("sum", str::to_lowercase), "SUM");
        assert_eq!(normalize_function_case("_xlfn.sum", str::to_lowercase), "SUM");
        assert_eq!(normalize_function_case("_XLUDF.sum", str::to_lowercase), "_xludf.SUM");
        assert_eq!(normalize_function_case("İNDİS", dotted_i_fold), "İNDİS");
    }
}
