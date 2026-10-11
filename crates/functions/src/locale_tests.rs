//! Behaviour that depends on the workbook's language and region.

use gridcraft_core::{CellError, Value};

use crate::util::testutil::*;

fn de_de() -> gridcraft_locale::Locale {
    let lang = gridcraft_locale::language("de-DE").unwrap();
    gridcraft_locale::Locale::new(lang, lang, *gridcraft_locale::region("de-DE").unwrap())
}

#[test]
fn text_to_number_conversions_use_the_region() {
    close(ev_in(pt_br(), "VALUE", vec![t("1,5")]), 1.5);
    close(ev_in(pt_br(), "VALUE", vec![t("1.234,5")]), 1234.5);
    close(ev_in(pt_br(), "VALUE", vec![t("R$ 3")]), 3.0);
    is_err(ev("VALUE", vec![t("1,5x")]), CellError::Value);
    close(ev("VALUE", vec![t("1,500")]), 1500.0);
    close(ev_in(pt_br(), "NUMBERVALUE", vec![t("2.500,27")]), 2500.27);
    close(ev("NUMBERVALUE", vec![t("2,500.27")]), 2500.27);
    close(ev_in(de_de(), "NUMBERVALUE", vec![t("1.234,5")]), 1234.5);
    close(ev_in(pt_br(), "NUMBERVALUE", vec![t("2.500,27"), t(","), t(".")]), 2500.27);
    close(ev_in(pt_br(), "DATEVALUE", vec![t("10/10/2026")]), 46305.0);
    close(ev("DATEVALUE", vec![t("10/10/2026")]), 46305.0);
    close(ev_in(pt_br(), "DATEVALUE", vec![t("2/1/2020")]), 43832.0);
    close(ev("DATEVALUE", vec![t("2/1/2020")]), 43862.0);
    close(ev_in(pt_br(), "TIMEVALUE", vec![t("18:00")]), 0.75);
    close(ev_in(pt_br(), "SUM", vec![t("1,5"), n(1.0)]), 2.5);
}

#[test]
fn fixed_and_dollar_use_the_region() {
    is_text(ev_in(pt_br(), "FIXED", vec![n(1234.5), n(1.0)]), "1.234,5");
    is_text(ev_in(pt_br(), "FIXED", vec![n(1234.5), n(1.0), b(true)]), "1234,5");
    is_text(ev("FIXED", vec![n(1234.5), n(1.0)]), "1,234.5");
    is_text(ev_in(pt_br(), "DOLLAR", vec![n(1234.567)]), "R$ 1.234,57");
    is_text(ev_in(pt_br(), "DOLLAR", vec![n(-1234.567), n(1.0)]), "-R$ 1.234,6");
    is_text(ev_in(de_de(), "DOLLAR", vec![n(1234.567)]), "1.234,57 €");
    is_text(ev("DOLLAR", vec![n(-1234.567), n(1.0)]), "($1,234.6)");
}

#[test]
fn concatenation_and_conversion_functions_write_local_numbers() {
    is_text(ev_in(pt_br(), "CONCAT", vec![n(1.5), t("|"), b(true)]), "1,5|VERDADEIRO");
    is_text(ev("CONCAT", vec![n(1.5), t("|"), b(true)]), "1.5|TRUE");
    is_text(ev_in(pt_br(), "VALUETOTEXT", vec![n(1.5)]), "1,5");
    is_text(ev_in(pt_br(), "VALUETOTEXT", vec![e(CellError::NA)]), "#N/D");
    is_text(ev_in(pt_br(), "TEXTJOIN", vec![t(";"), b(true), n(2.5), n(3.0)]), "2,5;3");
}

#[test]
fn criteria_read_local_numbers_and_booleans() {
    let nums = col(&[1.0, 1.4, 1.5, 2.0]);
    close(ev_in(pt_br(), "COUNTIF", vec![rf(nums.clone()), t("<1,5")]), 2.0);
    close(ev_in(pt_br(), "COUNTIF", vec![rf(nums.clone()), t(">=1,5")]), 2.0);
    close(ev_in(pt_br(), "COUNTIF", vec![rf(nums.clone()), t("1,5")]), 1.0);
    close(ev("COUNTIF", vec![rf(nums), t("<1.5")]), 2.0);
    let mixed = arr(vec![vec![Value::Bool(true)], vec![Value::Bool(false)], vec![Value::Number(1.0)]]);
    close(ev_in(pt_br(), "COUNTIF", vec![rf(mixed.clone()), t("VERDADEIRO")]), 1.0);
    close(ev("COUNTIF", vec![rf(mixed), t("TRUE")]), 1.0);
    close(ev_in(pt_br(), "SUMIF", vec![rf(col(&[1.0, 2.0, 3.0])), t(">1,5")]), 5.0);
    // Text cells holding local numbers match an equal number.
    let cells = arr(vec![vec![Value::from("1,5")], vec![Value::from("2,5")]]);
    close(ev_in(pt_br(), "COUNTIF", vec![rf(cells), t("2,5")]), 1.0);
}

