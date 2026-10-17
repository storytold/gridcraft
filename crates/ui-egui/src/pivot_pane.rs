//! The PivotTable Fields task pane: shown on the right when the active cell is in a pivot
//! report. Ticking a field adds it (text → Rows, numbers → Values); each area lists its fields
//! with move/remove menus. Fields can also be dragged from the list into an area, between
//! areas, within an area to reorder, and out of the areas (onto the list or off the pane) to
//! remove them. Everything runs `pivot.*` commands.

use egui::{Color32, CursorIcon, DragAndDrop, Pos2, Rect, Sense, Stroke, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n;
use crate::theme::{self, Tokens};

/// Whether the active cell is inside a pivot report (so the pane should show).
pub fn active_pivot(app: &SheetApp) -> Option<String> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let a = d.selection.active;
    sh.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.contains(a)) || p.anchor == a).map(|p| p.name.clone())
}

/// The pseudo-field standing for the value fields once there are two or more of them.
const VALUES_FIELD: &str = "Σ Values";

/// The four layout areas, in pane order (Filters and Rows in the left column).
const AREAS: [(&str, &str); 4] = [("filters", "Filters"), ("columns", "Columns"), ("rows", "Rows"), ("values", "Values")];

/// A field being dragged in the pane: from the field list (`from: None`) or out of an area.
#[derive(Clone, Debug, PartialEq)]
pub struct PivotDrag {
    pub field: String,
    pub from: Option<&'static str>,
}

/// Whether `area` takes `drag` (Σ Values only lives in Rows or Columns).
fn accepts(drag: &PivotDrag, area: &str) -> bool {
    drag.field != VALUES_FIELD || (drag.from.is_some() && matches!(area, "rows" | "columns"))
}

/// The commands that drop `drag` into `to` at `slot`: the gap before the `slot`th entry of
/// `shown` (the area's entries as the pane lists them, Σ Values included). Empty when the drop
/// changes nothing.
pub fn plan_drop(pivot: &str, drag: &PivotDrag, to: &'static str, shown: &[String], slot: usize) -> Vec<(&'static str, Json)> {
    let axis = matches!(to, "rows" | "columns");
    // Entries that leave `to` before the drop lands: the dragged field itself when it is moved
    // within the area, or re-added to an axis it already sits in (axes hold a field once).
    let leaves = |f: &String| *f == drag.field && (drag.from == Some(to) || (axis && drag.field != VALUES_FIELD));
    if let Some(i) = shown.iter().position(&leaves)
        && (slot == i || slot == i + 1)
    {
        return Vec::new();
    }
    // The engine's `position` counts the area's fields, never Σ Values.
    let pos = shown.iter().take(slot).filter(|f| *f != VALUES_FIELD && !leaves(f)).count();
    if drag.field == VALUES_FIELD {
        let Some(from) = drag.from else { return Vec::new() };
        return vec![("pivot.moveField", json!({"pivot": pivot, "field": VALUES_FIELD, "from": from, "to": to, "position": pos}))];
    }
    let mut cmds = vec![match drag.from {
        Some(from) => ("pivot.moveField", json!({"pivot": pivot, "field": drag.field, "from": from, "to": to, "position": pos})),
        None => ("pivot.addField", json!({"pivot": pivot, "field": drag.field, "area": to, "position": pos})),
    }];
    // Σ Values stays just ahead of the field it preceded, so a field dropped straight after it
    // in Rows would land ahead of it: put Σ Values back in front of the dropped field.
    if to == "rows"
        && let Some(va) = shown.iter().position(|f| f == VALUES_FIELD)
        && slot > va
        && pos == shown.iter().take(va).filter(|f| !leaves(f)).count()
    {
        cmds.push(("pivot.moveField", json!({"pivot": pivot, "field": VALUES_FIELD, "from": "rows", "to": "rows", "position": pos})));
    }
    cmds
}

