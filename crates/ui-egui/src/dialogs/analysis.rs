use super::ref_edit::{RefLabel, SHEET_DELETED, end_pick, home_sheet_deleted, ref_edit};
use super::show::{build_params, form_fields, ok_cancel};
use super::{Dialog, Field, Json, Map, SheetApp, json, vec2};

pub(super) fn goal_seek(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    egui::Grid::new("gs").show(ui, |ui| {
        let l = ui.label("Set cell:");
        ref_edit(app, ui, d, "set", RefLabel::By(l.id), 200.0);
        ui.end_row();
        let l = ui.label("To value:");
        let mut to = d.values.get("to").and_then(Json::as_str).unwrap_or("").to_string();
        if ui.add(egui::TextEdit::singleline(&mut to).desired_width(170.0)).labelled_by(l.id).changed() {
            d.values.insert("to".into(), json!(to));
        }
        ui.end_row();
        let l = ui.label("By changing cell:");
        ref_edit(app, ui, d, "changing", RefLabel::By(l.id), 200.0);
        ui.end_row();
    });
    let get = |k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let (set, to, changing) = (get("set"), get("to"), get("changing"));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm && home_sheet_deleted(app, d) {
        app.message = Some((d.title.clone(), SHEET_DELETED.into()));
        *confirm = false;
    }
    if *confirm && let Err(e) = end_pick(app, d) {
        // The dialog's sheet can't be shown again (a draft on the picked sheet could not be
        // saved): the cells would be read against the wrong sheet, so Goal Seek doesn't run.
        app.message = Some((d.title.clone(), crate::clean_error(&e)));
        *confirm = false;
    }
    if *confirm {
        match app.run("data.goalSeek", json!({"set": set, "to": to.parse::<f64>().unwrap_or(0.0), "changing": changing})) {
            Ok(r) => app.toast = Some((format!("Goal Seeking found a solution: {}", r["value"]), crate::now_ms())),
            Err(e) => app.message = Some(("Goal Seek".into(), crate::clean_error(&e))),
        }
    }
    if !open {
        *confirm = true;
    }
}

/// The fields of one Analysis ToolPak tool's form (input and options; output is drawn apart).
fn toolpak_fields(tool: &str) -> Vec<Field> {
    use Field::*;
    let grouped = || Choice { key: "groupedBy", label: "Grouped By:", options: vec![("columns", "Columns"), ("rows", "Rows")] };
    let labels = || Check { key: "labels", label: "Labels in first row" };
    let range = || Ref { key: "range", label: "Input Range:" };
    let two = || vec![Ref { key: "range1", label: "Variable 1 Range:" }, Ref { key: "range2", label: "Variable 2 Range:" }];
    match tool {
        "descriptive" => vec![
            range(),
            grouped(),
            labels(),
            Check { key: "summary", label: "Summary statistics" },
            Text { key: "confidence", label: "Confidence Level for Mean (%), blank for none:" },
            Text { key: "kthLargest", label: "Kth Largest (blank for none):" },
            Text { key: "kthSmallest", label: "Kth Smallest (blank for none):" },
        ],
        "histogram" => vec![
            range(),
            Ref { key: "bins", label: "Bin Range (blank for automatic bins):" },
            labels(),
            Check { key: "pareto", label: "Pareto (sorted histogram)" },
            Check { key: "cumulative", label: "Cumulative Percentage" },
        ],
        "correlation" | "covariance" | "rankPercentile" => vec![range(), grouped(), labels()],
        "anovaSingle" => vec![range(), grouped(), labels(), Text { key: "alpha", label: "Alpha:" }],
        "regression" => vec![
            Ref { key: "inputY", label: "Input Y Range:" },
            Ref { key: "inputX", label: "Input X Range:" },
            labels(),
            Check { key: "constantZero", label: "Constant is Zero" },
            Text { key: "confidence", label: "Confidence Level (%):" },
            Check { key: "residuals", label: "Residuals" },
            Check { key: "standardizedResiduals", label: "Standardized Residuals" },
            Check { key: "normalProbability", label: "Normal Probability" },
        ],
        "movingAverage" => {
            vec![range(), labels(), Text { key: "interval", label: "Interval:" }, Check { key: "standardErrors", label: "Standard Errors" }]
        }
        "exponentialSmoothing" => {
            vec![range(), labels(), Text { key: "damping", label: "Damping factor:" }, Check { key: "standardErrors", label: "Standard Errors" }]
        }
        "fTest" => {
            let mut v = two();
            v.extend([labels(), Text { key: "alpha", label: "Alpha:" }]);
            v
        }
        _ => {
            let mut v = two();
            v.extend([
                Text { key: "hypothesizedDifference", label: "Hypothesized Mean Difference:" },
                labels(),
                Text { key: "alpha", label: "Alpha:" },
            ]);
            v
        }
    }
}

