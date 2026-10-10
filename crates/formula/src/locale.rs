//! Formulas as people see and type them in an interface locale.
//!
//! Formulas are stored, saved and evaluated in en-US (`=SUM(A1,2.5)`). German Excel shows the same
//! formula as `=SUMME(A1;2,5)`: German function names, `;` between arguments, a decimal comma,
//! `WAHR`/`FALSCH`, German error names (`#NV`), `.` between the columns of an array constant and
//! German structured-reference items (`[#Kopfzeilen]`). [`to_local`] and [`from_local`] convert
//! between the two; references, sheet names, defined names and text in quotes never change.
//!
//! Function names are Excel feature names (CLAUDE.md allows them). Functions whose German name
//! isn't listed keep their English name, and English names are always accepted when typing.

use std::collections::HashMap;
use std::sync::OnceLock;

use gridcraft_core::{CellError, Locale};

use crate::lexer::{Tok, tokenize};
use crate::parser::normalize_function_name;

/// German Excel's names for the functions whose name differs from the English one.
const DE_FUNCTIONS: &[(&str, &str)] = &[
    ("ACCRINT", "AUFGELZINS"),
    ("ACCRINTM", "AUFGELZINSF"),
    ("ACOS", "ARCCOS"),
    ("ACOSH", "ARCCOSHYP"),
    ("ACOT", "ARCCOT"),
    ("ACOTH", "ARCCOTHYP"),
    ("ADDRESS", "ADRESSE"),
    ("AGGREGATE", "AGGREGAT"),
    ("AMORDEGRC", "AMORDEGRK"),
    ("AMORLINC", "AMORLINEARK"),
    ("AND", "UND"),
    ("ARABIC", "ARABISCH"),
    ("AREAS", "BEREICHE"),
    ("ARRAYTOTEXT", "MATRIXZUTEXT"),
    ("ASIN", "ARCSIN"),
    ("ASINH", "ARCSINHYP"),
    ("ATAN", "ARCTAN"),
    ("ATAN2", "ARCTAN2"),
    ("ATANH", "ARCTANHYP"),
    ("AVEDEV", "MITTELABW"),
    ("AVERAGE", "MITTELWERT"),
    ("AVERAGEA", "MITTELWERTA"),
    ("AVERAGEIF", "MITTELWERTWENN"),
    ("AVERAGEIFS", "MITTELWERTWENNS"),
    ("BASE", "BASIS"),
    ("BETA.DIST", "BETA.VERT"),
    ("BETADIST", "BETAVERT"),
    ("BIN2DEC", "BININDEZ"),
    ("BIN2HEX", "BININHEX"),
    ("BIN2OCT", "BININOKT"),
    ("BINOM.DIST", "BINOM.VERT"),
    ("BINOM.DIST.RANGE", "BINOM.VERT.BEREICH"),
    ("BINOMDIST", "BINOMVERT"),
    ("BITAND", "BITUND"),
    ("BITLSHIFT", "BITLVERSCHIEB"),
    ("BITOR", "BITODER"),
    ("BITRSHIFT", "BITRVERSCHIEB"),
    ("BITXOR", "BITXODER"),
    ("BYCOL", "NACHSPALTE"),
    ("BYROW", "NACHZEILE"),
    ("CEILING", "OBERGRENZE"),
    ("CEILING.MATH", "OBERGRENZE.MATHEMATIK"),
    ("CEILING.PRECISE", "OBERGRENZE.GENAU"),
    ("CELL", "ZELLE"),
    ("CHAR", "ZEICHEN"),
    ("CHIDIST", "CHIVERT"),
    ("CHISQ.DIST", "CHIQU.VERT"),
    ("CHISQ.DIST.RT", "CHIQU.VERT.RE"),
    ("CHISQ.INV", "CHIQU.INV"),
    ("CHISQ.INV.RT", "CHIQU.INV.RE"),
    ("CHISQ.TEST", "CHIQU.TEST"),
    ("CHOOSE", "WAHL"),
    ("CHOOSECOLS", "SPALTENWAHL"),
    ("CHOOSEROWS", "ZEILENWAHL"),
    ("CLEAN", "SÄUBERN"),
    ("COLUMN", "SPALTE"),
    ("COLUMNS", "SPALTEN"),
    ("COMBIN", "KOMBINATIONEN"),
    ("COMBINA", "KOMBINATIONEN2"),
    ("COMPLEX", "KOMPLEXE"),
    ("CONCAT", "TEXTKETTE"),
    ("CONCATENATE", "VERKETTEN"),
    ("CONFIDENCE", "KONFIDENZ"),
    ("CONFIDENCE.NORM", "KONFIDENZ.NORM"),
    ("CONFIDENCE.T", "KONFIDENZ.T"),
    ("CONVERT", "UMWANDELN"),
    ("CORREL", "KORREL"),
    ("COSH", "COSHYP"),
    ("COTH", "COTHYP"),
    ("COUNT", "ANZAHL"),
    ("COUNTA", "ANZAHL2"),
    ("COUNTBLANK", "ANZAHLLEEREZELLEN"),
    ("COUNTIF", "ZÄHLENWENN"),
    ("COUNTIFS", "ZÄHLENWENNS"),
    ("COUPDAYBS", "ZINSTERMTAGVA"),
    ("COUPDAYS", "ZINSTERMTAGE"),
    ("COUPDAYSNC", "ZINSTERMTAGNZ"),
    ("COUPNCD", "ZINSTERMNZ"),
    ("COUPNUM", "ZINSTERMZAHL"),
    ("COUPPCD", "ZINSTERMVZ"),
    ("COVAR", "KOVAR"),
    ("COVARIANCE.P", "KOVARIANZ.P"),
    ("COVARIANCE.S", "KOVARIANZ.S"),
    ("CRITBINOM", "KRITBINOM"),
    ("CSC", "COSEC"),
    ("CSCH", "COSECHYP"),
    ("CUBEKPIMEMBER", "CUBEKPIELEMENT"),
    ("CUBEMEMBER", "CUBEELEMENT"),
    ("CUBEMEMBERPROPERTY", "CUBEELEMENTEIGENSCHAFT"),
    ("CUBERANKEDMEMBER", "CUBERANGELEMENT"),
    ("CUBESET", "CUBEMENGE"),
    ("CUBESETCOUNT", "CUBEMENGENANZAHL"),
    ("CUBEVALUE", "CUBEWERT"),
    ("CUMIPMT", "KUMZINSZ"),
    ("CUMPRINC", "KUMKAPITAL"),
    ("DATE", "DATUM"),
    ("DATEVALUE", "DATWERT"),
    ("DAVERAGE", "DBMITTELWERT"),
    ("DAY", "TAG"),
    ("DAYS", "TAGE"),
    ("DAYS360", "TAGE360"),
    ("DB", "GDA2"),
    ("DCOUNT", "DBANZAHL"),
    ("DCOUNTA", "DBANZAHL2"),
    ("DDB", "GDA"),
    ("DEC2BIN", "DEZINBIN"),
    ("DEC2HEX", "DEZINHEX"),
    ("DEC2OCT", "DEZINOKT"),
    ("DECIMAL", "DEZIMAL"),
    ("DEGREES", "GRAD"),
    ("DEVSQ", "SUMQUADABW"),
    ("DGET", "DBAUSZUG"),
    ("DISC", "DISAGIO"),
    ("DMAX", "DBMAX"),
    ("DMIN", "DBMIN"),
    ("DOLLAR", "DM"),
    ("DOLLARDE", "NOTIERUNGDEZ"),
    ("DOLLARFR", "NOTIERUNGBRU"),
    ("DPRODUCT", "DBPRODUKT"),
    ("DROP", "WEGLASSEN"),
    ("DSTDEV", "DBSTDABW"),
    ("DSTDEVP", "DBSTDABWN"),
    ("DSUM", "DBSUMME"),
    ("DVAR", "DBVARIANZ"),
    ("DVARP", "DBVARIANZEN"),
    ("EDATE", "EDATUM"),
    ("EFFECT", "EFFEKTIV"),
    ("ENCODEURL", "URLCODIEREN"),
    ("EOMONTH", "MONATSENDE"),
    ("ERF", "GAUSSFEHLER"),
    ("ERF.PRECISE", "GAUSSF.GENAU"),
    ("ERFC", "GAUSSFKOMPL"),
    ("ERFC.PRECISE", "GAUSSFKOMPL.GENAU"),
    ("ERROR.TYPE", "FEHLER.TYP"),
    ("EVEN", "GERADE"),
    ("EXACT", "IDENTISCH"),
    ("EXPAND", "ERWEITERN"),
    ("EXPON.DIST", "EXPON.VERT"),
    ("EXPONDIST", "EXPONVERT"),
    ("F.DIST", "F.VERT"),
    ("F.DIST.RT", "F.VERT.RE"),
    ("F.INV.RT", "F.INV.RE"),
    ("FACT", "FAKULTÄT"),
    ("FACTDOUBLE", "ZWEIFAKULTÄT"),
    ("FALSE", "FALSCH"),
    ("FDIST", "FVERT"),
    ("FILTERXML", "XMLFILTERN"),
    ("FIND", "FINDEN"),
    ("FINDB", "FINDENB"),
    ("FIXED", "FEST"),
    ("FLOOR", "UNTERGRENZE"),
    ("FLOOR.MATH", "UNTERGRENZE.MATHEMATIK"),
    ("FLOOR.PRECISE", "UNTERGRENZE.GENAU"),
    ("FORECAST", "PROGNOSE"),
    ("FORECAST.LINEAR", "PROGNOSE.LINEAR"),
    ("FORMULATEXT", "FORMELTEXT"),
    ("FREQUENCY", "HÄUFIGKEIT"),
    ("FV", "ZW"),
    ("FVSCHEDULE", "ZW2"),
    ("GAMMA.DIST", "GAMMA.VERT"),
    ("GAMMADIST", "GAMMAVERT"),
    ("GAMMALN.PRECISE", "GAMMALN.GENAU"),
    ("GCD", "GGT"),
    ("GEOMEAN", "GEOMITTEL"),
    ("GESTEP", "GGANZZAHL"),
    ("GROWTH", "VARIATION"),
    ("HARMEAN", "HARMITTEL"),
    ("HEX2BIN", "HEXINBIN"),
    ("HEX2DEC", "HEXINDEZ"),
    ("HEX2OCT", "HEXINOKT"),
    ("HLOOKUP", "WVERWEIS"),
    ("HOUR", "STUNDE"),
    ("HSTACK", "HSTAPELN"),
    ("HYPGEOM.DIST", "HYPGEOM.VERT"),
    ("HYPGEOMDIST", "HYPGEOMVERT"),
    ("IF", "WENN"),
    ("IFERROR", "WENNFEHLER"),
    ("IFNA", "WENNNV"),
    ("IFS", "WENNS"),
    ("IMAGINARY", "IMAGINÄRTEIL"),
    ("IMCONJUGATE", "IMKONJUGIERTE"),
    ("IMCOSH", "IMCOSHYP"),
    ("IMCSC", "IMCOSEC"),
    ("IMCSCH", "IMCOSECHYP"),
    ("IMPOWER", "IMAPOTENZ"),
    ("IMPRODUCT", "IMPRODUKT"),
    ("IMREAL", "IMREALTEIL"),
    ("IMSECH", "IMSECHYP"),
    ("IMSINH", "IMSINHYP"),
    ("IMSQRT", "IMWURZEL"),
    ("IMSUM", "IMSUMME"),
    ("INDIRECT", "INDIREKT"),
    ("INT", "GANZZAHL"),
    ("INTERCEPT", "ACHSENABSCHNITT"),
    ("INTRATE", "ZINSSATZ"),
    ("IPMT", "ZINSZ"),
    ("IRR", "IKV"),
    ("ISBLANK", "ISTLEER"),
    ("ISERR", "ISTFEHL"),
    ("ISERROR", "ISTFEHLER"),
    ("ISEVEN", "ISTGERADE"),
    ("ISFORMULA", "ISTFORMEL"),
    ("ISLOGICAL", "ISTLOG"),
    ("ISNA", "ISTNV"),
    ("ISNONTEXT", "ISTKTEXT"),
    ("ISNUMBER", "ISTZAHL"),
    ("ISO.CEILING", "ISO.OBERGRENZE"),
    ("ISODD", "ISTUNGERADE"),
    ("ISOMITTED", "ISTAUSGELASSEN"),
    ("ISOWEEKNUM", "ISOKALENDERWOCHE"),
    ("ISREF", "ISTBEZUG"),
    ("ISTEXT", "ISTTEXT"),
    ("LARGE", "KGRÖSSTE"),
    ("LCM", "KGV"),
    ("LEFT", "LINKS"),
    ("LEFTB", "LINKSB"),
    ("LEN", "LÄNGE"),
    ("LENB", "LÄNGEB"),
    ("LINEST", "RGP"),
    ("LOGEST", "RKP"),
    ("LOGNORM.DIST", "LOGNORM.VERT"),
    ("LOGNORMDIST", "LOGNORMVERT"),
    ("LOOKUP", "VERWEIS"),
    ("LOWER", "KLEIN"),
    ("MAKEARRAY", "MATRIXERSTELLEN"),
    ("MAP", "ZUORDNEN"),
    ("MATCH", "VERGLEICH"),
    ("MAXIFS", "MAXWENNS"),
    ("MDETERM", "MDET"),
    ("MID", "TEIL"),
    ("MIDB", "TEILB"),
    ("MINIFS", "MINWENNS"),
    ("MINVERSE", "MINV"),
    ("MIRR", "QIKV"),
    ("MOD", "REST"),
    ("MODE", "MODALWERT"),
    ("MODE.MULT", "MODUS.VIELF"),
    ("MODE.SNGL", "MODUS.EINF"),
    ("MONTH", "MONAT"),
    ("MROUND", "VRUNDEN"),
    ("MULTINOMIAL", "POLYNOMIAL"),
    ("MUNIT", "MEINHEIT"),
    ("NA", "NV"),
    ("NEGBINOM.DIST", "NEGBINOM.VERT"),
    ("NEGBINOMDIST", "NEGBINOMVERT"),
    ("NETWORKDAYS", "NETTOARBEITSTAGE"),
    ("NETWORKDAYS.INTL", "NETTOARBEITSTAGE.INTL"),
    ("NORM.DIST", "NORM.VERT"),
    ("NORM.S.DIST", "NORM.S.VERT"),
    ("NORMDIST", "NORMVERT"),
    ("NORMSDIST", "STANDNORMVERT"),
    ("NORMSINV", "STANDNORMINV"),
    ("NOT", "NICHT"),
    ("NOW", "JETZT"),
    ("NPER", "ZZR"),
    ("NPV", "NBW"),
    ("NUMBERVALUE", "ZAHLENWERT"),
    ("OCT2BIN", "OKTINBIN"),
    ("OCT2DEC", "OKTINDEZ"),
    ("OCT2HEX", "OKTINHEX"),
    ("ODD", "UNGERADE"),
    ("OFFSET", "BEREICH.VERSCHIEBEN"),
    ("OR", "ODER"),
    ("PERCENTILE", "QUANTIL"),
    ("PERCENTILE.EXC", "QUANTIL.EXKL"),
    ("PERCENTILE.INC", "QUANTIL.INKL"),
    ("PERCENTRANK", "QUANTILSRANG"),
    ("PERCENTRANK.EXC", "QUANTILSRANG.EXKL"),
    ("PERCENTRANK.INC", "QUANTILSRANG.INKL"),
    ("PERMUT", "VARIATIONEN"),
    ("PERMUTATIONA", "VARIATIONEN2"),
    ("PMT", "RMZ"),
    ("POISSON.DIST", "POISSON.VERT"),
    ("POWER", "POTENZ"),
    ("PPMT", "KAPZ"),
    ("PRICE", "KURS"),
    ("PRICEDISC", "KURSDISAGIO"),
    ("PRICEMAT", "KURSFÄLLIG"),
    ("PROB", "WAHRSCHBEREICH"),
    ("PRODUCT", "PRODUKT"),
    ("PROPER", "GROSS2"),
    ("PV", "BW"),
    ("QUARTILE.EXC", "QUARTILE.EXKL"),
    ("QUARTILE.INC", "QUARTILE.INKL"),
    ("RADIANS", "BOGENMASS"),
    ("RAND", "ZUFALLSZAHL"),
    ("RANDARRAY", "ZUFALLSMATRIX"),
    ("RANDBETWEEN", "ZUFALLSBEREICH"),
    ("RANK", "RANG"),
    ("RANK.AVG", "RANG.MITTELW"),
    ("RANK.EQ", "RANG.GLEICH"),
    ("RATE", "ZINS"),
    ("RECEIVED", "AUSZAHLUNG"),
    ("REPLACE", "ERSETZEN"),
    ("REPLACEB", "ERSETZENB"),
    ("REPT", "WIEDERHOLEN"),
    ("RIGHT", "RECHTS"),
    ("RIGHTB", "RECHTSB"),
    ("ROMAN", "RÖMISCH"),
    ("ROUND", "RUNDEN"),
    ("ROUNDDOWN", "ABRUNDEN"),
    ("ROUNDUP", "AUFRUNDEN"),
    ("ROW", "ZEILE"),
    ("ROWS", "ZEILEN"),
    ("RRI", "ZSATZINVEST"),
    ("RSQ", "BESTIMMTHEITSMASS"),
    ("SEARCH", "SUCHEN"),
    ("SEARCHB", "SUCHENB"),
    ("SECH", "SECHYP"),
    ("SECOND", "SEKUNDE"),
    ("SEQUENCE", "SEQUENZ"),
    ("SERIESSUM", "POTENZREIHE"),
    ("SHEET", "BLATT"),
    ("SHEETS", "BLÄTTER"),
    ("SIGN", "VORZEICHEN"),
    ("SINH", "SINHYP"),
    ("SKEW", "SCHIEFE"),
    ("SKEW.P", "SCHIEFE.P"),
    ("SLN", "LIA"),
    ("SLOPE", "STEIGUNG"),
    ("SMALL", "KKLEINSTE"),
    ("SORT", "SORTIEREN"),
    ("SORTBY", "SORTIERENNACH"),
    ("SQRT", "WURZEL"),
    ("SQRTPI", "WURZELPI"),
    ("STANDARDIZE", "STANDARDISIERUNG"),
    ("STDEV", "STABW"),
    ("STDEV.P", "STABW.N"),
    ("STDEV.S", "STABW.S"),
    ("STDEVA", "STABWA"),
    ("STDEVP", "STABWN"),
    ("STDEVPA", "STABWNA"),
    ("STEYX", "STFEHLERYX"),
    ("SUBSTITUTE", "WECHSELN"),
    ("SUBTOTAL", "TEILERGEBNIS"),
    ("SUM", "SUMME"),
    ("SUMIF", "SUMMEWENN"),
    ("SUMIFS", "SUMMEWENNS"),
    ("SUMPRODUCT", "SUMMENPRODUKT"),
    ("SUMSQ", "QUADRATESUMME"),
    ("SUMX2MY2", "SUMMEX2MY2"),
    ("SUMX2PY2", "SUMMEX2PY2"),
    ("SUMXMY2", "SUMMEXMY2"),
    ("SWITCH", "ERSTERWERT"),
    ("SYD", "DIA"),
    ("T.DIST", "T.VERT"),
    ("T.DIST.2T", "T.VERT.2S"),
    ("T.DIST.RT", "T.VERT.RE"),
    ("T.INV.2T", "T.INV.2S"),
    ("TAKE", "ÜBERNEHMEN"),
    ("TANH", "TANHYP"),
    ("TBILLEQ", "TBILLÄQUIV"),
    ("TBILLPRICE", "TBILLKURS"),
    ("TBILLYIELD", "TBILLRENDITE"),
    ("TDIST", "TVERT"),
    ("TEXTAFTER", "TEXTNACH"),
    ("TEXTBEFORE", "TEXTVOR"),
    ("TEXTJOIN", "TEXTVERKETTEN"),
    ("TEXTSPLIT", "TEXTTEILEN"),
    ("TIME", "ZEIT"),
    ("TIMEVALUE", "ZEITWERT"),
    ("TOCOL", "ZUSPALTE"),
    ("TODAY", "HEUTE"),
    ("TOROW", "ZUZEILE"),
    ("TRANSPOSE", "MTRANS"),
    ("TRIM", "GLÄTTEN"),
    ("TRIMMEAN", "GESTUTZTMITTEL"),
    ("TRUE", "WAHR"),
    ("TRUNC", "KÜRZEN"),
    ("TYPE", "TYP"),
    ("UNICHAR", "UNIZEICHEN"),
    ("UNIQUE", "EINDEUTIG"),
    ("UPPER", "GROSS"),
    ("VALUE", "WERT"),
    ("VALUETOTEXT", "WERTZUTEXT"),
    ("VAR", "VARIANZ"),
    ("VARA", "VARIANZA"),
    ("VARP", "VARIANZEN"),
    ("VARPA", "VARIANZENA"),
    ("VLOOKUP", "SVERWEIS"),
    ("VSTACK", "VSTAPELN"),
    ("WEBSERVICE", "WEBDIENST"),
    ("WEEKDAY", "WOCHENTAG"),
    ("WEEKNUM", "KALENDERWOCHE"),
    ("WEIBULL.DIST", "WEIBULL.VERT"),
    ("WORKDAY", "ARBEITSTAG"),
    ("WORKDAY.INTL", "ARBEITSTAG.INTL"),
    ("WRAPCOLS", "SPALTENUMBRUCH"),
    ("WRAPROWS", "ZEILENUMBRUCH"),
    ("XIRR", "XINTZINSFUSS"),
    ("XLOOKUP", "XVERWEIS"),
    ("XMATCH", "XVERGLEICH"),
    ("XNPV", "XKAPITALWERT"),
    ("XOR", "XODER"),
    ("YEAR", "JAHR"),
    ("YEARFRAC", "BRTEILJAHRE"),
    ("YIELD", "RENDITE"),
    ("YIELDDISC", "RENDITEDIS"),
    ("YIELDMAT", "RENDITEFÄLL"),
    ("Z.TEST", "G.TEST"),
    ("ZTEST", "GTEST"),
];

