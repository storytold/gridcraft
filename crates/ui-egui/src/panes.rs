//! Right-hand task panes: Comments, Watch Window, Selection Pane, Format Chart.

use egui::{Color32, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::l10n::{Arg, Tr, msg};
use crate::theme::{self, Tokens};

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let Some(pane) = app.grid.pane.clone() else { return };
    let t = Tokens::get(ui.ctx());
    let l = app.l10n;
    egui::Panel::right("task_pane").default_size(300.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let title = match pane.as_str() {
                "comments" => l.tr("Comments"),
                "watch" => l.tr("Watch Window"),
                "selection" => l.tr("Selection"),
                "formatChart" => l.tr("Format Chart Area"),
                _ => std::borrow::Cow::Borrowed(""),
            };
            ui.label(egui::RichText::new(title).font(theme::ui_bold(15.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::icon_button(ui, crate::icons::Icon::Close, t.text_dim, &l.tr("Close"), vec2(22.0, 22.0)).clicked() {
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

fn comments(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let Some(d) = app.session.active() else { return };
    let Some(sh) = d.wb.active() else { return };
    let list: Vec<(String, gridcraft_engine::model::Comment)> = sh.comments.iter().map(|(c, m)| (c.a1(), m.clone())).collect();
    if list.is_empty() {
        ui.label(egui::RichText::new(l.tr("No comments on this sheet yet. Select a cell and choose New Comment.")).italics());
    }
    for (cell, c) in list {
        egui::Frame::NONE
            .fill(if c.resolved { Color32::from_gray(240) } else { Color32::WHITE })
            .corner_radius(6.0)
            .inner_margin(8.0)
            .stroke(egui::Stroke::new(1.0, Color32::from_gray(215)))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.link(egui::RichText::new(&cell).strong()).clicked() {
                        let _ = app.run("selection.set", json!({"cell": cell}));
                        app.grid.ensure_visible = true;
                    }
                    ui.label(egui::RichText::new(&c.author).color(Color32::from_gray(90)));
                    if c.resolved {
                        ui.label(egui::RichText::new(l.tr("Resolved")).small().color(Color32::from_rgb(0x10, 0x7C, 0x41)));
                    }
                });
                ui.label(&c.text);
                for (a, txt) in &c.replies {
                    ui.label(egui::RichText::new(format!("↳ {a}: {txt}")).color(Color32::from_gray(60)));
                }
                let key = egui::Id::new(("reply", &cell));
                let mut draft: String = ui.ctx().data_mut(|m| m.get_temp(key)).unwrap_or_default();
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut draft).hint_text(l.tr("Reply…")).desired_width(ui.available_width() - 120.0));
                    if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || ui.small_button(l.tr("Send")).clicked())
                        && !draft.is_empty()
                    {
                        app.run_or_alert("review.replyComment", json!({"cell": cell, "text": draft}));
                        draft.clear();
                    }
                    if ui.small_button(if c.resolved { l.tr("Reopen") } else { l.tr("Resolve") }).clicked() {
                        app.run_or_alert("review.resolveComment", json!({"cell": cell, "resolved": !c.resolved}));
                    }
                    if ui.small_button(l.tr("Delete")).clicked() {
                        app.run_or_alert("review.deleteComment", json!({"cell": cell}));
                    }
                });
                ui.ctx().data_mut(|m| m.insert_temp(key, draft));
            });
        ui.add_space(6.0);
    }
}

