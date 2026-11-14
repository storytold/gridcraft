//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Pt,
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

/// Brazilian Portuguese function names, sorted by the English name. A function missing here
/// keeps its English name. REPLACEB is left out: the name it was given is REPLACE, the English
/// name of another function, so typing REPLACE would call REPLACEB.
const PORTUGUESE: &[(&str, &str)] = &[
    ("ABS", "ABS"),
    ("ACCRINT", "JUROSACUM"),
    ("ACCRINTM", "JUROSACUMV"),
    ("ACOS", "ACOS"),
    ("ACOSH", "ACOSH"),
    ("ACOT", "ACOT"),
    ("ACOTH", "ACOTH"),
    ("ADDRESS", "ENDEREÇO"),
    ("AGGREGATE", "AGREGAR"),
    ("AND", "E"),
    ("ARABIC", "ARÁBICO"),
    ("AREAS", "ÁREAS"),
    ("ARRAYTOTEXT", "MATRIZPARATEXTO"),
    ("ASIN", "ASEN"),
    ("ASINH", "ASENH"),
    ("ATAN", "ATAN"),
    ("ATAN2", "ATAN2"),
    ("ATANH", "ATANH"),
    ("AVEDEV", "DESV.MÉDIO"),
    ("AVERAGE", "MÉDIA"),
    ("AVERAGEA", "MÉDIAA"),
    ("AVERAGEIF", "MÉDIASE"),
    ("AVERAGEIFS", "MÉDIASES"),
    ("BETA.DIST", "DIST.BETA"),
    ("BETA.INV", "INV.BETA"),
    ("BETADIST", "DISTBETA"),
    ("BETAINV", "BETA.ACUM.INV"),
    ("BIN2DEC", "BINADEC"),
    ("BIN2HEX", "BINAHEX"),
    ("BIN2OCT", "BINAOCT"),
    ("BINOM.DIST", "DISTR.BINOM"),
    ("BINOM.DIST.RANGE", "INTERV.DISTR.BINOM"),
    ("BINOM.INV", "INV.BINOM"),
    ("BINOMDIST", "DISTRBINOM"),
    ("BITLSHIFT", "DESLOCESQBIT"),
    ("BITRSHIFT", "DESLOCDIRBIT"),
    ("CEILING", "TETO"),
    ("CEILING.MATH", "TETO.MAT"),
    ("CEILING.PRECISE", "TETO.PRECISO"),
    ("CELL", "CÉL"),
    ("CHAR", "CARACT"),
    ("CHIDIST", "DIST.QUI"),
    ("CHIINV", "INV.QUI"),
    ("CHISQ.DIST", "DIST.QUIQUA"),
    ("CHISQ.DIST.RT", "DIST.QUIQUA.CD"),
    ("CHISQ.INV", "INV.QUIQUA"),
    ("CHISQ.INV.RT", "INV.QUIQUA.CD"),
    ("CHISQ.TEST", "TESTE.QUIQUA"),
    ("CHITEST", "TESTE.QUI"),
    ("CHOOSE", "ESCOLHER"),
    ("CLEAN", "TIRAR"),
    ("CODE", "CÓDIGO"),
    ("COLUMN", "COL"),
    ("COLUMNS", "COLS"),
    ("COMPLEX", "COMPLEXO"),
    ("CONCATENATE", "CONCATENAR"),
    ("CONFIDENCE", "INT.CONFIANÇA"),
    ("CONFIDENCE.NORM", "INT.CONFIANÇA.NORM"),
    ("CONFIDENCE.T", "INT.CONFIANÇA.T"),
    ("CONVERT", "CONVERTER"),
    ("COS", "COS"),
    ("COSH", "COSH"),
    ("COT", "COT"),
    ("COTH", "COTH"),
    ("COUNT", "CONT.NÚM"),
    ("COUNTA", "CONT.VALORES"),
    ("COUNTBLANK", "CONTAR.VAZIO"),
    ("COUNTIF", "CONT.SE"),
    ("COUNTIFS", "CONT.SES"),
    ("COUPDAYBS", "CUPDIASINLIQ"),
    ("COUPDAYS", "CUPDIAS"),
    ("COUPDAYSNC", "CUPDIASPRÓX"),
    ("COUPNCD", "CUPDATAPRÓX"),
    ("COUPNUM", "CUPNÚM"),
    ("COUPPCD", "CUPDATAANT"),
    ("COVARIANCE.P", "COVARIAÇÃO.P"),
    ("COVARIANCE.S", "COVARIAÇÃO.S"),
    ("CRITBINOM", "CRIT.BINOM"),
    ("CSC", "COSEC"),
    ("CSCH", "COSECH"),
    ("CUBEKPIMEMBER", "MEMBROKPICUBO"),
    ("CUBEMEMBER", "MEMBROCUBO"),
    ("CUBEMEMBERPROPERTY", "PROPRIEDADEMEMBROCUBO"),
    ("CUBERANKEDMEMBER", "MEMBROCLASSIFICADOCUBO"),
    ("CUBESET", "CONJUNTOCUBO"),
    ("CUBESETCOUNT", "CONTAGEMCONJUNTOCUBO"),
    ("CUBEVALUE", "VALORCUBO"),
    ("CUMIPMT", "PGTOJURACUM"),
    ("CUMPRINC", "PGTOCAPACUM"),
    ("DATE", "DATA"),
    ("DATEDIF", "DATADIF"),
    ("DATEVALUE", "DATA.VALOR"),
    ("DAVERAGE", "BDMÉDIA"),
    ("DAY", "DIA"),
    ("DAYS", "DIAS"),
    ("DAYS360", "DIAS360"),
    ("DB", "BD"),
    ("DCOUNT", "BDCONTAR"),
    ("DCOUNTA", "BDCONTARA"),
    ("DDB", "BDD"),
    ("DEC2BIN", "DECABIN"),
    ("DEC2HEX", "DECAHEX"),
    ("DEC2OCT", "DECAOCT"),
    ("DEGREES", "GRAUS"),
    ("DEVSQ", "DESVQ"),
    ("DGET", "BDEXTRAIR"),
    ("DISC", "DESC"),
    ("DMAX", "BDMÁX"),
    ("DMIN", "BDMÍN"),
    ("DOLLAR", "MOEDA"),
    ("DOLLARDE", "MOEDADEC"),
    ("DOLLARFR", "MOEDAFRA"),
    ("DPRODUCT", "BDMULTIPL"),
    ("DSTDEV", "BDEST"),
    ("DSTDEVP", "BDDESVPA"),
    ("DSUM", "BDSOMA"),
    ("DURATION", "DURAÇÃO"),
    ("DVAR", "BDVAREST"),
    ("DVARP", "BDVARP"),
    ("EDATE", "DATAM"),
    ("EFFECT", "EFETIVA"),
    ("ENCODEURL", "CODIFURL"),
    ("EOMONTH", "FIMMÊS"),
    ("ERF", "FUNERRO"),
    ("ERF.PRECISE", "FUNERRO.PRECISO"),
    ("ERFC", "FUNERROCOMPL"),
    ("ERFC.PRECISE", "FUNERROCOMPL.PRECISO"),
    ("ERROR.TYPE", "TIPO.ERRO"),
    ("EUROCONVERT", "CONVERTEREURO"),
    ("EVEN", "PAR"),
    ("EXACT", "EXATO"),
    ("EXP", "EXP"),
    ("EXPON.DIST", "DISTR.EXPON"),
    ("EXPONDIST", "DISTEXPON"),
    ("F.DIST", "DIST.F"),
    ("F.DIST.RT", "DIST.F.CD"),
    ("F.INV", "INV.F"),
    ("F.INV.RT", "INV.F.CD"),
    ("F.TEST", "TESTE.F"),
    ("FACT", "FATORIAL"),
    ("FACTDOUBLE", "FATDUPLO"),
    ("FALSE", "FALSO"),
    ("FDIST", "DISTF"),
    ("FILTER", "FILTRO"),
    ("FILTERXML", "FILTROXML"),
    ("FINV", "INVF"),
    ("FIXED", "FIXO"),
    ("FLOOR", "ARREDMULTB"),
    ("FLOOR.MATH", "ARREDMULTB.MAT"),
    ("FLOOR.PRECISE", "ARREDMULTB.PRECISO"),
    ("FORECAST", "PREVISÃO"),
    ("FORECAST.ETS", "PREVISÃO.ETS"),
    ("FORECAST.ETS.CONFINT", "PREVISÃO.ETS.CONFINT"),
    ("FORECAST.ETS.SEASONALITY", "PREVISÃO.ETS.SAZONALIDADE"),
    ("FORECAST.ETS.STAT", "PREVISÃO.ETS.STAT"),
    ("FORECAST.LINEAR", "PREVISÃO.LINEAR"),
    ("FORMULATEXT", "FÓRMULATEXTO"),
    ("FREQUENCY", "FREQÜÊNCIA"),
    ("FTEST", "TESTEF"),
    ("FV", "VF"),
    ("FVSCHEDULE", "VFPLANO"),
    ("GAMMA", "GAMA"),
    ("GAMMA.DIST", "DIST.GAMA"),
    ("GAMMA.INV", "INV.GAMA"),
    ("GAMMADIST", "DISTGAMA"),
    ("GAMMAINV", "INVGAMA"),
    ("GAMMALN", "LNGAMA"),
    ("GAMMALN.PRECISE", "LNGAMA.PRECISO"),
    ("GCD", "MDC"),
    ("GEOMEAN", "MÉDIA.GEOMÉTRICA"),
    ("GESTEP", "DEGRAU"),
    ("GROUPBY", "AGRUPARPOR"),
    ("GROWTH", "CRESCIMENTO"),
    ("HARMEAN", "MÉDIA.HARMÔNICA"),
    ("HEX2BIN", "HEXABIN"),
    ("HEX2DEC", "HEXADEC"),
    ("HEX2OCT", "HEXAOCT"),
    ("HLOOKUP", "PROCH"),
    ("HOUR", "HORA"),
    ("HSTACK", "EMPILHARH"),
    ("HYPERLINK", "HIPERLINK"),
    ("HYPGEOM.DIST", "DIST.HIPERGEOM.N"),
    ("HYPGEOMDIST", "DIST.HIPERGEOM"),
    ("IF", "SE"),
    ("IFERROR", "SEERRO"),
    ("IFNA", "SENÃODISP"),
    ("IFS", "SES"),
    ("IMAGINARY", "IMAGINÁRIO"),
    ("IMARGUMENT", "IMARG"),
    ("IMCONJUGATE", "IMCONJ"),
    ("IMCSC", "IMCOSEC"),
    ("IMCSCH", "IMCOSECH"),
    ("IMPOWER", "IMPOT"),
    ("IMPRODUCT", "IMPROD"),
    ("IMSIN", "IMSENO"),
    ("IMSINH", "IMSENH"),
    ("IMSQRT", "IMRAIZ"),
    ("IMSUB", "IMSUBTR"),
    ("IMSUM", "IMSOMA"),
    ("INDEX", "ÍNDICE"),
    ("INDIRECT", "INDIRETO"),
    ("INFO", "INFORMAÇÃO"),
    ("INTERCEPT", "INTERCEPÇÃO"),
    ("INTRATE", "TAXAJUROS"),
    ("IPMT", "IPGTO"),
    ("IRR", "TIR"),
    ("ISBLANK", "ÉCÉL.VAZIA"),
    ("ISERR", "ÉERRO"),
    ("ISERROR", "ÉERROS"),
    ("ISEVEN", "ÉPAR"),
    ("ISFORMULA", "ÉFÓRMULA"),
    ("ISLOGICAL", "ÉLÓGICO"),
    ("ISNA", "É.NÃO.DISP"),
    ("ISNONTEXT", "É.NÃO.TEXTO"),
    ("ISNUMBER", "ÉNÚM"),
    ("ISO.CEILING", "ISO.TETO"),
    ("ISODD", "ÉIMPAR"),
    ("ISOWEEKNUM", "NÚMSEMANAISO"),
    ("ISPMT", "ÉPGTO"),
    ("ISREF", "ÉREF"),
    ("ISTEXT", "ÉTEXTO"),
    ("KURT", "CURT"),
    ("LARGE", "MAIOR"),
    ("LCM", "MMC"),
    ("LEFT", "ESQUERDA"),
    ("LEN", "NÚM.CARACT"),
    ("LINEST", "PROJ.LIN"),
    ("LN", "LN"),
    ("LOG", "LOG"),
    ("LOG10", "LOG10"),
    ("LOGEST", "PROJ.LOG"),
    ("LOGINV", "INVLOG"),
    ("LOGNORM.DIST", "DIST.LOGNORMAL.N"),
    ("LOGNORMDIST", "DISTLOGNORMAL"),
    ("LOOKUP", "PROC"),
    ("LOWER", "MINÚSCULA"),
    ("MATCH", "CORRESP"),
    ("MAX", "MÁXIMO"),
    ("MAXA", "MÁXIMOA"),
    ("MAXIFS", "MÁXIMOSES"),
    ("MDETERM", "MATRIZ.DETERM"),
    ("MDURATION", "MDURAÇÃO"),
    ("MEDIAN", "MED"),
    ("MID", "EXT.TEXTO"),
    ("MIN", "MÍNIMO"),
    ("MINA", "MÍNIMOA"),
    ("MINIFS", "MÍNIMOSES"),
    ("MINUTE", "MINUTO"),
    ("MINVERSE", "MATRIZ.INVERSO"),
    ("MIRR", "MTIR"),
    ("MMULT", "MATRIZ.MULT"),
    ("MODE", "MODO"),
    ("MODE.MULT", "MODO.MULT"),
    ("MODE.SNGL", "MODO.ÚNICO"),
    ("MONTH", "MÊS"),
    ("MROUND", "MARRED"),
    ("NA", "NÃO.DISP"),
    ("NEGBINOM.DIST", "DIST.BIN.NEG.N"),
    ("NEGBINOMDIST", "DIST.BIN.NEG"),
    ("NETWORKDAYS", "DIATRABALHOTOTAL"),
    ("NETWORKDAYS.INTL", "DIATRABALHOTOTAL.INTL"),
    ("NORM.DIST", "DIST.NORM.N"),
    ("NORM.INV", "INV.NORM.N"),
    ("NORM.S.DIST", "DIST.NORMP.N"),
    ("NORM.S.INV", "INV.NORMP.N"),
    ("NORMDIST", "DIST.NORM"),
    ("NORMINV", "INV.NORM"),
    ("NORMSDIST", "DISTNORMP"),
    ("NORMSINV", "INV.NORMP"),
    ("NOT", "NÃO"),
    ("NOW", "AGORA"),
    ("NPV", "VPL"),
    ("NUMBERVALUE", "VALORNUMÉRICO"),
    ("OCT2BIN", "OCTABIN"),
    ("OCT2DEC", "OCTADEC"),
    ("OCT2HEX", "OCTAHEX"),
    ("ODD", "ÍMPAR"),
    ("ODDFPRICE", "PREÇOPRIMINC"),
    ("ODDFYIELD", "LUCROPRIMINC"),
    ("ODDLPRICE", "PREÇOÚLTINC"),
    ("ODDLYIELD", "LUCROÚLTINC"),
    ("OFFSET", "DESLOC"),
    ("OR", "OU"),
    ("PDURATION", "DURAÇÃOP"),
    ("PERCENTILE", "PERCENTIL"),
    ("PERCENTILE.EXC", "PERCENTIL.EXC"),
    ("PERCENTILE.INC", "PERCENTIL.INC"),
    ("PERCENTOF", "PORCENTAGEMDE"),
    ("PERCENTRANK", "ORDEM.PORCENTUAL"),
    ("PERCENTRANK.EXC", "ORDEM.PORCENTUAL.EXC"),
    ("PERCENTRANK.INC", "ORDEM.PORCENTUAL.INC"),
    ("PERMUTATIONA", "PERMUTAS"),
    ("PHONETIC", "FONÉTICA"),
    ("PI", "PI"),
    ("PMT", "PGTO"),
    ("POISSON.DIST", "DIST.POISSON"),
    ("POWER", "POTÊNCIA"),
    ("PPMT", "PPGTO"),
    ("PRICE", "PREÇO"),
    ("PRICEDISC", "PREÇODESC"),
    ("PRICEMAT", "PREÇOVENC"),
    ("PRODUCT", "PRODUTO"),
    ("PROPER", "PRI.MAIÚSCULA"),
    ("PV", "VP"),
    ("QUARTILE", "QUARTIL"),
    ("QUARTILE.EXC", "QUARTIL.EXC"),
    ("QUARTILE.INC", "QUARTIL.INC"),
    ("QUOTIENT", "QUOCIENTE"),
    ("RADIANS", "RADIANOS"),
    ("RAND", "ALEATÓRIO"),
    ("RANDARRAY", "MATRIZALEATÓRIA"),
    ("RANDBETWEEN", "ALEATÓRIOENTRE"),
    ("RANK", "ORDEM"),
    ("RANK.AVG", "ORDEM.MÉD"),
    ("RANK.EQ", "ORDEM.EQ"),
    ("RATE", "TAXA"),
    ("RECEIVED", "RECEBER"),
    ("REGEXTEST", "TEST.REGEX"),
    ("RIGHT", "DIREITA"),
    ("ROMAN", "ROMANO"),
    ("ROUND", "ARRED"),
    ("ROUNDDOWN", "ARREDONDAR.PARA.BAIXO"),
    ("ROUNDUP", "ARREDONDAR.PARA.CIMA"),
    ("ROW", "LIN"),
    ("ROWS", "LINS"),
    ("RRI", "TAXAJURO"),
    ("RSQ", "RQUAD"),
    ("SEARCH", "LOCALIZAR"),
    ("SEC", "SEC"),
    ("SECH", "SECH"),
    ("SECOND", "SEGUNDO"),
    ("SEQUENCE", "SEQUÊNCIA"),
    ("SERIESSUM", "SOMASEQUÊNCIA"),
    ("SHEET", "PLAN"),
    ("SHEETS", "PLANS"),
    ("SIGN", "SINAL"),
    ("SIN", "SEN"),
    ("SINH", "SENH"),
    ("SKEW", "DISTORÇÃO"),
    ("SKEW.P", "DISTORÇÃO.P"),
    ("SLN", "DPD"),
    ("SLOPE", "INCLINAÇÃO"),
    ("SMALL", "MENOR"),
    ("SORT", "CLASSIFICAR"),
    ("SORTBY", "CLASSIFICARPOR"),
    ("SQRT", "RAIZ"),
    ("SQRTPI", "RAIZPI"),
    ("STANDARDIZE", "PADRONIZAR"),
    ("STDEV", "DESVPAD"),
    ("STDEV.P", "DESVPAD.P"),
    ("STDEV.S", "DESVPAD.A"),
    ("STDEVA", "DESVPADA"),
    ("STDEVP", "DESVPADP"),
    ("STDEVPA", "DESVPADPA"),
    ("STEYX", "EPADYX"),
    ("SUBSTITUTE", "SUBSTITUIR"),
    ("SUM", "SOMA"),
    ("SUMIF", "SOMASE"),
    ("SUMIFS", "SOMASES"),
    ("SUMPRODUCT", "SOMARPRODUTO"),
    ("SUMSQ", "SOMAQUAD"),
    ("SUMX2MY2", "SOMAX2DY2"),
    ("SUMX2PY2", "SOMAX2SY2"),
    ("SUMXMY2", "SOMAXMY2"),
    ("SWITCH", "PARÂMETRO"),
    ("SYD", "SDA"),
    ("T.DIST", "DIST.T"),
    ("T.DIST.2T", "DIST.T.BC"),
    ("T.DIST.RT", "DIST.T.CD"),
    ("T.INV", "INV.T"),
    ("T.INV.2T", "INV.T.BC"),
    ("T.TEST", "TESTE.T"),
    ("TAN", "TAN"),
    ("TANH", "TANH"),
    ("TBILLEQ", "OTN"),
    ("TBILLPRICE", "OTNVALOR"),
    ("TBILLYIELD", "OTNLUCRO"),
    ("TDIST", "DISTT"),
    ("TEXT", "TEXTO"),
    ("TEXTAFTER", "TEXTODEPOIS"),
    ("TEXTBEFORE", "TEXTOANTES"),
    ("TEXTJOIN", "UNIRTEXTO"),
    ("TIME", "TEMPO"),
    ("TIMEVALUE", "VALOR.TEMPO"),
    ("TINV", "INVT"),
    ("TODAY", "HOJE"),
    ("TRANSPOSE", "TRANSPOR"),
    ("TREND", "TENDÊNCIA"),
    ("TRIM", "ARRUMAR"),
    ("TRIMMEAN", "MÉDIA.INTERNA"),
    ("TRIMRANGE", "INTERVALAR"),
    ("TRUE", "VERDADEIRO"),
    ("TRUNC", "TRUNCAR"),
    ("TTEST", "TESTET"),
    ("TYPE", "TIPO"),
    ("UNICHAR", "CARACTUNICODE"),
    ("UNIQUE", "ÚNICO"),
    ("UPPER", "MAIÚSCULA"),
    ("VALUE", "VALOR"),
    ("VALUETOTEXT", "VALORPARATEXTO"),
    ("VAR", "VAREST"),
    ("VAR.S", "VAR.A"),
    ("VDB", "BDV"),
    ("VLOOKUP", "PROCV"),
    ("VSTACK", "EMPILHARV"),
    ("WEBSERVICE", "SERVIÇOWEB"),
    ("WEEKDAY", "DIA.DA.SEMANA"),
    ("WEEKNUM", "NÚMSEMANA"),
    ("WEIBULL.DIST", "DIST.WEIBULL"),
    ("WORKDAY", "DIATRABALHO"),
    ("WORKDAY.INTL", "DIATRABALHO.INTL"),
    ("WRAPCOLS", "QUEBRARCOLS"),
    ("XIRR", "XTIR"),
    ("XLOOKUP", "PROCX"),
    ("XMATCH", "CORRESPX"),
    ("XNPV", "XVPL"),
    ("YEAR", "ANO"),
    ("YEARFRAC", "FRAÇÃOANO"),
    ("YIELD", "LUCRO"),
    ("YIELDDISC", "LUCRODESC"),
    ("YIELDMAT", "LUCROVENC"),
    ("Z.TEST", "TESTE.Z"),
    ("ZTEST", "TESTEZ"),
];

