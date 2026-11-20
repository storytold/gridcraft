use super::controls::{act, control_keytip, keytip, menu_items, sep};
use super::{Icon, SheetApp, Ui, big_button, json, keytip_table, small_button};

pub(super) fn data(app: &mut SheetApp, ui: &mut Ui) {
    let is_rtl = app.ui.language.is_rtl();
    let ribbon_id = egui::Id::unique("data_total_content_width");
    let last_measured_width = ui.data(|d| d.get_temp::<f32>(ribbon_id)).unwrap_or(1100.0);
    let container_width = ui.max_rect().width();
    let margin_right = 5.0;

    if is_rtl && container_width > (last_measured_width + margin_right) {
        let left_padding = container_width - last_measured_width - margin_right;
        ui.add_space(left_padding);
    }

    let start_x = ui.cursor().min.x;

    let draw_get_data = |app: &mut SheetApp, ui: &mut Ui| {
        let gd = big_button(ui, Icon::Folder, "Get Data\n(Text/CSV)", "Import a text or CSV file", false);
        control_keytip(app, ui, &gd, "data.getData");
        if gd.clicked() {
            app.open_dialog("open", json!({}));
        }
    };

    let draw_sort_filter = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_quick_sort = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        let draw_sort_dialog = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::SortFilter, "Sort", "Custom Sort", false);
            control_keytip(app, ui, &__k, "dialog:sort");
            if __k.clicked() {
                app.open_dialog("sort", json!({}));
            }
        };

        let draw_filter = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Filter, "Filter", "Filter", false);
            control_keytip(app, ui, &__k, "data.filter");
            if __k.clicked() {
                act(app, "data.filter", json!({}));
            }
        };

        let draw_filter_helpers = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        if is_rtl {
            draw_filter_helpers(app, ui);
            draw_filter(app, ui);
            draw_sort_dialog(app, ui);
            draw_quick_sort(app, ui);
        } else {
            draw_quick_sort(app, ui);
            draw_sort_dialog(app, ui);
            draw_filter(app, ui);
            draw_filter_helpers(app, ui);
        }
    };

    let draw_data_tools = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_text_to_columns = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::TextColumns, "Text to\nColumns", "Text to Columns", false);
            control_keytip(app, ui, &__k, "dialog:textToColumns");
            if __k.clicked() {
                app.open_dialog("textToColumns", json!({}));
            }
        };

        let draw_flash_fill = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::FillDown, "Flash\nFill", "Flash Fill", false);
            control_keytip(app, ui, &__k, "edit.flashFill");
            if __k.clicked() {
                act(app, "edit.flashFill", json!({}));
            }
        };

        let draw_remove_duplicates = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Duplicates, "Remove\nDuplicates", "Remove Duplicates", false);
            control_keytip(app, ui, &__k, "dialog:removeDuplicates");
            if __k.clicked() {
                app.open_dialog("removeDuplicates", json!({}));
            }
        };

        let draw_data_validation = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        if is_rtl {
            draw_data_validation(app, ui);
            draw_remove_duplicates(app, ui);
            draw_flash_fill(app, ui);
            draw_text_to_columns(app, ui);
        } else {
            draw_text_to_columns(app, ui);
            draw_flash_fill(app, ui);
            draw_remove_duplicates(app, ui);
            draw_data_validation(app, ui);
        }
    };

    let draw_forecast = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_what_if = |app: &mut SheetApp, ui: &mut Ui| {
            let wi = big_button(ui, Icon::Calc, "What-If\nAnalysis", "What-If Analysis", true);
            for sequence in keytip_table::group_extension(
                app.keytips.prefix().unwrap_or_default(),
                &["dialog:goalSeek", "data.scenarioManager", "data.dataTable"],
            ) {
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
        };

        let draw_forecast_sheet = |app: &mut SheetApp, ui: &mut Ui| {
            if big_button(ui, Icon::ChartLine, "Forecast\nSheet", "Forecast Sheet", false).clicked() {
                app.open_dialog("forecastSheet", json!({}));
            }
        };

        if is_rtl {
            draw_forecast_sheet(app, ui);
            draw_what_if(app, ui);
        } else {
            draw_what_if(app, ui);
            draw_forecast_sheet(app, ui);
        }
    };

    let draw_outline = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_group_ungroup = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        let draw_detail_settings = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        if is_rtl {
            draw_detail_settings(app, ui);
            draw_group_ungroup(app, ui);
        } else {
            draw_group_ungroup(app, ui);
            draw_detail_settings(app, ui);
        }
    };

    let draw_analysis = |app: &mut SheetApp, ui: &mut Ui| {
        if big_button(ui, Icon::ChartBar, "Data\nAnalysis", "Data Analysis", false).clicked() {
            app.open_dialog("analysisToolPak", json!({}));
        }
    };

    let draw_solver = |app: &mut SheetApp, ui: &mut Ui| {
        if big_button(ui, Icon::Function, "Solver", "Solver", false).clicked() {
            app.open_dialog("solver", json!({}));
        }
    };

    if is_rtl {
        draw_solver(app, ui);
        sep(ui);
        draw_analysis(app, ui);
        sep(ui);
        draw_outline(app, ui);
        sep(ui);
        draw_forecast(app, ui);
        sep(ui);
        draw_data_tools(app, ui);
        sep(ui);
        draw_sort_filter(app, ui);
        sep(ui);
        draw_get_data(app, ui);
    } else {
        draw_get_data(app, ui);
        sep(ui);
        draw_sort_filter(app, ui);
        sep(ui);
        draw_data_tools(app, ui);
        sep(ui);
        draw_forecast(app, ui);
        sep(ui);
        draw_outline(app, ui);
        sep(ui);
        draw_analysis(app, ui);
        sep(ui);
        draw_solver(app, ui);
    }

    if is_rtl {
        let total_width = ui.cursor().min.x - start_x;
        ui.data_mut(|d| d.insert_temp(ribbon_id, total_width));
    }
}