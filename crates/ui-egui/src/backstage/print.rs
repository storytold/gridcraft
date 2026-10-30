use super::{
    Align2, Backstage, Color32, Icon, Json, Outcome, Rect, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, dim, json, pos2, square_button, theme,
    vec2,
};

/// Builds the print scope and fit-to-page parameters, caches `file.printPreview` per document revision/options, and draws page thumbnails with navigation plus the print settings and Print action.
pub(super) fn print(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let Some((uid, revision)) = app.session.active().map(|d| (d.uid, d.revision)) else {
        dim(ui, lang.tr("No workbook is open."));
        return;
    };
    let mut params = json!({"sheets": if bs.print_all { "all" } else { "active" }});
    if bs.fit_to_page {
        params["fitToPage"] = json!(true);
    }
    let key = (uid, revision, bs.print_all, bs.fit_to_page);
    if bs.preview.as_ref().is_none_or(|(k, _)| *k != key) {
        let preview = app.session.run("file.printPreview", params.clone()).unwrap_or(Json::Null);
        bs.preview = Some((key, preview));
    }
    let pages: Vec<Json> = bs.preview.as_ref().and_then(|(_, p)| p.get("pageList")).and_then(Json::as_array).cloned().unwrap_or_default();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            // One Printer list: the system's queues, then "Network printer…" where the host offers it.
            if let Some(discover) = &app.services.printers {
                let running = app.network.state.draft.running();
                let has_network = app.services.network_print.is_some();
                let network = has_network.then_some(&mut app.network.selected);
                crate::printing::show(&mut app.printing, ui, discover, network, running, |label| lang.tr(label).to_owned());
            } else if app.services.network_print.is_some() {
                // No local queues here: the network printer is the only listed destination.
                app.network.selected = true;
            }
            ui.add_space(8.0);
            if app.network_selected() {
                network_destination(app, ui, &params, pages.len(), page_media(&pages), lang);
            } else {
                let can_print = app.services.print_native.is_none() || app.printing.ready();
                if ui.add_enabled_ui(can_print, |ui| square_button(ui, Icon::Print, lang.tr("Print"))).inner.clicked()
                    && app.commit_edit(0, 0, false, false)
                {
                    app.print_now(params.clone());
                    out.close = app.services.print_native.is_none();
                }
            }
            ui.add_space(16.0);
            let scope = if bs.print_all { "Print Entire Workbook" } else { "Print Active Sheets" };
            craft_ui::ComboBox::from_id_salt("backstage_print_scope")
                .width(230.0)
                .selected_text(lang.tr(scope))
                .show_ui(ui, |ui| {
                    for (all, label) in [(false, "Print Active Sheets"), (true, "Print Entire Workbook")] {
                        if ui.selectable_label(bs.print_all == all, lang.tr(label)).clicked() {
                            bs.print_all = all;
                            bs.preview_page = 0;
                        }
                    }
                })
                .response
                .accessible_name(lang.tr("Print What"));
            ui.add_space(8.0);
            if ui.checkbox(&mut bs.fit_to_page, lang.tr("Fit Sheet on One Page")).changed() {
                bs.preview_page = 0;
            }
            ui.add_space(8.0);
            if ui.link(lang.tr("Page Setup…")).clicked() {
                app.open_dialog("pageSetup", json!({}));
            }
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            if pages.is_empty() {
                dim(ui, lang.tr("Nothing to print."));
                return;
            }
            let last = pages.len().saturating_sub(1);
            bs.preview_page = bs.preview_page.min(last);
            let Some(page) = pages.get(bs.preview_page) else { return };
            // Paper proportions from the preview (points); hostile sizes fall back to Letter.
            let w = page.get("width").and_then(Json::as_f64).filter(|w| w.is_finite() && *w > 1.0).unwrap_or(612.0) as f32;
            let h = page.get("height").and_then(Json::as_f64).filter(|h| h.is_finite() && *h > 1.0).unwrap_or(792.0) as f32;
            let scale = (520.0 / h).min(440.0 / w);
            let (rect, _) = ui.allocate_exact_size(vec2(w * scale, h * scale), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect.translate(vec2(2.0, 3.0)), 2.0, t.shadow);
            p.rect_filled(rect, 2.0, t.grid_bg);
            p.rect_stroke(rect, 2.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            let gray = app.print_grayscale();
            if let Some(d) = app.session.active() {
                paint_page(p, rect, w, page, &d.wb, &t, gray);
            }
            let sheet = page.get("sheet").and_then(Json::as_str).unwrap_or("");
            let range = page.get("range").and_then(Json::as_str).unwrap_or("");
            ui.add_space(4.0);
            dim(ui, &format!("{sheet} · {range}"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(bs.preview_page > 0, egui::Button::new("◀")).on_hover_text(lang.tr("Previous page")).clicked() {
                    bs.preview_page = bs.preview_page.saturating_sub(1);
                }
                let text = lang
                    .tr("Page {page} of {pages}")
                    .replace("{page}", &(bs.preview_page + 1).to_string())
                    .replace("{pages}", &pages.len().to_string());
                ui.label(text);
                if ui.add_enabled(bs.preview_page < last, egui::Button::new("▶")).on_hover_text(lang.tr("Next page")).clicked() {
                    bs.preview_page = (bs.preview_page + 1).min(last);
                }
            });
        });
    });
}

