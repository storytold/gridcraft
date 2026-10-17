//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

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
    ("TRUE", "VERDADERO"),
    ("FALSE", "FALSO"),
];

/// Polish names as Excel's Polish edition spells them. No alias may equal another function's
/// canonical English name, or committing it would call the wrong function.
const POLISH: &[(&str, &str)] = &[
    ("SUM", "SUMA"),
    ("AVERAGE", "ŚREDNIA"),
    ("IF", "JEŻELI"),
    ("IFS", "WARUNKI"),
    ("IFERROR", "JEŻELI.BŁĄD"),
    ("IFNA", "JEŻELI.ND"),
    ("SWITCH", "PRZEŁĄCZ"),
    ("AND", "ORAZ"),
    ("OR", "LUB"),
    ("NOT", "NIE"),
    ("TRUE", "PRAWDA"),
    ("FALSE", "FAŁSZ"),
    ("COUNT", "ILE.LICZB"),
    ("COUNTA", "ILE.NIEPUSTYCH"),
    ("COUNTBLANK", "LICZ.PUSTE"),
    ("COUNTIF", "LICZ.JEŻELI"),
    ("COUNTIFS", "LICZ.WARUNKI"),
    ("SUMIF", "SUMA.JEŻELI"),
    ("SUMIFS", "SUMA.WARUNKÓW"),
    ("AVERAGEIF", "ŚREDNIA.JEŻELI"),
    ("AVERAGEIFS", "ŚREDNIA.WARUNKÓW"),
    ("MAXIFS", "MAKS.WARUNKÓW"),
    ("MINIFS", "MIN.WARUNKÓW"),
    ("SUMPRODUCT", "SUMA.ILOCZYNÓW"),
    ("PRODUCT", "ILOCZYN"),
    ("SUBTOTAL", "SUMY.CZĘŚCIOWE"),
    ("MAX", "MAKS"),
    ("LARGE", "MAX.K"),
    ("SMALL", "MIN.K"),
    ("MEDIAN", "MEDIANA"),
    ("RANK", "POZYCJA"),
    ("STDEV.S", "ODCH.STANDARD.PRÓBKI"),
    ("ROUND", "ZAOKR"),
    ("ROUNDUP", "ZAOKR.GÓRA"),
    ("ROUNDDOWN", "ZAOKR.DÓŁ"),
    ("INT", "ZAOKR.DO.CAŁK"),
    ("TRUNC", "LICZBA.CAŁK"),
    ("CEILING", "ZAOKR.W.GÓRĘ"),
    ("FLOOR", "ZAOKR.W.DÓŁ"),
    ("ABS", "MODUŁ.LICZBY"),
    ("SIGN", "ZNAK.LICZBY"),
    ("SQRT", "PIERWIASTEK"),
    ("POWER", "POTĘGA"),
    ("RAND", "LOS"),
    ("RANDBETWEEN", "LOS.ZAKR"),
    ("VLOOKUP", "WYSZUKAJ.PIONOWO"),
    ("HLOOKUP", "WYSZUKAJ.POZIOMO"),
    ("XLOOKUP", "X.WYSZUKAJ"),
    ("LOOKUP", "WYSZUKAJ"),
    ("INDEX", "INDEKS"),
    ("MATCH", "PODAJ.POZYCJĘ"),
    ("XMATCH", "X.DOPASUJ"),
    ("CHOOSE", "WYBIERZ"),
    ("OFFSET", "PRZESUNIĘCIE"),
    ("INDIRECT", "ADR.POŚR"),
    ("ROW", "WIERSZ"),
    ("ROWS", "ILE.WIERSZY"),
    ("COLUMN", "NR.KOLUMNY"),
    ("COLUMNS", "LICZBA.KOLUMN"),
    ("TRANSPOSE", "TRANSPONUJ"),
    ("FILTER", "FILTRUJ"),
    ("SORT", "SORTUJ"),
    ("UNIQUE", "UNIKATOWE"),
    ("SEQUENCE", "SEKWENCJA"),
    ("TODAY", "DZIŚ"),
    ("NOW", "TERAZ"),
    ("DATE", "DATA"),
    ("YEAR", "ROK"),
    ("MONTH", "MIESIĄC"),
    ("DAY", "DZIEŃ"),
    ("HOUR", "GODZINA"),
    ("MINUTE", "MINUTA"),
    ("SECOND", "SEKUNDA"),
    ("WEEKDAY", "DZIEŃ.TYG"),
    ("EOMONTH", "NR.SER.OST.DN.MIES"),
    ("EDATE", "NR.SER.DATY"),
    ("NETWORKDAYS", "DNI.ROBOCZE"),
    ("WORKDAY", "DZIEŃ.ROBOCZY"),
    ("DATEDIF", "DATA.RÓŻNICA"),
    ("CONCATENATE", "ZŁĄCZ.TEKSTY"),
    ("CONCAT", "ZŁĄCZ.TEKST"),
    ("TEXTJOIN", "POŁĄCZ.TEKSTY"),
    ("LEFT", "LEWY"),
    ("RIGHT", "PRAWY"),
    ("MID", "FRAGMENT.TEKSTU"),
    ("LEN", "DŁ"),
    ("UPPER", "LITERY.WIELKIE"),
    ("LOWER", "LITERY.MAŁE"),
    ("PROPER", "Z.WIELKIEJ.LITERY"),
    ("TRIM", "USUŃ.ZBĘDNE.ODSTĘPY"),
    ("TEXT", "TEKST"),
    ("VALUE", "WARTOŚĆ"),
    ("FIND", "ZNAJDŹ"),
    ("SEARCH", "SZUKAJ.TEKST"),
    ("SUBSTITUTE", "PODSTAW"),
    ("REPLACE", "ZASTĄP"),
    ("REPT", "POWT"),
    ("ISBLANK", "CZY.PUSTA"),
    ("ISNUMBER", "CZY.LICZBA"),
    ("ISTEXT", "CZY.TEKST"),
    ("ISERROR", "CZY.BŁĄD"),
    ("ISNA", "CZY.BRAK"),
    ("NA", "BRAK"),
];

/// Case-insensitive comparison that also folds non-ASCII letters (`jeżeli` = `JEŻELI`).
fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars().flat_map(char::to_uppercase).eq(b.chars().flat_map(char::to_uppercase))
}

impl FormulaLocale {
    fn aliases(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::Pl => POLISH,
        }
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
        self.aliases().iter().find(|(_, local)| eq_ignore_case(local, localized)).map(|(en, _)| *en).unwrap_or(localized)
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

pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    let (truth, falsity) = locale.boolean_names();
    if locale == FormulaLocale::En {
        None
    } else if eq_ignore_case(name, truth) {
        Some(true)
    } else if eq_ignore_case(name, falsity) {
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
    let bound = |name: &str, bindings: &[String]| bindings.iter().rev().any(|n| n.eq_ignore_ascii_case(name)) || known_name(name);
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
        Expr::Binary(_, left, right) => {
            normalize(left, locale, known_name, bindings, depth + 1, display)?;
            normalize(right, locale, known_name, bindings, depth + 1, display)?;
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
