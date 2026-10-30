//! Thin editor for the engine's generic Monte Carlo command.
use super::{Dialog, Json, Map, json};

/// Starts a uniform input without preloading parameters belonging to another distribution.
/// The editor supplies those defaults only when that distribution is selected.
pub(super) fn input_template(cell: &str) -> Json {
    json!({"cell":cell,"distribution":"uniform","min":"0","max":"1"})
}

/// Edits a labeled textual parameter while retaining typed numeric values until an actual edit.
/// Engine validation receives invalid text unchanged instead of silently substituting a default.
fn text(ui: &mut egui::Ui, values: &mut Map<String, Json>, key: &str, label: &str) {
    ui.horizontal(|ui| {
        let label = ui.label(label);
        let mut value = values
            .get(key)
            .map(|v| match v {
                Json::String(s) => s.clone(),
                _ => v.to_string(),
            })
            .unwrap_or_default();
        if ui.add(egui::TextEdit::singleline(&mut value).desired_width(110.0)).labelled_by(label.id).changed() {
            values.insert(key.into(), json!(value));
        }
    });
}

/// Names distributions without presenting log-space parameters as arithmetic moments.
fn distribution_label(kind: &str) -> &str {
    match kind {
        "uniform" => "Uniform",
        "triangular" => "Triangular",
        "normal" => "Normal",
        "lognormal" => "Lognormal (arithmetic mean / CV)",
        "poisson" => "Poisson count",
        "binomial" => "Binomial count",
        "hypergeometric" => "Hypergeometric count",
        _ => kind,
    }
}

/// Lists scalar parameters and defaults; existing values survive distribution changes.
fn distribution_fields(kind: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    match kind {
        "normal" => &[("mean", "Mean:", "0"), ("stdDev", "Standard deviation:", "1")],
        "lognormal" => &[("mean", "Arithmetic mean:", "1"), ("cv", "CV (SD / mean):", "0.1")],
        "poisson" => &[("mean", "Mean events:", "10")],
        "binomial" => &[("trials", "Trials (integer):", "10"), ("probability", "Success probability:", "0.5")],
        "hypergeometric" => {
            &[("draws", "Draws (integer):", "10"), ("successes", "Population successes:", "20"), ("population", "Population size:", "100")]
        }
        "triangular" => &[("min", "Minimum:", "0"), ("mode", "Mode:", "0.5"), ("max", "Maximum:", "1")],
        _ => &[("min", "Minimum:", "0"), ("max", "Maximum:", "1")],
    }
}

/// Renders distribution-specific controls without replacing invalid user-entered values.
fn input_parameters(ui: &mut egui::Ui, values: &mut Map<String, Json>) {
    let fields = distribution_fields(values.get("distribution").and_then(Json::as_str).unwrap_or("uniform"));
    ui.horizontal_wrapped(|ui| {
        for &(key, label, default) in fields {
            values.entry(key.to_string()).or_insert_with(|| json!(default));
            text(ui, values, key, label);
        }
    });
}

