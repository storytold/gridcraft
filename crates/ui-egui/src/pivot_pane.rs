//! The PivotTable Fields task pane: shown on the right when the active cell is in a pivot
//! report. Ticking a field adds it (text → Rows, numbers → Values); each area lists its fields
//! with drag/drop and move/remove menus. Everything runs `pivot.*` commands.

use egui::{Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n;
use crate::theme::{self, Tokens};

const VALUES_FIELD: &str = "Σ Values";

struct AreaField {
    name: String,
    source: String,
    index: usize,
    virtual_values: bool,
}

/// Keep the engine index alongside each visible item. Report-filter selections can be
/// hidden, and Σ Values has a visible slot but no entry in either axis array.
fn area_fields(info: &Json, key: &str) -> Vec<AreaField> {
    let mut fields: Vec<_> = info["pivot"][key]
        .as_array()
        .or(info[key].as_array())
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, f)| key != "filters" || f["report"].as_bool().unwrap_or(true))
        .filter_map(|(index, f)| {
            let source = f["field"].as_str().or(f["sourceCol"].as_str()).or(f.as_str())?;
            Some(AreaField { name: f["name"].as_str().unwrap_or(source).to_string(), source: source.to_string(), index, virtual_values: false })
        })
        .collect();
    let values_key = if info["valuesOnRows"].as_bool().unwrap_or(false) { "rows" } else { "columns" };
    if key == values_key && info["values"].as_array().is_some_and(|values| values.len() > 1) {
        let at = info["valuesPosition"].as_u64().map_or(fields.len(), |p| p as usize).min(fields.len());
        fields.insert(at, AreaField { name: VALUES_FIELD.into(), source: VALUES_FIELD.into(), index: at, virtual_values: true });
    }
    fields
}

/// Convert a visual insertion slot into an engine position. The engine stores ordinary
/// axis fields separately from Σ Values, while report filters can have hidden entries.
fn area_drop_position(info: &Json, key: &str, drag: &FieldDrag, slot: usize) -> Option<usize> {
    let items = area_fields(info, key);
    let slot = slot.min(items.len());
    if drag.virtual_values {
        if !matches!(key, "rows" | "columns") {
            return None;
        }
        let position = items[..slot].iter().filter(|item| !item.virtual_values).count();
        let old = items.iter().position(|item| item.virtual_values);
        if key == "rows" && old.is_some_and(|old| old == slot || old + 1 == slot) {
            return None;
        }
        if key == "columns" && drag.from.as_ref().is_some_and(|(from, _)| from == key) {
            return None;
        }
        return Some(position);
    }
    let existing = if key == "values" {
        drag.from.as_ref().filter(|(from, _)| from == key).map(|(_, index)| *index)
    } else {
        items.iter().find(|item| !item.virtual_values && item.source.eq_ignore_ascii_case(&drag.source)).map(|item| item.index)
    };
    let position = if key == "filters" {
        items.get(slot).map_or_else(|| info[key].as_array().map_or(0, Vec::len), |item| item.index)
    } else {
        items[..slot].iter().filter(|item| !item.virtual_values).count()
    };
    // Moving an ordinary row across Σ Values can leave its ordinary-array index
    // unchanged. The engine still needs the move to shift valuesPosition.
    if let Some(old) = existing
        && matches!(key, "rows" | "columns")
        && let Some(old_visible) = items.iter().position(|item| !item.virtual_values && item.index == old)
        && items.iter().any(|item| item.virtual_values)
        && slot.saturating_sub(usize::from(old_visible < slot)) != old_visible
        && position.saturating_sub(usize::from(old < position)) == old
    {
        return Some(old);
    }
    drop_position(drag, key, position, existing)
}

