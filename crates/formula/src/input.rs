mod tr;

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Tr,
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
        if self == Self::Es || self == Self::Tr { ';' } else { ',' }
    }
    pub fn decimal_separator(self) -> char {
        if self == Self::Es || self == Self::Tr { ',' } else { '.' }
    }
    pub fn array_column_separator(self) -> char {
        if self == Self::Es || self == Self::Tr { '\\' } else { ',' }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        match self {
            Self::Es => SPANISH.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, es)| *es).unwrap_or(canonical),
            Self::Tr => tr::TURKISH.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, t)| *t).unwrap_or(canonical),
            Self::En => canonical,
        }
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        match self {
            Self::Es => SPANISH.iter().find(|(_, es)| es.eq_ignore_ascii_case(localized)).map(|(en, _)| *en).unwrap_or(localized),
            Self::Tr => tr::TURKISH.iter().find(|(_, t)| tr::turkish_eq_ignore_case(t, localized)).map(|(en, _)| *en).unwrap_or(localized),
            Self::En => localized,
        }
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale != FormulaLocale::En {
        normalize(&mut expr, locale, &known_name, &mut Bindings::new(locale), 0, false)?;
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
    normalize(&mut expr, locale, &known_name, &mut Bindings::new(locale), 0, true)?;
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
    match locale {
        FormulaLocale::Es => {
            if name.eq_ignore_ascii_case("VERDADERO") {
                Some(true)
            } else if name.eq_ignore_ascii_case("FALSO") {
                Some(false)
            } else {
                None
            }
        }
        FormulaLocale::Tr => {
            if tr::turkish_eq_ignore_case(name, "DOĞRU") {
                Some(true)
            } else if tr::turkish_eq_ignore_case(name, "YANLIŞ") {
                Some(false)
            } else {
                None
            }
        }
        FormulaLocale::En => None,
    }
}

/// The LET/LAMBDA names in scope, with how many times each is bound, so a lookup is one probe
/// instead of a scan of every binding for each call.
#[derive(Default)]
struct Bindings {
    locale: FormulaLocale,
    stack: Vec<String>,
    counts: std::collections::HashMap<String, usize>,
}

impl Bindings {
    fn new(locale: FormulaLocale) -> Self {
        Self { locale, stack: Vec::new(), counts: std::collections::HashMap::new() }
    }

    fn len(&self) -> usize {
        self.stack.len()
    }

    fn fold(&self, name: &str) -> String {
        match self.locale {
            FormulaLocale::Tr => tr_fold_name(name),
            _ => crate::fold_name(name),
        }
    }

    fn push(&mut self, name: &str) {
        let key = self.fold(name);
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
        self.counts.contains_key(&self.fold(name))
    }
}

fn tr_fold_name(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'i' | 'İ' | 'ı' | 'I' => ['i', 'i'].into_iter().take(1),
            'ç' | 'Ç' => ['ç', 'ç'].into_iter().take(1),
            'ğ' | 'Ğ' => ['ğ', 'ğ'].into_iter().take(1),
            'ö' | 'Ö' => ['ö', 'ö'].into_iter().take(1),
            'ş' | 'Ş' => ['ş', 'ş'].into_iter().take(1),
            'ü' | 'Ü' => ['ü', 'ü'].into_iter().take(1),
            c => [c, c].into_iter().take(1),
        })
        .flat_map(char::to_lowercase)
        .flat_map(|c| match c {
            'ß' => ['s', 's'].into_iter().take(2),
            'ſ' => ['s', 's'].into_iter().take(1),
            'ς' => ['σ', 'σ'].into_iter().take(1),
            c => [c, c].into_iter().take(1),
        })
        .collect()
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
            let alias = match locale {
                FormulaLocale::Es => {
                    if *value {
                        "VERDADERO"
                    } else {
                        "FALSO"
                    }
                }
                FormulaLocale::Tr => {
                    if *value {
                        "DOĞRU"
                    } else {
                        "YANLIŞ"
                    }
                }
                FormulaLocale::En => {
                    if *value {
                        "TRUE"
                    } else {
                        "FALSE"
                    }
                }
            };
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
