//! Right-hand task panes: Comments, Watch Window, Selection Pane, Format Chart.

use egui::{Color32, Ui, vec2};
use serde_json::{Value as Json, json};

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
    let panel = egui::Panel::right("task_pane").default_size(330.0).max_size(max).resizable(true);
    panel.frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let title = match pane.as_str() {
                "comments" => "Comments",
                "watch" => "Watch Window",
                "selection" => "Selection",
                "formatChart" => "Format Chart",
                _ => "",
            };
            let title = if pane == "formatChart" { chart_pane_title(app, ui.ctx()) } else { title };
            ui.label(egui::RichText::new(app.ui.language.tr(title)).font(theme::ui_font(20.0)).color(if pane == "formatChart" {
                t.accent
            } else {
                t.text
            }));
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
        if pane == "formatChart" {
            format_chart(app, ui);
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| match pane.as_str() {
            "comments" => comments(app, ui),
            "watch" => watch(app, ui),
            "selection" => selection(app, ui),
            "formatChart" => format_chart(app, ui),
            _ => {}
        });
    });
}

fn comments(app: &mut SheetApp, ui: &mut Ui) {
    let Some(d) = app.session.active() else { return };
    let Some(sh) = d.wb.active() else { return };
    let list: Vec<(String, gridcraft_engine::model::Comment)> = sh.comments.iter().map(|(c, m)| (c.a1(), m.clone())).collect();
    if list.is_empty() {
        ui.label(egui::RichText::new("No comments on this sheet yet. Select a cell and choose New Comment.").italics());
    }
    for (cell, c) in list {
        egui::Frame::NONE
            .fill(if c.resolved { Color32::from_gray(240) } else { Color32::WHITE })
            .corner_radius(6.0)
            .inner_margin(8.0)
            .stroke(egui::Stroke::new(1.0, Color32::from_gray(215)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if ui.link(egui::RichText::new(&cell).strong()).clicked() {
                        let _ = app.run("selection.set", json!({"cell": cell}));
                        app.grid.ensure_visible = true;
                    }
                    ui.label(egui::RichText::new(&c.author).color(Color32::from_gray(90)));
                    if c.resolved {
                        ui.label(egui::RichText::new("Resolved").small().color(Color32::from_rgb(0x10, 0x7C, 0x41)));
                    }
                });
                ui.label(&c.text);
                for (a, txt) in &c.replies {
                    ui.label(egui::RichText::new(format!("↳ {a}: {txt}")).color(Color32::from_gray(60)));
                }
                let key = egui::Id::new(("reply", &cell));
                let mut draft: String = ui.ctx().data_mut(|m| m.get_temp(key)).unwrap_or_default();
                // Buttons first, right to left, so the reply field gets exactly the width they leave:
                // a field sized by a guess wider than the row pushes the resizable pane wider every frame.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Delete").clicked() {
                            app.run_or_alert("review.deleteComment", json!({"cell": cell}));
                        }
                        if ui.small_button(if c.resolved { "Reopen" } else { "Resolve" }).clicked() {
                            app.run_or_alert("review.resolveComment", json!({"cell": cell, "resolved": !c.resolved}));
                        }
                        let send = ui.small_button("Send").clicked();
                        let r = ui.add(egui::TextEdit::singleline(&mut draft).hint_text("Reply…").desired_width(ui.available_width()));
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

fn watch(app: &mut SheetApp, ui: &mut Ui) {
    if ui.button("Add Watch (active cell)").clicked()
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
            if ui.small_button("✕").clicked() {
                let _ = app.session.run("formulas.watchWindow", json!({"remove": cell}));
            }
            ui.end_row();
        }
    });
}

fn selection(app: &mut SheetApp, ui: &mut Ui) {
    let list = app.session.run("arrange.selectionPane", json!({})).unwrap_or(Json::Null);
    let items = list.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        ui.label(egui::RichText::new("There are no shapes, pictures or charts on this sheet.").italics());
    }
    for it in items.iter().rev() {
        let kind = it["kind"].as_str().unwrap_or("").to_string();
        let id = it["id"].as_u64().unwrap_or(0) as u32;
        ui.horizontal(|ui| {
            let sel = app.selected_chart == Some(id);
            if ui.selectable_label(sel, it["name"].as_str().unwrap_or("")).clicked() {
                app.selected_chart = Some(id);
            }
            if ui.small_button("▲").on_hover_text("Bring Forward").clicked() {
                app.run_or_alert("arrange.bringForward", json!({"kind": kind, "id": id}));
            }
            if ui.small_button("▼").on_hover_text("Send Backward").clicked() {
                app.run_or_alert("arrange.sendBackward", json!({"kind": kind, "id": id}));
            }
            if ui.small_button("✕").on_hover_text("Delete").clicked() {
                app.run_or_alert("object.delete", json!({"kind": kind, "id": id}));
            }
        });
    }
}

fn format_chart(app: &mut SheetApp, ui: &mut Ui) {
    let Some(id) = app.selected_chart else {
        ui.label(egui::RichText::new("Select a chart to format it.").italics());
        return;
    };
    let Some(chart) = app.session.active().and_then(|d| d.wb.active().and_then(|s| s.charts.iter().find(|c| c.id == id).cloned())) else {
        ui.label("The chart no longer exists.");
        return;
    };
    chart_format_controls(app, ui, &chart);
}