/// Draws the bounded input editor, sampling settings, matrix and outputs without evaluating
/// formulas. Replica settings apply only to LHS; confirmation delegates validation to the engine.
pub(super) fn form(ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    ui.label(
        "Choose numeric input cells and formula outputs. Source values are kept unchanged; optional correlation is on latent Gaussian variables.",
    );
    ui.separator();
    let mut inputs = d.values.get("inputs").and_then(Json::as_array).cloned().unwrap_or_default();
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        for (index, item) in inputs.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.group(|ui| {
                    if let Some(values) = item.as_object_mut() {
                        ui.horizontal(|ui| {
                            text(ui, values, "cell", "Cell:");
                            let mut kind = values.get("distribution").and_then(Json::as_str).unwrap_or("uniform").to_string();
                            let label = ui.label("Distribution:");
                            craft_ui::ComboBox::from_id_salt("distribution")
                                .selected_text(distribution_label(&kind))
                                .show_ui(ui, |ui| {
                                    for k in ["uniform", "triangular", "normal", "lognormal", "poisson", "binomial", "hypergeometric"] {
                                        ui.selectable_value(&mut kind, k.to_string(), distribution_label(k));
                                    }
                                })
                                .response
                                .labelled_by(label.id);
                            values.insert("distribution".into(), json!(kind));
                            if ui.button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                        input_parameters(ui, values);
                    }
                });
            });
        }
    });
    if let Some(i) = remove {
        inputs.remove(i);
    }
    if inputs.len() < 32 && ui.button("Add input").clicked() {
        inputs.push(input_template(""));
    }
    d.values.insert("inputs".into(), json!(inputs));
    ui.label("For conditional counts and monthly AR1 processes, use MC.* worksheet functions with IF(MC.ACTIVE(), sample, baseline).");
    ui.separator();
    ui.horizontal(|ui| {
        let label = ui.label("Sampling:");
        let mut method = d.values.get("method").and_then(Json::as_str).unwrap_or("random").to_string();
        let selected = if method == "latinHypercube" { "Latin Hypercube" } else { "Random Monte Carlo" };
        craft_ui::ComboBox::from_id_salt("mcMethod")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut method, "random".into(), "Random Monte Carlo");
                ui.selectable_value(&mut method, "latinHypercube".into(), "Latin Hypercube");
            })
            .response
            .labelled_by(label.id);
        if method == "latinHypercube" {
            text(ui, &mut d.values, "replicates", "Independent replicas:");
        }
        d.values.insert("method".into(), json!(method));
    });
    let matrix_label = ui.label("Correlation matrix in input order (blank = independent). Rows on separate lines, numbers separated by spaces. Gaussian latent correlations, not marginal Pearson targets.");
    let mut matrix = match d.values.get("correlation") {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Array(rows)) => rows
            .iter()
            .filter_map(Json::as_array)
            .map(|row| row.iter().map(Json::to_string).collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let matrix_response = ui.add(egui::TextEdit::multiline(&mut matrix).desired_rows(2).desired_width(510.0)).labelled_by(matrix_label.id);
    let mut multiline_focused = matrix_response.has_focus();
    if matrix_response.changed() {
        d.values.insert("correlation".into(), json!(matrix));
    }
    let outputs_label = ui.label("Output cells, one per line (e.g. Model!F30). Qualify references to other sheets.");
    let mut outputs = d.values.get("outputs").and_then(Json::as_str).unwrap_or("").to_string();
    let outputs_response = ui.add(egui::TextEdit::multiline(&mut outputs).desired_rows(2).desired_width(510.0)).labelled_by(outputs_label.id);
    multiline_focused |= outputs_response.has_focus();
    if outputs_response.changed() {
        d.values.insert("outputs".into(), json!(outputs));
    }
    ui.horizontal(|ui| {
        text(ui, &mut d.values, "iterations", "Iterations:");
        text(ui, &mut d.values, "seed", "Seed:");
    });
    let mut samples = d.values.get("samples").and_then(Json::as_bool).unwrap_or(true);
    if ui.checkbox(&mut samples, "Include samples in the report").changed() {
        d.values.insert("samples".into(), json!(samples));
    }
    ui.label("Creates a static report with percentiles and a Monte Carlo tornado for the first output. Re-run after changing the model. Large models may take time.");
    super::show::ok_cancel_with_enter(ui, confirm, open, !multiline_focused);
}

