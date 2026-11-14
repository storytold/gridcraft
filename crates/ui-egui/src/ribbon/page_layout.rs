use super::controls::{act, anchor_keytip, checkbox_on, control_keytip, invoke, keytip, menu_items, popup_command, sep};
use super::home::FONTS;
use super::{Color32, Icon, Sense, SheetApp, Tokens, Ui, big_button, big_split_button, json, keytip_table, small_button, vec2};

/// Whether the Scale box reads "Automatic": a fit pair only counts when one dimension is positive.
/// Choosing Automatic for both Width and Height stores `Some((0, 0))`, which print layout treats as
/// no fit and prints at the stored scale, so the percentage is shown instead.
fn fits_automatic(fit: Option<(u16, u16)>) -> bool {
    fit.is_some_and(|(w, h)| w > 0 || h > 0)
}

pub(super) fn page_layout(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let th = big_button(ui, Icon::Theme, "Themes", "Themes", true);
    anchor_keytip(app, ui, &th, &["pageLayout.theme"]);
    egui::Popup::menu(&th).show(|ui| {
        for (name, colors) in gridcraft_engine::cmd::view::themes() {
            ui.horizontal(|ui| {
                for c in colors.iter().skip(4).take(6) {
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 1.0, Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8));
                }
                if ui.button(name).clicked() {
                    act(app, "pageLayout.theme", json!({"name": name}));
                }
            });
        }
    });
    ui.vertical(|ui| {
        // The Colors, Fonts and Effects menus open by keytip too (PTC/PTF/PTE), as Excel's do.
        let colors = small_button(ui, Icon::Fill, "Colors", "Theme colors", true);
        control_keytip(app, ui, &colors, "pageLayout.themeColors");
        let forced = invoke(app, "pageLayout.themeColors");
        egui::Popup::menu(&colors).open_memory(popup_command(forced, colors.clicked())).show(|ui| {
            for (name, _) in gridcraft_engine::cmd::view::themes() {
                if ui.button(name).clicked() {
                    act(app, "pageLayout.themeColors", json!({"name": name}));
                    ui.close();
                }
            }
        });
        let fonts = small_button(ui, Icon::FontColor, "Fonts", "Theme fonts", true);
        control_keytip(app, ui, &fonts, "pageLayout.themeFonts");
        let forced = invoke(app, "pageLayout.themeFonts");
        egui::Popup::menu(&fonts).open_memory(popup_command(forced, fonts.clicked())).show(|ui| {
            for f in FONTS {
                if ui.button(*f).clicked() {
                    act(app, "pageLayout.themeFonts", json!({"major": f, "minor": f}));
                    ui.close();
                }
            }
        });
        let effects = small_button(ui, Icon::Sparkline, "Effects", "Theme effects", true);
        control_keytip(app, ui, &effects, "pageLayout.themeEffects");
        let forced = invoke(app, "pageLayout.themeEffects");
        egui::Popup::menu(&effects).open_memory(popup_command(forced, effects.clicked())).show(|ui| {
            for (label, name) in [("Subtle", "subtle"), ("Medium", "medium"), ("Intense", "intense")] {
                if ui.button(lang.tr(label)).clicked() {
                    act(app, "pageLayout.themeEffects", json!({"name": name}));
                    ui.close();
                }
            }
        });
    });
    sep(ui);
    let m = big_button(ui, Icon::Margins, "Margins", "Margins", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["pageLayout.margins", "dialog:pageSetup"]) {
        keytip(app, ui, sequence, m.rect.center_bottom());
    }
    egui::Popup::menu(&m).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Normal", "pageLayout.margins", json!({"preset": "normal"})),
                ("Wide", "pageLayout.margins", json!({"preset": "wide"})),
                ("Narrow", "pageLayout.margins", json!({"preset": "narrow"})),
                ("Custom Margins…", "dialog:pageSetup", json!({})),
            ],
        );
    });
    let o = big_button(ui, Icon::Orient, "Page Layout|Orientation", "Page Layout|Orientation", true);
    control_keytip(app, ui, &o, "pageLayout.orientation");
    egui::Popup::menu(&o).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Portrait", "pageLayout.orientation", json!({"orientation": "portrait"})),
                ("Landscape", "pageLayout.orientation", json!({"orientation": "landscape"})),
            ],
        );
    });
    let s = big_button(ui, Icon::Paper, "Size", "Paper Size", true);
    control_keytip(app, ui, &s, "pageLayout.size");
    egui::Popup::menu(&s).show(|ui| {
        // B4/B5/B6 are the JIS sizes Japanese Excel uses; the ISO ones stay selectable by name.
        for p in [
            "Letter",
            "Legal",
            "Tabloid",
            "Executive",
            "A3",
            "A4",
            "A5",
            "A6",
            "B4",
            "B5",
            "B6",
            "Japanese Postcard",
            "Japanese Double Postcard",
            "Japanese Envelope #3",
            "Japanese Envelope #4",
            "Japanese Envelope Kaku #2",
            "Japanese Envelope Kaku #3",
            "B4 (ISO)",
            "B5 (ISO)",
            "B6 (ISO)",
        ] {
            if ui.button(p).clicked() {
                act(app, "pageLayout.size", json!({"paper": p}));
            }
        }
    });
    let pa = big_button(ui, Icon::PrintArea, "Print\nArea", "Print Area", true);
    anchor_keytip(app, ui, &pa, &["pageLayout.printArea"]);
    egui::Popup::menu(&pa).show(|ui| {
        menu_items(
            app,
            ui,
            &[("Set Print Area", "pageLayout.printArea", json!({})), ("Clear Print Area", "pageLayout.printArea", json!({"clear": true}))],
        );
    });
    let br = big_button(ui, Icon::PageBreak, "Breaks", "Page Breaks", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["pageLayout.breaks"]) {
        keytip(app, ui, sequence, br.rect.center_bottom());
    }
    egui::Popup::menu(&br).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Insert Page Break", "pageLayout.breaks", json!({"insert": true})),
                ("Remove Page Break", "pageLayout.breaks", json!({"remove": true})),
                ("Reset All Page Breaks", "pageLayout.breaks", json!({"reset": true})),
            ],
        );
    });
    let (bg, bg_menu) = big_split_button(ui, Icon::Picture, "Background", "Sheet Background");
    control_keytip(app, ui, &bg, "pageLayout.background");
    if bg.clicked() {
        act(app, "pageLayout.background", json!({}));
    }
    // Excel removes a saved background from the same control; its command only clears with
    // `{"clear": true}`, so the arrow offers it instead of opening the picture chooser again.
    egui::Popup::menu(&bg_menu).show(|ui| {
        if ui.button(app.ui.language.tr("Background…")).clicked() {
            act(app, "pageLayout.background", json!({}));
            ui.close();
        }
        if ui.button(app.ui.language.tr("Clear Background")).clicked() {
            act(app, "pageLayout.background", json!({"clear": true}));
            ui.close();
        }
    });
    let __k = big_button(ui, Icon::Sheet, "Print\nTitles", "Print Titles", false);
    control_keytip(app, ui, &__k, "pageLayout.printTitles");
    if __k.clicked() {
        app.open_dialog("pageSetup", json!({"tab": "Sheet"}));
    }
    sep(ui);
    let sh = app
        .session
        .active()
        .and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings, s.print.gridlines, s.print.headings)))
        .unwrap_or((true, true, false, false));
    // These two checkboxes mean the screen view vs. the printed page. English shows "View" (as in
    // Excel), but the key is distinct from the "View" tab so languages can word them apart.
    let screen = app.ui.language.tr("Sheet Options|View");
    let print = app.ui.language.tr("Print");
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(app.ui.language.tr("Gridlines")).strong().small());
        let mut v = sh.0;
        let resp = ui.checkbox(&mut v, screen);
        control_keytip(app, ui, &resp, "view.gridlines");
        if resp.changed() || invoke(app, "view.gridlines") {
            act(app, "view.gridlines", json!({"on": checkbox_on(resp.changed(), v)}));
        }
        let mut p = sh.2;
        let resp = ui.checkbox(&mut p, print);
        control_keytip(app, ui, &resp, "pageLayout.printGridlines");
        if resp.changed() || invoke(app, "pageLayout.printGridlines") {
            act(app, "pageLayout.printGridlines", json!({"on": checkbox_on(resp.changed(), p)}));
        }
    });
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(app.ui.language.tr("Headings")).strong().small());
        let mut v = sh.1;
        let resp = ui.checkbox(&mut v, screen);
        control_keytip(app, ui, &resp, "view.headings");
        if resp.changed() || invoke(app, "view.headings") {
            act(app, "view.headings", json!({"on": checkbox_on(resp.changed(), v)}));
        }
        let mut p = sh.3;
        let resp = ui.checkbox(&mut p, print);
        control_keytip(app, ui, &resp, "pageLayout.printHeadings");
        if resp.changed() || invoke(app, "pageLayout.printHeadings") {
            act(app, "pageLayout.printHeadings", json!({"on": checkbox_on(resp.changed(), p)}));
        }
    });
    sep(ui);
    // Scale to Fit
    let (scale, fit) = app.session.active().and_then(|d| d.wb.active().map(|s| (s.print.scale, s.print.fit_to))).unwrap_or((100, None));
    let (fit_w, fit_h) = fit.unwrap_or((0, 0));
    let auto = fits_automatic(fit);
    let t = Tokens::get(ui.ctx());
    let pages = |n: u16| if n == 0 { lang.tr("Automatic").to_string() } else { format!("{n} {}", lang.tr("pages")) };
    ui.vertical(|ui| {
        for (label, current, key) in [("Width", fit_w, "width"), ("Height", fit_h, "height")] {
            ui.horizontal(|ui| {
                let label_response = ui.label(egui::RichText::new(lang.tr(label)).font(crate::theme::ui_font(11.5)).color(t.text_dim));
                craft_ui::ComboBox::from_id_salt(("scale_to_fit", key))
                    .width(96.0)
                    .selected_text(egui::RichText::new(pages(current)).font(crate::theme::ui_font(12.5)))
                    .show_ui(ui, |ui| {
                        for n in 0u16..=8 {
                            if ui.selectable_label(n == current, pages(n)).clicked() {
                                let mut p = json!({});
                                p["width"] = json!(if key == "width" { n } else { fit_w });
                                p["height"] = json!(if key == "height" { n } else { fit_h });
                                act(app, "pageLayout.scaleToFit", p);
                            }
                        }
                    })
                    .response
                    .labelled_by(label_response.id);
            });
        }
        ui.horizontal(|ui| {
            let label = ui.label(egui::RichText::new(lang.tr("Scale")).font(crate::theme::ui_font(11.5)).color(t.text_dim));
            let text = if auto { lang.tr("Automatic").to_string() } else { format!("{scale}%") };
            craft_ui::ComboBox::from_id_salt("scale_to_fit_scale")
                .width(96.0)
                .selected_text(egui::RichText::new(text).font(crate::theme::ui_font(12.5)))
                .show_ui(ui, |ui| {
                    for v in [10u16, 25, 50, 75, 80, 90, 100, 125, 150, 200, 400] {
                        if ui.selectable_label(!auto && scale == v, format!("{v}%")).clicked() {
                            act(app, "pageLayout.scaleToFit", json!({"scale": v}));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        });
    });
    sep(ui);
    // Arrange: the commands act on the selected object, so they are disabled until one is.
    let object = match app.selected_object {
        Some(o) => json!({"kind": o.kind.name(), "id": o.id}),
        None => json!({}),
    };
    let picked = app.selected_object;
    // The object commands need a selection; the Selection Pane lists the sheet's objects and opens
    // without one, so it sits outside the disabled block.
    ui.vertical(|ui| {
        ui.add_enabled_ui(picked.is_some(), |ui| {
            ui.vertical(|ui| {
                let bf = small_button(ui, Icon::Plus, "Bring Forward", "Bring Forward", true);
                egui::Popup::menu(&bf).show(|ui| {
                    for (label, front) in [("Bring Forward", false), ("Bring to Front", true)] {
                        if ui.button(lang.tr(label)).clicked() {
                            let mut p = object.clone();
                            p["toFront"] = json!(front);
                            act(app, "arrange.bringForward", p);
                            ui.close();
                        }
                    }
                });
                let sb = small_button(ui, Icon::Minus, "Send Backward", "Send Backward", true);
                egui::Popup::menu(&sb).show(|ui| {
                    for (label, back) in [("Send Backward", false), ("Send to Back", true)] {
                        if ui.button(lang.tr(label)).clicked() {
                            let mut p = object.clone();
                            p["toBack"] = json!(back);
                            act(app, "arrange.sendBackward", p);
                            ui.close();
                        }
                    }
                });
            });
        });
        let __k = small_button(ui, Icon::Shapes, "Selection Pane", "Selection Pane", false);
        control_keytip(app, ui, &__k, "view.selectionPane");
        if __k.clicked() {
            act(app, "view.selectionPane", json!({}));
        }
    });
    // Rotate turns the selected shape 90°; `arrange.rotate` has no flip state, so the button does
    // not offer Flip Vertical/Horizontal (the command refuses them). Align and Group are not
    // drawn: they need two or more selected objects and the grid selects one at a time, so the
    // engine would accept the call and change nothing.
    // `arrange.rotate` turns a shape; for a chart, image or slicer it reports an error, so the
    // button is enabled only for a shape.
    let shape = picked.is_some_and(|o| o.kind == gridcraft_engine::model::ObjKind::Shape);
    ui.add_enabled_ui(shape, |ui| {
        let __k = small_button(ui, Icon::Orientation, "Rotate", "Rotate 90°", false);
        control_keytip(app, ui, &__k, "arrange.rotate");
        if __k.clicked() {
            act(app, "arrange.rotate", object.clone());
        }
    });
    let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
    let r = big_button(ui, Icon::SheetRtl, "Sheet\nRight-to-Left", "Sheet Right-to-Left", false);
    if rtl {
        let t = Tokens::get(ui.ctx());
        ui.painter().rect_stroke(r.rect, 5.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    }
    if r.clicked() {
        act(app, "view.rightToLeft", json!({"on": !rtl}));
    }
}

#[cfg(test)]
mod tests {
    use super::fits_automatic;

    #[test]
    fn automatic_only_when_a_fit_dimension_is_positive() {
        assert!(!fits_automatic(None));
        assert!(!fits_automatic(Some((0, 0))), "both Automatic stores (0, 0): the stored scale applies");
        assert!(fits_automatic(Some((1, 0))));
        assert!(fits_automatic(Some((0, 2))));
    }
}
