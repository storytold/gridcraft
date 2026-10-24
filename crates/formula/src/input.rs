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
    // Financial block (#1986): the loan-schedule and investment staples.
    ("NPV", "VNA"),
    ("IRR", "TIR"),
    ("PMT", "PAGO"),
    ("PV", "VA"),
    ("FV", "VF"),
    ("TRUE", "VERDADERO"),
    ("FALSE", "FALSO"),
];

const ITALIAN: &[(&str, &str)] = &[
    ("ABS", "VAL.ABS"),
    ("ACCRINT", "INTACCR"),
    ("ACCRINTM", "INTACCR.M"),
    ("ACOS", "ARCCOS"),
    ("ACOSH", "ARCCOSH"),
    ("ACOT", "ARCCOT"),
    ("ACOTH", "ARCCOTANH"),
    ("ADDRESS", "INDIRIZZO"),
    ("AMORDEGRC", "AMMORT.DEGR"),
    ("AMORLINC", "AMMORT.LIN"),
    ("AND", "E"),
    ("ARABIC", "ARABO"),
    ("AREAS", "AREE"),
    ("ARRAYTOTEXT", "TESTO.MATRICE"),
    ("ASIN", "ARCSEN"),
    ("ASINH", "ARCSENH"),
    ("ATAN", "ARCTAN"),
    ("ATAN2", "ARCTAN.2"),
    ("ATANH", "ARCTANH"),
    ("AVEDEV", "DEVIAZIONE.MEDIA"),
    ("AVERAGE", "MEDIA"),
    ("AVERAGEA", "MEDIA.VALORI"),
    ("AVERAGEIF", "MEDIA.SE"),
    ("AVERAGEIFS", "MEDIA.PIÙ.SE"),
    ("BASE", "BASE"),
    ("BETA.DIST", "BETA.DIST"),
    ("BETA.INV", "BETA.INV"),
    ("BINOM.DIST", "BINOM.DIST"),
    ("BINOM.DIST.RANGE", "BINOM.DIST.INTERVALLO"),
    ("BINOM.INV", "BINOM.INV"),
    ("BYCOL", "PERCOLONNA"),
    ("BYROW", "PERRIGA"),
    ("CEILING", "ARROTONDA.ECCESSO"),
    ("CEILING.MATH", "ARROTONDA.ECCESSO.MAT"),
    ("CEILING.PRECISE", "ARROTONDA.ECCESSO.ESATTO"),
    ("CELL", "CELLA"),
    ("CHAR", "CARATTERE"),
    ("CHIDIST", "DISTRIB.CHI"),
    ("CHIINV", "INV.CHI"),
    ("CHISQ.DIST", "DISTRIB.CHI.QUAD"),
    ("CHISQ.DIST.RT", "DISTRIB.CHI.QUAD.DS"),
    ("CHISQ.INV", "INV.CHI.QUAD"),
    ("CHISQ.INV.RT", "INV.CHI.QUAD.DS"),
    ("CHISQ.TEST", "TEST.CHI.QUAD"),
    ("CHITEST", "TEST.CHI"),
    ("CHOOSE", "SCEGLI"),
    ("CHOOSECOLS", "SCEGLI.COL"),
    ("CHOOSEROWS", "SCEGLI.RIGHE"),
    ("CLEAN", "PULISCI"),
    ("CODE", "CODICE"),
    ("COLUMN", "COLONNA"),
    ("COLUMNS", "COLONNE"),
    ("COMBIN", "COMBINAZIONE"),
    ("COMBINA", "COMBINAZIONE.A"),
    ("COMPLEX", "COMPLESSO"),
    ("CONCAT", "CONCAT"),
    ("CONCATENATE", "CONCATENA"),
    ("CONFIDENCE", "CONFIDENZA"),
    ("CONFIDENCE.NORM", "CONFIDENZA.NORM"),
    ("CONFIDENCE.T", "CONFIDENZA.T"),
    ("CONVERT", "CONVERTI"),
    ("CORREL", "CORREL"),
    ("COS", "COS"),
    ("COSH", "COSH"),
    ("COT", "COTAN"),
    ("COTH", "COTANH"),
    ("COUNT", "CONTA.NUMERI"),
    ("COUNTA", "CONTA.VALORI"),
    ("COUNTBLANK", "CONTA.VUOTE"),
    ("COUNTIF", "CONTA.SE"),
    ("COUNTIFS", "CONTA.PIÙ.SE"),
    ("COVAR", "COVARIANZA"),
    ("COVARIANCE.P", "COVARIANZA.P"),
    ("COVARIANCE.S", "COVARIANZA.S"),
    ("CRITBINOM", "CRIT.BINOM"),
    ("CSC", "COSEC"),
    ("CSCH", "COSEC.H"),
    ("CUMIPMT", "INT.CUM"),
    ("CUMPRINC", "CAP.CUM"),
    ("DATE", "DATA"),
    ("DATEDIF", "DATA.DIFF"),
    ("DATEVALUE", "DATA.VALORE"),
    ("DAVERAGE", "DBMEDIA"),
    ("DAY", "GIORNO"),
    ("DAYS", "GIORNI"),
    ("DAYS360", "GIORNI360"),
    ("DB", "AMMORT.ESA"),
    ("DCOUNT", "DB.CONTA"),
    ("DCOUNTA", "DB.CONTA.VALORI"),
    ("DDB", "AMMORT.ACCEL"),
    ("DEC2BIN", "DECIM.BIN"),
    ("DEC2HEX", "DECIM.HEX"),
    ("DEC2OCT", "DECIM.OCT"),
    ("DEGREES", "GRADI"),
    ("DELTA", "DELTA"),
    ("DEVSQ", "SQ.DEVIATIONI"),
    ("DGET", "DB.OTT"),
    ("DISC", "SCONTO"),
    ("DMAX", "DB.MAX"),
    ("DMIN", "DB.MIN"),
    ("DOLLAR", "VALUTA"),
    ("DOLLARDE", "VALUTA.DE"),
    ("DOLLARFR", "VALUTA.FR"),
    ("DPRODUCT", "DB.PRODOTTO"),
    ("DROP", "RILASCIA"),
    ("DSTDEV", "DB.DEV.ST"),
    ("DSTDEVP", "DB.DEV.ST.P"),
    ("DSUM", "DB.SOMMA"),
    ("DURATION", "DURATA"),
    ("DVAR", "DB.VAR"),
    ("DVARP", "DB.VAR.P"),
    ("EDATE", "DATA.MESE"),
    ("EFFECT", "EFFETTIVO"),
    ("EOMONTH", "FINE.MESE"),
    ("ERF", "ERF"),
    ("ERF.PRECISE", "ERF.ESATTO"),
    ("ERFC", "ERFC"),
    ("ERFC.PRECISE", "ERFC.ESATTO"),
    ("ERROR.TYPE", "TIPO.ERRORE"),
    ("EUROCONVERT", "EURO.CONVERTI"),
    ("EVEN", "PARI"),
    ("EXACT", "IDENTICO"),
    ("EXP", "EXP"),
    ("EXPAND", "ESPANDI"),
    ("EXPON.DIST", "ESPON.DIST"),
    ("F.DIST", "F.DIST"),
    ("F.DIST.RT", "F.DIST.DX"),
    ("F.INV", "F.INV"),
    ("F.INV.RT", "F.INV.DX"),
    ("F.TEST", "F.TEST"),
    ("FACT", "FATTORIALE"),
    ("FACTDOUBLE", "FATTORIALE.DOPPIA"),
    ("FALSE", "FALSO"),
    ("FILTER", "FILTRO"),
    ("FIND", "TROVA"),
    ("FISHER", "FISHER"),
    ("FISHERINV", "FISHER.INV"),
    ("FIXED", "FISSO"),
    ("FLOOR", "ARROTONDA.DIFETTO"),
    ("FLOOR.MATH", "ARROTONDA.DIFETTO.MAT"),
    ("FLOOR.PRECISE", "ARROTONDA.DIFETTO.ESATTO"),
    ("FORECAST", "PREVISIONE"),
    ("FORECAST.LINEAR", "PREVISIONE.LINEARE"),
    ("FORMULATEXT", "TESTO.FORMULA"),
    ("FREQUENCY", "FREQUENZA"),
    ("FV", "VAL.FUT"),
    ("FVSCHEDULE", "VAL.FUT.PROGR"),
    ("GAMMA", "GAMMA"),
    ("GAMMA.DIST", "GAMMA.DIST"),
    ("GAMMA.INV", "GAMMA.INV"),
    ("GAMMALN", "GAMMALN"),
    ("GAUSS", "GAUSS"),
    ("GCD", "MCD"),
    ("GEOMEAN", "MEDIA.GEOM"),
    ("GESTEP", "GRADOPASSO"),
    ("GROWTH", "CRESCITA"),
    ("HARMEAN", "MEDIA.ARM"),
    ("HEX2BIN", "HEX.BIN"),
    ("HEX2DEC", "HEX.DEC"),
    ("HEX2OCT", "HEX.OCT"),
    ("HLOOKUP", "CERCA.ORIZZ"),
    ("HOUR", "ORA"),
    ("HYPERLINK", "COLLEGAMENTO"),
    ("HYPGEOM.DIST", "IPERGEOM.DIST"),
    ("IF", "SE"),
    ("IFERROR", "SE.ERRORE"),
    ("IFNA", "SE.NON.DISP"),
    ("IFS", "PIÙ.SE"),
    ("IMABS", "IMMODULO"),
    ("IMAGINARY", "IMIMMAGINARIO"),
    ("IMARGUMENT", "IMARGOMENTO"),
    ("IMCONJUGATE", "IMMESSO"),
    ("IMCOS", "IMCOS"),
    ("IMCOSH", "IMCOSH"),
    ("IMCOT", "IMCOTAN"),
    ("IMCSC", "IMCOSEC"),
    ("IMCSCH", "IMCOSEC.H"),
    ("IMDIV", "IMDIV"),
    ("IMEXP", "IMEXP"),
    ("IMLN", "IMLN"),
    ("IMLOG10", "IMLOG10"),
    ("IMLOG2", "IMLOG2"),
    ("IMPOWER", "IMPOTENZA"),
    ("IMPRODUCT", "IMPRODOTTO"),
    ("IMREAL", "IMPARTEREALE"),
    ("IMSEC", "IMSEC"),
    ("IMSECH", "IMSEC.H"),
    ("IMSIN", "IMSEN"),
    ("IMSINH", "IMSENH"),
    ("IMSQRT", "IMRADICE"),
    ("IMSUB", "IMDIFF"),
    ("IMSUM", "IMSOMMA"),
    ("IMTAN", "IMTAN"),
    ("INDEX", "INDICE"),
    ("INDIRECT", "INDIRETTO"),
    ("INFO", "INFO"),
    ("INT", "INT"),
    ("INTERCEPT", "INTERCETTA"),
    ("INTRATE", "TASSOINT"),
    ("IPMT", "INT.RATA"),
    ("IRR", "TIR"),
    ("ISBLANK", "VAL.VUOTO"),
    ("ISERR", "VAL.ERR"),
    ("ISERROR", "VAL.ERRORE"),
    ("ISEVEN", "VAL.PARI"),
    ("ISFORMULA", "VAL.FORMULA"),
    ("ISLOGICAL", "VAL.LOGICO"),
    ("ISNA", "VAL.NON.DISP"),
    ("ISNONTEXT", "VAL.NON.TESTO"),
    ("ISNUMBER", "VAL.NUMERO"),
    ("ISO.CEILING", "ARROTONDA.ECCESSO.ISO"),
    ("ISODD", "VAL.DISPARI"),
    ("ISOMITTED", "ISOMITTED"),
    ("ISOWEEKNUM", "NUM.SETTIMANA.ISO"),
    ("ISPMT", "INT.P"),
    ("ISREF", "VAL.RIF"),
    ("ISTEXT", "VAL.TESTO"),
    ("KURT", "CURTOSI"),
    ("LARGE", "PIÙ.GRANDE"),
    ("LCM", "MCM"),
    ("LEFT", "SINISTRA"),
    ("LEN", "LUNGHEZZA"),
    ("LINEST", "REG.LIN"),
    ("LN", "LN"),
    ("LOG", "LOG"),
    ("LOG10", "LOG10"),
    ("LOGEST", "REG.LOG"),
    ("LOGNORM.DIST", "LOGNORM.DIST"),
    ("LOGNORM.INV", "LOGNORM.INV"),
    ("LOOKUP", "CERCA"),
    ("LOWER", "MINUSC"),
    ("MAKEARRAY", "CREA.MATRICE"),
    ("MAP", "MAPPA"),
    ("MATCH", "CONFRONTA"),
    ("MAX", "MAX"),
    ("MAXA", "MAX.VALORI"),
    ("MAXIFS", "MAX.PIÙ.SE"),
    ("MDURATION", "DURATA.MOD"),
    ("MEDIAN", "MEDIANA"),
    ("MID", "STRINGA.ESTRAI"),
    ("MIN", "MIN"),
    ("MINA", "MIN.VALORI"),
    ("MINIFS", "MIN.PIÙ.SE"),
    ("MINUTE", "MINUTO"),
    ("MOD", "RESTO"),
    ("MODE", "MODA"),
    ("MODE.MULT", "MODA.MULT"),
    ("MODE.SNGL", "MODA.SNGL"),
    ("MONTH", "MESE"),
    ("MROUND", "ARROTONDA.MULT"),
    ("MULTINOMIAL", "MULTINOMIALE"),
    ("MUNIT", "MATRIDE"),
    ("N", "N"),
    ("NA", "N.DISP"),
    ("NEGBINOM.DIST", "NEGBINOM.DIST"),
    ("NETWORKDAYS", "GIORNI.LAV"),
    ("NETWORKDAYS.INTL", "GIORNI.LAV.INTL"),
    ("NOMINAL", "NOMINALE"),
    ("NORM.DIST", "NORM.DIST"),
    ("NORM.INV", "NORM.INV"),
    ("NORM.S.DIST", "NORM.S.DIST"),
    ("NORM.S.INV", "NORM.S.INV"),
    ("NOT", "NON"),
    ("NOW", "ADESSO"),
    ("NPER", "NUM.RATE"),
    ("NPV", "VNA"),
    ("NUMBERVALUE", "VALORE.NUMERO"),
    ("OCT2BIN", "OTT.BIN"),
    ("OCT2DEC", "OTT.DEC"),
    ("OCT2HEX", "OTT.HEX"),
    ("ODD", "DISPARI"),
    ("OFFSET", "SPOSTA"),
    ("OR", "O"),
    ("PDURATION", "DURATA.P"),
    ("PEARSON", "PEARSON"),
    ("PERCENTILE", "PERCENTILE"),
    ("PERCENTILE.EXC", "PERCENTILE.ESC"),
    ("PERCENTILE.INC", "PERCENTILE.INC"),
    ("PERMUT", "PERMUTAZIONE"),
    ("PERMUTATIONA", "PERMUTAZIONE.A"),
    ("PHI", "PHI"),
    ("PI", "PI.GRECO"),
    ("PMT", "RATA"),
    ("POISSON", "POISSON"),
    ("POISSON.DIST", "POISSON.DIST"),
    ("POWER", "POTENZA"),
    ("PPMT", "CAP.RATA"),
    ("PRICE", "PREZZO"),
    ("PRICEDISC", "PREZZO.SCONTO"),
    ("PRICEMAT", "PREZZO.RIMBORSO"),
    ("PROB", "PROB"),
    ("PRODUCT", "PRODOTTO"),
    ("PROPER", "MAIUSC.INIZ"),
    ("PV", "VAL.ATT"),
    ("QUARTILE", "QUARTILE"),
    ("QUARTILE.EXC", "QUARTILE.ESC"),
    ("QUARTILE.INC", "QUARTILE.INC"),
    ("QUOTIENT", "QUOZIENTE"),
    ("RADIANS", "RADIANTI"),
    ("RAND", "RAND"),
    ("RANDARRAY", "RANDOM.MATRICE"),
    ("RANDBETWEEN", "RANDOM.NUMERO"),
    ("RANK", "RANK"),
    ("RANK.AVG", "RANK.MED"),
    ("RANK.EQ", "RANK.PIÙ"),
    ("RATE", "TASSO"),
    ("RECEIVED", "RICEVUTO"),
    ("REDUCE", "RIDUCI"),
    ("REPLACE", "RIMPIAZZA"),
    ("REPT", "RIPETI"),
    ("RIGHT", "DESTRA"),
    ("ROMAN", "ROMANO"),
    ("ROUND", "ARROTONDA"),
    ("ROUNDDOWN", "ARROTONDA.PER.DIF"),
    ("ROUNDUP", "ARROTONDA.PER.ECC"),
    ("ROW", "RIGA"),
    ("ROWS", "RIGHE"),
    ("RRI", "IR"),
    ("RSQ", "RQ"),
    ("SCAN", "SCAN"),
    ("SEARCH", "RICERCA"),
    ("SEC", "SEC"),
    ("SECH", "SEC.H"),
    ("SECOND", "SECONDO"),
    ("SEQUENCE", "SEQUENZA"),
    ("SHEET", "FOGLIO"),
    ("SHEETS", "FOGLI"),
    ("SIGN", "SEGNO"),
    ("SIN", "SEN"),
    ("SINH", "SENH"),
    ("SKEW", "ASIMMETRIA"),
    ("SKEW.P", "ASIMMETRIA.P"),
    ("SLN", "AMMORT.COSTANTE"),
    ("SLOPE", "PENDENZA"),
    ("SMALL", "PIÙ.PICCOLO"),
    ("SORT", "ORDINA"),
    ("SORTBY", "ORDINA.PER"),
    ("SQRT", "RADQ"),
    ("SQRTPI", "RADQ.PI"),
    ("STANDARDIZE", "STANDARDIZZA"),
    ("STDEV", "DEV.ST"),
    ("STDEV.P", "DEV.ST.P"),
    ("STDEV.S", "DEV.ST.S"),
    ("STDEVA", "DEV.ST.A"),
    ("STDEVPA", "DEV.ST.P.A"),
    ("STEYX", "ERR.STD.Y"),
    ("SUBSTITUTE", "SOSTITUISCI"),
    ("SUBTOTAL", "SUBTOTALE"),
    ("SUM", "SOMMA"),
    ("SUMIF", "SOMMA.SE"),
    ("SUMIFS", "SOMMA.PIÙ.SE"),
    ("SUMPRODUCT", "SOMMA.PRODOTTO"),
    ("SUMSQ", "SOMMA.Q"),
    ("SUMX2MY2", "SOMMA.X2MENOY2"),
    ("SUMX2PY2", "SOMMA.X2PIÙY2"),
    ("SUMXMY2", "SOMMA.XMENOY2"),
    ("SWITCH", "SWITCH"),
    ("SYD", "AMMORT.ANNI"),
    ("T", "T"),
    ("T.DIST", "T.DIST"),
    ("T.DIST.2T", "T.DIST.2C"),
    ("T.DIST.RT", "T.DIST.DX"),
    ("T.INV", "T.INV"),
    ("T.INV.2T", "T.INV.2C"),
    ("T.TEST", "T.TEST"),
    ("TAKE", "PRENDI"),
    ("TAN", "TAN"),
    ("TANH", "TANH"),
    ("TBILLEQ", "BOT.EQUIV"),
    ("TBILLPRICE", "BOT.PREZZO"),
    ("TBILLYIELD", "BOT.RENDIMENTO"),
    ("TEXT", "TESTO"),
    ("TEXTAFTER", "TESTO.DOPO"),
    ("TEXTBEFORE", "TESTO.PRIMA"),
    ("TEXTJOIN", "TESTO.UNISCI"),
    ("TEXTSPLIT", "TESTO.DIVIDI"),
    ("TIME", "ORARIO"),
    ("TIMEVALUE", "ORARIO.VALORE"),
    ("TOCOL", "INCOLONNA"),
    ("TODAY", "OGGI"),
    ("TOROW", "INRIGA"),
    ("TRANSPOSE", "TRASPONI"),
    ("TREND", "TENDENZA"),
    ("TRIM", "ANNULLA.SPAZI"),
    ("TRIMMEAN", "MEDIA.TRONCATA"),
    ("TRUE", "VERO"),
    ("TRUNC", "TRONCA"),
    ("TTEST", "T.TEST"),
    ("TYPE", "TIPO"),
    ("UNICHAR", "CARATTERE.UNI"),
    ("UNICODE", "UNICODE"),
    ("UNIQUE", "UNICO"),
    ("UPPER", "MAIUSC"),
    ("VALUE", "VALORE"),
    ("VALUETOTEXT", "VALORE.TESTO"),
    ("VAR", "VAR"),
    ("VAR.P", "VAR.P"),
    ("VAR.S", "VAR.S"),
    ("VARA", "VAR.A"),
    ("VARPA", "VAR.P.A"),
    ("VDB", "AMMORT.VAR"),
    ("VLOOKUP", "CERCA.VERT"),
    ("WEEKDAY", "GIORNO.SETT"),
    ("WEEKNUM", "NUM.SETTIMANA"),
    ("WEIBULL", "WEIBULL"),
    ("WEIBULL.DIST", "WEIBULL.DIST"),
    ("WORKDAY", "GIORNO.LAV"),
    ("WORKDAY.INTL", "GIORNO.LAV.INTL"),
    ("WRAPCOLS", "RITAGLIA.COLONNE"),
    ("WRAPROWS", "RITAGLIA.RIGHE"),
    ("XIRR", "TIR.X"),
    ("XLOOKUP", "CERCA.X"),
    ("XMATCH", "CONFRONTA.X"),
    ("XNPV", "VNA.X"),
    ("XOR", "XOR"),
    ("YEAR", "ANNO"),
    ("YEARFRAC", "FRAZIONE.ANNO"),
    ("YIELD", "RENDIMENTO"),
    ("YIELDDISC", "RENDIMENTO.SCONTO"),
    ("YIELDMAT", "RENDIMENTO.RIMBORSO"),
    ("Z.TEST", "Z.TEST"),
    ("ZTEST", "Z.TEST"),
];

