//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};
use gridcraft_core::CellError;

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Fr,
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

/// Excel's French function names (unaccented, as Excel spells them). Two aliases are also the
/// English name of another function, as in French Excel: `TRIM` is MIRR (TRIM is `SUPPRESPACE`)
/// and `VAR.P` is VARP (VAR.P is `VAR.P.N`). Functions French Excel leaves in English (ABS,
/// LET, LAMBDA, MAP…) have no row.
const FRENCH: &[(&str, &str)] = &[
    // Logical
    ("IF", "SI"),
    ("IFS", "SI.CONDITIONS"),
    ("IFERROR", "SIERREUR"),
    ("IFNA", "SI.NON.DISP"),
    ("SWITCH", "SI.MULTIPLE"),
    ("AND", "ET"),
    ("OR", "OU"),
    ("NOT", "NON"),
    ("XOR", "OUX"),
    ("TRUE", "VRAI"),
    ("FALSE", "FAUX"),
    // Math and trigonometry
    ("AGGREGATE", "AGREGAT"),
    ("ARABIC", "CHIFFRE.ARABE"),
    ("CEILING", "PLAFOND"),
    ("CEILING.MATH", "PLAFOND.MATH"),
    ("CEILING.PRECISE", "PLAFOND.PRECIS"),
    ("DEGREES", "DEGRES"),
    ("EVEN", "PAIR"),
    ("FLOOR", "PLANCHER"),
    ("FLOOR.MATH", "PLANCHER.MATH"),
    ("FLOOR.PRECISE", "PLANCHER.PRECIS"),
    ("GCD", "PGCD"),
    ("INT", "ENT"),
    ("ISO.CEILING", "ISO.PLAFOND"),
    ("LCM", "PPCM"),
    ("MDETERM", "DETERMAT"),
    ("MINVERSE", "INVERSEMAT"),
    ("MMULT", "PRODUITMAT"),
    ("MROUND", "ARRONDI.AU.MULTIPLE"),
    ("MULTINOMIAL", "MULTINOMIALE"),
    ("MUNIT", "MATRICE.UNITAIRE"),
    ("ODD", "IMPAIR"),
    ("POWER", "PUISSANCE"),
    ("PRODUCT", "PRODUIT"),
    ("RAND", "ALEA"),
    ("RANDARRAY", "TABLEAU.ALEA"),
    ("RANDBETWEEN", "ALEA.ENTRE.BORNES"),
    ("ROMAN", "ROMAIN"),
    ("ROUND", "ARRONDI"),
    ("ROUNDDOWN", "ARRONDI.INF"),
    ("ROUNDUP", "ARRONDI.SUP"),
    ("SERIESSUM", "SOMME.SERIES"),
    ("SIGN", "SIGNE"),
    ("SQRT", "RACINE"),
    ("SQRTPI", "RACINE.PI"),
    ("SUBTOTAL", "SOUS.TOTAL"),
    ("SUM", "SOMME"),
    ("SUMIF", "SOMME.SI"),
    ("SUMIFS", "SOMME.SI.ENS"),
    ("SUMPRODUCT", "SOMMEPROD"),
    ("SUMSQ", "SOMME.CARRES"),
    ("SUMX2MY2", "SOMME.X2MY2"),
    ("SUMX2PY2", "SOMME.X2PY2"),
    ("SUMXMY2", "SOMME.XMY2"),
    ("TRUNC", "TRONQUE"),
    // Statistical
    ("AVEDEV", "ECART.MOYEN"),
    ("AVERAGE", "MOYENNE"),
    ("AVERAGEIF", "MOYENNE.SI"),
    ("AVERAGEIFS", "MOYENNE.SI.ENS"),
    ("BETA.DIST", "LOI.BETA.N"),
    ("BETADIST", "LOI.BETA"),
    ("BETA.INV", "BETA.INVERSE.N"),
    ("BETAINV", "BETA.INVERSE"),
    ("BINOM.DIST", "LOI.BINOMIALE.N"),
    ("BINOMDIST", "LOI.BINOMIALE"),
    ("BINOM.DIST.RANGE", "LOI.BINOMIALE.SERIE"),
    ("BINOM.INV", "LOI.BINOMIALE.INVERSE"),
    ("CHIDIST", "LOI.KHIDEUX"),
    ("CHIINV", "KHIDEUX.INVERSE"),
    ("CHISQ.DIST", "LOI.KHIDEUX.N"),
    ("CHISQ.DIST.RT", "LOI.KHIDEUX.DROITE"),
    ("CHISQ.INV", "LOI.KHIDEUX.INVERSE"),
    ("CHISQ.INV.RT", "LOI.KHIDEUX.INVERSE.DROITE"),
    ("CHITEST", "TEST.KHIDEUX"),
    ("CONFIDENCE", "INTERVALLE.CONFIANCE"),
    ("CONFIDENCE.NORM", "INTERVALLE.CONFIANCE.NORMAL"),
    ("CONFIDENCE.T", "INTERVALLE.CONFIANCE.STUDENT"),
    ("CORREL", "COEFFICIENT.CORRELATION"),
    ("COUNT", "NB"),
    ("COUNTA", "NBVAL"),
    ("COUNTBLANK", "NB.VIDE"),
    ("COUNTIF", "NB.SI"),
    ("COUNTIFS", "NB.SI.ENS"),
    ("COVAR", "COVARIANCE"),
    ("COVARIANCE.P", "COVARIANCE.PEARSON"),
    ("COVARIANCE.S", "COVARIANCE.STANDARD"),
    ("CRITBINOM", "CRITERE.LOI.BINOMIALE"),
    ("DEVSQ", "SOMME.CARRES.ECARTS"),
    ("EXPON.DIST", "LOI.EXPONENTIELLE.N"),
    ("EXPONDIST", "LOI.EXPONENTIELLE"),
    ("F.DIST", "LOI.F.N"),
    ("FDIST", "LOI.F"),
    ("F.DIST.RT", "LOI.F.DROITE"),
    ("F.INV", "INVERSE.LOI.F.N"),
    ("FINV", "INVERSE.LOI.F"),
    ("F.INV.RT", "INVERSE.LOI.F.DROITE"),
    ("FISHERINV", "FISHER.INVERSE"),
    ("FORECAST", "PREVISION"),
    ("FORECAST.ETS", "PREVISION.ETS"),
    ("FORECAST.ETS.CONFINT", "PREVISION.ETS.CONFINT"),
    ("FORECAST.ETS.SEASONALITY", "PREVISION.ETS.CARACTERESAISONNIER"),
    ("FORECAST.ETS.STAT", "PREVISION.ETS.STAT"),
    ("FORECAST.LINEAR", "PREVISION.LINEAIRE"),
    ("FREQUENCY", "FREQUENCE"),
    ("FTEST", "TEST.F"),
    ("GAMMA.DIST", "LOI.GAMMA.N"),
    ("GAMMADIST", "LOI.GAMMA"),
    ("GAMMA.INV", "LOI.GAMMA.INVERSE.N"),
    ("GAMMAINV", "LOI.GAMMA.INVERSE"),
    ("GAMMALN", "LNGAMMA"),
    ("GAMMALN.PRECISE", "LNGAMMA.PRECIS"),
    ("GEOMEAN", "MOYENNE.GEOMETRIQUE"),
    ("GROWTH", "CROISSANCE"),
    ("HARMEAN", "MOYENNE.HARMONIQUE"),
    ("HYPGEOM.DIST", "LOI.HYPERGEOMETRIQUE.N"),
    ("HYPGEOMDIST", "LOI.HYPERGEOMETRIQUE"),
    ("INTERCEPT", "ORDONNEE.ORIGINE"),
    ("KURT", "KURTOSIS"),
    ("LARGE", "GRANDE.VALEUR"),
    ("LINEST", "DROITEREG"),
    ("LOGEST", "LOGREG"),
    ("LOGINV", "LOI.LOGNORMALE.INVERSE"),
    ("LOGNORM.DIST", "LOI.LOGNORMALE.N"),
    ("LOGNORMDIST", "LOI.LOGNORMALE"),
    ("LOGNORM.INV", "LOI.LOGNORMALE.INVERSE.N"),
    ("MAXIFS", "MAX.SI.ENS"),
    ("MEDIAN", "MEDIANE"),
    ("MINIFS", "MIN.SI.ENS"),
    ("MODE.MULT", "MODE.MULTIPLE"),
    ("MODE.SNGL", "MODE.SIMPLE"),
    ("NEGBINOM.DIST", "LOI.BINOMIALE.NEG.N"),
    ("NEGBINOMDIST", "LOI.BINOMIALE.NEG"),
    ("NORM.DIST", "LOI.NORMALE.N"),
    ("NORMDIST", "LOI.NORMALE"),
    ("NORM.INV", "LOI.NORMALE.INVERSE.N"),
    ("NORMINV", "LOI.NORMALE.INVERSE"),
    ("NORM.S.DIST", "LOI.NORMALE.STANDARD.N"),
    ("NORMSDIST", "LOI.NORMALE.STANDARD"),
    ("NORM.S.INV", "LOI.NORMALE.STANDARD.INVERSE.N"),
    ("NORMSINV", "LOI.NORMALE.STANDARD.INVERSE"),
    ("PERCENTILE", "CENTILE"),
    ("PERCENTILE.EXC", "CENTILE.EXCLURE"),
    ("PERCENTILE.INC", "CENTILE.INCLURE"),
    ("PERCENTRANK", "RANG.POURCENTAGE"),
    ("PERCENTOF", "POURCENTAGEDE"),
    ("PERCENTRANK.EXC", "RANG.POURCENTAGE.EXCLURE"),
    ("PERCENTRANK.INC", "RANG.POURCENTAGE.INCLURE"),
    ("PERMUT", "PERMUTATION"),
    ("POISSON", "LOI.POISSON"),
    ("POISSON.DIST", "LOI.POISSON.N"),
    ("PROB", "PROBABILITE"),
    ("QUARTILE.EXC", "QUARTILE.EXCLURE"),
    ("QUARTILE.INC", "QUARTILE.INCLURE"),
    ("RANK", "RANG"),
    ("RANK.AVG", "MOYENNE.RANG"),
    ("RANK.EQ", "EQUATION.RANG"),
    ("RSQ", "COEFFICIENT.DETERMINATION"),
    ("SKEW", "COEFFICIENT.ASYMETRIE"),
    ("SKEW.P", "COEFFICIENT.ASYMETRIE.P"),
    ("SLOPE", "PENTE"),
    ("SMALL", "PETITE.VALEUR"),
    ("STANDARDIZE", "CENTREE.REDUITE"),
    ("STDEV", "ECARTYPE"),
    ("STDEV.P", "ECARTYPE.PEARSON"),
    ("STDEVP", "ECARTYPEP"),
    ("STDEV.S", "ECARTYPE.STANDARD"),
    ("STEYX", "ERREUR.TYPE.XY"),
    ("T.DIST", "LOI.STUDENT.N"),
    ("TDIST", "LOI.STUDENT"),
    ("T.DIST.2T", "LOI.STUDENT.BILATERALE"),
    ("T.DIST.RT", "LOI.STUDENT.DROITE"),
    ("T.INV", "LOI.STUDENT.INVERSE.N"),
    ("TINV", "LOI.STUDENT.INVERSE"),
    ("T.INV.2T", "LOI.STUDENT.INVERSE.BILATERALE"),
    ("TREND", "TENDANCE"),
    ("TRIMMEAN", "MOYENNE.REDUITE"),
    ("TTEST", "TEST.STUDENT"),
    ("VAR.P", "VAR.P.N"),
    ("VARP", "VAR.P"),
    ("WEIBULL", "LOI.WEIBULL"),
    ("WEIBULL.DIST", "LOI.WEIBULL.N"),
    ("ZTEST", "TEST.Z"),
    // Text
    ("ARRAYTOTEXT", "TABLEAU.EN.TEXTE"),
    ("CHAR", "CAR"),
    ("CLEAN", "EPURAGE"),
    ("CONCATENATE", "CONCATENER"),
    ("DOLLAR", "DEVISE"),
    ("FIND", "TROUVE"),
    ("FINDB", "TROUVERB"),
    ("FIXED", "CTXT"),
    ("LEFT", "GAUCHE"),
    ("LEFTB", "GAUCHEB"),
    ("LEN", "NBCAR"),
    ("LOWER", "MINUSCULE"),
    ("MID", "STXT"),
    ("MIDB", "STXTB"),
    ("NUMBERVALUE", "VALEURNOMBRE"),
    ("PHONETIC", "PHONETIQUE"),
    ("PROPER", "NOMPROPRE"),
    ("REPLACE", "REMPLACER"),
    ("REPLACEB", "REMPLACERB"),
    ("RIGHT", "DROITE"),
    ("RIGHTB", "DROITEB"),
    ("SEARCH", "CHERCHE"),
    ("SEARCHB", "CHERCHERB"),
    ("SUBSTITUTE", "SUBSTITUE"),
    ("TEXT", "TEXTE"),
    ("TEXTAFTER", "TEXTE.APRES"),
    ("TEXTBEFORE", "TEXTE.AVANT"),
    ("TEXTJOIN", "JOINDRE.TEXTE"),
    ("TEXTSPLIT", "FRACTIONNER.TEXTE"),
    ("TRIM", "SUPPRESPACE"),
    ("UNICHAR", "UNICAR"),
    ("UPPER", "MAJUSCULE"),
    ("VALUE", "CNUM"),
    ("VALUETOTEXT", "VALEUR.EN.TEXTE"),
    // Date and time
    ("DATEVALUE", "DATEVAL"),
    ("DAY", "JOUR"),
    ("DAYS", "JOURS"),
    ("DAYS360", "JOURS360"),
    ("EDATE", "MOIS.DECALER"),
    ("EOMONTH", "FIN.MOIS"),
    ("HOUR", "HEURE"),
    ("ISOWEEKNUM", "NO.SEMAINE.ISO"),
    ("MONTH", "MOIS"),
    ("NETWORKDAYS", "NB.JOURS.OUVRES"),
    ("NETWORKDAYS.INTL", "NB.JOURS.OUVRES.INTL"),
    ("NOW", "MAINTENANT"),
    ("SECOND", "SECONDE"),
    ("TIME", "TEMPS"),
    ("TIMEVALUE", "TEMPSVAL"),
    ("TODAY", "AUJOURDHUI"),
    ("WEEKDAY", "JOURSEM"),
    ("WEEKNUM", "NO.SEMAINE"),
    ("WORKDAY", "SERIE.JOUR.OUVRE"),
    ("WORKDAY.INTL", "SERIE.JOUR.OUVRE.INTL"),
    ("YEAR", "ANNEE"),
    ("YEARFRAC", "FRACTION.ANNEE"),
    // Lookup and reference
    ("ADDRESS", "ADRESSE"),
    ("AREAS", "ZONES"),
    ("CHOOSE", "CHOISIR"),
    ("CHOOSECOLS", "CHOISIRCOLS"),
    ("CHOOSEROWS", "CHOISIRLIGNES"),
    ("COLUMN", "COLONNE"),
    ("COLUMNS", "COLONNES"),
    ("DROP", "EXCLURE"),
    ("EXPAND", "ETENDRE"),
    ("FILTER", "FILTRE"),
    ("FORMULATEXT", "FORMULETEXTE"),
    ("GROUPBY", "GROUPER.PAR"),
    ("HLOOKUP", "RECHERCHEH"),
    ("HSTACK", "ASSEMB.H"),
    ("HYPERLINK", "LIEN_HYPERTEXTE"),
    ("LOOKUP", "RECHERCHE"),
    ("MATCH", "EQUIV"),
    ("OFFSET", "DECALER"),
    ("PIVOTBY", "PIVOTER.PAR"),
    ("ROW", "LIGNE"),
    ("ROWS", "LIGNES"),
    ("SORT", "TRIER"),
    ("SORTBY", "TRIERPAR"),
    ("TAKE", "PRENDRE"),
    ("VLOOKUP", "RECHERCHEV"),
    ("VSTACK", "ASSEMB.V"),
    ("WRAPCOLS", "ORGA.COLS"),
    ("WRAPROWS", "ORGA.LIGNES"),
    ("TOCOL", "DANSCOL"),
    ("TOROW", "DANSLIGNE"),
    ("XLOOKUP", "RECHERCHEX"),
    ("XMATCH", "EQUIVX"),
    // Information
    ("CELL", "CELLULE"),
    ("ERROR.TYPE", "TYPE.ERREUR"),
    ("INFO", "INFORMATIONS"),
    ("ISBLANK", "ESTVIDE"),
    ("ISERR", "ESTERR"),
    ("ISERROR", "ESTERREUR"),
    ("ISEVEN", "EST.PAIR"),
    ("ISFORMULA", "ESTFORMULE"),
    ("ISLOGICAL", "ESTLOGIQUE"),
    ("ISNA", "ESTNA"),
    ("ISNONTEXT", "ESTNONTEXTE"),
    ("ISNUMBER", "ESTNUM"),
    ("ISODD", "EST.IMPAIR"),
    ("ISREF", "ESTREF"),
    ("ISTEXT", "ESTTEXTE"),
    ("SHEET", "FEUILLE"),
    ("SHEETS", "FEUILLES"),
    // Financial
    ("ACCRINT", "INTERET.ACC"),
    ("ACCRINTM", "INTERET.ACC.MAT"),
    ("COUPDAYBS", "NB.JOURS.COUPON.PREC"),
    ("COUPDAYS", "NB.JOURS.COUPONS"),
    ("COUPDAYSNC", "NB.JOURS.COUPON.SUIV"),
    ("COUPNCD", "DATE.COUPON.SUIV"),
    ("COUPNUM", "NB.COUPONS"),
    ("COUPPCD", "DATE.COUPON.PREC"),
    ("CUMIPMT", "CUMUL.INTER"),
    ("CUMPRINC", "CUMUL.PRINCPER"),
    ("DISC", "TAUX.ESCOMPTE"),
    ("DOLLARDE", "PRIX.DEC"),
    ("DOLLARFR", "PRIX.FRAC"),
    ("DURATION", "DUREE"),
    ("EFFECT", "TAUX.EFFECTIF"),
    ("FV", "VC"),
    ("FVSCHEDULE", "VC.PAIEMENTS"),
    ("INTRATE", "TAUX.INTERET"),
    ("IPMT", "INTPER"),
    ("IRR", "TRI"),
    ("MDURATION", "DUREE.MODIFIEE"),
    ("MIRR", "TRIM"),
    ("NOMINAL", "TAUX.NOMINAL"),
    ("NPER", "NPM"),
    ("NPV", "VAN"),
    ("ODDFPRICE", "PRIX.PCOUPON.IRREG"),
    ("ODDFYIELD", "REND.PCOUPON.IRREG"),
    ("ODDLPRICE", "PRIX.DCOUPON.IRREG"),
    ("ODDLYIELD", "REND.DCOUPON.IRREG"),
    ("PDURATION", "PDUREE"),
    ("PMT", "VPM"),
    ("PPMT", "PRINCPER"),
    ("PRICE", "PRIX.TITRE"),
    ("PRICEDISC", "VALEUR.ENCAISSEMENT"),
    ("PRICEMAT", "PRIX.TITRE.ECHEANCE"),
    ("PV", "VA"),
    ("RATE", "TAUX"),
    ("RECEIVED", "VALEUR.NOMINALE"),
    ("RRI", "TAUX.INT.EQUIV"),
    ("SLN", "AMORLIN"),
    ("TBILLEQ", "TAUX.ESCOMPTE.R"),
    ("TBILLPRICE", "PRIX.BON.TRESOR"),
    ("TBILLYIELD", "RENDEMENT.BON.TRESOR"),
    ("XIRR", "TRI.PAIEMENTS"),
    ("XNPV", "VAN.PAIEMENTS"),
    ("YIELD", "RENDEMENT.TITRE"),
    ("YIELDDISC", "RENDEMENT.SIMPLE"),
    ("YIELDMAT", "RENDEMENT.TITRE.ECHEANCE"),
    // Engineering
    ("BIN2DEC", "BINDEC"),
    ("BIN2HEX", "BINHEX"),
    ("BIN2OCT", "BINOCT"),
    ("BITAND", "BITET"),
    ("BITLSHIFT", "BITDECALG"),
    ("BITOR", "BITOU"),
    ("BITRSHIFT", "BITDECALD"),
    ("BITXOR", "BITOUEXCLUSIF"),
    ("COMPLEX", "COMPLEXE"),
    ("DEC2BIN", "DECBIN"),
    ("DEC2HEX", "DECHEX"),
    ("DEC2OCT", "DECOCT"),
    ("ERF.PRECISE", "ERF.PRECIS"),
    ("ERFC.PRECISE", "ERFC.PRECIS"),
    ("GESTEP", "SUP.SEUIL"),
    ("HEX2BIN", "HEXBIN"),
    ("HEX2DEC", "HEXDEC"),
    ("HEX2OCT", "HEXOCT"),
    ("IMABS", "COMPLEXE.MODULE"),
    ("IMAGINARY", "COMPLEXE.IMAGINAIRE"),
    ("IMARGUMENT", "COMPLEXE.ARGUMENT"),
    ("IMCONJUGATE", "COMPLEXE.CONJUGUE"),
    ("IMCOS", "COMPLEXE.COS"),
    ("IMCOSH", "COMPLEXE.COSH"),
    ("IMCOT", "COMPLEXE.COT"),
    ("IMCSC", "COMPLEXE.CSC"),
    ("IMCSCH", "COMPLEXE.CSCH"),
    ("IMDIV", "COMPLEXE.DIV"),
    ("IMEXP", "COMPLEXE.EXP"),
    ("IMLN", "COMPLEXE.LN"),
    ("IMLOG10", "COMPLEXE.LOG10"),
    ("IMLOG2", "COMPLEXE.LOG2"),
    ("IMPOWER", "COMPLEXE.PUISSANCE"),
    ("IMPRODUCT", "COMPLEXE.PRODUIT"),
    ("IMREAL", "COMPLEXE.REEL"),
    ("IMSEC", "COMPLEXE.SEC"),
    ("IMSECH", "COMPLEXE.SECH"),
    ("IMSIN", "COMPLEXE.SIN"),
    ("IMSINH", "COMPLEXE.SINH"),
    ("IMSQRT", "COMPLEXE.RACINE"),
    ("IMSUB", "COMPLEXE.DIFFERENCE"),
    ("IMSUM", "COMPLEXE.SOMME"),
    ("IMTAN", "COMPLEXE.TAN"),
    ("OCT2BIN", "OCTBIN"),
    ("OCT2DEC", "OCTDEC"),
    ("OCT2HEX", "OCTHEX"),
    // Database
    ("DAVERAGE", "BDMOYENNE"),
    ("DCOUNT", "BDNB"),
    ("DCOUNTA", "BDNBVAL"),
    ("DGET", "BDLIRE"),
    ("DMAX", "BDMAX"),
    ("DMIN", "BDMIN"),
    ("DPRODUCT", "BDPRODUIT"),
    ("DSTDEV", "BDECARTYPE"),
    ("DSTDEVP", "BDECARTYPEP"),
    ("DSUM", "BDSOMME"),
    ("DVAR", "BDVAR"),
    ("DVARP", "BDVARP"),
    // Cube
    ("CUBEKPIMEMBER", "MEMBREKPICUBE"),
    ("CUBEMEMBER", "MEMBRECUBE"),
    ("CUBEMEMBERPROPERTY", "PROPRIETEMEMBRECUBE"),
    ("CUBERANKEDMEMBER", "RANGMEMBRECUBE"),
    ("CUBESET", "JEUCUBE"),
    ("CUBESETCOUNT", "NBJEUCUBE"),
    ("CUBEVALUE", "VALEURCUBE"),
    // Web
    ("ENCODEURL", "URLENCODAGE"),
    ("FILTERXML", "FILTRE.XML"),
    ("WEBSERVICE", "SERVICEWEB"),
];

