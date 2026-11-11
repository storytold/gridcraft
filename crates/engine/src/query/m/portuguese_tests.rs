//! Portuguese full date names and the bounded standard formatting profile.
//! No external data, machine culture or installed spreadsheet application is used.

use gridcraft_model::Workbook;

use super::eval::Ev;
use super::value::{MResult, MValue};
use crate::query::{DataAccess, EvalCx, FetchCache};

struct NoAccess;

impl DataAccess for NoAccess {
    fn read_file(&self, _: &str, _: usize) -> Result<Vec<u8>, String> {
        Err("Portuguese culture tests do not read external files.".into())
    }
}

fn evaluate(formula: &str) -> MResult<MValue> {
    let wb = Workbook::new();
    let mut cache = FetchCache::default();
    let mut cx = EvalCx::new(&wb, &NoAccess, Err("No external sources in culture tests.".into()), &mut cache);
    Ev::new(&mut cx).eval(&super::parser::parse(formula)?, &None)
}

fn assert_text(formula: &str, expected: &str) {
    let actual = evaluate(formula).unwrap();
    let MValue::Text(actual) = actual else { panic!("Expected text for {formula}, got {actual:?}") };
    assert_eq!(actual.as_ref(), expected, "{formula}");
}

#[test]
fn portuguese_weekday_names_cover_the_week_and_the_reported_thursday() {
    // The reported pt-BR regression was an English Thursday rather than quinta-feira.
    assert_text("Date.DayOfWeekName(#date(2026, 5, 21), \"pt-BR\")", "quinta-feira");
    let days = ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"];
    for culture in ["pt", "pt-BR", "pt-PT", "PT_br"] {
        for (offset, expected) in days.iter().enumerate() {
            assert_text(&format!("Date.DayOfWeekName(#date(2026, 5, {}), \"{culture}\")", 17 + offset), expected);
        }
    }
}

#[test]
fn portuguese_month_names_cover_the_year_including_unicode() {
    let months = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
    for culture in ["pt", "pt-BR", "pt-PT", "PT_pt"] {
        for (index, expected) in months.iter().enumerate() {
            assert_text(&format!("Date.MonthName(#date(2026, {}, 15), \"{culture}\")", index + 1), expected);
        }
    }
}

#[test]
fn portuguese_names_keep_null_and_date_time_contracts() {
    for formula in ["Date.DayOfWeekName(null, \"pt-BR\")", "Date.MonthName(null, \"pt-PT\")"] {
        assert!(matches!(evaluate(formula).unwrap(), MValue::Null), "{formula}");
    }
    assert_text("Date.DayOfWeekName(#datetime(2026, 5, 18, 13, 5, 9), \"pt-BR\")", "segunda-feira");
    assert_text("Date.MonthName(#datetime(2026, 3, 4, 13, 5, 9), \"pt-PT\")", "março");
    // The date component is local to the supplied zone, without an extra UTC conversion.
    assert_text("Date.DayOfWeekName(#datetimezone(2026, 3, 4, 23, 15, 0, -12, 0), \"pt-BR\")", "quarta-feira");
    assert_text("Date.MonthName(#datetimezone(2026, 3, 31, 23, 15, 0, -12, 0), \"pt-PT\")", "março");
}

