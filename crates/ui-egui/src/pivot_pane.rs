//! The PivotTable Fields task pane: shown on the right when the active cell is in a pivot
//! report. Ticking a field adds it (text → Rows, numbers → Values); each area lists its fields
//! with move/remove menus. Everything runs `pivot.*` commands.

use egui::{Color32, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::l10n::{Arg, Tr, msg};
use crate::theme::{self, Tokens};

/// Whether the active cell is inside a pivot report (so the pane should show).
pub fn active_pivot(app: &SheetApp) -> Option<String> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let a = d.selection.active;
    sh.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.contains(a)) || p.anchor == a).map(|p| p.name.clone())
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let Some(name) = active_pivot(app) else { return };
    let t = Tokens::get(ui.ctx());
    let l = app.l10n;
    let info = match app.session.run("pivot.fields", json!({"pivot": name})) {
        Ok(v) => v,
        Err(_) => return,
    };
    egui::Panel::right("pivot_pane").default_size(280.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        ui.label(egui::RichText::new(l.tr("PivotTable Fields")).font(theme::ui_bold(15.0)));
        ui.label(egui::RichText::new(l.tr("Choose fields to add to report:")).small().color(t.text_dim));
        ui.add_space(4.0);
        let fields = info["fields"].as_array().cloned().unwrap_or_default();
        egui::ScrollArea::vertical().max_height(220.0).id_salt("pv_fields").show(ui, |ui| {
            for f in &fields {
                let fname = f["name"].as_str().unwrap_or("").to_string();
                let areas: Vec<String> =
                    f["areas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                let mut on = !areas.is_empty();
                let numeric = f["numeric"].as_bool().unwrap_or(false);
                let r = ui.checkbox(&mut on, egui::RichText::new(&fname).font(theme::ui_font(13.0)));
                if r.changed() {
                    let res = if on {
                        app.run_typed("pivot.addField", json!({"pivot": name, "field": fname, "area": if numeric { "values" } else { "rows" }}))
                    } else {
                        app.run_typed("pivot.removeField", json!({"pivot": name, "field": fname}))
                    };
                    if let Err(e) = res {
                        app.message = Some((l.tr("PivotTable").into(), app.error_text(&e)));
                    }
                }
                let samples: Vec<String> =
                    f["samples"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                r.on_hover_text(samples.join(", "));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(l.tr("Use a field's menu to move it between areas:")).small().color(t.text_dim));
        let area = |key: &str| -> Vec<String> {
            info["pivot"][key]
                .as_array()
                .or(info[key].as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x["name"].as_str().or(x["field"].as_str()).or(x["sourceCol"].as_str()).or(x.as_str()).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        ui.columns(2, |cols| {
            for (i, (key, label)) in
                [("filters", msg!("Filters")), ("columns", msg!("Columns")), ("rows", msg!("Rows")), ("values", msg!("Values"))].iter().enumerate()
            {
                let ui = &mut cols[i % 2];
                ui.label(egui::RichText::new(l.tr(label)).strong());
                egui::Frame::NONE.fill(t.window).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), 60.0));
                    for f in area(key) {
                        let b = ui.add(
                            egui::Button::new(egui::RichText::new(format!("{f} ▾")).font(theme::ui_font(12.0)))
                                .fill(Color32::WHITE)
                                .min_size(vec2(ui.available_width(), 20.0)),
                        );
                        egui::Popup::menu(&b).show(|ui| {
                            for (to, label) in [
                                ("rows", msg!("Move to Row Labels")),
                                ("columns", msg!("Move to Column Labels")),
                                ("filters", msg!("Move to Report Filter")),
                                ("values", msg!("Move to Values")),
                            ] {
                                if to != *key && ui.button(l.tr(label)).clicked() {
                                    app.run_or_alert("pivot.moveField", json!({"pivot": name, "field": f, "from": key, "to": to}));
                                }
                            }
                            if *key == "values" {
                                ui.separator();
                                for (func, label) in [
                                    ("sum", msg!("Sum")),
                                    ("count", msg!("Count")),
                                    ("average", msg!("Average")),
                                    ("max", msg!("Max")),
                                    ("min", msg!("Min")),
                                ] {
                                    if ui.button(l.text("ui-pivot-summarize-by", &[("function", Arg::from(&*l.tr(label)))])).clicked() {
                                        app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f, "func": func}));
                                    }
                                }
                                for (sa, label) in [
                                    ("normal", msg!("No Calculation")),
                                    ("percentOfGrandTotal", msg!("% of Grand Total")),
                                    ("runningTotal", msg!("Running Total")),
                                    ("rank", msg!("Rank")),
                                ] {
                                    if ui.button(l.text("ui-pivot-show-as", &[("calculation", Arg::from(&*l.tr(label)))])).clicked() {
                                        app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f, "showAs": sa}));
                                    }
                                }
                            } else {
                                ui.separator();
                                for (o, label) in [("asc", msg!("Sort A to Z")), ("desc", msg!("Sort Z to A"))] {
                                    if ui.button(l.tr(label)).clicked() {
                                        app.run_or_alert("pivot.sort", json!({"pivot": name, "field": f, "order": o}));
                                    }
                                }
                                for (g, label) in [
                                    ("years", msg!("Group by Years")),
                                    ("quarters", msg!("Group by Quarters")),
                                    ("months", msg!("Group by Months")),
                                    ("none", msg!("Ungroup")),
                                ] {
                                    if ui.button(l.tr(label)).clicked() {
                                        app.run_or_alert("pivot.group", json!({"pivot": name, "field": f, "by": g}));
                                    }
                                }
                            }
                            ui.separator();
                            if ui.button(l.tr("Remove Field")).clicked() {
                                app.run_or_alert("pivot.removeField", json!({"pivot": name, "field": f, "area": key}));
                            }
                        });
                    }
                });
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(l.tr("Refresh")).clicked() {
                app.run_or_alert("pivot.refresh", json!({"pivot": name}));
            }
            egui::ComboBox::from_id_salt("pv_layout").selected_text(l.tr("Report Layout")).show_ui(ui, |ui| {
                for (label, v) in [(msg!("Compact"), "compact"), (msg!("Outline"), "outline"), (msg!("Tabular"), "tabular")] {
                    if ui.button(l.tr(label)).clicked() {
                        app.run_or_alert("pivot.layout", json!({"pivot": name, "layout": v}));
                    }
                }
            });
        });
        let _: Option<Json> = None;
    });
}