#[test]
fn byte_functions_count_double_byte_characters_only_for_dbcs_interfaces() {
    close(ev_in(ja_jp(), "LENB", vec![t("日本")]), 4.0);
    close(ev("LENB", vec![t("日本")]), 2.0);
    close(ev_in(pt_br(), "LENB", vec![t("日本")]), 2.0);
    close(ev_in(ja_jp(), "LENB", vec![t("ab日")]), 4.0);
    is_text(ev_in(ja_jp(), "LEFTB", vec![t("日本語"), n(4.0)]), "日本");
    is_text(ev_in(ja_jp(), "LEFTB", vec![t("日本語"), n(3.0)]), "日 ");
    is_text(ev_in(ja_jp(), "RIGHTB", vec![t("日本語"), n(2.0)]), "語");
    is_text(ev_in(ja_jp(), "MIDB", vec![t("a日本"), n(2.0), n(2.0)]), "日");
    close(ev_in(ja_jp(), "FINDB", vec![t("本"), t("日本語")]), 3.0);
    close(ev_in(ja_jp(), "SEARCHB", vec![t("本"), t("a日本語")]), 4.0);
    is_text(ev_in(ja_jp(), "REPLACEB", vec![t("日本語"), n(3.0), n(2.0), t("x")]), "日x語");
    is_text(ev("LEFTB", vec![t("日本語"), n(2.0)]), "日本");
}

#[test]
fn info_and_cell_keywords_accept_local_spellings() {
    let local = pt_br().formula.info_types.iter().find(|(c, _)| *c == "recalc").map(|(_, l)| *l).unwrap();
    is_text(ev_in(pt_br(), "INFO", vec![t(local)]), "Automatic");
    is_text(ev_in(pt_br(), "INFO", vec![t("RECALC")]), "Automatic");
    is_err(ev_in(pt_br(), "INFO", vec![t("bogus")]), CellError::Value);
}

#[test]
fn address_uses_the_local_r1c1_letters() {
    is_text(ev_in(pt_br(), "ADDRESS", vec![n(2.0), n(3.0), n(1.0), b(false)]), "L2C3");
    is_text(ev("ADDRESS", vec![n(2.0), n(3.0), n(1.0), b(false)]), "R2C3");
    is_text(ev_in(pt_br(), "ADDRESS", vec![n(2.0), n(3.0)]), "$C$2");
}

#[test]
fn engineering_text_uses_the_region_decimal() {
    is_text(ev_in(pt_br(), "COMPLEX", vec![n(1.5), n(2.5)]), "1,5+2,5i");
    close(ev_in(pt_br(), "IMREAL", vec![t("1,5+2,5i")]), 1.5);
    is_text(ev("COMPLEX", vec![n(1.5), n(2.5)]), "1.5+2.5i");
}

#[test]
fn byte_functions_use_the_code_page_width_rule() {
    // ASCII and half-width katakana are one byte; every other character is two.
    close(ev_in(ja_jp(), "LENB", vec![t("α")]), 2.0);
    close(ev_in(ja_jp(), "LENB", vec![t("Ж")]), 2.0);
    close(ev_in(ja_jp(), "LENB", vec![t("ｱ")]), 1.0);
    close(ev_in(ja_jp(), "LENB", vec![t("Ａ")]), 2.0);
    close(ev_in(ja_jp(), "FINDB", vec![t("x"), t("αx")]), 3.0);
    is_text(ev_in(ja_jp(), "MIDB", vec![t("αx"), n(3.0), n(1.0)]), "x");
}

#[test]
fn byte_search_validates_the_byte_start() {
    // A zero-length slice never produces the space that stands in for a cut character.
    is_text(ev_in(ja_jp(), "MIDB", vec![t("日"), n(2.0), n(0.0)]), "");
    is_text(ev_in(ja_jp(), "MIDB", vec![t("日"), n(2.0), n(1.0)]), " ");
    is_err(ev_in(ja_jp(), "FINDB", vec![t(""), t("日"), n(100.0)]), CellError::Value);
    is_err(ev_in(ja_jp(), "SEARCHB", vec![t(""), t("日"), n(100.0)]), CellError::Value);
    is_err(ev_in(ja_jp(), "FINDB", vec![t("x"), t("日"), n(4.0)]), CellError::Value);
    close(ev_in(ja_jp(), "FINDB", vec![t(""), t("日"), n(3.0)]), 3.0);
    close(ev_in(ja_jp(), "FINDB", vec![t(""), t(""), n(1.0)]), 1.0);
    is_err(ev_in(ja_jp(), "FINDB", vec![t(""), t(""), n(2.0)]), CellError::Value);
    is_err(ev_in(ja_jp(), "FINDB", vec![t("a"), t("a"), n(0.0)]), CellError::Value);
}

#[test]
fn arraytotext_strict_uses_the_regional_array_separators() {
    let a = arr(vec![vec![nv(1.5), nv(2.5)], vec![nv(3.0), tv("x")]]);
    is_text(ev_in(pt_br(), "ARRAYTOTEXT", vec![av(a.clone()), n(1.0)]), "{1,5\\2,5;3\\\"x\"}");
    is_text(ev("ARRAYTOTEXT", vec![av(a.clone()), n(1.0)]), "{1.5,2.5;3,\"x\"}");
    is_text(ev_in(pt_br(), "ARRAYTOTEXT", vec![av(a)]), "1,5, 2,5, 3, x");
}
