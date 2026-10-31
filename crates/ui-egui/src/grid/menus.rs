use super::editor::popup_dismissed;
use super::geo::Geo;
use super::tables::filter_table_range;
use super::{Arc, Drag, FillTag, RangeRef, Rect, Sense, Sheet, SheetApp, Stroke, Tokens, col_to_letters, json, pos2, vec2};

/// Draws the filter-header popup: sort, search/select-all, apply/clear, a lazily installed table autofilter, new-open search focus and dismissal.
pub(super) fn filter_menu(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, was_open: bool) {
    let Some((col, row, at)) = app.grid.filter_menu else { return };
    let Some(d) = app.session.active() else { return };
    let wb = d.wb.clone();
    let si = wb.active_sheet();
    let Some(sh) = wb.sheet(si) else { return };
    let _ = geo;
    // Tables get an autofilter on demand.
    let table_range = filter_table_range(sh, col, row);
    if sh.autofilter.is_none()
        && let Some(r) = table_range
    {
        let _ = app.session.run("data.filter", json!({"range": r.a1()}));
        let _ = app.session.run("data.filter", json!({"range": r.a1()}));
        if let Some(sh2) = app.session.active().and_then(|d| d.wb.active()).map(|s| s.autofilter.is_some())
            && !sh2
        {
            let d = app.session.active_mut();
            if let Some(d) = d {
                let wbm = Arc::make_mut(&mut d.wb);
                if let Some(s) = wbm.sheet_mut(si) {
                    s.autofilter = Some(gridcraft_engine::model::AutoFilter { range: r, criteria: vec![] });
                }
            }
        }
    }
    let wb = app.session.active().map(|d| d.wb.clone()).unwrap_or(wb);
    let values = gridcraft_engine::cmd::data::filter_values(&wb, si, col);
    let key = egui::Id::unique(("filter_sel", col));
    let search_key = egui::Id::unique(("filter_search", col));
    let buttons_key = egui::Id::unique(("filter_buttons", col));
    // Enter is the menu's OK, as in Excel: taken before the search box (which would only drop its
    // focus) and the value checkboxes (which would toggle) see it. A focused button keeps it as its click.
    let buttons: Vec<egui::Id> = ui.ctx().data_mut(|d| d.get_temp(buttons_key)).unwrap_or_default();
    let on_button = ui.ctx().memory(|m| m.focused()).is_some_and(|f| buttons.contains(&f));
    let enter = !on_button && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let mut close = false;
    let area = egui::Area::new(egui::Id::unique("filter_menu")).fixed_pos(at + vec2(-180.0, 10.0)).order(egui::Order::Foreground);
    let response = area.show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(230.0);
            let ascending = ui.button("↑  Sort Ascending");
            if ascending.clicked() {
                let _ = app.run("data.sortAscending", json!({"column": col_to_letters(col)}));
                close = true;
            }
            let descending = ui.button("↓  Sort Descending");
            if descending.clicked() {
                let _ = app.run("data.sortDescending", json!({"column": col_to_letters(col)}));
                close = true;
            }
            ui.separator();
            ui.label(egui::RichText::new("Filter").strong());
            let mut checks: Vec<(String, bool)> = ui.ctx().data_mut(|d| d.get_temp::<Vec<(String, bool)>>(key)).unwrap_or(values.clone());
            // A search narrows the list; Select All and Apply then only cover the matches.
            let mut search: String = ui.ctx().data_mut(|d| d.get_temp(search_key)).unwrap_or_default();
            let field = ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search").desired_width(f32::INFINITY));
            if !was_open {
                field.request_focus();
            }
            let needle = search.trim().to_lowercase();
            let matches = |v: &str| needle.is_empty() || v.to_lowercase().contains(&needle);
            let all = checks.iter().filter(|(v, _)| matches(v)).all(|(_, b)| *b);
            let mut all_new = all;
            let label = if needle.is_empty() { "(Select All)" } else { "(Select All Search Results)" };
            if ui.checkbox(&mut all_new, label).changed() {
                for c in checks.iter_mut().filter(|(v, _)| matches(v)) {
                    c.1 = all_new;
                }
            }
            // Full width: shrunk to the longest value, the scroll bar sat mid-menu and the wheel only
            // worked over the values themselves (#432).
            egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                for (v, on) in checks.iter_mut().filter(|(v, _)| matches(v)) {
                    ui.checkbox(on, v.as_str());
                }
            });
            ui.ctx().data_mut(|d| {
                d.insert_temp(key, checks.clone());
                d.insert_temp(search_key, search.clone());
            });
            let checks: Vec<(String, bool)> = checks.iter().map(|(v, on)| (v.clone(), *on && matches(v))).collect();
            ui.separator();
            ui.horizontal(|ui| {
                let clear = ui.button("Clear Filter");
                if clear.clicked() {
                    let _ = app.run("data.filterBy", json!({"column": col_to_letters(col), "clear": true}));
                    close = true;
                }
                // Nothing checked would hide every row, so Enter (unlike a click on Apply) does nothing then.
                let any = checks.iter().any(|(_, on)| *on);
                let apply = ui.button("Apply");
                ui.ctx().data_mut(|d| d.insert_temp(buttons_key, vec![ascending.id, descending.id, clear.id, apply.id]));
                if apply.clicked() || (enter && any) {
                    let blanks = checks.iter().any(|(v, on)| v == "(Blanks)" && *on);
                    let vals: Vec<String> = checks.iter().filter(|(v, on)| *on && v != "(Blanks)").map(|(v, _)| v.clone()).collect();
                    let p = if checks.iter().all(|(_, b)| *b) {
                        json!({"column": col_to_letters(col), "clear": true})
                    } else {
                        json!({"column": col_to_letters(col), "values": vals, "blanks": blanks})
                    };
                    let _ = app.run("data.filterBy", p);
                    close = true;
                }
            });
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        ui.ctx().data_mut(|d| {
            d.remove::<Vec<(String, bool)>>(key);
            d.remove::<String>(search_key);
            d.remove::<Vec<egui::Id>>(buttons_key);
        });
        app.grid.filter_menu = None;
    }
}

