//! Rendering and format-code spelling in locales other than en-US.

use gridcraft_core::{CellError, DateSystem, Value};
use gridcraft_locale::{Dialect, FormatWords, LANGUAGES, Language, Locale, REGIONS, language, region};

use super::*;

const S: DateSystem = DateSystem::D1900;
/// 2026-10-07, a Wednesday.
const OCT_7: f64 = 46302.0;
/// 2026-10-10, a Saturday.
const OCT_10: f64 = 46305.0;

const TABLE_ITEMS: [&str; 5] = ["#All", "#Data", "#Headers", "#Totals", "#This Row"];

const fn fixture(
    tag: &'static str,
    bool_true: &'static str,
    bool_false: &'static str,
    errors: &'static [(&'static str, &'static str)],
    format: FormatWords,
) -> Language {
    Language {
        tag,
        native_name: tag,
        dbcs: false,
        bool_true,
        bool_false,
        r1c1: ['R', 'C'],
        table_items: TABLE_ITEMS,
        errors,
        format,
        functions: &[],
        by_local: &[],
        gaps: &[],
        cell_types: &[],
        info_types: &[],
        content: &[],
    }
}

const fn words(year: char, month: char, day: char, minute: char, general: &'static str) -> FormatWords {
    FormatWords { year, month, day, hour: 'h', minute, second: 's', general }
}

// Languages as documented for Excel, so these tests do not depend on the generated tables. The
// region tables (separators, names) come from the real data.
static PT_BR: Language = fixture(
    "pt-BR",
    "VERDADEIRO",
    "FALSO",
    &[("#NAME?", "#NOME?"), ("#VALUE!", "#VALOR!"), ("#N/A", "#N/D"), ("#NUM!", "#NÚM!")],
    words('a', 'm', 'd', 'm', "Geral"),
);
static DE_DE: Language = fixture("de-DE", "WAHR", "FALSCH", &[("#NAME?", "#NAME?"), ("#VALUE!", "#WERT!")], words('J', 'M', 'T', 'm', "Standard"));
static FR_FR: Language = fixture("fr-FR", "VRAI", "FAUX", &[("#N/A", "#N/A"), ("#VALUE!", "#VALEUR!")], words('a', 'm', 'j', 'm', "Standard"));
static JA_JP: Language = fixture("ja-JP", "TRUE", "FALSE", &[], words('y', 'm', 'd', 'm', "G/標準"));
static FIXTURES: [&Language; 4] = [&PT_BR, &DE_DE, &FR_FR, &JA_JP];

/// Every shipped language plus the fixtures above.
fn all_languages() -> Vec<&'static Language> {
    LANGUAGES.iter().copied().chain(FIXTURES).collect()
}

fn loc(lang: &str, reg: &str) -> Locale {
    let l = FIXTURES.iter().copied().find(|l| l.tag == lang).or_else(|| language(lang)).unwrap_or_else(|| panic!("language {lang} missing"));
    let r = region(reg).unwrap_or_else(|| panic!("region {reg} missing"));
    Locale::new(l, l, *r)
}

fn fl(l: &Locale, code: &str, n: f64) -> String {
    format_value_in(&Value::Number(n), &NumberFormat::parse(code), S, l).text
}

fn fv(l: &Locale, code: &str, v: Value) -> String {
    format_value_in(&v, &NumberFormat::parse(code), S, l).text
}

fn full_in(l: &Locale, code: &str, n: f64) -> Formatted {
    format_value_in(&Value::Number(n), &NumberFormat::parse(code), S, l)
}

fn dialect(l: &Locale) -> Dialect<'_> {
    l.dialect()
}

#[test]
fn invariant_locale_is_unchanged() {
    let inv = &INVARIANT;
    assert_eq!(fl(inv, "#,##0.00", 1234.5), "1,234.50");
    assert_eq!(fl(inv, "m/d/yyyy", OCT_7), "10/7/2026");
    assert_eq!(fl(inv, "m/d/yyyy h:mm", OCT_7 + 0.75), "10/7/2026 18:00");
    assert_eq!(fv(inv, "General", Value::Bool(true)), "TRUE");
    assert_eq!(fv(inv, "General", Value::Error(CellError::NA)), "#N/A");
}