/// Preserve the visible side of Σ Values in the same engine edit as the field drop.
fn drop_values_position(info: &Json, key: &str, drag: &FieldDrag, slot: usize) -> Option<usize> {
    if key != "rows" || drag.virtual_values {
        return None;
    }
    let items = area_fields(info, key);
    let virtual_slot = items.iter().position(|item| item.virtual_values)?;
    let existing = items.iter().position(|item| !item.virtual_values && item.source.eq_ignore_ascii_case(&drag.source));
    let before = items[..virtual_slot].iter().filter(|item| !item.virtual_values).count();
    Some(before.saturating_sub(usize::from(existing.is_some_and(|i| i < virtual_slot))) + usize::from(slot <= virtual_slot))
}

/// An assigned Values field is identified by its summary caption, not its source column:
/// the same source can have several summaries with different settings.
#[derive(Clone)]
struct FieldDrag {
    context: (u64, u64, usize, String),
    field: String,
    source: String,
    from: Option<(String, usize)>,
    calculated: bool,
    virtual_values: bool,
}

fn drag_context(app: &SheetApp, pivot: &str) -> Option<(u64, u64, usize, String)> {
    let d = app.session.active()?;
    Some((d.uid, d.revision, d.wb.active_sheet, pivot.to_string()))
}

fn field_source(
    response: &egui::Response,
    context: &(u64, u64, usize, String),
    field: &str,
    source: &str,
    from: Option<(&str, usize)>,
    calculated: bool,
    virtual_values: bool,
) {
    response.dnd_set_drag_payload(FieldDrag {
        context: context.clone(),
        field: field.to_string(),
        source: source.to_string(),
        from: from.map(|(a, i)| (a.to_string(), i)),
        calculated,
        virtual_values,
    });
}

/// UI insertion slots count the item being dragged; the engine's position counts the list
/// after removal. Returning None avoids a refresh/undo entry for a no-op reorder.
fn drop_position(drag: &FieldDrag, area: &str, slot: usize, existing: Option<usize>) -> Option<usize> {
    if let Some(old) = existing {
        let position = slot.saturating_sub(usize::from(old < slot));
        // A source-list drop onto its own categorical role is also a reorder.
        let reorder = drag.from.as_ref().is_none_or(|(from, _)| from == area);
        return (!reorder || position != old).then_some(position);
    }
    Some(slot)
}

