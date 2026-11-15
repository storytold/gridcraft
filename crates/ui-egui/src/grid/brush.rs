use egui::{Color32, ColorImage, Event, Pos2, Rect, TextureHandle, TouchDeviceId, TouchId, TouchPhase, pos2, vec2};
use gridcraft_engine::brush::{Error, LiveBrush, Sample, Style};
use gridcraft_engine::model::{Sheet, SheetUid};
use serde_json::json;

use super::Geo;
use crate::SheetApp;

type Touch = (TouchDeviceId, TouchId);
type Target = (u64, SheetUid, Option<u64>);

pub(crate) struct Gesture {
    target: Target,
    contact: Option<Touch>,
    style: Style,
    live: LiveBrush,
    preview: Option<Preview>,
    changed: bool,
    mirrored: bool,
}

struct Preview {
    origin: [f64; 2],
    size: [f32; 2],
    texture: TextureHandle,
}

pub(super) fn interact(app: &mut SheetApp, ui: &egui::Ui, response: &egui::Response, geo: &Geo, sheet: &Sheet) -> bool {
    if app.session.draw_tool != "brush" {
        app.grid.brush = None;
        return false;
    }
    let ctx = ui.ctx();
    let under_brush = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| hit(ui, response, geo, p));
    let handled = under_brush || app.grid.brush.is_some();
    if under_brush {
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let Some(doc) = app.session.active() else {
        app.grid.brush = None;
        return handled;
    };
    let target = (doc.uid, sheet.uid, app.session.active_window());
    let (focused, escape, time) = ctx.input(|i| (i.focused, i.key_pressed(egui::Key::Escape), i.time * 1000.0));
    if !focused || escape || app.dialog.is_some() || app.editor.is_some() {
        app.grid.brush = None;
        return handled;
    }
    if app.grid.brush.as_ref().is_some_and(|g| g.target != target) {
        app.grid.brush = None;
    }
    let frame = ctx.cumulative_frame_nr();
    if app.grid.brush_input_frame != Some(frame) {
        app.grid.brush_input_frame = Some(frame);
        let events = ctx.input(|i| i.events.clone());
        let use_touch = app.grid.brush.as_ref().map_or_else(|| events.iter().any(|e| matches!(e, Event::Touch { .. })), |g| g.contact.is_some());
        for event in events {
            let Some(action) = input_action(event, use_touch) else { continue };
            let pointer = StrokePointer { ui, response, geo, sheet, target, time };
            if let Err(err) = apply_action(app, &pointer, action) {
                app.grid.brush = None;
                app.message = Some(("PhotoCraft Brush".into(), err.to_string()));
                break;
            }
        }
        if let Some(gesture) = app.grid.brush.as_mut()
            && gesture.changed
            && let Err(err) = update_preview(ctx, gesture)
        {
            app.grid.brush = None;
            app.message = Some(("PhotoCraft Brush".into(), err.to_string()));
        }
    }
    if let Some(preview) = app.grid.brush.as_ref().and_then(|g| g.preview.as_ref()) {
        paint_preview(ui, geo, sheet, preview);
    }
    handled || app.grid.brush.is_some()
}

fn hit(ui: &egui::Ui, response: &egui::Response, geo: &Geo, pos: Pos2) -> bool {
    response.rect.contains(pos)
        && ui.clip_rect().contains(pos)
        && geo.in_cells(geo.mirror_pos(pos))
        && ui.ctx().layer_id_at(pos) == Some(response.layer_id)
}

enum Action {
    Start(Pos2, Option<Touch>, Option<f32>),
    Move(Pos2, Option<Touch>, Option<f32>),
    Finish(Pos2, Option<Touch>, Option<f32>),
    Cancel(Option<Touch>),
}

fn input_action(event: Event, use_touch: bool) -> Option<Action> {
    match event {
        Event::Touch { device_id, id, phase, pos, force } if use_touch => {
            let touch = Some((device_id, id));
            Some(match phase {
                TouchPhase::Start => Action::Start(pos, touch, force),
                TouchPhase::Move => Action::Move(pos, touch, force),
                TouchPhase::End => Action::Finish(pos, touch, force),
                TouchPhase::Cancel => Action::Cancel(touch),
            })
        }
        Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } if !use_touch => {
            Some(if pressed { Action::Start(pos, None, None) } else { Action::Finish(pos, None, None) })
        }
        Event::PointerMoved(pos) if !use_touch => Some(Action::Move(pos, None, None)),
        Event::PointerGone if !use_touch => Some(Action::Cancel(None)),
        _ => None,
    }
}

struct StrokePointer<'a> {
    ui: &'a egui::Ui,
    response: &'a egui::Response,
    geo: &'a Geo,
    sheet: &'a Sheet,
    target: Target,
    time: f64,
}

