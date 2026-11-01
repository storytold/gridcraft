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
    ("ABS", "ASS"),
    ("ACCRINT", "INT.MATURATO.PER"),
    ("ACCRINTM", "INT.MATURATO.SCAD"),
    ("ACOS", "ARCCOS"),
    ("ACOSH", "ARCCOSH"),
    ("ACOT", "ARCCOT"),
    ("ACOTH", "ARCCOTH"),
    ("ADDRESS", "INDIRIZZO"),
    ("AGGREGATE", "AGGREGA"),
    ("AMORDEGRC", "AMMORT.DEGR"),
    ("AMORLINC", "AMMORT.PER"),
    ("AND", "E"),
    ("ARABIC", "ARABO"),
    ("AREAS", "AREE"),
    ("ARRAYTOTEXT", "MATRICE.A.TESTO"),
    ("ASIN", "ARCSEN"),
    ("ASINH", "ARCSENH"),
    ("ATAN", "ARCTAN"),
    ("ATAN2", "ARCTAN.2"),
    ("ATANH", "ARCTANH"),
    ("AVEDEV", "MEDIA.DEV"),
    ("AVERAGE", "MEDIA"),
    ("AVERAGEA", "MEDIA.VALORI"),
    ("AVERAGEIF", "MEDIA.SE"),
    ("AVERAGEIFS", "MEDIA.PIÙ.SE"),
    ("BAHTTEXT", "BAHTTESTO"),
    ("BESSELI", "BESSEL.I"),
    ("BESSELJ", "BESSEL.J"),
    ("BESSELK", "BESSEL.K"),
    ("BESSELY", "BESSEL.Y"),
    ("BETA.DIST", "DISTRIB.BETA.N"),
    ("BETA.INV", "INV.BETA.N"),
    ("BETADIST", "DISTRIB.BETA"),
    ("BETAINV", "INV.BETA"),
    ("BIN2DEC", "BINARIO.DECIMALE"),
    ("BIN2HEX", "BINARIO.HEX"),
    ("BIN2OCT", "BINARIO.OCT"),
    ("BINOM.DIST", "DISTRIB.BINOM.N"),
    ("BINOM.DIST.RANGE", "INTERVALLO.DISTRIB.BINOM.N."),
    ("BINOM.INV", "INV.BINOM"),
    ("BINOMDIST", "DISTRIB.BINOM"),
    ("BITLSHIFT", "BIT.SPOSTA.SX"),
    ("BITRSHIFT", "BIT.SPOSTA.DX"),
    ("BYCOL", "PERCOL"),
    ("BYROW", "PERRIGA"),
    ("CEILING", "ARROTONDA.ECCESSO"),
    ("CEILING.MATH", "ARROTONDA.ECCESSO.MAT"),
    ("CEILING.PRECISE", "ARROTONDA.ECCESSO.PRECISA"),
    ("CELL", "CELLA"),
    ("CHAR", "CODICE.CARATT"),
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
    ("CHOOSEROWS", "SCEGLI.RIGA"),
    ("CLEAN", "LIBERA"),
    ("CODE", "CODICE"),
    ("COLUMN", "RIF.COLONNA"),
    ("COLUMNS", "COLONNE"),
    ("COMBIN", "COMBINAZIONE"),
    ("COMBINA", "COMBINAZIONE.VALORI"),
    ("COMPLEX", "COMPLESSO"),
    ("CONCATENATE", "CONCATENA"),
    ("CONFIDENCE", "CONFIDENZA"),
    ("CONFIDENCE.NORM", "CONFIDENZA.NORM"),
    ("CONFIDENCE.T", "CONFIDENZA.T"),
    ("CONVERT", "CONVERTI"),
    ("CORREL", "CORRELAZIONE"),
    ("COUNT", "CONTA.NUMERI"),
    ("COUNTA", "CONTA.VALORI"),
    ("COUNTBLANK", "CONTA.VUOTE"),
    ("COUNTIF", "CONTA.SE"),
    ("COUNTIFS", "CONTA.PIÙ.SE"),
    ("COUPDAYBS", "GIORNI.CED.INIZ.LIQ"),
    ("COUPDAYS", "GIORNI.CED"),
    ("COUPDAYSNC", "GIORNI.CED.NUOVA"),
    ("COUPNCD", "DATA.CED.SUCC"),
    ("COUPNUM", "NUM.CED"),
    ("COUPPCD", "DATA.CED.PREC"),
    ("COVAR", "COVARIANZA"),
    ("COVARIANCE.P", "COVARIANZA.P"),
    ("COVARIANCE.S", "COVARIANZA.C"),
    ("CRITBINOM", "CRIT.BINOM"),
    ("CUBEKPIMEMBER", "MEMBRO.KPI.CUBO"),
    ("CUBEMEMBER", "MEMBRO.CUBO"),
    ("CUBEMEMBERPROPERTY", "PROPRIETÀ.MEMBRO.CUBO"),
    ("CUBERANKEDMEMBER", "MEMBRO.CUBO.CON.RANGO"),
    ("CUBESET", "SET.CUBO"),
    ("CUBESETCOUNT", "CONTA.SET.CUBO"),
    ("CUBEVALUE", "VALORE.CUBO"),
    ("CUMIPMT", "INT.CUMUL"),
    ("CUMPRINC", "CAP.CUM"),
    ("DATE", "DATA"),
    ("DATEDIF", "DATA.DIFF"),
    ("DATEVALUE", "DATA.VALORE"),
    ("DAVERAGE", "DB.MEDIA"),
    ("DAY", "GIORNO"),
    ("DAYS", "GIORNI"),
    ("DAYS360", "GIORNO360"),
    ("DB", "AMMORT.FISSO"),
    ("DCOUNT", "DB.CONTA.NUMERI"),
    ("DCOUNTA", "DB.CONTA.VALORI"),
    ("DDB", "AMMORT"),
    ("DEC2BIN", "DECIMALE.BINARIO"),
    ("DEC2HEX", "DECIMALE.HEX"),
    ("DEC2OCT", "DECIMALE.OCT"),
    ("DECIMAL", "DECIMALE"),
    ("DEGREES", "GRADI"),
    ("DEVSQ", "DEV.Q"),
    ("DGET", "DB.VALORI"),
    ("DISC", "TASSO.SCONTO"),
    ("DMAX", "DB.MAX"),
    ("DMIN", "DB.MIN"),
    ("DOLLAR", "VALUTA"),
    ("DOLLARDE", "VALUTA.DEC"),
    ("DOLLARFR", "VALUTA.FRAZ"),
    ("DPRODUCT", "DB.PRODOTTO"),
    ("DROP", "ESCLUDI"),
    ("DSTDEV", "DB.DEV.ST"),
    ("DSTDEVP", "DB.DEV.ST.POP"),
    ("DSUM", "DB.SOMMA"),
    ("DURATION", "DURATA"),
    ("DVAR", "DB.VAR"),
    ("DVARP", "DB.VAR.POP"),
    ("EDATE", "DATA.MESE"),
    ("EFFECT", "EFFETTIVO"),
    ("ENCODEURL", "CODIFICA.URL"),
    ("EOMONTH", "FINE.MESE"),
    ("ERF", "FUNZ.ERRORE"),
    ("ERF.PRECISE", "FUNZ.ERRORE.PRECISA"),
    ("ERFC", "FUNZ.ERRORE.COMP"),
    ("ERFC.PRECISE", "FUNZ.ERRORE.COMP.PRECISA"),
    ("ERROR.TYPE", "ERRORE.TIPO"),
    ("EVEN", "PARI"),
    ("EXACT", "IDENTICO"),
    ("EXPAND", "ESPANDI"),
    ("EXPON.DIST", "DISTRIB.EXP.N"),
    ("EXPONDIST", "DISTRIB.EXP"),
    ("F.DIST", "DISTRIBF"),
    ("F.DIST.RT", "DISTRIB.F.DS"),
    ("F.INV", "INVF"),
    ("F.INV.RT", "INV.F.DS"),
    ("F.TEST", "TESTF"),
    ("FACT", "FATTORIALE"),
    ("FACTDOUBLE", "FATT.DOPPIO"),
    ("FALSE", "FALSO"),
    ("FDIST", "DISTRIB.F"),
    ("FILTER", "FILTRO"),
    ("FILTERXML", "FILTRO.XML"),
    ("FIND", "TROVA"),
    ("FINDB", "TROVA.B"),
    ("FINV", "INV.F"),
    ("FISHERINV", "INV.FISHER"),
    ("FIXED", "FISSO"),
    ("FLOOR", "ARROTONDA.DIFETTO"),
    ("FLOOR.MATH", "ARROTONDA.DIFETTO.MAT"),
    ("FLOOR.PRECISE", "ARROTONDA.DIFETTO.PRECISA"),
    ("FORECAST", "PREVISIONE"),
    ("FORECAST.ETS", "PREVISIONE.ETS"),
    ("FORECAST.ETS.CONFINT", "PREVISIONE.ETS.INTCONF"),
    ("FORECAST.ETS.SEASONALITY", "PREVISIONE.ETS.STAGIONALITÀ"),
    ("FORECAST.ETS.STAT", "PREVISIONE.ETS.STAT"),
    ("FORECAST.LINEAR", "PREVISIONE.LINEARE"),
    ("FORMULATEXT", "TESTO.FORMULA"),
    ("FREQUENCY", "FREQUENZA"),
    ("FTEST", "TEST.F"),
    ("FV", "VAL.FUT"),
    ("FVSCHEDULE", "VAL.FUT.CAPITALE"),
    ("GAMMA.DIST", "DISTRIB.GAMMA.N"),
    ("GAMMA.INV", "INV.GAMMA.N"),
    ("GAMMADIST", "DISTRIB.GAMMA"),
    ("GAMMAINV", "INV.GAMMA"),
    ("GAMMALN", "LN.GAMMA"),
    ("GAMMALN.PRECISE", "LN.GAMMA.PRECISA"),
    ("GCD", "MCD"),
    ("GEOMEAN", "MEDIA.GEOMETRICA"),
    ("GESTEP", "SOGLIA"),
    ("GROUPBY", "RAGGRUPPAPER"),
    ("GROWTH", "CRESCITA"),
    ("HARMEAN", "MEDIA.ARMONICA"),
    ("HEX2BIN", "HEX.BINARIO"),
    ("HEX2DEC", "HEX.DECIMALE"),
    ("HEX2OCT", "HEX.OCT"),
    ("HLOOKUP", "CERCA.ORIZZ"),
    ("HOUR", "ORA"),
    ("HSTACK", "STACK.ORIZ"),
    ("HYPERLINK", "COLLEG.IPERTESTUALE"),
    ("HYPGEOM.DIST", "DISTRIB.IPERGEOM.N"),
    ("HYPGEOMDIST", "DISTRIB.IPERGEOM"),
    ("IF", "SE"),
    ("IFERROR", "SE.ERRORE"),
    ("IFNA", "SE.NON.DISP."),
    ("IFS", "PIÙ.SE"),
    ("IMABS", "COMP.MODULO"),
    ("IMAGINARY", "COMP.IMMAGINARIO"),
    ("IMARGUMENT", "COMP.ARGOMENTO"),
    ("IMCONJUGATE", "COMP.CONIUGATO"),
    ("IMCOS", "COMP.COS"),
    ("IMCOSH", "COMP.COSH"),
    ("IMCOT", "COMP.COT"),
    ("IMCSC", "COMP.CSC"),
    ("IMCSCH", "COMP.CSCH"),
    ("IMDIV", "COMP.DIV"),
    ("IMEXP", "COMP.EXP"),
    ("IMLN", "COMP.LN"),
    ("IMLOG10", "COMP.LOG10"),
    ("IMLOG2", "COMP.LOG2"),
    ("IMPOWER", "COMP.POTENZA"),
    ("IMPRODUCT", "COMP.PRODOTTO"),
    ("IMREAL", "COMP.PARTE.REALE"),
    ("IMSEC", "COMP.SEC"),
    ("IMSECH", "COMP.SECH"),
    ("IMSIN", "COMP.SEN"),
    ("IMSINH", "COMP.SENH"),
    ("IMSQRT", "COMP.RADQ"),
    ("IMSUB", "COMP.DIFF"),
    ("IMSUM", "COMP.SOMMA"),
    ("IMTAN", "COMP.TAN"),
    ("INDEX", "INDICE"),
    ("INDIRECT", "INDIRETTO"),
    ("INFO", "AMBIENTE.INFO"),
    ("INTERCEPT", "INTERCETTA"),
    ("INTRATE", "TASSO.INT"),
    ("IPMT", "INTERESSI"),
    ("IRR", "TIR.COST"),
    ("ISBLANK", "VAL.VUOTO"),
    ("ISERR", "VAL.ERR"),
    ("ISERROR", "VAL.ERRORE"),
    ("ISEVEN", "VAL.PARI"),
    ("ISFORMULA", "VAL.FORMULA"),
    ("ISLOGICAL", "VAL.LOGICO"),
    ("ISNA", "VAL.NON.DISP"),
    ("ISNONTEXT", "VAL.NON.TESTO"),
    ("ISNUMBER", "VAL.NUMERO"),
    ("ISO.CEILING", "ISO.ARROTONDA.ECCESSO"),
    ("ISODD", "VAL.DISPARI"),
    ("ISOWEEKNUM", "NUM.SETTIMANA.ISO"),
    ("ISPMT", "INTERESSE.RATA"),
    ("ISREF", "VAL.RIF"),
    ("ISTEXT", "VAL.TESTO"),
    ("KURT", "CURTOSI"),
    ("LARGE", "GRANDE"),
    ("LCM", "MCM"),
    ("LEFT", "SINISTRA"),
    ("LEFTB", "SINISTRAB"),
    ("LEN", "LUNGHEZZA"),
    ("LENB", "LUNGB"),
    ("LINEST", "REGR.LIN"),
    ("LOGEST", "REGR.LOG"),
    ("LOGINV", "INV.LOGNORM"),
    ("LOGNORM.DIST", "DISTRIB.LOGNORM.N"),
    ("LOGNORM.INV", "INV.LOGNORM.N"),
    ("LOGNORMDIST", "DISTRIB.LOGNORM"),
    ("LOOKUP", "CERCA"),
    ("LOWER", "MINUSC"),
    ("MATCH", "CONFRONTA"),
    ("MAXA", "MAX.VALORI"),
    ("MAXIFS", "MAX.PIÙ.SE"),
    ("MDETERM", "MATR.DETERM"),
    ("MDURATION", "DURATA.M"),
    ("MEDIAN", "MEDIANA"),
    ("MID", "STRINGA.ESTRAI"),
    ("MIDB", "MEDIA.B"),
    ("MINA", "MIN.VALORI"),
    ("MINIFS", "MIN.PIÙ.SE"),
    ("MINUTE", "MINUTO"),
    ("MINVERSE", "MATR.INVERSA"),
    ("MIRR", "TIR.VAR"),
    ("MMULT", "MATR.PRODOTTO"),
    ("MOD", "RESTO"),
    ("MODE", "MODA"),
    ("MODE.MULT", "MODA.MULT"),
    ("MODE.SNGL", "MODA.SNGL"),
    ("MONTH", "MESE"),
    ("MROUND", "ARROTONDA.MULTIPLO"),
    ("MULTINOMIAL", "MULTINOMIALE"),
    ("MUNIT", "MATR.UNIT"),
    ("N", "NUM"),
    ("NA", "NON.DISP"),
    ("NEGBINOM.DIST", "DISTRIB.BINOM.NEG.N"),
    ("NEGBINOMDIST", "DISTRIB.BINOM.NEG"),
    ("NETWORKDAYS", "GIORNI.LAVORATIVI.TOT"),
    ("NETWORKDAYS.INTL", "GIORNI.LAVORATIVI.TOT.INTL"),
    ("NOMINAL", "NOMINALE"),
    ("NORM.DIST", "DISTRIB.NORM.N"),
    ("NORM.INV", "INV.NORM.N"),
    ("NORM.S.DIST", "DISTRIB.NORM.ST.N"),
    ("NORM.S.INV", "INV.NORM.S"),
    ("NORMDIST", "DISTRIB.NORM"),
    ("NORMINV", "INV.NORM"),
    ("NORMSDIST", "DISTRIB.NORM.ST"),
    ("NORMSINV", "INV.NORM.ST"),
    ("NOT", "NON"),
    ("NOW", "ADESSO"),
    ("NPER", "NUM.RATE"),
    ("NPV", "VAN"),
    ("NUMBERVALUE", "NUMERO.VALORE"),
    ("OCT2BIN", "OCT.BINARIO"),
    ("OCT2DEC", "OCT.DECIMALE"),
    ("OCT2HEX", "OCT.HEX"),
    ("ODD", "DISPARI"),
    ("ODDFPRICE", "PREZZO.PRIMO.IRR"),
    ("ODDFYIELD", "REND.PRIMO.IRR"),
    ("ODDLPRICE", "PREZZO.ULTIMO.IRR"),
    ("ODDLYIELD", "REND.ULTIMO.IRR"),
    ("OFFSET", "SCARTO"),
    ("OR", "O"),
    ("PDURATION", "DURATA.P"),
    ("PERCENTILE.EXC", "ESC.PERCENTILE"),
    ("PERCENTILE.INC", "INC.PERCENTILE"),
    ("PERCENTOF", "PERCENTUALE.DI"),
    ("PERCENTRANK", "PERCENT.RANGO"),
    ("PERCENTRANK.EXC", "ESC.PERCENT.RANGO"),
    ("PERCENTRANK.INC", "INC.PERCENT.RANGO"),
    ("PERMUT", "PERMUTAZIONE"),
    ("PERMUTATIONA", "PERMUTAZIONE.VALORI"),
    ("PHONETIC", "FURIGANA"),
    ("PI", "PI.GRECO"),
    ("PIVOTBY", "PIVOTPER"),
    ("PMT", "RATA"),
    ("POISSON.DIST", "DISTRIB.POISSON"),
    ("POWER", "POTENZA"),
    ("PPMT", "P.RATA"),
    ("PRICE", "PREZZO"),
    ("PRICEDISC", "PREZZO.SCONT"),
    ("PRICEMAT", "PREZZO.SCAD"),
    ("PROB", "PROBABILITÀ"),
    ("PRODUCT", "PRODOTTO"),
    ("PROPER", "MAIUSC.INIZ"),
    ("PV", "VA"),
    ("QUARTILE.EXC", "ESC.QUARTILE"),
    ("QUARTILE.INC", "INC.QUARTILE"),
    ("QUOTIENT", "QUOZIENTE"),
    ("RADIANS", "RADIANTI"),
    ("RAND", "CASUALE"),
    ("RANDARRAY", "MATR.CASUALE"),
    ("RANDBETWEEN", "CASUALE.TRA"),
    ("RANK", "RANGO"),
    ("RANK.AVG", "RANGO.MEDIA"),
    ("RANK.EQ", "RANGO.UG"),
    ("RATE", "TASSO"),
    ("RECEIVED", "RICEV.SCAD"),
    ("REGEXEXTRACT", "REGEX.ESTRAI"),
    ("REGEXREPLACE", "REGEX.SOSTITUISCI"),
    ("REGEXTEST", "REGEX.TEST"),
    ("REPLACE", "RIMPIAZZA"),
    ("REPLACEB", "SOSTITUISCI.B"),
    ("REPT", "RIPETI"),
    ("RIGHT", "DESTRA"),
    ("RIGHTB", "DESTRA.B"),
    ("ROMAN", "ROMANO"),
    ("ROUND", "ARROTONDA"),
    ("ROUNDDOWN", "ARROTONDA.PER.DIF"),
    ("ROUNDUP", "ARROTONDA.PER.ECC"),
    ("ROW", "RIF.RIGA"),
    ("ROWS", "RIGHE"),
    ("RRI", "RIT.INVEST.EFFETT"),
    ("RSQ", "RQ"),
    ("SEARCH", "RICERCA"),
    ("SEARCHB", "CERCA.B"),
    ("SECOND", "SECONDO"),
    ("SEQUENCE", "SEQUENZA"),
    ("SERIESSUM", "SOMMA.SERIE"),
    ("SHEET", "FOGLIO"),
    ("SHEETS", "FOGLI"),
    ("SIGN", "SEGNO"),
    ("SIN", "SEN"),
    ("SINH", "SENH"),
    ("SKEW", "ASIMMETRIA"),
    ("SKEW.P", "ASIMMETRIA.P"),
    ("SLN", "AMMORT.COST"),
    ("SLOPE", "PENDENZA"),
    ("SMALL", "PICCOLO"),
    ("SORT", "DATI.ORDINA"),
    ("SORTBY", "DATI.ORDINA.PER"),
    ("SQRT", "RADQ"),
    ("SQRTPI", "RADQ.PI.GRECO"),
    ("STANDARDIZE", "NORMALIZZA"),
    ("STDEV", "DEV.ST"),
    ("STDEV.P", "DEV.ST.P"),
    ("STDEV.S", "DEV.ST.C"),
    ("STDEVA", "DEV.ST.VALORI"),
    ("STDEVP", "DEV.ST.POP"),
    ("STDEVPA", "DEV.ST.POP.VALORI"),
    ("STEYX", "ERR.STD.YX"),
    ("SUBSTITUTE", "SOSTITUISCI"),
    ("SUBTOTAL", "SUBTOTALE"),
    ("SUM", "SOMMA"),
    ("SUMIF", "SOMMA.SE"),
    ("SUMIFS", "SOMMA.PIÙ.SE"),
    ("SUMPRODUCT", "MATR.SOMMA.PRODOTTO"),
    ("SUMSQ", "SOMMA.Q"),
    ("SUMX2MY2", "SOMMA.DIFF.Q"),
    ("SUMX2PY2", "SOMMA.SOMMA.Q"),
    ("SUMXMY2", "SOMMA.Q.DIFF"),
    ("SYD", "AMMORT.ANNUO"),
    ("T.DIST", "DISTRIB.T.N"),
    ("T.DIST.2T", "DISTRIB.T.2T"),
    ("T.DIST.RT", "DISTRIB.T.DS"),
    ("T.INV", "INVT"),
    ("T.INV.2T", "INV.T.2T"),
    ("T.TEST", "TESTT"),
    ("TAKE", "INCLUDI"),
    ("TBILLEQ", "BOT.EQUIV"),
    ("TBILLPRICE", "BOT.PREZZO"),
    ("TBILLYIELD", "BOT.REND"),
    ("TDIST", "DISTRIB.T"),
    ("TEXT", "TESTO"),
    ("TEXTAFTER", "TESTO.SUCCESSIVO"),
    ("TEXTBEFORE", "TESTO.PRECEDENTE"),
    ("TEXTJOIN", "TESTO.UNISCI"),
    ("TEXTSPLIT", "DIVIDI.TESTO"),
    ("TIME", "ORARIO"),
    ("TIMEVALUE", "ORARIO.VALORE"),
    ("TINV", "INV.T"),
    ("TOCOL", "A.COL"),
    ("TODAY", "OGGI"),
    ("TOROW", "A.RIGA"),
    ("TRANSPOSE", "MATR.TRASPOSTA"),
    ("TREND", "TENDENZA"),
    ("TRIM", "ANNULLA.SPAZI"),
    ("TRIMMEAN", "MEDIA.TRONCATA"),
    ("TRIMRANGE", "ESCLUDI.VUOTE"),
    ("TRUE", "VERO"),
    ("TRUNC", "TRONCA"),
    ("TTEST", "TEST.T"),
    ("TYPE", "TIPO"),
    ("UNICHAR", "CARATT.UNI"),
    ("UNIQUE", "UNICI"),
    ("UPPER", "MAIUSC"),
    ("VALUE", "VALORE"),
    ("VALUETOTEXT", "VALORE.A.TESTO"),
    ("VAR.S", "VAR.C"),
    ("VARA", "VAR.VALORI"),
    ("VARP", "VAR.POP"),
    ("VARPA", "VAR.POP.VALORI"),
    ("VDB", "AMMORT.VAR"),
    ("VLOOKUP", "CERCA.VERT"),
    ("VSTACK", "STACK.VERT"),
    ("WEBSERVICE", "SERVIZIO.WEB"),
    ("WEEKDAY", "GIORNO.SETTIMANA"),
    ("WEEKNUM", "NUM.SETTIMANA"),
    ("WEIBULL.DIST", "DISTRIB.WEIBULL"),
    ("WORKDAY", "GIORNO.LAVORATIVO"),
    ("WORKDAY.INTL", "GIORNO.LAVORATIVO.INTL"),
    ("WRAPCOLS", "A.CAPO.COL"),
    ("WRAPROWS", "A.CAPO.RIGA"),
    ("XIRR", "TIR.X"),
    ("XLOOKUP", "CERCA.X"),
    ("XMATCH", "CONFRONTA.X"),
    ("XNPV", "VAN.X"),
    ("YEAR", "ANNO"),
    ("YEARFRAC", "FRAZIONE.ANNO"),
    ("YIELD", "REND"),
    ("YIELDDISC", "REND.TITOLI.SCONT"),
    ("YIELDMAT", "REND.SCAD"),
    ("Z.TEST", "TESTZ"),
    ("ZTEST", "TEST.Z"),
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
        let folded = fold_name(localized);
        table.iter().find(|(_, it)| fold_name(it) == folded).map(|(en, _)| *en).unwrap_or(localized)
    }
}