fn chart_general_controls(app: &mut SheetApp, ui: &mut Ui, chart: &gridcraft_engine::model::Chart) {
    let id = chart.id;
    let theme = app.session.active().map(|d| d.wb.theme.clone()).unwrap_or_default();
    ui.collapsing(crate::widgets::tr(ui, "Chart Settings"), |ui| {
        ui.collapsing(crate::widgets::tr(ui, "Chart Title"), |ui| {
            let field = egui::Id::new(("gridcraft.chart_title", id));
            if let Some(title) = crate::widgets::committed_text(ui, field, chart.title.as_deref().unwrap_or(""), None) {
                app.run_or_alert("chart.set", json!({"chart": id, "title": title}));
            }
        });
        ui.collapsing(crate::widgets::tr(ui, "Chart Type"), |ui| {
            ui.horizontal_wrapped(|ui| {
                for (l, k, s) in [
                    ("Column", "column", ""),
                    ("Stacked", "column", "stacked"),
                    ("Bar", "bar", ""),
                    ("Line", "line", "markers"),
                    ("Area", "area", ""),
                    ("Pie", "pie", ""),
                    ("Doughnut", "doughnut", ""),
                    ("Scatter", "scatter", ""),
                    ("Radar", "radar", ""),
                ] {
                    if ui.button(crate::widgets::tr(ui, l)).clicked() {
                        app.run_or_alert("chart.set", json!({"chart": id, "type": k, "subtype": s}));
                    }
                }
            });
        });
        ui.collapsing(crate::widgets::tr(ui, "Legend"), |ui| {
            ui.horizontal(|ui| {
                for (l, v) in [("None", "none"), ("Bottom", "bottom"), ("Top", "top"), ("Left", "left"), ("Right", "right")] {
                    let cur = format!("{:?}", chart.legend).eq_ignore_ascii_case(v);
                    if ui.selectable_label(cur, crate::widgets::tr(ui, l)).clicked() {
                        app.run_or_alert("chart.set", json!({"chart": id, "legend": v}));
                    }
                }
            });
        });
        ui.collapsing(crate::widgets::tr(ui, "Elements"), |ui| {
            let mut dl = chart.data_labels;
            if ui.checkbox(&mut dl, "Data Labels").changed() {
                app.run_or_alert("chart.set", json!({"chart": id, "dataLabels": dl}));
            }
            let mut gl = chart.gridlines;
            if ui.checkbox(&mut gl, "Gridlines").changed() {
                app.run_or_alert("chart.set", json!({"chart": id, "gridlines": gl}));
            }
            ui.horizontal(|ui| {
                ui.label("Horizontal axis title:");
                let field = egui::Id::new(("gridcraft.chart_x_title", id));
                if let Some(xt) = crate::widgets::committed_text(ui, field, chart.x_title.as_deref().unwrap_or(""), None) {
                    app.run_or_alert("chart.set", json!({"chart": id, "xTitle": if xt.is_empty() { Json::Null } else { json!(xt) }}));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Vertical axis title:");
                let field = egui::Id::new(("gridcraft.chart_y_title", id));
                if let Some(yt) = crate::widgets::committed_text(ui, field, chart.y_title.as_deref().unwrap_or(""), None) {
                    app.run_or_alert("chart.set", json!({"chart": id, "yTitle": if yt.is_empty() { Json::Null } else { json!(yt) }}));
                }
            });
        });
        ui.collapsing(crate::widgets::tr(ui, "Chart|Series"), |ui| {
            let data = app.session.active().map(|d| gridcraft_chart::resolve(&d.wb, d.wb.active_sheet, chart));
            for (i, s) in chart.series.iter().enumerate() {
                let name = data.as_ref().and_then(|d| d.series.get(i)).map(|x| x.name.clone()).unwrap_or_else(|| format!("Series {}", i + 1));
                let col = data
                    .as_ref()
                    .and_then(|d| d.series.get(i))
                    .map(|x| Color32::from_rgb(x.color[0], x.color[1], x.color[2]))
                    .unwrap_or(Color32::GRAY);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, col);
                    ui.label(&name);
                    ui.label(egui::RichText::new(&s.values).small().color(Color32::from_gray(110)));
                    ui.menu_button("Fill…", |ui| {
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
        ui.collapsing(crate::widgets::tr(ui, "Colors & Layout"), |ui| {
            ui.horizontal_wrapped(|ui| {
                for p in ["colorful", "monochrome", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"] {
                    if ui.button(p).clicked() {
                        app.run_or_alert("chart.changeColors", json!({"chart": id, "palette": p}));
                    }
                }
            });
            ui.horizontal(|ui| {
                for n in 1..=6 {
                    if ui.button(format!("Layout {n}")).clicked() {
                        app.run_or_alert("chart.quickLayout", json!({"chart": id, "layout": n}));
                    }
                }
            });
        });
    });
    chart_section(ui, "Size", |ui| {
        // Keep the value being typed or dragged between frames; reloading it from the chart every frame threw the edit away.
        let draft = egui::Id::new(("gridcraft.chart_size_draft", id));
        let (mut w, mut h) = ui.data_mut(|d| d.get_temp::<(f32, f32)>(draft)).unwrap_or((chart.anchor.width, chart.anchor.height));
        let a = ui.add(egui::DragValue::new(&mut w).prefix(format!("{} ", crate::widgets::tr(ui, "Width"))).range(40.0..=4000.0));
        let b = ui.add(egui::DragValue::new(&mut h).prefix(format!("{} ", crate::widgets::tr(ui, "Height"))).range(40.0..=4000.0));
        if a.drag_stopped() || a.lost_focus() || b.drag_stopped() || b.lost_focus() {
            ui.data_mut(|d| d.remove::<(f32, f32)>(draft));
            if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.run_or_alert("chart.set", json!({"chart": id, "width": w, "height": h}));
            }
        } else if a.dragged() || a.has_focus() || b.dragged() || b.has_focus() {
            ui.data_mut(|d| d.insert_temp(draft, (w, h)));
        } else {
            ui.data_mut(|d| d.remove::<(f32, f32)>(draft));
        }
    });
}

/// Familiar Current Selection / Axis Options / Fill & Line controls, backed by chart.set.
fn chart_format_controls(app: &mut SheetApp, ui: &mut Ui, chart: &gridcraft_engine::model::Chart) {
    use crate::chartview::{ChartElement, select_element, selected_element};
    use gridcraft_engine::model::ChartKind;
    let id = chart.id;
    let colors = app.session.active().map(|d| d.wb.theme.colors).unwrap_or_else(|| gridcraft_engine::model::Theme::default().colors);
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("chart_theme_colors"), colors));
    let names = app
        .session
        .active()
        .map(|d| gridcraft_chart::resolve(&d.wb, d.wb.active_sheet, chart).series.into_iter().map(|s| s.name).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut selected = selected_element(ui.ctx(), id);
    ui.label(crate::widgets::tr(ui, "Current Selection"));
    egui::ComboBox::from_id_salt(("chart_current_selection", id)).selected_text(crate::widgets::tr(ui, selected.label())).show_ui(ui, |ui| {
        for e in [
            ChartElement::ChartArea,
            ChartElement::PlotArea,
            ChartElement::Title,
            ChartElement::XAxis,
            ChartElement::YAxis,
            ChartElement::XAxisTitle,
            ChartElement::YAxisTitle,
            ChartElement::Gridlines,
            ChartElement::Legend,
            ChartElement::DataLabels,
            ChartElement::TrendlineText,
        ] {
            ui.selectable_value(&mut selected, e, crate::widgets::tr(ui, e.label()));
        }
        for (i, s) in chart.series.iter().enumerate() {
            ui.selectable_value(
                &mut selected,
                ChartElement::Series(i),
                format!(
                    "{} {} — {}",
                    crate::widgets::tr(ui, "Chart|Series"),
                    i + 1,
                    names.get(i).map(String::as_str).or(s.name.as_deref()).unwrap_or(crate::widgets::tr(ui, "Data"))
                ),
            );
        }
    });
    select_element(ui.ctx(), id, selected);
    let page_id = ui.id().with(("chart_pane_page", id, selected));
    let mut page = ui
        .ctx()
        .data_mut(|d| d.get_temp::<u8>(page_id))
        .unwrap_or(if matches!(selected, ChartElement::Series(_) | ChartElement::ChartArea | ChartElement::PlotArea) { 0 } else { 1 });
    ui.horizontal(|ui| {
        for (value, label, icon) in [
            (0, "Fill & Line", crate::icons::Icon::Fill),
            (1, "Element Options", crate::icons::Icon::ChartBar),
            (2, "Size & Properties", crate::icons::Icon::Format),
        ] {
            let active = page == value;
            let response = ui.add(egui::Button::new("").selected(active).min_size(vec2(36.0, 30.0)));
            let accessible_label = crate::widgets::tr(ui, label);
            let enabled = ui.is_enabled();
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, accessible_label));
            let t = Tokens::get(ui.ctx());
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(response.rect.center(), vec2(20.0, 20.0)),
                icon,
                if active { t.accent } else { t.text_dim },
            );
            if response.clicked() {
                page = value;
            }
            response.on_hover_text(crate::widgets::tr(ui, label));
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(page_id, page));
    ui.separator();
    let mut format = chart.format.clone();
    let mut changed = false;
    let bars = matches!(chart.kind, ChartKind::BarClustered | ChartKind::BarStacked | ChartKind::BarStacked100);
    let scatter = matches!(chart.kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble);
    let supported = scatter
        || matches!(
            chart.kind,
            ChartKind::ColumnClustered
                | ChartKind::ColumnStacked
                | ChartKind::ColumnStacked100
                | ChartKind::BarClustered
                | ChartKind::BarStacked
                | ChartKind::BarStacked100
                | ChartKind::Line
                | ChartKind::LineMarkers
                | ChartKind::LineStacked
                | ChartKind::Area
                | ChartKind::AreaStacked
                | ChartKind::Combo
        );
    egui::ScrollArea::vertical().id_salt(("chart_properties_scroll", id, selected, page)).auto_shrink([false, false]).show(ui, |ui| {
        if page == 2 {
            chart_general_controls(app, ui, chart);
            return;
        }
        ui.push_id(("chart_format", id, format!("{selected:?}")), |ui| match selected {
            ChartElement::XAxis | ChartElement::YAxis if supported && page == 0 => {
                let axis = if selected == ChartElement::XAxis { &mut format.x_axis } else { &mut format.y_axis };
                chart_section(ui, "Chart|Line", |ui| {
                    changed |= chart_stroke(ui, &mut axis.line);
                });
                chart_section(ui, "Tick Marks", |ui| {
                    changed |= chart_stroke(ui, &mut axis.tick_line);
                });
            }
            ChartElement::XAxis | ChartElement::YAxis if supported && page == 1 => {
                let x = selected == ChartElement::XAxis;
                let numeric = if x { scatter || bars } else { !bars };
                ui.label(crate::widgets::tr(ui, "Axis Title"));
                let title = if x { chart.x_title.as_deref() } else { chart.y_title.as_deref() }.unwrap_or("");
                if let Some(title) = crate::widgets::committed_text(ui, ui.id().with("axis_title"), title, None) {
                    let key = if x { "xTitle" } else { "yTitle" };
                    let mut params = json!({"chart":id});
                    params[key] = if title.is_empty() { Json::Null } else { json!(title) };
                    app.run_or_alert("chart.set", params);
                }
                let axis = if x { &mut format.x_axis } else { &mut format.y_axis };
                chart_section(ui, "Axis Options", |ui| {
                    if numeric {
                        ui.label(crate::widgets::tr(ui, "Bounds (blank = Automatic)"));
                        changed |= chart_number(ui, "Minimum", &mut axis.minimum);
                        changed |= chart_number(ui, "Maximum", &mut axis.maximum);
                        if !axis.logarithmic {
                            changed |= chart_number(ui, "Major Unit", &mut axis.major_unit);
                        }
                        changed |= ui.checkbox(&mut axis.logarithmic, crate::widgets::tr(ui, "Logarithmic Scale (base 10)")).changed();
                        if axis.logarithmic {
                            ui.small(crate::widgets::tr(ui, "Only positive values can be plotted."));
                        }
                    }
                    changed |= ui
                        .checkbox(
                            &mut axis.reversed,
                            crate::widgets::tr(ui, if numeric { "Values in reverse order" } else { "Categories in reverse order" }),
                        )
                        .changed();
                });
                chart_section(ui, "Tick Marks", |ui| {
                    changed |= chart_tick_picker(ui, "Minor Tick Marks", &mut axis.minor_tick);
                    if numeric && !axis.logarithmic {
                        changed |= chart_number(ui, "Minor Unit", &mut axis.minor_unit);
                    }
                    ui.label(crate::widgets::tr(ui, "Major Tick Marks"));
                    let old_tick = axis.major_tick.clone();
                    egui::ComboBox::from_id_salt("major_ticks")
                        .selected_text(crate::widgets::tr(
                            ui,
                            match axis.major_tick.as_deref() {
                                Some("in") => "Inside",
                                Some("out") => "Outside",
                                Some("cross") => "Cross",
                                _ => "None",
                            },
                        ))
                        .show_ui(ui, |ui| {
                            for (value, label) in [("none", "None"), ("in", "Inside"), ("out", "Outside"), ("cross", "Cross")] {
                                ui.selectable_value(&mut axis.major_tick, Some(value.into()), crate::widgets::tr(ui, label));
                            }
                        });
                    changed |= old_tick != axis.major_tick;
                });
                if numeric {
                    chart_section(ui, "Number", |ui| {
                        ui.label(crate::widgets::tr(ui, "Category"));
                        let category = match axis.number_format.as_deref() {
                            None => "Linked to source",
                            Some("General") => "General",
                            Some("0.00") => "Number",
                            Some("0.00%") => "Percentage",
                            Some("0.00E+00") => "Scientific",
                            _ => "Custom",
                        };
                        egui::ComboBox::from_id_salt("number_category").selected_text(crate::widgets::tr(ui, category)).show_ui(ui, |ui| {
                            for (label, code) in [
                                ("Linked to source", None),
                                ("General", Some("General")),
                                ("Number", Some("0.00")),
                                ("Percentage", Some("0.00%")),
                                ("Scientific", Some("0.00E+00")),
                            ] {
                                if ui.selectable_label(category == label, crate::widgets::tr(ui, label)).clicked() {
                                    axis.number_format = code.map(str::to_string);
                                    changed = true;
                                }
                            }
                        });
                        ui.label(crate::widgets::tr(ui, "Number Format (blank = linked to source)"));
                        if let Some(code) =
                            crate::widgets::committed_text(ui, ui.id().with("axis_number_format"), axis.number_format.as_deref().unwrap_or(""), None)
                        {
                            axis.number_format = if code.trim().is_empty() { None } else { Some(code) };
                            changed = true;
                        }
                        ui.small(crate::widgets::tr(ui, "Examples: 0.00; 0%; 0.00E+00; 0 \"kg\". Use the internal format syntax."));
                    });
                }
                if ui.button(crate::widgets::tr(ui, "Reset to Automatic")).clicked() {
                    *axis = Default::default();
                    changed = true;
                }
            }
            ChartElement::Gridlines if page == 0 => {
                chart_section(ui, "Primary Horizontal", |ui| {
                    changed |= chart_stroke(ui, &mut format.horizontal_gridline);
                });
                chart_section(ui, "Primary Vertical", |ui| {
                    changed |= chart_stroke(ui, &mut format.vertical_gridline);
                });
            }
            ChartElement::Gridlines if page == 1 => {
                let mut grid = chart.gridlines;
                if ui.checkbox(&mut grid, crate::widgets::tr(ui, if bars { "Primary Vertical" } else { "Primary Horizontal" })).changed() {
                    app.run_or_alert("chart.set", json!({"chart":id,"gridlines":grid}));
                }
                changed |= ui
                    .checkbox(&mut format.vertical_gridlines, crate::widgets::tr(ui, if bars { "Primary Horizontal" } else { "Primary Vertical" }))
                    .changed();
            }
            ChartElement::Series(i) if supported && i < chart.series.len() => {
                let colors = app.session.active().map(|d| d.wb.theme.colors).unwrap_or_default();
                if page == 0 {
                    ui.menu_button(crate::widgets::tr(ui, "Series Color…"), |ui| {
                        if let Some(color) = crate::widgets::color_palette(ui, &colors, crate::widgets::tr(ui, "Automatic")) {
                            app.run_or_alert(
                                "chart.formatSelection",
                                json!({"chart":id,"series":i,"color":if color=="none" {Json::Null}else{json!(color)}}),
                            );
                            ui.close();
                        }
                    });
                }
                let spec = format.series.entry(i).or_default();

                let lines = matches!(
                    chart.kind,
                    ChartKind::Scatter
                        | ChartKind::ScatterLines
                        | ChartKind::Line
                        | ChartKind::LineMarkers
                        | ChartKind::LineStacked
                        | ChartKind::Combo
                );
                if page == 0 && lines {
                    let marker_id = ui.id().with("line_or_marker");
                    let mut marker = ui.ctx().data_mut(|d| d.get_temp::<bool>(marker_id)).unwrap_or(false);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut marker, false, crate::widgets::tr(ui, "Chart|Line"));
                        ui.selectable_value(&mut marker, true, crate::widgets::tr(ui, "Marker"));
                    });
                    ui.ctx().data_mut(|d| d.insert_temp(marker_id, marker));
                    if !marker {
                        chart_section(ui, "Chart|Line", |ui| {
                            let mut visible = !spec.no_line && (chart.kind != ChartKind::Scatter || spec.line_width.is_some());
                            if ui.checkbox(&mut visible, crate::widgets::tr(ui, "Show Line")).changed() {
                                spec.no_line = !visible;
                                if visible {
                                    spec.line_width.get_or_insert(2.25);
                                }
                                changed = true;
                            }
                            changed |= chart_number_f32(ui, "Line Width (pt)", &mut spec.line_width);
                            changed |= ui.checkbox(&mut spec.dashed, crate::widgets::tr(ui, "Dashed Line")).changed();
                            let mut smooth = spec.smooth.unwrap_or_else(|| chart.series.get(i).is_some_and(|s| s.smooth));
                            if ui.checkbox(&mut smooth, crate::widgets::tr(ui, "Smoothed Line")).changed() {
                                spec.smooth = Some(smooth);
                                changed = true;
                            }
                            changed |= chart_color(ui, "Line Color & Transparency", &mut spec.line_color);
                        });
                    } else {
                        chart_section(ui, "Marker Options", |ui| {
                            let old = spec.marker.clone();
                            egui::ComboBox::from_id_salt("marker_type")
                                .selected_text(crate::widgets::tr(
                                    ui,
                                    match spec.marker.as_deref() {
                                        None => "Automatic",
                                        Some("none") => "None",
                                        Some("circle") => "Circle",
                                        Some("square") => "Square",
                                        Some("diamond") => "Diamond",
                                        Some("triangle") => "Triangle",
                                        Some("cross") => "X",
                                        Some("plus") => "Plus (+)",
                                        Some("star") => "Star",
                                        Some("dash") => "Dash",
                                        _ => "Automatic",
                                    },
                                ))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut spec.marker, None, crate::widgets::tr(ui, "Automatic"));
                                    for (key, label) in [
                                        ("none", "None"),
                                        ("circle", "Circle"),
                                        ("square", "Square"),
                                        ("diamond", "Diamond"),
                                        ("triangle", "Triangle"),
                                        ("cross", "X"),
                                        ("plus", "Plus (+)"),
                                        ("star", "Star"),
                                        ("dash", "Dash"),
                                    ] {
                                        ui.selectable_value(&mut spec.marker, Some(key.into()), crate::widgets::tr(ui, label));
                                    }
                                });
                            changed |= old != spec.marker;
                            changed |= chart_number_f32(ui, "Marker Size (2–72 pt)", &mut spec.marker_size);
                        });
                        chart_section(ui, "Chart Fill|Fill", |ui| {
                            changed |= ui.checkbox(&mut spec.marker_no_fill, crate::widgets::tr(ui, "No Fill Marker")).changed();
                            changed |= chart_color(ui, "Marker Fill", &mut spec.marker_fill);
                        });
                        chart_section(ui, "Chart|Border", |ui| {
                            changed |= chart_color(ui, "Marker Border Color", &mut spec.marker_border_color);
                            changed |= chart_number_f32(ui, "Marker Border Width (pt)", &mut spec.marker_border_width);
                        });
                    }
                }
                if page == 0 && !lines && chart.kind != ChartKind::Bubble {
                    chart_section(ui, "Chart Fill|Fill", |ui| {
                        changed |= chart_fill(ui, &mut spec.fill_color);
                    });
                    chart_section(ui, "Chart|Border", |ui| {
                        changed |= chart_number_f32(ui, "Border Width (pt)", &mut spec.border_width);
                    });
                }
                if page == 1
                    && matches!(
                        chart.kind,
                        ChartKind::ColumnClustered
                            | ChartKind::ColumnStacked
                            | ChartKind::ColumnStacked100
                            | ChartKind::BarClustered
                            | ChartKind::BarStacked
                            | ChartKind::BarStacked100
                    )
                {
                    changed |= chart_number_f32(ui, "Gap Width (0–500%)", &mut format.gap_width);
                    changed |= chart_number_f32(ui, "Series Overlap (-100–100%)", &mut format.overlap);
                }
                if page == 1 {
                    chart_trendline_controls(app, ui, chart, i);
                }
            }
            ChartElement::ChartArea if page == 0 => {
                changed |= chart_surface(ui, &mut format.chart_area);
            }
            ChartElement::PlotArea if supported && page == 0 => {
                changed |= chart_surface(ui, &mut format.plot_area);
            }
            ChartElement::Title if page == 1 => {
                ui.label(crate::widgets::tr(ui, "Title Text"));
                if let Some(title) = crate::widgets::committed_text(ui, ui.id().with("title"), chart.title.as_deref().unwrap_or(""), None) {
                    app.run_or_alert("chart.set", json!({"chart":id,"title":title}));
                }
            }
            ChartElement::Legend if page == 1 => {
                ui.label(crate::widgets::tr(ui, "Legend Position"));
                ui.horizontal_wrapped(|ui| {
                    for (label, pos) in [("None", "none"), ("Bottom", "bottom"), ("Top", "top"), ("Left", "left"), ("Right", "right")] {
                        if ui.button(crate::widgets::tr(ui, label)).clicked() {
                            app.run_or_alert("chart.set", json!({"chart":id,"legend":pos}));
                        }
                    }
                });
                changed |= ui.checkbox(&mut format.legend_overlay, crate::widgets::tr(ui, "Overlay Legend")).changed();
                ui.label(crate::widgets::tr(ui, "Drag the legend inside the plot area."));
                if ui.button(crate::widgets::tr(ui, "Reset Position")).clicked() {
                    format.legend_position = None;
                    changed = true;
                }
            }
            ChartElement::TrendlineText => {
                if page == 1 {
                    let key = ui.id().with(("trendline_series", id));
                    let mut i = ui.ctx().data_mut(|d| d.get_temp::<usize>(key)).unwrap_or(0).min(chart.series.len().saturating_sub(1));
                    egui::ComboBox::from_id_salt("trendline_series")
                        .selected_text(format!("{} {}", crate::widgets::tr(ui, "Chart|Series"), i + 1))
                        .show_ui(ui, |ui| {
                            for j in 0..chart.series.len() {
                                ui.selectable_value(&mut i, j, format!("{} {}", crate::widgets::tr(ui, "Chart|Series"), j + 1));
                            }
                        });
                    ui.ctx().data_mut(|d| d.insert_temp(key, i));
                    chart_trendline_controls(app, ui, chart, i);
                }
                ui.label(crate::widgets::tr(ui, "Use Home > Font to format the selected chart text."));
                ui.label(crate::widgets::tr(ui, "Drag the equation to reposition it."));
                if ui.button(crate::widgets::tr(ui, "Reset Position")).clicked() {
                    format.trendline_label_position = None;
                    changed = true;
                }
            }
            ChartElement::DataLabels | ChartElement::XAxisTitle | ChartElement::YAxisTitle => {
                ui.label(crate::widgets::tr(ui, "Use Home > Font to format the selected chart text."));
            }
            _ => {
                ui.label(crate::widgets::tr(ui, "Choose another category to format this element."));
            }
        });
    });
    if changed {
        app.run_or_alert("chart.set", json!({"chart": id, "format": format}));
    }
    ui.separator();
}