/// The starting values of a tool's form: the selection as its input, a new sheet as output.
pub(super) fn toolpak_defaults(sel: &str, tool: &str) -> Map<String, Json> {
    let mut v = json!({
        "tool": tool, "range": sel, "inputY": "", "inputX": "", "range1": sel, "range2": "", "bins": "",
        "groupedBy": "columns", "labels": false, "summary": true, "confidence": "", "kthLargest": "", "kthSmallest": "",
        "interval": "3", "damping": "0.3", "alpha": "0.05", "hypothesizedDifference": "0",
        "outputTo": "newSheet", "output": "", "newSheet": "",
    });
    if tool == "regression" {
        v["confidence"] = json!("95");
        v["inputY"] = json!(sel);
    }
    v.as_object().cloned().unwrap_or_default()
}

/// Data › Data Analysis: pick a tool, then fill in its form. OK runs `data.analysisToolPak`
/// (through the dialog's command); a report that would replace data asks first.
pub(super) fn analysis_toolpak(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    use gridcraft_engine::cmd::analysis::TOOLS;
    if d.values.get("askOverwrite").and_then(Json::as_bool) == Some(true) {
        ui.label("The output range already holds data. Replace it with the report?");
        ui.horizontal(|ui| {
            if ui.button("Replace").clicked() {
                d.values.remove("askOverwrite");
                d.values.insert("overwrite".into(), json!(true));
                *confirm = true;
            }
            if ui.button("Cancel").clicked() {
                d.values.remove("askOverwrite");
            }
        });
        return;
    }
    let tool = d.values.get("tool").and_then(Json::as_str).unwrap_or("").to_string();
    let Some((_, label)) = TOOLS.iter().find(|(id, _)| *id == tool) else {
        ui.label(egui::RichText::new("Analysis Tools").strong());
        let pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
        let mut chosen = false;
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for (id, label) in TOOLS {
                let r = ui.selectable_label(pick == *id, *label);
                if r.clicked() {
                    d.values.insert("pick".into(), json!(id));
                }
                if r.double_clicked() {
                    chosen = true;
                }
            }
        });
        let mut ok = false;
        ok_cancel(ui, &mut ok, open);
        let pick = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
        if (ok || chosen) && TOOLS.iter().any(|(id, _)| *id == pick) {
            let sel = app.session.active().map(|doc| doc.selection.current().a1()).unwrap_or_default();
            d.values = toolpak_defaults(&sel, &pick);
        }
        return;
    };
    d.title = label.to_string();
    d.fields = toolpak_fields(&tool);
    form_fields(app, ui, d);
    ui.separator();
    ui.label(egui::RichText::new("Output options").strong());
    let to = d.values.get("outputTo").and_then(Json::as_str).unwrap_or("newSheet").to_string();
    for (key, choice, text_label) in [("output", "range", "Output Range:"), ("newSheet", "newSheet", "New Worksheet Ply:")] {
        ui.horizontal(|ui| {
            let r = ui.radio(to == choice, text_label);
            if r.clicked() {
                d.values.insert("outputTo".into(), json!(choice));
                // Another output option ends picking into Output Range, so a later selection on
                // the grid doesn't choose it again.
                if key != "output" && d.refs.target.as_deref() == Some("output") {
                    d.refs.target = None;
                }
            }
            if key == "output" {
                let before = d.values.get(key).cloned();
                let was_target = d.refs.target.as_deref() == Some(key);
                ref_edit(app, ui, d, key, RefLabel::By(r.id), 190.0);
                // Clicking into the box, typing or picking an output range chooses it; only a
                // change does, so an option chosen later stays chosen.
                if d.refs.target.as_deref() == Some(key) && (!was_target || d.values.get(key) != before.as_ref()) {
                    d.values.insert("outputTo".into(), json!(choice));
                }
            } else {
                let mut t = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
                if ui.add(egui::TextEdit::singleline(&mut t).desired_width(160.0)).changed() {
                    d.values.insert(key.into(), json!(t));
                    d.values.insert("outputTo".into(), json!(choice));
                }
            }
        });
    }
    ok_cancel(ui, confirm, open);
}

