//! UI formula syntax boundary. Engine commands, control/API callers and XLSX keep English.
use gridcraft_engine::formula::{self, FormulaLocale, ParseError};
use gridcraft_engine::model::Workbook;

pub(crate) fn canonical(
    text: &str,
    locale: FormulaLocale,
    wb: &Workbook,
    sheet: usize,
    cell: gridcraft_engine::core::CellRef,
) -> Result<String, ParseError> {
    // The editor hid the `_xludf.` prefixes of this cell's formula; confirming it unchanged keeps
    // the user-defined calls instead of binding them to the built-ins of the same name.
    if let Some(stored) = wb.sheet(sheet).and_then(|sh| sh.cell(cell)).filter(|c| c.formula.is_some()).map(|c| c.input_text())
        && has_udf_prefix(&stored)
        && text == display(&stored, locale, wb, sheet)
    {
        return Ok(stored);
    }
    if locale == FormulaLocale::En || wb.sheet(sheet).is_some_and(|sh| wb.styles.get(sh.style_id(cell)).num_fmt.as_str() == "@") {
        return Ok(text.to_string());
    }
    // Same formula detection as the engine's `input_to_cell` (no trimming: " =1" is text).
    let body = text;
    if body.starts_with('=')
        || ((body.starts_with('+') || body.starts_with('-')) && body.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic() || c == '('))
    {
        let known = |name: &str| wb.name(name, sheet).is_some() || wb.table(name).is_some();
        // Match the engine's existing convenience for missing closing parentheses.
        let mut body = body.to_string();
        let mut expr = formula::parse_input(&body, locale, known);
        for _ in 0..8 {
            if expr.is_ok() {
                break;
            }
            body.push(')');
            expr = formula::parse_input(&body, locale, known);
        }
        return Ok(format!("={}", formula::print(&expr?)));
    }
    // Decimal-comma number entry follows the same preference, without rewriting text or dates.
    let normalized = body.trim().replace(locale.decimal_separator(), ".");
    let number = normalized.strip_suffix('%').unwrap_or(&normalized);
    if number.parse::<f64>().is_ok_and(f64::is_finite) {
        return Ok(normalized);
    }
    Ok(text.to_string())
}

pub(crate) fn display(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    // English (and every language without a formula table) shows the stored text, `_xludf.`
    // prefixes aside, and skips a parse per frame.
    if locale == FormulaLocale::En {
        return hide_udf_prefix(text, locale);
    }
    let Some(body) = text.strip_prefix('=') else { return text.to_string() };
    let shown = formula::parse(body)
        .and_then(|expr| formula::print_input(&expr, locale, |name| wb.name(name, sheet).is_some() || wb.table(name).is_some()))
        .map(|body| format!("={body}"))
        .unwrap_or_else(|_| text.to_string());
    hide_udf_prefix(&shown, locale)
}

const UDF_PREFIX: &str = "_xludf.";

fn has_udf_prefix(text: &str) -> bool {
    text.to_ascii_lowercase().contains(UDF_PREFIX)
}

/// Excel shows a user-defined call such as `_xludf.EOMONTH()` as `=EOMONTH()`. Only function
/// names lose the prefix: text literals and names keep it.
fn hide_udf_prefix(text: &str, locale: FormulaLocale) -> String {
    let Some(body) = text.strip_prefix('=').filter(|b| has_udf_prefix(b)) else { return text.to_string() };
    let Ok(tokens) = formula::lexer::tokenize_locale(body, locale) else { return text.to_string() };
    let mut out = String::from("=");
    let mut from = 0;
    for t in tokens {
        if matches!(t.tok, formula::lexer::Tok::Func(_))
            && body.get(t.start..t.start + UDF_PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(UDF_PREFIX))
        {
            out.push_str(body.get(from..t.start).unwrap_or(""));
            from = t.start + UDF_PREFIX.len();
        }
    }
    out.push_str(body.get(from..).unwrap_or(""));
    out
}

