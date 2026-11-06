use super::Align2;
use super::{
    Backstage, Color32, Icon, Json, Outcome, Rect, RichText, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, dim, json, pos2, square_button, theme,
    vec2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PreviewKey {
    uid: u64,
    revision: u64,
    sheet: usize,
    all_sheets: bool,
    fit_to_page: bool,
}

/// Draws print settings beside a preview that fills the available window.
pub(super) fn print(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let Some((uid, revision, sheet)) = app.session.active().map(|d| (d.uid, d.revision, d.wb.active_sheet())) else {
        dim(ui, lang.tr("No workbook is open."));
        return;
    };
    let mut params = json!({"sheets": if bs.print_all { "all" } else { "active" }});
    if bs.fit_to_page {
        params["fitToPage"] = json!(true);
    }
    let key = PreviewKey { uid, revision, sheet, all_sheets: bs.print_all, fit_to_page: bs.fit_to_page };
    if bs.preview.as_ref().is_none_or(|(k, _)| *k != key) {
        let preview = app.session.run("file.printPreview", params.clone()).unwrap_or(Json::Null);
        bs.preview = Some((key, preview));
    }
    let pages: Vec<Json> = bs.preview.as_ref().and_then(|(_, p)| p.get("pageList")).and_then(Json::as_array).cloned().unwrap_or_default();
    let settings_width = (ui.available_width() * 0.43).clamp(125.0, 265.0);
    egui::Panel::left("print_settings").exact_size(settings_width).resizable(false).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(14)).show(
        ui,
        |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| settings(app, ui, bs, out, &params, !pages.is_empty()));
        },
    );
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.desk)).show(ui, |ui| {
        if pages.is_empty() {
            ui.centered_and_justified(|ui| dim(ui, lang.tr("Nothing to print.")));
            return;
        }
        let last = pages.len().saturating_sub(1);
        bs.preview_page = bs.preview_page.min(last);
        egui::Panel::bottom("print_preview_toolbar")
            .resizable(false)
            .frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(8))
            .show(ui, |ui| navigation(app, ui, bs, last));
        if let Some(page) = pages.get(bs.preview_page) {
            show_paper(app, ui, bs, page);
        }
    });
}

fn settings(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, out: &mut Outcome, params: &Json, has_pages: bool) {
    let lang = app.ui.language;
    ui.label(RichText::new(lang.tr("Print")).font(theme::ui_font(22.0)).strong());
    ui.add_space(6.0);
    let can_print = has_pages && (app.services.print_native.is_none() || app.printing.ready());
    if ui.add_enabled_ui(can_print, |ui| square_button(ui, Icon::Print, lang.tr("Print"))).inner.clicked() && app.commit_edit(0, 0, false, false) {
        app.print_now(params.clone());
        out.close = app.services.print_native.is_none();
    }
    ui.add_space(12.0);
    if let Some(discover) = &app.services.printers {
        crate::printing::show(&mut app.printing, ui, discover, |label| lang.tr(label).to_owned());
        ui.add_space(12.0);
    }
    ui.label(RichText::new(lang.tr("Settings")).strong());
    let width = ui.available_width().clamp(32.0, 230.0);
    let scope = if bs.print_all { "Print Entire Workbook" } else { "Print Active Sheets" };
    craft_ui::ComboBox::from_id_salt("backstage_print_scope")
        .width(width)
        .selected_text(lang.tr(scope))
        .show_ui(ui, |ui| {
            for (all, label) in [(false, "Print Active Sheets"), (true, "Print Entire Workbook")] {
                if ui.selectable_value(&mut bs.print_all, all, lang.tr(label)).changed() {
                    bs.preview_page = 0;
                }
            }
        })
        .response
        .accessible_name(lang.tr("Print What"));
    let scaling = if bs.fit_to_page { "Fit Sheet on One Page" } else { "Use Page Setup Scaling" };
    craft_ui::ComboBox::from_id_salt("backstage_print_scaling")
        .width(width)
        .selected_text(lang.tr(scaling))
        .show_ui(ui, |ui| {
            for (fit, label) in [(false, "Use Page Setup Scaling"), (true, "Fit Sheet on One Page")] {
                if ui.selectable_value(&mut bs.fit_to_page, fit, lang.tr(label)).changed() {
                    bs.preview_page = 0;
                }
            }
        })
        .response
        .accessible_name(lang.tr("Scaling"));
    if ui.link(lang.tr("Page Setup…")).clicked() {
        app.open_dialog("pageSetup", json!({}));
    }
}

