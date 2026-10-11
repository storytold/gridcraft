//! File › Options › Language: interface language, formula language, regional format and
//! separators. Applying runs `app.setLocale`, which takes effect at once, without a restart.

use egui::vec2;
use serde_json::{Map, Value as Json, json};

use crate::SheetApp;
use crate::dialogs::Dialog;
use crate::l10n::Localizer;

/// Initial form values: the stored preferences. The separator fields start with what is in effect
/// (the region's own while the system separators are in use, else the custom pair).
pub fn defaults(app: &mut SheetApp) -> Json {
    let intl = app.session.execute("app.getInternational", json!({})).unwrap_or(Json::Null);
    let prefs = intl.get("prefs").cloned().unwrap_or(Json::Null);
    let pref = |key: &str, fallback: &str| prefs.get(key).and_then(Json::as_str).filter(|s| !s.is_empty()).unwrap_or(fallback).to_string();
    let active = |key: &str| intl.get(key).and_then(Json::as_str).unwrap_or("").to_string();
    let system = prefs.get("useSystemSeparators").and_then(Json::as_bool).unwrap_or(true);
    let separator = |pref_key: &str, active_key: &str| if system { active(active_key) } else { pref(pref_key, &active(active_key)) };
    json!({
        "uiLanguage": pref("uiLanguage", "system"),
        "formulaLanguage": pref("formulaLanguage", "followUi"),
        "regionalFormat": pref("regionalFormat", "system"),
        "useSystemSeparators": system,
        "decimalSeparator": separator("decimalSeparator", "decimal"),
        "thousandsSeparator": separator("thousandsSeparator", "thousands"),
        // The region the "System" choice resolves to, whatever region is stored.
        "systemRegion": active("systemRegion"),
    })
}

fn text(values: &Map<String, Json>, key: &str) -> String {
    values.get(key).and_then(Json::as_str).unwrap_or_default().to_string()
}

/// The decimal and thousands separators the fields show after the Region choice or the "use
/// system separators" box changed: those of the chosen region (the system's own region for
/// "System"). `None` when nothing changed, or when custom separators are in use.
fn region_separators(values: &Map<String, Json>, region: &str, system: bool) -> Option<(String, String)> {
    let was_system = values.get("useSystemSeparators").and_then(Json::as_bool).unwrap_or(true);
    if !system || (region == text(values, "regionalFormat") && was_system) {
        return None;
    }
    let tag = if region == "system" { text(values, "systemRegion") } else { region.to_string() };
    let r = gridcraft_locale::region(&tag)?;
    Some((r.decimal.to_string(), r.group.to_string()))
}

/// The one character that was typed into a separator field holding `previous`: what is left of
/// `now` once the old character is taken out (the last character when there is no such change).
fn single_char(previous: &str, now: &str) -> String {
    if now.chars().count() <= 1 {
        return now.to_string();
    }
    let mut old = previous.chars().next();
    let typed: Vec<char> = now
        .chars()
        .filter(|c| {
            if old == Some(*c) {
                old = None;
                false
            } else {
                true
            }
        })
        .collect();
    typed.last().copied().or_else(|| now.chars().last()).map(String::from).unwrap_or_default()
}

/// A drop-down over `(value, label)` pairs that edits `current`.
fn choice(ui: &mut egui::Ui, id: &str, current: &mut String, items: &[(String, String)]) {
    let shown = items.iter().find(|(v, _)| v == current).map_or(current.as_str(), |(_, label)| label.as_str()).to_string();
    egui::ComboBox::from_id_salt(id).selected_text(shown).width(260.0).show_ui(ui, |ui| {
        for (value, label) in items {
            ui.selectable_value(current, value.clone(), label);
        }
    });
}

/// Draws the Language page, editing the dialog's values.
pub fn show(l: Localizer, ui: &mut egui::Ui, d: &mut Dialog) {
    ui.label(egui::RichText::new(l.text("ui-options-page-language", &[])).strong().size(15.0));
    ui.label(egui::RichText::new(l.text("ui-options-intro", &[])).weak());
    ui.add_space(6.0);

    let mut ui_language = text(&d.values, "uiLanguage");
    let mut formula_language = text(&d.values, "formulaLanguage");
    let mut region = text(&d.values, "regionalFormat");
    let mut system = d.values.get("useSystemSeparators").and_then(Json::as_bool).unwrap_or(true);
    let mut decimal = text(&d.values, "decimalSeparator");
    let mut thousands = text(&d.values, "thousandsSeparator");

    let mut languages = vec![("system".to_string(), l.text("ui-options-system-language", &[]).into_owned())];
    languages.extend(gridcraft_locale::LANGUAGES.iter().map(|lang| (lang.tag.to_string(), lang.native_name.to_string())));
    let mut regions = vec![("system".to_string(), l.text("ui-options-system-region", &[]).into_owned())];
    regions.extend(gridcraft_locale::REGIONS.iter().map(|r| (r.tag.to_string(), r.native_name.to_string())));

    egui::Grid::new("options_language").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
        ui.label(&*l.text("ui-options-interface", &[]));
        choice(ui, "options_ui_language", &mut ui_language, &languages);
        ui.end_row();

        ui.label(&*l.text("ui-options-formulas", &[]));
        ui.vertical(|ui| {
            ui.radio_value(&mut formula_language, "followUi".to_string(), &*l.text("ui-options-formulas-follow", &[]));
            ui.radio_value(&mut formula_language, "en-US".to_string(), &*l.text("ui-options-formulas-english", &[]));
        });
        ui.end_row();

        ui.label(&*l.text("ui-options-region", &[]));
        choice(ui, "options_region", &mut region, &regions);
        ui.end_row();

        ui.label("");
        ui.checkbox(&mut system, &*l.text("ui-options-system-separators", &[]));
        ui.end_row();

        ui.label(&*l.text("ui-options-decimal", &[]));
        ui.add_enabled(!system, egui::TextEdit::singleline(&mut decimal).desired_width(36.0));
        ui.end_row();

        ui.label(&*l.text("ui-options-thousands", &[]));
        ui.add_enabled(!system, egui::TextEdit::singleline(&mut thousands).desired_width(36.0));
        ui.end_row();
    });
    ui.label(egui::RichText::new(l.text("ui-options-separator-hint", &[])).weak().small());

    // Typing replaces the one character a separator is made of.
    decimal = single_char(&text(&d.values, "decimalSeparator"), &decimal);
    thousands = single_char(&text(&d.values, "thousandsSeparator"), &thousands);
    // While the region's own separators are in use, the fields show the chosen region's.
    if let Some((d1, t1)) = region_separators(&d.values, &region, system) {
        decimal = d1;
        thousands = t1;
    }
    d.values.insert("uiLanguage".into(), json!(ui_language));
    d.values.insert("formulaLanguage".into(), json!(formula_language));
    d.values.insert("regionalFormat".into(), json!(region));
    d.values.insert("useSystemSeparators".into(), json!(system));
    d.values.insert("decimalSeparator".into(), json!(decimal));
    d.values.insert("thousandsSeparator".into(), json!(thousands));
}

