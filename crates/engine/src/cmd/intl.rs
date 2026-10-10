//! Language and regional settings: `app.setLocale`, `app.getInternational`, `app.languages`
//! and the Options dialog.

use serde_json::{Value as Json, json};

use super::*;
use crate::LocalePrefs;
use crate::UiRequest;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            pref "app.setLocale",
            "Set Language and Region",
            [],
            None,
            "{uiLanguage?: \"system\"|\"pt-BR\"|…, formulaLanguage?: \"followUi\"|\"en-US\"|…, regionalFormat?: \"system\"|\"de-DE\"|…, useSystemSeparators?: bool, decimalSeparator?: \",\", thousandsSeparator?: \".\"} → the app.getInternational payload; applies at once, recalculating in automatic mode",
            always,
            set_locale
        ),
        cmd!(query "app.getInternational", "International Settings", [], None, "{} → separators, date order, R1C1 letters, booleans, currency, the active languages and the system locale the \"system\" preferences resolve to (systemLocale, systemLanguage, systemRegion) (Application.International)", always, get_international),
        cmd!(query "app.languages", "Available Languages", [], None, "{} → {languages: [{tag, nativeName, functionsTranslated}], regions: [{tag, nativeName}]}", always, languages),
        cmd!(pref "file.options", "Options…", ["File"], None, "{} (opens the Options dialog)", always, options),
    ]
}

fn text(p: &Json, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(t)) => Ok(Some(t.clone())),
        Some(_) => Err(bad("app.setLocale", format!("`{key}` must be a string"))),
    }
}

fn separator(p: &Json, key: &str) -> Result<Option<char>> {
    let Some(t) = text(p, key)? else { return Ok(None) };
    let mut it = t.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Ok(Some(c)),
        _ => Err(bad("app.setLocale", format!("`{key}` must be exactly one character"))),
    }
}

fn set_locale(s: &mut Session, p: &Json) -> Result<Json> {
    let use_system_separators = match p.get("useSystemSeparators") {
        None | Some(Json::Null) => None,
        Some(Json::Bool(b)) => Some(*b),
        Some(_) => return Err(bad("app.setLocale", "`useSystemSeparators` must be true or false")),
    };
    let prefs = LocalePrefs {
        ui_language: text(p, "uiLanguage")?,
        formula_language: text(p, "formulaLanguage")?,
        regional_format: text(p, "regionalFormat")?,
        use_system_separators,
        decimal_separator: separator(p, "decimalSeparator")?,
        thousands_separator: separator(p, "thousandsSeparator")?,
    };
    s.set_locale(prefs)?;
    Ok(crate::locale::international(s))
}

fn get_international(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(crate::locale::international(s))
}

fn languages(_: &mut Session, _: &Json) -> Result<Json> {
    let languages: Vec<Json> = gridcraft_locale::LANGUAGES
        .iter()
        .map(|l| json!({"tag": l.tag, "nativeName": l.native_name, "functionsTranslated": l.functions.iter().any(|(canonical, local)| canonical != local)}))
        .collect();
    let regions: Vec<Json> = gridcraft_locale::REGIONS.iter().map(|r| json!({"tag": r.tag, "nativeName": r.native_name})).collect();
    Ok(json!({"languages": languages, "regions": regions}))
}

fn options(s: &mut Session, _: &Json) -> Result<Json> {
    s.ui_requests.push(UiRequest::Dialog("options".into(), json!({})));
    ok()
}
