use super::controls::{act, anchor_keytip, control_keytip, invoke, keytip, menu_items, popup_command, sep};
use super::{Icon, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_button, json, keytip_table, small_button};

pub(super) fn review(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::Spell, "Spelling", "Spelling (F7)", false);
    control_keytip(app, ui, &__k, "dialog:spelling");
    if __k.clicked() {
        app.open_dialog("spelling", json!({}));
    }
    let __k = big_button(ui, Icon::Calc, "Workbook\nStatistics", "Workbook Statistics", false);
    control_keytip(app, ui, &__k, "statistics");
    if __k.clicked() {
        app.open_dialog("statistics", json!({}));
    }
    let __k = big_button(ui, Icon::Validation, "Check\nAccessibility", "Check Accessibility", false);
    control_keytip(app, ui, &__k, "dialog:accessibility");
    if __k.clicked() {
        app.open_dialog("accessibility", json!({}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::Comment, "New\nComment", "New Comment", false);
    control_keytip(app, ui, &__k, "dialog:comment");
    if __k.clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    let __k = big_button(ui, Icon::Delete, "Delete", "Delete Comment", false);
    control_keytip(app, ui, &__k, "review.deleteComment");
    if __k.clicked() {
        act(app, "review.deleteComment", json!({}));
    }
    ui.vertical(|ui| {
        for (icon, label, tip, id) in
            [(Icon::Left, "Previous", "Previous Comment", "review.previousComment"), (Icon::Right, "Next", "Next Comment", "review.nextComment")]
        {
            let button = small_button(ui, icon, label, tip, false);
            control_keytip(app, ui, &button, id);
            if button.clicked() {
                act(app, id, json!({}));
                app.grid.ensure_visible = true;
            }
        }
    });
    let shown = app.session.comments_pane;
    let r = big_button(ui, Icon::Comment, "Show\nComments", "Comments pane", false);
    if shown {
        ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if r.clicked() {
        act(app, "review.showComments", json!({"on": !shown}));
        if app.session.comments_pane {
            app.grid.pane = None;
        }
    }
    notes(app, ui);
    sep(ui);
    let protected = app.active_sheet_protected();
    let __k = big_button(ui, Icon::Lock, if protected { "Unprotect\nSheet" } else { "Protect\nSheet" }, "Protect Sheet", false);
    control_keytip(app, ui, &__k, "dialog:protectSheet");
    if __k.clicked() {
        app.open_dialog("protectSheet", json!({}));
    }
    let __k = big_button(ui, Icon::Book, "Protect\nWorkbook", "Protect Workbook structure", false);
    control_keytip(app, ui, &__k, "review.protectWorkbook");
    if __k.clicked() {
        crate::backstage::protect_workbook(app);
    }
    if big_button(ui, Icon::Validation, "Allow Edit\nRanges", "Allow Edit Ranges", false).clicked() {
        app.open_dialog("allowEditRanges", json!({}));
    }
}

fn notes(app: &mut SheetApp, ui: &mut Ui) {
    let button = big_button(ui, Icon::Note, "Notes", "Notes", true);
    anchor_keytip(app, ui, &button, &["review.newNote", "review.editNote", "review.showAllNotes"]);
    let has_note = app.session.active().and_then(|d| d.wb.active()?.comments.get(&d.selection.active)).is_some_and(|c| !c.threaded);
    let (label, target) = if has_note { ("Edit Note", "review.editNote") } else { ("New Note", "review.newNote") };
    let new = invoke(app, "review.newNote");
    let edit = invoke(app, "review.editNote");
    if (new && !has_note) || (edit && has_note) {
        app.open_dialog("comment", json!({"threaded": false}));
    }

    let prefix = app.keytips.prefix().unwrap_or_default().to_owned();
    let keyed = keytip_table::opens_menu(&prefix, "review.newNote");
    let focus_item = keyed && !egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&button));
    egui::Popup::menu(&button).accessible_name(app.ui.language.tr("Notes")).open_memory(popup_command(keyed, button.clicked())).show(|ui| {
        let label = app.ui.language.tr(label);
        let item = ui.add(egui::Button::new(label).frame_when_inactive(false).min_size(egui::vec2(220.0, 22.0)));
        if focus_item {
            item.request_focus();
        }
        if let Some(sequence) = keytip_table::item_letter(&prefix, target) {
            keytip(app, ui, sequence, egui::pos2(item.rect.right() - 10.0, item.rect.center().y));
        }
        if item.clicked() {
            app.open_dialog("comment", json!({"threaded": false}));
        }
        menu_items(
            app,
            ui,
            &[
                ("Show/Hide Note", "review.showNote", json!({})),
                ("Show All Notes", "review.showAllNotes", json!({})),
                ("-", "", json!({})),
                ("Convert to Comments", "review.convertToComments", json!({})),
            ],
        );
    });
}