fn chart_trendline_controls(app: &mut SheetApp, ui: &mut Ui, chart: &gridcraft_engine::model::Chart, series: usize) {
    if !gridcraft_chart::supports_trendline(gridcraft_chart::series_plot_kind(chart, series)) {
        return;
    }
    ui.collapsing(crate::widgets::tr(ui, "Trendline"),|ui| {
        let id=ui.id().with(("trendline_draft",chart.id,series));
        let mut draft=ui.ctx().data_mut(|d|d.get_temp::<(String,u32,u32,bool,bool)>(id)).unwrap_or(("linear".into(),2,2,false,false));
        for (key,label) in [("linear","Linear"),("exponential","Exponential"),("logarithmic","Logarithmic"),("power","Power"),("polynomial","Polynomial"),("movingAverage","Moving Average")] {
            ui.radio_value(&mut draft.0,key.to_string(),crate::widgets::tr(ui,label));
        }
        if draft.0=="polynomial" {ui.add(egui::DragValue::new(&mut draft.1).prefix(crate::widgets::tr(ui,"Order ")).range(2..=6));}
        if draft.0=="movingAverage" {ui.add(egui::DragValue::new(&mut draft.2).prefix(crate::widgets::tr(ui,"Period ")).range(2..=255));}
        if draft.0!="movingAverage" {
            ui.checkbox(&mut draft.3,crate::widgets::tr(ui,"Display Equation on Chart"));
            ui.checkbox(&mut draft.4,crate::widgets::tr(ui,"Display R-squared Value on Chart"));
        }
        if ui.button(crate::widgets::tr(ui,"Add Trendline")).clicked() {
            app.run_or_alert("chart.addElement",json!({"chart":chart.id,"element":"trendline","series":series,"type":draft.0,"order":draft.1,"period":draft.2,"displayEquation":draft.3,"displayRSquared":draft.4}));
        }
        if let Some(s)=chart.series.get(series) {
            ui.label(format!("{}: {}",crate::widgets::tr(ui,"Trendlines in this series"),s.trendlines.len()));
            if !s.trendlines.is_empty() && ui.button(crate::widgets::tr(ui,"Remove Trendlines from Series")).clicked() {
                app.run_or_alert("chart.addElement",json!({"chart":chart.id,"element":"trendline","series":series,"on":false}));
            }
        }
        ui.ctx().data_mut(|d|d.insert_temp(id,draft));
    });
}

