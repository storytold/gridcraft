use super::controls::{RibbonSections, act, anchor_keytip, checkbox_on, control_keytip, group_end, invoke, keytip, menu_items, section, sep};
use super::{Color32, Icon, Sense, SheetApp, Tokens, Ui, big_button, json, keytip_table, vec2};

pub(super) fn page_layout(app: &mut SheetApp, ui: &mut Ui) {
    // Excel's measured Page Layout rows: Themes is one row until 760 client points, then three;
    // Scale to Fit is one compact row below 680, then two rows of fields.
    RibbonSections::breaks(ui, "Themes", &[(760.0, 3)]);
    RibbonSections::breaks(ui, "Scale to Fit", &[(680.0, 2)]);
    section(ui, app, "Themes", Icon::Theme, |app, ui| {
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
    });
    sep(ui, "Themes");
    section(ui, app, "Page Setup", Icon::PageLayout, |app, ui| {
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
        let __k = big_button(ui, Icon::Sheet, "Print\nTitles", "Print Titles", false);
        control_keytip(app, ui, &__k, "pageLayout.printTitles");
        if __k.clicked() {
            app.open_dialog("pageSetup", json!({"tab": "Sheet"}));
        }
    });
    sep(ui, "Page Setup");
    section(ui, app, "Scale to Fit", Icon::PageLayout, |app, ui| {
        scale_to_fit(app, ui);
    });
    sep(ui, "Scale to Fit");
    section(ui, app, "Sheet Options", Icon::Sheet, |app, ui| {
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
        let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
        let r = big_button(ui, Icon::SheetRtl, "Sheet\nRight-to-Left", "Sheet Right-to-Left", false);
        if rtl {
            let t = Tokens::get(ui.ctx());
            ui.painter().rect_stroke(r.rect, 5.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
        }
        if r.clicked() {
            act(app, "view.rightToLeft", json!({"on": !rtl}));
        }
    });
    group_end(ui, "Sheet Options");
}

/// Page Layout › Scale to Fit: Excel's Width, Height and Scale fields, on two rows (width and
/// height, then scale) as Excel draws them from 680 client points.
fn scale_to_fit(app: &mut SheetApp, ui: &mut Ui) {
    let (fit, scale) = app.session.active().and_then(|d| d.wb.active().map(|s| (s.print.fit_to, s.print.scale))).unwrap_or((None, 100));
    let (width, height) = fit.unwrap_or((0, 0));
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            fit_pages(app, ui, "Width:", width, height, true, scale);
            ui.add_space(6.0);
            fit_pages(app, ui, "Height:", height, width, false, scale);
        });
        ui.horizontal(|ui| {
            let caption = ui.label(egui::RichText::new(app.ui.language.tr("Scale:")).small());
            let mut percent = scale as f64;
            let resp = ui
                .add_sized([54.0, 18.0], egui::DragValue::new(&mut percent).range(10.0..=400.0).max_decimals(0).suffix("%"))
                .labelled_by(caption.id);
            if resp.changed() {
                act(app, "pageLayout.scaleToFit", json!({"scale": percent.round()}));
            }
        });
    });
}

/// One of Scale to Fit's page counts: Automatic (which drops the fit-to and scales by `scale`),
/// or 1–9 pages tall or wide.
fn fit_pages(app: &mut SheetApp, ui: &mut Ui, label: &str, pages: u16, other: u16, is_width: bool, scale: u16) {
    let caption = ui.label(egui::RichText::new(app.ui.language.tr(label)).small());
    let pages_text = |n: u16| if n == 0 { "Automatic".to_owned() } else { format!("{n} page{}", if n == 1 { "" } else { "s" }) };
    let id = if is_width { "scale_to_fit_width" } else { "scale_to_fit_height" };
    let combo = craft_ui::ComboBox::from_id_salt(id).selected_text(egui::RichText::new(pages_text(pages)).small()).width(78.0).show_ui(ui, |ui| {
        for n in 0..=9u16 {
            if ui.selectable_label(n == pages, pages_text(n)).clicked() {
                if n == 0 {
                    // Automatic: no fit-to pages, the percent scale applies.
                    act(app, "pageLayout.scaleToFit", json!({"scale": scale}));
                } else if is_width {
                    act(app, "pageLayout.scaleToFit", json!({"width": n, "height": other}));
                } else {
                    act(app, "pageLayout.scaleToFit", json!({"width": other, "height": n}));
                }
            }
        }
    });
    combo.response.labelled_by(caption.id);
}