fn watch(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    if ui.button(l.tr("Add Watch (active cell)")).clicked()
        && let Some(d) = app.session.active()
        && let Some(sh) = d.wb.active()
    {
        let r = format!("{}!{}", gridcraft_engine::formula::quote_sheet(&sh.name), d.selection.active.a1());
        let _ = app.session.run("formulas.watchWindow", json!({"add": r}));
    }
    let list = app.session.run("formulas.watchWindow", json!({})).unwrap_or(Json::Null);
    egui::Grid::new("watch_grid").striped(true).num_columns(4).show(ui, |ui| {
        ui.strong(l.tr("Cell"));
        ui.strong(l.tr("Value"));
        ui.strong(l.tr("Formula"));
        ui.label("");
        ui.end_row();
        for w in list.as_array().cloned().unwrap_or_default() {
            let cell = w["cell"].as_str().unwrap_or("").to_string();
            ui.label(&cell);
            ui.label(w["text"].as_str().or_else(|| w["value"].as_str()).unwrap_or(""));
            ui.label(w["formulaLocal"].as_str().or_else(|| w["formula"].as_str()).unwrap_or(""));
            if ui.small_button("✕").clicked() {
                let _ = app.session.run("formulas.watchWindow", json!({"remove": cell}));
            }
            ui.end_row();
        }
    });
}

fn selection(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let list = app.session.run("arrange.selectionPane", json!({})).unwrap_or(Json::Null);
    let items = list.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        ui.label(egui::RichText::new(l.tr("There are no shapes, pictures or charts on this sheet.")).italics());
    }
    for it in items.iter().rev() {
        let kind = it["kind"].as_str().unwrap_or("").to_string();
        let id = it["id"].as_u64().unwrap_or(0) as u32;
        ui.horizontal(|ui| {
            let sel = app.selected_chart == Some(id);
            if ui.selectable_label(sel, it["name"].as_str().unwrap_or("")).clicked() {
                app.selected_chart = Some(id);
            }
            if ui.small_button("▲").on_hover_text(l.tr("Bring Forward")).clicked() {
                app.run_or_alert("arrange.bringForward", json!({"kind": kind, "id": id}));
            }
            if ui.small_button("▼").on_hover_text(l.tr("Send Backward")).clicked() {
                app.run_or_alert("arrange.sendBackward", json!({"kind": kind, "id": id}));
            }
            if ui.small_button("✕").on_hover_text(l.tr("Delete")).clicked() {
                app.run_or_alert("object.delete", json!({"kind": kind, "id": id}));
            }
        });
    }
}

