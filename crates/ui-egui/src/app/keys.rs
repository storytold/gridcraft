use super::{Json, SheetApp, backstage, json, keytips};

impl SheetApp {
    pub(super) fn ribbon_keys(&mut self, ctx: &egui::Context) {
        let before = self.keytips.prefix().map(str::to_string);
        let enabled = self.editor.is_none()
            && self.dialog.is_none()
            && self.message.is_none()
            && self.name_box.is_none()
            && !ctx.text_edit_focused()
            && self.grid.context_menu.is_none()
            && self.grid.object_menu.is_none()
            && self.grid.header_menu.is_none()
            && self.grid.fill_menu.is_none()
            && self.grid.filter_menu.is_none()
            && self.grid.list_picker.is_none()
            && self.grid.renaming_tab.is_none()
            && (before.is_some() || !egui::Popup::is_any_open(ctx));
        let actions = self.keytips.process(ctx, enabled);
        // Backing out of F to the tab letters, or finishing an F sequence, closes the File view
        // that Alt, F opened: the command runs on the sheet, as it did without the view.
        if self.backstage.is_some() && before.is_some() && (self.keytips.prefix() == Some("") || !actions.is_empty()) {
            self.backstage = None;
        }
        if before.is_some() && before.as_deref() != self.keytips.prefix() && !ctx.input(|i| i.pointer.any_pressed()) {
            // Leave pointer cancellation to the popup so clicking a menu item still works.
            egui::Popup::close_all(ctx);
        }
        for action in actions {
            match action {
                keytips::Action::Tab("File") => self.open_backstage(backstage::Page::Home),
                keytips::Action::Tab(tab) => {
                    self.run_or_alert("ui.ribbonTab", json!({"tab": tab}));
                    self.ui.ribbon_collapsed = false;
                }
                keytips::Action::Command(id @ ("edit.copy" | "edit.cut")) => self.copy_to_clipboard(ctx, id),
                keytips::Action::Command("edit.paste") => self.paste_from_clipboard(),
                keytips::Action::Command(id) => self.run_or_alert(id, json!({})),
                keytips::Action::CommandWith(id, params) => {
                    let params: Json = serde_json::from_str(params).unwrap_or_else(|_| json!({}));
                    // A Borders preset picked by key tip becomes the split button's face, as a
                    // click on the same menu item does.
                    if id == "home.borders"
                        && let Some(preset) = params.get("preset").and_then(Json::as_str)
                    {
                        self.grid.last_border = preset.to_string();
                    }
                    self.run_or_alert(id, params);
                }
                keytips::Action::Dialog(name) => self.open_dialog(name, json!({})),
                keytips::Action::DialogWith(name, params) => self.open_dialog(name, serde_json::from_str(params).unwrap_or_else(|_| json!({}))),
                // Reserved sequence: fires silently while its command is missing. When the
                // command lands in the catalog it goes live with no table edit.
                keytips::Action::Pending(id) => {
                    if gridcraft_engine::find_command(id).is_some() {
                        self.run_or_alert(id, json!({}));
                    }
                }
                // Route to the ribbon control that owns this keytip: it opens its dropdown or
                // runs its click handler with the right params (the engine command alone would
                // be missing them).
                keytips::Action::Control(id) => self.ui.keytip_invoke = Some(id),
            }
        }
    }
}
