use super::controls::{act, anchor_keytip, checkbox_on, control_keytip, invoke, keytip, menu_items, sep};
use super::{Color32, Icon, Sense, SheetApp, Tokens, Ui, big_button, json, keytip_table, vec2};
use crate::rtl::UiRtlExt;

pub(super) fn page_layout(app: &mut SheetApp, ui: &mut Ui) {
    let is_rtl = app.ui.language.is_rtl();

    let ribbon_id = egui::Id::unique("page_layout_total_content_width");
    let last_measured_width = ui.data(|d| d.get_temp::<f32>(ribbon_id)).unwrap_or(850.0);
    let container_width = ui.max_rect().width();
    let margin_right = 5.0;

    if is_rtl && container_width > (last_measured_width + margin_right) {
        let left_padding = container_width - last_measured_width - margin_right;
        ui.add_space(left_padding);
    }

    let start_x = ui.cursor().min.x;

    let draw_themes = |app: &mut SheetApp, ui: &mut Ui| {
        let th = big_button(ui, Icon::Theme, "Themes", "Themes", true);
        anchor_keytip(app, ui, &th, &["pageLayout.theme"]);
        egui::Popup::menu(&th).show(|ui| {
            ui.set_max_width(200.0);
            // שימוש בגריד בטוח ללא פריסות RTL עוטפות שגורמות ל-NaN
            egui::Grid::new("themes_grid")
                .spacing(vec2(8.0, 4.0))
                .show(ui, |ui| {
                    for (name, colors) in gridcraft_engine::cmd::view::themes() {
                        if is_rtl {
                            if ui.add(egui::Button::new(app.ui.language.tr(name)).frame(false).min_size(vec2(100.0, 20.0))).clicked() {
                                act(app, "pageLayout.theme", json!({"name": name}));
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for c in colors.iter().skip(4).take(6) {
                                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                                    ui.painter().rect_filled(r, 1.0, Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8));
                                }
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for c in colors.iter().skip(4).take(6) {
                                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                                    ui.painter().rect_filled(r, 1.0, Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8));
                                }
                            });
                            if ui.add(egui::Button::new(app.ui.language.tr(name)).frame(false).min_size(vec2(100.0, 20.0))).clicked() {
                                act(app, "pageLayout.theme", json!({"name": name}));
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    };

    let draw_page_setup = |app: &mut SheetApp, ui: &mut Ui| {
        let m = big_button(ui, Icon::Margins, "Margins", "Margins", true);
        for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["pageLayout.margins", "dialog:pageSetup"]) {
            keytip(app, ui, sequence, m.rect.center_bottom());
        }
        egui::Popup::menu(&m).show(|ui| {
            ui.with_layout(ui.menu_layout(is_rtl), |ui| {
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
        });

        let o = big_button(ui, Icon::Orient, "Page Layout|Orientation", "Page Layout|Orientation", true);
        control_keytip(app, ui, &o, "pageLayout.orientation");
        egui::Popup::menu(&o).show(|ui| {
            ui.with_layout(ui.menu_layout(is_rtl), |ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("Portrait", "pageLayout.orientation", json!({"orientation": "portrait"})),
                        ("Landscape", "pageLayout.orientation", json!({"orientation": "landscape"})),
                    ],
                );
            });
        });

        let s = big_button(ui, Icon::Paper, "Size", "Paper Size", true);
        control_keytip(app, ui, &s, "pageLayout.size");
        egui::Popup::menu(&s).show(|ui| {
            ui.with_layout(ui.menu_layout(is_rtl), |ui| {
                for p in [
                    "Letter", "Legal", "Tabloid", "Executive", "A3", "A4", "A5", "A6", "B4", "B5", "B6", "Japanese Postcard",
                    "Japanese Double Postcard", "Japanese Envelope #3", "Japanese Envelope #4", "Japanese Envelope Kaku #2",
                    "Japanese Envelope Kaku #3", "B4 (ISO)", "B5 (ISO)", "B6 (ISO)",
                ] {
                    if ui.button(app.ui.language.tr(p)).clicked() {
                        act(app, "pageLayout.size", json!({"paper": p}));
                    }
                }
            });
        });

        let pa = big_button(ui, Icon::PrintArea, "Print\nArea", "Print Area", true);
        anchor_keytip(app, ui, &pa, &["pageLayout.printArea"]);
        egui::Popup::menu(&pa).show(|ui| {
            ui.with_layout(ui.menu_layout(is_rtl), |ui| {
                menu_items(
                    app,
                    ui,
                    &[("Set Print Area", "pageLayout.printArea", json!({})), ("Clear Print Area", "pageLayout.printArea", json!({"clear": true}))],
                );
            });
        });

        let br = big_button(ui, Icon::PageBreak, "Breaks", "Page Breaks", true);
        for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["pageLayout.breaks"]) {
            keytip(app, ui, sequence, br.rect.center_bottom());
        }
        egui::Popup::menu(&br).show(|ui| {
            ui.with_layout(ui.menu_layout(is_rtl), |ui| {
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
        });

        let __k = big_button(ui, Icon::Sheet, "Print\nTitles", "Print Titles", false);
        control_keytip(app, ui, &__k, "pageLayout.printTitles");
        if __k.clicked() {
            app.open_dialog("pageSetup", json!({"tab": "Sheet"}));
        }
    };

    let draw_sheet_options = |app: &mut SheetApp, ui: &mut Ui| {
        let sh = app
            .session
            .active()
            .and_then(|d| d.wb.active().map(|s| (s.show_gridlines, s.show_headings, s.print.gridlines, s.print.headings)))
            .unwrap_or((true, true, false, false));

        let screen = app.ui.language.tr("Sheet Options|View");
        let print = app.ui.language.tr("Print");
        let draw_headings = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        let draw_gridlines = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        if is_rtl {
            draw_headings(app, ui);
            draw_gridlines(app, ui);
        } else {
            draw_gridlines(app, ui);
            draw_headings(app, ui);
        }
    };

    let draw_sheet_rtl = |app: &mut SheetApp, ui: &mut Ui| {
        let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
        let r = big_button(ui, Icon::SheetRtl, "Sheet\nRight-to-Left", "Sheet Right-to-Left", false);
        if rtl {
            let t = Tokens::get(ui.ctx());
            ui.painter().rect_stroke(r.rect, 5.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
        }
        if r.clicked() {
            act(app, "view.rightToLeft", json!({"on": !rtl}));
        }
    };

    if is_rtl {
        draw_sheet_rtl(app, ui);
        sep(ui);
        draw_sheet_options(app, ui);
        sep(ui);
        draw_page_setup(app, ui);
        sep(ui);
        draw_themes(app, ui);
    } else {
        draw_themes(app, ui);
        sep(ui);
        draw_page_setup(app, ui);
        sep(ui);
        draw_sheet_options(app, ui);
        draw_sheet_rtl(app, ui);
    }

    if is_rtl {
        let total_width = ui.cursor().min.x - start_x;
        ui.data_mut(|d| d.insert_temp(ribbon_id, total_width));
    }
}