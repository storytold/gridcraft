use super::{AgentKey, Approval, Mode, PanelState, ToolRounds, provider};
use crate::{SheetApp, i18n::Language, theme::Tokens};
use egui::{Align, Align2, Button, Context, Layout, Rect, RichText, ScrollArea, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::json;

const ICON: &[u8] = include_bytes!("../../../../assets/wingman/artcraft-ai-256.png");

#[derive(Default)]
pub(super) struct RibbonGuide {
    section: String,
    query: String,
    offset: usize,
    page: Option<serde_json::Value>,
}

impl RibbonGuide {
    fn show(&mut self, ui: &mut Ui, l: Language) {
        ui.collapsing(l.tr("Ribbon guide"), |ui| {
            let Ok(sections) = super::workbook::menu::sections() else {
                ui.label(l.tr("The ribbon guide is unavailable."));
                return;
            };
            let mut changed = false;
            egui::ComboBox::from_id_salt("wingman_guide_section")
                .selected_text(if self.section.is_empty() { l.tr("All sections") } else { l.tr(&self.section) })
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut self.section, String::new(), l.tr("All sections")).changed();
                    for &name in sections {
                        changed |= ui.selectable_value(&mut self.section, name.to_owned(), l.tr(name)).changed();
                    }
                });
            changed |= ui.add(egui::TextEdit::singleline(&mut self.query).char_limit(120).hint_text(l.tr("Find a control or group…"))).changed();
            if changed {
                self.offset = 0;
                self.page = None;
            }
            if self.page.is_none() {
                let section = (!self.section.is_empty()).then_some(&self.section);
                self.page = super::workbook::menu::find(&json!({"search":self.query,"section":section,"offset":self.offset})).ok();
            }
            let Some(page) = &self.page else {
                return;
            };
            ScrollArea::vertical().id_salt("wingman_guide_results").max_height(160.0).show(ui, |ui| {
                for entry in page["items"].as_array().into_iter().flatten() {
                    let get = |k: &str| entry[k].as_str().unwrap_or("");
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(l.tr(get("label")));
                        ui.weak(format!("{} › {}", l.tr(get("section")), l.tr(get("group"))));
                    });
                    let tool = get("wingmanTool");
                    if tool.is_empty() {
                        ui.small(l.tr("Manual control"));
                    } else {
                        ui.small(format!("{} · {tool}", l.tr(get("access"))));
                    }
                }
                if page["items"].as_array().is_none_or(Vec::is_empty) {
                    ui.label(l.tr("No matching controls."));
                }
            });
            let next = page["nextOffset"].as_u64().map(|n| n as usize);
            ui.horizontal(|ui| {
                if ui.add_enabled(self.offset > 0, Button::new(l.tr("Previous"))).clicked() {
                    self.offset = self.offset.saturating_sub(16);
                    self.page = None;
                }
                if ui.add_enabled(next.is_some(), Button::new(l.tr("Next"))).clicked() {
                    self.offset = next.unwrap_or(self.offset);
                    self.page = None;
                }
            });
        });
    }
}

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
    if !app.ui.show_wingman_pane || band.width() < 128.0 {
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
    if !app.ui.show_wingman_pane || !app.wingman.open {
        return;
    }
    // Workbook actions need the ordinary &mut SheetApp dispatch path. This panel is restored
    // before grid rendering and parks with the other per-window UI in multi-window mode.
    let mut state = std::mem::take(&mut app.wingman);
    let ctx = ui.ctx().clone();
    state.tick(app, &ctx);
    let t = Tokens::get(&ctx);
    let max = (ui.available_width() * 0.65).max(160.0);
    let panel = egui::Panel::right("wingman")
        .min_size(195.0f32.min(max))
        .default_size(330.0f32.min(max))
        .max_size(max)
        .resizable(true)
        .frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(8))
        .show(ui, |ui| {
            super::history::shortcuts(app, ui, &state.composer);
            state.draw(app, ui);
        });
    state.dock_rect = Some(panel.response.rect);
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
        self.settings(ui, l, !app.synthetic_pending);
        self.guide.show(ui, l);
        if let Some(t) = self.turn.as_ref().filter(|t| t.agent.supports_tools && self.mode != Mode::Chat) {
            let (tab, group) = t.ribbon.location();
            let path = group.map_or_else(|| l.tr(tab).to_owned(), |g| format!("{} › {}", l.tr(tab), l.tr(g)));
            ui.weak(format!("{}: {path}", l.tr("Agent ribbon")));
        }
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
                    if matches!(line.speaker, "Workbook read" | "Applied edit" | "Tool refused" | "External tool") {
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
                        message(ui, l, &line.text, true);
                    }
                    ui.separator();
                });
            }
            if let Some(t) = &self.turn
                && !t.partial.is_empty()
            {
                message(ui, l, &t.partial, false);
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

    fn settings(&mut self, ui: &mut Ui, l: Language, physical: bool) {
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
                if let Some(a) = self.agents.get_mut(self.selected) {
                    ui.weak(format!("{} · {}", a.model, a.endpoint));
                    if !a.is_loopback {
                        ui.label(l.tr("Remote provider: messages and attached workbook data are sent to this endpoint."));
                    }
                    let mut mode = self.mode;
                    let supports_tools = a.supports_tools;
                    let mut access = |ui: &mut Ui| {
                        let label = ui.label(l.tr("Access"));
                        if supports_tools {
                            let selected = l.tr(mode.label());
                            egui::ComboBox::from_id_salt("wingman_access")
                                .width(ui.available_width().min(170.0))
                                .truncate()
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for m in [Mode::Chat, Mode::Read, Mode::Edit, Mode::Mcp] {
                                        ui.selectable_value(&mut mode, m, l.tr(m.label()));
                                    }
                                })
                                .response
                                .labelled_by(label.id);
                        } else {
                            ui.weak(l.tr("Chat only")).on_hover_text(l.tr("Text only · explicitly attached context is available"));
                        }
                    };
                    if ui.available_width() >= 310.0 {
                        ui.columns(2, |col| {
                            access(&mut col[0]);
                            a.thinking.picker(&mut col[1], l);
                        });
                    } else {
                        access(ui);
                        a.thinking.picker(ui, l);
                    }
                    if supports_tools {
                        self.change_mode(mode);
                        self.turn_controls(ui, l, physical);
                        if self.mode == Mode::Mcp {
                            self.mcp.ui(ui, l, physical);
                        }
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

    fn turn_controls(&mut self, ui: &mut Ui, l: Language, physical: bool) {
        let rounds = |ui: &mut Ui, budget: &mut ToolRounds| {
            let label = ui.label(l.tr("Tool rounds"));
            let caption = |n: Option<usize>| n.map_or_else(|| l.tr("Unlimited").to_owned(), |n| n.to_string());
            egui::ComboBox::from_id_salt("wingman_rounds")
                .width(ui.available_width().min(170.0))
                .truncate()
                .selected_text(caption(budget.0))
                .show_ui(ui, |ui| {
                    for n in [Some(4), Some(8), Some(12), Some(24), Some(48), Some(96), None] {
                        ui.selectable_value(budget, ToolRounds(n), caption(n));
                    }
                })
                .response
                .labelled_by(label.id)
                .on_hover_text(l.tr("Finite limits reserve the last round for an answer. Unlimited runs until the model finishes or you stop it."));
        };
        let may_change_approval = physical && self.mode.writes();
        let approvals = |ui: &mut Ui, approval: &mut Approval| {
            let label = ui.label(l.tr("Approval"));
            let mut choice = *approval;
            ui.add_enabled_ui(may_change_approval, |ui| {
                egui::ComboBox::from_id_salt("wingman_approval")
                    .width(ui.available_width().min(170.0))
                    .truncate()
                    .selected_text(l.tr(choice.label()))
                    .show_ui(ui, |ui| {
                        for value in [Approval::Ask, Approval::AlwaysAllow] {
                            ui.selectable_value(&mut choice, value, l.tr(value.label()));
                        }
                    })
                    .response
                    .labelled_by(label.id)
                    .on_hover_text(l.tr("Always allow skips confirmations for edits and connected MCP tools in this thread and access mode."))
                    .on_disabled_hover_text(l.tr("Approval applies to writes and connected external tools."));
            });
            if may_change_approval {
                *approval = choice;
            }
        };
        if ui.available_width() >= 275.0 {
            ui.columns(2, |cols| {
                rounds(&mut cols[0], &mut self.tool_rounds);
                approvals(&mut cols[1], &mut self.approval);
            });
        } else {
            rounds(ui, &mut self.tool_rounds);
            approvals(ui, &mut self.approval);
        }
        if self.approval == Approval::AlwaysAllow && self.mode == Mode::Mcp {
            ui.weak(l.tr("Connected tools can act without another prompt. GridCraft cannot undo external actions."));
        }
    }

    fn input(&mut self, app: &mut SheetApp, ui: &mut Ui, l: Language) {
        let ctx = ui.ctx().clone();
        ui.separator();
        if self.turn.is_some() {
            let waiting = self
                .turn
                .as_ref()
                .filter(|t| t.external.is_none() && t.approval == Approval::Ask)
                .and_then(|t| t.queue.front().filter(|c| self.requires_consent(t, c)).map(|c| (c, t.offered.get(&c.name))));
            if let Some((call, external)) = waiting {
                status(ui, l.tr(if external.is_some() { "Review external tool call" } else { "Review workbook edit" }));
                if let Some(e) = external {
                    ui.strong(e.label());
                    ui.weak(l.tr("These arguments go to this server. External actions cannot be undone by GridCraft."));
                }
                if let Some(d) = app.session.active()
                    && let Some(s) = d.wb.active()
                {
                    ui.weak(format!("{}: {} · {}!{}", l.tr("Current selection"), d.display_title(), s.name, d.selection.current().a1()));
                }
                ui.monospace(&call.name);
                if call.name == "comments_to_column" {
                    ui.weak(l.tr(
                        "Copy keeps the original notes and threads. Move removes them after writing their text. Undo restores the entire change.",
                    ));
                }
                ScrollArea::vertical().id_salt("wingman_args").max_height((ctx.content_rect().height() * 0.22).max(48.0)).show(ui, |ui| {
                    if call.name == "apply_edits" && external.is_none() {
                        ui.label(l.tr("One approval and one undo step. If a step fails, the plan is rolled back."));
                        ui.monospace(call.plan_outline());
                        ui.collapsing(l.tr("Exact arguments"), |ui| ui.monospace(call.preview()));
                    } else {
                        ui.monospace(call.preview());
                    }
                });
                let label = if external.is_some() { "Allow call" } else { "Apply edit" };
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!app.synthetic_pending, Button::new(l.tr(label))).clicked() {
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
                    ui.label(l.tr(if self.turn.as_ref().is_some_and(|t| t.search.is_some()) { "Searching workbook…" } else { "Working…" }));
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
fn message(ui: &mut Ui, l: Language, text: &str, finished: bool) {
    message_parts(text, |language, value, closed| {
        let visible = value.strip_suffix("\r\n").or_else(|| value.strip_suffix('\n')).unwrap_or(value);
        let mut label = RichText::new(visible);
        if let Some(language) = language {
            ui.horizontal_wrapped(|ui| {
                if !language.is_empty() {
                    ui.weak(language);
                }
                if (closed || finished) && ui.small_button(l.tr("Copy code")).clicked() {
                    ui.ctx().copy_text(value.to_owned());
                }
            });
            label = label.monospace();
        }
        ui.add(egui::Label::new(label).wrap().selectable(true));
    });
}

/// Borrow the original code, including indentation and line endings, for the clipboard.
/// Only a matching fence with no trailing text can close a block.
fn message_parts<'a>(text: &'a str, mut emit: impl FnMut(Option<&'a str>, &'a str, bool)) {
    let mut fence: Option<(u8, usize, &str)> = None;
    let (mut start, mut offset) = (0, 0);
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let rest = bare.trim_start_matches(' ');
        let next = offset + line.len();
        if bare.len() - rest.len() < 4
            && let Some(&mark @ (b'`' | b'~')) = rest.as_bytes().first()
        {
            let count = rest.bytes().take_while(|&b| b == mark).count();
            let info = rest[count..].trim();
            match fence {
                Some((open, n, lang)) if open == mark && count >= n && info.is_empty() => {
                    emit(Some(lang), &text[start..offset], true);
                    fence = None;
                    start = next;
                }
                None if count >= 3 && (mark != b'`' || !info.contains('`')) => {
                    if offset > start {
                        emit(None, &text[start..offset], true);
                    }
                    fence = Some((mark, count, info.split_whitespace().next().unwrap_or("")));
                    start = next;
                }
                _ => {}
            }
        }
        offset = next;
    }
    if start < text.len() {
        emit(fence.map(|(_, _, lang)| lang), &text[start..], fence.is_none());
    }
}

fn status(ui: &mut Ui, text: &str) {
    let id = ui.label(text).id;
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::Role::Status);
        node.set_live(egui::accesskit::Live::Polite);
    });
}

#[cfg(test)]
mod message_tests {
    #[test]
    fn copyable_blocks_keep_source_and_exclude_surrounding_prose() {
        let mut parts = Vec::new();
        super::message_parts("First\n````vba\r\nSub Update()\r\n  ' ```\r\nEnd Sub\r\n````\r\nNext\n~~~\n  x\n~~~\nLast", |lang, s, closed| {
            parts.push((lang, s, closed));
        });
        assert_eq!(
            parts,
            vec![
                (None, "First\n", true),
                (Some("vba"), "Sub Update()\r\n  ' ```\r\nEnd Sub\r\n", true),
                (None, "Next\n", true),
                (Some(""), "  x\n", true),
                (None, "Last", true)
            ]
        );
        parts.clear();
        super::message_parts("```vb\nSub Pending()", |lang, s, closed| parts.push((lang, s, closed)));
        assert_eq!(parts, vec![(Some("vb"), "Sub Pending()", false)]);
    }
}