#[test]
fn pt_br_numbers() {
    let l = loc("pt-BR", "pt-BR");
    assert_eq!(fl(&l, "#,##0.00", 1234.5), "1.234,50");
    assert_eq!(fl(&l, "#,##0.00", -1234567.891), "-1.234.567,89");
    assert_eq!(fl(&l, "General", 1.5), "1,5");
    assert_eq!(fl(&l, "General", 1234.5), "1234,5");
    assert_eq!(fl(&l, "0.00%", 0.1234), "12,34%");
    assert_eq!(fl(&l, "0.00E+00", 12345.0), "1,23E+04");
    assert_eq!(fl(&l, "# ?/?", 1.5), "1 1/2");
    assert_eq!(fl(&l, "#,##0,", 1234567.0), "1.235");
    assert_eq!(fl(&l, "General\" kg\"", 2.25), "2,25 kg");
    assert_eq!(fl(&l, "0.0", 0.0), "0,0");
    assert_eq!(fl(&l, "@", 1.5), "1,5");
    assert_eq!(fl(&l, "\"R$\" #,##0.00_);[Red](\"R$\" #,##0.00)", 1234.5), "R$ 1.234,50 ");
}

#[test]
fn pt_br_dates() {
    let l = loc("pt-BR", "pt-BR");
    assert_eq!(fl(&l, "m/d/yyyy", OCT_10), "10/10/2026");
    assert_eq!(fl(&l, "m/d/yyyy", OCT_7), "07/10/2026");
    assert_eq!(fl(&l, "m/d/yyyy h:mm", OCT_7 + 0.75), "07/10/2026 18:00");
    assert_eq!(fl(&l, "M/D/YYYY", OCT_7), "07/10/2026");
    // A custom code that merely looks different keeps its own order.
    assert_eq!(fl(&l, "m/d/yy", OCT_7), "10/7/26");
    assert_eq!(fl(&l, "mmmm", OCT_10), "outubro");
    assert_eq!(fl(&l, "mmm", OCT_10), "out");
    assert_eq!(fl(&l, "mmmmm", OCT_10), "o");
    assert_eq!(fl(&l, "dddd", OCT_10), "sábado");
    assert_eq!(fl(&l, "ddd", OCT_10), "sáb");
    assert_eq!(fl(&l, "mm:ss.00", 61.25 / 86400.0), "01:01,25");
    assert_eq!(fl(&l, "[$-F800]dddd, mmmm dd, yyyy", OCT_10), "sábado, 10 de outubro de 2026");
    assert_eq!(fl(&l, "[$-F400]h:mm:ss AM/PM", 0.75), "18:00:00");
}

#[test]
fn pt_br_booleans_and_errors() {
    let l = loc("pt-BR", "pt-BR");
    assert_eq!(fv(&l, "General", Value::Bool(true)), "VERDADEIRO");
    assert_eq!(fv(&l, "General", Value::Bool(false)), "FALSO");
    assert_eq!(fv(&l, "General", Value::Error(CellError::NA)), "#N/D");
    assert_eq!(fv(&l, "General", Value::Error(CellError::Name)), "#NOME?");
    assert_eq!(fv(&l, "0.00", Value::Error(CellError::Value)), "#VALOR!");
    assert_eq!(fv(&l, "General", Value::Number(f64::INFINITY)), "#NÚM!");
}

#[test]
fn group_separators_follow_the_region() {
    let fr = loc("fr-FR", "fr-FR");
    assert_eq!(fl(&fr, "#,##0.00", 1234567.891), "1\u{a0}234\u{a0}567,89");
    let pt = loc("en-US", "pt-PT");
    assert_eq!(fl(&pt, "#,##0", 1234567.0), "1\u{a0}234\u{a0}567");
    let de = loc("de-DE", "de-DE");
    assert_eq!(fl(&de, "#,##0.00", 1234567.891), "1.234.567,89");
    assert_eq!(fl(&de, "m/d/yyyy", OCT_10), "10.10.2026");
    assert_eq!(fl(&de, "mmmm", OCT_10), "Oktober");
    assert_eq!(fl(&de, "dddd", OCT_10), "Samstag");
    // A region with custom separators (Excel's "use system separators" off).
    let custom = Locale::new(language("en-US").unwrap(), language("en-US").unwrap(), region("en-US").unwrap().with_separators(',', '.'));
    assert_eq!(fl(&custom, "#,##0.00", 1234.5), "1.234,50");
    assert_eq!(fl(&custom, "General", 0.25), "0,25");
}