/// French error spellings; GridCraft's own #CIRC! stays unchanged.
const FRENCH_ERRORS: &[(CellError, &str)] = &[
    (CellError::Null, "#NUL!"),
    (CellError::Div0, "#DIV/0!"),
    (CellError::Value, "#VALEUR!"),
    (CellError::Ref, "#REF!"),
    (CellError::Name, "#NOM?"),
    (CellError::Num, "#NOMBRE!"),
    (CellError::NA, "#N/A"),
    (CellError::GettingData, "#CHARGEMENT_DONNEES"),
    (CellError::Spill, "#PROPAGATION!"),
    (CellError::Calc, "#CALC!"),
];

impl FormulaLocale {
    /// `(canonical English, localized)` function names; names absent from it are spelled alike.
    pub fn function_table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::Fr => FRENCH,
        }
    }
    pub fn list_separator(self) -> char {
        if self == Self::En { ',' } else { ';' }
    }
    pub fn decimal_separator(self) -> char {
        if self == Self::En { '.' } else { ',' }
    }
    pub fn array_column_separator(self) -> char {
        match self {
            Self::En => ',',
            Self::Es => '\\',
            // French Excel writes `{1.2;3.4}`: the decimal comma frees the dot.
            Self::Fr => '.',
        }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        self.function_table().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        self.function_table().iter().find(|(_, local)| local.eq_ignore_ascii_case(localized)).map(|(en, _)| *en).unwrap_or(localized)
    }
    /// Localized error literals; unlisted errors keep their canonical spelling.
    pub(crate) fn errors(self) -> &'static [(CellError, &'static str)] {
        match self {
            Self::Fr => FRENCH_ERRORS,
            Self::En | Self::Es => &[],
        }
    }
    /// The error spelling used when displaying a formula in this language.
    pub fn error_name(self, error: CellError) -> &'static str {
        self.errors().iter().find(|(e, _)| *e == error).map_or(error.as_str(), |(_, name)| *name)
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
            Tok::Error(e) => out.push_str(locale.error_name(e)),
            Tok::Number(_) => out.push_str(&text.replace('.', &locale.decimal_separator().to_string())),
            Tok::Comma => out.push(if array_depth > 0 { locale.array_column_separator() } else { locale.list_separator() }),
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

/// `VERDADERO`/`VRAI` → `true`: only the locale's boolean aliases, never the English words.
pub(crate) fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
    match locale.function_table().iter().find(|(_, local)| local.eq_ignore_ascii_case(name)) {
        Some(("TRUE", _)) => Some(true),
        Some(("FALSE", _)) => Some(false),
        _ => None,
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
            let alias = locale.function_name(if *value { "TRUE" } else { "FALSE" });
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !bound(name, bindings) => {
            if !display && let Some(value) = boolean_alias(name, locale) {
                *expr = Expr::Bool(value);
            } else if locale == FormulaLocale::Fr {
                // GROUPBY/PIVOTBY and LAMBDA helpers also take a function by its bare name.
                let alias = if display { locale.function_name(name) } else { locale.canonical_name(name) };
                if !display || !bound(alias, bindings) {
                    *name = alias.to_string();
                }
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