fn format_chart(app: &mut SheetApp, ui: &mut Ui) {
    let l = app.l10n;
    let Some(id) = app.selected_chart else {
        ui.label(egui::RichText::new(l.tr("Select a chart to format it.")).italics());
        return;
    };
    let Some(chart) = app.session.active().and_then(|d| d.wb.active().and_then(|s| s.charts.iter().find(|c| c.id == id).cloned())) else {
        ui.label(l.tr("The chart no longer exists."));
        return;
    };
    let theme = app.session.active().map(|d| d.wb.theme.clone()).unwrap_or_default();
    ui.collapsing(l.tr("Chart Title"), |ui| {
        let mut title = chart.title.clone().unwrap_or_default();
        if ui.text_edit_singleline(&mut title).lost_focus() {
            app.run_or_alert("chart.set", json!({"chart": id, "title": title}));
        }
    });
    ui.collapsing(l.tr("Chart Type"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for (label, k, s) in [
                (msg!("Column"), "column", ""),
                (msg!("Stacked"), "column", "stacked"),
                (msg!("Bar"), "bar", ""),
                (msg!("Line"), "line", "markers"),
                (msg!("Area"), "area", ""),
                (msg!("Pie"), "pie", ""),
                (msg!("Doughnut"), "doughnut", ""),
                (msg!("Scatter"), "scatter", ""),
                (msg!("Radar"), "radar", ""),
            ] {
                if ui.button(l.tr(label)).clicked() {
                    app.run_or_alert("chart.set", json!({"chart": id, "type": k, "subtype": s}));
                }
            }
        });
    });
    ui.collapsing(l.tr("Legend"), |ui| {
        ui.horizontal(|ui| {
            for (label, v) in
                [(msg!("None"), "none"), (msg!("Bottom"), "bottom"), (msg!("Top"), "top"), (msg!("Left"), "left"), (msg!("Right"), "right")]
            {
                let cur = format!("{:?}", chart.legend).eq_ignore_ascii_case(v);
                if ui.selectable_label(cur, l.tr(label)).clicked() {
                    app.run_or_alert("chart.set", json!({"chart": id, "legend": v}));
                }
            }
        });
    });
    ui.collapsing(l.tr("Elements"), |ui| {
        let mut dl = chart.data_labels;
        if ui.checkbox(&mut dl, l.tr("Data Labels")).changed() {
            app.run_or_alert("chart.set", json!({"chart": id, "dataLabels": dl}));
        }
        let mut gl = chart.gridlines;
        if ui.checkbox(&mut gl, l.tr("Gridlines")).changed() {
            app.run_or_alert("chart.set", json!({"chart": id, "gridlines": gl}));
        }
        let mut xt = chart.x_title.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(l.tr("Horizontal axis title:"));
            if ui.text_edit_singleline(&mut xt).lost_focus() {
                app.run_or_alert("chart.set", json!({"chart": id, "xTitle": if xt.is_empty() { Json::Null } else { json!(xt) }}));
            }
        });
        let mut yt = chart.y_title.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(l.tr("Vertical axis title:"));
            if ui.text_edit_singleline(&mut yt).lost_focus() {
                app.run_or_alert("chart.set", json!({"chart": id, "yTitle": if yt.is_empty() { Json::Null } else { json!(yt) }}));
            }
        });
    });
    ui.collapsing(l.tr("Series"), |ui| {
        let data = app.session.active().map(|d| gridcraft_chart::resolve(&d.wb, d.wb.active_sheet, &chart));
        for (i, s) in chart.series.iter().enumerate() {
            let name = data
                .as_ref()
                .and_then(|d| d.series.get(i))
                .map(|x| x.name.clone())
                .unwrap_or_else(|| l.text("ui-pane-series-number", &[("n", Arg::from(i + 1))]).into_owned());
            let col =
                data.as_ref().and_then(|d| d.series.get(i)).map(|x| Color32::from_rgb(x.color[0], x.color[1], x.color[2])).unwrap_or(Color32::GRAY);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, col);
                ui.label(&name);
                ui.label(egui::RichText::new(&s.values).small().color(Color32::from_gray(110)));
                ui.menu_button(l.tr("Fill…"), |ui| {
                    if let Some(c) = crate::widgets::color_palette(ui, l, &theme.colors, &l.tr("Automatic")) {
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
    ui.collapsing(l.tr("Colors & Layout"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for (p, label) in [
                ("colorful", msg!("colorful")),
                ("monochrome", msg!("monochrome")),
                ("accent1", msg!("accent1")),
                ("accent2", msg!("accent2")),
                ("accent3", msg!("accent3")),
                ("accent4", msg!("accent4")),
                ("accent5", msg!("accent5")),
                ("accent6", msg!("accent6")),
            ] {
                if ui.button(l.tr(label)).clicked() {
                    app.run_or_alert("chart.changeColors", json!({"chart": id, "palette": p}));
                }
            }
        });
        ui.horizontal(|ui| {
            for n in 1..=6 {
                if ui.button(l.text("ui-pane-layout-number", &[("n", Arg::from(n))])).clicked() {
                    app.run_or_alert("chart.quickLayout", json!({"chart": id, "layout": n}));
                }
            }
        });
    });
    let numbers = app.number_style();
    ui.collapsing(l.tr("Size"), |ui| {
        let mut w = chart.anchor.width;
        let mut h = chart.anchor.height;
        let a = ui.add(numbers.drag(&mut w).prefix(format!("{} ", l.tr("Width"))).range(40.0..=4000.0));
        let b = ui.add(numbers.drag(&mut h).prefix(format!("{} ", l.tr("Height"))).range(40.0..=4000.0));
        if a.drag_stopped() || a.lost_focus() || b.drag_stopped() || b.lost_focus() {
            app.run_or_alert("chart.set", json!({"chart": id, "width": w, "height": h}));
        }
    });
}