#[test]
fn ja_jp_designators() {
    let l = loc("ja-JP", "ja-JP");
    assert_eq!(fl(&l, "h:mm AM/PM", 15.0 / 24.0 + 5.0 / 1440.0), "3:05 午後");
    assert_eq!(fl(&l, "AM/PM h:mm", 9.0 / 24.0 + 5.0 / 1440.0), "午前 9:05");
    assert_eq!(fl(&l, "m/d/yyyy", OCT_7), "2026/10/7");
    assert_eq!(fl(&l, "dddd", OCT_10), "土曜日");
    assert_eq!(fl(&l, "h a/p", 0.75), "6 p");
}

#[test]
fn lcid_tag_selects_names() {
    // German names even in the invariant locale.
    assert_eq!(fl(&INVARIANT, "[$-407]mmmm", OCT_10), "Oktober");
    assert_eq!(fl(&INVARIANT, "[$-407]dddd, d. mmmm yyyy", OCT_10), "Samstag, 10. Oktober 2026");
    assert_eq!(fl(&INVARIANT, "[$-0407]ddd", OCT_10), "Sa");
    assert_eq!(fl(&INVARIANT, "[$-10407]mmm", OCT_10), "Okt");
    // A currency symbol before the id.
    assert_eq!(fl(&INVARIANT, "[$€-407]#,##0.00", 1234.5), "€1,234.50");
    // And English names in a Portuguese locale.
    let pt = loc("pt-BR", "pt-BR");
    assert_eq!(fl(&pt, "[$-409]mmmm", OCT_10), "October");
    // Unknown ids keep the locale's names.
    assert_eq!(fl(&pt, "[$-FFFF]mmmm", OCT_10), "outubro");
    // The tag does not change separators.
    assert_eq!(fl(&pt, "[$-409]#,##0.00", 1234.5), "1.234,50");
}

#[test]
fn text_function_in_locale() {
    let pt = loc("pt-BR", "pt-BR");
    assert_eq!(text_function_in(&Value::Number(OCT_10), "dd/mm/aaaa", S, &pt), Ok("10/10/2026".into()));
    assert_eq!(text_function_in(&Value::Number(OCT_10), "dddd, d \"de\" mmmm \"de\" aaaa", S, &pt), Ok("sábado, 10 de outubro de 2026".into()));
    assert_eq!(text_function_in(&Value::Number(1234.5), "#.##0,00", S, &pt), Ok("1.234,50".into()));
    assert_eq!(text_function_in(&Value::Number(0.5), "Geral", S, &pt), Ok("0,5".into()));
    assert_eq!(text_function_in(&Value::from("1,5"), "0,00", S, &pt), Ok("1,50".into()));
    assert_eq!(text_function_in(&Value::Bool(true), "0", S, &pt), Ok("VERDADEIRO".into()));
    assert_eq!(text_function_in(&Value::Number(-1.0), "dd/mm/aaaa", S, &pt), Err(CellError::Value));
    assert_eq!(text_function_in(&Value::Number(0.75), "hh:mm AM/PM", S, &pt), Ok("06:00 PM".into()));
    let de = loc("de-DE", "de-DE");
    assert_eq!(text_function_in(&Value::Number(OCT_10), "TT.MM.JJJJ", S, &de), Ok("10.10.2026".into()));
    assert_eq!(text_function_in(&Value::Number(1234.5), "#.##0,00", S, &de), Ok("1.234,50".into()));
    assert_eq!(text_function_in(&Value::Number(0.25), "Standard", S, &de), Ok("0,25".into()));
    // The invariant function is the invariant locale.
    assert_eq!(text_function(&Value::Number(OCT_10), "dd/mm/yyyy", S), text_function_in(&Value::Number(OCT_10), "dd/mm/yyyy", S, &INVARIANT));
}