/// Invalid typed numbers stay strings so the engine rejects them instead of silently defaulting.
/// Replica counts are sent only for LHS, allowing arbitrary valid random iteration counts;
/// output references stay line-delimited and correlation rows preserve invalid numeric tokens.
pub(super) fn params(d: &Dialog) -> Json {
    let mut values = d.values.clone();
    if values.get("method").and_then(Json::as_str) != Some("latinHypercube") {
        values.remove("replicates");
    }
    for key in ["iterations", "seed", "replicates"] {
        if let Some(n) = values.get(key).and_then(Json::as_str).and_then(|s| s.trim().parse::<u64>().ok()) {
            values.insert(key.into(), json!(n));
        }
    }
    if let Some(inputs) = values.get_mut("inputs").and_then(Json::as_array_mut) {
        for input in inputs {
            if let Some(input) = input.as_object_mut() {
                for key in ["min", "mode", "max", "mean", "stdDev", "cv", "trials", "probability", "draws", "successes", "population"] {
                    if let Some(n) = input.get(key).and_then(Json::as_str).and_then(|s| s.trim().parse::<f64>().ok()).filter(|n| n.is_finite()) {
                        input.insert(key.into(), json!(n));
                    }
                }
            }
        }
    }
    if let Some(outputs) = values.get("outputs").and_then(Json::as_str) {
        let outputs = outputs.lines().map(str::trim).filter(|s| !s.is_empty()).map(|s| json!(s)).collect::<Vec<_>>();
        values.insert("outputs".into(), json!(outputs));
    }
    if let Some(text) = values.get("correlation").and_then(Json::as_str) {
        if text.trim().is_empty() {
            values.remove("correlation");
        } else {
            let matrix = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    Json::Array(
                        line.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                            .filter(|s| !s.is_empty())
                            .map(|v| v.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| json!(n)).unwrap_or_else(|| json!(v)))
                            .collect(),
                    )
                })
                .collect::<Vec<_>>();
            values.insert("correlation".into(), json!(matrix));
        }
    }
    Json::Object(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adding an input and then selecting a distribution supplies its own untouched defaults.
    #[test]
    fn monte_carlo_added_inputs_use_distribution_defaults_without_overwriting_edits() {
        use egui_kittest::kittest::Queryable as _;
        for (kind, mean) in [("lognormal", 1.0), ("poisson", 10.0)] {
            let d = Dialog::custom("monteCarlo", "Monte Carlo", json!({"inputs":[input_template("A1")],"method":"random"}));
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1000.0, 1000.0))
                .build_ui_state(|ui, d: &mut Dialog| form(ui, d, &mut false, &mut true), d);
            h.run_steps(2);
            h.get_by_label("Add input").click();
            h.run_steps(2);
            let inputs = h.state_mut().values.get_mut("inputs").unwrap().as_array_mut().unwrap();
            assert_eq!(inputs.len(), 2);
            for input in inputs {
                assert!(input.get("mean").is_none());
                input["distribution"] = json!(kind);
            }
            h.run_steps(2);
            let p = params(h.state());
            assert_eq!(p["inputs"][0]["mean"], mean);
            assert_eq!(p["inputs"][1]["mean"], mean);
            h.state_mut().values["inputs"][1]["mean"] = json!("0");
            h.run_steps(2);
            assert_eq!(params(h.state())["inputs"][1]["mean"], 0.0);
        }
    }

    /// Every added distribution reaches the engine with numeric parameters, including whitespace.
    #[test]
    fn monte_carlo_dialog_serializes_discrete_and_lognormal_parameters() {
        let d = Dialog::custom(
            "monteCarlo",
            "Monte Carlo",
            json!({"inputs":[
                {"cell":"A1","distribution":"lognormal","mean":" 120 ","cv":"0.25"},
                {"cell":"A2","distribution":"poisson","mean":"12.5"},
                {"cell":"A3","distribution":"binomial","trials":" 40 ","probability":"0.3"},
                {"cell":"A4","distribution":"hypergeometric","draws":"10","successes":"20","population":"100"}
            ]}),
        );
        let p = params(&d);
        assert_eq!(p["inputs"][0]["distribution"], "lognormal");
        assert_eq!(p["inputs"][0]["mean"], 120.0);
        assert_eq!(p["inputs"][0]["cv"], 0.25);
        assert_eq!(p["inputs"][1]["mean"], 12.5);
        assert_eq!(p["inputs"][2]["trials"], 40.0);
        assert_eq!(p["inputs"][2]["probability"], 0.3);
        assert_eq!(p["inputs"][3]["draws"], 10.0);
        assert_eq!(p["inputs"][3]["successes"], 20.0);
        assert_eq!(p["inputs"][3]["population"], 100.0);
    }

    /// Fractions are never truncated into valid counts; invalid text and nonfinite values remain visible to validation.
    #[test]
    fn monte_carlo_dialog_preserves_invalid_new_distribution_parameters() {
        let d = Dialog::custom(
            "monteCarlo",
            "Monte Carlo",
            json!({"inputs":[
                {"distribution":"binomial","trials":"3.5","probability":"bad"},
                {"distribution":"hypergeometric","draws":"1.25","successes":"NaN","population":"inf"},
                {"distribution":"lognormal","mean":"-1","cv":"overflow"}
            ]}),
        );
        let p = params(&d);
        assert_eq!(p["inputs"][0]["trials"], 3.5);
        assert_eq!(p["inputs"][0]["probability"], "bad");
        assert_eq!(p["inputs"][1]["draws"], 1.25);
        assert_eq!(p["inputs"][1]["successes"], "NaN");
        assert_eq!(p["inputs"][1]["population"], "inf");
        assert_eq!(p["inputs"][2]["mean"], -1.0);
        assert_eq!(p["inputs"][2]["cv"], "overflow");
    }

    /// Invalid numeric text must survive UI conversion so the engine can report it to the user.
    #[test]
    fn monte_carlo_dialog_builds_typed_params_without_hiding_invalid_input() {
        let d = Dialog::custom(
            "monteCarlo",
            "Monte Carlo",
            json!({"iterations":"2000","seed":"42","inputs":[{"cell":"B2","distribution":"normal","mean":"10","stdDev":"bad"}],"outputs":"Sheet1!C1\n'Other, sheet'!B2\n"}),
        );
        let p = params(&d);
        assert_eq!(p["iterations"], 2000);
        assert_eq!(p["seed"], 42);
        assert_eq!(p["inputs"][0]["mean"], 10.0);
        assert_eq!(p["inputs"][0]["stdDev"], "bad");
        assert_eq!(p["outputs"].as_array().map(Vec::len), Some(2));
    }

    /// Switching a dialog from LHS to random must remove the stale replica restriction.
    /// Exercise the resulting command as well as the serialized parameters for odd trial counts.
    #[test]
    fn monte_carlo_random_dialog_accepts_iterations_not_divisible_by_lhs_replicates() {
        for iterations in [1001, 1500] {
            let mut d = Dialog::custom(
                "monteCarlo",
                "Monte Carlo",
                json!({"method":"latinHypercube","replicates":"8","iterations":iterations.to_string(),"inputs":[{"cell":"A1","min":"0","max":"1"}],"outputs":"B1","samples":false}),
            );
            let lhs = params(&d);
            assert_eq!(lhs["replicates"], 8);
            d.values.insert("method".into(), json!("random"));
            let p = params(&d);
            assert!(p.get("replicates").is_none());
            let mut session = gridcraft_engine::Session::new();
            session.new_workbook();
            session.execute("cell.set", json!({"cell":"A1","input":"0.5"})).unwrap();
            session.execute("cell.set", json!({"cell":"B1","input":"=A1*2"})).unwrap();
            let result = session.execute("data.monteCarlo", p).unwrap();
            assert_eq!(result["iterations"], iterations);
            assert_eq!(result["replicates"], 1);
        }
    }
}
