use super::{
    Align2, Backstage, CONTENT_W, Icon, Json, Key, Language, Modifiers, Outcome, Rect, RichText, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui,
    dim, json, pos2, square_button, theme, vec2,
};

/// Normal margins in inches, in [`MARGIN_KEYS`] order.
const NORMAL_MARGINS: [f32; 6] = [0.7, 0.7, 0.75, 0.75, 0.3, 0.3];
/// Margin names as `pageLayout.pageSetup` takes them.
const MARGIN_KEYS: [&str; 6] = ["left", "right", "top", "bottom", "header", "footer"];
const MARGIN_LABELS: [&str; 6] = ["Left", "Right", "Top", "Bottom", "Header", "Footer"];
/// Printable width and height (inches) a dragged margin always leaves, as pagination does.
const MIN_PRINTABLE: f32 = 1.0;
/// Pointer travel (points) along an axis before dragging the table moves it on that axis.
const TABLE_SLOP: f32 = 3.0;
/// How close (screen points) the pointer must be to a margin line to grab it.
const GRAB: f32 = 5.0;
/// Room around the previewed page for the margin handles.
const HANDLE_ROOM: f32 = 12.0;

/// What a drag on the print preview moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragTarget {
    /// One margin, by its index in [`MARGIN_KEYS`].
    Margin(usize),
    /// The table: the left/right and top/bottom margins shift together.
    Table,
}

/// Margins being changed on the Print page (dragged on the preview or in a field), shown live and
/// committed through `pageLayout.pageSetup` as one undo step when the drag ends.
#[derive(Clone, Debug)]
pub(super) struct MarginDraft {
    sheet: String,
    /// Set while the pointer drags on the preview (a field drag has none).
    target: Option<DragTarget>,
    /// Margins (inches) and centring when the drag started.
    start: [f32; 6],
    start_center: (bool, bool),
    /// Where the printed block sat when the drag started: [x, y, w, h] in points.
    block: [f32; 4],
    /// Pointer travel so far, in points.
    moved: egui::Vec2,
    margins: [f32; 6],
    center: (bool, bool),
}

/// Margins (inches) and centring after dragging `target` by `moved` points on paper `paper`
/// (points), the printed block having been at `block` ([x, y, w, h] points).
fn dragged_margins(
    target: DragTarget,
    start: [f32; 6],
    center: (bool, bool),
    paper: (f32, f32),
    block: [f32; 4],
    moved: egui::Vec2,
) -> ([f32; 6], (bool, bool)) {
    let (pw, ph) = (paper.0 / 72.0, paper.1 / 72.0);
    let (dx, dy) = (moved.x / 72.0, moved.y / 72.0);
    let mut m = start;
    let mut c = center;
    let room = |total: f32, other: f32| (total - other - MIN_PRINTABLE).max(0.0);
    match target {
        DragTarget::Margin(0) => m[0] = (start[0] + dx).clamp(0.0, room(pw, start[1])),
        DragTarget::Margin(1) => m[1] = (start[1] - dx).clamp(0.0, room(pw, start[0])),
        DragTarget::Margin(2) => m[2] = (start[2] + dy).clamp(0.0, room(ph, start[3])),
        DragTarget::Margin(3) => m[3] = (start[3] - dy).clamp(0.0, room(ph, start[2])),
        DragTarget::Margin(4) => m[4] = (start[4] + dy).clamp(0.0, ph / 2.0),
        DragTarget::Margin(_) => m[5] = (start[5] - dy).clamp(0.0, ph / 2.0),
        DragTarget::Table => {
            // The table moves by its near margin; the far one gives way, so the printable size
            // (and with it the page breaks) stays put as long as there is room.
            let axes = [(moved.x, 0usize, 1usize, block[0], block[2], pw), (moved.y, 2, 3, block[1], block[3], ph)];
            for (travel, near, far, at, size, total) in axes {
                if travel.abs() < TABLE_SLOP {
                    continue;
                }
                let (Some(&n0), Some(&f0)) = (start.get(near), start.get(far)) else { continue };
                let size = (size / 72.0).max(MIN_PRINTABLE);
                let pos = (at / 72.0 + travel / 72.0).clamp(0.0, (total - size).max(0.0));
                if let Some(slot) = m.get_mut(near) {
                    *slot = pos;
                }
                if let Some(slot) = m.get_mut(far) {
                    *slot = (n0 + f0 - pos).max(0.0);
                }
                if near == 0 {
                    c.0 = false;
                } else {
                    c.1 = false;
                }
            }
        }
    }
    (m, c)
}