#[test]
fn local_code_spelling() {
    let pt = loc("pt-BR", "pt-BR");
    assert_eq!(to_local_code("#,##0.00", &dialect(&pt)), "#.##0,00");
    assert_eq!(to_local_code("dd/mm/yyyy", &dialect(&pt)), "dd/mm/aaaa");
    assert_eq!(to_local_code("General", &dialect(&pt)), "Geral");
    assert_eq!(to_local_code("h:mm AM/PM", &dialect(&pt)), "h:mm AM/PM");
    assert_eq!(to_local_code("YYYY", &dialect(&pt)), "AAAA");
    assert_eq!(from_local_code("dd/mm/aaaa", &dialect(&pt)), "dd/mm/yyyy");
    // The keyword keeps the case it was typed in.
    assert_eq!(from_local_code("geral", &dialect(&pt)), "general");
    assert_eq!(from_local_code("Geral", &dialect(&pt)), "General");
    assert_eq!(from_local_code("#.##0,00", &dialect(&pt)), "#,##0.00");
    // Literals, escapes, colours and the system tags are not touched.
    assert_eq!(to_local_code("\"a.b,c\"0.0\\.[Red]", &dialect(&pt)), "\"a.b,c\"0,0\\.[Red]");
    assert_eq!(to_local_code("[$-416]yyyy\"y\"", &dialect(&pt)), "[$-416]aaaa\"y\"");
    assert_eq!(to_local_code("[>1.5]0.0;[<-1.5]-0.0;0", &dialect(&pt)), "[>1,5]0,0;[<-1,5]-0,0;0");
    assert_eq!(to_local_code("_(* #,##0.00_)", &dialect(&pt)), "_(* #.##0,00_)");

    let de = loc("de-DE", "de-DE");
    assert_eq!(to_local_code("dd.mm.yyyy", &dialect(&de)), "TT.MM.JJJJ");
    assert_eq!(from_local_code("TT.MM.JJJJ", &dialect(&de)), "dd.mm.yyyy");
    assert_eq!(to_local_code("General", &dialect(&de)), "Standard");
    assert_eq!(from_local_code("Standard", &dialect(&de)), "General");
    assert_eq!(to_local_code("dd.mm.yyyy hh:mm:ss.00", &dialect(&de)), "TT.MM.JJJJ hh:mm:ss,00");
    assert_eq!(from_local_code("TT.MM.JJJJ hh:mm:ss,00", &dialect(&de)), "dd.mm.yyyy hh:mm:ss.00");
    assert_eq!(to_local_code("[h]:mm:ss", &dialect(&de)), "[h]:mm:ss");
    assert_eq!(to_local_code("#,##0.00", &dialect(&de)), "#.##0,00");
    assert_eq!(from_local_code("#.##0,00", &dialect(&de)), "#,##0.00");
    // A date section keeps its literal point and comma.
    assert_eq!(to_local_code("dddd, d. mmmm yyyy", &dialect(&de)), "TTTT, T. MMMM JJJJ");
    assert_eq!(to_local_code("h:mm AM/PM", &dialect(&de)), "h:mm AM/PM");
    // Case is kept relative to the language's own spelling.
    assert_eq!(to_local_code("DD.MM.YYYY", &dialect(&de)), "tt.mm.jjjj");

    let fr = loc("fr-FR", "fr-FR");
    assert_eq!(to_local_code("#,##0.00", &dialect(&fr)), "#\u{a0}##0,00");
    assert_eq!(from_local_code("#\u{a0}##0,00", &dialect(&fr)), "#,##0.00");
    // A plain space between placeholders is accepted as the group separator.
    assert_eq!(from_local_code("# ##0,00", &dialect(&fr)), "#,##0.00");
    // But not elsewhere: this is a literal space (and `# ?/?` is a fraction).
    assert_eq!(from_local_code("# ?/?", &dialect(&fr)), "# ?/?");
    assert_eq!(from_local_code("0 \"kg\"", &dialect(&fr)), "0 \"kg\"");
    // The space of a fraction is never a group separator, whatever the placeholders.
    for code in ["# #/#", "# 0/0", "0 0/0", "# ?/?", "# ##/##", "# #/8"] {
        assert_eq!(from_local_code(code, &dialect(&fr)), code, "{code}");
        assert_eq!(to_local_code(code, &dialect(&fr)), code, "{code}");
    }
    assert_eq!(text_function_in(&Value::Number(1.5), "# #/#", S, &fr), Ok("1 1/2".into()));
    assert_eq!(text_function_in(&Value::Number(1.5), "0 0/0", S, &fr), Ok("1 1/2".into()));
    assert_eq!(text_function_in(&Value::Number(1234.5), "# ##0,00", S, &fr), Ok("1\u{a0}234,50".into()));
}

