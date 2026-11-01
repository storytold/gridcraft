use super::{
    Align2, Backstage, Icon, Json, Outcome, Rect, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, dim, json, pos2, square_button, theme, vec2,
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
            if let Some(discover) = &app.services.printers {
                crate::printing::show(&mut app.printing, ui, discover, |label| lang.tr(label).to_owned());
            }
            let can_print = app.services.print_native.is_none() || app.printing.ready();
            if ui.add_enabled_ui(can_print, |ui| square_button(ui, Icon::Print, lang.tr("Print"))).inner.clicked()
                && app.commit_edit(0, 0, false, false)
            {
                app.print_now(params.clone());
                out.close = app.services.print_native.is_none();
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
            // What this page prints (borders and text, placed as the PDF places them), fetched
            // for the shown page only.
            let cells_key = (key, bs.preview_page);
            if bs.preview_cells.as_ref().is_none_or(|(k, _)| *k != cells_key) {
                let mut query = params.clone();
                query["page"] = json!(bs.preview_page + 1);
                query["cells"] = json!(true);
                let detail = app.session.run("file.printPreview", query).ok().and_then(|r| r.get("page").cloned()).unwrap_or(Json::Null);
                bs.preview_cells = Some((cells_key, detail));
            }
            // Paper proportions from the preview (points); hostile sizes fall back to Letter.
            let w = page.get("width").and_then(Json::as_f64).filter(|w| w.is_finite() && *w > 1.0).unwrap_or(612.0) as f32;
            let h = page.get("height").and_then(Json::as_f64).filter(|h| h.is_finite() && *h > 1.0).unwrap_or(792.0) as f32;
            let scale = (520.0 / h).min(440.0 / w);
            let (rect, _) = ui.allocate_exact_size(vec2(w * scale, h * scale), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect.translate(vec2(2.0, 3.0)), 2.0, t.shadow);
            p.rect_filled(rect, 2.0, t.grid_bg);
            p.rect_stroke(rect, 2.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            if let Some((_, detail)) = &bs.preview_cells {
                paint_page(p, rect, w, detail, &t);
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

/// Draws one printed page in miniature from what `file.printPreview` (with `cells`) says it
/// prints: each cell's borders and text lines, at the page positions the PDF uses, so the
/// margins, Center on Page and merged-cell alignment match the paper (#3059). `paper_w` is the
/// page width in points.
fn paint_page(p: &egui::Painter, rect: Rect, paper_w: f32, page: &Json, t: &Tokens) {
    let Some(cells) = page.get("cells").and_then(Json::as_array) else { return };
    let per_pt = rect.width() / paper_w;
    if !per_pt.is_finite() || per_pt <= 0.0 {
        return;
    }
    let num = |v: &Json, k: &str| v.get(k).and_then(Json::as_f64).map(|f| f as f32).filter(|f| f.is_finite());
    let at = |x: f32, y: f32| pos2(rect.left() + x * per_pt, rect.top() + y * per_pt);
    let p = p.with_clip_rect(rect);
    for c in cells {
        let (Some(x), Some(y), Some(w), Some(h)) = (num(c, "x"), num(c, "y"), num(c, "w"), num(c, "h")) else { continue };
        let (tl, br) = (at(x, y), at(x + w, y + h));
        // Borders: top, right, bottom, left.
        let borders = c.get("borders");
        for (side, a, b) in
            [("top", tl, pos2(br.x, tl.y)), ("right", pos2(br.x, tl.y), br), ("bottom", pos2(tl.x, br.y), br), ("left", tl, pos2(tl.x, br.y))]
        {
            let Some(line) = borders.and_then(|bd| bd.get(side)) else { continue };
            let width = (num(line, "width").unwrap_or(0.5) * per_pt).clamp(0.6, 6.0);
            let color = line.get("color").and_then(Json::as_str).and_then(|h| egui::Color32::from_hex(h).ok()).unwrap_or(t.cell_text);
            let stroke = Stroke::new(width, color);
            if line.get("double").and_then(Json::as_bool) == Some(true) {
                let off = if a.y == b.y { vec2(0.0, width) } else { vec2(width, 0.0) };
                p.line_segment([a - off, b - off], stroke);
                p.line_segment([a + off, b + off], stroke);
            } else if line.get("dashed").and_then(Json::as_bool) == Some(true) {
                p.extend(egui::Shape::dashed_line(&[a, b], stroke, 3.0 * width, 2.0 * width));
            } else {
                p.line_segment([a, b], stroke);
            }
        }
        // Text lines, anchored on the side they are aligned to (screen glyphs are not the PDF's
        // widths), and clipped to where the text may overflow.
        let Some(lines) = c.get("lines").and_then(Json::as_array).filter(|l| !l.is_empty()) else { continue };
        let size = num(c, "size").unwrap_or(11.0).max(0.0);
        let font = theme::ui_font((size * per_pt).clamp(2.0, 96.0));
        let clip = c.get("clip").and_then(Json::as_array);
        let edge = |i: usize, default: f32| {
            clip.and_then(|v| v.get(i)).and_then(Json::as_f64).map(|f| f as f32).filter(|f| f.is_finite()).unwrap_or(default)
        };
        let clip = Rect::from_min_max(pos2(at(edge(0, x), 0.0).x, tl.y), pos2(at(edge(1, x + w), 0.0).x, br.y)).intersect(rect);
        if !clip.is_positive() {
            continue;
        }
        let painter = p.with_clip_rect(clip);
        let align = c.get("align").and_then(Json::as_str).unwrap_or("left");
        for line in lines {
            let (Some(text), Some(lx), Some(base)) = (line.get("text").and_then(Json::as_str), num(line, "x"), num(line, "baseline")) else {
                continue;
            };
            let lw = num(line, "width").unwrap_or(0.0);
            // The PDF's baseline sits about a third of the font size below the middle of the glyphs.
            let mid = base - size * 0.33;
            let (anchor, ax) = match align {
                "right" => (Align2::RIGHT_CENTER, lx + lw),
                "center" => (Align2::CENTER_CENTER, lx + lw / 2.0),
                _ => (Align2::LEFT_CENTER, lx),
            };
            painter.text(at(ax, mid), anchor, text, font.clone(), t.cell_text);
        }
    }
}