fn chart_pane_title(app: &SheetApp, ctx: &egui::Context) -> &'static str {
    use crate::chartview::ChartElement;
    match app.selected_chart.map(|id| crate::chartview::selected_element(ctx, id)) {
        Some(ChartElement::XAxis | ChartElement::YAxis) => "Format Axis",
        Some(ChartElement::Series(_)) => "Format Data Series",
        Some(ChartElement::Title) => "Format Chart Title",
        Some(ChartElement::XAxisTitle | ChartElement::YAxisTitle) => "Format Axis Title",
        Some(ChartElement::Legend) => "Format Legend",
        Some(ChartElement::Gridlines) => "Format Gridlines",
        Some(ChartElement::DataLabels) => "Format Data Labels",
        Some(ChartElement::TrendlineText) => "Format Trendline Labels",
        Some(ChartElement::PlotArea) => "Format Plot Area",
        _ => "Format Chart Area",
    }
}

fn chart_section(ui: &mut Ui, label: &str, body: impl FnOnce(&mut Ui)) {
    let title = egui::RichText::new(crate::widgets::tr(ui, label)).strong().color(Tokens::get(ui.ctx()).accent);
    egui::CollapsingHeader::new(title).id_salt(label).default_open(true).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        body(ui);
    });
}

fn chart_surface(ui: &mut Ui, surface: &mut gridcraft_engine::model::ChartSurface) -> bool {
    let mut changed = false;
    chart_section(ui, "Chart Fill|Fill", |ui| {
        changed |= chart_fill(ui, &mut surface.fill);
    });
    chart_section(ui, "Chart|Border", |ui| {
        changed |= chart_color(ui, "Border Color", &mut surface.border);
        changed |= chart_number_f32(ui, "Border Width (pt; 0 = no border)", &mut surface.border_width);
    });
    changed
}