#[test]
fn general_keeps_its_case() {
    for lang in ["pt-BR", "de-DE", "ja-JP"] {
        let l = loc(lang, lang);
        let d = dialect(&l);
        for word in ["General", "general", "GENERAL", "GeNeRaL"] {
            let code = format!("{word}\" kg\"");
            let local = to_local_code(&code, &d);
            assert_eq!(from_local_code(&local, &d), code, "{lang}: {code} -> {local}");
            assert_eq!(text_function_in(&Value::Number(1.5), &local, S, &l), Ok(format!("{} kg", fl(&l, "General", 1.5))), "{lang}: {local}");
        }
    }
    let pt = loc("pt-BR", "pt-BR");
    assert_eq!(to_local_code("general", &dialect(&pt)), "geral");
    assert_eq!(to_local_code("GENERAL", &dialect(&pt)), "GERAL");
    assert_eq!(from_local_code("GERAL", &dialect(&pt)), "GENERAL");
    // The canonical word is accepted as typed.
    assert_eq!(from_local_code("General", &dialect(&pt)), "General");
}

#[test]
fn literals_that_look_like_syntax_are_escaped() {
    let pt = loc("pt-BR", "pt-BR");
    let d = dialect(&pt);
    // `a` is a year letter in Portuguese but plain text in a canonical code, and `y` the reverse.
    assert_eq!(to_local_code("0.0 a", &d), "0,0 \\a");
    assert_eq!(from_local_code("0,0 \\a", &d), "0.0 a");
    assert_eq!(from_local_code("0,0 y", &d), "0.0 \\y");
    assert_eq!(to_local_code("0.0 \\y", &d), "0,0 y");
    // A redundant escape of a letter that is plain text in the source is kept as such.
    assert_eq!(to_local_code("0.0 \\a", &d), "0,0 \\a");
    assert_eq!(text_function_in(&Value::Number(1.5), "0,0 \\a", S, &pt), Ok("1,5 a".into()));
    assert_eq!(text_function_in(&Value::Number(1.5), "0,0 y", S, &pt), Ok("1,5 y".into()));
    // The no-break space is the group separator in French: a literal one is escaped.
    let fr = loc("fr-FR", "fr-FR");
    assert_eq!(to_local_code("0.0\u{a0}x", &dialect(&fr)), "0,0\\\u{a0}x");
    assert_eq!(from_local_code("0,0\\\u{a0}x", &dialect(&fr)), "0.0\u{a0}x");
}

#[test]
fn code_length_is_capped() {
    let pt = loc("pt-BR", "pt-BR");
    let long = "[".repeat(256);
    assert_eq!(to_local_code(&long, &dialect(&pt)), long);
    assert_eq!(from_local_code(&long, &dialect(&pt)), long);
    let ok = format!("0,00{}", "\" \"".repeat(80));
    assert!(ok.chars().count() <= 255);
    assert_ne!(to_local_code(&ok, &dialect(&pt)), ok);
    let too_long = format!("0{}", "\"x\"".repeat(100));
    assert!(too_long.chars().count() > 255);
    assert_eq!(text_function_in(&Value::Number(1.0), &too_long, S, &pt), Err(CellError::Value));
    assert_eq!(text_function(&Value::Number(1.0), &too_long, S), Err(CellError::Value));
    // Many unmatched brackets stay linear and do not panic.
    let brackets = "[".repeat(255);
    assert!(text_function_in(&Value::Number(1.0), &brackets, S, &pt).is_ok());
}

