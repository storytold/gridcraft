//! Right-hand task panes: Comments, Watch Window, Selection Pane, Format Chart.

use egui::{Color32, Ui, vec2};
use serde_json::{Value as Json, json};

use gridcraft_engine::model::{ObjKind, ObjRef};

use crate::SheetApp;
use crate::theme::{self, Tokens};

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    // The Comments pane is engine state (`review.showComments`, so agents can drive it); the other
    // panes are UI state. One pane shows at a time: opening another one closes Comments.
    if app.grid.pane.as_deref() == Some("comments") {
        app.grid.pane = None;
        app.run_or_alert("review.showComments", json!({"on": true}));
    } else if app.grid.pane.is_some() && app.session.comments_pane {
        app.run_or_alert("review.showComments", json!({"on": false}));
    }
    let Some(pane) = app.grid.pane.clone().or_else(|| app.session.comments_pane.then(|| "comments".to_string())) else { return };
    let t = Tokens::get(ui.ctx());
    // Capped at half the window so a pane can never cover the sheet, whatever its content asks for.
    let max = (ui.available_width() / 2.0).max(300.0);
    let panel = egui::Panel::right("task_pane").default_size(300.0).max_size(max).resizable(true);
    panel.frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let lang = crate::i18n::current(ui.ctx());
            let title = match pane.as_str() {
                "comments" => lang.tr("Comments"),
                "watch" => lang.tr("Watch Window"),
                "selection" => lang.tr("Selection"),
                "formatChart" => lang.tr("Format Chart Area"),
                _ => "",
            };
            ui.label(egui::RichText::new(title).font(theme::ui_bold(15.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::icon_button(ui, crate::icons::Icon::Close, t.text_dim, "Close", vec2(22.0, 22.0)).clicked() {
                    if pane == "comments" {
                        app.run_or_alert("review.showComments", json!({"on": false}));
                    }
                    app.grid.pane = None;
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| match pane.as_str() {
            "comments" => comments(app, ui),
            "watch" => watch(app, ui),
            "selection" => selection(app, ui),
            "formatChart" => format_chart(app, ui),
            _ => {}
        });
    });
}

/// Lists comments and replies, links each to its cell, and runs delete/resolve/reopen/reply commands.
fn comments(app: &mut SheetApp, ui: &mut Ui) {
    let Some(d) = app.session.active() else { return };
    let Some(sh) = d.wb.active() else { return };
    let list: Vec<(String, gridcraft_engine::model::Comment)> = sh.comments.iter().map(|(c, m)| (c.a1(), m.clone())).collect();
    if list.is_empty() {
        ui.label(
            egui::RichText::new(crate::i18n::current(ui.ctx()).tr("No comments on this sheet yet. Select a cell and choose New Comment.")).italics(),
        );
    }
    let t = crate::theme::Tokens::get(ui.ctx());
    for (cell, c) in list {
        egui::Frame::NONE
            .fill(if c.resolved { t.window } else { t.menu_bg })
            .corner_radius(6.0)
            .inner_margin(8.0)
            .stroke(egui::Stroke::new(1.0, t.separator))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if ui.link(egui::RichText::new(&cell).strong()).clicked() {
                        let _ = app.run("selection.set", json!({"cell": cell}));
                        app.grid.ensure_visible = true;
                    }
                    ui.label(egui::RichText::new(crate::i18n::current(ui.ctx()).author_name(&c.author)).color(t.text_dim));
                    if c.resolved {
                        ui.label(egui::RichText::new(crate::i18n::current(ui.ctx()).tr("Resolved")).small().color(t.accent));
                    }
                });
                ui.label(&c.text);
                for (a, txt) in &c.replies {
                    ui.label(egui::RichText::new(format!("↳ {}: {txt}", crate::i18n::current(ui.ctx()).author_name(a))).color(t.text));
                }
                let key = egui::Id::unique(("reply", &cell));
                let mut draft: String = ui.ctx().data_mut(|m| m.get_temp(key)).unwrap_or_default();
                // Buttons first, right to left, so the reply field gets exactly the width they leave:
                // a field sized by a guess wider than the row pushes the resizable pane wider every frame.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(crate::i18n::current(ui.ctx()).tr("Delete")).clicked() {
                            app.run_or_alert("review.deleteComment", json!({"cell": cell}));
                        }
                        if ui.small_button(crate::i18n::current(ui.ctx()).tr(if c.resolved { "Reopen" } else { "Resolve" })).clicked() {
                            app.run_or_alert("review.resolveComment", json!({"cell": cell, "resolved": !c.resolved}));
                        }
                        let send = ui.small_button(crate::i18n::current(ui.ctx()).tr("Send")).clicked();
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut draft)
                                .hint_text(crate::i18n::current(ui.ctx()).tr("Reply…"))
                                .desired_width(ui.available_width()),
                        );
                        if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || send) && !draft.is_empty() {
                            app.run_or_alert("review.replyComment", json!({"cell": cell, "text": draft}));
                            draft.clear();
                        }
                    });
                });
                ui.ctx().data_mut(|m| m.insert_temp(key, draft));
            });
        ui.add_space(6.0);
    }
}

