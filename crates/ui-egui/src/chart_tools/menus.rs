//! Chart context actions and the shared chart-element menu.
use crate::{SheetApp, chartview::ChartElement};
use serde_json::{Value as Json, json};

/// The same element hierarchy is used by the ribbon and chart context menu.
pub(crate) fn element_menu(app: &mut SheetApp, ui: &mut egui::Ui, id: u32) {
    let Some(chart) = app.selected_chart_snapshot().filter(|c| c.id == id) else { return };
    let lang = app.ui.language;
    if ui.button(lang.tr("Chart Title…")).clicked() {
        app.open_dialog("chartTitle", json!({"chart":id}));
        ui.close();
    }
    ui.menu_button(lang.tr("Axis Titles"), |ui| {
        for (label, key, element) in
            [("Primary Horizontal", "xTitle", ChartElement::XAxisTitle), ("Primary Vertical", "yTitle", ChartElement::YAxisTitle)]
        {
            let on = if key == "xTitle" { chart.x_title.is_some() } else { chart.y_title.is_some() };
            if ui.selectable_label(on, lang.tr(label)).clicked() {
                let value = if on { Json::Null } else { json!(if key == "xTitle" { "Horizontal Axis" } else { "Vertical Axis" }) };
                let mut p = json!({"chart":id});
                p[key] = value;
                app.run_or_alert("chart.set", p);
                crate::chartview::select_element(ui.ctx(), id, element);
                ui.close();
            }
        }
    });
    if ui.selectable_label(chart.data_labels, lang.tr("Data Labels")).clicked() {
        app.run_or_alert("chart.set", json!({"chart":id,"dataLabels":!chart.data_labels}));
        ui.close();
    }
    ui.menu_button(lang.tr("Data Table"), |ui| {
        for (label, params) in [
            ("With Legend Keys", json!({"chart":id,"element":"dataTable","dataTableKeys":true})),
            ("No Legend Keys", json!({"chart":id,"element":"dataTable","dataTableKeys":false})),
            ("None", json!({"chart":id,"element":"dataTable","on":false})),
        ] {
            if ui.button(lang.tr(label)).clicked() {
                app.run_or_alert("chart.addElement", params);
                ui.close();
            }
        }
    });
    ui.menu_button(lang.tr("Gridlines"), |ui| {
        if ui.selectable_label(chart.gridlines, lang.tr("Primary Horizontal")).clicked() {
            app.run_or_alert("chart.set", json!({"chart":id,"gridlines":!chart.gridlines}));
            ui.close();
        }
        if ui.selectable_label(chart.format.vertical_gridlines, lang.tr("Primary Vertical")).clicked() {
            let mut f = chart.format.clone();
            f.vertical_gridlines = !f.vertical_gridlines;
            app.run_or_alert("chart.set", json!({"chart":id,"format":f}));
            ui.close();
        }
    });
    ui.menu_button(lang.tr("Legend"),|ui| {
        for (label,pos) in [("None","none"),("Bottom","bottom"),("Top","top"),("Left","left"),("Right","right")] {
            if ui.button(lang.tr(label)).clicked() {app.run_or_alert("chart.set",json!({"chart":id,"legend":pos}));ui.close();}
        }
        if ui.selectable_label(chart.format.legend_overlay,lang.tr("Overlay Legend")).clicked() {
            let mut f=chart.format.clone();f.legend_overlay = !f.legend_overlay;
            app.run_or_alert("chart.set",json!({"chart":id,"legend":if chart.legend==gridcraft_engine::model::LegendPos::None {"right"} else {
                match chart.legend {gridcraft_engine::model::LegendPos::Top=>"top",gridcraft_engine::model::LegendPos::Left=>"left",gridcraft_engine::model::LegendPos::Bottom=>"bottom",_=>"right"}
            },"format":f}));ui.close();
        }
    });
    if gridcraft_chart::supports_trendline(chart.kind) && ui.button(lang.tr("Trendline…")).clicked() {
        crate::chartview::select_element(ui.ctx(), id, ChartElement::TrendlineText);
        app.grid.pane = Some("formatChart".into());
        ui.close();
    }
}

/// Chart-only actions; reports whether the containing object popup should close.
pub(crate) fn context_menu(app: &mut SheetApp, ui: &mut egui::Ui, id: u32) -> bool {
    let lang = app.ui.language;
    let mut close = false;
    ui.separator();
    if ui.button(lang.tr("Select Data…")).clicked() {
        app.open_dialog("selectData", json!({"chart":id}));
        close = true;
    }
    ui.menu_button(lang.tr("Change Chart Type"), |ui| {
        for (label, kind) in [
            ("Column", "column"),
            ("Bar", "bar"),
            ("Line", "line"),
            ("Scatter", "scatter"),
            ("Area", "area"),
            ("Pie", "pie"),
            ("Doughnut", "doughnut"),
        ] {
            if ui.button(lang.tr(label)).clicked() {
                app.run_or_alert("chart.set", json!({"chart":id,"type":kind}));
                close = true;
                ui.close();
            }
        }
    });
    ui.menu_button(lang.tr("Add Chart Element"), |ui| element_menu(app, ui, id));
    ui.separator();
    if ui.button(lang.tr("Format Selection…")).clicked() {
        app.grid.pane = Some("formatChart".into());
        close = true;
    }
    if ui.button(lang.tr("Delete Chart")).clicked() {
        app.run_or_alert("chart.delete", json!({"chart":id}));
        app.selected_object = None;
        close = true;
    }
    close
}