#[test]
fn multibyte_decimal_separators_truncate_sub_seconds() {
    for decimal in ['٫', '．', '·', '.'] {
        let reg = region("en-US").unwrap().with_separators(decimal, ',');
        assert_eq!(reg.decimal, decimal);
        let l = Locale::new(language("en-US").unwrap(), language("en-US").unwrap(), reg);
        let serial = 61.2 / 86400.0;
        assert_eq!(fl(&l, "mm:ss.0", serial), format!("01:01{decimal}2"));
        assert_eq!(fl(&l, "mm:ss.00", serial), format!("01:01{decimal}20"));
        assert_eq!(fl(&l, "mm:ss.000", serial), format!("01:01{decimal}200"));
        assert_eq!(fl(&l, "mm:ss.0000", serial), format!("01:01{decimal}2000"));
    }
}

#[test]
fn system_formats_keep_colour_and_follow_the_section() {
    let pt = loc("pt-BR", "pt-BR");
    let red = full_in(&pt, "[Red][$-F800]dddd, mmmm dd, yyyy", OCT_10);
    assert_eq!(red.text, "sábado, 10 de outubro de 2026");
    assert_eq!(red.color, Some(FormatColor::Red));
    // Each section decides for itself.
    assert_eq!(fl(&pt, "[$-F800]dddd, mmmm dd, yyyy;[$-F400]h:mm:ss AM/PM", OCT_10), "sábado, 10 de outubro de 2026");
    assert_eq!(fl(&pt, "[>50000]0.0;[$-F800]dddd, mmmm dd, yyyy", OCT_10), "sábado, 10 de outubro de 2026");
    assert_eq!(fl(&pt, "[>50000]0.0;[$-F800]dddd, mmmm dd, yyyy", 60000.5), "60000,5");
    // The tag wins over whatever pattern follows it.
    assert_eq!(fl(&pt, "[$-F800]yyyy", OCT_10), "sábado, 10 de outubro de 2026");
    assert_eq!(fl(&pt, "[$-F400]h:mm", 0.75), "18:00:00");
    // en-US is left alone.
    assert_eq!(fl(&INVARIANT, "[$-F800]dddd, mmmm dd, yyyy", OCT_7), "Wednesday, October 07, 2026");
}

#[test]
fn general_fit_follows_the_region() {
    let pt = region("pt-BR").unwrap();
    assert_eq!(format_general_fit_in(1.5, 11, pt).as_deref(), Some("1,5"));
    assert_eq!(format_general_fit_in(-1234.5678, 6, pt).as_deref(), Some("-1235"));
    assert_eq!(format_general_fit_in(1234.5678, 7, pt).as_deref(), Some("1234,57"));
    assert_eq!(format_general_fit_in(123456789012.0, 11, pt).as_deref(), Some("1,23457E+11"));
    assert_eq!(format_general_fit_in(f64::NAN, 11, pt), None);
    assert_eq!(format_general_fit_in(123456.0, 3, pt), None);
    // The invariant function is the invariant region, and a wide separator still fits.
    assert_eq!(format_general_fit(1.5, 11), format_general_fit_in(1.5, 11, region("en-US").unwrap()));
    let wide = region("en-US").unwrap().with_separators('．', ',');
    let s = format_general_fit_in(1234.5678, 8, &wide).unwrap();
    assert_eq!(s, "1234．568");
    assert!(s.chars().count() <= 8);
}