/// Brazilian Portuguese structured-reference specifiers, as the canonical printer spells them.
const PORTUGUESE_STRUCT_ITEMS: &[(&str, &str)] =
    &[("#All", "#Tudo"), ("#Data", "#Dados"), ("#Headers", "#Cabeçalhos"), ("#Totals", "#Totais"), ("#This Row", "#Esta linha")];

/// Case-insensitive name comparison that also folds accented letters (`média` is `MÉDIA`).
fn same_name(a: &str, b: &str) -> bool {
    a.chars().flat_map(char::to_uppercase).eq(b.chars().flat_map(char::to_uppercase))
}

impl FormulaLocale {
    pub fn list_separator(self) -> char {
        if matches!(self, Self::Es | Self::Pt) { ';' } else { ',' }
    }
    pub fn decimal_separator(self) -> char {
        if matches!(self, Self::Es | Self::Pt) { ',' } else { '.' }
    }
    pub fn array_column_separator(self) -> char {
        if matches!(self, Self::Es | Self::Pt) { '\\' } else { ',' }
    }
    fn names(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::Pt => PORTUGUESE,
        }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        self.names().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        self.names().iter().find(|(_, local)| same_name(local, localized)).map(|(en, _)| *en).unwrap_or(localized)
    }
    /// How this syntax spells a logical value.
    pub fn boolean_name(self, value: bool) -> &'static str {
        match (self, value) {
            (Self::En, true) => "TRUE",
            (Self::En, false) => "FALSE",
            (Self::Es, true) => "VERDADERO",
            (Self::Pt, true) => "VERDADEIRO",
            (Self::Es | Self::Pt, false) => "FALSO",
        }
    }
    /// The logical value a localized word names (`VERDADEIRO` in Portuguese), any case.
    /// English has no aliases: `TRUE` and `FALSE` are already literals.
    pub fn boolean_alias(self, name: &str) -> Option<bool> {
        if self == Self::En {
            return None;
        }
        [true, false].into_iter().find(|value| same_name(self.boolean_name(*value), name))
    }
    fn struct_items(self) -> &'static [(&'static str, &'static str)] {
        if self == Self::Pt { PORTUGUESE_STRUCT_ITEMS } else { &[] }
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
            Tok::Struct(_) => match text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
                Some(body) => {
                    out.push('[');
                    out.push_str(&struct_body(body, locale, false));
                    out.push(']');
                }
                None => out.push_str(text),
            },
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