/// Draws and dispatches the cell context menu, including Show Details when offered, clipboard and insert/format actions; closes on Esc or an outside press.
pub(super) fn context_menu(app: &mut SheetApp, ui: &mut egui::Ui) {
    let Some(at) = app.grid.context_menu else { return };
    let mut close = false;
    let resp = egui::Area::new(egui::Id::unique("cell_context")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(200.0);
            let insert_copied = insert_copied_item(app);
            let mut items: Vec<(&str, &str)> = vec![
                ("Cut", "edit.cut"),
                ("Copy", "edit.copy"),
                ("Paste", "edit.paste"),
                ("Paste Special…", "ui:pasteSpecial"),
                ("-", ""),
                ("Insert…", "ui:insertCells"),
                ("Delete…", "ui:deleteCells"),
                ("Clear Contents", "edit.clearContents"),
                ("-", ""),
                ("Filter by Selected Cell's Value", "ui:filterValue"),
                ("Sort A to Z", "data.sortAscending"),
                ("Sort Z to A", "data.sortDescending"),
                ("-", ""),
                ("New Comment", "ui:newComment"),
                ("New Note", "ui:newNote"),
                ("-", ""),
                ("Format Cells…", "ui:formatCells"),
                ("Pick From Drop-down List…", "ui:pickList"),
                ("Define Name…", "ui:defineName"),
                ("Link…", "ui:insertLink"),
            ];
            if let Some(item) = insert_copied {
                items.insert(6, item);
            }
            if show_details_offered(app)
                && let Some(i) = items.iter().position(|(_, id)| *id == "data.sortDescending")
            {
                items.insert(i + 1, ("-", ""));
                items.insert(i + 2, ("Show Details", "pivot.showDetails"));
            }
            if app.ask_agent.hub.is_some() {
                items.extend([("-", ""), ("Ask Agent…", "ui:askAgent")]);
            }
            let lang = crate::i18n::current(ui.ctx());
            for (label, id) in items {
                if label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(190.0, 20.0))).clicked() {
                    close = true;
                    match id {
                        "ui:pasteSpecial" => app.open_dialog("pasteSpecial", json!({})),
                        "ui:insertPaste" => app.open_dialog("insertPaste", json!({})),
                        "ui:insertCells" => app.open_dialog("insertCells", json!({})),
                        "ui:deleteCells" => app.open_dialog("deleteCells", json!({})),
                        "ui:formatCells" => app.open_dialog("formatCells", json!({})),
                        "ui:newComment" => app.open_dialog("comment", json!({"threaded": true})),
                        "ui:newNote" => app.open_dialog("comment", json!({"threaded": false})),
                        "ui:defineName" => app.open_dialog("defineName", json!({})),
                        "ui:insertLink" => app.open_dialog("insertLink", json!({})),
                        "ui:askAgent" => crate::ask_agent::open_from_ui(app),
                        "ui:pickList" => app.open_dialog("pickList", json!({})),
                        "ui:filterValue" => {
                            if let Some(d) = app.session.active() {
                                let a = d.selection.active;
                                let text = d.wb.active().map(|sh| gridcraft_engine::display::cell_text(&d.wb, sh, a)).unwrap_or_default();
                                let has = d.wb.active().is_some_and(|sh| sh.autofilter.is_some());
                                if !has {
                                    let _ = app.run("data.filter", json!({}));
                                }
                                app.run_or_alert("data.filterBy", json!({"column": col_to_letters(a.col), "values": [text]}));
                            }
                        }
                        "edit.copy" | "edit.cut" => {
                            app.copy_to_clipboard(ui.ctx(), id);
                        }
                        "edit.paste" => app.paste_from_clipboard(),
                        other => app.run_or_alert(other, json!({})),
                    }
                }
            }
        });
    });
    // A quick right-click presses and releases in one frame, and the menu's corner snaps to the
    // nearest physical pixel, which can put the pointer just outside it: that press opened the
    // menu, so it doesn't count as a click elsewhere.
    let opened_now = app.grid.context_menu_frame == ui.ctx().cumulative_frame_nr();
    if close
        || ui.input(|i| i.key_pressed(egui::Key::Escape))
        || (!opened_now
            && ui.input(|i| i.pointer.any_pressed())
            && !resp.response.contains_pointer()
            && !resp.response.hovered()
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !resp.response.rect.contains(p)))
    {
        app.grid.context_menu = None;
    }
}