impl StrokePointer<'_> {
    fn sample(&self, pos: Pos2, pressure: f32) -> Option<Sample> {
        let p = self.geo.mirror_pos(pos);
        if !self.geo.in_cells(p) {
            return None;
        }
        let c = self.geo.cell_at(self.sheet, p);
        let x = self.sheet.col_left(c.col) + f64::from((p.x - self.geo.x(self.sheet, c.col)) / self.geo.z);
        let y = self.sheet.row_top(c.row) + f64::from((p.y - self.geo.y(self.sheet, c.row)) / self.geo.z);
        Some(Sample { x: x.max(0.0), y: y.max(0.0), pressure, time_ms: self.time.max(0.0), ..Sample::default() })
    }
}

fn apply_action(app: &mut SheetApp, p: &StrokePointer<'_>, action: Action) -> Result<(), Error> {
    match action {
        Action::Start(pos, contact, force) => {
            if app.grid.brush.is_some() || !hit(p.ui, p.response, p.geo, pos) || p.sheet.is_protected() {
                return Ok(());
            }
            let Some(first) = p.sample(pos, pressure(force, 1.0)) else { return Ok(()) };
            let style = app.session.draw_brush;
            let live = LiveBrush::new(style, first)?;
            app.selected_object = None;
            app.grid.brush = Some(Gesture { target: p.target, contact, style, live, preview: None, changed: true, mirrored: p.geo.rtl });
        }
        Action::Move(pos, contact, force) | Action::Finish(pos, contact, force) => {
            let Some(gesture) = app.grid.brush.as_mut().filter(|g| g.contact == contact) else { return Ok(()) };
            let finish = matches!(action, Action::Finish(..));
            let previous = gesture.live.samples().last().copied().ok_or(Error::Empty)?;
            if let Some(sample) = p.sample(pos, pressure(force, previous.pressure))
                && ((sample.x - previous.x).hypot(sample.y - previous.y) > 0.05 || (!finish && (sample.pressure - previous.pressure).abs() > 0.005))
            {
                match gesture.live.push(sample) {
                    Ok(()) => {}
                    Err(Error::Limit) => {
                        finish_gesture(app, true);
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                }
                gesture.changed = true;
            }
            if finish {
                finish_gesture(app, false);
            }
        }
        Action::Cancel(contact) => {
            if app.grid.brush.as_ref().is_some_and(|g| g.contact == contact) {
                app.grid.brush = None;
            }
        }
    }
    Ok(())
}

fn finish_gesture(app: &mut SheetApp, reached_limit: bool) {
    let Some(gesture) = app.grid.brush.take() else { return };
    match app.run("draw.brushStroke", json!({"samples": gesture.live.into_samples(), "style": gesture.style})) {
        Ok(_) if reached_limit => app.toast = Some(("Saved the stroke up to the drawing limit".into(), crate::now_ms())),
        Ok(_) => {}
        Err(error) => app.message = Some(("PhotoCraft Brush".into(), error)),
    }
}

fn pressure(force: Option<f32>, fallback: f32) -> f32 {
    force.filter(|v| v.is_finite()).map_or(fallback, |v| v.clamp(0.0, 1.0))
}

fn update_preview(ctx: &egui::Context, gesture: &mut Gesture) -> Result<(), Error> {
    let first = gesture.live.samples().first().copied().ok_or(Error::Empty)?;
    let mut raster = gesture.live.preview()?;
    let units = 1.0 / raster.pixels_per_unit;
    raster.crop_origin([first.x, first.y], [units; 2])?;
    if gesture.mirrored {
        raster.mirror_x();
    }
    let image = ColorImage::from_rgba_unmultiplied(raster.size.map(|v| v as usize), &raster.rgba);
    let texture = match gesture.preview.take() {
        Some(mut preview) => {
            preview.texture.set(image, egui::TextureOptions::LINEAR);
            preview.texture
        }
        None => ctx.load_texture("gridcraft.brush.preview", image, egui::TextureOptions::LINEAR),
    };
    let origin = [first.x + f64::from(raster.offset[0]) * units, first.y + f64::from(raster.offset[1]) * units];
    gesture.preview = Some(Preview { origin, size: raster.size.map(|v| (f64::from(v) * units) as f32), texture });
    gesture.changed = false;
    Ok(())
}

fn paint_preview(ui: &egui::Ui, geo: &Geo, sheet: &Sheet, preview: &Preview) {
    let [x, y] = preview.origin;
    let (col, row) = (sheet.col_at(x), sheet.row_at(y));
    let top_left = pos2(geo.x(sheet, col) + (x - sheet.col_left(col)) as f32 * geo.z, geo.y(sheet, row) + (y - sheet.row_top(row)) as f32 * geo.z);
    let rect = geo.mirror_rect(Rect::from_min_size(top_left, vec2(preview.size[0] * geo.z, preview.size[1] * geo.z)));
    ui.painter().with_clip_rect(geo.mirror_rect(geo.cells)).image(
        preview.texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}