fn chart_stroke(ui: &mut Ui, style: &mut gridcraft_engine::model::ChartStroke) -> bool {
    let mut changed = chart_color(ui, "Color", &mut style.color);
    changed |= chart_number_f32(ui, "Line Width (pt)", &mut style.width);
    changed
}

fn chart_tick_picker(ui: &mut Ui, label: &str, value: &mut Option<String>) -> bool {
    ui.label(crate::widgets::tr(ui, label));
    let old = value.clone();
    egui::ComboBox::from_id_salt(label)
        .selected_text(crate::widgets::tr(
            ui,
            match value.as_deref() {
                Some("in") => "Inside",
                Some("out") => "Outside",
                Some("cross") => "Cross",
                _ => "None",
            },
        ))
        .show_ui(ui, |ui| {
            for (key, label) in [("none", "None"), ("in", "Inside"), ("out", "Outside"), ("cross", "Cross")] {
                ui.selectable_value(value, Some(key.into()), crate::widgets::tr(ui, label));
            }
        });
    old != *value
}

/// Commit RGB/HEX and transparency edits as commands, rather than one undo entry per slider frame.
fn chart_fill(ui: &mut Ui, value: &mut Option<[u8; 4]>) -> bool {
    let mut kind = match value {
        None => 0,
        Some(c) if c[3] == 0 => 1,
        _ => 2,
    };
    let old = kind;
    for (i, label) in [(0, "Automatic"), (1, "No Fill"), (2, "Solid Fill")] {
        ui.radio_value(&mut kind, i, crate::widgets::tr(ui, label));
    }
    if kind != old {
        *value = match kind {
            0 => None,
            1 => Some([0, 0, 0, 0]),
            _ => Some([128, 128, 128, 255]),
        };
    }
    let mut changed = kind != old;
    if kind == 2 {
        changed |= chart_color(ui, "Color", value);
    }
    changed
}