/// Whether the cell menu offers Show Details (Excel's right-click › Show Details): the active cell
/// is a value, subtotal or grand total cell of a PivotTable, or a cell of a report that is out of
/// date, where running the command says so. Labels and headers of a current report don't offer it,
/// and no cell does while the PivotTable's Enable show details option is off.
fn show_details_offered(app: &mut SheetApp) -> bool {
    let Some(d) = app.session.active() else { return false };
    let (si, at) = (d.wb.active_sheet(), d.selection.active);
    let Some((pi, pt)) = d.wb.active().and_then(|sh| sh.pivots.iter().enumerate().find(|(_, pt)| pt.last_range.is_some_and(|r| r.contains(at))))
    else {
        return false;
    };
    if !pt.enable_show_details {
        return false;
    }
    let origin = pt.last_range.map(|r| r.start).unwrap_or_default();
    let cell = (at.row.saturating_sub(origin.row), at.col.saturating_sub(origin.col));
    let key = (d.uid, d.revision, si, pi);
    if let Some((k, cells)) = &app.grid.show_details_cells
        && *k == key
    {
        return cells.as_ref().is_none_or(|c| c.contains(&cell));
    }
    let cells = gridcraft_engine::pivot::detail_cells(&d.wb, pt, si).ok();
    let on = cells.as_ref().is_none_or(|c| c.contains(&cell));
    app.grid.show_details_cells = Some((key, cells));
    on
}

/// The ways to fill a dragged range, as the fill menu and the Auto Fill Options button list them.
const FILL_MODES: [(&str, &str); 4] =
    [("Copy Cells", "copy"), ("Fill Series", "extend"), ("Fill Formatting Only", "formats"), ("Fill Without Formatting", "values")];

/// The ways to fill from a date, which both menus add after [`FILL_MODES`] when the source starts
/// with one.
const DATE_FILL_MODES: [(&str, &str); 4] = [("Fill Days", "days"), ("Fill Weekdays", "weekdays"), ("Fill Months", "months"), ("Fill Years", "years")];

/// The fill modes the menus list for a fill from `src` on the active sheet.
fn fill_modes(app: &SheetApp, src: RangeRef) -> Vec<(&'static str, &'static str)> {
    let dates = app.session.active().is_some_and(|d| gridcraft_engine::fill::starts_with_date(&d.wb, d.wb.active_sheet(), src));
    FILL_MODES.into_iter().chain(DATE_FILL_MODES.into_iter().filter(|_| dates)).collect()
}