/// The Solver Options fields: (key, label, default).
const SOLVER_OPTIONS: [(&str, &str, &str); 7] = [
    ("maxEvaluations", "Max Evaluations:", "20000"),
    ("integerOptimality", "Integer Optimality (%):", "0"),
    ("convergence", "Convergence:", "0.0001"),
    ("mutationRate", "Mutation Rate:", "0.075"),
    ("populationSize", "Population Size:", "100"),
    ("randomSeed", "Random Seed:", "0"),
    ("generationsWithoutImprovement", "Maximum Generations without Improvement:", "50"),
];

/// Data › Solver › Options: the Solver Options in the Solver Parameters window. OK keeps the
/// changes for the next solve; Cancel restores them.
fn solver_options(ui: &mut egui::Ui, d: &mut Dialog, edit: impl Fn(&mut egui::Ui, &mut Dialog, &str, f32, &str)) {
    let mut done = false;
    egui::Grid::new("solverOptions").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        for (title, keys) in [("All Methods", &SOLVER_OPTIONS[..2]), ("Evolutionary", &SOLVER_OPTIONS[2..])] {
            ui.label(egui::RichText::new(title).strong());
            ui.end_row();
            for (key, label, _) in keys {
                ui.label(*label);
                edit(ui, d, key, 100.0, label);
                ui.end_row();
            }
        }
    });
    let mut bounds = d.values.get("requireBounds").and_then(Json::as_bool).unwrap_or(true);
    if ui.checkbox(&mut bounds, "Require Bounds on Variables").changed() {
        d.values.insert("requireBounds".into(), json!(bounds));
    }
    ui.add_space(8.0);
    // A row as tall as the buttons (see `ok_cancel`): a right-to-left layout straight in the
    // window takes all the height it is offered.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(" Cancel ").clicked() {
                cancel_solver_options(d);
            }
            if ui.button("  OK  ").clicked() {
                done = true;
            }
        });
    });
    if done {
        close_solver_options(d);
    }
}

/// Whether the Solver dialog shows its Solver Options.
pub(super) fn in_solver_options(d: &Dialog) -> bool {
    d.name == "solver" && d.values.get("optionsView").and_then(Json::as_bool) == Some(true)
}

/// Solver Options › Cancel (also Escape or closing the window there): back to the parameters
/// with the options as they were.
pub(super) fn cancel_solver_options(d: &mut Dialog) {
    if let Some(old) = d.values.get("optionsBackup").and_then(Json::as_object).cloned() {
        for (k, v) in old {
            d.values.insert(k, v);
        }
    }
    close_solver_options(d);
}

/// Solver Parameters › Options (also opening the dialog with `optionsView`): shows the Solver
/// Options, each field with its value or its default, and keeps the values for Cancel.
pub(super) fn open_solver_options(d: &mut Dialog) {
    let mut backup = serde_json::Map::new();
    for (key, _, default) in SOLVER_OPTIONS {
        let v = d.values.get(key).cloned().unwrap_or_else(|| json!(default));
        backup.insert(key.into(), v.clone());
        d.values.insert(key.into(), v);
    }
    backup.insert("requireBounds".into(), d.values.get("requireBounds").cloned().unwrap_or(json!(true)));
    d.values.insert("optionsBackup".into(), Json::Object(backup));
    d.values.insert("optionsView".into(), json!(true));
    d.title = "Solver Options".into();
}

fn close_solver_options(d: &mut Dialog) {
    d.values.insert("optionsView".into(), json!(false));
    d.values.remove("optionsBackup");
    d.title = "Solver Parameters".into();
}

