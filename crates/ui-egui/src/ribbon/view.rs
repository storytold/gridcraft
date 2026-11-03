use super::controls::{act, checkbox_on, control_keytip, invoke, keytip, menu_items, popup_command, sep};
use super::*;

/// Draws the View tab: view modes, ruler/gridlines/formula-bar/headings toggles, zoom, freeze panes, theme, animations, interface size and language.
pub(super) fn view(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let _ = t;
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
    sep(ui);
    let s = app.session.active().and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings))).unwrap_or((true, true));
    let lang = app.ui.language;
    let page_layout =
        app.session.active().and_then(|d| d.wb.active().map(|s| s.view == gridcraft_engine::model::SheetViewMode::PageLayout)).unwrap_or(false);
    // Two columns of two, as in Excel's Show group: four stacked rows overflow the ribbon body.
    ui.vertical(|ui| {
        // Rulers exist only in Page Layout view.
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
    ui.add_space(6.0);
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
    sep(ui);
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
    sep(ui);
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
    for sequence in
        keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["view.freezePanes", "view.freezeTopRow", "view.freezeFirstColumn"])
    {
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
    sep(ui);
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
}

/// View › Switch Windows: the open workbooks, the active one checked and unsaved ones marked;
/// choosing one brings it forward.
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
