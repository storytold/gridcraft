use super::controls::{RibbonSections, act, checkbox_on, control_keytip, group_end, invoke, keytip, menu_items, popup_command, section, sep};
use super::{Icon, Rect, SheetApp, Tokens, Ui, big_button, json, keytip_table, pos2, small_button, theme, vec2};

/// Draws the View tab: view modes, ruler/gridlines/formula-bar/headings toggles, zoom, freeze panes, theme, animations, interface size and language.
pub(super) fn view(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let _ = t;
    let lang = app.ui.language;
    // Excel's measured View rows, in client points: Sheet View is one row of its two buttons
    // below 928, then two rows; Workbook Views is three tall buttons from 880 and Window from
    // 819; Show is one row of four checkboxes until 1280, then two rows.
    RibbonSections::breaks(ui, "Sheet View", &[(928.0, 2), (0.0, 1)]);
    RibbonSections::breaks(ui, "Workbook Views", &[(880.0, 3)]);
    RibbonSections::breaks(ui, "Show", &[(1280.0, 2), (0.0, 1)]);
    RibbonSections::breaks(ui, "Window", &[(819.0, 3)]);
    section("View", ui, app, "Sheet View", Icon::Sheet, |app, ui| {
        // New Sheet View over Custom Views from 928 client points, side by side below it.
        if RibbonSections::level(ui, "Sheet View") == 2 {
            ui.vertical(|ui| {
                new_sheet_view(app, ui);
                custom_views(app, ui);
            });
        } else {
            new_sheet_view(app, ui);
            custom_views(app, ui);
        }
    });
    sep(ui, "Sheet View");
    section("View", ui, app, "Workbook Views", Icon::Normal, |app, ui| {
        for (icon, label, cmd) in [
            (Icon::Normal, "Normal", "view.normal"),
            (Icon::PageBreak, "Page Break\nPreview", "view.pageBreakPreview"),
            (Icon::PageLayout, "Page\nLayout", "view.pageLayout"),
        ] {
            let __k = big_button(ui, icon, label, label, false);
            control_keytip(app, ui, &__k, cmd);
            if __k.clicked() {
                act(app, cmd, json!({}));
            }
        }
    });
    sep(ui, "Workbook Views");
    section("View", ui, app, "Show", Icon::PageLayout, |app, ui| {
        if RibbonSections::level(ui, "Show") == 1 {
            // One row of four checkboxes below 1280 client points.
            ui.horizontal(|ui| {
                show_ruler(app, ui);
                show_gridlines(app, ui);
                show_formula_bar(app, ui);
                show_headings(app, ui);
            });
        } else {
            // Two columns of two, as in Excel's Show group: four stacked rows overflow the ribbon body.
            ui.vertical(|ui| {
                show_ruler(app, ui);
                show_gridlines(app, ui);
            });
            ui.add_space(6.0);
            ui.vertical(|ui| {
                show_formula_bar(app, ui);
                show_headings(app, ui);
            });
        }
    });
    sep(ui, "Show");
    section("View", ui, app, "Zoom", Icon::Zoom, |app, ui| {
        let __k = big_button(ui, Icon::Zoom, "Zoom", "Zoom…", false);
        control_keytip(app, ui, &__k, "dialog:zoom");
        if __k.clicked() {
            app.open_dialog("zoom", json!({}));
        }
        let __k = big_button(ui, Icon::Search, "100%", "Zoom to 100%", false);
        control_keytip(app, ui, &__k, "view.zoom");
        if __k.clicked() || invoke(app, "view.zoom") {
            act(app, "view.zoom", json!({"percent": 100}));
        }
        let __k = big_button(ui, Icon::Zoom, "Zoom to\nSelection", "Zoom to Selection", false);
        control_keytip(app, ui, &__k, "view.zoomToSelection");
        if __k.clicked() {
            let r = app.grid.cells_rect.unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0)));
            act(app, "view.zoomToSelection", json!({"viewWidth": r.width(), "viewHeight": r.height()}));
        }
    });
    sep(ui, "Zoom");
    section("View", ui, app, "Window", Icon::Book, |app, ui| {
        let __k = big_button(ui, Icon::NewFile, "New\nWindow", "New Window", false);
        control_keytip(app, ui, &__k, "view.newWindow");
        if __k.clicked() {
            act(app, "view.newWindow", json!({}));
        }
        let __k = big_button(ui, Icon::Table, "Arrange\nAll", "Arrange All", false);
        control_keytip(app, ui, &__k, "view.arrangeAll");
        if __k.clicked() {
            app.open_dialog("arrangeAll", json!({}));
        }
        let fp = big_button(ui, Icon::Freeze, "Freeze\nPanes", "Freeze Panes", true);
        for sequence in keytip_table::group_extension(
            app.keytips.prefix().unwrap_or_default(),
            &["view.freezePanes", "view.freezeTopRow", "view.freezeFirstColumn"],
        ) {
            keytip(app, ui, sequence, fp.rect.center_bottom());
        }
        egui::Popup::menu(&fp).show(|ui| {
            menu_items(
                app,
                ui,
                &[
                    ("Freeze Panes", "view.freezePanes", json!({})),
                    ("Freeze Top Row", "view.freezeTopRow", json!({})),
                    ("Freeze First Column", "view.freezeFirstColumn", json!({})),
                    ("Unfreeze Panes", "view.unfreezePanes", json!({})),
                ],
            );
        });
        switch_windows(app, ui);
        ui.vertical(|ui| {
            let label_response = ui.label(lang.tr("Display theme"));
            let mode = app.ui.theme_mode();
            let label = match mode {
                "system" => "System",
                "dark" => "Dark",
                _ => "Light",
            };
            craft_ui::ComboBox::from_id_salt("display_theme")
                .selected_text(lang.tr(label))
                .width(88.0)
                .show_ui(ui, |ui| {
                    for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                        if ui.selectable_label(mode == value, lang.tr(label)).clicked() {
                            act(app, "view.theme", json!({"mode": value}));
                        }
                    }
                })
                .response
                .labelled_by(label_response.id);
            let mut animations = app.ui.animations;
            let hint = lang.tr("Glide the selection outline to the cell you move to instead of jumping there");
            if ui.checkbox(&mut animations, lang.tr("Animations")).on_hover_text(hint).changed() {
                act(app, "view.animations", json!({"on": animations}));
            }
        });
    });
    sep(ui, "Window");
    section("View", ui, app, "Interface", Icon::Settings, |app, ui| {
        // Interface size (issue #230): scales the whole window on top of the desktop's scale.
        ui.vertical(|ui| {
            let label = ui.label(egui::RichText::new(lang.tr("Interface size")).font(theme::ui_font(11.5)).color(t.text_dim));
            let current = app.ui.interface_scale;
            let text = |v: u16| if v == 100 { lang.tr("Auto").to_string() } else { format!("{v}%") };
            craft_ui::ComboBox::from_id_salt("ui_interface_size")
                .width(120.0)
                .selected_text(egui::RichText::new(text(current)).font(theme::ui_font(12.5)))
                .show_ui(ui, |ui| {
                    for v in [100, 75, 90, 110, 125, 150, 175, 200] {
                        if ui.selectable_label(v == current, text(v)).clicked() {
                            act(app, "view.interfaceScale", json!({"percent": v}));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
        // Interface language: settings live in the View tab, like Excel's Options. The label follows
        // the current language so it stays readable; the choices are always shown in their own script.
        ui.vertical(|ui| {
            let label = ui.label(egui::RichText::new(lang.tr("Interface language")).font(theme::ui_font(11.5)).color(t.text_dim));
            craft_ui::ComboBox::from_id_salt("ui_language")
                .width(120.0)
                .selected_text(egui::RichText::new(lang.name()).font(theme::ui_font(12.5)))
                .show_ui(ui, |ui| {
                    for l in crate::i18n::Language::ALL {
                        if ui.selectable_label(l == lang, l.name()).clicked() {
                            act(app, "app.language.set", json!({"language": l.code()}));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
    });
    group_end(ui, "Interface");
}

/// View › Show, Ruler: only Page Layout view has rulers.
fn show_ruler(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let page_layout =
        app.session.active().and_then(|d| d.wb.active().map(|s| s.view == gridcraft_engine::model::SheetViewMode::PageLayout)).unwrap_or(false);
    let mut ruler = !app.session.hide_ruler;
    let resp = ui.add_enabled(page_layout, egui::Checkbox::new(&mut ruler, lang.tr("Ruler")));
    control_keytip(app, ui, &resp, "view.ruler");
    if resp.changed() {
        act(app, "view.ruler", json!({"on": ruler}));
    }
}

fn show_gridlines(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let mut g = app.session.active().and_then(|d| d.wb.active().map(|s| s.show_gridlines)).unwrap_or(true);
    let resp = ui.checkbox(&mut g, lang.tr("Gridlines"));
    control_keytip(app, ui, &resp, "view.gridlines");
    if resp.changed() {
        act(app, "view.gridlines", json!({"on": g}));
    }
}

fn show_formula_bar(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let mut fb = app.session.prefs.formula_bar;
    let resp = ui.checkbox(&mut fb, lang.tr("Formula Bar"));
    control_keytip(app, ui, &resp, "view.formulaBar");
    if resp.changed() || invoke(app, "view.formulaBar") {
        act(app, "view.formulaBar", json!({"on": checkbox_on(resp.changed(), fb)}));
    }
}

fn show_headings(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let mut h = app.session.active().and_then(|d| d.wb.active().map(|s| s.show_headings)).unwrap_or(true);
    let resp = ui.checkbox(&mut h, lang.tr("Headings"));
    control_keytip(app, ui, &resp, "view.headings");
    if resp.changed() {
        act(app, "view.headings", json!({"on": h}));
    }
}

/// View › Switch Windows: the open workbooks, the active one checked and unsaved ones marked;
/// choosing one brings it forward.
fn switch_windows(app: &mut SheetApp, ui: &mut Ui) {
    let anchor = big_button(ui, Icon::Book, "Switch\nWindows", "Switch Windows", true);
    anchor.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Switch Windows"));
    control_keytip(app, ui, &anchor, "view.switchWindows");
    let forced = std::mem::take(&mut app.open_switch_windows) | invoke(app, "view.switchWindows");
    // A stable id: the View tab is drawn for the first time on the frame the command opens this
    // list, and its groups settle into their compact/natural forms on later frames. Anchoring by
    // response id would lose the open state when those controls move.
    egui::Popup::menu(&anchor).id(egui::Id::unique("gridcraft.switch_windows")).open_memory(popup_command(forced, anchor.clicked())).show(|ui| {
        for w in app.open_windows() {
            let label = theme::available_text(ui, &format!("{}{}", if w.active { "✓ " } else { "    " }, w.label()));
            if ui.add(egui::Button::selectable(w.active, label).min_size(vec2(220.0, 22.0))).clicked() {
                act(app, "view.switchWindows", json!({"window": w.window}));
                ui.close();
            }
        }
    });
}

/// View › Sheet View, New Sheet View: asks for a name and saves the sheet, selection, zoom and
/// freeze as a custom view (`view.customViews`).
fn new_sheet_view(app: &mut SheetApp, ui: &mut Ui) {
    if small_button(ui, Icon::Plus, "New Sheet View", "Save this sheet's selection, zoom and frozen panes as a view", false).clicked() {
        app.open_dialog("newSheetView", json!({}));
    }
}

/// View › Sheet View, Custom Views: the saved views, each one shown on a click, with a Delete
/// beside it.
fn custom_views(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let anchor = small_button(ui, Icon::Book, "Custom Views", "Show or delete a saved view", true);
    control_keytip(app, ui, &anchor, "view.customViews");
    // A stable id: the View tab is drawn for the first time on the frame the command opens this
    // list, and its groups settle into their compact/natural forms on later frames. Anchoring by
    // response id would lose the open state when those controls move (see `switch_windows`).
    let forced = invoke(app, "view.customViews");
    egui::Popup::menu(&anchor).id(egui::Id::unique("gridcraft.custom_views")).open_memory(popup_command(forced, anchor.clicked())).show(|ui| {
        let views = saved_views(app);
        if views.is_empty() {
            ui.label(lang.tr("No saved views"));
            return;
        }
        // A long saved-view list exceeds the window: cap the popup and scroll the rows.
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for name in views {
                ui.horizontal(|ui| {
                    if ui.button(&name).clicked() {
                        act(app, "view.customViews", json!({"show": name}));
                        app.grid.ensure_visible = true;
                        ui.close();
                    }
                    if ui.small_button(lang.tr("Delete")).clicked() {
                        act(app, "view.customViews", json!({"delete": name}));
                    }
                });
            }
        });
    });
}

/// The views saved in the workbook (`view.customViews` with no parameters returns them by name).
fn saved_views(app: &mut SheetApp) -> Vec<String> {
    app.session.execute("view.customViews", json!({})).ok().and_then(|v| v.as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default()
}