/// Removes the model's options (the Solver Options fields and Require Bounds) from the form. A
/// key that is absent means the option's default, so Reset All and Load Model remove them all
/// before a model of their own; an option of the model before would otherwise go with the next
/// solve.
fn clear_solver_options(d: &mut Dialog) {
    for (key, _, _) in SOLVER_OPTIONS {
        d.values.remove(key);
    }
    d.values.remove("requireBounds");
}

/// Save Model from the Solver Parameters form: writes the form's model down from `range`. A
/// range that holds data is replaced only with `overwrite`; without it the form asks first.
fn save_solver_model(app: &mut SheetApp, d: &mut Dialog, range: &str, overwrite: bool) {
    d.values.remove("askModelOverwrite");
    let note = match app.run("data.solverSaveModel", json!({"to": range.trim(), "model": build_params(d), "overwrite": overwrite})) {
        Ok(r) => format!("The model was saved to {}.", r.get("range").and_then(Json::as_str).unwrap_or("")),
        Err(e) if !overwrite && e.contains(gridcraft_engine::cmd::analysis::OVERWRITE_ERROR) => {
            d.values.insert("askModelOverwrite".into(), json!(range.trim()));
            crate::clean_error(&e)
        }
        Err(e) => crate::clean_error(&e),
    };
    d.values.insert("modelNote".into(), json!(note));
}