/// Structured-reference items: `[#Headers]` is `[#Kopfzeilen]` in German.
const DE_STRUCT_ITEMS: &[(&str, &str)] =
    &[("#All", "#Alle"), ("#Data", "#Daten"), ("#Headers", "#Kopfzeilen"), ("#Totals", "#Ergebnisse"), ("#This Row", "#Diese Zeile")];

struct Names {
    to_local: HashMap<&'static str, &'static str>,
    from_local: HashMap<String, &'static str>,
}

fn names(loc: Locale) -> Option<&'static Names> {
    static DE: OnceLock<Names> = OnceLock::new();
    match loc {
        Locale::EnUs => None,
        Locale::De => Some(DE.get_or_init(|| Names {
            to_local: DE_FUNCTIONS.iter().copied().collect(),
            from_local: DE_FUNCTIONS.iter().map(|(en, de)| (de.to_uppercase(), *en)).collect(),
        })),
    }
}

/// A function's name in `loc` (`SUM` → `SUMME`). Names without a translation come back as is.
pub fn function_name(en: &str, loc: Locale) -> String {
    let key = normalize_function_name(en);
    match names(loc).and_then(|n| n.to_local.get(key.as_str())) {
        Some(local) => (*local).to_string(),
        None => en.to_string(),
    }
}

