use super::controls::{act, anchor_keytip, control_keytip, keytip, menu_items, sep};
use super::galleries::{paint_table_swatch, table_gallery};
use super::{Color32, Icon, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_button, icon_button, json, keytip_table, small_button, vec2};

/// Draws the Table Design tab: rename, resize, remove duplicates, convert to range, slicer, external
/// table data, table options and the style strip.
pub(super) fn table_design(app: &mut SheetApp, ui: &mut Ui) {
    let Some((tname, header, totals, banded, banded_c, first, last, filter, style, external)) = app.session.active().and_then(|d| {
        let sh = d.wb.active()?;
        let t = sh.table_at(d.selection.active)?;
        Some((
            t.name.clone(),
            t.header_row,
            t.totals_row,
            t.banded_rows,
            t.banded_cols,
            t.first_col,
            t.last_col,
            t.filter_button,
            t.style.clone(),
            t.is_external(),
        ))
    }) else {
        return;
    };
    ui.vertical(|ui| {
        ui.label(egui::RichText::new("Table Name:").small());
        let field = egui::Id::unique(("gridcraft.table_name", &tname));
        if let Some(name) = crate::widgets::committed_text(ui, field, &tname, Some(120.0))
            && name != tname
        {
            act(app, "table.rename", json!({"table": tname, "name": name}));
        }
    });
    let __k = big_button(ui, Icon::Table, "Resize\nTable", "Resize Table", false);
    control_keytip(app, ui, &__k, "dialog:tableResize");
    if __k.clicked() {
        app.open_dialog("tableResize", json!({}));
    }
    sep(ui);
    external_table_data(app, ui, &tname, external);
    sep(ui);
    let __k = big_button(ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", false);
    control_keytip(app, ui, &__k, "data.removeDuplicates");
    if __k.clicked() {
        act(app, "data.removeDuplicates", json!({}));
    }
    let __k = big_button(ui, Icon::Table, "Convert\nto Range", "Convert to Range", false);
    control_keytip(app, ui, &__k, "table.convertToRange");
    if __k.clicked() {
        act(app, "table.convertToRange", json!({"table": tname}));
    }
    sep(ui);
    if big_button(ui, Icon::Filter, "Insert\nSlicer", "Insert Slicer", false).clicked() {
        act(app, "table.insertSlicer", json!({"table": tname}));
    }
    sep(ui);
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            ("Header Row", "table.headerRow", header && filter),
            ("Total Row", "table.totalRow", totals),
            ("Banded Rows", "table.bandedRows", banded),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, label).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    ui.vertical(|ui| {
        for (label, cmd, v) in [
            ("First Column", "table.firstColumn", first),
            ("Last Column", "table.lastColumn", last),
            ("Banded Columns", "table.bandedColumns", banded_c),
        ] {
            let mut on = v;
            if ui.checkbox(&mut on, label).changed() {
                act(app, cmd, json!({"table": tname, "on": on}));
            }
        }
    });
    sep(ui);
    table_style_strip(app, ui, &tname, &style);
}

/// The first page of Excel's Table Styles gallery, drawn in the ribbon. The More arrow opens the
/// full gallery (Light, Medium, Dark) as a popup.
const RIBBON_TABLE_STYLES: [&str; 7] =
    ["TableStyleLight1", "TableStyleLight2", "TableStyleLight3", "TableStyleLight4", "TableStyleLight5", "TableStyleLight6", "TableStyleLight7"];

fn table_style_strip(app: &mut SheetApp, ui: &mut Ui, tname: &str, current: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        // Borrow the workbook only while painting; the session must be free for `act` afterwards.
        let clicked = {
            let Some(d) = app.session.active() else { return };
            let wb = &d.wb;
            let mut clicked = None;
            for name in RIBBON_TABLE_STYLES {
                let (r, resp) = ui.allocate_exact_size(vec2(52.0, 40.0), Sense::click());
                paint_table_swatch(ui, r, wb, name);
                if name == current {
                    ui.painter().rect_stroke(r.expand(2.5), 2.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
                }
                if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.0), 2.0, Stroke::new(2.0, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                }
                if resp.on_hover_text(name).clicked() {
                    clicked = Some(name);
                }
            }
            clicked
        };
        if let Some(name) = clicked {
            act(app, "table.style", json!({"table": tname, "style": name}));
        }
        let more = icon_button(ui, Icon::Chevron, t.text_dim, "More Table Styles", vec2(18.0, 40.0));
        anchor_keytip(app, ui, &more, &["table.style"]);
        egui::Popup::menu(&more).show(|ui| table_gallery(app, ui, "table.style"));
    });
}