/// Remembers the fill just made so the Auto Fill Options button can offer the other ways.
pub(super) fn set_fill_tag(app: &mut SheetApp, src: RangeRef, target: RangeRef) {
    app.grid.fill_tag =
        app.session.active().map(|d| FillTag { src, target, uid: d.uid, sheet: d.wb.active_sheet(), revision: d.revision, undo_len: d.undo.len() });
}

/// The Auto Fill Options button below the bottom-right corner of cells a left-drag of the fill
/// handle just filled. Its menu redoes that fill as Copy Cells, Fill Series, Fill Formatting Only
/// or Fill Without Formatting, and from a date also as Fill Days, Fill Weekdays, Fill Months or
/// Fill Years: it undoes the drag's fill, then runs `edit.autoFill` in that mode.
/// The button goes away as soon as anything else changes the workbook or the selection moves.
pub(super) fn fill_options_tag(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet) {
    let Some(tag) = app.grid.fill_tag.clone() else { return };
    let current = app.session.active().is_some_and(|d| {
        d.uid == tag.uid
            && d.wb.active_sheet() == tag.sheet
            && d.revision == tag.revision
            && d.undo.len() == tag.undo_len
            && d.selection.ranges.len() == 1
            && d.selection.ranges.first().is_some_and(|r| *r == tag.target)
    });
    if !current {
        app.grid.fill_tag = None;
        return;
    }
    if app.editor.is_some() || app.dialog.is_some() || app.grid.drag != Drag::None {
        return;
    }
    let size = vec2(18.0, 18.0);
    let corner = geo.range_rect(sh, tag.target).right_bottom();
    let button = geo.mirror_rect(Rect::from_min_size(corner + vec2(2.0, 2.0), size));
    // `button` is in screen space, so compare it with the mirrored cells area.
    if !geo.mirror_rect(geo.cells).contains_rect(button) {
        return;
    }
    let lang = crate::i18n::current(ui.ctx());
    let resp = ui.interact(button, egui::Id::unique("gridcraft.fill_options_tag"), Sense::click()).on_hover_text(lang.tr("Auto Fill Options"));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, lang.tr("Auto Fill Options")));
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    p.rect_filled(button, 3.0, if resp.hovered() { t.hover } else { t.window });
    p.rect_stroke(button, 3.0, Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    // Two small cells and a drop-down arrow.
    let c = button.center();
    p.rect_stroke(Rect::from_min_size(pos2(c.x - 6.0, c.y - 6.0), vec2(6.0, 6.0)), 0.0, Stroke::new(1.0, t.text), egui::StrokeKind::Inside);
    p.rect_stroke(Rect::from_min_size(pos2(c.x - 3.0, c.y - 3.0), vec2(6.0, 6.0)), 0.0, Stroke::new(1.0, t.text), egui::StrokeKind::Inside);
    p.add(egui::Shape::convex_polygon(
        vec![pos2(c.x + 1.0, c.y + 5.0), pos2(c.x + 7.0, c.y + 5.0), pos2(c.x + 4.0, c.y + 8.0)],
        t.text,
        Stroke::NONE,
    ));
    let mut chosen = None;
    let modes = fill_modes(app, tag.src);
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(190.0);
        for (label, mode) in modes {
            if ui.add(egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(180.0, 20.0))).clicked() {
                chosen = Some(mode);
                ui.close();
            }
        }
    });
    if let Some(mode) = chosen {
        if let Err(e) = app.run("edit.undo", json!({})) {
            app.grid.fill_tag = None;
            app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
            return;
        }
        match app.run("edit.autoFill", json!({"source": tag.src.a1(), "target": tag.target.a1(), "mode": mode})) {
            Ok(_) => set_fill_tag(app, tag.src, tag.target),
            Err(e) => {
                app.grid.fill_tag = None;
                app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
            }
        }
    }
}

