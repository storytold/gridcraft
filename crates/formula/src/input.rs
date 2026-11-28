//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
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

/// Brazilian Portuguese (pt-BR) worksheet function aliases.
const PORTUGUESE_BR: &[(&str, &str)] = &[
    // Lookup & Reference
    ("VLOOKUP", "PROCV"),
    ("HLOOKUP", "PROCH"),
    ("XLOOKUP", "PROCX"),
    ("LOOKUP", "PROC"),
    ("MATCH", "CORRESP"),
    ("XMATCH", "CORRESPX"),
    ("INDEX", "ÍNDICE"),
    ("INDEX", "INDICE"),
    ("OFFSET", "DESLOC"),
    ("INDIRECT", "INDIRETO"),
    ("ROW", "LIN"),
    ("ROWS", "LINS"),
    ("COLUMN", "COL"),
    ("COLUMNS", "COLS"),
    ("TRANSPOSE", "TRANSPOR"),
    ("CHOOSE", "ESCOLHER"),
    ("ADDRESS", "ENDEREÇO"),
    ("ADDRESS", "ENDERECO"),
    // Math & Trig
    ("SUM", "SOMA"),
    ("SUMIF", "SOMASE"),
    ("SUMIFS", "SOMASES"),
    ("PRODUCT", "MULT"),
    ("ROUND", "ARRED"),
    ("ROUNDUP", "ARREDONDAR.PARA.CIMA"),
    ("ROUNDDOWN", "ARREDONDAR.PARA.BAIXO"),
    ("ABS", "ABS"),
    ("INT", "INT"),
    ("MOD", "RESTO"),
    ("POWER", "POTÊNCIA"),
    ("POWER", "POTENCIA"),
    ("SQRT", "RAIZ"),
    ("SUBTOTAL", "SUBTOTAL"),
    ("AGGREGATE", "AGREGAR"),
    // Statistical
    ("AVERAGE", "MÉDIA"),
    ("AVERAGE", "MEDIA"),
    ("AVERAGEIF", "MÉDIASE"),
    ("AVERAGEIF", "MEDIASE"),
    ("AVERAGEIFS", "MÉDIASES"),
    ("AVERAGEIFS", "MEDIASES"),
    ("COUNT", "CONT.NUM"),
    ("COUNTA", "CONT.VALORES"),
    ("COUNTBLANK", "CONTAR.VAZIO"),
    ("COUNTIF", "CONT.SE"),
    ("COUNTIFS", "CONT.SES"),
    ("MAX", "MÁXIMO"),
    ("MAX", "MAXIMO"),
    ("MIN", "MÍNIMO"),
    ("MIN", "MINIMO"),
    ("LARGE", "MAIOR"),
    ("SMALL", "MENOR"),
    ("MEDIAN", "MED"),
    ("MODE", "MODO"),
    // Logical
    ("IF", "SE"),
    ("IFS", "SES"),
    ("IFERROR", "SEERRO"),
    ("IFNA", "SENÃODISP"),
    ("IFNA", "SENAODISP"),
    ("AND", "E"),
    ("OR", "OU"),
    ("NOT", "NÃO"),
    ("NOT", "NAO"),
    ("XOR", "OUEXCL"),
    ("TRUE", "VERDADEIRO"),
    ("FALSE", "FALSO"),
    // Text
    ("CONCAT", "CONCAT"),
    ("CONCATENATE", "CONCATENAR"),
    ("TEXT", "TEXTO"),
    ("TEXTJOIN", "UNIRTEXTO"),
    ("LEFT", "ESQUERDA"),
    ("RIGHT", "DIREITA"),
    ("MID", "EXT.TEXTO"),
    ("LEN", "NÚM.CARACT"),
    ("LEN", "NUM.CARACT"),
    ("TRIM", "ARRUMAR"),
    ("UPPER", "MAIÚSCULA"),
    ("UPPER", "MAIUSCULA"),
    ("LOWER", "MINÚSCULA"),
    ("LOWER", "MINUSCULA"),
    ("PROPER", "PRI.MAIÚSCULA"),
    ("PROPER", "PRI.MAIUSCULA"),
    ("REPLACE", "MUDAR"),
    ("SUBSTITUTE", "SUBSTITUIR"),
    ("FIND", "PROCURAR"),
    ("SEARCH", "LOCALIZAR"),
    // Date & Time
    ("TODAY", "HOJE"),
    ("NOW", "AGORA"),
    ("DATE", "DATA"),
    ("TIME", "TEMPO"),
    ("YEAR", "ANO"),
    ("MONTH", "MÊS"),
    ("MONTH", "MES"),
    ("DAY", "DIA"),
    ("HOUR", "HORA"),
    ("MINUTE", "MINUTO"),
    ("SECOND", "SEGUNDO"),
    ("WEEKDAY", "DIA.DA.SEMANA"),
    ("NETWORKDAYS", "DIATRABALHOTOTAL"),
    ("EDATE", "DATAM"),
    ("EOMONTH", "FIMMÊS"),
    ("EOMONTH", "FIMMES"),
    // Information
    ("ISBLANK", "ÉVAZIA"),
    ("ISBLANK", "EVAZIA"),
    ("ISERROR", "ÉERRO"),
    ("ISERROR", "EERRO"),
    ("ISNUMBER", "ÉNÚM"),
    ("ISNUMBER", "ENUM"),
    ("ISTEXT", "ÉTEXTO"),
    ("ISTEXT", "ETEXTO"),
    ("ISNA", "É.NÃO.DISP"),
    ("ISNA", "E.NAO.DISP"),
    // Dynamic Arrays
    ("FILTER", "FILTRO"),
    ("SORT", "CLASSIFICAR"),
    ("SORTBY", "CLASSIFICARPOR"),
    ("UNIQUE", "ÚNICO"),
    ("UNIQUE", "UNICO"),
    ("SEQUENCE", "SEQUÊNCIA"),
    ("SEQUENCE", "SEQUENCIA"),
    ("RANDARRAY", "MATRIZALEATÓRIA"),
    ("RANDARRAY", "MATRIZALEATORIA"),
    ("DROP", "DESCARTAR"),
    ("TAKE", "OBTER"),
    ("EXPAND", "EXPANDIR"),
    ("TOCOL", "PARACOL"),
    ("TOROW", "PARALIN"),
    ("VSTACK", "EMPILHARV"),
    ("HSTACK", "EMPILHARH"),
    ("CHOOSEROWS", "ESCOLHERLINS"),
    ("CHOOSECOLS", "ESCOLHERCOLS"),
    ("WRAPROWS", "MOLDARLINS"),
    ("WRAPCOLS", "MOLDARCOLS"),
];

