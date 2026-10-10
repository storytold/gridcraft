//! In-place editing of a text box. The draft becomes one undoable engine command.

use egui::{Color32, FontId, Key, Modifiers, Rect, TextEdit};
use gridcraft_engine::{Mode, model::ShapeKind};
use serde_json::json;

use crate::l10n::{Tr, msg};
use crate::{SheetApp, grid::Geo, theme};

#[derive(Clone, Debug)]
pub struct EditState {
    pub id: u32,
    pub text: String,
    uid: u64,
    sheet: usize,
    original: String,
    request_focus: bool,
    rect: Option<Rect>,
}

pub(crate) fn editor_id() -> egui::Id {
    egui::Id::new("gridcraft.text_box_editor")
}

impl SheetApp {
    pub fn begin_text_box_edit(&mut self, id: u32) -> bool {
        if !self.commit_edit(0, 0, false, false) {
            return false;
        }
        if let Err(e) = self.commit_text_box_edit() {
            self.message = Some((self.l10n.tr("Text Box").into_owned(), self.error_text(&e)));
            return false;
        }
        let Some(d) = self.session.active() else { return false };
        let Some(sh) = d.wb.active() else { return false };
        let Some(shape) = sh.shapes.iter().find(|s| s.id == id && s.kind == ShapeKind::TextBox) else {
            return false;
        };
        // shape.setText refuses edits on a protected sheet; don't open an editor that can't commit.
        if sh.is_protected() {
            self.message = Some((self.l10n.tr("Text Box").into_owned(), self.error_text(&gridcraft_engine::EngineError::Protected)));
            return false;
        }
        self.text_box_editor = Some(EditState {
            id,
            text: shape.text.clone(),
            uid: d.uid,
            sheet: d.wb.active_sheet,
            original: shape.text.clone(),
            request_focus: true,
            rect: None,
        });
        self.selected_chart = Some(id);
        self.grid.drag = crate::grid::Drag::None;
        self.grid.drag_select = false;
        self.session.mode = Mode::Edit;
        true
    }

    pub fn commit_text_box_edit(&mut self) -> Result<(), gridcraft_engine::EngineError> {
        let Some(ed) = self.text_box_editor.take() else { return Ok(()) };
        let result = if self.view_key() != Some((ed.uid, ed.sheet)) {
            Err(gridcraft_engine::EngineError::Other(msg!("The text box's worksheet is no longer active.").into()))
        } else if ed.text == ed.original {
            Ok(())
        } else {
            // Call the engine directly: app.run flushes this draft before saving/switching.
            self.session.execute("shape.setText", json!({"id": ed.id, "text": ed.text})).map(|_| ())
        };
        match result {
            Ok(()) => {
                self.session.mode = Mode::Ready;
                self.after_engine();
                Ok(())
            }
            Err(e) => {
                self.text_box_editor = Some(ed);
                Err(e)
            }
        }
    }

    pub fn cancel_text_box_edit(&mut self) {
        self.text_box_editor = None;
        self.session.mode = Mode::Ready;
    }

    pub(crate) fn editing_text_box(&self, id: u32) -> bool {
        self.text_box_editor.as_ref().is_some_and(|ed| ed.id == id && self.view_key() == Some((ed.uid, ed.sheet)))
    }
}

/// Resolve outside clicks before the ribbon/tabs can save or switch the workbook.
pub(crate) fn before_ui(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(ed) = &app.text_box_editor else { return };
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        app.cancel_text_box_edit();
        ctx.memory_mut(|m| m.surrender_focus(editor_id()));
        return;
    }
    let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| ed.rect.is_none_or(|r| !r.contains(p))));
    if outside {
        match app.commit_text_box_edit() {
            Ok(()) => ctx.memory_mut(|m| m.surrender_focus(editor_id())),
            Err(e) => app.message = Some((app.l10n.tr("Text Box").into_owned(), app.error_text(&e))),
        }
    }
}

pub(crate) fn contains_pointer(app: &SheetApp, ctx: &egui::Context) -> bool {
    app.text_box_editor.as_ref().and_then(|ed| ed.rect).is_some_and(|r| ctx.input(|i| i.pointer.hover_pos().is_some_and(|p| r.contains(p))))
}

pub(crate) fn show(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &gridcraft_engine::model::Sheet) {
    let Some(mut ed) = app.text_box_editor.take() else { return };
    if app.view_key() != Some((ed.uid, ed.sheet)) {
        app.text_box_editor = Some(ed);
        return;
    }
    let Some(shape) = sh.shapes.iter().find(|s| s.id == ed.id && s.kind == ShapeKind::TextBox) else {
        app.session.mode = Mode::Ready;
        return;
    };
    let save = ui.input_mut(|i| {
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            Some("file.saveAs")
        } else if i.consume_key(Modifiers::COMMAND, Key::S) {
            Some("file.save")
        } else {
            None
        }
    });
    let rect = crate::chartview::anchor_rect(geo, sh, &shape.anchor).shrink(4.0).intersect(geo.cells);
    if !rect.is_positive() {
        ed.rect = None;
        app.text_box_editor = Some(ed);
        if let Some(command) = save {
            app.run_or_alert(command, json!({}));
        }
        return;
    }
    ed.rect = Some(rect);
    let id = editor_id();
    if ed.request_focus {
        // A new editing session must not reuse the previous box's caret/undo history.
        egui::text_edit::TextEditState::default().store(ui.ctx(), id);
    }
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect));
    child.set_clip_rect(rect);
    egui::ScrollArea::vertical().id_salt(id).max_height(rect.height()).auto_shrink([false, false]).show(&mut child, |ui| {
        let response = ui.add(
            TextEdit::multiline(&mut ed.text)
                .id(id)
                .font(FontId::new(14.0 * geo.z, egui::FontFamily::Name(theme::CELL.into())))
                .text_color(Color32::BLACK)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .desired_width(rect.width())
                .desired_rows(1)
                // shape.setText's limit (a cell's).
                .char_limit(32_767)
                .min_size(rect.size())
                .lock_focus(true),
        );
        if ed.request_focus {
            response.request_focus();
            ed.request_focus = false;
        }
    });
    app.text_box_editor = Some(ed);
    if let Some(command) = save {
        app.run_or_alert(command, json!({}));
    }
}