fn navigation(app: &SheetApp, ui: &mut Ui, bs: &mut Backstage, last: usize) {
    let lang = app.ui.language;
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(bs.preview_page > 0, egui::Button::new("◀")).on_hover_text(lang.tr("Previous page")).clicked() {
            bs.preview_page = bs.preview_page.saturating_sub(1);
        }
        let page_label = ui.label(lang.tr("Page"));
        let mut number = bs.preview_page + 1;
        if ui.add_enabled(last > 0, egui::DragValue::new(&mut number).range(1..=last + 1)).labelled_by(page_label.id).changed() {
            bs.preview_page = number.saturating_sub(1);
        }
        ui.label(lang.tr("of {pages}").replace("{pages}", &(last + 1).to_string()));
        if ui.add_enabled(bs.preview_page < last, egui::Button::new("▶")).on_hover_text(lang.tr("Next page")).clicked() {
            bs.preview_page = (bs.preview_page + 1).min(last);
        }
        ui.separator();
        ui.toggle_value(&mut bs.preview_margins, lang.tr("Show Margins"));
        let zoom_label = if bs.preview_actual_size { "Fit Preview to Window" } else { "Actual Size" };
        if ui.button(lang.tr(zoom_label)).clicked() {
            bs.preview_actual_size = !bs.preview_actual_size;
        }
    });
}

fn show_paper(app: &SheetApp, ui: &mut Ui, bs: &Backstage, page: &Json) {
    // Preview metadata is generated by the same layout used to print.
    let w = page.get("width").and_then(Json::as_f64).filter(|w| w.is_finite() && *w > 1.0).unwrap_or(612.0) as f32;
    let h = page.get("height").and_then(Json::as_f64).filter(|h| h.is_finite() && *h > 1.0).unwrap_or(792.0) as f32;
    let viewport = ui.available_size().max(vec2(1.0, 1.0));
    let padding = 12.0;
    let fit = (viewport - vec2(padding * 2.0, padding * 2.0)).max(vec2(1.0, 1.0));
    // Actual size is 96 logical pixels per inch, while PDF dimensions are 72 points per inch.
    let scale = if bs.preview_actual_size { 96.0 / 72.0 } else { (fit.x / w).min(fit.y / h) };
    let size = vec2(w, h) * scale;
    egui::ScrollArea::both().id_salt(("print_preview_paper", bs.preview_actual_size)).auto_shrink([false, false]).show(ui, |ui| {
        let canvas = (size + vec2(padding * 2.0, padding * 2.0)).max(viewport);
        let (area, response) = ui.allocate_exact_size(canvas, Sense::hover());
        let sheet = page.get("sheet").and_then(Json::as_str).unwrap_or("");
        let range = page.get("range").and_then(Json::as_str).unwrap_or("");
        response.on_hover_text(format!("{sheet} · {range}"));
        let rect = Rect::from_center_size(area.center(), size);
        let t = Tokens::get(ui.ctx());
        let p = ui.painter();
        p.rect_filled(rect.translate(vec2(2.0, 3.0)), 0.0, t.shadow);
        p.rect_filled(rect, 0.0, Color32::WHITE);
        if let Some(d) = app.session.active() {
            paint_page(p, rect, w, page, &d.wb, &t);
        }
        p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::DARK_GRAY), StrokeKind::Inside);
        if bs.preview_margins {
            paint_margins(p, rect, scale, page);
        }
    });
}

fn paint_margins(p: &egui::Painter, rect: Rect, scale: f32, page: &Json) {
    let margin = |index| page["marginsPt"][index].as_f64().unwrap_or(0.0) as f32 * scale;
    let ink = Stroke::new(1.0, Color32::from_rgb(95, 112, 128));
    let p = p.with_clip_rect(rect);
    for x in [rect.left() + margin(0usize), rect.right() - margin(1)] {
        p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], ink);
    }
    for y in [rect.top() + margin(2), rect.bottom() - margin(3)] {
        p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], ink);
    }
}

/// Most rows and columns one preview page draws (a page holds far fewer at any readable scale).
const PREVIEW_ROWS: u32 = 200;

const PREVIEW_COLS: u32 = 60;

/// Draws one printed page in miniature: the cells' displayed text at the page's print scale,
/// inside the sheet's margins. `paper_w` is the page width in points.
fn paint_page(p: &egui::Painter, rect: Rect, paper_w: f32, page: &Json, wb: &gridcraft_engine::model::Workbook, t: &Tokens) {
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
            let text = gridcraft_engine::display::cell_text(wb, sh, cell);
            if !text.is_empty() {
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
                p.with_clip_rect(clip).text(at, anchor, text, font.clone(), t.cell_text);
            }
            x += w;
        }
        y += h;
    }
}
