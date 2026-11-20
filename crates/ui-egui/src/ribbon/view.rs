use super::controls::{act, checkbox_on, control_keytip, invoke, keytip, menu_items, popup_command, sep};
use super::{Icon, Rect, SheetApp, Tokens, Ui, big_button, json, keytip_table, pos2, theme, vec2};

/// Draws the View tab: view modes, ruler/gridlines/formula-bar/headings toggles, zoom, freeze panes, theme, animations, interface size and language.
pub(super) fn view(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let is_rtl = lang.is_rtl();

    // =========================================================================
    // הצמדה דינמית לקצה הימני ב-RTL
    // =========================================================================
    let ribbon_id = egui::Id::unique("view_total_content_width");
    let last_measured_width = ui.data(|d| d.get_temp::<f32>(ribbon_id)).unwrap_or(1100.0);
    let container_width = ui.max_rect().width();
    let margin_right = 5.0;

    if is_rtl && container_width > (last_measured_width + margin_right) {
        let left_padding = container_width - last_measured_width - margin_right;
        ui.add_space(left_padding);
    }

    let start_x = ui.cursor().min.x;

    let draw_views = |app: &mut SheetApp, ui: &mut Ui| {
        let items = [
            (Icon::Normal, "Normal", "view.normal"),
            (Icon::PageBreak, "Page Break\nPreview", "view.pageBreakPreview"),
            (Icon::PageLayout, "Page\nLayout", "view.pageLayout"),
        ];

        let render_view = |(icon, label, cmd): (Icon, &str, &str), app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, icon, label, label, false);
            control_keytip(app, ui, &__k, cmd);
            if __k.clicked() {
                act(app, cmd, json!({}));
            }
        };

        if is_rtl {
            for it in items.into_iter().rev() {
                render_view(it, app, ui);
            }
        } else {
            for it in items {
                render_view(it, app, ui);
            }
        }
    };

    let draw_show = |app: &mut SheetApp, ui: &mut Ui| {
        let s = app.session.active().and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings))).unwrap_or((true, true));
        let page_layout =
            app.session.active().and_then(|d| d.wb.active().map(|s| s.view == gridcraft_engine::model::SheetViewMode::PageLayout)).unwrap_or(false);

        let draw_ruler_gridlines = |app: &mut SheetApp, ui: &mut Ui| {
            ui.vertical(|ui| {
                let mut ruler = !app.session.hide_ruler;
                let ruler_resp = ui.add_enabled(page_layout, egui::Checkbox::new(&mut ruler, lang.tr("Ruler")));
                control_keytip(app, ui, &ruler_resp, "view.ruler");
                if ruler_resp.changed() {
                    act(app, "view.ruler", json!({"on": ruler}));
                }
                let mut g = s.0;
                let resp = ui.checkbox(&mut g, lang.tr("Gridlines"));
                control_keytip(app, ui, &resp, "view.gridlines");
                if resp.changed() {
                    act(app, "view.gridlines", json!({"on": g}));
                }
            });
        };

        let draw_formulabar_headings = |app: &mut SheetApp, ui: &mut Ui| {
            ui.vertical(|ui| {
                let mut fb = app.session.prefs.formula_bar;
                let resp = ui.checkbox(&mut fb, lang.tr("Formula Bar"));
                control_keytip(app, ui, &resp, "view.formulaBar");
                if resp.changed() || invoke(app, "view.formulaBar") {
                    act(app, "view.formulaBar", json!({"on": checkbox_on(resp.changed(), fb)}));
                }
                let mut h = s.1;
                let resp = ui.checkbox(&mut h, lang.tr("Headings"));
                control_keytip(app, ui, &resp, "view.headings");
                if resp.changed() {
                    act(app, "view.headings", json!({"on": h}));
                }
            });
        };

        if is_rtl {
            draw_formulabar_headings(app, ui);
            ui.add_space(6.0);
            draw_ruler_gridlines(app, ui);
        } else {
            draw_ruler_gridlines(app, ui);
            ui.add_space(6.0);
            draw_formulabar_headings(app, ui);
        }
    };

    let draw_zoom = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_zoom_dialog = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Zoom, "Zoom", "Zoom…", false);
            control_keytip(app, ui, &__k, "dialog:zoom");
            if __k.clicked() {
                app.open_dialog("zoom", json!({}));
            }
        };

        let draw_zoom_100 = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Search, "100%", "Zoom to 100%", false);
            control_keytip(app, ui, &__k, "view.zoom");
            if __k.clicked() || invoke(app, "view.zoom") {
                act(app, "view.zoom", json!({"percent": 100}));
            }
        };

        let draw_zoom_selection = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Zoom, "Zoom to\nSelection", "Zoom to Selection", false);
            control_keytip(app, ui, &__k, "view.zoomToSelection");
            if __k.clicked() {
                let r = app.grid.cells_rect.unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 700.0)));
                act(app, "view.zoomToSelection", json!({"viewWidth": r.width(), "viewHeight": r.height()}));
            }
        };

        if is_rtl {
            draw_zoom_selection(app, ui);
            draw_zoom_100(app, ui);
            draw_zoom_dialog(app, ui);
        } else {
            draw_zoom_dialog(app, ui);
            draw_zoom_100(app, ui);
            draw_zoom_selection(app, ui);
        }
    };

    let draw_window = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_new_window = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::NewFile, "New\nWindow", "New Window", false);
            control_keytip(app, ui, &__k, "view.newWindow");
            if __k.clicked() {
                act(app, "view.newWindow", json!({}));
            }
        };

        let draw_arrange_all = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Table, "Arrange\nAll", "Arrange All", false);
            control_keytip(app, ui, &__k, "view.arrangeAll");
            if __k.clicked() {
                app.open_dialog("arrangeAll", json!({}));
            }
        };

        let draw_freeze_panes = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        if is_rtl {
            switch_windows(app, ui);
            draw_freeze_panes(app, ui);
            draw_arrange_all(app, ui);
            draw_new_window(app, ui);
        } else {
            draw_new_window(app, ui);
            draw_arrange_all(app, ui);
            draw_freeze_panes(app, ui);
            switch_windows(app, ui);
        }
    };

    let draw_theme_and_animations = |app: &mut SheetApp, ui: &mut Ui| {
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
    };

    let draw_scale_and_language = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_scale = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        let draw_language = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        if is_rtl {
            draw_language(app, ui);
            draw_scale(app, ui);
        } else {
            draw_scale(app, ui);
            draw_language(app, ui);
        }
    };

    if is_rtl {
        draw_scale_and_language(app, ui);
        sep(ui);
        draw_theme_and_animations(app, ui);
        draw_window(app, ui);
        sep(ui);
        draw_zoom(app, ui);
        sep(ui);
        draw_show(app, ui);
        sep(ui);
        draw_views(app, ui);
    } else {
        draw_views(app, ui);
        sep(ui);
        draw_show(app, ui);
        sep(ui);
        draw_zoom(app, ui);
        sep(ui);
        draw_window(app, ui);
        draw_theme_and_animations(app, ui);
        sep(ui);
        draw_scale_and_language(app, ui);
    }

    if is_rtl {
        let total_width = ui.cursor().min.x - start_x;
        ui.data_mut(|d| d.insert_temp(ribbon_id, total_width));
    }
}

fn switch_windows(app: &mut SheetApp, ui: &mut Ui) {
    let anchor = big_button(ui, Icon::Book, "Switch\nWindows", "Switch Windows", true);
    anchor.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Switch Windows"));
    control_keytip(app, ui, &anchor, "view.switchWindows");
    let forced = std::mem::take(&mut app.open_switch_windows) | invoke(app, "view.switchWindows");
    egui::Popup::menu(&anchor).open_memory(popup_command(forced, anchor.clicked())).show(|ui| {
        for w in app.open_windows() {
            let label = theme::available_text(ui, &format!("{}{}", if w.active { "✓ " } else { "    " }, w.label()));
            if ui.add(egui::Button::selectable(w.active, label).min_size(vec2(220.0, 22.0))).clicked() {
                act(app, "view.switchWindows", json!({"window": w.window}));
                ui.close();
            }
        }
    });
}