/// Data › Solver: the Solver Parameters form. Solve runs `data.solver` through the dialog's
/// command; the result opens Solver Results. Reset All clears the form and the sheet's model;
/// Load/Save reads or writes the model on cells of the sheet; Options shows the Solver Options
/// in the same window.
pub(super) fn solver(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let text = |d: &Dialog, k: &str| d.values.get(k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default();
    let edit = |ui: &mut egui::Ui, d: &mut Dialog, k: &str, width: f32, label: &str| {
        let mut t = text(d, k);
        if ui.add(egui::TextEdit::singleline(&mut t).desired_width(width)).accessible_name(lang.tr(label)).changed() {
            d.values.insert(k.into(), json!(t));
        }
    };
    if d.values.get("optionsView").and_then(Json::as_bool) == Some(true) {
        solver_options(ui, d, edit);
        return;
    }
    ui.horizontal(|ui| {
        let l = ui.label("Set Objective:");
        ref_edit(app, ui, d, "objective", RefLabel::By(l.id), 230.0);
    });
    let goal = text(d, "goal");
    ui.horizontal(|ui| {
        ui.label("To:");
        for (g, label) in [("max", "Max"), ("min", "Min"), ("value", "Value Of:")] {
            if ui.radio(goal == g, label).clicked() {
                d.values.insert("goal".into(), json!(g));
            }
        }
        edit(ui, d, "value", 80.0, "Value Of:");
    });
    let l = ui.label("By Changing Variable Cells:");
    ref_edit(app, ui, d, "variables", RefLabel::By(l.id), 420.0);
    ui.label("Subject to the Constraints:");
    let list: Vec<String> = d
        .values
        .get("constraints")
        .and_then(Json::as_array)
        .map(|a| a.iter().map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string())).collect())
        .unwrap_or_default();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_size(vec2(420.0, 90.0));
        egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
            for (i, c) in list.iter().enumerate() {
                if ui.selectable_label(d.list_index == i, c).clicked() {
                    d.list_index = i;
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ref_edit(app, ui, d, "newLhs", RefLabel::Named("Constraint Cell Reference"), 130.0);
        let op = text(d, "newOp");
        craft_ui::ComboBox::from_id_salt("solverOp")
            .selected_text(op.clone())
            .width(60.0)
            .show_ui(ui, |ui| {
                for o in ["<=", "=", ">=", "int", "bin"] {
                    if ui.selectable_label(op == o, o).clicked() {
                        d.values.insert("newOp".into(), json!(o));
                    }
                }
            })
            .response
            .accessible_name(lang.tr("Constraint Operator"));
        if !matches!(op.as_str(), "int" | "bin") {
            ref_edit(app, ui, d, "newRhs", RefLabel::Named("Constraint Value"), 130.0);
        }
        if ui.button("Add").clicked() {
            let lhs = text(d, "newLhs");
            if !lhs.trim().is_empty() {
                let rhs = if matches!(op.as_str(), "int" | "bin") { String::new() } else { format!(" {}", text(d, "newRhs").trim()) };
                let mut list = list.clone();
                list.push(format!("{} {op}{rhs}", lhs.trim()));
                d.values.insert("constraints".into(), json!(list));
                d.values.insert("newLhs".into(), json!(""));
                d.values.insert("newRhs".into(), json!(""));
            }
        }
        if ui.add_enabled(d.list_index < list.len(), egui::Button::new("Delete")).clicked() {
            let mut list = list.clone();
            if d.list_index < list.len() {
                list.remove(d.list_index);
            }
            d.values.insert("constraints".into(), json!(list));
        }
    });
    let mut nonneg = d.values.get("nonNegative").and_then(Json::as_bool).unwrap_or(true);
    if ui.checkbox(&mut nonneg, "Make Unconstrained Variables Non-Negative").changed() {
        d.values.insert("nonNegative".into(), json!(nonneg));
    }
    let method = text(d, "method");
    ui.horizontal(|ui| {
        ui.label("Select a Solving Method:");
        let label = |m: &str| match m {
            "simplex" => "Simplex LP",
            "evolutionary" => "Evolutionary",
            _ => "GRG Nonlinear",
        };
        craft_ui::ComboBox::from_id_salt("solverMethod")
            .selected_text(label(&method))
            .show_ui(ui, |ui| {
                for m in ["grg", "simplex", "evolutionary"] {
                    if ui.selectable_label(method == m, label(m)).clicked() {
                        d.values.insert("method".into(), json!(m));
                    }
                }
            })
            .response
            .accessible_name(lang.tr("Select a Solving Method:"));
    });
    ui.label(
        egui::RichText::new(match method.as_str() {
            "simplex" => "Simplex LP: for linear models; integer and binary constraints by branch and bound.",
            "evolutionary" => "Evolutionary: a genetic search for models that aren't smooth; every variable cell needs bounds.",
            _ => "GRG Nonlinear: a local optimum of a smooth model, from the current values.",
        })
        .small(),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.button("Reset All").clicked() {
            match app.run("data.solverReset", json!({})) {
                Ok(_) => {
                    for (k, v) in [
                        ("objective", json!("")),
                        ("goal", json!("max")),
                        ("value", json!("0")),
                        ("variables", json!("")),
                        ("constraints", json!([])),
                        ("nonNegative", json!(true)),
                        ("method", json!("grg")),
                    ] {
                        d.values.insert(k.into(), v);
                    }
                    // The options go back to their defaults too.
                    clear_solver_options(d);
                    for key in ["optionsBackup", "modelNote", "askModelOverwrite"] {
                        d.values.remove(key);
                    }
                }
                Err(e) => {
                    d.values.insert("modelNote".into(), json!(crate::clean_error(&e)));
                }
            }
        }
        let shown = d.values.get("modelArea").and_then(Json::as_bool).unwrap_or(false);
        if ui.selectable_label(shown, "Load/Save").clicked() {
            d.values.insert("modelArea".into(), json!(!shown));
            // Hiding the area ends picking into Model Range, so a later selection on the grid
            // doesn't go into a box that isn't shown.
            if shown && d.refs.target.as_deref() == Some("modelRange") {
                d.refs.target = None;
            }
        }
        if ui.button("Options").clicked() {
            open_solver_options(d);
        }
    });
    if d.values.get("modelArea").and_then(Json::as_bool) == Some(true) {
        ui.horizontal(|ui| {
            let l = ui.label("Model Range:");
            ref_edit(app, ui, d, "modelRange", RefLabel::By(l.id), 170.0);
            let range = text(d, "modelRange");
            let ready = !range.trim().is_empty();
            if ui.add_enabled(ready, egui::Button::new("Load")).clicked() {
                d.values.remove("askModelOverwrite");
                match app.run("data.solverLoadModel", json!({"from": range.trim()})) {
                    Ok(r) => {
                        if let Some(m) = r.get("model").and_then(Json::as_object) {
                            // The loaded model's options only: one it omits takes its default.
                            clear_solver_options(d);
                            for (k, x) in m {
                                d.values.insert(k.clone(), if k == "value" { json!(x.to_string()) } else { x.clone() });
                            }
                        }
                        d.values.insert("modelNote".into(), json!("The model was loaded."));
                    }
                    Err(e) => {
                        d.values.insert("modelNote".into(), json!(crate::clean_error(&e)));
                    }
                }
            }
            if ui.add_enabled(ready, egui::Button::new("Save")).clicked() {
                save_solver_model(app, d, &range, false);
            }
        });
        // Save Model asks before it replaces data in the model range; the question goes when
        // the range is edited, so Replace writes only where it asked about.
        let asked = d.values.get("askModelOverwrite").and_then(Json::as_str).map(str::to_string);
        if asked.as_deref().is_some_and(|a| a != text(d, "modelRange").trim()) {
            d.values.remove("askModelOverwrite");
            d.values.remove("modelNote");
        } else if let Some(asked) = asked {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{asked} already holds data. Replace it with the model?"));
                if ui.button("Replace").clicked() {
                    save_solver_model(app, d, &asked, true);
                }
                if ui.button("Don't Replace").clicked() {
                    d.values.remove("askModelOverwrite");
                    d.values.remove("modelNote");
                }
            });
        }
    }
    let note = text(d, "modelNote");
    if !note.is_empty() {
        ui.label(egui::RichText::new(note).small());
    }
    ui.add_space(8.0);
    // A row as tall as the buttons (see `ok_cancel`): the Load/Save area and the notes can make
    // the form shorter, and a right-to-left layout straight in the window would fill what is
    // left.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let t = crate::theme::Tokens::get(ui.ctx());
            let solve = ui.add(egui::Button::new(egui::RichText::new("  Solve  ").color(t.on_accent)).fill(t.accent));
            if solve.clicked() && !text(d, "objective").trim().is_empty() {
                *confirm = true;
            }
            if ui.button(" Close ").clicked() {
                *open = false;
            }
        });
    });
}

