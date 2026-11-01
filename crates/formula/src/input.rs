//! Localized aliases accepted at user-entry boundaries; file syntax stays English.
//!
//! Each formula language is a [`Dialect`]: function names, booleans, error literals and
//! structured-reference specifiers as that language's Excel spells them, and its separators.
//! Functions a language doesn't list keep their English name, and English names are accepted in
//! every language.

use gridcraft_core::CellError;

use crate::{Expr, ParseError, parser, same_name};

mod es;

/// Formula-entry syntax selected by the interface language. Interface languages without a
/// formula table use [`FormulaLocale::En`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
}

/// One formula language's words and separators.
pub(crate) struct Dialect {
    list: char,
    decimal: char,
    array_column: char,
    array_row: char,
    bool_true: &'static str,
    bool_false: &'static str,
    /// Every error literal, as this language spells it.
    errors: &'static [(CellError, &'static str)],
    /// The local spellings of [`TABLE_ITEMS`], in the same order.
    table_items: [&'static str; 5],
    /// `(canonical, local)` for the functions spelled differently from English, sorted by the
    /// canonical name.
    functions: &'static [(&'static str, &'static str)],
}

/// Structured-reference specifiers in canonical syntax.
const TABLE_ITEMS: [&str; 5] = ["#All", "#Data", "#Headers", "#Totals", "#This Row"];

impl FormulaLocale {
    pub const ALL: [Self; 2] = [Self::En, Self::Es];

    /// The language's table; `None` for English, which is the canonical syntax itself.
    fn dialect(self) -> Option<&'static Dialect> {
        match self {
            Self::En => None,
            Self::Es => Some(&es::DIALECT),
        }
    }

    /// Separates function arguments (`,` in English, `;` in Spanish).
    pub fn list_separator(self) -> char {
        self.dialect().map_or(',', |d| d.list)
    }
    /// The decimal mark in typed numbers (`.` in English, `,` in Spanish).
    pub fn decimal_separator(self) -> char {
        self.dialect().map_or('.', |d| d.decimal)
    }
    /// Separates the columns of an array constant (`,` in English, `\` in Spanish).
    pub fn array_column_separator(self) -> char {
        self.dialect().map_or(',', |d| d.array_column)
    }
    /// Separates the rows of an array constant (`;` in English and Spanish).
    pub fn array_row_separator(self) -> char {
        self.dialect().map_or(';', |d| d.array_row)
    }

    /// The local name of a canonical function name (itself when the language keeps English).
    pub fn function_name(self, canonical: &str) -> &str {
        self.dialect()
            .and_then(|d| d.functions.binary_search_by(|(en, _)| cmp_ascii_ignore_case(en, canonical)).ok().and_then(|i| d.functions.get(i)))
            .map_or(canonical, |(_, local)| *local)
    }

    /// The canonical name of a local function name, case-insensitively (itself when it isn't one).
    pub fn canonical_name(self, localized: &str) -> &str {
        self.dialect().and_then(|d| d.functions.iter().find(|(_, local)| same_name(local, localized))).map_or(localized, |(en, _)| *en)
    }

    /// `(canonical, local)` for every function this language spells differently from English,
    /// sorted by the canonical name.
    pub fn translations(self) -> &'static [(&'static str, &'static str)] {
        self.dialect().map_or(&[], |d| d.functions)
    }

    /// How this language spells a boolean (`TRUE`, `VERDADERO`).
    pub fn boolean_name(self, value: bool) -> &'static str {
        match (self.dialect(), value) {
            (Some(d), true) => d.bool_true,
            (Some(d), false) => d.bool_false,
            (None, true) => "TRUE",
            (None, false) => "FALSE",
        }
    }

    /// The boolean a local word spells, case-insensitively. English `TRUE`/`FALSE` are lexed as
    /// booleans in every language, so only the local words are matched here.
    pub(crate) fn boolean(self, word: &str) -> Option<bool> {
        let d = self.dialect()?;
        if same_name(word, d.bool_true) {
            Some(true)
        } else if same_name(word, d.bool_false) {
            Some(false)
        } else {
            None
        }
    }

    /// How this language spells an error literal (`#VALUE!`, `#¡VALOR!`).
    pub fn error_name(self, error: CellError) -> &'static str {
        self.dialect().and_then(|d| d.errors.iter().find(|(e, _)| *e == error)).map_or_else(|| error.as_str(), |(_, local)| *local)
    }

    /// The longest local error literal `text` starts with, and its length in characters.
    pub(crate) fn error_prefix(self, text: &str) -> Option<(CellError, usize)> {
        let d = self.dialect()?;
        d.errors
            .iter()
            .filter(|(_, local)| starts_with_ignore_case(text, local))
            .map(|(e, local)| (*e, local.chars().count()))
            .max_by_key(|(_, len)| *len)
    }

    /// Converts the inside of a local structured reference (`[...]` without its outer brackets)
    /// to canonical syntax: specifiers in English, `,` between bracketed items.
    pub(crate) fn struct_to_canonical(self, inner: &str) -> String {
        match self.dialect() {
            Some(d) => translate_struct(inner, &d.table_items, &TABLE_ITEMS, d.list, ','),
            None => inner.to_string(),
        }
    }

    /// Converts the inside of a canonical structured reference to this language.
    pub(crate) fn struct_to_local(self, inner: &str) -> String {
        match self.dialect() {
            Some(d) => translate_struct(inner, &TABLE_ITEMS, &d.table_items, ',', d.list),
            None => inner.to_string(),
        }
    }
}

