//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Ru,
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

const RUSSIAN: &[(&str, &str)] = &[
    ("ABS", "ABS"),
    ("ACCRINT", "НАКОПДОХОД"),
    ("ACCRINTM", "НАКОПДОХОДВУК"),
    ("ACOS", "ACOS"),
    ("ACOSH", "ACOSH"),
    ("ACOT", "ACOT"),
    ("ACOTH", "ACOTH"),
    ("ADDRESS", "АДРЕС"),
    ("AGGREGATE", "АГРЕГАТ"),
    ("AMORDEGR", "АМОРУМ"),
    ("AMORDEGRC", "АМОРУМ"),
    ("AMORLINC", "АМОРУМЛ"),
    ("AND", "И"),
    ("ARABIC", "АРАБСКОЕ"),
    ("ARRAYTOTEXT", "МАССИВВТЕКСТ"),
    ("ASC", "ASC"),
    ("ASIN", "ASIN"),
    ("ASINH", "ASINH"),
    ("ATAN", "ATAN"),
    ("ATAN2", "ATAN2"),
    ("ATANH", "ATANH"),
    ("AVEDEV", "МАОТКЛ"),
    ("AVERAGE", "СРЗНАЧ"),
    ("AVERAGEA", "СРЗНАЧА"),
    ("AVERAGEIF", "СРЗНАЧЕСЛИ"),
    ("AVERAGEIFS", "СРЗНАЧЕСЛИМН"),
    ("BAHTTEXT", "БАХТТЕКСТ"),
    ("BASE", "ОСНОВАНИЕ"),
    ("BESSELI", "БЕССЕЛЬ.I"),
    ("BESSELJ", "БЕССЕЛЬ.J"),
    ("BESSELK", "БЕССЕЛЬ.K"),
    ("BESSELY", "БЕССЕЛЬ.Y"),
    ("BETA.DIST", "БЕТА.РАСП"),
    ("BETA.INV", "БЕТА.ОБР"),
    ("BETADIST", "БЕТАРАСП"),
    ("BETAINV", "БЕТАОБР"),
    ("BIN2DEC", "ДВ.В.ДЕС"),
    ("BIN2HEX", "ДВ.В.ШЕСТ"),
    ("BIN2OCT", "ДВ.В.ВОСМ"),
    ("BINOM.DIST", "БИНОМ.РАСП"),
    ("BINOM.DIST.RANGE", "БИНОМ.РАСП.ДИАП"),
    ("BINOM.INV", "БИНОМ.ОБР"),
    ("BINOMDIST", "БИНОМРАСП"),
    ("BITAND", "БИТ.И"),
    ("BITLSHIFT", "БИТ.СДВИГЛ"),
    ("BITOR", "БИТ.ИЛИ"),
    ("BITRSHIFT", "БИТ.СДВИГП"),
    ("BITXOR", "БИТ.ИСКЛИЛИ"),
    ("BYCOL", "ПОСТОЛБЦОВО"),
    ("BYROW", "ПОСТРОЧНО"),
    ("CEILING", "ОКРВВЕРХ"),
    ("CEILING.MATH", "ОКРВВЕРХ.МАТ"),
    ("CEILING.PRECISE", "ОКРВВЕРХ.ТОЧНО"),
    ("CHAR", "СИМВОЛ"),
    ("CHIDIST", "ХИ2РАСП"),
    ("CHIINV", "ХИ2ОБР"),
    ("CHISQ.DIST", "ХИ2.РАСП"),
    ("CHISQ.DIST.RT", "ХИ2.РАСП.ПРАВ"),
    ("CHISQ.INV", "ХИ2.ОБР"),
    ("CHISQ.INV.RT", "ХИ2.ОБР.ПРАВ"),
    ("CHISQ.TEST", "ХИ2.ТЕСТ"),
    ("CHITEST", "ХИ2ТЕСТ"),
    ("CHOOSE", "ВЫБОР"),
    ("CHOOSECOLS", "ВЫБРАТЬСТОЛБЦЫ"),
    ("CHOOSEROWS", "ВЫБРАТЬСТРОКИ"),
    ("CLEAN", "ПЕЧСИМВ"),
    ("CODE", "КОДСИМВ"),
    ("COLUMNS", "ЧСТОЛБЦОВ"),
    ("COMBIN", "ЧИСЛКОМБ"),
    ("COMBINA", "ЧИСЛКОМБА"),
    ("COMPLEX", "КОМПЛЕКСН"),
    ("CONCAT", "СЦЕП"),
    ("CONCATENATE", "СЦЕПИТЬ"),
    ("CONFIDENCE", "ДОВЕРИТ"),
    ("CONFIDENCE.NORM", "ДОВЕРИТ.НОРМ"),
    ("CONFIDENCE.T", "ДОВЕРИТ.СЧУДЕНТ"),
    ("CONVERT", "ПРЕОБР"),
    ("CORREL", "КОРРЕЛ"),
    ("COS", "COS"),
    ("COSH", "COSH"),
    ("COT", "COT"),
    ("COTH", "COTH"),
    ("COUNT", "СЧЁТ"),
    ("COUNTA", "СЧЁТЗ"),
    ("COUNTBLANK", "СЧИТАТЬПУСТОТЫ"),
    ("COUNTIF", "СЧЁТЕСЛИ"),
    ("COUNTIFS", "СЧЁТЕСЛИМН"),
    ("COUPDAYBS", "ДНЕЙДОКУПОН"),
    ("COUPDAYS", "ДНЕЙКУПОН"),
    ("COUPDAYSNC", "ДНЕЙПОСЛЕКУПОН"),
    ("COUPNCD", "ДНЕЙКУПОНДАТА"),
    ("COUPNUM", "КУПОНЧИСЛО"),
    ("COUPPCD", "КУПОНДАТАДО"),
    ("COVAR", "КОВАР"),
    ("COVARIANCE.P", "КОВАРИАЦИЯ.Г"),
    ("COVARIANCE.S", "КОВАРИАЦИЯ.В"),
    ("CRITBINOM", "КРИТБИНОМ"),
    ("CSC", "CSC"),
    ("CSCH", "CSCH"),
    ("CUBEKPIMEMBER", "КУБЭЛЕМЕНТКПИ"),
    ("CUBEMEMBER", "КУБЭЛЕМЕНТ"),
    ("CUBEMEMBERPROPERTY", "КУБСВОЙСТВОЭЛЕМЕНТА"),
    ("CUBERANKEDMEMBER", "КУБПОРЯДКОВЫЙЭЛЕМЕНТ"),
    ("CUBESET", "КУБМНОЖ"),
    ("CUBESETCOUNT", "КУБЧИСЛОЭЛЕМЕНТОВ"),
    ("CUBEVALUE", "КУБЗНАЧЕНИЕ"),
    ("CUMIPMT", "ВЫПЛАТАПРОЦ"),
    ("CUMPRINC", "ВЫПЛАТАОСН"),
    ("DATE", "ДАТА"),
    ("DATEDIF", "РАЗНДАТ"),
    ("DATEVALUE", "ДАТАЗНАЧ"),
    ("DAVERAGE", "БДСРЗНАЧ"),
    ("DAY", "ДЕНЬ"),
    ("DAYS", "ДНИ"),
    ("DAYS360", "ДНИ360"),
    ("DB", "ФУО"),
    ("DBCS", "DBCS"),
    ("DCOUNT", "БДСЧЁТ"),
    ("DCOUNTA", "БДСЧЁТА"),
    ("DDB", "ДДО"),
    ("DEC2BIN", "ДЕС.В.ДВ"),
    ("DEC2HEX", "ДЕС.В.ШЕСТ"),
    ("DEC2OCT", "ДЕС.В.ВОСМ"),
    ("DECIMAL", "ДЕСЯТИЧНОЕ"),
    ("DEGREES", "ГРАДУСЫ"),
    ("DELTA", "ДЕЛЬТА"),
    ("DEVSQ", "КВАДРОТКЛ"),
    ("DGET", "БДИЗВЛЕЧ"),
    ("DISC", "СКИДКА"),
    ("DMAX", "БДМАКС"),
    ("DMIN", "БДМИН"),
    ("DOLLAR", "РУБЛЬ"),
    ("DOLLARDE", "РУБЛЬ.ДЕС"),
    ("DOLLARFR", "РУБЛЬ.ДРОБЬ"),
    ("DPRODUCT", "БДПРОИЗВ"),
    ("DROP", "ОТБРОСИТЬ"),
    ("DSTDEV", "БДСТОТКЛ"),
    ("DSTDEVP", "БДГСТАНДОТКЛ"),
    ("DSUM", "БДСУММ"),
    ("DURATION", "ДЛИТ"),
    ("DVAR", "БДДИСП"),
    ("DVARP", "БДДИСПР"),
    ("ECMA.CEILING", "ECMA.ОКРВВЕРХ"),
    ("EDATE", "ДАТАМЕС"),
    ("EFFECT", "ЭФФЕКТ"),
    ("ENCODEURL", "КОДИР.URL"),
    ("EOMONTH", "КОНМЕС"),
    ("ERF", "ФОШ"),
    ("ERF.PRECISE", "ФОШ.ТОЧНО"),
    ("ERFC", "ДОПФОШ"),
    ("ERFC.PRECISE", "ДОПФОШ.ТОЧНО"),
    ("ERROR.TYPE", "ТИП.ОШИБКИ"),
    ("EUROCONVERT", "ЕВРОКОНВ"),
    ("EVEN", "ЧЁТН"),
    ("EXACT", "СОВПАД"),
    ("EXP", "EXP"),
    ("EXPAND", "РАСШИРИТЬ"),
    ("EXPON.DIST", "ЭКСП.РАСП"),
    ("EXPONDIST", "ЭКСПРАСП"),
    ("F.DIST", "Ф.РАСП"),
    ("F.DIST.RT", "Ф.РАСП.ПРАВ"),
    ("F.INV", "Ф.ОБР"),
    ("F.INV.RT", "Ф.ОБР.ПРАВ"),
    ("F.TEST", "Ф.ТЕСТ"),
    ("FACT", "ФАКТР"),
    ("FACTDOUBLE", "ДВФАКТР"),
    ("FALSE", "ЛОЖЬ"),
    ("FDIST", "ФРАСП"),
    ("FILTER", "ФИЛЬТР"),
    ("FILTERXML", "ФИЛЬТР.XML"),
    ("FIND", "НАЙТИ"),
    ("FINDB", "НАЙТИБ"),
    ("FINV", "ФОБР"),
    ("FISHER", "ФИШЕР"),
    ("FISHERINV", "ФИШЕРОБР"),
    ("FIXED", "ФИКСИРОВАННЫЙ"),
    ("FLOOR", "ОКРВНИЗ"),
    ("FLOOR.MATH", "ОКРВНИЗ.МАТ"),
    ("FLOOR.PRECISE", "ОКРВНИЗ.ТОЧНО"),
    ("FORECAST", "ПРЕДСКАЗ"),
    ("FORECAST.ETS", "ПРЕДСКАЗ.ETS"),
    ("FORECAST.ETS.CONFINT", "ПРЕДСКАЗ.ETS.DOVINT"),
    ("FORECAST.ETS.SEASONALITY", "ПРЕДСКАЗ.ETS.SEZONNOST"),
    ("FORECAST.ETS.STAT", "ПРЕДСКАЗ.ETS.STAT"),
    ("FORECAST.LINEAR", "ПРЕДСКАЗ.ЛИН"),
    ("FREQUENCY", "ЧАСТОТА"),
    ("FTEST", "ФТЕСТ"),
    ("FV", "БС"),
    ("FVSCHEDULE", "БСРАСПИС"),
    ("GAMMA", "ГАММА"),
    ("GAMMA.DIST", "ГАММА.РАСП"),
    ("GAMMA.INV", "ГАММА.ОБР"),
    ("GAMMADIST", "ГАММАРАСП"),
    ("GAMMAINV", "ГАММАОБР"),
    ("GAMMALN", "ГАММАНУП"),
    ("GAMMALN.PRECISE", "ГАММАНУП.ТОЧНО"),
    ("GAUSS", "ГАУСС"),
    ("GCD", "НОД"),
    ("GEOMEAN", "СPГЕОМ"),
    ("GESTEP", "ПОРОГ"),
    ("GROUPBY", "ГРУППИРОВАТЬПО"),
    ("GROWTH", "РОСТ"),
    ("HARMEAN", "СPГАРМ"),
    ("HEX2BIN", "ШЕСТ.В.ДВ"),
    ("HEX2DEC", "ШЕСТ.В.ДЕС"),
    ("HEX2OCT", "ШЕСТ.В.ВОСМ"),
    ("HLOOKUP", "ГПР"),
    ("HOUR", "ЧАС"),
    ("HSTACK", "ГСТЕК"),
    ("HYPERLINK", "ГИПЕРССЫЛКА"),
    ("HYPGEOM.DIST", "ГИПЕРГЕОМ.РАСП"),
    ("HYPGEOMDIST", "ГИПЕРГЕОМРАСП"),
    ("IF", "ЕСЛИ"),
    ("IFERROR", "ЕСЛИОШИБКА"),
    ("IFNA", "ЕСЛИНЕД"),
    ("IFS", "ЕСЛИМН"),
    ("IMABS", "МНИМ.ABS"),
    ("IMAGINARY", "МНИМ.ЧАСТЬ"),
    ("IMARGUMENT", "МНИМ.АРГУМЕНТ"),
    ("IMCONJUGATE", "МНИМ.СОПРЯЖ"),
    ("IMCOS", "МНИМ.COS"),
    ("IMCOSH", "МНИМ.COSH"),
    ("IMCOT", "МНИМ.COT"),
    ("IMCSC", "МНИМ.CSC"),
    ("IMCSCH", "МНИМ.CSCH"),
    ("IMDIV", "МНИМ.ДЕП"),
    ("IMEXP", "МНИМ.EXP"),
    ("IMLN", "МНИМ.LN"),
    ("IMLOG10", "МНИМ.LOG10"),
    ("IMLOG2", "МНИМ.LOG2"),
    ("IMPOWER", "МНИМ.СТЕПЕНЬ"),
    ("IMPRODUCT", "МНИМ.ПРОИЗВЕД"),
    ("IMREAL", "МНИМ.ВЕЩ"),
    ("IMSEC", "МНИМ.SEC"),
    ("IMSECH", "МНИМ.SECH"),
    ("IMSIN", "МНИМ.SIN"),
    ("IMSINH", "МНИМ.SINH"),
    ("IMSQRT", "МНИМ.КОРЕНЬ"),
    ("IMSUB", "МНИМ.РАЗН"),
    ("IMSUM", "МНИМ.СУММ"),
    ("IMTAN", "МНИМ.TAN"),
    ("INDEX", "ИНДЕКС"),
    ("INDIRECT", "ДВССЫЛ"),
    ("INFO", "СВЕДЕНИЯ"),
    ("INT", "ЦЕЛОЕ"),
    ("INTERCEPT", "ОТРЕЗОК"),
    ("INTRATE", "ИПОТЕКА"),
    ("IPMT", "ПРПЛТ"),
    ("IRR", "ВНДОХ"),
    ("ISBLANK", "ЕПУСТО"),
    ("ISERR", "ЕОШ"),
    ("ISERROR", "ЕОШИБКА"),
    ("ISEVEN", "ЕЧЁТН"),
    ("ISLOGICAL", "ЕЛОГИЧ"),
    ("ISNA", "ЕНЕД"),
    ("ISNONTEXT", "ЕНЕТЕКСТ"),
    ("ISNUMBER", "ЕЧИСЛО"),
    ("ISO.CEILING", "ISO.ОКРВВЕРХ"),
    ("ISODD", "ЕНЕЧЁТН"),
    ("ISOWEEKNUM", "ИЗОНОМНЕД"),
    ("ISPMT", "ПРОЦПЛТ"),
    ("ISTEXT", "ЕТЕКСТ"),
    ("JIS", "JIS"),
    ("KURT", "ЭКССЕСС"),
    ("LAMBDA", "ЛЯМБДА"),
    ("LARGE", "НАИБОЛЬШИЙ"),
    ("LCM", "НОК"),
    ("LEFT", "ЛЕВСИМВ"),
    ("LEFTB", "ЛЕВБ"),
    ("LEN", "ДЛСТР"),
    ("LENB", "ДЛИНБ"),
    ("LET", "ПУСТЬ"),
    ("LINEST", "ЛИНЕЙН"),
    ("LN", "LN"),
    ("LOG", "LOG"),
    ("LOG10", "LOG10"),
    ("LOGEST", "ЛГРФПРИБ"),
    ("LOGINV", "ЛОГНОРМОБР"),
    ("LOGNORM.DIST", "ЛОГНОРМ.РАСП"),
    ("LOGNORM.INV", "ЛОГНОРМ.ОБР"),
    ("LOGNORMDIST", "ЛОГНОРМРАСП"),
    ("LOWER", "СТРОЧН"),
    ("MAKEARRAY", "СОЗДАТЬМАССИВ"),
    ("MAP", "КАРТА"),
    ("MAX", "МАКС"),
    ("MAXA", "МАКСА"),
    ("MAXIFS", "МАКСЕСЛИ"),
    ("MDETERM", "МОПРЕД"),
    ("MDURATION", "МДЛИТ"),
    ("MEDIAN", "МЕДИАНА"),
    ("MID", "ПСТР"),
    ("MIDB", "ПСТРБ"),
    ("MIN", "МИН"),
    ("MINA", "МИНА"),
    ("MINIFS", "МИНЕСЛИ"),
    ("MINUTE", "МИНУТЫ"),
    ("MINVERSE", "МОБРАТ"),
    ("MIRR", "МВНДОХ"),
    ("MMULT", "МУМНОЖ"),
    ("MOD", "ОСТАТ"),
    ("MODE", "МОДА"),
    ("MODE.MULT", "МОДА.НСК"),
    ("MODE.SNGL", "МОДА.ОДН"),
    ("MONTH", "МЕСЯЦ"),
    ("MROUND", "ОКРУГЛТ"),
    ("MULTINOMIAL", "МУЛЬТИНОМ"),
    ("MUNIT", "ЕДМАTP"),
    ("N", "Ч"),
    ("NA", "НД"),
    ("NEGBINOM.DIST", "ОТРБИНОМ.РАСП"),
    ("NEGBINOMDIST", "ОТРБИНОМРАСП"),
    ("NETWORKDAYS", "ЧИСТРАБДНИ"),
    ("NETWORKDAYS.INTL", "ЧИСТРАБДНИ.МЕЖД"),
    ("NOMINAL", "НОМИНАЛ"),
    ("NORM.DIST", "НОРМ.РАСП"),
    ("NORM.INV", "НОРМ.ОБР"),
    ("NORM.S.DIST", "НОРМ.СТ.РАСП"),
    ("NORM.S.INV", "НОРМ.СТ.ОБР"),
    ("NORMDIST", "НОРМРАСП"),
    ("NORMINV", "НОРМОБР"),
    ("NORMSDIST", "НОРМСТРАСП"),
    ("NORMSINV", "НОРМСТОБР"),
    ("NOT", "НЕ"),
    ("NOW", "ТЕДАТА"),
    ("NPER", "КПЕР"),
    ("NPV", "ЧПС"),
    ("NUMBERVALUE", "ЧИСЛЗНАЧ"),
    ("OCT2BIN", "ВОСМ.В.ДВ"),
    ("OCT2DEC", "ВОСМ.В.ДЕС"),
    ("OCT2HEX", "ВОСМ.В.ШЕСТ"),
    ("ODD", "НЕЧЁТ"),
    ("ODDFPRICE", "ЦЕНАМАЛПЕРВ"),
    ("ODDFYIELD", "ДОХОДМАЛПЕРВ"),
    ("ODDLPRICE", "ЦЕНАМАЛПОСЛ"),
    ("ODDLYIELD", "ДОХОДМАЛПОСЛ"),
    ("OFFSET", "СМЕЩ"),
    ("OR", "ИЛИ"),
    ("PDURATION", "ПСРОК"),
    ("PEARSON", "ПИРСОН"),
    ("PERCENTILE", "ПРОЦЕНТИЛЬ"),
    ("PERCENTILE.EXC", "ПРОЦЕНТИЛЬ.ИСКЛ"),
    ("PERCENTILE.INC", "ПРОЦЕНТИЛЬ.ВКЛ"),
    ("PERCENTOF", "ПРОЦЕНТОТ"),
    ("PERCENTRANK", "ПРОЦЕНТРАНГ"),
    ("PERCENTRANK.EXC", "РАНГ.ПРОЦ.ИСКЛ"),
    ("PERCENTRANK.INC", "РАНГ.ПРОЦ.ВКЛ"),
    ("PERMUT", "ПЕРЕСТ"),
    ("PERMUTATIONA", "ПЕРЕСТА"),
    ("PHI", "ФИ"),
    ("PHONETIC", "ФОНЕТИЧЕСКИЙ"),
    ("PI", "ПИ"),
    ("PIVOTBY", "СВОДПО"),
    ("PMT", "ПЛТ"),
    ("POISSON", "ПУАССОН"),
    ("POISSON.DIST", "ПУАССОН.РАСП"),
    ("POWER", "СТЕПЕНЬ"),
    ("PPMT", "ОСПЛТ"),
    ("PRICE", "ЦЕНА"),
    ("PRICEDISC", "ЦЕНАСКИДКА"),
    ("PRICEMAT", "ЦЕНАПОГАШ"),
    ("PROB", "ВЕРОЯТНОСТЬ"),
    ("PRODUCT", "ПРОИЗВЕД"),
    ("PROPER", "ПРОПНАЧ"),
    ("PV", "ПС"),
    ("QUARTILE", "КВАРТИЛЬ"),
    ("QUARTILE.EXC", "КВАРТИЛЬ.ИСКЛ"),
    ("QUARTILE.INC", "КВАРТИЛЬ.ВКЛ"),
    ("QUOTIENT", "ЧАСТНОЕ"),
    ("RADIANS", "РАДИАНЫ"),
    ("RAND", "СЛЧИС"),
    ("RANDARRAY", "СЛУЧМАC"),
    ("RANDBETWEEN", "СЛУЧМЕЖДУ"),
    ("RANK", "РАНГ"),
    ("RANK.AVG", "РАНГ.СР"),
    ("RANK.EQ", "РАНГ.РВ"),
    ("RATE", "СТАВКА"),
    ("RECEIVED", "ПОЛУЧЕНО"),
    ("REDUCE", "СВЕРТКА"),
    ("REGEXEXTRACT", "REGEXEXTRACT"),
    ("REGEXREPLACE", "REGEXREPLACE"),
    ("REGEXTEST", "REGEXTEST"),
    ("REPLACE", "ЗАМЕНИТЬ"),
    ("REPLACEB", "ЗАМЕНИТЬБ"),
    ("REPT", "ПОВТОР"),
    ("RIGHT", "ПРАВСИМВ"),
    ("RIGHTB", "ПРАВБ"),
    ("ROMAN", "РИМСКОЕ"),
    ("ROUND", "ОКРУГЛ"),
    ("ROUNDDOWN", "ОКРУГЛВНИЗ"),
    ("ROUNDUP", "ОКРУГЛВВЕРХ"),
    ("ROWS", "ЧСТРОК"),
    ("RRI", "ЭКВ.СТАВКА"),
    ("RSQ", "КВАДРАТ_ПИРСОН"),
    ("SCAN", "СКАНИРОВАТЬ"),
    ("SEARCH", "ПОИСК"),
    ("SEARCHB", "ПОИСКБ"),
    ("SEC", "SEC"),
    ("SECH", "SECH"),
    ("SECOND", "СЕКУНДЫ"),
    ("SEQUENCE", "ПОСЛЕДОВ"),
    ("SIGN", "ЗНАК"),
    ("SIN", "SIN"),
    ("SINH", "SINH"),
    ("SKEW", "СКОС"),
    ("SKEW.P", "СКОС.Г"),
    ("SLN", "АПЛ"),
    ("SLOPE", "НАКЛОН"),
    ("SMALL", "НАИМЕНЬШИЙ"),
    ("SORT", "СОРТ"),
    ("SORTBY", "СОРТПО"),
    ("SQRT", "КОРЕНЬ"),
    ("SQRTPI", "КОРЕНЬПИ"),
    ("STANDARDIZE", "НОРМАЛИЗАЦИЯ"),
    ("STDEV", "СТАНДОТКЛОН"),
    ("STDEV.P", "СТАНДОТКЛОН.Г"),
    ("STDEV.S", "СТАНДОТКЛОН.В"),
    ("STDEVA", "СТАНДОТКЛОНА"),
    ("STDEVP", "СТАНДОТКЛОНП"),
    ("STDEVPA", "СТАНДОТКЛОНПА"),
    ("STEYX", "СТОШYX"),
    ("SUBSTITUTE", "ПОДСТАВИТЬ"),
    ("SUBTOTAL", "ПРОМЕЖУТОЧНЫЕ.ИТОГИ"),
    ("SUM", "СУММ"),
    ("SUMIF", "СУММЕСЛИ"),
    ("SUMIFS", "СУММЕСЛИМН"),
    ("SUMPRODUCT", "СУММПРОИЗВ"),
    ("SUMSQ", "СУММКВ"),
    ("SUMX2MY2", "СУММРАЗНКВ"),
    ("SUMX2PY2", "СУММСУММКВ"),
    ("SUMXMY2", "СУММКВРАЗН"),
    ("SWITCH", "ПЕРЕКЛ"),
    ("SYD", "АСЮ"),
    ("T", "Т"),
    ("T.DIST", "СЧУДЕНТ.РАСП"),
    ("T.DIST.2T", "СЧУДЕНТ.РАСП.2Т"),
    ("T.DIST.RT", "СЧУДЕНТ.РАСП.ПРАВ"),
    ("T.INV", "СЧУДЕНТ.ОБР"),
    ("T.INV.2T", "СЧУДЕНТ.ОБР.2Т"),
    ("T.TEST", "СЧУДЕНТ.ТЕСТ"),
    ("TAKE", "ВЗЯТЬ"),
    ("TAN", "TAN"),
    ("TANH", "TANH"),
    ("TBILLEQ", "РАВНОК.ТРЕК"),
    ("TBILLPRICE", "ЦЕНАТРЕК"),
    ("TBILLYIELD", "ДОХОД.ТРЕК"),
    ("TDIST", "ТРЕСП"),
    ("TEXT", "ТЕКСТ"),
    ("TEXTAFTER", "ТЕКСТПОСЛЕ"),
    ("TEXTBEFORE", "ТЕКСТДО"),
    ("TEXTJOIN", "ОБЪЕДИНИТЬ"),
    ("TEXTSPLIT", "РАЗДЕЛИТЬТЕКСТ"),
    ("TIME", "ВРЕМЯ"),
    ("TIMEVALUE", "ВРЕМЯЗНАЧ"),
    ("TINV", "ТОБР"),
    ("TOCOL", "ВСТОЛБЕЦ"),
    ("TODAY", "СЕГОДНЯ"),
    ("TOROW", "ВСТРОКУ"),
    ("TRANSPOSE", "ТРАНСП"),
    ("TREND", "ТЕНДЕНЦИЯ"),
    ("TRIM", "СЖПРОБЕЛЫ"),
    ("TRIMMEAN", "СРЗНАЧУСЕЧ"),
    ("TRUE", "ИСТИНА"),
    ("TRUNC", "ОТБРОС"),
    ("TTEST", "ТТЕСТ"),
    ("TYPE", "ТИП"),
    ("UNICHAR", "ЮНИКОД"),
    ("UNICODE", "СИМВЮНИКОД"),
    ("UNIQUE", "УНИК"),
    ("UPPER", "ПРОПИСН"),
    ("USDOLLAR", "USDOLLAR"),
    ("VALUE", "ЗНАЧЕН"),
    ("VALUETOTEXT", "ЗНАЧВТЕКСТ"),
    ("VAR", "ДИСП"),
    ("VAR.P", "ДИСП.Г"),
    ("VAR.S", "ДИСП.В"),
    ("VARA", "ДИСПА"),
    ("VARP", "ДИСПР"),
    ("VARPA", "ДИСПРА"),
    ("VDB", "ПУО"),
    ("VLOOKUP", "ВПР"),
    ("VSTACK", "ВСТЕК"),
    ("WEBSERVICE", "ВЕБСЛУЖБА"),
    ("WEEKDAY", "ДЕНЬНЕД"),
    ("WEEKNUM", "НОМНЕД"),
    ("WEIBULL", "ВЕЙБУЛЛ"),
    ("WEIBULL.DIST", "ВЕЙБУЛЛ.РАСП"),
    ("WORKDAY", "РАБДЕНЬ"),
    ("WORKDAY.INTL", "РАБДЕНЬ.МЕЖД"),
    ("WRAPCOLS", "СВЕРНУТЬСТОЛБЦЫ"),
    ("WRAPROWS", "СВЕРНУТЬСТРОКИ"),
    ("XIRR", "ЧИСТВНДОХ"),
    ("XLOOKUP", "ПРОСМОТРX"),
    ("XMATCH", "ПОИСКПОЗX"),
    ("XNPV", "ЧИСТСВ"),
    ("XOR", "ИСКЛИЛИ"),
    ("YEAR", "ГОД"),
    ("YEARFRAC", "ДОЛЯГОДА"),
    ("YIELD", "ДОХОД"),
    ("YIELDDISC", "ДОХОДСКИДКА"),
    ("YIELDMAT", "ДОХОДПОГАШ"),
    ("Z.TEST", "Z.ТЕСТ"),
    ("ZTEST", "ZТЕСТ"),
];