/// External Table Data, in Excel's position and order. A plain table has no connection, so every
/// control is disabled, as in Excel. Unlink is real for a table that carries external metadata
/// (an XML-map table); refresh, properties and Open in Browser wait for the connection model.
fn external_table_data(app: &mut SheetApp, ui: &mut Ui, tname: &str, external: bool) {
    let refresh =
        ui.add_enabled_ui(false, |ui| big_button(ui, Icon::Refresh, "Refresh", "Refresh needs a supported external data connection", false)).inner;
    let _ = refresh.on_disabled_hover_text("Refresh needs a supported external data connection");
    ui.vertical(|ui| {
        let props = ui
            .add_enabled_ui(false, |ui| small_button(ui, Icon::Format, "Properties", "External data properties need a supported connection", false))
            .inner;
        let _ = props.on_disabled_hover_text("External data properties need a supported connection");
        let open = ui.add_enabled_ui(false, |ui| small_button(ui, Icon::Link, "Open in Browser", "No server URL for this table", false)).inner;
        let _ = open.on_disabled_hover_text("No server URL for this table");
        let mut unlink =
            ui.add_enabled_ui(external, |ui| small_button(ui, Icon::Unlink, "Unlink", "Unlink the table from its external data", false)).inner;
        if !external {
            unlink = unlink.on_disabled_hover_text("The table has no external data connection");
        }
        if unlink.clicked() {
            act(app, "table.unlink", json!({"table": tname}));
        }
    });
}

pub(super) fn chart_design(app: &mut SheetApp, ui: &mut Ui) {
    let Some(id) = app.selected_chart() else { return };
    let ae = big_button(ui, Icon::Chart, "Add Chart\nElement", "Add Chart Element", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["chart.set"]) {
        keytip(app, ui, sequence, ae.rect.center_bottom());
    }
    egui::Popup::menu(&ae).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Legend: Bottom", "chart.set", json!({"chart": id, "legend": "bottom"})),
                ("Legend: Right", "chart.set", json!({"chart": id, "legend": "right"})),
                ("Legend: Top", "chart.set", json!({"chart": id, "legend": "top"})),
                ("Legend: None", "chart.set", json!({"chart": id, "legend": "none"})),
                ("-", "", json!(null)),
                ("Data Labels: Show", "chart.set", json!({"chart": id, "dataLabels": true})),
                ("Data Labels: None", "chart.set", json!({"chart": id, "dataLabels": false})),
                ("Data Table: With Legend Keys", "chart.addElement", json!({"chart": id, "element": "dataTable", "dataTableKeys": true})),
                ("Data Table: No Legend Keys", "chart.addElement", json!({"chart": id, "element": "dataTable", "dataTableKeys": false})),
                ("Data Table: None", "chart.addElement", json!({"chart": id, "element": "dataTable", "on": false})),
                ("Gridlines: Show", "chart.set", json!({"chart": id, "gridlines": true})),
                ("Gridlines: None", "chart.set", json!({"chart": id, "gridlines": false})),
            ],
        );
    });
    let __k = big_button(ui, Icon::Redo, "Switch\nRow/Column", "Switch Row/Column", false);
    control_keytip(app, ui, &__k, "chart.switchRowColumn");
    if __k.clicked() {
        act(app, "chart.switchRowColumn", json!({"chart": id}));
    }
    if big_button(ui, Icon::Table, "Select\nData", "Select Data", false).clicked() {
        app.open_dialog("selectData", json!({"chart": id}));
    }
    let ct = big_button(ui, Icon::ChartBar, "Change\nChart Type", "Change Chart Type", true);
    for sequence in keytip_table::group_extension(
        app.keytips.prefix().unwrap_or_default(),
        &["column", "bar", "line", "area", "pie", "doughnut", "scatter", "radar", "waterfall", "funnel", "treemap"],
    ) {
        keytip(app, ui, sequence, ct.rect.center_bottom());
    }
    egui::Popup::menu(&ct).show(|ui| {
        for (label, k, s) in [
            ("Clustered Column", "column", ""),
            ("Stacked Column", "column", "stacked"),
            ("Bar", "bar", ""),
            ("Line", "line", "markers"),
            ("Area", "area", ""),
            ("Pie", "pie", ""),
            ("Doughnut", "doughnut", ""),
            ("Scatter", "scatter", ""),
            ("Radar", "radar", ""),
            ("Waterfall", "waterfall", ""),
            ("Funnel", "funnel", ""),
            ("Treemap", "treemap", ""),
        ] {
            if ui.button(label).clicked() {
                act(app, "chart.set", json!({"chart": id, "type": k, "subtype": s}));
            }
        }
    });
    if big_button(ui, Icon::Settings, "Format\nPane", "Format Chart pane", false).clicked() {
        app.grid.pane = Some("formatChart".into());
    }
    let __k = big_button(ui, Icon::TextBox, "Chart\nTitle", "Edit chart title", false);
    control_keytip(app, ui, &__k, "chartTitle");
    if __k.clicked() {
        app.open_dialog("chartTitle", json!({"chart": id}));
    }
    let __k = big_button(ui, Icon::Delete, "Delete\nChart", "Delete chart", false);
    control_keytip(app, ui, &__k, "chart.delete");
    if __k.clicked() {
        act(app, "chart.delete", json!({"chart": id}));
        app.selected_object = None;
    }
}