/// ASCII case-insensitive ordering, the order the language tables are sorted in.
fn cmp_ascii_ignore_case(a: &str, b: &str) -> std::cmp::Ordering {
    a.bytes().map(|c| c.to_ascii_uppercase()).cmp(b.bytes().map(|c| c.to_ascii_uppercase()))
}

/// Whether `text` starts with `prefix`, compared case-insensitively in any script.
fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.chars().take(prefix.chars().count()).flat_map(char::to_uppercase).eq(prefix.chars().flat_map(char::to_uppercase))
}

/// Rewrites specifiers (`#Headers` <-> `#Kopfzeilen`) and the separator between bracketed
/// items, leaving column names and `'` escapes alone.
fn translate_struct(inner: &str, from: &[&'static str; 5], to: &[&'static str; 5], from_sep: char, to_sep: char) -> String {
    let item = |text: &str| -> Option<&'static str> { from.iter().position(|f| same_name(text.trim(), f)).and_then(|i| to.get(i).copied()) };
    // Excel allows spaces around bracketed items (`Tabla1[ [#Datos]; [Col1] ]`).
    if !inner.trim_start().starts_with('[') {
        return item(inner).map_or_else(|| inner.to_string(), str::to_string);
    }
    let mut out = String::with_capacity(inner.len());
    let mut group = String::new();
    let mut depth = 0usize;
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let target = if depth > 0 { &mut group } else { &mut out };
                target.push(c);
                if let Some(escaped) = chars.next() {
                    target.push(escaped);
                }
            }
            '[' => {
                if depth == 0 {
                    group.clear();
                } else {
                    group.push(c);
                }
                depth += 1;
            }
            ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    out.push('[');
                    out.push_str(item(&group).unwrap_or(&group));
                    out.push(']');
                } else {
                    group.push(c);
                }
            }
            c if depth == 0 && c == from_sep => out.push(to_sep),
            c if depth > 0 => group.push(c),
            c => out.push(c),
        }
    }
    if depth > 0 {
        // Unbalanced input: keep what was read so the parser reports the error.
        out.push('[');
        out.push_str(&group);
    }
    out
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
            Tok::Semicolon if array_depth > 0 => out.push(locale.array_row_separator()),
            Tok::Error(e) => out.push_str(locale.error_name(e)),
            Tok::Struct(inner) => {
                out.push('[');
                out.push_str(&locale.struct_to_local(&inner));
                out.push(']');
            }
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

/// Rewrites booleans and function names between the language and canonical syntax, recursively.
/// `display` goes canonical → local, otherwise local → canonical. LET/LAMBDA bindings and names
/// `known_name` accepts are never rewritten, so a binding may share a local function's spelling.
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
    // Names ignore case in any script, so a binding `zählenwenn` shadows the call `ZÄHLENWENN(…)`.
    let bound = |name: &str, bindings: &[String]| bindings.iter().rev().any(|n| same_name(n, name)) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let alias = locale.boolean_name(*value);
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !display && !bound(name, bindings) => {
            if let Some(value) = locale.boolean(name) {
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
                // A worksheet function passed by name (`BYROW(A1:C3;SOMA)`, saved as `_xleta.SUM`)
                // is spelled like its call. A local boolean word stays a boolean.
                if let Expr::Name(function) = arg
                    && !bound(function, bindings)
                    && (display || locale.boolean(function).is_none())
                {
                    let alias = if display { locale.function_name(function) } else { locale.canonical_name(function) };
                    // The canonical name must not become a reference to a binding or defined name
                    // (`LET(SUM;42;ESERROR(SUMA))` keeps `SUMA`).
                    if alias != function.as_str() && !bound(alias, bindings) && (!display || locale.boolean(alias).is_none()) {
                        *function = alias.to_string();
                        continue;
                    }
                }
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
