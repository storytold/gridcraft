use super::{AgentKey, Mode, PanelState, provider, workbook};
use crate::{SheetApp, i18n::Language, theme::Tokens};
use egui::{Align, Align2, Button, Context, Layout, Rect, RichText, ScrollArea, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

const ICON: &[u8] = include_bytes!("../../../../assets/wingman/artcraft-ai-256.png");

fn icon(ctx: &Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("gc_wingman_icon");
    if let Some(texture) = ctx.data(|d| d.get_temp(id)) {
        return Some(texture);
    }
    let pixels = image::load_from_memory_with_format(ICON, image::ImageFormat::Png).ok()?.into_rgba8();
    let image = egui::ColorImage::from_rgba_unmultiplied([pixels.width() as usize, pixels.height() as usize], &pixels);
    let texture = ctx.load_texture("gridcraft-wingman", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// The assistant is a peer of the command card rather than its last scrollable group.
/// Return the remaining rectangle so the ribbon's overflow arrows never cover the launcher.
pub(crate) fn ribbon_tile(app: &mut SheetApp, ui: &mut Ui, band: Rect, compact: bool) -> Rect {
    if band.width() < 128.0 {
        return band;
    }
    let l = app.ui.language;
    let label = l.tr("Wingman");
    let t = Tokens::get(ui.ctx());
    let width = if compact { 88.0 } else { 74.0 };
    let tile = Rect::from_min_max(pos2(band.right() - width, band.top() + 1.0), pos2(band.right(), band.bottom() - 1.0));
    let response = ui.interact(tile, ui.make_persistent_id("wingman_tile"), egui::Sense::click());
    app.wingman.launcher_id = Some(response.id);
    let open = app.wingman.open;
    ui.painter().rect_filled(
        tile,
        if compact { 6.0 } else { 10.0 },
        if response.hovered() {
            t.hover
        } else if open {
            t.accent_soft
        } else {
            t.ribbon
        },
    );
    ui.painter().rect_stroke(
        tile,
        if compact { 6.0 } else { 10.0 },
        Stroke::new(if response.has_focus() { 2.0 } else { 1.0 }, if response.has_focus() || open { t.accent } else { t.ribbon_border }),
        StrokeKind::Inside,
    );
    let (center, side, caption, anchor) = if compact {
        (pos2(tile.left() + 14.0, tile.center().y), 20.0, pos2(tile.left() + 28.0, tile.center().y), Align2::LEFT_CENTER)
    } else {
        (tile.center() - vec2(0.0, 10.0), 32.0, tile.center() + vec2(0.0, 19.0), Align2::CENTER_CENTER)
    };
    if let Some(texture) = icon(ui.ctx()) {
        ui.painter().image(
            texture.id(),
            Rect::from_center_size(center, vec2(side, side)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    ui.painter().text(caption, anchor, label, crate::theme::ui_font(13.0), t.text);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, app.wingman.open, label));
    if response.on_hover_text(l.tr("Open or close the workbook assistant")).clicked() {
        app.run_or_alert("view.wingman", json!({}));
    }
    Rect::from_min_max(band.min, pos2(tile.left() - 6.0, band.bottom()))
}

pub(crate) fn show(app: &mut SheetApp, ui: &mut Ui) {
    if !app.wingman.open {
        return;
    }
    // Workbook actions need the ordinary &mut SheetApp dispatch path. This panel is restored
    // before grid rendering and parks with the other per-window UI in multi-window mode.
    let mut state = std::mem::take(&mut app.wingman);
    let ctx = ui.ctx().clone();
    state.tick(app, &ctx);
    let t = Tokens::get(&ctx);
    let max = (ui.available_width() * 0.65).max(160.0);
    egui::Panel::right("wingman")
        .min_size(195.0f32.min(max))
        .default_size(330.0f32.min(max))
        .max_size(max)
        .resizable(true)
        .frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(8))
        .show(ui, |ui| state.draw(app, ui));
    app.wingman = state;
}

impl PanelState {
    fn header(&mut self, ui: &mut Ui, l: Language) {
        let actions = |ui: &mut Ui| {
            if ui.button(l.tr("Close")).clicked() {
                let _ = self.toggle(&json!({"on":false}));
                if let Some(id) = self.launcher_id {
                    ui.memory_mut(|m| m.request_focus(id));
                }
            }
            if ui.button(l.tr("New")).on_hover_text(l.tr("Keep this thread and start another")).clicked() {
                self.fresh();
            }
            self.thread_menu(ui, l);
        };
        if ui.available_width() < 245.0 {
            ui.heading(l.tr("Wingman"));
            ui.horizontal_wrapped(actions);
        } else {
            ui.horizontal(|ui| {
                ui.heading(l.tr("Wingman"));
                ui.with_layout(Layout::right_to_left(Align::Center), actions);
            });
        }
    }

    fn draw(&mut self, app: &mut SheetApp, ui: &mut Ui) {
        let l = app.ui.language;
        self.header(ui, l);
        if !self.open {
            return;
        }
        ui.separator();
        self.settings(ui, l);
        if !self.note.is_empty() {
            status(ui, &self.note);
        }
        if self.agents.is_empty() {
            ui.separator();
            ui.label(l.tr("Choose a provider and model in wingman.json, then reload. For discovery-only servers, choose Find models."));
            return;
        }
        ui.separator();
        let h = ui.available_height();
        egui::Panel::bottom("wingman_input").frame(egui::Frame::NONE).max_size((h * 0.65).max(120.0)).show(ui, |ui| {
            self.input(app, ui, l);
        });
        ScrollArea::vertical().id_salt("wingman_transcript").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            if self.thread.log.is_empty() {
                ui.label(l.tr("What would you like to do with this workbook?"));
                ui.weak(l.tr("Type @ to attach cells or / for panel commands."));
            }
            for (i, line) in self.thread.log.iter().enumerate() {
                ui.push_id(i, |ui| {
                    if matches!(line.speaker, "Workbook read" | "Applied edit" | "Tool refused") {
                        let tool = line.text.lines().next().unwrap_or_default();
                        ui.collapsing(format!("{} · {}", l.tr(line.speaker), tool), |ui| {
                            ui.monospace(&line.text);
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.strong(l.tr(line.speaker));
                            if ui.small_button(l.tr("Copy")).clicked() {
                                ui.ctx().copy_text(line.text.clone());
                            }
                            if line.speaker == "You"
                                && self.turn.is_none()
                                && self.composer.text.is_empty()
                                && ui.small_button(l.tr("Use as draft")).clicked()
                            {
                                self.composer.text.clone_from(&line.text);
                                self.composer.request_focus();
                            }
                        });
                        message(ui, &line.text);
                    }
                    ui.separator();
                });
            }
            if let Some(t) = &self.turn
                && !t.partial.is_empty()
            {
                message(ui, &t.partial);
            }
        });
    }

    fn thread_menu(&mut self, ui: &mut Ui, l: Language) {
        let mut restore = None;
        let current = self.agents.get(self.selected).map(AgentKey::from);
        ui.menu_button(l.tr("Threads"), |ui| {
            let mut listed = false;
            for (i, thread) in self.archives.iter().enumerate().filter(|(_, t)| t.owner == current) {
                listed = true;
                if ui.button(&thread.title).clicked() {
                    restore = Some(i);
                    ui.close();
                }
            }
            if !listed {
                ui.weak(l.tr("No earlier threads for this agent."));
            }
        });
        if let Some(thread) = restore.and_then(|i| self.archives.remove(i)) {
            self.fresh();
            self.thread = thread;
        }
    }

    fn settings(&mut self, ui: &mut Ui, l: Language) {
        let busy = self.turn.is_some() || self.discovery.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            if !self.agents.is_empty() {
                let mut index = self.selected;
                egui::ComboBox::from_label(l.tr("Agent")).selected_text(&self.agents[index].name).show_ui(ui, |ui| {
                    for (i, a) in self.agents.iter().enumerate() {
                        ui.selectable_value(&mut index, i, &a.name).on_hover_text(format!("{} · {}", a.provider, a.model));
                    }
                });
                if index != self.selected {
                    self.fresh();
                    self.selected = index;
                    self.mode = Mode::initial(self.agents.get(index));
                }
                if let Some(a) = self.agents.get(self.selected) {
                    ui.weak(format!("{} · {}", a.model, a.endpoint));
                    if a.supports_tools {
                        egui::ComboBox::from_label(l.tr("Access")).selected_text(l.tr(self.mode.label())).show_ui(ui, |ui| {
                            for m in [Mode::Chat, Mode::Read, Mode::Edit] {
                                ui.selectable_value(&mut self.mode, m, l.tr(m.label()));
                            }
                        });
                    } else {
                        ui.weak(l.tr("Text only · explicitly attached context is available"));
                    }
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ui.button(l.tr("Open JSON")).clicked() {
                    self.note = match provider::open_config() {
                        Ok(()) => l.tr("Save the JSON, then choose Reload.").into(),
                        Err(e) => e,
                    };
                }
                if ui.button(l.tr("Reload")).clicked() {
                    self.reload();
                }
                if ui.button(l.tr("Find models")).clicked() {
                    match provider::discover(ui.ctx().clone()) {
                        Ok(d) => {
                            self.discovery = Some(d);
                            self.note.clear();
                        }
                        Err(e) => self.note = e,
                    }
                }
            });
        });
        if self.discovery.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(l.tr("Finding models…"));
                if ui.button(l.tr("Cancel")).clicked() {
                    self.discovery = None;
                }
            });
        }
    }

    fn input(&mut self, app: &mut SheetApp, ui: &mut Ui, l: Language) {
        let ctx = ui.ctx().clone();
        ui.separator();
        if self.turn.is_some() {
            let waiting =
                self.turn.as_ref().and_then(|t| t.queue.front()).filter(|c| self.mode == Mode::Edit && workbook::is_write(&c.name) == Ok(true));
            if let Some(call) = waiting {
                status(ui, l.tr("Review workbook edit"));
                if let Some(d) = app.session.active()
                    && let Some(s) = d.wb.active()
                {
                    ui.weak(format!("{}: {} · {}!{}", l.tr("Current selection"), d.display_title(), s.name, d.selection.current().a1()));
                }
                ui.monospace(&call.name);
                ScrollArea::vertical().id_salt("wingman_args").max_height((ctx.content_rect().height() * 0.22).max(48.0)).show(ui, |ui| {
                    ui.monospace(call.preview());
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!app.synthetic_pending, Button::new(l.tr("Apply edit"))).clicked() {
                        self.decision(app, true, &ctx);
                    }
                    if ui.button(l.tr("Deny")).clicked() {
                        self.decision(app, false, &ctx);
                    }
                    if ui.button(l.tr("Stop")).clicked() {
                        self.stop();
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(l.tr("Working…"));
                    if ui.button(l.tr("Stop")).clicked() {
                        self.stop();
                    }
                });
            }
            return;
        }
        let enabled = self.discovery.is_none();
        let mut submitted = self.composer.show(ui, enabled, l, app);
        ui.horizontal(|ui| {
            if ui.add_enabled(enabled && !self.composer.text.trim().is_empty(), Button::new(l.tr("Send"))).clicked() {
                submitted = self.composer.take_draft();
            }
            ui.weak(l.tr("Shift+Enter for a new line"));
        });
        if let Some(text) = submitted
            && let Err(e) = self.submit(app, text.clone(), &ctx)
        {
            self.composer.text = text;
            self.note = e;
            self.composer.request_focus();
        }
    }
}

// Render code as copyable, selectable text; response text never activates links or commands.
fn message(ui: &mut Ui, text: &str) {
    let mut code = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            block(ui, &mut lines, code);
            code = !code;
        } else {
            lines.push(line);
        }
    }
    block(ui, &mut lines, code);
}

fn status(ui: &mut Ui, text: &str) {
    let id = ui.label(text).id;
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::Role::Status);
        node.set_live(egui::accesskit::Live::Polite);
    });
}

fn block(ui: &mut Ui, lines: &mut Vec<&str>, code: bool) {
    if lines.is_empty() {
        return;
    }
    let value = RichText::new(lines.join("\n"));
    ui.add(egui::Label::new(if code { value.monospace() } else { value }).wrap().selectable(true));
    lines.clear();
}
