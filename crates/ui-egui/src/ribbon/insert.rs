use super::controls::{act, anchor_keytip, control_keytip, invoke, popup_command, sep};
use super::{Icon, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_button, icons, json, small_button, vec2};

/// Draws the Insert tab: PivotTable, Table, Pictures, Shapes, Icons, charts, slicer, sparklines, checkbox, links, comments, text box, header/footer and symbols.
pub(super) fn insert(app: &mut SheetApp, ui: &mut Ui) {
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
    sep(ui);
    let pic = big_button(ui, Icon::Picture, "Pictures", "Insert a picture", true);
    anchor_keytip(app, ui, &pic, &["insert.picture"]);
    pic.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Pictures"));
    egui::Popup::menu(&pic).show(|ui| {
        for (label, placement) in [("Place in Cell", "cell"), ("Place over Cells", "overCells")] {
            if ui.button(label).clicked() {
                app.open_dialog("insertPicture", json!({"placement": placement}));
                ui.close();
            }
        }
    });
    let shapes = big_button(ui, Icon::Shapes, "Shapes", "Shapes", true);
    anchor_keytip(app, ui, &shapes, &["insert.shape"]);
    egui::Popup::menu(&shapes).show(|ui| {
        for (label, kind) in [
            ("Rectangle", "rectangle"),
            ("Rounded Rectangle", "roundedRectangle"),
            ("Oval", "ellipse"),
            ("Triangle", "triangle"),
            ("Line", "line"),
            ("Arrow", "arrow"),
            ("Text Box", "textBox"),
        ] {
            if ui.button(label).clicked() {
                act(app, "insert.shape", json!({"kind": kind}));
            }
        }
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
    sep(ui);
    let __k = big_button(ui, Icon::Chart, "Recommended\nCharts", "Recommended Charts", false);
    control_keytip(app, ui, &__k, "insert.recommendedCharts");
    if __k.clicked() {
        // Menu-style: with no chart type, run_or_alert opens the Insert Chart dialog.
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
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
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
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
    });
    sep(ui);
    if big_button(ui, Icon::Filter, "Slicer", "Insert a slicer to filter a table", false).clicked() {
        act(app, "insert.slicer", json!({}));
    }
    sep(ui);
    let sp = big_button(ui, Icon::Sparkline, "Sparklines", "Sparklines", true);
    anchor_keytip(app, ui, &sp, &["dialog:sparkline"]);
    egui::Popup::menu(&sp).show(|ui| {
        for (label, kind) in [("Line", "line"), ("Column", "column"), ("Win/Loss", "winLoss")] {
            if ui.button(label).clicked() {
                app.open_dialog("sparkline", json!({"type": kind}));
            }
        }
    });
    sep(ui);
    // Cell Controls.
    if big_button(ui, Icon::Checkbox, "Checkbox", "Insert checkboxes in the selected cells", false).clicked() {
        act(app, "insert.checkbox", json!({}));
    }
    sep(ui);
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
    sep(ui);
    let __k = big_button(ui, Icon::TextBox, "Text\nBox", "Text Box", false);
    control_keytip(app, ui, &__k, "insert.textBox");
    if __k.clicked() {
        act(app, "insert.textBox", json!({}));
    }
    let __k = big_button(ui, Icon::PageLayout, "Header &\nFooter", "Header & Footer", false);
    control_keytip(app, ui, &__k, "dialog:headerFooter");
    if __k.clicked() {
        // As in Excel: Page Layout view, editing the header in place (the dialog is under Page Setup).
        crate::paper_view::start_editing(app);
    }
    let sym = big_button(ui, Icon::Symbol, "Symbol", "Symbol", true);
    control_keytip(app, ui, &sym, "insert.symbol");
    egui::Popup::menu(&sym).show(|ui| {
        ui.set_width(260.0);
        ui.horizontal_wrapped(|ui| {
            for ch in [
                "€", "£", "¥", "©", "®", "™", "±", "≠", "≤", "≥", "÷", "×", "∞", "µ", "α", "β", "π", "Σ", "Ω", "°", "•", "…", "→", "←", "↑", "↓",
                "✓", "★", "½", "¼",
            ] {
                // No bundled face has ✓: the button paints a code-drawn check instead of a tofu box.
                if crate::widgets::glyph_button(ui, ch.chars().next().unwrap_or('?'), &format!("Insert {ch}")).clicked() {
                    act(app, "insert.symbol", json!({"char": ch}));
                }
            }
        });
    });
}

pub(super) fn draw(app: &mut SheetApp, ui: &mut Ui) {
    let tool = app.session.draw_tool.clone();
    let __k = big_button(ui, Icon::Shapes, "Select\nObjects", "Select and move ink and shapes", false);
    control_keytip(app, ui, &__k, "draw.lasso");
    if __k.clicked() {
        act(app, "draw.lasso", json!({}));
    }
    sep(ui);
    for (label, color) in [("Pen\nBlue", "#1F5FC9"), ("Pen\nBlack", "#000000"), ("Pen\nRed", "#D13B2F"), ("Highlighter", "#F2E33A")] {
        let on = tool == "pen" && app.session.draw_color.eq_ignore_ascii_case(color);
        let r = big_button(ui, Icon::Pen, label, "Draw with ink", false);
        if on {
            ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
        }
        if r.clicked() {
            let width = if label == "Highlighter" { 12.0 } else { 2.0 };
            act(app, "draw.pen", json!({"on": !on, "color": color, "width": width}));
        }
    }
    let er = big_button(ui, Icon::Eraser, "Eraser", "Erase ink strokes", false);
    if tool == "eraser" {
        ui.painter().rect_stroke(er.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if er.clicked() {
        act(app, "draw.eraser", json!({}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::Shapes, "Ink to\nShape", "Convert the last ink stroke to a shape", false);
    control_keytip(app, ui, &__k, "draw.inkToShape");
    if __k.clicked() {
        act(app, "draw.inkToShape", json!({}));
    }
}