/// Where the printed block of size `size` (points) starts on the paper for these margins.
fn block_origin(m: [f32; 6], center: (bool, bool), paper: (f32, f32), size: (f32, f32)) -> egui::Vec2 {
    let [l, r, t, b, ..] = m.map(|v| v * 72.0);
    let x = l + if center.0 { ((paper.0 - l - r - size.0) / 2.0).max(0.0) } else { 0.0 };
    let y = t + if center.1 { ((paper.1 - t - b - size.1) / 2.0).max(0.0) } else { 0.0 };
    vec2(x, y)
}

/// Rounds inches to what the margin fields show: hundredths of a centimetre or of an inch.
fn round_margin(inches: f32, metric: bool) -> f32 {
    if metric { (inches * 254.0).round() / 254.0 } else { (inches * 100.0).round() / 100.0 }
}

/// The margins to save: those still equal to `kept` (the saved margins, or Normal on a reset)
/// stay exactly as they are, so a drag or one field edit never rewrites the others; changed ones
/// are rounded to what the fields show.
fn margins_to_save(kept: [f32; 6], margins: [f32; 6], metric: bool) -> [f32; 6] {
    let mut out = margins;
    for (v, k) in out.iter_mut().zip(kept) {
        *v = if (*v - k).abs() < 1e-4 { k } else { round_margin(*v, metric) };
    }
    out
}

/// The page's margins (inches) from the preview, hostile values replaced by Normal.
fn page_margins(page: &Json) -> [f32; 6] {
    let m = page.get("margins");
    let mut out = NORMAL_MARGINS;
    for (slot, key) in out.iter_mut().zip(MARGIN_KEYS) {
        if let Some(v) = m.and_then(|m| m.get(key)).and_then(Json::as_f64).filter(|v| v.is_finite() && *v >= 0.0) {
            *slot = v.min(20.0) as f32;
        }
    }
    out
}

fn num(v: Option<&Json>, key: &str) -> Option<f32> {
    v.and_then(|v| v.get(key)).and_then(Json::as_f64).filter(|v| v.is_finite()).map(|v| v as f32)
}

/// Saves margins and centring for `sheet` as one undo step; margins equal to `kept` are saved as
/// they are (see [`margins_to_save`]).
fn commit_margins(app: &mut SheetApp, sheet: &str, kept: [f32; 6], margins: [f32; 6], center: (bool, bool), metric: bool) {
    let mut m = serde_json::Map::new();
    for (key, v) in MARGIN_KEYS.iter().zip(margins_to_save(kept, margins, metric)) {
        m.insert((*key).to_string(), json!(v));
    }
    app.run_or_alert("pageLayout.pageSetup", json!({"sheet": sheet, "margins": m, "centerH": center.0, "centerV": center.1}));
}