/// Codes used to check the round trip: every built-in plus customs covering dates, times,
/// conditions, fractions, literals and escapes.
fn sample_codes() -> Vec<String> {
    let mut codes: Vec<String> = (0..=49).filter_map(builtin_format).map(str::to_string).collect();
    for c in [
        "0.00%",
        "#,##0.00_);[Red](#,##0.00)",
        "\"Total: \"#,##0.00",
        "0.0,\"K\"",
        "#,##0,,\"M\"",
        "[>1000]#,##0.0,\"K\";[<-1000]-#,##0.0,\"K\";0",
        "[>1.5]0.0;[<=-1.5]-0.0;0.00",
        "[<=9999999]###-####;(###) ###-####",
        "dddd, mmmm d, yyyy",
        "dd/mm/yyyy",
        "dd.mm.yyyy",
        "yyyy-mm-dd hh:mm:ss",
        "yyyy-mm-dd hh:mm:ss.000",
        "mm:ss.0",
        "[h]:mm:ss",
        "[hh]:mm",
        "[mm]:ss",
        "[ss]",
        "MM/DD/YYYY HH:MM:SS",
        "m/d/yy h:mm:ss AM/PM",
        "h:mm AM/PM",
        "h a/p",
        "[$-409]d-mmm-yy",
        "[$-F800]dddd, mmmm dd, yyyy",
        "[$R$-416] #,##0.00",
        "yyyy\"年\"m\"月\"d\"日\"",
        "General\" kg\"",
        "General;General;\"zero\"",
        "0.00E+00",
        "##0.0E+0",
        "# ?/?",
        "# ??/??",
        "?/8",
        "@",
        "@\" units\"",
        "0;-0;;@",
        "\\#\\,\\# 0",
        "\"a\"0\"s\"",
        "0 \"d\"",
        "*-0.00",
        "_-* #,##0.00_-",
        "[Red]0.00;[Blue]-0.00",
        "0.00,",
        ".00",
        "#,##0.0##",
        "general",
        "GENERAL",
        "GeNeRaL\" kg\"",
        "0.0 a",
        "0 y",
        "0 x",
        "0.0\u{a0}x",
        "# #/#",
        "# 0/0",
        "0 0/0",
        "# ##0 ?/?",
        "[Red][$-F800]dddd, mmmm dd, yyyy",
        "[$-F400]h:mm:ss AM/PM;@",
        "",
    ] {
        codes.push(c.to_string());
    }
    codes
}

#[test]
fn local_codes_round_trip_in_every_dialect() {
    let codes = sample_codes();
    for lang in all_languages() {
        for reg in REGIONS {
            let d = Dialect { names: lang, regional: reg };
            for code in &codes {
                let local = to_local_code(code, &d);
                assert_eq!(from_local_code(&local, &d), *code, "{} / {}: {code:?} -> {local:?}", lang.tag, reg.tag);
            }
        }
    }
}

#[test]
fn local_codes_render_like_the_canonical_ones() {
    // Typing the local spelling in TEXT() gives what the canonical code gives in the same locale.
    for lang in all_languages() {
        for reg in REGIONS {
            let l = Locale::new(lang, lang, **reg);
            for code in sample_codes().into_iter().filter(|c| !c.is_empty()) {
                let local = to_local_code(&code, &l.dialect());
                for n in [0.0, 0.5, 1234.5678, OCT_10, OCT_7 + 0.6, -3.25] {
                    let got = text_function_in(&Value::Number(n), &local, S, &l);
                    let want = format_value_in(&Value::Number(n), &NumberFormat::parse(&code), S, &l);
                    let context = format!("{} / {}: {code:?} as {local:?} on {n}", lang.tag, reg.tag);
                    if want.text == "########" {
                        // A date that cannot be shown: TEXT() reports #VALUE!.
                        assert_eq!(got, Err(CellError::Value), "{context}");
                    } else {
                        assert_eq!(got, Ok(want.text), "{context}");
                    }
                }
            }
        }
    }
}

#[test]
fn month_and_minute_letters_agree() {
    // Date letters are case-insensitive: a language may write the month `M` and the minute `m`,
    // but they must be the same letter.
    for lang in all_languages() {
        assert!(lang.format.month.to_lowercase().eq(lang.format.minute.to_lowercase()), "{}", lang.tag);
    }
}

#[test]
fn builtins_round_trip_through_ids() {
    for id in (0..=49).filter(|id| builtin_format(*id).is_some()) {
        let code = builtin_format(id).unwrap();
        assert_eq!(builtin_id(code), Some(id));
    }
}
