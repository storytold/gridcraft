//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    /// Russian: localized function names (translated, as in real Russian-interface Excel),
    /// plus the `;` argument / `,` decimal / `;` array-column / `:` array-row separators (see
    /// [`Self::array_row_separator`] - Russian's array syntax is not the same scheme as
    /// Spanish's).
    Ru,
    /// Ukrainian: Ukrainian Excel has never shipped its own translated function-name table
    /// (unlike Russian) - formulas stay in English even under a Ukrainian interface. Only the
    /// separators differ from English: `;` for arguments, `,` for decimals, `;` for array
    /// columns, `:` for array rows - same as Russian. See docs/localization-parity.md for
    /// sourcing.
    Uk,
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

// Russian Excel's own function names, same scope as SPANISH above (the common/financial staples
// from #45's first stage). Needs native-speaker review before being trusted further (see
// docs/localization-parity.md).
const RUSSIAN: &[(&str, &str)] = &[
    ("SUM", "СУММ"),
    ("AVERAGE", "СРЗНАЧ"),
    ("IF", "ЕСЛИ"),
    ("COUNT", "СЧЁТ"),
    ("SUMIF", "СУММЕСЛИ"),
    ("COUNTIF", "СЧЁТЕСЛИ"),
    ("VLOOKUP", "ВПР"),
    ("TODAY", "СЕГОДНЯ"),
    ("NPV", "ЧПС"),
    ("IRR", "ВСД"),
    ("PMT", "ПЛТ"),
    ("PV", "ПС"),
    ("FV", "БС"),
    ("TRUE", "ИСТИНА"),
    ("FALSE", "ЛОЖЬ"),
];

// Empty on purpose: Ukrainian Excel has no localized function names to alias (see the `Uk`
// variant's doc comment). Kept as its own table, rather than reusing `&[]` inline, so a future
// translation (should Microsoft ever ship one, or should this prove wrong for some functions)
// is a one-line change here instead of a new branch everywhere `table()` is matched.
const UKRAINIAN: &[(&str, &str)] = &[];

impl FormulaLocale {
    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::Ru => RUSSIAN,
            Self::Uk => UKRAINIAN,
        }
    }
    pub fn list_separator(self) -> char {
        match self {
            Self::En => ',',
            Self::Es | Self::Ru | Self::Uk => ';',
        }
    }
    pub fn decimal_separator(self) -> char {
        match self {
            Self::En => '.',
            Self::Es | Self::Ru | Self::Uk => ',',
        }
    }
    pub fn array_column_separator(self) -> char {
        match self {
            Self::En => ',',
            Self::Es => '\\',
            Self::Ru | Self::Uk => ';',
        }
    }
    /// The array-literal row separator, independent of [`Self::list_separator`] - they only
    /// happen to share a character in locales whose row separator is `;` (English, Spanish).
    /// Russian's own array syntax uses `;` for columns and `:` for rows instead (confirmed
    /// against Microsoft's own ru-ru documentation and multiple Russian sources,
    /// e.g. `{1;2;3:4;5;6}`) - the opposite pairing from Spanish's `\`/`;`, not the same scheme
    /// copied over. Ukrainian follows Russian here; unconfirmed directly, but Ukrainian has no
    /// source suggesting otherwise and shares every other separator with Russian.
    pub fn array_row_separator(self) -> char {
        match self {
            Self::En | Self::Es => ';',
            Self::Ru | Self::Uk => ':',
        }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        self.table().iter().find(|(en, _)| crate::same_name(en, canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        self.table().iter().find(|(_, local)| crate::same_name(local, localized)).map(|(en, _)| *en).unwrap_or(localized)
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale != FormulaLocale::En {
        normalize(&mut expr, locale, &known_name, &mut Bindings::default(), 0, false)?;
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
    normalize(&mut expr, locale, &known_name, &mut Bindings::default(), 0, true)?;
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
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

impl FormulaLocale {
    /// The boolean literal a bare word spells in this locale, e.g. `VERDADERO`/`FALSO` for
    /// Spanish - looked up from the same alias table as function names, so a locale with no
    /// table (English, Ukrainian) falls back to recognizing the English `TRUE`/`FALSE` words.
    pub(crate) fn boolean_alias(self, name: &str) -> Option<bool> {
        if crate::same_name(name, self.function_name("TRUE")) {
            Some(true)
        } else if crate::same_name(name, self.function_name("FALSE")) {
            Some(false)
        } else {
            None
        }
    }
}

/// The LET/LAMBDA names in scope, with how many times each is bound, so a lookup is one probe
/// instead of a scan of every binding for each call.
#[derive(Default)]
struct Bindings {
    stack: Vec<String>,
    counts: std::collections::HashMap<String, usize>,
}

impl Bindings {
    fn len(&self) -> usize {
        self.stack.len()
    }

    fn push(&mut self, name: &str) {
        let key = crate::fold_name(name);
        *self.counts.entry(key.clone()).or_default() += 1;
        self.stack.push(key);
    }

    fn truncate(&mut self, len: usize) {
        while self.stack.len() > len {
            let Some(key) = self.stack.pop() else { break };
            if let Some(n) = self.counts.get_mut(&key) {
                *n -= 1;
                if *n == 0 {
                    self.counts.remove(&key);
                }
            }
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.counts.contains_key(&crate::fold_name(name))
    }
}

fn normalize(
    expr: &mut Expr,
    locale: FormulaLocale,
    known_name: &impl Fn(&str) -> bool,
    bindings: &mut Bindings,
    depth: usize,
    display: bool,
) -> Result<(), ParseError> {
    if depth > parser::MAX_DEPTH {
        return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
    }
    let bound = |name: &str, bindings: &Bindings| bindings.contains(name) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let alias = locale.function_name(if *value { "TRUE" } else { "FALSE" });
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !display && !bound(name, bindings) => {
            if let Some(value) = locale.boolean_alias(name) {
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
                            bindings.push(name);
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
                        bindings.push(name);
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