/// Rewrites a structured reference body (the text inside its outer brackets) between the
/// canonical and the localized spelling: specifiers (`[#Headers]`, `[#Cabeçalhos]`) and the
/// separators between bracketed items. Column names stay as written. A syntax without
/// localized specifiers returns the body unchanged.
pub(crate) fn struct_body(body: &str, locale: FormulaLocale, to_canonical: bool) -> String {
    let items = locale.struct_items();
    if items.is_empty() {
        return body.to_string();
    }
    let item = |text: &str| {
        items.iter().find_map(|&(canonical, local)| {
            let (from, to) = if to_canonical { (local, canonical) } else { (canonical, local) };
            same_name(from, text.trim()).then_some(to)
        })
    };
    // Simple form: `[#Headers]`, `[Col]`, `[@Col]`.
    if !body.trim_start().starts_with('[') {
        return item(body).map_or_else(|| body.to_string(), str::to_string);
    }
    // Compound form: every item is bracketed (`[[#Headers],[Col1]:[Col3]]`), so the text
    // between items holds only separators. Inside an item `'` escapes the next character.
    let (from_separator, to_separator) = if to_canonical { (locale.list_separator(), ',') } else { (',', locale.list_separator()) };
    let mut out = String::with_capacity(body.len());
    let mut part = String::new();
    let mut depth = 0usize;
    let mut escaped = false;
    for c in body.chars() {
        if depth == 0 {
            match c {
                '[' => depth = 1,
                c if c == from_separator => out.push(to_separator),
                c => out.push(c),
            }
            continue;
        }
        if escaped {
            escaped = false;
            part.push(c);
            continue;
        }
        match c {
            '\'' => {
                escaped = true;
                part.push(c);
            }
            '[' => {
                depth += 1;
                part.push(c);
            }
            ']' if depth == 1 => {
                depth = 0;
                out.push('[');
                out.push_str(item(&part).unwrap_or(&part));
                out.push(']');
                part.clear();
            }
            ']' => {
                depth -= 1;
                part.push(c);
            }
            c => part.push(c),
        }
    }
    if depth > 0 {
        // Unbalanced: keep the text so the parser reports it.
        out.push('[');
        out.push_str(&part);
    }
    out
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
            let alias = locale.boolean_name(*value);
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