/// Solver Results: the outcome, and keeping or restoring the variable cells.
pub(super) fn solver_results(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, open: &mut bool) {
    let message = d.values.get("message").and_then(Json::as_str).unwrap_or("").to_string();
    ui.label(message);
    let changed = d.values.get("changed").and_then(Json::as_bool).unwrap_or(false);
    let restore = d.values.get("restore").and_then(Json::as_bool).unwrap_or(false);
    if changed {
        ui.add_space(4.0);
        if ui.radio(!restore, "Keep Solver Solution").clicked() {
            d.values.insert("restore".into(), json!(false));
        }
        if ui.radio(restore, "Restore Original Values").clicked() {
            d.values.insert("restore".into(), json!(true));
        }
    }
    // Reports describe the solution whether it is kept or not.
    let solved = d.values.get("values").and_then(Json::as_object).is_some_and(|v| !v.is_empty());
    if solved {
        ui.add_space(4.0);
        ui.label("Reports:");
        for (key, label) in [("answerReport", "Answer"), ("limitsReport", "Limits")] {
            let mut on = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
            if ui.checkbox(&mut on, label).changed() {
                d.values.insert(key.into(), json!(on));
            }
        }
    }
    let mut confirm = false;
    ok_cancel(ui, &mut confirm, open);
    if confirm {
        // The solution (and the saved model) is one undo step.
        if changed && restore {
            app.run_or_alert("edit.undo", json!({}));
        }
        let reports: Vec<&str> = [("answerReport", "answer"), ("limitsReport", "limits")]
            .into_iter()
            .filter(|(key, _)| d.values.get(*key).and_then(Json::as_bool) == Some(true))
            .map(|(_, kind)| kind)
            .collect();
        if solved && !reports.is_empty() {
            let field = |k: &str| d.values.get(k).cloned().unwrap_or(Json::Null);
            let params = json!({"reports": reports, "model": field("model"), "values": field("values"), "original": field("original"), "message": field("message")});
            app.run_or_alert("data.solverReport", params);
        }
        *open = false;
    }
}