/// The English function a name typed in `loc` stands for (`summe` → `SUM`). `None` when the
/// name is not a translated one (English names are used as typed).
pub fn function_from_local(name: &str, loc: Locale) -> Option<&'static str> {
    names(loc).and_then(|n| n.from_local.get(&name.to_uppercase()).copied())
}

/// The en-US pairs with a German name, for listing and tests.
pub fn german_function_names() -> &'static [(&'static str, &'static str)] {
    DE_FUNCTIONS
}

fn struct_item(item: &str, to_local: bool) -> Option<&'static str> {
    DE_STRUCT_ITEMS.iter().find_map(|(en, de)| {
        let (from, to) = if to_local { (*en, *de) } else { (*de, *en) };
        item.trim().eq_ignore_ascii_case(from).then_some(to)
    })
}

/// The text inside a structured reference's brackets (`#Headers`, `[#Headers],[Region]`) with
/// its special items and the separator between items translated. Column names are kept.
fn struct_items(body: &str, loc: Locale, to_local: bool) -> String {
    if loc.is_en() {
        return body.to_string();
    }
    if !body.contains('[') {
        return struct_item(body, to_local).map(str::to_string).unwrap_or_else(|| body.to_string());
    }
    let (from_sep, to_sep) = if to_local { (',', loc.list_separator()) } else { (loc.list_separator(), ',') };
    let mut out = String::with_capacity(body.len() + 8);
    let mut item = String::new();
    let mut depth = 0usize;
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            // `'` escapes the next character of a column name.
            '\'' if depth > 0 => {
                item.push(c);
                if let Some(d) = chars.next() {
                    item.push(d);
                }
            }
            '[' => {
                if depth > 0 {
                    item.push(c);
                }
                depth += 1;
            }
            ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    out.push('[');
                    match struct_item(&item, to_local) {
                        Some(name) => out.push_str(name),
                        None => out.push_str(&item),
                    }
                    out.push(']');
                    item.clear();
                } else {
                    item.push(c);
                }
            }
            c if depth > 0 => item.push(c),
            c if c == from_sep => out.push(to_sep),
            c => out.push(c),
        }
    }
    if depth > 0 {
        // Unbalanced: keep what was there.
        out.push('[');
        out.push_str(&item);
    }
    out
}