/// Adds the active cell, lists watch values and formulas, and removes watched entries.
fn watch(app: &mut SheetApp, ui: &mut Ui) {
    if ui.button(crate::i18n::current(ui.ctx()).tr("Add Watch (active cell)")).clicked()
        && let Some(d) = app.session.active()
        && let Some(sh) = d.wb.active()
    {
        let r = format!("{}!{}", gridcraft_engine::formula::quote_sheet(&sh.name), d.selection.active.a1());
        let _ = app.session.run("formulas.watchWindow", json!({"add": r}));
    }
    let list = app.session.run("formulas.watchWindow", json!({})).unwrap_or(Json::Null);
    egui::Grid::new("watch_grid").striped(true).num_columns(4).show(ui, |ui| {
        ui.strong("Cell");
        ui.strong("Value");
        ui.strong("Formula");
        ui.label("");
        ui.end_row();
        for w in list.as_array().cloned().unwrap_or_default() {
            let cell = w["cell"].as_str().unwrap_or("").to_string();
            ui.label(&cell);
            ui.label(w["value"].as_str().unwrap_or(""));
            ui.label(w["formula"].as_str().unwrap_or(""));
            if crate::widgets::icon_button(ui, crate::icons::Icon::Close, Tokens::get(ui.ctx()).text_dim, "Delete", vec2(18.0, 18.0)).clicked() {
                let _ = app.session.run("formulas.watchWindow", json!({"remove": cell}));
            }
            ui.end_row();
        }
    });
}

/// Lists the sheet's objects and dispatches select, bring-forward, send-backward and delete.
fn selection(app: &mut SheetApp, ui: &mut Ui) {
    let list = app.session.run("arrange.selectionPane", json!({})).unwrap_or(Json::Null);
    let items = list.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        ui.label(egui::RichText::new(crate::i18n::current(ui.ctx()).tr("There are no shapes, pictures or charts on this sheet.")).italics());
    }
    for it in items.iter().rev() {
        let kind = it["kind"].as_str().unwrap_or("").to_string();
        let id = it["id"].as_u64().unwrap_or(0) as u32;
        let obj = ObjKind::parse(&kind).map(|k| ObjRef::new(k, id));
        ui.horizontal(|ui| {
            let sel = obj.is_some() && app.selected_object == obj;
            if ui.selectable_label(sel, it["name"].as_str().unwrap_or("")).clicked() {
                app.selected_object = obj;
            }
            if ui.small_button("▲").on_hover_text(crate::i18n::current(ui.ctx()).tr("Bring Forward")).clicked() {
                app.run_or_alert("arrange.bringForward", json!({"kind": kind, "id": id}));
            }
            if ui.small_button("▼").on_hover_text(crate::i18n::current(ui.ctx()).tr("Send Backward")).clicked() {
                app.run_or_alert("arrange.sendBackward", json!({"kind": kind, "id": id}));
            }
            if crate::widgets::icon_button(
                ui,
                crate::icons::Icon::Close,
                Tokens::get(ui.ctx()).text_dim,
                crate::i18n::current(ui.ctx()).tr("Delete"),
                vec2(18.0, 18.0),
            )
            .clicked()
            {
                app.run_or_alert("object.delete", json!({"kind": kind, "id": id}));
            }
        });
    }
}