/// Builds the print scope and fit-to-page parameters, caches `file.printPreview` per document revision/options, and draws the previewed page (with Show Margins, its draggable margins and table) plus the print settings and Print action.
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
    let last = pages.len().saturating_sub(1);
    bs.preview_page = bs.preview_page.min(last);
    let page = pages.get(bs.preview_page).cloned();
    let sheet = page.as_ref().and_then(|p| p.get("sheet")).and_then(Json::as_str).unwrap_or("").to_string();
    // A draft for another sheet (the user paged on) no longer applies.
    if bs.margin_draft.as_ref().is_some_and(|d| d.sheet != sheet) {
        bs.margin_draft = None;
    }
    let metric = *bs.metric.get_or_insert_with(|| {
        let paper = page.as_ref().and_then(|p| p.get("paper")).and_then(Json::as_str).unwrap_or("");
        !matches!(lang, Language::En) || paper.starts_with(['A', 'a', 'B', 'b'])
    });
    // Undo and redo a margin drag without leaving the page (a focused field keeps its own undo).
    if !ui.ctx().text_edit_focused() && bs.margin_draft.is_none() {
        let (undo, redo) = ui.input_mut(|i| {
            let redo = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y);
            (i.consume_key(Modifiers::COMMAND, Key::Z), redo)
        });
        if undo || redo {
            app.run_or_alert(if undo { "edit.undo" } else { "edit.redo" }, json!({}));
        }
    }
    let saved_margins = page.as_ref().map_or(NORMAL_MARGINS, page_margins);
    let saved_center = page.as_ref().map_or((false, false), |p| {
        (p.get("centerH").and_then(Json::as_bool).unwrap_or(false), p.get("centerV").and_then(Json::as_bool).unwrap_or(false))
    });
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
            if page.is_some() {
                ui.add_space(16.0);
                margin_fields(app, ui, bs, &sheet, saved_margins, saved_center, metric);
            }
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            let Some(page) = &page else {
                dim(ui, lang.tr("Nothing to print."));
                return;
            };
            // Paper proportions from the preview (points); hostile sizes fall back to Letter.
            let w = page.get("width").and_then(Json::as_f64).filter(|w| w.is_finite() && *w > 1.0).unwrap_or(612.0) as f32;
            let h = page.get("height").and_then(Json::as_f64).filter(|h| h.is_finite() && *h > 1.0).unwrap_or(792.0) as f32;
            let scale = (560.0 / h).min((CONTENT_W - 250.0 - 24.0 - 2.0 * HANDLE_ROOM) / w);
            preview_page(app, ui, bs, page, &sheet, (w, h), scale, (saved_margins, saved_center), metric, &t);
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
                ui.add_space(16.0);
                ui.toggle_value(&mut bs.show_margins, lang.tr("Show Margins"))
                    .on_hover_text(lang.tr("Drag the margin lines, or the table itself, to place it on the page."));
            });
        });
    });
}