/// The menu a right-drag of the fill handle opens on release, as in Excel.
pub(super) fn fill_menu(app: &mut SheetApp, ui: &mut egui::Ui, was_open: bool) {
    let Some((src, target, at)) = app.grid.fill_menu else { return };
    let mut close = false;
    let mut items = fill_modes(app, src);
    items.extend([("-", ""), ("Series…", "ui:series")]);
    let response = egui::Area::new(egui::Id::unique("fill_menu")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(190.0);
            let lang = crate::i18n::current(ui.ctx());
            for (label, mode) in items {
                if label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(180.0, 20.0))).clicked() {
                    close = true;
                    if mode == "ui:series" {
                        let _ = app.session.run("selection.set", json!({"range": target.union(&src).a1(), "active": src.start.a1()}));
                        let direction = if target.width() != src.width() { "rows" } else { "columns" };
                        app.open_dialog("series", json!({"direction": direction}));
                    } else if let Err(e) = app.run("edit.autoFill", json!({"source": src.a1(), "target": target.a1(), "mode": mode})) {
                        app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                    }
                }
            }
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        app.grid.fill_menu = None;
    }
}

/// The right-click menu of a chart, picture, shape or slicer.
pub(super) fn object_menu(app: &mut SheetApp, ui: &mut egui::Ui) {
    let Some((at, kind, id)) = app.grid.object_menu else { return };
    let mut close = false;
    let resp = egui::Area::new(egui::Id::unique("object_context")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(200.0);
            let mut items = vec![("Delete", "object.delete")];
            if kind == "chart" {
                items.extend([("-", ""), ("Select Data…", "ui:selectData"), ("Format Chart Area…", "ui:formatChart")]);
            }
            let lang = crate::i18n::current(ui.ctx());
            for (label, action) in items {
                if label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(190.0, 20.0))).clicked() {
                    close = true;
                    match action {
                        "ui:selectData" => app.open_dialog("selectData", json!({"chart": id})),
                        "ui:formatChart" => app.grid.pane = Some("formatChart".into()),
                        _ => {
                            app.run_or_alert("object.delete", json!({"kind": kind, "id": id}));
                            app.selected_chart = None;
                        }
                    }
                }
            }
        });
    });
    if close
        || ui.input(|i| i.key_pressed(egui::Key::Escape))
        || (ui.input(|i| i.pointer.any_pressed())
            && !resp.response.contains_pointer()
            && !resp.response.hovered()
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !resp.response.rect.contains(p)))
    {
        app.grid.object_menu = None;
    }
}

/// The warning button beside an active cell holding a number stored as text, as in Excel: its
/// menu names the problem and offers Convert to Number for the selection (#1994).
pub(super) fn error_check_tag(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet) {
    if app.editor.is_some() || app.dialog.is_some() || sh.show_formulas || app.grid.drag != Drag::None {
        return;
    }
    let Some(active) = app.session.active().map(|d| d.selection.active) else { return };
    if sh.cell(active).and_then(gridcraft_engine::cmd::formulas::number_stored_as_text).is_none() {
        return;
    }
    let cell = geo.cell_rect(sh, active);
    let size = vec2(18.0, 18.0);
    // Left of the cell, or right of it when that would cover the row headings.
    let left = if cell.left() - size.x - 2.0 >= geo.cells.left() { cell.left() - size.x - 2.0 } else { cell.right() + 2.0 };
    let tag = geo.mirror_rect(Rect::from_min_size(pos2(left, cell.top()), size));
    if !geo.cells.intersects(tag) {
        return;
    }
    let lang = crate::i18n::current(ui.ctx());
    let resp = ui.interact(tag, egui::Id::unique("gridcraft.error_check_tag"), Sense::click()).on_hover_text(lang.tr("Number Stored as Text"));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, lang.tr("Error checking options")));
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    p.rect_filled(tag, 3.0, if resp.hovered() { t.hover } else { t.window });
    p.rect_stroke(tag, 3.0, Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    // A warning sign: an amber diamond with an exclamation mark.
    let c = tag.center();
    let r = 6.0;
    p.add(egui::Shape::convex_polygon(
        vec![pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r), pos2(c.x - r, c.y)],
        t.warning_fill,
        Stroke::NONE,
    ));
    p.text(c, egui::Align2::CENTER_CENTER, "!", egui::FontId::proportional(10.0), t.warning_ink);
    egui::Popup::menu(&resp).show(|ui| {
        ui.label(egui::RichText::new(lang.tr("Number Stored as Text")).strong());
        ui.separator();
        if ui.button(lang.tr("Convert to Number")).clicked() {
            app.run_or_alert("formulas.convertToNumber", json!({}));
            ui.close();
        }
    });
}

