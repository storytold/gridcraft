//! File tab (Backstage): New, Open, Save, Save As, Print, Export.

use egui::{Align2, Sense, Ui, pos2, vec2};
use serde_json::json;

use crate::SheetApp;
use crate::theme::{self, Tokens};

/// Excel's brand green — fixed regardless of light/dark mode, same as the blue backstage panel
/// in WordCraft.
pub const APP_COLOR: egui::Color32 = egui::Color32::from_rgb(0x10, 0x7C, 0x41);

const PAGES: [(&str, &str); 7] =
    [("new", "New"), ("open", "Open"), ("save", "Save"), ("saveAs", "Save As"), ("print", "Print"), ("export", "Export"), ("options", "Options")];

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::left("backstage_nav")
        .exact_size(200.0)
        .frame(egui::Frame::NONE.fill(APP_COLOR).inner_margin(egui::Margin { left: 0, right: 0, top: 12, bottom: 12 }))
        .show(ui, |ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(200.0, 40.0), Sense::click());
            ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, "←", theme::ui_bold(16.0), egui::Color32::WHITE);
            if resp.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.ui.backstage = false;
            }
            ui.add_space(6.0);
            for (id, label) in PAGES {
                let (r, resp) = ui.allocate_exact_size(vec2(200.0, 38.0), Sense::click());
                let active = app.ui.backstage_page == id;
                if active {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(46));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(26));
                }
                ui.painter().text(
                    pos2(r.min.x + 22.0, r.center().y),
                    Align2::LEFT_CENTER,
                    crate::tl!(label),
                    if active { theme::ui_bold(14.0) } else { theme::ui_font(14.0) },
                    egui::Color32::WHITE,
                );
                if resp.clicked() {
                    match id {
                        "save" => app.run_or_alert("file.save", json!({})),
                        "saveAs" => app.open_dialog("saveAs", json!({})),
                        _ => app.ui.backstage_page = id.into(),
                    }
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(200.0, 32.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(26));
                }
                ui.painter().text(
                    pos2(r.min.x + 22.0, r.center().y),
                    Align2::LEFT_CENTER,
                    crate::tl!("About"),
                    theme::ui_font(13.0),
                    egui::Color32::WHITE,
                );
                if resp.clicked() {
                    app.open_dialog("about", json!({}));
                }
            });
        });
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin::symmetric(40, 30))).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| match app.ui.backstage_page.as_str() {
            "open" => open_page(app, ui),
            "print" | "export" => export_page(app, ui),
            "options" => options_page(app, ui),
            _ => new_page(app, ui),
        });
    });
}

fn heading(ui: &mut Ui, s: &str) {
    ui.label(egui::RichText::new(crate::tl!(s)).font(theme::ui_bold(26.0)));
    ui.add_space(16.0);
}

fn recent_list(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(crate::tl!("Recent")).font(theme::ui_bold(16.0)));
    ui.add_space(6.0);
    if app.ui.recent.is_empty() {
        ui.label(egui::RichText::new(crate::tl!("Workbooks you open will show up here.")).color(t.text_dim));
    }
    for p in app.ui.recent.clone() {
        let name = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p.clone());
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(700.0), 40.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.hover);
        }
        ui.painter().text(pos2(r.min.x + 6.0, r.min.y + 13.0), Align2::LEFT_CENTER, &name, theme::ui_font(13.0), t.text);
        ui.painter().text(pos2(r.min.x + 6.0, r.min.y + 29.0), Align2::LEFT_CENTER, &p, theme::ui_font(11.0), t.text_dim);
        if resp.clicked() {
            app.open_path(&p);
            app.ui.backstage = false;
        }
    }
}

fn new_page(app: &mut SheetApp, ui: &mut Ui) {
    heading(ui, "New");
    if ui.add(egui::Button::new(egui::RichText::new(crate::tl!("Blank workbook")).font(theme::ui_font(14.0))).min_size(vec2(220.0, 34.0))).clicked() {
        app.run_or_alert("file.new", json!({}));
        app.ui.backstage = false;
    }
    ui.add_space(28.0);
    recent_list(app, ui);
}

fn open_page(app: &mut SheetApp, ui: &mut Ui) {
    heading(ui, "Open");
    if ui.add(egui::Button::new(egui::RichText::new(crate::tl!("Browse…")).font(theme::ui_font(14.0))).min_size(vec2(220.0, 34.0))).clicked() {
        app.open_dialog("open", json!({}));
    }
    ui.add_space(18.0);
    recent_list(app, ui);
}

fn options_page(app: &mut SheetApp, ui: &mut Ui) {
    heading(ui, "Options");
    ui.label(crate::tl!("Interface language:"));
    let current = crate::i18n::Lang::from_pref(&app.ui.language);
    egui::ComboBox::from_id_salt("language")
        .selected_text(if app.ui.language == crate::i18n::AUTO {
            format!("{} ({})", crate::tl!("Automatic"), current.name())
        } else {
            current.name().to_string()
        })
        .show_ui(ui, |ui| {
            if ui.selectable_label(app.ui.language == crate::i18n::AUTO, format!("{} ({})", crate::tl!("Automatic"), current.name())).clicked() {
                app.ui.language = crate::i18n::AUTO.into();
            }
            for lang in crate::i18n::Lang::all() {
                if lang == crate::i18n::Lang::EN {
                    continue;
                }
                if ui.selectable_label(app.ui.language == lang.code(), lang.name()).clicked() {
                    app.ui.language = lang.code().into();
                }
            }
            if ui.selectable_label(app.ui.language == "en", "English").clicked() {
                app.ui.language = "en".into();
            }
        });
}

fn export_page(app: &mut SheetApp, ui: &mut Ui) {
    let printing = app.ui.backstage_page == "print";
    heading(ui, if printing { "Print" } else { "Export" });
    if printing {
        ui.label(crate::tl!("Send the active workbook straight to a printer."));
        ui.add_space(12.0);
        if ui.add(egui::Button::new(egui::RichText::new(crate::tl!("Print")).font(theme::ui_font(13.5))).min_size(vec2(320.0, 34.0))).clicked() {
            app.print_now();
        }
        return;
    }
    ui.label(crate::tl!("Save a copy of this workbook in another format."));
    ui.add_space(12.0);
    for (label, fmt) in [("Excel Workbook (*.xlsx)", "xlsx"), ("CSV (*.csv)", "csv"), ("Web Page (*.html)", "html")] {
        if ui.add(egui::Button::new(egui::RichText::new(crate::tl!(label)).font(theme::ui_font(13.5))).min_size(vec2(320.0, 34.0))).clicked() {
            app.open_dialog("saveCopy", json!({"format": fmt}));
        }
        ui.add_space(4.0);
    }
}