/// The margin fields, centring checkboxes and units under the Print page's settings.
fn margin_fields(app: &mut SheetApp, ui: &mut Ui, bs: &mut Backstage, sheet: &str, saved: [f32; 6], saved_center: (bool, bool), metric: bool) {
    let lang = app.ui.language;
    ui.label(RichText::new(lang.tr("Margins")).strong());
    ui.add_space(4.0);
    let (margins, center) = bs.margin_draft.as_ref().map_or((saved, saved_center), |d| (d.margins, d.center));
    let (factor, unit, step) = if metric { (2.54, " cm", 0.01) } else { (1.0, " in", 0.005) };
    let mut commit: Option<([f32; 6], (bool, bool))> = None;
    let mut kept = saved;
    egui::Grid::new("backstage_print_margins").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
        for (pair, labels) in [[0usize, 1], [2, 3], [4, 5]].iter().zip([["Left", "Right"], ["Top", "Bottom"], ["Header", "Footer"]]) {
            for (&i, label) in pair.iter().zip(labels) {
                ui.label(lang.tr(label));
                let Some(&current) = margins.get(i) else { continue };
                let mut v = current * factor;
                let r = ui.add(
                    egui::DragValue::new(&mut v)
                        .speed(step * factor)
                        .range(0.0..=20.0 * factor)
                        .max_decimals(2)
                        .min_decimals(2)
                        .suffix(unit)
                        .update_while_editing(false),
                );
                if r.changed() {
                    let mut m = margins;
                    if let Some(slot) = m.get_mut(i) {
                        *slot = (v / factor).clamp(0.0, 20.0);
                    }
                    if r.dragged() {
                        let d = bs.margin_draft.get_or_insert_with(|| MarginDraft {
                            sheet: sheet.to_string(),
                            target: None,
                            start: margins,
                            start_center: center,
                            block: [0.0; 4],
                            moved: egui::Vec2::ZERO,
                            margins,
                            center,
                        });
                        d.margins = m;
                    } else {
                        commit = Some((m, center));
                    }
                }
                if r.drag_stopped()
                    && let Some(d) = &bs.margin_draft
                {
                    commit = Some((d.margins, d.center));
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.label(lang.tr("Center on page"));
    ui.horizontal(|ui| {
        let (mut ch, mut cv) = center;
        if ui.checkbox(&mut ch, lang.tr("Horizontally")).changed() {
            commit = Some((margins, (ch, cv)));
        }
        if ui.checkbox(&mut cv, lang.tr("Vertically")).changed() {
            commit = Some((margins, (ch, cv)));
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.link(lang.tr("Normal margins")).on_hover_text(lang.tr("Margins back to Normal and the table back to the top left.")).clicked() {
            commit = Some((NORMAL_MARGINS, (false, false)));
            kept = NORMAL_MARGINS;
        }
        ui.add_space(12.0);
        let mut m = metric;
        ui.selectable_value(&mut m, true, "cm");
        ui.selectable_value(&mut m, false, "in");
        bs.metric = Some(m);
    });
    if let Some((m, c)) = commit {
        bs.margin_draft = None;
        commit_margins(app, sheet, kept, m, c, metric);
    }
}

/// Draws the previewed page; with Show Margins on, its margin lines and its table can be dragged.
#[allow(clippy::too_many_arguments)]
fn preview_page(
    app: &mut SheetApp,
    ui: &mut Ui,
    bs: &mut Backstage,
    page: &Json,
    sheet: &str,
    paper: (f32, f32),
    scale: f32,
    saved: ([f32; 6], (bool, bool)),
    metric: bool,
    t: &Tokens,
) {
    let lang = app.ui.language;
    let content = page.get("content");
    let block = [num(content, "x"), num(content, "y"), num(content, "width"), num(content, "height")].map(|v| v.unwrap_or(0.0).max(0.0));
    let room = if bs.show_margins { HANDLE_ROOM } else { 0.0 };
    let (outer, resp) = ui.allocate_exact_size(vec2(paper.0 * scale + 2.0 * room, paper.1 * scale + 2.0 * room), Sense::click_and_drag());
    let rect = Rect::from_min_size(outer.min + vec2(room, room), vec2(paper.0 * scale, paper.1 * scale));
    let (margins, center) = bs.margin_draft.as_ref().map_or(saved, |d| (d.margins, d.center));
    // The table follows a live drag before the engine re-paginates.
    let origin = block_origin(margins, center, paper, (block[2], block[3]));
    let offset = if bs.margin_draft.is_some() { origin - vec2(block[0], block[1]) } else { egui::Vec2::ZERO };
    let p = ui.painter();
    p.rect_filled(rect.translate(vec2(2.0, 3.0)), 2.0, t.shadow);
    p.rect_filled(rect, 2.0, t.grid_bg);
    p.rect_stroke(rect, 2.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
    if let Some(d) = app.session.active() {
        paint_page(p, rect, scale, page, &d.wb, t, offset);
    }
    if !bs.show_margins {
        return;
    }
    // Margin lines in screen space: (index, vertical, coordinate).
    let [l, r, top, b, hd, ft] = margins.map(|m| m * 72.0 * scale);
    let lines = [
        (0usize, true, rect.left() + l),
        (1, true, rect.right() - r),
        (2, false, rect.top() + top),
        (3, false, rect.bottom() - b),
        (4, false, rect.top() + hd),
        (5, false, rect.bottom() - ft),
    ];
    let table = Rect::from_min_size(rect.min + (vec2(block[0], block[1]) + offset) * scale, vec2(block[2], block[3]) * scale);
    let hit = |pos: egui::Pos2| -> Option<DragTarget> {
        let near = lines
            .iter()
            .filter(|(_, vertical, at)| {
                if *vertical {
                    (pos.x - at).abs() <= GRAB && pos.y >= outer.top() && pos.y <= outer.bottom()
                } else {
                    (pos.y - at).abs() <= GRAB && pos.x >= outer.left() && pos.x <= outer.right()
                }
            })
            .min_by(|a, b| {
                let d = |(_, v, at): &(usize, bool, f32)| if *v { (pos.x - at).abs() } else { (pos.y - at).abs() };
                d(a).total_cmp(&d(b))
            });
        match near {
            Some((i, ..)) => Some(DragTarget::Margin(*i)),
            None if table.contains(pos) => Some(DragTarget::Table),
            None => None,
        }
    };
    let dragging = bs.margin_draft.as_ref().and_then(|d| d.target);
    let hovered = dragging.or_else(|| resp.hover_pos().and_then(hit));
    if resp.drag_started()
        && let Some(target) = ui.input(|i| i.pointer.press_origin()).and_then(hit)
    {
        bs.margin_draft = Some(MarginDraft {
            sheet: sheet.to_string(),
            target: Some(target),
            start: saved.0,
            start_center: saved.1,
            block,
            moved: egui::Vec2::ZERO,
            margins: saved.0,
            center: saved.1,
        });
    }
    if resp.dragged()
        && let Some(d) = bs.margin_draft.as_mut()
        && let Some(target) = d.target
    {
        d.moved += resp.drag_delta() / scale;
        (d.margins, d.center) = dragged_margins(target, d.start, d.start_center, paper, d.block, d.moved);
    }
    if resp.drag_stopped()
        && let Some(d) = bs.margin_draft.take_if(|d| d.target.is_some())
    {
        if d.margins != d.start || d.center != d.start_center {
            commit_margins(app, sheet, d.start, d.margins, d.center, metric);
        }
        return;
    }
    let cursor = match hovered {
        Some(DragTarget::Margin(i)) if i < 2 => egui::CursorIcon::ResizeHorizontal,
        Some(DragTarget::Margin(_)) => egui::CursorIcon::ResizeVertical,
        Some(DragTarget::Table) if dragging.is_some() => egui::CursorIcon::Grabbing,
        Some(DragTarget::Table) => egui::CursorIcon::Grab,
        None => egui::CursorIcon::Default,
    };
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(cursor);
    }
    let p = ui.painter();
    let table_on = hovered == Some(DragTarget::Table);
    let table_color = if table_on { t.accent } else { t.accent.gamma_multiply(0.45) };
    for (a, b) in [
        (table.left_top(), table.right_top()),
        (table.right_top(), table.right_bottom()),
        (table.right_bottom(), table.left_bottom()),
        (table.left_bottom(), table.left_top()),
    ] {
        p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(if table_on { 1.5 } else { 1.0 }, table_color), 4.0, 3.0));
    }
    for (i, vertical, at) in lines {
        let on = hovered == Some(DragTarget::Margin(i));
        let color = if on { t.accent } else { t.text_dim };
        let stroke = Stroke::new(if on { 1.5 } else { 1.0 }, color);
        let (a, b, handle) = if vertical {
            (pos2(at, rect.top()), pos2(at, rect.bottom()), Rect::from_center_size(pos2(at, rect.top() - room / 2.0), vec2(6.0, room - 2.0)))
        } else {
            (pos2(rect.left(), at), pos2(rect.right(), at), Rect::from_center_size(pos2(rect.right() + room / 2.0, at), vec2(room - 2.0, 6.0)))
        };
        if on {
            p.line_segment([a, b], stroke);
        } else {
            p.extend(egui::Shape::dashed_line(&[a, b], stroke, 3.0, 3.0));
        }
        // Header and footer handles are hollow, the page margins filled.
        if i >= 4 {
            p.rect_stroke(handle, 1.0, Stroke::new(1.0, color), StrokeKind::Inside);
        } else {
            p.rect_filled(handle, 1.0, color);
        }
    }
    // While dragging (or hovering), name what moves and by how much.
    if let Some(target) = hovered {
        let (factor, unit) = if metric { (2.54, "cm") } else { (1.0, "in") };
        let show = |i: usize| margins.get(i).map_or(0.0, |m| m * factor);
        let text = match target {
            DragTarget::Margin(i) => {
                let label = MARGIN_LABELS.get(i).map_or("", |l| lang.tr(l));
                format!("{label}: {:.2} {unit}", show(i))
            }
            DragTarget::Table => {
                format!("{}: {:.2} {unit} · {}: {:.2} {unit}", lang.tr("Left"), origin.x / 72.0 * factor, lang.tr("Top"), origin.y / 72.0 * factor)
            }
        };
        if let Some(pos) = resp.hover_pos().or_else(|| ui.input(|i| i.pointer.latest_pos())) {
            let galley = p.layout_no_wrap(text, theme::ui_font(11.5), t.text);
            let at = (pos + vec2(14.0, 14.0)).min(outer.right_bottom() - galley.size() - vec2(8.0, 4.0));
            let bg = Rect::from_min_size(at, galley.size() + vec2(8.0, 4.0));
            p.rect_filled(bg, 3.0, t.menu_bg);
            p.rect_stroke(bg, 3.0, Stroke::new(1.0, t.ribbon_border), StrokeKind::Inside);
            p.galley(at + vec2(4.0, 2.0), galley, t.text);
        }
    }
}

/// Most rows and columns one preview page draws (a page holds far fewer at any readable scale).
const PREVIEW_ROWS: u32 = 200;
const PREVIEW_COLS: u32 = 60;

/// Draws one printed page in miniature: the cells' displayed text at the page's print scale,
/// where the print places them (`body`, moved by `offset` points during a drag). `per_pt` is
/// preview pixels per point.
fn paint_page(p: &egui::Painter, rect: Rect, per_pt: f32, page: &Json, wb: &gridcraft_engine::model::Workbook, t: &Tokens, offset: egui::Vec2) {
    let Some(range) = page.get("range").and_then(Json::as_str).and_then(gridcraft_engine::core::RangeRef::parse) else { return };
    let Some(sh) = page.get("sheet").and_then(Json::as_str).and_then(|n| wb.sheet_index(n)).and_then(|i| wb.sheets.get(i)) else { return };
    // Preview pixels per sheet pixel (sheet geometry is 96 dpi: 0.75 pt per pixel).
    let print_scale =
        page.get("scale").and_then(Json::as_f64).filter(|s| s.is_finite() && *s > 0.0).map_or(1.0, |s| (s / 100.0) as f32).clamp(0.1, 4.0);
    let per_px = per_pt * 0.75 * print_scale;
    let content = page.get("content");
    let body = page.get("body");
    let start = vec2(num(body, "x").unwrap_or(0.0), num(body, "y").unwrap_or(0.0)) + offset;
    let block = Rect::from_min_size(
        rect.min + (vec2(num(content, "x").unwrap_or(0.0), num(content, "y").unwrap_or(0.0)) + offset) * per_pt,
        vec2(num(content, "width").unwrap_or(0.0), num(content, "height").unwrap_or(0.0)) * per_pt,
    )
    .intersect(rect);
    if !block.is_positive() {
        return;
    }
    let font = theme::ui_font((11.0 * per_pt * print_scale).clamp(3.0, 14.0));
    let last_row = range.end.row.min(range.start.row.saturating_add(PREVIEW_ROWS));
    let last_col = range.end.col.min(range.start.col.saturating_add(PREVIEW_COLS));
    let mut y = rect.top() + start.y * per_pt;
    for row in range.start.row..=last_row {
        let h = sh.row_height(row) * per_px;
        if h <= 0.0 {
            continue;
        }
        if y >= block.bottom() {
            break;
        }
        let mut x = rect.left() + start.x * per_pt;
        for col in range.start.col..=last_col {
            let w = sh.col_width(col) * per_px;
            if w <= 0.0 {
                continue;
            }
            if x >= block.right() {
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
                    while next <= last_col && x + span < block.right() && sh.value(gridcraft_engine::core::CellRef::new(row, next)).is_empty() {
                        span += sh.col_width(next) * per_px;
                        next = next.saturating_add(1);
                    }
                }
                let clip = Rect::from_min_size(pos2(x, y), vec2(span, h)).intersect(block);
                let (anchor, at) =
                    if numeric { (Align2::RIGHT_CENTER, pos2(x + w - 1.0, y + h / 2.0)) } else { (Align2::LEFT_CENTER, pos2(x + 1.0, y + h / 2.0)) };
                p.with_clip_rect(clip).text(at, anchor, text, font.clone(), t.cell_text);
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
    fn dragging_margins_and_the_table() {
        let letter = (612.0, 792.0);
        let block = [50.4, 54.0, 144.0, 60.0];
        // Left margin dragged an inch right; never past an inch short of the right margin.
        let (m, _) = dragged_margins(DragTarget::Margin(0), NORMAL_MARGINS, (false, false), letter, block, vec2(72.0, 0.0));
        assert!((m[0] - 1.7).abs() < 1e-4 && m[1] == 0.7);
        let (m, _) = dragged_margins(DragTarget::Margin(0), NORMAL_MARGINS, (false, false), letter, block, vec2(9_000.0, 0.0));
        assert!((m[0] - (8.5 - 0.7 - MIN_PRINTABLE)).abs() < 1e-4);
        // Bottom margin dragged up grows; the footer can't go below the paper.
        let (m, _) = dragged_margins(DragTarget::Margin(3), NORMAL_MARGINS, (false, false), letter, block, vec2(0.0, -36.0));
        assert!((m[3] - 1.25).abs() < 1e-4);
        let (m, _) = dragged_margins(DragTarget::Margin(5), NORMAL_MARGINS, (false, false), letter, block, vec2(0.0, 500.0));
        assert_eq!(m[5], 0.0);
        // The table moves by its near margin and the far one gives way: same printable size.
        let (m, c) = dragged_margins(DragTarget::Table, NORMAL_MARGINS, (true, true), letter, block, vec2(36.0, 1.0));
        assert!((m[0] - 1.2).abs() < 1e-4 && (m[1] - 0.2).abs() < 1e-4);
        assert_eq!((m[2], m[3]), (0.75, 0.75), "a jitter under the slop leaves the other axis alone");
        assert_eq!(c, (false, true));
        // It stays on the paper.
        let (m, _) = dragged_margins(DragTarget::Table, NORMAL_MARGINS, (false, false), letter, block, vec2(-500.0, 9_000.0));
        assert_eq!(m[0], 0.0);
        // A block shorter than an inch still keeps an inch of printable height.
        assert!((m[2] - (11.0 - MIN_PRINTABLE)).abs() < 1e-4);
        assert_eq!(m[3], 0.0);
        // Centring puts the block in the middle of the printable area.
        let at = block_origin(NORMAL_MARGINS, (true, false), letter, (144.0, 60.0));
        assert!((at.x - (50.4 + (612.0 - 100.8 - 144.0) / 2.0)).abs() < 1e-3 && (at.y - 54.0).abs() < 1e-3);
        // Metric rounds to hundredths of a centimetre, as the fields show.
        assert!((round_margin(0.5, true) * 2.54 - 1.27).abs() < 1e-4);
        assert!((round_margin(0.7, true) * 2.54 - 1.78).abs() < 1e-4);
        // Only changed margins are rounded: a one-field edit leaves Normal's exact inches alone.
        let mut edited = NORMAL_MARGINS;
        edited[0] = 1.27 / 2.54 + 0.0001;
        let saved = margins_to_save(NORMAL_MARGINS, edited, true);
        assert_eq!(saved[1..], NORMAL_MARGINS[1..]);
        assert!((saved[0] * 2.54 - 1.27).abs() < 1e-4);
        // A reset to Normal saves Normal exactly.
        assert_eq!(margins_to_save(NORMAL_MARGINS, NORMAL_MARGINS, true), NORMAL_MARGINS);
    }
}