// Descriptions/signatures for the supported Spanish function table; untranslated functions
// retain their English help. Parameter punctuation follows the same locale as entry.
const SPANISH_HELP: &[(&str, &str, &str)] = &[
    ("SUM", "SUMA(número1; [número2]; ...)", "Suma los números."),
    ("AVERAGE", "PROMEDIO(número1; [número2]; ...)", "Devuelve el promedio de los números."),
    ("IF", "SI(prueba_lógica; valor_si_verdadero; [valor_si_falso])", "Devuelve un valor según una condición."),
    ("COUNT", "CONTAR(valor1; [valor2]; ...)", "Cuenta las celdas que contienen números."),
    ("SUMIF", "SUMAR.SI(rango; criterio; [rango_suma])", "Suma las celdas que cumplen un criterio."),
    ("COUNTIF", "CONTAR.SI(rango; criterio)", "Cuenta las celdas que cumplen un criterio."),
    ("VLOOKUP", "BUSCARV(valor_buscado; matriz; indicador_columnas; [ordenado])", "Busca un valor en la primera columna de una tabla."),
    ("TODAY", "HOY()", "Devuelve la fecha actual."),
    ("TRUE", "VERDADERO()", "Devuelve el valor lógico VERDADERO."),
    ("FALSE", "FALSO()", "Devuelve el valor lógico FALSO."),
];

// Italian help for the most common functions; the rest of the Italian table falls back to the
// English help with Italian separators.
const ITALIAN_HELP: &[(&str, &str, &str)] = &[
    ("SUM", "SOMMA(num1; [num2]; ...)", "Somma i numeri."),
    ("AVERAGE", "MEDIA(num1; [num2]; ...)", "Restituisce la media dei numeri."),
    ("IF", "SE(test; [se_vero]; [se_falso])", "Restituisce un valore in base a una condizione."),
    ("COUNT", "CONTA.NUMERI(val1; [val2]; ...)", "Conta le celle che contengono numeri."),
    ("COUNTA", "CONTA.VALORI(val1; [val2]; ...)", "Conta le celle non vuote."),
    ("SUMIF", "SOMMA.SE(intervallo; criterio; [int_somma])", "Somma le celle che soddisfano un criterio."),
    ("COUNTIF", "CONTA.SE(intervallo; criterio)", "Conta le celle che soddisfano un criterio."),
    ("IFERROR", "SE.ERRORE(valore; valore_se_errore)", "Restituisce un valore alternativo se la formula restituisce un errore."),
    ("VLOOKUP", "CERCA.VERT(valore; matrice_tabella; indice; [intervallo])", "Cerca un valore nella prima colonna di una tabella."),
    ("ROUND", "ARROTONDA(num; num_cifre)", "Arrotonda un numero al numero di cifre specificato."),
    ("TODAY", "OGGI()", "Restituisce la data corrente."),
    ("NOW", "ADESSO()", "Restituisce la data e l'ora correnti."),
    ("TRUE", "VERO()", "Restituisce il valore logico VERO."),
    ("FALSE", "FALSO()", "Restituisce il valore logico FALSO."),
];

fn help(locale: FormulaLocale) -> &'static [(&'static str, &'static str, &'static str)] {
    match locale {
        FormulaLocale::En => &[],
        FormulaLocale::Es => SPANISH_HELP,
        FormulaLocale::It => ITALIAN_HELP,
    }
}

pub(crate) fn signature(name: &str, locale: FormulaLocale) -> Option<String> {
    let canonical = locale.canonical_name(name);
    if let Some((_, signature, _)) = help(locale).iter().find(|(name, ..)| *name == canonical) {
        return Some((*signature).into());
    }
    crate::formula_bar::function_signature(canonical).map(|s| s.replace(',', &locale.list_separator().to_string()))
}

pub(crate) fn description(name: &str, locale: FormulaLocale) -> Option<String> {
    let canonical = locale.canonical_name(name);
    if let Some((_, _, description)) = help(locale).iter().find(|(name, ..)| *name == canonical) {
        return Some((*description).into());
    }
    crate::formula_bar::function_description(canonical)
}

/// Avoid suggesting an alias that would call a workbook/table name instead of the function.
pub(crate) fn completion_name<'a>(name: &'a str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> &'a str {
    let alias = locale.function_name(name);
    if wb.name(alias, sheet).is_some() || wb.table(alias).is_some() { name } else { alias }
}

/// Engine-generated edit templates use canonical syntax. The only incomplete template is
/// the bare function opening emitted by Insert Function; typed user input never enters here.
pub(crate) fn display_template(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    if let Some(name) = text.strip_prefix('=').and_then(|s| s.strip_suffix('('))
        && !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
    {
        return format!("={}(", completion_name(name, locale, wb, sheet));
    }
    display(text, locale, wb, sheet)
}