impl FormulaLocale {
    pub fn list_separator(self) -> char {
        if self == Self::Es || self == Self::It { ';' } else { ',' }
    }
    pub fn decimal_separator(self) -> char {
        if self == Self::Es || self == Self::It { ',' } else { '.' }
    }
    pub fn array_column_separator(self) -> char {
        if self == Self::Es || self == Self::It { '\\' } else { ',' }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        let table = match self {
            Self::Es => SPANISH,
            Self::It => ITALIAN,
            _ => return canonical,
        };
        table.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, it)| *it).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        let table = match self {
            Self::Es => SPANISH,
            Self::It => ITALIAN,
            _ => return localized,
        };
        // Unicode-aware case folding: Italian aliases contain accented letters (e.g. "PIÙ.SE"),
        // which `eq_ignore_ascii_case` would miss when the user types them in lower case.
        table.iter().find(|(_, it)| it.to_uppercase() == localized.to_uppercase()).map(|(en, _)| *en).unwrap_or(localized)
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale == FormulaLocale::Es || locale == FormulaLocale::It {
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

pub(crate) fn boolean_alias(locale: FormulaLocale, name: &str) -> Option<bool> {
    let (true_name, false_name) = match locale {
        FormulaLocale::Es => ("VERDADERO", "FALSO"),
        FormulaLocale::It => ("VERO", "FALSO"),
        _ => return None,
    };
    if name.eq_ignore_ascii_case(true_name) {
        Some(true)
    } else if name.eq_ignore_ascii_case(false_name) {
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
            let (true_name, false_name) = match locale {
                FormulaLocale::Es => ("VERDADERO", "FALSO"),
                FormulaLocale::It => ("VERO", "FALSO"),
                _ => ("VERDADERO", "FALSO"),
            };
            let alias = if *value { true_name } else { false_name };
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !display && !bound(name, bindings) => {
            if let Some(value) = boolean_alias(locale, name) {
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