/// An en-US formula (with or without the leading `=`) as written in `loc`. A formula that does
/// not tokenize is returned unchanged.
pub fn to_local(formula: &str, loc: Locale) -> String {
    if loc.is_en() {
        return formula.to_string();
    }
    let (eq, body) = match formula.strip_prefix('=') {
        Some(b) => ("=", b),
        None => ("", formula),
    };
    let Ok(tokens) = tokenize(body) else { return formula.to_string() };
    let mut out = String::with_capacity(formula.len() + 8);
    out.push_str(eq);
    let mut braces = 0usize;
    let mut last = 0usize;
    for t in &tokens {
        let src = body.get(t.start..t.end).unwrap_or("");
        // Text between tokens (there is none in practice) is copied as is.
        out.push_str(body.get(last..t.start).unwrap_or(""));
        last = t.end;
        match &t.tok {
            Tok::Number(_) => out.push_str(&loc.number_literal(src)),
            Tok::Bool(b) => out.push_str(loc.bool_name(*b)),
            Tok::Error(e) => out.push_str(loc.error_name(*e)),
            Tok::Func(name) => {
                out.push_str(&function_name(name, loc));
                out.push('(');
            }
            Tok::Struct(inner) => {
                out.push('[');
                out.push_str(&struct_items(inner, loc, true));
                out.push(']');
            }
            Tok::Comma => out.push(if braces > 0 { loc.array_column_separator() } else { loc.list_separator() }),
            Tok::LBrace => {
                braces += 1;
                out.push_str(src);
            }
            Tok::RBrace => {
                braces = braces.saturating_sub(1);
                out.push_str(src);
            }
            _ => out.push_str(src),
        }
    }
    out.push_str(body.get(last..).unwrap_or(""));
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '\\' | '?')
}

