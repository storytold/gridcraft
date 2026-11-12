use super::controls::{RibbonSections, act, control_keytip, group_end, keytip, menu_items, section, sep};
use super::{Icon, SheetApp, Ui, big_button, json, keytip_table, small_button};

pub(super) fn data(app: &mut SheetApp, ui: &mut Ui) {
    // Excel's measured Data rows: Data Tools is three rows from 1040 client points, Outline
    // from 1520.
    RibbonSections::breaks(ui, "Data Tools", &[(1040.0, 3)]);
    RibbonSections::breaks(ui, "Outline", &[(1520.0, 3)]);
    section("Data", ui, app, "Get & Transform Data", Icon::Folder, |app, ui| {
        let gd = big_button(ui, Icon::Folder, "Get Data\n(Text/CSV)", "Import a text or CSV file", false);
        control_keytip(app, ui, &gd, "data.getData");
        if gd.clicked() {
            app.open_dialog("open", json!({}));
        }
    });
    sep(ui, "Get & Transform Data");
    section("Data", ui, app, "Sort & Filter", Icon::SortFilter, |app, ui| {
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
    });
    sep(ui, "Sort & Filter");
    section("Data", ui, app, "Data Tools", Icon::Validation, |app, ui| {
        // The measured three-row form stacks the four tools as small buttons; other levels
        // keep one row of four big buttons.
        if RibbonSections::level(ui, "Data Tools") == 3 {
            // Three rows of two: the stacked columns of small buttons Excel's narrow form uses.
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    small_data_tool(app, ui, Icon::TextColumns, "Text to Columns", "dialog:textToColumns");
                    small_data_tool(app, ui, Icon::FillDown, "Flash Fill", "edit.flashFill");
                });
                ui.add_space(2.0);
                ui.vertical(|ui| {
                    small_data_tool(app, ui, Icon::Duplicates, "Remove Duplicates", "dialog:removeDuplicates");
                    small_data_validation(app, ui);
                });
            });
        } else {
            data_tool(app, ui, Icon::TextColumns, "Text to\nColumns", "Text to Columns", "dialog:textToColumns");
            data_tool(app, ui, Icon::FillDown, "Flash\nFill", "Flash Fill", "edit.flashFill");
            data_tool(app, ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", "dialog:removeDuplicates");
            data_validation(app, ui);
        }
    });
    sep(ui, "Data Tools");
    section("Data", ui, app, "Forecast", Icon::ChartLine, |app, ui| {
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
    });
    sep(ui, "Forecast");
    section("Data", ui, app, "Outline", Icon::Group, |app, ui| {
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
    });
    sep(ui, "Outline");
    section("Data", ui, app, "Analysis", Icon::ChartBar, |app, ui| {
        if big_button(ui, Icon::ChartBar, "Data\nAnalysis", "Data Analysis", false).clicked() {
            app.open_dialog("analysisToolPak", json!({}));
        }
        // Excel's Analysis group holds Data Analysis and Solver together.
        if big_button(ui, Icon::Function, "Solver", "Solver", false).clicked() {
            app.open_dialog("solver", json!({}));
        }
    });
    group_end(ui, "Analysis");
}

/// One Data Tools big button: opens `dialog` for `dialog:*` targets, else runs the command.
fn data_tool(app: &mut SheetApp, ui: &mut Ui, icon: Icon, label: &str, tip: &str, target: &str) {
    let button = big_button(ui, icon, label, tip, false);
    control_keytip(app, ui, &button, target);
    if button.clicked() {
        if let Some(dialog) = target.strip_prefix("dialog:") {
            app.open_dialog(dialog, json!({}));
        } else {
            act(app, target, json!({}));
        }
    }
}

/// The Data Validation split control with its menu.
fn data_validation(app: &mut SheetApp, ui: &mut Ui) {
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
}

/// One Data Tools small button for the stacked three-row form: opens `dialog` for `dialog:*`
/// targets, else runs the command.
fn small_data_tool(app: &mut SheetApp, ui: &mut Ui, icon: Icon, label: &str, target: &str) {
    let button = small_button(ui, icon, label, label, false);
    control_keytip(app, ui, &button, target);
    if button.clicked() {
        if let Some(dialog) = target.strip_prefix("dialog:") {
            app.open_dialog(dialog, json!({}));
        } else {
            act(app, target, json!({}));
        }
    }
}

/// The Data Validation control for the stacked three-row form, with its menu.
fn small_data_validation(app: &mut SheetApp, ui: &mut Ui) {
    let dv = small_button(ui, Icon::Validation, "Data Validation", "Data Validation", true);
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
}