/// The gap the pointer at `y` points at among entries laid out at `rects`.
fn slot_at(rects: &[Rect], y: f32) -> usize {
    rects.iter().filter(|r| r.center().y < y).count()
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let Some(name) = active_pivot(app) else { return };
    let t = Tokens::get(ui.ctx());
    let lang = i18n::current(ui.ctx());
    let info = match app.session.run("pivot.fields", json!({"pivot": name})) {
        Ok(v) => v,
        Err(_) => return,
    };
    let ctx = ui.ctx().clone();
    let dragging = DragAndDrop::payload::<PivotDrag>(&ctx);
    let pointer = ctx.pointer_latest_pos();
    let released = ctx.input(|i| i.pointer.any_released());
    // Drops run after the pane is laid out, against the state it was drawn from.
    let mut drops: Vec<(&'static str, Json)> = Vec::new();
    let mut over_area: Option<bool> = None; // Some(accepted) while over an area box
    let mut list_rect = Rect::NOTHING;
    let pane = egui::Panel::right("pivot_pane").default_size(280.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(
        ui,
        |ui| {
            ui.label(egui::RichText::new(lang.tr("PivotTable Fields")).font(theme::ui_bold(15.0)));
            ui.label(egui::RichText::new(lang.tr("Choose fields to add to report:")).small().color(t.text_dim));
            ui.add_space(4.0);
            let fields = info["fields"].as_array().cloned().unwrap_or_default();
            list_rect = egui::ScrollArea::vertical()
                .max_height(220.0)
                .id_salt("pv_fields")
                .show(ui, |ui| {
                    for f in &fields {
                        let fname = f["name"].as_str().unwrap_or("").to_string();
                        let areas: Vec<String> =
                            f["areas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                        let mut on = !areas.is_empty();
                        let numeric = f["numeric"].as_bool().unwrap_or(false);
                        let r = ui.checkbox(&mut on, egui::RichText::new(&fname).font(theme::ui_font(13.0)));
                        r.interact(Sense::drag()).dnd_set_drag_payload(PivotDrag { field: fname.clone(), from: None });
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
                })
                .inner_rect;
            ui.separator();
            ui.label(egui::RichText::new(lang.tr("Use a field's menu to move it between areas:")).small().color(t.text_dim));
            let area = |key: &str| -> Vec<String> {
                info["pivot"][key]
                    .as_array()
                    .or(info[key].as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x["name"].as_str().or(x["field"].as_str()).or(x["sourceCol"].as_str()).or(x.as_str()).map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            // Σ Values (two or more value fields) sits in Rows or Columns; it moves only between them.
            let values_key = if info["valuesOnRows"].as_bool().unwrap_or(false) { "rows" } else { "columns" };
            let values_at = info["valuesPosition"].as_u64().map(|p| p as usize);
            let area_with_values = |key: &str| -> Vec<String> {
                let mut v = area(key);
                if key == values_key && area("values").len() > 1 {
                    let at = values_at.unwrap_or(v.len()).min(v.len());
                    v.insert(at, VALUES_FIELD.to_string());
                }
                v
            };
            ui.columns(2, |cols| {
                for (i, (key, label)) in AREAS.iter().enumerate() {
                    let key: &'static str = key;
                    let ui = &mut cols[i % 2];
                    ui.label(egui::RichText::new(lang.tr(label)).strong());
                    let shown = area_with_values(key);
                    let mut rects: Vec<Rect> = Vec::with_capacity(shown.len());
                    let zone = egui::Frame::NONE.fill(t.window).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                        ui.set_min_size(vec2(ui.available_width(), 60.0));
                        for f in shown.clone() {
                            let is_values = f == VALUES_FIELD;
                            let b = ui.add(
                                egui::Button::new(egui::RichText::new(format!("{f} ▾")).font(theme::ui_font(12.0)))
                                    .fill(Color32::WHITE)
                                    .sense(Sense::click_and_drag())
                                    .min_size(vec2(ui.available_width(), 20.0)),
                            );
                            rects.push(b.rect);
                            b.dnd_set_drag_payload(PivotDrag { field: f.clone(), from: Some(key) });
                            egui::Popup::menu(&b).show(|ui| {
                                for (to, l) in [
                                    ("rows", "Move to Row Labels"),
                                    ("columns", "Move to Column Labels"),
                                    ("filters", "Move to Report Filter"),
                                    ("values", "Move to Values"),
                                ] {
                                    if to != key && (!is_values || matches!(to, "rows" | "columns")) && ui.button(lang.tr(l)).clicked() {
                                        app.run_or_alert("pivot.moveField", json!({"pivot": name, "field": f, "from": key, "to": to}));
                                    }
                                }
                                if is_values {
                                    return;
                                }
                                if key == "values" {
                                    ui.separator();
                                    for (func, l) in [
                                        ("sum", "Summarize by Sum"),
                                        ("count", "Summarize by Count"),
                                        ("average", "Summarize by Average"),
                                        ("max", "Summarize by Max"),
                                        ("min", "Summarize by Min"),
                                    ] {
                                        if ui.button(lang.tr(l)).clicked() {
                                            app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f, "func": func}));
                                        }
                                    }
                                    for (sa, l) in [
                                        ("normal", "Show as No Calculation"),
                                        ("percentOfGrandTotal", "Show as % of Grand Total"),
                                        ("runningTotal", "Show as Running Total"),
                                        ("rank", "Show as Rank"),
                                    ] {
                                        if ui.button(lang.tr(l)).clicked() {
                                            app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f, "showAs": sa}));
                                        }
                                    }
                                } else {
                                    ui.separator();
                                    for (o, l) in [("asc", "Sort A to Z"), ("desc", "Sort Z to A")] {
                                        if ui.button(lang.tr(l)).clicked() {
                                            app.run_or_alert("pivot.sort", json!({"pivot": name, "field": f, "order": o}));
                                        }
                                    }
                                    for (g, l) in [
                                        ("years", "Group by Years"),
                                        ("quarters", "Group by Quarters"),
                                        ("months", "Group by Months"),
                                        ("none", "Ungroup"),
                                    ] {
                                        if ui.button(lang.tr(l)).clicked() {
                                            app.run_or_alert("pivot.group", json!({"pivot": name, "field": f, "by": g}));
                                        }
                                    }
                                }
                                ui.separator();
                                if ui.button(lang.tr("Remove Field")).clicked() {
                                    app.run_or_alert("pivot.removeField", json!({"pivot": name, "field": f, "area": key}));
                                }
                            });
                        }
                    });
                    // A field held over this box: outline the box and mark where it would land.
                    let zr = zone.response.rect;
                    if let (Some(drag), Some(p)) = (dragging.as_deref(), pointer)
                        && zr.contains(p)
                    {
                        let ok = accepts(drag, key);
                        over_area = Some(ok);
                        if ok {
                            let painter = ui.painter();
                            painter.rect_stroke(zr, 4.0, Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                            let slot = slot_at(&rects, p.y);
                            let y = match (rects.get(slot), rects.last()) {
                                (Some(r), _) => r.top() - 1.0,
                                (None, Some(r)) => r.bottom() + 1.0,
                                (None, None) => zr.top() + 6.0,
                            };
                            painter.line_segment([Pos2::new(zr.left() + 4.0, y), Pos2::new(zr.right() - 4.0, y)], Stroke::new(2.0, t.accent));
                            if released {
                                DragAndDrop::clear_payload(&ctx);
                                drops = plan_drop(&name, drag, key, &shown, slot);
                            }
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
    let pane_rect = pane.response.rect;

    if let (Some(drag), Some(p)) = (dragging.as_deref(), pointer) {
        // Dragging a field out of the areas — onto the field list or off the pane — removes it.
        let removing = drag.from.is_some() && drag.field != VALUES_FIELD && (!pane_rect.contains(p) || list_rect.contains(p));
        ctx.set_cursor_icon(match over_area {
            Some(false) => CursorIcon::NotAllowed,
            _ if removing => CursorIcon::NotAllowed,
            _ => CursorIcon::Grabbing,
        });
        if released {
            if removing && drops.is_empty() {
                drops.push(("pivot.removeField", json!({"pivot": name, "field": drag.field, "area": drag.from})));
            }
            DragAndDrop::clear_payload(&ctx);
        } else {
            // The field travels with the pointer.
            egui::Area::new(egui::Id::new("pivot_drag_ghost")).order(egui::Order::Tooltip).fixed_pos(p + vec2(12.0, 8.0)).interactable(false).show(
                &ctx,
                |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        let text = if removing { format!("✕ {}", drag.field) } else { drag.field.clone() };
                        ui.label(egui::RichText::new(text).font(theme::ui_font(12.0)));
                    });
                },
            );
        }
    }
    for (id, params) in drops {
        if let Err(e) = app.run(id, params) {
            app.message = Some(("PivotTable".into(), crate::clean_error(&e)));
            break;
        }
    }
}