fn chart_color(ui: &mut Ui, label: &str, value: &mut Option<[u8; 4]>) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.allocate_ui_with_layout(vec2((ui.available_width() - 76.0).max(40.0), 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(crate::widgets::tr(ui, label)).wrap());
            });
            let preview = value.map(|c| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])).unwrap_or(Tokens::get(ui.ctx()).text_dim);
            ui.menu_button(egui::RichText::new("■  ▾").color(preview).size(18.0), |ui| {
                let colors = ui
                    .ctx()
                    .data_mut(|d| d.get_temp::<[u32; 12]>(egui::Id::new("chart_theme_colors")))
                    .unwrap_or_else(|| gridcraft_engine::model::Theme::default().colors);
                if let Some(color) = crate::widgets::color_palette(ui, &colors, crate::widgets::tr(ui, "Automatic")) {
                    *value = if color == "none" {
                        None
                    } else {
                        gridcraft_engine::model::Color::from_hex(&color)
                            .and_then(|c| c.resolve(&Default::default()))
                            .map(|c| [c[0], c[1], c[2], value.map(|v| v[3]).unwrap_or(255)])
                    };
                    changed = true;
                    ui.close();
                }
                ui.separator();
                ui.label(crate::widgets::tr(ui, "HEX Color"));
                let text = value.map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])).unwrap_or_default();
                if let Some(text) = crate::widgets::committed_text(ui, ui.id().with("hex"), &text, Some(120.0)) {
                    if text.trim().is_empty() {
                        *value = None;
                        changed = true;
                    } else if let Some(c) = gridcraft_engine::model::Color::from_hex(&text).and_then(|c| c.resolve(&Default::default())) {
                        *value = Some([c[0], c[1], c[2], value.map(|v| v[3]).unwrap_or(255)]);
                        changed = true;
                    }
                }
            })
            .response
            .on_hover_text(crate::widgets::tr(ui, "Choose Color…"));
        });
        if let Some(mut c) = *value {
            let slider_id = ui.id().with("transparency_draft");
            let mut percent = ui.ctx().data_mut(|d| d.get_temp::<f64>(slider_id)).unwrap_or((255 - c[3]) as f64 * 100.0 / 255.0);
            let response = ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%").text(crate::widgets::tr(ui, "Transparency")));
            let commit = response.drag_stopped() || (response.changed() && !ui.input(|i| i.pointer.any_down()));
            if commit {
                ui.ctx().data_mut(|d| d.remove::<f64>(slider_id));
                c[3] = ((100.0 - percent) * 255.0 / 100.0).round() as u8;
                *value = Some(c);
                changed = true;
            } else if response.dragged() || response.has_focus() {
                ui.ctx().data_mut(|d| d.insert_temp(slider_id, percent));
            } else {
                ui.ctx().data_mut(|d| d.remove::<f64>(slider_id));
            }
        }
    });
    changed
}

