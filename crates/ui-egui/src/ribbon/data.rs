use super::controls::{act, control_keytip, keytip, menu_items, sep};
use super::{Icon, SheetApp, Ui, big_button, big_split_button, json, keytip_table, pos2, small_button, vec2};

/// What a Get Data menu item does when it is enabled.
#[derive(Clone, Copy)]
enum Source {
    /// Pick a file (or folder) of this kind, then the Navigator.
    File(&'static str),
    /// Open this dialog with these params.
    Dialog(&'static str, &'static str),
    /// Run the command without loading, then open its draft in the Power Query Editor.
    Edit,
    /// Open the Power Query Editor.
    Editor,
    /// Run the command menu-style (it shows its own dialog, or says why not).
    Run,
}

/// A Get Data menu item: disabled with the command's reason when it can't run here.
fn source_item(app: &mut SheetApp, ui: &mut Ui, label: &str, id: &str, then: Source) {
    let lang = app.ui.language;
    let state = gridcraft_engine::find_command(id).map(|c| (c.enabled)(&app.session));
    let text = lang.tr(label);
    let response = match state {
        Some(Err(why)) => {
            ui.add_enabled(false, egui::Button::new(text).frame(false).min_size(vec2(240.0, 22.0))).on_disabled_hover_text(lang.tr(&why).to_string());
            return;
        }
        _ => ui.add(egui::Button::new(text).frame(false).min_size(vec2(240.0, 22.0))),
    };
    if let Some(sequence) = keytip_table::item_letter(app.keytips.prefix().unwrap_or_default(), id) {
        keytip(app, ui, sequence, pos2(response.rect.right() - 10.0, response.rect.center().y));
    }
    if !response.clicked() {
        return;
    }
    ui.close();
    match then {
        Source::File(kind) => app.open_dialog("importFile", json!({"kind": kind})),
        Source::Dialog(name, flag) => {
            let mut params = serde_json::Map::new();
            if !flag.is_empty() {
                params.insert(flag.to_string(), json!(true));
            }
            let params = serde_json::Value::Object(params);
            app.open_dialog(name, params);
        }
        Source::Edit => match app.run(id, json!({"load": "none"})) {
            Ok(r) => {
                if let Some(q) = r.get("query").filter(|q| q.is_object()) {
                    crate::power_query::open_editor(app, None, Some(q.clone()));
                }
            }
            Err(e) => app.message = Some((lang.tr(label).to_string(), crate::clean_error(&e))),
        },
        Source::Editor => crate::power_query::open_editor(app, None, None),
        Source::Run => act(app, id, json!({})),
    }
}

/// Data › Get Data: Excel's menu, in its order. Sources GridCraft can't reach stay listed,
/// disabled with the reason.
fn get_data_menu(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    ui.menu_button(lang.tr("From File"), |ui| {
        source_item(app, ui, "From Excel Workbook", "data.fromExcelWorkbook", Source::File("excel"));
        source_item(app, ui, "From Text/CSV", "data.fromTextCsv", Source::File("csv"));
        source_item(app, ui, "From XML", "data.fromXml", Source::File("xml"));
        source_item(app, ui, "From JSON", "data.fromJson", Source::File("json"));
        source_item(app, ui, "From PDF", "data.fromPdf", Source::Run);
        source_item(app, ui, "From Folder", "data.fromFolder", Source::File("folder"));
        source_item(app, ui, "From SharePoint Folder", "data.fromSharePointFolder", Source::Run);
    });
    ui.menu_button(lang.tr("From Database"), |ui| {
        source_item(app, ui, "From SQL Server Database", "data.fromSqlServer", Source::Dialog("fromSqlServer", ""));
        source_item(app, ui, "From Microsoft Access Database", "data.fromAccess", Source::File("access"));
        source_item(app, ui, "From Analysis Services", "data.fromAnalysisServices", Source::Run);
        source_item(app, ui, "From SQL Server Analysis Services Database (Import)", "data.fromAnalysisServicesImport", Source::Run);
    });
    ui.menu_button(lang.tr("From Azure"), |ui| {
        for label in [
            "From Azure SQL Database",
            "From Azure Synapse Analytics (SQL DW)",
            "From Azure Analysis Services",
            "From Azure Blob Storage",
            "From Azure Table Storage",
            "From Azure Data Lake Storage",
            "From Azure HDInsight (HDFS)",
            "From Azure Data Explorer (Kusto)",
        ] {
            source_item(app, ui, label, "data.fromAzure", Source::Run);
        }
    });
    ui.menu_button(lang.tr("From Power Platform"), |ui| {
        for label in ["From Power BI semantic model", "From Dataflows", "From Dataverse"] {
            source_item(app, ui, label, "data.fromPowerPlatform", Source::Run);
        }
    });
    ui.menu_button(lang.tr("From Online Services"), |ui| {
        for label in [
            "From SharePoint Online List",
            "From Microsoft Exchange Online",
            "From Dynamics 365 (online)",
            "From Salesforce Objects",
            "From Salesforce Reports",
        ] {
            source_item(app, ui, label, "data.fromOnlineServices", Source::Run);
        }
    });
    ui.menu_button(lang.tr("From Other Sources"), |ui| {
        source_item(app, ui, "From Table/Range", "data.fromTableRange", Source::Edit);
        source_item(app, ui, "From Web", "data.fromWeb", Source::Dialog("fromWeb", ""));
        source_item(app, ui, "From Microsoft Query", "data.fromMicrosoftQuery", Source::Run);
        source_item(app, ui, "From OData Feed", "data.fromODataFeed", Source::Dialog("fromWeb", "odata"));
        source_item(app, ui, "From ODBC", "data.fromOdbc", Source::Dialog("fromOdbc", ""));
        source_item(app, ui, "From OLEDB", "data.fromOledb", Source::Run);
        source_item(app, ui, "Blank Query", "data.blankQuery", Source::Edit);
    });
    ui.separator();
    ui.menu_button(lang.tr("Combine Queries"), |ui| {
        source_item(app, ui, "Merge", "data.mergeQueries", Source::Dialog("mergeQueries", ""));
        source_item(app, ui, "Append", "data.appendQueries", Source::Dialog("appendQueries", ""));
    });
    ui.separator();
    source_item(app, ui, "Launch Power Query Editor…", "data.powerQueryEditor", Source::Editor);
    source_item(app, ui, "Data Source Settings…", "data.dataSourceSettings", Source::Dialog("dataSourceSettings", ""));
    source_item(app, ui, "Query Options", "data.queryOptions", Source::Dialog("queryOptions", ""));
}

pub(super) fn data(app: &mut SheetApp, ui: &mut Ui) {
    // Get & Transform Data
    let gd = big_button(ui, Icon::Database, "Get\nData", "Get Data", true);
    control_keytip(app, ui, &gd, "data.getData");
    egui::Popup::menu(&gd).show(|ui| get_data_menu(app, ui));
    ui.vertical(|ui| {
        let k = small_button(ui, Icon::FileName, "From Text/CSV", "From Text/CSV", false);
        control_keytip(app, ui, &k, "data.fromTextCsv");
        if k.clicked() {
            app.open_dialog("importFile", json!({"kind": "csv"}));
        }
        let web = gridcraft_engine::find_command("data.fromWeb").map(|c| (c.enabled)(&app.session));
        let k = ui.add_enabled_ui(!matches!(web, Some(Err(_))), |ui| small_button(ui, Icon::Globe, "From Web", "From Web", false)).inner;
        let k = match web {
            Some(Err(why)) => k.on_disabled_hover_text(app.ui.language.tr(&why).to_string()),
            _ => k,
        };
        control_keytip(app, ui, &k, "data.fromWeb");
        if k.clicked() {
            app.open_dialog("fromWeb", json!({}));
        }
        let k = small_button(ui, Icon::Table, "From Table/Range", "From Table/Range", false);
        control_keytip(app, ui, &k, "data.fromTableRange");
        if k.clicked() {
            match app.run("data.fromTableRange", json!({"load": "none"})) {
                Ok(r) => {
                    if let Some(q) = r.get("query").filter(|q| q.is_object()) {
                        crate::power_query::open_editor(app, None, Some(q.clone()));
                    }
                }
                Err(e) => app.message = Some((app.ui.language.tr("From Table/Range").to_string(), crate::clean_error(&e))),
            }
        }
    });
    ui.vertical(|ui| {
        let k = small_button(ui, Icon::Clock, "Recent Sources", "Recent Sources", false);
        control_keytip(app, ui, &k, "data.recentSources");
        if k.clicked() {
            app.open_dialog("recentSources", json!({}));
        }
        let k = small_button(ui, Icon::Link, "Existing Connections", "Existing Connections", false);
        control_keytip(app, ui, &k, "data.existingConnections");
        if k.clicked() {
            app.open_dialog("existingConnections", json!({}));
        }
    });
    sep(ui);
    // Queries & Connections
    let (refresh_all, refresh_menu) = big_split_button(ui, Icon::Refresh, "Refresh\nAll", "Refresh All (Ctrl+Alt+F5)");
    for sequence in keytip_table::group_extension(
        app.keytips.prefix().unwrap_or_default(),
        &["data.refreshAll", "data.refresh", "data.refreshStatus", "data.cancelRefresh", "data.connectionProperties"],
    ) {
        keytip(app, ui, sequence, refresh_menu.rect.center_bottom());
    }
    if refresh_all.clicked() {
        act(app, "data.refreshAll", json!({}));
    }
    egui::Popup::menu(&refresh_menu).show(|ui| {
        source_item(app, ui, "Refresh All", "data.refreshAll", Source::Run);
        source_item(app, ui, "Refresh", "data.refresh", Source::Run);
        let status = ui.add(egui::Button::new(app.ui.language.tr("Refresh Status")).frame(false).min_size(vec2(240.0, 22.0)));
        if status.clicked() {
            ui.close();
            app.open_dialog("refreshStatus", json!({}));
        }
        source_item(app, ui, "Cancel Refresh", "data.cancelRefresh", Source::Run);
        source_item(app, ui, "Connection Properties…", "data.connectionProperties", Source::Run);
    });
    ui.vertical(|ui| {
        let on = app.session.queries_pane;
        let k = small_button(ui, Icon::Query, "Queries & Connections", "Queries & Connections", false);
        control_keytip(app, ui, &k, "data.queriesConnections");
        if k.clicked() {
            act(app, "data.queriesConnections", json!({"on": !on}));
        }
        let props = gridcraft_engine::find_command("data.properties").map(|c| (c.enabled)(&app.session));
        let k = ui.add_enabled_ui(!matches!(props, Some(Err(_))), |ui| small_button(ui, Icon::Settings, "Properties", "Properties", false)).inner;
        control_keytip(app, ui, &k, "data.properties");
        if k.clicked() {
            act(app, "data.properties", json!({}));
        }
    });
    sep(ui);
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::SortAsc, "", "Sort A to Z", false);
        control_keytip(app, ui, &__k, "data.sortAscending");
        if __k.clicked() {
            act(app, "data.sortAscending", json!({}));
        }
        let __k = small_button(ui, Icon::SortDesc, "", "Sort Z to A", false);
        control_keytip(app, ui, &__k, "data.sortDescending");
        if __k.clicked() {
            act(app, "data.sortDescending", json!({}));
        }
    });
    let __k = big_button(ui, Icon::SortFilter, "Sort", "Custom Sort", false);
    control_keytip(app, ui, &__k, "dialog:sort");
    if __k.clicked() {
        app.open_dialog("sort", json!({}));
    }
    let __k = big_button(ui, Icon::Filter, "Filter", "Filter", false);
    control_keytip(app, ui, &__k, "data.filter");
    if __k.clicked() {
        act(app, "data.filter", json!({}));
    }
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Eraser, "Clear", "Clear Filter", false);
        control_keytip(app, ui, &__k, "data.clearFilter");
        if __k.clicked() {
            act(app, "data.clearFilter", json!({}));
        }
        let __k = small_button(ui, Icon::Redo, "Reapply", "Reapply", false);
        control_keytip(app, ui, &__k, "data.reapply");
        if __k.clicked() {
            act(app, "data.reapply", json!({}));
        }
        let __k = small_button(ui, Icon::Filter, "Advanced", "Advanced Filter", false);
        control_keytip(app, ui, &__k, "dialog:advancedFilter");
        if __k.clicked() {
            app.open_dialog("advancedFilter", json!({}));
        }
    });
    sep(ui);
    let __k = big_button(ui, Icon::TextColumns, "Text to\nColumns", "Text to Columns", false);
    control_keytip(app, ui, &__k, "dialog:textToColumns");
    if __k.clicked() {
        app.open_dialog("textToColumns", json!({}));
    }
    let __k = big_button(ui, Icon::FillDown, "Flash\nFill", "Flash Fill", false);
    control_keytip(app, ui, &__k, "edit.flashFill");
    if __k.clicked() {
        act(app, "edit.flashFill", json!({}));
    }
    let __k = big_button(ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", false);
    control_keytip(app, ui, &__k, "dialog:removeDuplicates");
    if __k.clicked() {
        app.open_dialog("removeDuplicates", json!({}));
    }
    let dv = big_button(ui, Icon::Validation, "Data\nValidation", "Data Validation", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["data.validation", "data.circleInvalid"]) {
        keytip(app, ui, sequence, dv.rect.center_bottom());
    }
    egui::Popup::menu(&dv).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Data Validation…", "dialog:dataValidation", json!({})),
                ("Circle Invalid Data", "data.circleInvalid", json!({})),
                ("Clear Validation", "data.validation", json!({"clear": true})),
            ],
        );
    });
    sep(ui);
    let wi = big_button(ui, Icon::Calc, "What-If\nAnalysis", "What-If Analysis", true);
    for sequence in
        keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["dialog:goalSeek", "data.scenarioManager", "data.dataTable"])
    {
        keytip(app, ui, sequence, wi.rect.center_bottom());
    }
    egui::Popup::menu(&wi).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Goal Seek…", "dialog:goalSeek", json!({})),
                ("Scenario Manager…", "data.scenarioManager", json!({})),
                ("Data Table…", "data.dataTable", json!({})),
            ],
        );
    });
    if big_button(ui, Icon::ChartLine, "Forecast\nSheet", "Forecast Sheet", false).clicked() {
        app.open_dialog("forecastSheet", json!({}));
    }
    sep(ui);
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Group, "Group", "Group", false);
        control_keytip(app, ui, &__k, "data.group");
        if __k.clicked() {
            act(app, "data.group", json!({}));
        }
        let __k = small_button(ui, Icon::Ungroup, "Ungroup", "Ungroup", false);
        control_keytip(app, ui, &__k, "data.ungroup");
        if __k.clicked() {
            act(app, "data.ungroup", json!({}));
        }
        let __k = small_button(ui, Icon::Subtotal, "Subtotal", "Subtotal", false);
        control_keytip(app, ui, &__k, "dialog:subtotal");
        if __k.clicked() {
            app.open_dialog("subtotal", json!({}));
        }
    });
    ui.vertical(|ui| {
        if small_button(ui, Icon::Plus, "Show Detail", "Show Detail", false).clicked() {
            act(app, "data.showDetail", json!({}));
        }
        if small_button(ui, Icon::Minus, "Hide Detail", "Hide Detail", false).clicked() {
            act(app, "data.hideDetail", json!({}));
        }
        if small_button(ui, Icon::Settings, "Settings", "Outline Settings", false).clicked() {
            app.open_dialog("outlineSettings", json!({}));
        }
    });
    sep(ui);
    if big_button(ui, Icon::ChartBar, "Data\nAnalysis", "Data Analysis", false).clicked() {
        app.open_dialog("analysisToolPak", json!({}));
    }
    sep(ui);
    if big_button(ui, Icon::Function, "Solver", "Solver", false).clicked() {
        app.open_dialog("solver", json!({}));
    }
}
