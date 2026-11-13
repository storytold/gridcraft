//! Formula-based actuarial report dialog, populated from the function registry.
use super::ref_edit::{RefLabel, ref_edit};
use super::{Dialog, Json, SheetApp, json};

/// Returns the available actuarial functions and their published signatures and descriptions.
fn functions() -> &'static [Json] {
    static LIST: std::sync::OnceLock<Vec<Json>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        gridcraft_engine::cmd::formulas::function_list().into_iter().filter(|f| f["name"].as_str().is_some_and(|n| n.starts_with("ACT."))).collect()
    })
}

/// Whether the application includes the actuarial function pack.
pub(crate) fn available() -> bool {
    !functions().is_empty()
}

/// Argument labels in the public signature, preserving optional arguments and their defaults.
fn labels(signature: &str) -> Vec<String> {
    signature
        .split_once('(')
        .and_then(|(_, rest)| rest.rsplit_once(')'))
        .map(|(args, _)| args.split(',').map(str::trim).filter(|a| !a.is_empty()).take(32).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Draws a searchable function selector, argument reference pickers and a report action.
pub(super) fn form(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    ui.label("Select an actuarial function and enter a cell, range or literal for each argument.");
    ui.horizontal(|ui| {
        let label = ui.label("Search:");
        ui.text_edit_singleline(&mut d.search).labelled_by(label.id);
    });
    let selected = d.values.get("function").and_then(Json::as_str).unwrap_or("ACT.ANNUITY").to_string();
    let mut name = selected.clone();
    let query = d.search.trim().to_ascii_lowercase();
    let function_label = ui.label("Actuarial function:");
    craft_ui::ComboBox::from_id_salt("actuarialFunction")
        .selected_text(&name)
        .width(350.0)
        .show_ui(ui, |ui| {
            for f in functions().iter().filter(|f| {
                query.is_empty()
                    || f["name"].as_str().unwrap_or("").to_ascii_lowercase().contains(&query)
                    || f["description"].as_str().unwrap_or("").to_ascii_lowercase().contains(&query)
            }) {
                if let Some(n) = f["name"].as_str() {
                    ui.selectable_value(&mut name, n.to_string(), n);
                }
            }
        })
        .response
        .labelled_by(function_label.id);
    if name != selected {
        d.values.retain(|key, _| !key.starts_with("argument"));
        d.refs.target = None;
        d.refs.collapsed = None;
    }
    d.values.insert("function".into(), json!(name));
    if let Some(f) = functions().iter().find(|f| f["name"].as_str() == Some(name.as_str())) {
        let signature = f["signature"].as_str().unwrap_or("");
        ui.label(egui::RichText::new(signature).strong());
        ui.label(f["description"].as_str().unwrap_or(""));
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (i, label) in labels(signature).iter().enumerate() {
                let key = format!("argument{i}");
                ui.horizontal(|ui| {
                    let l = ui.label(format!("{label}:"));
                    ref_edit(app, ui, d, &key, RefLabel::By(l.id), 260.0);
                });
            }
        });
    } else {
        ui.label("No actuarial functions are available in this build.");
    }
    ui.label("Values remain linked to the source cells. For text, use quotes, e.g. \"poisson\".");
    ui.label("The report can be used as an output in Data → Monte Carlo.");
    super::show::ok_cancel(ui, confirm, open);
}

/// Builds ordered arguments without losing skipped optional positions.
pub(super) fn params(d: &Dialog) -> Json {
    let name = d.values.get("function").and_then(Json::as_str).unwrap_or("");
    let count =
        functions().iter().find(|f| f["name"].as_str() == Some(name)).map(|f| labels(f["signature"].as_str().unwrap_or("")).len()).unwrap_or(0);
    let mut args = (0..count).map(|i| d.values.get(&format!("argument{i}")).and_then(Json::as_str).unwrap_or("").to_string()).collect::<Vec<_>>();
    while args.last().is_some_and(|s| s.trim().is_empty()) {
        args.pop();
    }
    json!({"function":name,"arguments":args})
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All registered actuarial functions are selectable and retain their signatures.
    #[test]
    fn actuarial_selector_covers_the_registered_pack() {
        let all = functions();
        // The shared UI also builds in Monte Carlo without the actuarial pack.
        if all.is_empty() {
            return;
        }
        assert!(all.len() >= 95);
        for f in all {
            assert!(!labels(f["signature"].as_str().unwrap()).is_empty(), "{f}");
        }
    }

    /// Optional blanks retain their positions rather than shifting subsequent values.
    #[test]
    fn actuarial_parameters_preserve_optional_positions() {
        if !available() {
            return;
        }
        let d = Dialog::custom(
            "actuarial",
            "Actuarial Report",
            json!({"function":"ACT.ANNUITY","argument0":"A1:A3","argument1":"0.05","argument2":"","argument3":"FALSE"}),
        );
        assert_eq!(params(&d)["arguments"], json!(["A1:A3", "0.05", "", "FALSE"]));
    }
}