fn chart_number(ui: &mut Ui, label: &str, value: &mut Option<f64>) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            let short = match label {
                "Marker Size (2–72 pt)" => "Size",
                "Line Width (pt)" | "Border Width (pt)" | "Marker Border Width (pt)" | "Border Width (pt; 0 = no border)" => "Width",
                "Gap Width (0–500%)" => "Gap Width",
                "Series Overlap (-100–100%)" => "Series Overlap",
                _ => label,
            };
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.allocate_ui_with_layout(
                vec2((ui.available_width() - 180.0).max(40.0), 24.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add(egui::Label::new(crate::widgets::tr(ui, short)).wrap()).on_hover_text(crate::widgets::tr(ui, label));
                },
            );
            let text = value.map(|v| v.to_string()).unwrap_or_default();
            if let Some(draft) = crate::widgets::committed_text(ui, ui.id().with("value"), &text, Some(62.0)) {
                let draft = draft.trim().replace(',', ".");
                if draft.is_empty() {
                    *value = None;
                    changed = true;
                } else if let Ok(n) = draft.parse::<f64>()
                    && n.is_finite()
                {
                    *value = Some(n);
                    changed = true;
                }
            }
            let (default, min, max, step) = if label == "Marker Size (2–72 pt)" {
                (5.0, 2.0, 72.0, 1.0)
            } else if label.contains("(pt") {
                (0.75, 0.0, 20.0, 0.25)
            } else if label == "Gap Width (0–500%)" {
                (150.0, 0.0, 500.0, 1.0)
            } else if label == "Series Overlap (-100–100%)" {
                (0.0, -100.0, 100.0, 1.0)
            } else if matches!(label, "Major Unit" | "Minor Unit") {
                (1.0, 0.000001, 1e100, 0.1)
            } else {
                (0.0, -1e100, 1e100, 1.0)
            };
            for (caption, delta) in [("−", -step), ("+", step)] {
                if ui.small_button(caption).clicked() {
                    *value = Some((value.unwrap_or(default) + delta).clamp(min, max));
                    changed = true;
                }
            }
            if ui.small_button(crate::widgets::tr(ui, if value.is_some() { "Reset" } else { "Auto" })).clicked() {
                *value = None;
                changed = true;
            }
        });
    });
    changed
}