/// Folds a localized name for lookup: upper case, then the diacritics a Romance alias can carry
/// reduced to their base letter. Excel itself is not accent-insensitive, but a keyboard that
/// cannot type `Ù` would otherwise turn `PIÙ.SE` into `#NAME?`, so the unaccented spelling is
/// accepted as well. It only ever adds input that used to fail, never changes what a name means.
fn fold_name(name: &str) -> String {
    name.to_uppercase().chars().map(base_letter).collect()
}

/// The base letter of an accented Latin character, or the character itself.
fn base_letter(ch: char) -> char {
    match ch {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'Ç' => 'C',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'Ñ' => 'N',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'Ý' | 'Ÿ' => 'Y',
        other => other,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables are scanned linearly, so they may hold any order, but every invariant that
    /// would make a lookup wrong is checked here: a name must never repeat the English one (it
    /// would be dead weight), two functions must never share an Italian spelling (the reverse
    /// lookup would be ambiguous), and the accented names must survive case folding.
    #[test]
    fn localized_tables_are_unambiguous() {
        for (locale, table) in [(FormulaLocale::Es, SPANISH), (FormulaLocale::It, ITALIAN)] {
            let mut seen: Vec<String> = Vec::new();
            for (en, localized) in table {
                assert_ne!(en.to_uppercase(), localized.to_uppercase(), "{locale:?}: {en} maps to itself");
                let folded = fold_name(localized);
                assert!(!seen.contains(&folded), "{locale:?}: {localized} is claimed twice");
                seen.push(folded);
                assert_eq!(locale.canonical_name(localized), *en, "{locale:?}: {localized} does not round-trip");
                assert_eq!(locale.function_name(en), *localized, "{locale:?}: {en} does not display as {localized}");
            }
        }
    }

    /// An alias the fold cannot reduce to ASCII would be reachable only with the accent typed,
    /// which is the bug this guards: the next accented name added has to teach `base_letter`
    /// its letter instead of silently going back to accent-sensitive.
    #[test]
    fn every_accented_alias_folds_to_ascii() {
        for (locale, table) in [(FormulaLocale::Es, SPANISH), (FormulaLocale::It, ITALIAN)] {
            for (en, localized) in table {
                let folded = fold_name(localized);
                assert!(folded.is_ascii(), "{locale:?}: {en} -> {localized} folds to {folded}");
                assert_eq!(locale.canonical_name(&folded), *en, "{locale:?}: {folded} does not resolve to {en}");
            }
        }
    }
}