fn scroll_drag_edge(ui: &Ui, scroll: &mut egui::scroll_area::ScrollAreaOutput<()>) {
    if egui::DragAndDrop::has_payload_of_type::<FieldDrag>(ui.ctx())
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pointer) = ui.ctx().pointer_interact_pos()
        && scroll.inner_rect.intersect(ui.clip_rect()).contains(pointer)
    {
        let rect = scroll.inner_rect;
        let direction = if pointer.y < rect.top() + 16.0 {
            -1.0
        } else if pointer.y > rect.bottom() - 16.0 {
            1.0
        } else {
            0.0
        };
        let delta = ui.input(|i| i.stable_dt.min(0.1)) * 180.0 * direction;
        let maximum = (scroll.content_size.y - rect.height()).max(0.0);
        let offset = (scroll.state.offset.y + delta).clamp(0.0, maximum);
        if offset != scroll.state.offset.y {
            scroll.state.offset.y = offset;
            scroll.state.store(ui.ctx(), scroll.id);
            ui.ctx().request_repaint();
        }
    }
}
/// Whether the active cell is inside a pivot report (so the pane should show).
pub fn active_pivot(app: &SheetApp) -> Option<String> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let a = d.selection.active;
    sh.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.contains(a)) || p.anchor == a).map(|p| p.name.clone())
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let Some(name) = active_pivot(app) else {
        if egui::DragAndDrop::has_payload_of_type::<FieldDrag>(ui.ctx()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
        }
        return;
    };
    let Some(context) = drag_context(app, &name) else { return };
    if let Some(drag) = egui::DragAndDrop::payload::<FieldDrag>(ui.ctx())
        && drag.context != context
    {
        egui::DragAndDrop::clear_payload(ui.ctx());
    }
    let t = Tokens::get(ui.ctx());
    let lang = i18n::current(ui.ctx());
    let info = match app.session.run("pivot.fields", json!({"pivot": name})) {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut drop = None;
    let pane = egui::Panel::right("pivot_pane").default_size(280.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(
        ui,
        |ui| {
            ui.label(egui::RichText::new(lang.tr("PivotTable Fields")).font(theme::ui_bold(15.0)));
            ui.label(egui::RichText::new(lang.tr("Choose fields to add to report:")).small().color(t.text_dim));
            ui.add_space(4.0);
            let fields = info["fields"].as_array().cloned().unwrap_or_default();
            // Reserve the four area boxes and footer before allocating the source list.
            let source_height = (ui.available_height() - 280.0).clamp(32.0, 220.0);
            egui::ScrollArea::vertical().min_scrolled_height(0.0).max_height(source_height).id_salt("pv_fields").show(ui, |ui| {
                for f in &fields {
                    let fname = f["name"].as_str().unwrap_or("").to_string();
                    let areas: Vec<String> =
                        f["areas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    let mut on = !areas.is_empty();
                    let numeric = f["numeric"].as_bool().unwrap_or(false);
                    let r = ui.checkbox(&mut on, egui::RichText::new(&fname).font(theme::ui_font(13.0)));
                    // Keep the checkbox's changed flag; `interact` returns a new response.
                    field_source(
                        &r.interact(Sense::click_and_drag()),
                        &context,
                        &fname,
                        &fname,
                        None,
                        f["calculated"].as_bool().unwrap_or(false),
                        false,
                    );
                    if r.changed() {
                        let res = if on {
                            app.run("pivot.addField", json!({"pivot": name, "field": fname, "area": if numeric { "values" } else { "rows" }}))
                        } else {
                            app.run("pivot.removeField", json!({"pivot": name, "field": fname}))
                        };
                        if let Err(e) = res {
                            app.message = Some(("PivotTable".into(), crate::clean_error(&e)));
                        }
                    }
                    let samples: Vec<String> =
                        f["samples"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    r.on_hover_text(samples.join(", "));
                }
            });
            ui.separator();
            ui.label(egui::RichText::new(lang.tr("Use a field's menu to move it between areas:")).small().color(t.text_dim));
            let area_height = ((ui.available_height() - 110.0) * 0.5).clamp(48.0, 140.0);
            ui.columns(2, |cols| {
                for (i, (key, label)) in [("filters", "Filters"), ("columns", "Columns"), ("rows", "Rows"), ("values", "Values")].iter().enumerate() {
                    let ui = &mut cols[i % 2];
                    ui.label(egui::RichText::new(lang.tr(label)).strong());
                    let mut items = Vec::<Rect>::new();
                    let frame = egui::Frame::NONE.fill(t.window).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                        ui.set_min_size(vec2(ui.available_width(), area_height.min(60.0)));
                        let mut scroll =
                            egui::ScrollArea::vertical().id_salt(("pv_area", key)).min_scrolled_height(0.0).max_height(area_height).show(ui, |ui| {
                                for f in area_fields(&info, key) {
                                    let b = ui.add(
                                        egui::Button::new(egui::RichText::new(format!("{} ▾", f.name)).font(theme::ui_font(12.0)))
                                            .fill(t.input_bg)
                                            .sense(Sense::click_and_drag())
                                            .min_size(vec2(ui.available_width(), 20.0)),
                                    );
                                    items.push(b.rect);
                                    let calculated = fields.iter().any(|source| {
                                        source["name"].as_str() == Some(f.source.as_str()) && source["calculated"].as_bool().unwrap_or(false)
                                    });
                                    field_source(&b, &context, &f.name, &f.source, Some((key, f.index)), calculated, f.virtual_values);
                                    egui::Popup::menu(&b).show(|ui| {
                                        for (to, l) in [
                                            ("rows", "Move to Row Labels"),
                                            ("columns", "Move to Column Labels"),
                                            ("filters", "Move to Report Filter"),
                                            ("values", "Move to Values"),
                                        ] {
                                            if to != *key
                                                && (!f.virtual_values || matches!(to, "rows" | "columns"))
                                                && ui.button(lang.tr(l)).clicked()
                                            {
                                                app.run_or_alert("pivot.moveField", json!({"pivot": name, "field": f.name, "from": key, "to": to}));
                                            }
                                        }
                                        if f.virtual_values {
                                            return;
                                        }
                                        if *key == "values" {
                                            ui.separator();
                                            for (func, l) in [
                                                ("sum", "Summarize by Sum"),
                                                ("count", "Summarize by Count"),
                                                ("average", "Summarize by Average"),
                                                ("max", "Summarize by Max"),
                                                ("min", "Summarize by Min"),
                                            ] {
                                                if ui.button(lang.tr(l)).clicked() {
                                                    app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f.name, "func": func}));
                                                }
                                            }
                                            for (sa, l) in [
                                                ("normal", "Show as No Calculation"),
                                                ("percentOfGrandTotal", "Show as % of Grand Total"),
                                                ("runningTotal", "Show as Running Total"),
                                                ("rank", "Show as Rank"),
                                            ] {
                                                if ui.button(lang.tr(l)).clicked() {
                                                    app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f.name, "showAs": sa}));
                                                }
                                            }
                                        } else {
                                            ui.separator();
                                            for (o, l) in [("asc", "Sort A to Z"), ("desc", "Sort Z to A")] {
                                                if ui.button(lang.tr(l)).clicked() {
                                                    app.run_or_alert("pivot.sort", json!({"pivot": name, "field": f.name, "order": o}));
                                                }
                                            }
                                            for (g, l) in [
                                                ("years", "Group by Years"),
                                                ("quarters", "Group by Quarters"),
                                                ("months", "Group by Months"),
                                                ("none", "Ungroup"),
                                            ] {
                                                if ui.button(lang.tr(l)).clicked() {
                                                    app.run_or_alert("pivot.group", json!({"pivot": name, "field": f.name, "by": g}));
                                                }
                                            }
                                        }
                                        ui.separator();
                                        if ui.button(lang.tr("Remove Field")).clicked() {
                                            app.run_or_alert("pivot.removeField", json!({"pivot": name, "field": f.name, "area": key}));
                                        }
                                    });
                                }
                            });
                        scroll_drag_edge(ui, &mut scroll);
                    });
                    if let Some(drag) = egui::DragAndDrop::payload::<FieldDrag>(ui.ctx())
                        && drag.context == context
                        && (!drag.calculated || *key == "values")
                        && (!drag.virtual_values || matches!(*key, "rows" | "columns"))
                        && let Some(pointer) = ui.ctx().pointer_interact_pos()
                        && frame.response.rect.intersect(ui.clip_rect()).contains(pointer)
                    {
                        let slot = items.iter().take_while(|r| pointer.y > r.center().y).count();
                        let marker_y = items
                            .get(slot)
                            .map(|r| r.top() - 2.0)
                            .or_else(|| items.last().map(|r| r.bottom() + 2.0))
                            .unwrap_or(frame.response.rect.top() + 5.0);
                        let rect = frame.response.rect.shrink(4.0);
                        let y = marker_y.clamp(rect.top(), rect.bottom());
                        ui.painter().line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(2.0, t.accent));
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        if ui.input(|i| i.pointer.primary_released()) {
                            let position = area_drop_position(&info, key, &drag, slot);
                            let values_position = drop_values_position(&info, key, &drag, slot);
                            drop = Some((drag, *key, position, values_position));
                        }
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(lang.tr("Refresh")).clicked() {
                    app.run_or_alert("pivot.refresh", json!({"pivot": name}));
                }
                if ui.button(lang.tr("Calculated Field…")).clicked() {
                    app.run_or_alert("pivot.calculatedField", json!({"pivot": name}));
                }
                egui::ComboBox::from_id_salt("pv_layout").selected_text(lang.tr("Report Layout")).show_ui(ui, |ui| {
                    for (l, v) in [("Compact", "compact"), ("Outline", "outline"), ("Tabular", "tabular")] {
                        if ui.button(lang.tr(l)).clicked() {
                            app.run_or_alert("pivot.layout", json!({"pivot": name, "layout": v}));
                        }
                    }
                });
            });
            let _: Option<Json> = None;
        },
    );
    if let Some(drag) = egui::DragAndDrop::payload::<FieldDrag>(ui.ctx()) {
        if drag.context != context || drag_context(app, &name).as_ref() != Some(&context) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            return;
        }
        if ui.input(|i| i.pointer.primary_released()) {
            if let Some((drag, to, position, values_position)) = drop {
                if let Some(position) = position {
                    let (command, mut params) = if let Some((from, _)) = &drag.from {
                        ("pivot.moveField", json!({"pivot":name,"field":drag.field,"from":from,"to":to,"position":position}))
                    } else {
                        ("pivot.addField", json!({"pivot":name,"field":drag.field,"area":to,"position":position}))
                    };
                    if let Some(values_position) = values_position {
                        params["valuesPosition"] = json!(values_position);
                    }
                    app.run_or_alert(command, params);
                }
            } else if let Some(pointer) = ui.ctx().pointer_interact_pos()
                && !pane.response.rect.contains(pointer)
                && let Some((from, _)) = &drag.from
                && !drag.virtual_values
            {
                app.run_or_alert("pivot.removeField", json!({"pivot":name,"field":drag.field,"area":from}));
            }
            egui::DragAndDrop::clear_payload(ui.ctx());
        } else if let Some(pointer) = ui.ctx().pointer_interact_pos() {
            egui::Area::new(egui::Id::new("pivot_drag_label"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pointer + vec2(12.0, 12.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(140.0);
                        ui.label(&drag.field);
                    });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(field: &str, source: &str, from: Option<(&str, usize)>, virtual_values: bool) -> FieldDrag {
        FieldDrag {
            context: (0, 0, 0, String::new()),
            field: field.into(),
            source: source.into(),
            from: from.map(|(area, index)| (area.into(), index)),
            calculated: false,
            virtual_values,
        }
    }

    #[test]
    fn virtual_field_and_duplicate_summaries_keep_their_positions() {
        let info = json!({
            "rows": [{"field":"Region"}, {"field":"Product"}],
            "columns": [],
            "values": [
                {"field":"Sales","name":"Sum of Sales"},
                {"field":"Sales","name":"Average of Sales"}
            ],
            "filters": [],
            "valuesOnRows": true,
            "valuesPosition": 1
        });
        let rows = area_fields(&info, "rows");
        assert_eq!(rows.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(), ["Region", VALUES_FIELD, "Product"]);
        assert_eq!(rows.iter().map(|field| field.index).collect::<Vec<_>>(), [0, 1, 1]);
        let values = area_fields(&info, "values");
        assert_eq!(values.iter().map(|field| field.index).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(area_drop_position(&info, "rows", &drag(VALUES_FIELD, VALUES_FIELD, Some(("rows", 1)), true), 0), Some(0));
        assert_eq!(area_drop_position(&info, "values", &drag(VALUES_FIELD, VALUES_FIELD, Some(("rows", 1)), true), 0), None);
        assert_eq!(area_drop_position(&info, "values", &drag("Average of Sales", "Sales", Some(("values", 1)), false), 0), Some(0));
    }

    #[test]
    fn hidden_report_filter_does_not_change_the_underlying_index() {
        let info = json!({
            "filters": [
                {"field":"Region","report":false},
                {"field":"Product","report":true},
                {"field":"Units","report":true}
            ],
            "values": [],
            "rows": [],
            "columns": []
        });
        let filters = area_fields(&info, "filters");
        assert_eq!(filters.iter().map(|field| field.index).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(area_drop_position(&info, "filters", &drag("Units", "Units", Some(("filters", 2)), false), 0), Some(1));
    }
}