impl FormulaLocale {
    pub fn list_separator(self) -> char {
        match self {
            Self::Es | Self::Ru => ';',
            Self::En => ',',
        }
    }
    pub fn decimal_separator(self) -> char {
        match self {
            Self::Es | Self::Ru => ',',
            Self::En => '.',
        }
    }
    pub fn array_column_separator(self) -> char {
        match self {
            Self::Es | Self::Ru => '\\',
            Self::En => ',',
        }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        match self {
            Self::Es => SPANISH.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, es)| *es).unwrap_or(canonical),
            Self::Ru => RUSSIAN.iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, ru)| *ru).unwrap_or(canonical),
            Self::En => canonical,
        }
    }
    pub fn canonical_name(self, localized: &str) -> &str {
        match self {
            Self::Es => SPANISH.iter().find(|(_, es)| es.eq_ignore_ascii_case(localized)).map(|(en, _)| *en).unwrap_or(localized),
            Self::Ru => RUSSIAN.iter().find(|(_, ru)| ru.eq_ignore_ascii_case(localized)).map(|(en, _)| *en).unwrap_or(localized),
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

pub fn boolean_alias(name: &str, locale: FormulaLocale) -> Option<bool> {
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
        FormulaLocale::Ru => {
            if name.eq_ignore_ascii_case("ИСТИНА") {
                Some(true)
            } else if name.eq_ignore_ascii_case("ЛОЖЬ") {
                Some(false)
            } else {
                None
            }
        }
        FormulaLocale::En => None,
    }
}

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

fn normalize(expr: &mut Expr, locale: FormulaLocale, known_name: &impl Fn(&str) -> bool, bindings: &mut Bindings, depth: usize, display: bool) -> Result<(), ParseError> {
    if depth > parser::MAX_DEPTH {
        return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
    }
    let bound = |name: &str, bindings: &Bindings| bindings.contains(name) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let alias = match locale {
                FormulaLocale::Es => if *value { "VERDADERO" } else { "FALSO" },
                FormulaLocale::Ru => if *value { "ИСТИНА" } else { "ЛОЖЬ" },
                FormulaLocale::En => if *value { "TRUE" } else { "FALSE" },
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
        _ => {}
    }
    Ok(())
}