/// The media size every previewed page prints on, as a size name printers list: each page's paper
/// name, else its size in points (either orientation). Pages of different sizes name no media, so no
/// page's size is imposed on the others.
fn page_media(pages: &[Json]) -> Option<&'static str> {
    let media = |page: &Json| {
        page.get("paper").and_then(Json::as_str).and_then(crate::printing::media_for_paper).or_else(|| {
            let size = |key: &str| page.get(key).and_then(Json::as_f64);
            crate::printing::media_for_points(size("width")?, size("height")?)
        })
    };
    let mut all = pages.iter().map(media);
    let first = all.next()??;
    all.all(|m| m == Some(first)).then_some(first)
}

/// The network-printer destination of the Print page: craft-ui's section (address, query, the
/// printer's three-state options, the confirmation, the job's status) and the Print button.
fn network_destination(app: &mut SheetApp, ui: &mut Ui, params: &Json, sheets: usize, media: Option<&'static str>, lang: crate::i18n::Language) {
    let Some(net) = &app.services.network_print else { return };
    let host = crate::printing::net::Host { validate: net.validate.as_ref(), query: &net.query, sheets_ok: sheets > 0, formats: net.formats };
    let labels = crate::printing::network_labels(lang);
    let response = ui.scope(|ui| {
        ui.set_min_width(330.0);
        app.network.state.show(ui, egui::Id::unique("gridcraft_network_printer"), &host, &labels)
    });
    let response = response.inner;
    ui.add_space(8.0);
    let can_print = response.block.is_none();
    if ui.add_enabled_ui(can_print, |ui| square_button(ui, Icon::Print, lang.tr("Print"))).inner.clicked() && app.commit_edit(0, 0, false, false) {
        let pages = u32::try_from(sheets).ok();
        if let Err(error) = app.print_network(params.clone(), pages, media) {
            app.alert_text(error);
        }
    }
}

/// Most rows and columns one preview page draws (a page holds far fewer at any readable scale).
const PREVIEW_ROWS: u32 = 200;

const PREVIEW_COLS: u32 = 60;

/// A preview colour as it will print: unchanged, or its Rec. 709 gray (the weights the grayscale PDF
/// uses, so the preview and the page agree).
fn preview_colour(c: Color32, gray: bool) -> Color32 {
    if !gray {
        return c;
    }
    let y = gridcraft_engine::gray_of(c.r(), c.g(), c.b());
    Color32::from_rgba_unmultiplied(y, y, y, c.a())
}