#[test]
fn portuguese_formatting_uses_full_names_and_standard_patterns() {
    for culture in ["pt", "pt-BR", "pt-PT", "PT_br"] {
        for options in [format!("null, \"{culture}\""), format!("[Culture = \"{culture}\"]")] {
            assert_text(&format!("Date.ToText(#date(2026, 3, 4), {options})"), "04/03/2026");
            assert_text(&format!("DateTime.ToText(#datetime(2026, 3, 4, 13, 5, 9), {options})"), "04/03/2026 13:05:09");
            assert_text(&format!("Time.ToText(#time(13, 5, 9), {options})"), "13:05:09");
        }
        assert_text(&format!("Date.ToText(#date(2026, 3, 4), [Format = \"D\", Culture = \"{culture}\"])"), "quarta-feira, 4 de março de 2026");
        assert_text(&format!("Date.ToText(#date(2026, 3, 4), \"dddd, d 'de' MMMM 'de' yyyy\", \"{culture}\")"), "quarta-feira, 4 de março de 2026");
        assert_text(&format!("Date.ToText(#date(2026, 3, 4), \"yyyy-MM-dd\", \"{culture}\")"), "2026-03-04");
        assert_text(&format!("Text.From(#date(2026, 3, 4), \"{culture}\")"), "04/03/2026");
        assert_text(&format!("Text.From(#datetime(2026, 3, 4, 13, 5, 9), \"{culture}\")"), "04/03/2026 13:05:09");
        assert_text(&format!("Text.From(#time(13, 5, 9), \"{culture}\")"), "13:05:09");
    }
    assert!(matches!(evaluate("Date.ToText(null, [Culture = \"pt-BR\"])").unwrap(), MValue::Null));
}

#[test]
fn portuguese_culture_does_not_suppress_type_or_source_errors() {
    for formula in [
        "Date.DayOfWeekName(\"2026-05-21\", \"pt-BR\")",
        "Date.MonthName(1, \"pt-PT\")",
        "Date.DayOfWeekName({}, \"pt-BR\")",
        "Date.MonthName(#date(2026, 3, 4), 17)",
        "Date.DayOfWeekName(#date(2026, 5, 21), {})",
        "Date.ToText(#date(2026, 3, 4), null, 17)",
    ] {
        assert_eq!(evaluate(formula).unwrap_err().reason, "Expression.Error", "{formula}");
    }
    let error = evaluate("Date.DayOfWeekName(error Error.Record(\"Culture.Test\", \"original failure\", null), \"pt-BR\")").unwrap_err();
    assert_eq!(error.reason, "Culture.Test");
    assert_eq!(error.message, "original failure");
}

#[test]
fn existing_portuguese_numeric_date_conversion_and_other_cultures_stay_unchanged() {
    for culture in ["pt-BR", "pt-PT"] {
        let formula = format!("Date.FromText(\"04/03/2026\", [Culture = \"{culture}\"]) = #date(2026, 3, 4)");
        assert!(matches!(evaluate(&formula).unwrap(), MValue::Logical(true)), "{formula}");
        let formula = format!("Number.FromText(\"1.234,5\", \"{culture}\") = 1234.5");
        assert!(matches!(evaluate(&formula).unwrap(), MValue::Logical(true)), "{formula}");
        assert_eq!(evaluate(&format!("Date.FromText(\"31/02/2026\", [Culture = \"{culture}\"])")).unwrap_err().reason, "DataFormat.Error");
    }
    for (formula, expected) in [
        ("Date.DayOfWeekName(#date(2026, 5, 21))", "Thursday"),
        ("Date.DayOfWeekName(#date(2026, 5, 21), null)", "Thursday"),
        ("Date.DayOfWeekName(#date(2026, 5, 21), \"en-US\")", "Thursday"),
        ("Date.DayOfWeekName(#date(2026, 5, 21), \"de-DE\")", "Donnerstag"),
        ("Date.DayOfWeekName(#date(2026, 5, 21), \"nl-NL\")", "donderdag"),
        ("Date.DayOfWeekName(#date(2026, 5, 21), \"unknown\")", "Thursday"),
        ("Date.MonthName(#date(2026, 3, 4))", "March"),
        ("Date.MonthName(#date(2026, 3, 4), null)", "March"),
        ("Date.ToText(#date(2026, 3, 4))", "3/4/2026"),
        ("Date.ToText(#date(2026, 3, 4), null, \"de-DE\")", "04.03.2026"),
        ("Date.ToText(#date(2026, 3, 4), null, \"nl-NL\")", "4-3-2026"),
        ("Date.ToText(#date(2026, 3, 4), null, \"unknown\")", "3/4/2026"),
    ] {
        assert_text(formula, expected);
    }
}
