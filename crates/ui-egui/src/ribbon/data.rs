use super::controls::{act, control_keytip, keytip, menu_items, sep};
use super::*;

pub(super) fn data(app: &mut SheetApp, ui: &mut Ui) {
    let gd = big_button(ui, Icon::Folder, "Get Data\n(Text/CSV)", "Import a text or CSV file", false);
    control_keytip(app, ui, &gd, "data.getData");
    if gd.clicked() {
        app.open_dialog("open", json!({}));
    }
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