/// A formula typed in `loc` (with or without the leading `=`) as en-US. Never fails: anything
/// it doesn't recognise is passed through for the parser to judge.
pub fn from_local(text: &str, loc: Locale) -> String {
    if loc.is_en() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let at = |i: usize| chars.get(i).copied();
    let mut out = String::with_capacity(text.len());
    let mut braces = 0usize;
    let mut i = 0usize;
    // A leading `=` (or `+`/`-` start) is copied like any operator.
    while i < n {
        let Some(c) = at(i) else { break };
        match c {
            '"' | '\'' => {
                // Text, or a quoted sheet name: copied up to the closing quote (doubled quotes
                // inside are escapes).
                out.push(c);
                i += 1;
                while let Some(d) = at(i) {
                    out.push(d);
                    i += 1;
                    if d == c {
                        if at(i) == Some(c) {
                            out.push(c);
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            '[' => {
                let mut depth = 0usize;
                let mut j = i;
                let mut closed = false;
                while let Some(d) = at(j) {
                    match d {
                        '\'' => j += 1,
                        '[' => depth += 1,
                        ']' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                closed = true;
                                j += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let j = j.min(n);
                if closed {
                    let body: String = chars.get(i + 1..j.saturating_sub(1)).map(|s| s.iter().collect()).unwrap_or_default();
                    out.push('[');
                    out.push_str(&struct_items(&body, loc, false));
                    out.push(']');
                } else {
                    let rest: String = chars.get(i..j).map(|s| s.iter().collect()).unwrap_or_default();
                    out.push_str(&rest);
                }
                i = j.max(i + 1);
            }
            '#' => {
                let rest: String = chars.get(i..).map(|s| s.iter().take(16).collect()).unwrap_or_default();
                let upper = rest.to_uppercase();
                let found = CellError::ALL
                    .iter()
                    .chain([CellError::Circ].iter())
                    .map(|e| (*e, loc.error_name(*e)))
                    .filter(|(_, name)| upper.starts_with(&name.to_uppercase()))
                    .max_by_key(|(_, name)| name.chars().count());
                match found {
                    Some((e, name)) => {
                        out.push_str(e.as_str());
                        i += name.chars().count();
                    }
                    None => {
                        out.push('#');
                        i += 1;
                    }
                }
            }
            '{' => {
                braces += 1;
                out.push(c);
                i += 1;
            }
            '}' => {
                braces = braces.saturating_sub(1);
                out.push(c);
                i += 1;
            }
            ';' => {
                out.push(if braces > 0 { ';' } else { ',' });
                i += 1;
            }
            '.' | '\\' if braces > 0 => {
                out.push(',');
                i += 1;
            }
            d if d.is_ascii_digit() || (d == ',' && at(i + 1).is_some_and(|x| x.is_ascii_digit())) => {
                // A number: digits, a decimal comma, an exponent.
                while let Some(x) = at(i).filter(char::is_ascii_digit) {
                    out.push(x);
                    i += 1;
                }
                if at(i) == Some(',') && at(i + 1).is_some_and(|x| x.is_ascii_digit()) {
                    out.push('.');
                    i += 1;
                    while let Some(x) = at(i).filter(char::is_ascii_digit) {
                        out.push(x);
                        i += 1;
                    }
                }
                if matches!(at(i), Some('e' | 'E'))
                    && (at(i + 1).is_some_and(|x| x.is_ascii_digit())
                        || (matches!(at(i + 1), Some('+' | '-')) && at(i + 2).is_some_and(|x| x.is_ascii_digit())))
                {
                    for _ in 0..2 {
                        if let Some(x) = at(i) {
                            out.push(x);
                            i += 1;
                        }
                    }
                    while let Some(x) = at(i).filter(char::is_ascii_digit) {
                        out.push(x);
                        i += 1;
                    }
                }
            }
            w if is_word_char(w) => {
                let mut j = i;
                while at(j).is_some_and(is_word_char) {
                    j += 1;
                }
                let word: String = chars.get(i..j).map(|s| s.iter().collect()).unwrap_or_default();
                if at(j) == Some('(') {
                    out.push_str(function_from_local(&word, loc).unwrap_or(&word));
                } else if at(j) != Some('[')
                    && at(j) != Some('!')
                    && let Some(b) = loc.parse_bool(&word)
                {
                    out.push_str(Locale::EnUs.bool_name(b));
                } else {
                    out.push_str(&word);
                }
                i = j;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DE: Locale = Locale::De;

    fn round_trip(en: &str, de: &str) {
        assert_eq!(to_local(en, DE), de, "en → de");
        assert_eq!(from_local(de, DE), en, "de → en");
    }

    #[test]
    fn functions_separators_and_decimals() {
        round_trip("=SUM(A1:A3)", "=SUMME(A1:A3)");
        round_trip("=SUM(A1,2.5)", "=SUMME(A1;2,5)");
        round_trip("=IF(A1>0,\"yes, really\",\"no\")", "=WENN(A1>0;\"yes, really\";\"no\")");
        round_trip("=VLOOKUP(A1,Sheet2!A:B,2,FALSE)", "=SVERWEIS(A1;Sheet2!A:B;2;FALSCH)");
        round_trip("=ROUND(1.5E+3*0.25,2)", "=RUNDEN(1,5E+3*0,25;2)");
        round_trip("=AVERAGE('My Sheet, 2'!A1:A9)", "=MITTELWERT('My Sheet, 2'!A1:A9)");
        round_trip("=NORM.S.DIST(1.96,TRUE)", "=NORM.S.VERT(1,96;WAHR)");
        round_trip("=OFFSET(A1,1,1)", "=BEREICH.VERSCHIEBEN(A1;1;1)");
        round_trip("=COUNTIF(B:B,\">=10\")+COUNTIFS(C:C,\"x\")", "=ZÄHLENWENN(B:B;\">=10\")+ZÄHLENWENNS(C:C;\"x\")");
        round_trip("=1:1", "=1:1");
        round_trip("=A1#", "=A1#");
    }

    #[test]
    fn errors_arrays_and_structured_references() {
        round_trip("=IFERROR(A1,#N/A)", "=WENNFEHLER(A1;#NV)");
        round_trip("=ISERROR(#VALUE!)+ISNA(#REF!)", "=ISTFEHLER(#WERT!)+ISTNV(#BEZUG!)");
        round_trip("=SUM({1,2;3,4.5})", "=SUMME({1.2;3.4,5})");
        round_trip("=Sales[[#Headers],[Region]]", "=Sales[[#Kopfzeilen];[Region]]");
        round_trip("=SUM(Sales[Q1])", "=SUMME(Sales[Q1])");
        round_trip("=Sales[[#This Row],[Total]]", "=Sales[[#Diese Zeile];[Total]]");
    }

    #[test]
    fn untranslated_names_and_english_input() {
        // Defined names, references and unknown functions are never renamed.
        round_trip("=Summe+Tax_Rate*A1", "=Summe+Tax_Rate*A1");
        round_trip("=MYFUNC(1)", "=MYFUNC(1)");
        round_trip("=LET(x,1,x+1)", "=LET(x;1;x+1)");
        // English names typed in a German interface still work.
        assert_eq!(from_local("=SUM(A1;2,5)", DE), "=SUM(A1,2.5)");
        assert_eq!(from_local("=summe(a1;b1)", DE), "=SUM(a1,b1)");
        // A leading decimal comma.
        assert_eq!(from_local("=,5*2", DE), "=.5*2");
        assert_eq!(from_local("=WAHR", DE), "=TRUE");
        assert_eq!(to_local("=SUM(", DE), "=SUMME(", "an unfinished call is still shown in German");
        assert_eq!(to_local("=SUM(\"abc", DE), "=SUM(\"abc", "text that doesn't tokenize comes back unchanged");
    }

    #[test]
    fn english_is_unchanged() {
        for f in ["=SUM(A1,2.5)", "=IF(TRUE,#N/A,{1,2})", "anything"] {
            assert_eq!(to_local(f, Locale::EnUs), f);
            assert_eq!(from_local(f, Locale::EnUs), f);
        }
    }

    #[test]
    fn german_names_are_unique_and_do_not_shadow_other_functions() {
        let mut seen = std::collections::HashSet::new();
        let english: std::collections::HashSet<&str> = DE_FUNCTIONS.iter().map(|(en, _)| *en).collect();
        for (en, de) in DE_FUNCTIONS {
            assert!(seen.insert(de.to_uppercase()), "duplicate German name {de}");
            assert_ne!(en, de, "{en}: only names that differ are listed");
            assert!(!english.contains(de), "{de} ({en}) is also the English name of another function");
        }
        assert_eq!(function_name("SUM", DE), "SUMME");
        assert_eq!(function_name("_xlfn.XLOOKUP", DE), "XVERWEIS");
        assert_eq!(function_name("ABS", DE), "ABS");
        assert_eq!(function_from_local("Sverweis", DE), Some("VLOOKUP"));
        assert_eq!(function_from_local("SUM", DE), None);
    }

    #[test]
    fn hostile_input_never_panics() {
        for s in [
            "=",
            "=\"",
            "='",
            "=[",
            "=]",
            "={",
            "=}",
            "=#",
            "=,",
            "=;;;",
            "=1,",
            "=1E",
            "=1E+",
            "=[[[",
            "='a''",
            "=\"\"\"",
            "=SUMME(",
            "=Ä(",
            "=#ÜBERLAUF",
        ] {
            let _ = from_local(s, DE);
            let _ = to_local(s, DE);
        }
    }
}