impl FormulaLocale {
    pub fn list_separator(self) -> char {
        if self == Self::Es || self == Self::PtBr { ';' } else { ',' }
    }
    pub fn decimal_separator(self) -> char {
        if self == Self::Es || self == Self::PtBr { ',' } else { '.' }
    }
    pub fn array_column_separator(self) -> char {
        if self == Self::Es || self == Self::PtBr { '\\' } else { ',' }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        match self {
            Self::Es => SPANISH.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, es)| *es).unwrap_or(canonical),
            Self::PtBr => PORTUGUESE_BR.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, pt)| *pt).unwrap_or(canonical),
            Self::En => canonical,
        }
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        match self {
            Self::Es => SPANISH.iter().find(|(_, es)| crate::same_name(es, localized)).map(|(en, _)| *en).unwrap_or(localized),
            Self::PtBr => PORTUGUESE_BR.iter().find(|(_, pt)| crate::same_name(pt, localized)).map(|(en, _)| *en).unwrap_or(localized),
            Self::En => localized,
        }
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
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    let (t, f) = match locale {
        FormulaLocale::En => return None,
        FormulaLocale::Es => ("VERDADERO", "FALSO"),
        FormulaLocale::PtBr => ("VERDADEIRO", "FALSO"),
    };
    if name.eq_ignore_ascii_case(t) {
        Some(true)
    } else if name.eq_ignore_ascii_case(f) {
        Some(false)
    } else {
        None
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
            let alias = match (locale, *value) {
                (FormulaLocale::En, true) => "TRUE",
                (FormulaLocale::En, false) => "FALSE",
                (FormulaLocale::Es, true) => "VERDADERO",
                (FormulaLocale::Es, false) => "FALSO",
                (FormulaLocale::PtBr, true) => "VERDADEIRO",
                (FormulaLocale::PtBr, false) => "FALSO",
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