/// Draws one printed page in miniature: the cells' displayed text at the page's print scale,
/// inside the sheet's margins. `paper_w` is the page width in points.
fn paint_page(p: &egui::Painter, rect: Rect, paper_w: f32, page: &Json, wb: &gridcraft_engine::model::Workbook, t: &Tokens, gray: bool) {
    let Some(range) = page.get("range").and_then(Json::as_str).and_then(gridcraft_engine::core::RangeRef::parse) else { return };
    let Some(sh) = page.get("sheet").and_then(Json::as_str).and_then(|n| wb.sheet_index(n)).and_then(|i| wb.sheets.get(i)) else { return };
    // Preview pixels per point, and per sheet pixel (sheet geometry is 96 dpi: 0.75 pt per pixel).
    let per_pt = rect.width() / paper_w;
    let print_scale =
        page.get("scale").and_then(Json::as_f64).filter(|s| s.is_finite() && *s > 0.0).map_or(1.0, |s| (s / 100.0) as f32).clamp(0.1, 4.0);
    let per_px = per_pt * 0.75 * print_scale;
    let margin =
        |i: usize, default: f32| sh.print.margins.get(i).copied().filter(|m| m.is_finite()).unwrap_or(default).clamp(0.0, 3.0) * 72.0 * per_pt;
    let body = Rect::from_min_max(
        pos2(rect.left() + margin(0, 0.7), rect.top() + margin(2, 0.75)),
        pos2(rect.right() - margin(1, 0.7), rect.bottom() - margin(3, 0.75)),
    );
    if !body.is_positive() {
        return;
    }
    let font = theme::ui_font((11.0 * per_pt * print_scale).clamp(3.0, 14.0));
    let last_row = range.end.row.min(range.start.row.saturating_add(PREVIEW_ROWS));
    let last_col = range.end.col.min(range.start.col.saturating_add(PREVIEW_COLS));
    let mut y = body.top();
    for row in range.start.row..=last_row {
        let h = sh.row_height(row) * per_px;
        if h <= 0.0 {
            continue;
        }
        if y >= body.bottom() {
            break;
        }
        let mut x = body.left();
        for col in range.start.col..=last_col {
            let w = sh.col_width(col) * per_px;
            if w <= 0.0 {
                continue;
            }
            if x >= body.right() {
                break;
            }
            let cell = gridcraft_engine::core::CellRef::new(row, col);
            let style = wb.styles.get(sh.style_id(cell));
            if style.fill.pattern == gridcraft_engine::model::PatternType::Solid
                && let Some(rgb) = style.fill.fg.resolve(&wb.theme)
            {
                let fill = preview_colour(Color32::from_rgb(rgb[0], rgb[1], rgb[2]), gray);
                p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(w, h)).intersect(body), 0.0, fill);
            }
            let text = gridcraft_engine::display::cell_text(wb, sh, cell);
            if !text.is_empty() {
                let ink = style.font.color.resolve(&wb.theme).map_or(t.cell_text, |rgb| Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                let ink = preview_colour(ink, gray);
                let numeric = sh.value(cell).as_f64().is_some();
                // Text runs on over empty neighbours, as on the sheet; numbers stay in their cell.
                let mut span = w;
                if !numeric {
                    let mut next = col.saturating_add(1);
                    while next <= last_col && x + span < body.right() && sh.value(gridcraft_engine::core::CellRef::new(row, next)).is_empty() {
                        span += sh.col_width(next) * per_px;
                        next = next.saturating_add(1);
                    }
                }
                let clip = Rect::from_min_size(pos2(x, y), vec2(span, h)).intersect(body);
                let (anchor, at) =
                    if numeric { (Align2::RIGHT_CENTER, pos2(x + w - 1.0, y + h / 2.0)) } else { (Align2::LEFT_CENTER, pos2(x + 1.0, y + h / 2.0)) };
                p.with_clip_rect(clip).text(at, anchor, text, font.clone(), ink);
            }
            x += w;
        }
        y += h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_print_preview_colours_are_gray_exactly_when_grayscale_is_chosen() {
        let red = Color32::from_rgb(220, 30, 40);
        assert_eq!(preview_colour(red, false), red);
        let gray = preview_colour(red, true);
        assert_eq!((gray.r(), gray.g(), gray.b()), (71, 71, 71), "Rec. 709, the weights the PDF uses");
        assert_eq!(preview_colour(Color32::WHITE, true), Color32::WHITE);
        assert_eq!(preview_colour(Color32::BLACK, true), Color32::BLACK);
    }

    #[test]
    fn a_network_job_names_a_media_only_when_every_page_shares_it() {
        let letter = json!({"paper": "Letter", "width": 612.0, "height": 792.0});
        let legal = json!({"paper": "Legal", "width": 612.0, "height": 1008.0});
        let wide_letter = json!({"paper": "Letter", "orientation": "landscape", "width": 792.0, "height": 612.0});
        assert_eq!(page_media(&[letter.clone(), legal.clone()]), None, "Letter and Legal sheets in one workbook");
        assert_eq!(page_media(&[legal, letter.clone()]), None);
        assert_eq!(page_media(&[letter.clone(), wide_letter]), Some("na_letter_8.5x11in"), "landscape Letter is Letter");
        let executive = json!({"paper": "Executive", "width": 522.0, "height": 756.0});
        assert_eq!(page_media(&[executive]), Some("na_executive_7.25x10.5in"));
        // An unnamed or unknown paper falls back to the page's size.
        assert_eq!(page_media(&[json!({"paper": "Napkin", "width": 595.28, "height": 841.89})]), Some("iso_a4_210x297mm"));
        assert_eq!(page_media(&[letter, json!({"width": 500.0, "height": 500.0})]), None);
        assert_eq!(page_media(&[]), None);
    }
}