/// Draws row/column header context menus with insert/delete, format, size, hide/unhide and group actions, plus Insert Copied Cells when available.
pub(super) fn header_menu(app: &mut SheetApp, ui: &mut egui::Ui, was_open: bool) {
    let Some((at, rows)) = app.grid.header_menu else { return };
    let mut close = false;
    let response = egui::Area::new(egui::Id::unique("header_menu")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(190.0);
            let insert_copied = insert_copied_item(app);
            let mut items: Vec<(&str, &str)> = if rows {
                vec![
                    ("Cut", "edit.cut"),
                    ("Copy", "edit.copy"),
                    ("Paste", "edit.paste"),
                    ("-", ""),
                    ("Insert", "home.insertRows"),
                    ("Delete", "home.deleteRows"),
                    ("Clear Contents", "edit.clearContents"),
                    ("-", ""),
                    ("Format Cells…", "ui:formatCells"),
                    ("Row Height…", "ui:rowHeight"),
                    ("AutoFit Row Height", "home.autofitRowHeight"),
                    ("Hide", "home.hideRows"),
                    ("Unhide", "home.unhideRows"),
                    ("Group", "data.group"),
                ]
            } else {
                vec![
                    ("Cut", "edit.cut"),
                    ("Copy", "edit.copy"),
                    ("Paste", "edit.paste"),
                    ("-", ""),
                    ("Insert", "home.insertColumns"),
                    ("Delete", "home.deleteColumns"),
                    ("Clear Contents", "edit.clearContents"),
                    ("-", ""),
                    ("Format Cells…", "ui:formatCells"),
                    ("Column Width…", "ui:columnWidth"),
                    ("AutoFit Column Width", "home.autofitColumnWidth"),
                    ("Hide", "home.hideColumns"),
                    ("Unhide", "home.unhideColumns"),
                    ("Sort A to Z", "data.sortAscending"),
                    ("Group", "data.group"),
                ]
            };
            if let Some(item) = insert_copied {
                items.insert(5, item);
            }
            let lang = crate::i18n::current(ui.ctx());
            for (label, id) in items {
                if label == "-" {
                    ui.separator();
                    continue;
                }
                if ui.add(egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(180.0, 20.0))).clicked() {
                    close = true;
                    match id {
                        "ui:insertPaste" => app.open_dialog("insertPaste", json!({})),
                        "ui:rowHeight" => app.open_dialog("rowHeight", json!({})),
                        "ui:formatCells" => app.open_dialog("formatCells", json!({})),
                        "ui:columnWidth" => app.open_dialog("columnWidth", json!({})),
                        "edit.copy" | "edit.cut" => {
                            app.copy_to_clipboard(ui.ctx(), id);
                        }
                        "edit.paste" => app.paste_from_clipboard(),
                        other => app.run_or_alert(other, json!({})),
                    }
                }
            }
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        app.grid.header_menu = None;
    }
}

/// The context-menu entry for Insert Copied Cells / Insert Cut Cells, when the clipboard holds a
/// copy or cut of this app: whole rows or columns insert at once, a block asks which way to shift
/// (Insert Paste). A cut block can only be pasted, so it gets no entry.
fn insert_copied_item(app: &SheetApp) -> Option<(&'static str, &'static str)> {
    let clip = app.session.clipboard.as_ref()?;
    let lines = clip.selected.is_full_rows() != clip.selected.is_full_cols();
    let cut = clip.cut && app.session.active().is_some_and(|d| d.uid == clip.doc_uid);
    match (cut, lines) {
        (true, true) => Some(("Insert Cut Cells", "home.insertCopiedCells")),
        (true, false) => None,
        (false, true) => Some(("Insert Copied Cells", "home.insertCopiedCells")),
        (false, false) => Some(("Insert Copied Cells…", "ui:insertPaste")),
    }
}

pub(super) fn is_checkbox(sh: &Sheet, app: &SheetApp) -> bool {
    let Some(d) = app.session.active() else { return false };
    d.wb.styles.get(sh.style_id(d.selection.active)).num_fmt.as_str() == "checkbox"
}