fn chart_number_f32(ui: &mut Ui, label: &str, value: &mut Option<f32>) -> bool {
    let mut n = value.map(f64::from);
    if chart_number(ui, label, &mut n) {
        *value = n.map(|v| v as f32);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod chart_numeric_tests {
    use super::*;
    use egui_kittest::kittest::Queryable as _;
    #[test]
    fn chart_marker_step_buttons_work_from_automatic_and_respect_bounds() {
        let mut h = egui_kittest::Harness::builder().build_ui_state(
            |ui, value: &mut Option<f64>| {
                chart_number(ui, "Marker Size (2–72 pt)", value);
            },
            None,
        );
        h.run_steps(2);
        h.get_by_label("+").click();
        h.run_steps(2);
        assert_eq!(*h.state(), Some(6.0));
        h.get_by_label("−").click();
        h.run_steps(2);
        assert_eq!(*h.state(), Some(5.0));
        *h.state_mut() = Some(72.0);
        h.run_steps(2);
        h.get_by_label("+").click();
        h.run_steps(2);
        assert_eq!(*h.state(), Some(72.0));
        h.get_by_label("Reset").click();
        h.run_steps(2);
        assert_eq!(*h.state(), None);
        h.get_by_label("−").click();
        h.run_steps(2);
        assert_eq!(*h.state(), Some(4.0));
    }
}
