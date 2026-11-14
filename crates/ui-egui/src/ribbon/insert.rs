use super::controls::{act, anchor_keytip, control_keytip, invoke, popup_command, sep};
use super::{
    Icon, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_button, big_split_button, icons, json, small_button, small_toggle_button, vec2,
};

/// Draws the Insert tab: PivotTable, Table, Pictures, Shapes, Icons, charts, slicer, sparklines, form controls (button, slider, spin button), checkbox, links, comments, text box, header/footer and symbols.
pub(super) fn insert(app: &mut SheetApp, ui: &mut Ui) {
    let is_rtl = app.ui.language.is_rtl();
    let lang = app.ui.language;

    let ribbon_id = egui::Id::unique("insert_total_content_width");
    let last_measured_width = ui.data(|d| d.get_temp::<f32>(ribbon_id)).unwrap_or(1150.0);
    let container_width = ui.max_rect().width();
    let margin_right = 5.0;

    if is_rtl && container_width > (last_measured_width + margin_right) {
        let left_padding = container_width - last_measured_width - margin_right;
        ui.add_space(left_padding);
    }

    let start_x = ui.cursor().min.x;

    let draw_tables = |app: &mut SheetApp, ui: &mut Ui| {
        let __k = big_button(ui, Icon::PivotTable, "PivotTable", "PivotTable", false);
        control_keytip(app, ui, &__k, "insert.pivotTable");
        if __k.clicked() {
            act(app, "insert.pivotTable", json!({}));
        }
        let __k = big_button(ui, Icon::Table, "Table", "Table", false);
        control_keytip(app, ui, &__k, "insert.table");
        if __k.clicked() {
            act(app, "insert.table", json!({}));
        }
    };

    let draw_illustrations = |app: &mut SheetApp, ui: &mut Ui| {
        let pic = big_button(ui, Icon::Picture, "Pictures", "Insert a picture", true);
        anchor_keytip(app, ui, &pic, &["insert.picture"]);
        pic.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Pictures"));
        egui::Popup::menu(&pic).show(|ui| {
            let menu_w = 170.0;
            let layout = if is_rtl { egui::Layout::top_down(egui::Align::Max) } else { egui::Layout::top_down(egui::Align::Min) };
            ui.allocate_ui_with_layout(vec2(menu_w, 0.0), layout, |ui| {
                ui.set_width(menu_w);
                for (label, placement) in [("Place in Cell", "cell"), ("Place over Cells", "overCells")] {
                    let btn = egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(menu_w, 22.0));
                    if ui.add(btn).clicked() {
                        app.open_dialog("insertPicture", json!({"placement": placement}));
                        ui.close();
                    }
                }
            });
        });

        let shapes = big_button(ui, Icon::Shapes, "Shapes", "Shapes", true);
        anchor_keytip(app, ui, &shapes, &["insert.shape"]);
        egui::Popup::menu(&shapes).show(|ui| {
            let menu_w = 170.0;
            let layout = if is_rtl { egui::Layout::top_down(egui::Align::Max) } else { egui::Layout::top_down(egui::Align::Min) };
            ui.allocate_ui_with_layout(vec2(menu_w, 0.0), layout, |ui| {
                ui.set_width(menu_w);
                for (label, kind) in [
                    ("Rectangle", "rectangle"),
                    ("Rounded Rectangle", "roundedRectangle"),
                    ("Oval", "ellipse"),
                    ("Triangle", "triangle"),
                    ("Line", "line"),
                    ("Arrow", "arrow"),
                    ("Text Box", "textBox"),
                ] {
                    let btn = egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(menu_w, 22.0));
                    if ui.add(btn).clicked() {
                        act(app, "insert.shape", json!({"kind": kind}));
                        ui.close();
                    }
                }
            });
        });

        let icn = big_button(ui, Icon::Theme, "Icons", "Insert an icon", true);
        anchor_keytip(app, ui, &icn, &["insert.icons"]);
        egui::Popup::menu(&icn).show(|ui| {
            ui.set_width(260.0);
            egui::Grid::new("icon_lib").show(ui, |ui| {
                for (i, (name, icon)) in icons::LIBRARY.iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 4.0, Tokens::get(ui.ctx()).hover);
                    }
                    icons::paint(ui.painter(), r.shrink(6.0), *icon, Tokens::get(ui.ctx()).text);
                    if resp.on_hover_text(*name).clicked() {
                        act(app, "insert.icons", json!({"name": name}));
                        ui.close();
                    }
                    if (i + 1) % 6 == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    };

    let draw_charts = |app: &mut SheetApp, ui: &mut Ui| {
        let __k = big_button(ui, Icon::Chart, "Recommended\nCharts", "Recommended Charts", false);
        control_keytip(app, ui, &__k, "insert.recommendedCharts");
        if __k.clicked() {
            act(app, "insert.recommendedCharts", json!({}));
        }
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for (icon, tip, cid, subs) in [
                    (
                        Icon::ChartBar,
                        "Column or Bar",
                        "insert.chart.column",
                        vec![
                            ("Clustered Column", "column", ""),
                            ("Stacked Column", "column", "stacked"),
                            ("100% Stacked Column", "column", "stacked100"),
                            ("Clustered Bar", "bar", ""),
                            ("Stacked Bar", "bar", "stacked"),
                            ("100% Stacked Bar", "bar", "stacked100"),
                        ],
                    ),
                    (
                        Icon::ChartLine,
                        "Line or Area",
                        "insert.chart.line",
                        vec![
                            ("Line", "line", ""),
                            ("Line with Markers", "line", "markers"),
                            ("Stacked Line", "line", "stacked"),
                            ("Area", "area", ""),
                            ("Stacked Area", "area", "stacked"),
                            ("100% Stacked Area", "area", "stacked100"),
                        ],
                    ),
                    (Icon::ChartPie, "Pie or Doughnut", "insert.chart.pie", vec![("Pie", "pie", ""), ("Doughnut", "doughnut", "")]),
                ] {
                    let r = small_button(ui, icon, "", tip, true);
                    control_keytip(app, ui, &r, cid);
                    let forced_c = invoke(app, cid);
                    egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
                        let menu_w = 180.0;
                        let layout = if is_rtl { egui::Layout::top_down(egui::Align::Max) } else { egui::Layout::top_down(egui::Align::Min) };
                        ui.allocate_ui_with_layout(vec2(menu_w, 0.0), layout, |ui| {
                            ui.set_width(menu_w);
                            for (label, k, s) in &subs {
                                let btn = egui::Button::new(lang.tr(*label)).frame_when_inactive(false).min_size(vec2(menu_w, 22.0));
                                if ui.add(btn).clicked() {
                                    act(app, "insert.chart", json!({"type": k, "subtype": s}));
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            });
            ui.horizontal(|ui| {
                for (icon, tip, cid, subs) in [
                    (
                        Icon::ChartScatter,
                        "Scatter or Bubble",
                        "insert.chart.scatter",
                        vec![("Scatter", "scatter", ""), ("Scatter with Lines", "scatter", "lines"), ("Bubble", "bubble", "")],
                    ),
                    (Icon::Chart, "Statistic", "insert.chart.statistic", vec![("Histogram", "histogram", ""), ("Box & Whisker", "boxWhisker", "")]),
                    (
                        Icon::Theme,
                        "Hierarchy & more",
                        "insert.chart.hierarchy",
                        vec![
                            ("Treemap", "treemap", ""),
                            ("Sunburst", "sunburst", ""),
                            ("Waterfall", "waterfall", ""),
                            ("Funnel", "funnel", ""),
                            ("Radar", "radar", ""),
                            ("Stock", "stock", ""),
                            ("Combo", "combo", ""),
                        ],
                    ),
                ] {
                    let r = small_button(ui, icon, "", tip, true);
                    control_keytip(app, ui, &r, cid);
                    let forced_c = invoke(app, cid);
                    egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
                        let menu_w = 170.0;
                        let layout = if is_rtl { egui::Layout::top_down(egui::Align::Max) } else { egui::Layout::top_down(egui::Align::Min) };
                        ui.allocate_ui_with_layout(vec2(menu_w, 0.0), layout, |ui| {
                            ui.set_width(menu_w);
                            for (label, k, s) in &subs {
                                let btn = egui::Button::new(lang.tr(*label)).frame_when_inactive(false).min_size(vec2(menu_w, 22.0));
                                if ui.add(btn).clicked() {
                                    act(app, "insert.chart", json!({"type": k, "subtype": s}));
                                    ui.close();
                                }
                            }
                        });
                    });
                }
            });
        });
    };

    let draw_slicer = |app: &mut SheetApp, ui: &mut Ui| {
        if big_button(ui, Icon::Filter, "Slicer", "Insert a slicer to filter a table", false).clicked() {
            act(app, "insert.slicer", json!({}));
        }
    };

    let draw_sparklines = |app: &mut SheetApp, ui: &mut Ui| {
        let sp = big_button(ui, Icon::Sparkline, "Sparklines", "Sparklines", true);
        anchor_keytip(app, ui, &sp, &["dialog:sparkline"]);
        egui::Popup::menu(&sp).show(|ui| {
            let menu_w = 140.0;
            let layout = if is_rtl { egui::Layout::top_down(egui::Align::Max) } else { egui::Layout::top_down(egui::Align::Min) };
            ui.allocate_ui_with_layout(vec2(menu_w, 0.0), layout, |ui| {
                ui.set_width(menu_w);
                for (label, kind) in [("Line", "line"), ("Column", "column"), ("Win/Loss", "winLoss")] {
                    let btn = egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(menu_w, 22.0));
                    if ui.add(btn).clicked() {
                        app.open_dialog("sparkline", json!({"type": kind}));
                        ui.close();
                    }
                }
            });
        });
    };

    let draw_form_controls = |app: &mut SheetApp, ui: &mut Ui| {
        use gridcraft_engine::model::ControlKind;
        let placing = app.grid.placing;
        ui.vertical(|ui| {
            for (icon, kind, label, tip, id) in [
                (Icon::Button, ControlKind::Button, "Button", "Draw a button that runs a macro or a command", "insert.button"),
                (Icon::Slider, ControlKind::Slider, "Slider", "Draw a slider that sets a whole number in a linked cell", "insert.slider"),
                (Icon::Spin, ControlKind::Spin, "Spin Button", "Draw a spin button that steps a whole number in a linked cell", "insert.spinButton"),
            ] {
                let on = placing == Some(kind);
                let r = small_toggle_button(ui, icon, label, tip, on);
                control_keytip(app, ui, &r, id);
                if r.clicked() {
                    app.grid.placing = if placing == Some(kind) { None } else { Some(kind) };
                    app.selected_object = None;
                }
            }
        });
    };

    let draw_checkbox = |app: &mut SheetApp, ui: &mut Ui| {
        if big_button(ui, Icon::Checkbox, "Checkbox", "Insert checkboxes in the selected cells", false).clicked() {
            act(app, "insert.checkbox", json!({}));
        }
    };

    let draw_links_and_comments = |app: &mut SheetApp, ui: &mut Ui| {
        let __k = big_button(ui, Icon::Link, "Link", "Insert Link", false);
        control_keytip(app, ui, &__k, "dialog:insertLink");
        if __k.clicked() {
            app.open_dialog("insertLink", json!({}));
        }
        let __k = big_button(ui, Icon::Comment, "Comment", "New Comment", false);
        control_keytip(app, ui, &__k, "dialog:comment");
        if __k.clicked() {
            app.open_dialog("comment", json!({"threaded": true}));
        }
    };

    let draw_text = |app: &mut SheetApp, ui: &mut Ui| {
        let __k = big_button(ui, Icon::TextBox, "Text\nBox", "Text Box", false);
        control_keytip(app, ui, &__k, "insert.textBox");
        if __k.clicked() {
            act(app, "insert.textBox", json!({}));
        }
        let wa_label = lang.tr("Decorative Text").replacen(' ', "\n", 1);
        let wa = big_button(ui, Icon::WordArt, &wa_label, "Decorative Text", true);
        anchor_keytip(app, ui, &wa, &["insert.wordArt"]);
        egui::Popup::menu(&wa).show(|ui| crate::wordart::gallery(app, ui));

        let __k = big_button(ui, Icon::PageLayout, "Header &\nFooter", "Header & Footer", false);
        control_keytip(app, ui, &__k, "dialog:headerFooter");
        if __k.clicked() {
            crate::paper_view::start_editing(app);
        }
    };

    let draw_symbols = |app: &mut SheetApp, ui: &mut Ui| {
        let (eq, eq_menu) = big_split_button(ui, Icon::Equation, "Equation", "Insert an equation, or pick one from the gallery");
        control_keytip(app, ui, &eq, "dialog:equation");
        if eq.clicked() {
            app.open_dialog("equation", json!({}));
        }
        egui::Popup::menu(&eq_menu).show(|ui| {
            ui.set_width(300.0);
            if ui.add(egui::Button::new(lang.tr("Insert New Equation...")).frame(false).min_size(vec2(290.0, 24.0))).clicked() {
                app.open_dialog("equation", json!({}));
                ui.close();
            }
            ui.separator();
            if let Some(latex) = crate::equation::gallery(ui) {
                act(app, "insert.equation", json!({"latex": latex}));
                ui.close();
            }
        });
        let sym = big_button(ui, Icon::Symbol, "Symbol", "Symbol", true);
        control_keytip(app, ui, &sym, "insert.symbol");
        let forced = invoke(app, "insert.symbol");
        egui::Popup::menu(&sym).open_memory(popup_command(forced, sym.clicked())).show(|ui| {
            ui.set_width(260.0);
            ui.horizontal_wrapped(|ui| {
                for ch in [
                    "€", "£", "¥", "©", "®", "™", "±", "≠", "≤", "≥", "÷", "×", "∞", "µ", "α", "β", "π", "Σ", "Ω", "°", "•", "…", "→", "←", "↑", "↓",
                    "✓", "★", "½", "¼",
                ] {
                    if crate::widgets::glyph_button(ui, ch.chars().next().unwrap_or('?'), &format!("Insert {ch}")).clicked() {
                        act(app, "insert.symbol", json!({"char": ch}));
                    }
                }
            });
        });
    };

    if is_rtl {
        draw_symbols(app, ui);
        sep(ui);
        draw_text(app, ui);
        sep(ui);
        draw_links_and_comments(app, ui);
        sep(ui);
        draw_checkbox(app, ui);
        sep(ui);
        draw_form_controls(app, ui);
        sep(ui);
        draw_sparklines(app, ui);
        sep(ui);
        draw_slicer(app, ui);
        sep(ui);
        draw_charts(app, ui);
        sep(ui);
        draw_illustrations(app, ui);
        sep(ui);
        draw_tables(app, ui);
    } else {
        draw_tables(app, ui);
        sep(ui);
        draw_illustrations(app, ui);
        sep(ui);
        draw_charts(app, ui);
        sep(ui);
        draw_slicer(app, ui);
        sep(ui);
        draw_sparklines(app, ui);
        sep(ui);
        draw_form_controls(app, ui);
        sep(ui);
        draw_checkbox(app, ui);
        sep(ui);
        draw_links_and_comments(app, ui);
        sep(ui);
        draw_text(app, ui);
        sep(ui);
        draw_symbols(app, ui);
    }

    if is_rtl {
        let total_width = ui.cursor().min.x - start_x;
        ui.data_mut(|d| d.insert_temp(ribbon_id, total_width));
    }


    }

    pub(super) fn draw(app: &mut SheetApp, ui: &mut Ui) {
    let is_rtl = app.ui.language.is_rtl();
    let ribbon_id = egui::Id::unique("draw_total_content_width");
    let last_measured_width = ui.data(|d| d.get_temp::<f32>(ribbon_id)).unwrap_or(480.0);
    let container_width = ui.max_rect().width();
    let margin_right = 5.0;

    if is_rtl && container_width > (last_measured_width + margin_right) {
        let left_padding = container_width - last_measured_width - margin_right;
        ui.add_space(left_padding);
    }

    let start_x = ui.cursor().min.x;
    let tool = app.session.draw_tool.clone();

    let draw_lasso = |app: &mut SheetApp, ui: &mut Ui| {
        let __k = big_button(ui, Icon::Shapes, "Select\nObjects", "Select and move ink and shapes", false);
        control_keytip(app, ui, &__k, "draw.lasso");
        if __k.clicked() {
            act(app, "draw.lasso", json!({}));
        }
    };

    let draw_pens = |app: &mut SheetApp, ui: &mut Ui| {
        let pens = [("Pen\nBlue", "#1F5FC9"), ("Pen\nBlack", "#000000"), ("Pen\nRed", "#D13B2F"), ("Highlighter", "#F2E33A")];
        let render_pen = |(label, color): (&str, &str), app: &mut SheetApp, ui: &mut Ui| {
            let on = app.session.draw_tool == "pen" && app.session.draw_color.eq_ignore_ascii_case(color);
            let r = big_button(ui, Icon::Pen, label, "Draw with ink", false);
            if on {
                ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
            }
            if r.clicked() {
                let width = if label == "Highlighter" { 12.0 } else { 2.0 };
                act(app, "draw.pen", json!({"on": !on, "color": color, "width": width}));
            }
        };

        if is_rtl {
            for pen in pens.into_iter().rev() {
                render_pen(pen, app, ui);
            }
        } else {
            for pen in pens {
                render_pen(pen, app, ui);
            }
        }
    };

    let draw_eraser = |app: &mut SheetApp, ui: &mut Ui| {
        let er = big_button(ui, Icon::Eraser, "Eraser", "Erase ink strokes", false);
        if tool == "eraser" {
            ui.painter().rect_stroke(er.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
        }
        if er.clicked() {
            act(app, "draw.eraser", json!({}));
        }
    };

    let draw_convert = |app: &mut SheetApp, ui: &mut Ui| {
        let __k = big_button(ui, Icon::Shapes, "Ink to\nShape", "Convert the last ink stroke to a shape", false);
        control_keytip(app, ui, &__k, "draw.inkToShape");
        if __k.clicked() {
            act(app, "draw.inkToShape", json!({}));
        }
    };

    if is_rtl {
        draw_convert(app, ui);
        sep(ui);
        draw_eraser(app, ui);
        draw_pens(app, ui);
        sep(ui);
        draw_lasso(app, ui);
    } else {
        draw_lasso(app, ui);
        sep(ui);
        draw_pens(app, ui);
        draw_eraser(app, ui);
        sep(ui);
        draw_convert(app, ui);
    }

    if is_rtl {
        let total_width = ui.cursor().min.x - start_x;
        ui.data_mut(|d| d.insert_temp(ribbon_id, total_width));
    }

}