/// Draws the chart format pane: title, type, legend, elements/axis titles, series colors, palettes/layouts and size.
fn format_chart(app: &mut SheetApp, ui: &mut Ui) {
    let Some(id) = app.selected_chart() else {
        ui.label(egui::RichText::new(crate::i18n::current(ui.ctx()).tr("Select a chart to format it.")).italics());
        return;
    };
    let Some(chart) = app.session.active().and_then(|d| d.wb.active().and_then(|s| s.charts.iter().find(|c| c.id == id).cloned())) else {
        ui.label(crate::i18n::current(ui.ctx()).tr("The chart no longer exists."));
        return;
    };
    let theme = app.session.active().map(|d| d.wb.theme.clone()).unwrap_or_default();
    let lang = crate::i18n::current(ui.ctx());
    ui.collapsing(lang.tr("Chart Title"), |ui| {
        let field = egui::Id::unique(("gridcraft.chart_title", id));
        if let Some(title) = crate::widgets::committed_text(ui, field, chart.title.as_deref().unwrap_or(""), None) {
            app.run_or_alert("chart.set", json!({"chart": id, "title": title}));
        }
    });
    ui.collapsing(crate::i18n::current(ui.ctx()).tr("Chart Type"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for (l, k, s) in [
                ("Column", "column", ""),
                ("Stacked Column", "column", "stacked"),
                ("100% Stacked Column", "column", "stacked100"),
                ("Bar", "bar", ""),
                ("Stacked Bar", "bar", "stacked"),
                ("100% Stacked Bar", "bar", "stacked100"),
                ("Line", "line", "markers"),
                ("Stacked Line", "line", "stacked"),
                ("Area", "area", ""),
                ("Stacked Area", "area", "stacked"),
                ("100% Stacked Area", "area", "stacked100"),
                ("Pie", "pie", ""),
                ("Doughnut", "doughnut", ""),
                ("Scatter", "scatter", ""),
                ("Radar", "radar", ""),
            ] {
                let cur = chart.kind == gridcraft_engine::cmd::insert::chart_kind(k, s);
                if ui.selectable_label(cur, crate::i18n::current(ui.ctx()).tr(l)).clicked() {
                    app.run_or_alert("chart.set", json!({"chart": id, "type": k, "subtype": s}));
                }
            }
        });
        if matches!(chart.kind, gridcraft_chart::ChartKind::Pie | gridcraft_chart::ChartKind::Doughnut) {
            let draft = egui::Id::unique(("gridcraft.chart_first_slice_draft", id));
            let current = u32::from(chart.first_slice_angle);
            if let Some(angle) = chart_slider(ui, draft, current, 0..=359, lang.tr("First slice angle")) {
                app.run_or_alert("chart.set", json!({"chart": id, "firstSliceAngle": angle}));
            }
        }
        if chart.kind == gridcraft_chart::ChartKind::Doughnut {
            let draft = egui::Id::unique(("gridcraft.chart_hole_size_draft", id));
            let current = u32::from(chart.hole_size);
            if let Some(hole) = chart_slider(ui, draft, current, 10..=90, lang.tr("Doughnut hole size")) {
                app.run_or_alert("chart.set", json!({"chart": id, "holeSize": hole}));
            }
        }
    });
    ui.collapsing(crate::i18n::current(ui.ctx()).tr("Legend"), |ui| {
        ui.horizontal(|ui| {
            for (l, v) in [("None", "none"), ("Bottom", "bottom"), ("Top", "top"), ("Left", "left"), ("Right", "right")] {
                let cur = format!("{:?}", chart.legend).eq_ignore_ascii_case(v);
                if ui.selectable_label(cur, crate::i18n::current(ui.ctx()).tr(l)).clicked() {
                    app.run_or_alert("chart.set", json!({"chart": id, "legend": v}));
                }
            }
        });
    });
    ui.collapsing(crate::i18n::current(ui.ctx()).tr("Elements"), |ui| {
        let mut dl = chart.data_labels;
        if ui.checkbox(&mut dl, crate::i18n::current(ui.ctx()).tr("Data Labels")).changed() {
            app.run_or_alert("chart.set", json!({"chart": id, "dataLabels": dl}));
        }
        let mut gl = chart.gridlines;
        if ui.checkbox(&mut gl, crate::i18n::current(ui.ctx()).tr("Gridlines")).changed() {
            app.run_or_alert("chart.set", json!({"chart": id, "gridlines": gl}));
        }
        ui.horizontal(|ui| {
            ui.label(crate::i18n::current(ui.ctx()).tr("Horizontal axis title:"));
            let field = egui::Id::unique(("gridcraft.chart_x_title", id));
            if let Some(xt) = crate::widgets::committed_text(ui, field, chart.x_title.as_deref().unwrap_or(""), None) {
                app.run_or_alert("chart.set", json!({"chart": id, "xTitle": if xt.is_empty() { Json::Null } else { json!(xt) }}));
            }
        });
        ui.horizontal(|ui| {
            ui.label(crate::i18n::current(ui.ctx()).tr("Vertical axis title:"));
            let field = egui::Id::unique(("gridcraft.chart_y_title", id));
            if let Some(yt) = crate::widgets::committed_text(ui, field, chart.y_title.as_deref().unwrap_or(""), None) {
                app.run_or_alert("chart.set", json!({"chart": id, "yTitle": if yt.is_empty() { Json::Null } else { json!(yt) }}));
            }
        });
    });
    ui.collapsing(crate::i18n::current(ui.ctx()).tr("Data Table"), |ui| {
        let supported = chart.supports_data_table();
        let mut on = chart.data_table.is_some();
        if ui.add_enabled(supported, egui::Checkbox::new(&mut on, crate::i18n::current(ui.ctx()).tr("Show Data Table"))).changed() {
            app.run_or_alert("chart.set", json!({"chart": id, "dataTable": on}));
        }
        if !supported {
            ui.label(crate::i18n::current(ui.ctx()).tr("Available for column, bar, line, area and compatible combo charts."));
        }
        if let Some(table) = &chart.data_table {
            for (label, key, mut value) in [
                ("Show Legend Keys", "dataTableKeys", table.show_keys),
                ("Horizontal Borders", "dataTableHorizontalBorder", table.horizontal_border),
                ("Vertical Borders", "dataTableVerticalBorder", table.vertical_border),
                ("Data Table|Outline", "dataTableOutline", table.outline),
            ] {
                if ui.checkbox(&mut value, crate::i18n::current(ui.ctx()).tr(label)).changed() {
                    app.run_or_alert("chart.set", json!({"chart": id, key: value}));
                }
            }
        }
    });
    ui.collapsing(crate::i18n::current(ui.ctx()).tr("Series"), |ui| {
        let data = app.session.active().map(|d| gridcraft_chart::resolve(&d.wb, d.wb.active_sheet(), &chart));
        for (i, s) in chart.series.iter().enumerate() {
            let name = data
                .as_ref()
                .and_then(|d| d.series.get(i))
                .map(|x| x.name.clone())
                .unwrap_or_else(|| crate::i18n::current(ui.ctx()).tr_fill("Series {n}", &[("n", &(i + 1).to_string())]));
            let col =
                data.as_ref().and_then(|d| d.series.get(i)).map(|x| Color32::from_rgb(x.color[0], x.color[1], x.color[2])).unwrap_or(Color32::GRAY);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, col);
                ui.label(&name);
                ui.label(egui::RichText::new(&s.values).small().color(crate::theme::Tokens::get(ui.ctx()).text_dim));
                ui.menu_button(crate::i18n::current(ui.ctx()).tr("Fill…"), |ui| {
                    if let Some(c) = crate::widgets::color_palette_fixed(ui, &theme.colors, "Automatic") {
                        app.run_or_alert(
                            "chart.formatSelection",
                            json!({"chart": id, "series": i, "color": if c == "none" { Json::Null } else { json!(c) }}),
                        );
                        ui.close();
                    }
                });
            });
        }
    });
    ui.collapsing(crate::i18n::current(ui.ctx()).tr("Colors & Layout"), |ui| {
        let lang = crate::i18n::current(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            for p in ["colorful", "monochrome", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"] {
                if ui.button(lang.tr(p)).clicked() {
                    app.run_or_alert("chart.changeColors", json!({"chart": id, "palette": p}));
                }
            }
        });
        ui.horizontal(|ui| {
            for n in 1..=6 {
                if ui
                    .button(match crate::i18n::current(ui.ctx()) {
                        crate::i18n::Language::Zh => format!("版式 {n}"),
                        _ => format!("Layout {n}"),
                    })
                    .clicked()
                {
                    app.run_or_alert("chart.quickLayout", json!({"chart": id, "layout": n}));
                }
            }
        });
    });
    ui.collapsing(crate::i18n::current(ui.ctx()).tr("Chart|Size"), |ui| {
        // Keep the value being typed or dragged between frames; reloading it from the chart every frame threw the edit away.
        let draft = egui::Id::unique(("gridcraft.chart_size_draft", id));
        let current = (chart.anchor.width, chart.anchor.height);
        let (mut w, mut h) = ui.data_mut(|d| d.get_temp::<(f32, f32)>(draft)).unwrap_or(current);
        // A file can make a chart larger than the 4,000 points a field otherwise reaches. The
        // range keeps such a size, so the field shows it instead of clamping it to 4,000.
        let range = |v: f32| 40.0..=v.max(4000.0);
        let a = ui.add(egui::DragValue::new(&mut w).prefix("Width ").range(range(current.0)));
        let b = ui.add(egui::DragValue::new(&mut h).prefix("Height ").range(range(current.1)));
        if a.drag_stopped() || a.lost_focus() || b.drag_stopped() || b.lost_focus() {
            ui.data_mut(|d| d.remove::<(f32, f32)>(draft));
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                // Only an edited dimension is sent: the other keeps what the chart has, however
                // large a file made it.
                let mut params = serde_json::Map::new();
                params.insert("chart".into(), json!(id));
                if w != current.0 {
                    params.insert("width".into(), json!(w));
                }
                if h != current.1 {
                    params.insert("height".into(), json!(h));
                }
                if params.len() > 1 {
                    app.run_or_alert("chart.set", Json::Object(params));
                }
            }
        } else if a.dragged() || a.has_focus() || b.dragged() || b.has_focus() {
            ui.data_mut(|d| d.insert_temp(draft, (w, h)));
        } else {
            ui.data_mut(|d| d.remove::<(f32, f32)>(draft));
        }
    });
}

/// A chart slider that keeps the value being dragged in a per-chart draft and submits it once
/// through `chart.set` when the interaction ends, like the Size controls. Escape cancels, and a
/// value the chart already has is not sent again.
fn chart_slider(ui: &mut Ui, draft: egui::Id, value: u32, range: std::ops::RangeInclusive<u32>, label: &str) -> Option<u32> {
    let mut current = ui.data_mut(|d| d.get_temp::<u32>(draft)).unwrap_or(value);
    let r = ui.add(egui::Slider::new(&mut current, range).text(label));
    if r.drag_stopped() || r.clicked() || r.lost_focus() {
        ui.data_mut(|d| d.remove::<u32>(draft));
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || current == value { None } else { Some(current) }
    } else if r.dragged() || r.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft, current));
        None
    } else {
        ui.data_mut(|d| d.remove::<u32>(draft));
        None
    }
}