/// Applies the page through `app.setLocale`. Custom separators are sent only when they are in use.
pub fn apply(app: &mut SheetApp, d: &Dialog) -> Result<(), String> {
    let v = &d.values;
    let mut params = json!({
        "uiLanguage": text(v, "uiLanguage"),
        "formulaLanguage": text(v, "formulaLanguage"),
        "regionalFormat": text(v, "regionalFormat"),
    });
    let system = v.get("useSystemSeparators").and_then(Json::as_bool).unwrap_or(true);
    params["useSystemSeparators"] = json!(system);
    if !system {
        let (decimal, thousands) = (text(v, "decimalSeparator"), text(v, "thousandsSeparator"));
        if let Err(key) = check_separators(&decimal, &thousands) {
            return Err(app.l10n.text(key, &[]).into_owned());
        }
        params["decimalSeparator"] = json!(decimal);
        params["thousandsSeparator"] = json!(thousands);
    }
    app.run_typed("app.setLocale", params).map(|_| ()).map_err(|e| app.error_text(&e))
}

/// Characters a custom decimal or thousands separator may be: the ones real regions use
/// (period, comma, space, no-break spaces, apostrophes).
const SEPARATOR_CHARS: [char; 7] = ['.', ',', ' ', '\u{00A0}', '\u{202F}', '\'', '\u{2019}'];

/// Checks a custom separator pair the way the engine does; the error is the message key of the
/// reason, so the dialog can say it in the interface language.
fn check_separators(decimal: &str, thousands: &str) -> Result<(), &'static str> {
    let single = |s: &str| {
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    };
    let (Some(dec), Some(group)) = (single(decimal), single(thousands)) else {
        return Err("ui-options-error-separator-length");
    };
    if !SEPARATOR_CHARS.contains(&dec) || !SEPARATOR_CHARS.contains(&group) {
        return Err("ui-options-error-separator-chars");
    }
    if dec.is_whitespace() {
        return Err("ui-options-error-decimal-space");
    }
    if dec == group {
        return Err("ui-options-error-separators-equal");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::{check_separators, region_separators, single_char};

    fn values(region: &str, system: bool) -> Map<String, serde_json::Value> {
        let json = json!({"regionalFormat": region, "useSystemSeparators": system, "systemRegion": "de-DE"});
        json.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn choosing_system_takes_the_separators_of_the_system_region() {
        // Stored region en-US, system region de-DE: picking "System" shows `,` and `.`.
        assert_eq!(region_separators(&values("en-US", true), "system", true), Some((",".into(), ".".into())));
        // A concrete region shows its own.
        assert_eq!(region_separators(&values("de-DE", true), "en-US", true), Some((".".into(), ",".into())));
        // Ticking the box again refills from the region in effect.
        assert_eq!(region_separators(&values("system", false), "system", true), Some((",".into(), ".".into())));
        // Nothing changed, or custom separators in use: the fields are left alone.
        assert_eq!(region_separators(&values("system", true), "system", true), None);
        assert_eq!(region_separators(&values("en-US", true), "system", false), None);
    }

    #[test]
    fn custom_separators_are_checked_with_a_reason() {
        assert_eq!(check_separators(",", "."), Ok(()));
        assert_eq!(check_separators(",", "\u{00A0}"), Ok(()));
        assert_eq!(check_separators("", "."), Err("ui-options-error-separator-length"));
        assert_eq!(check_separators(",,", "."), Err("ui-options-error-separator-length"));
        assert_eq!(check_separators("x", "."), Err("ui-options-error-separator-chars"));
        assert_eq!(check_separators(" ", "."), Err("ui-options-error-decimal-space"));
        assert_eq!(check_separators(",", ","), Err("ui-options-error-separators-equal"));
    }

    #[test]
    fn typing_replaces_the_single_separator_character() {
        assert_eq!(single_char(".", ".,"), ",");
        assert_eq!(single_char(".", ",."), ",");
        assert_eq!(single_char(".", ""), "");
        assert_eq!(single_char(".", ","), ",");
        assert_eq!(single_char("", "ab"), "b");
        assert_eq!(single_char(".", ".."), ".");
    }
}
