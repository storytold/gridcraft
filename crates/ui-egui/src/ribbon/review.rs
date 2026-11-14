use super::controls::{act, anchor_keytip, control_keytip, menu_items, sep};
use super::{Icon, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_button, json, small_button};

pub(super) fn review(app: &mut SheetApp, ui: &mut Ui) {
    let is_rtl = app.ui.language.is_rtl();

    // =========================================================================
    // הצמדה דינמית לקצה הימני ב-RTL
    // =========================================================================
    let ribbon_id = egui::Id::unique("review_total_content_width");
    let last_measured_width = ui.data(|d| d.get_temp::<f32>(ribbon_id)).unwrap_or(860.0);
    let container_width = ui.max_rect().width();
    let margin_right = 5.0;

    if is_rtl && container_width > (last_measured_width + margin_right) {
        let left_padding = container_width - last_measured_width - margin_right;
        ui.add_space(left_padding);
    }

    let start_x = ui.cursor().min.x;

    let draw_proofing = |app: &mut SheetApp, ui: &mut Ui| {
        let items = [
            (Icon::Spell, "Spelling", "Spelling (F7)", "dialog:spelling", true),
            (Icon::Calc, "Workbook\nStatistics", "Workbook Statistics", "statistics", false),
            (Icon::Validation, "Check\nAccessibility", "Check Accessibility", "dialog:accessibility", true),
        ];

        let render_btn = |(icon, label, tip, cmd, is_dialog): (Icon, &str, &str, &str, bool), app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, icon, label, tip, false);
            control_keytip(app, ui, &__k, cmd);
            if __k.clicked() {
                if is_dialog {
                    let d = cmd.strip_prefix("dialog:").unwrap_or(cmd);
                    app.open_dialog(d, json!({}));
                } else {
                    app.open_dialog(cmd, json!({}));
                }
            }
        };

        if is_rtl {
            for it in items.into_iter().rev() {
                render_btn(it, app, ui);
            }
        } else {
            for it in items {
                render_btn(it, app, ui);
            }
        }
    };

    let draw_comments = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_new_comment = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Comment, "New\nComment", "New Comment", false);
            control_keytip(app, ui, &__k, "dialog:comment");
            if __k.clicked() {
                app.open_dialog("comment", json!({"threaded": true}));
            }
        };

        let draw_delete_comment = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Delete, "Delete", "Delete Comment", false);
            control_keytip(app, ui, &__k, "review.deleteComment");
            if __k.clicked() {
                act(app, "review.deleteComment", json!({}));
            }
        };

        let draw_nav_comments = |app: &mut SheetApp, ui: &mut Ui| {
            ui.vertical(|ui| {
                let nav_items = if is_rtl {
                    [
                        (Icon::Right, "Next", "Next Comment", "review.nextComment"),
                        (Icon::Left, "Previous", "Previous Comment", "review.previousComment"),
                    ]
                } else {
                    [
                        (Icon::Left, "Previous", "Previous Comment", "review.previousComment"),
                        (Icon::Right, "Next", "Next Comment", "review.nextComment"),
                    ]
                };

                for (icon, label, tip, id) in nav_items {
                    if small_button(ui, icon, label, tip, false).clicked() {
                        act(app, id, json!({}));
                        app.grid.ensure_visible = true;
                    }
                }
            });
        };

        let draw_show_comments = |app: &mut SheetApp, ui: &mut Ui| {
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
        };

        let draw_notes = |app: &mut SheetApp, ui: &mut Ui| {
            let notes = big_button(ui, Icon::Note, "Notes", "Notes", true);
            anchor_keytip(app, ui, &notes, &["review.nextComment"]);
            egui::Popup::menu(&notes).show(|ui| {
                menu_items(
                    app,
                    ui,
                    &[
                        ("New Note", "dialog:comment", json!({"threaded": false})),
                        ("Show/Hide Note", "review.showNote", json!({})),
                        ("Show All Notes", "review.showAllNotes", json!({})),
                        ("-", "", json!({})),
                        ("Convert to Comments", "review.convertToComments", json!({})),
                    ],
                );
            });
        };

        if is_rtl {
            draw_notes(app, ui);
            draw_show_comments(app, ui);
            draw_nav_comments(app, ui);
            draw_delete_comment(app, ui);
            draw_new_comment(app, ui);
        } else {
            draw_new_comment(app, ui);
            draw_delete_comment(app, ui);
            draw_nav_comments(app, ui);
            draw_show_comments(app, ui);
            draw_notes(app, ui);
        }
    };

    let draw_protect = |app: &mut SheetApp, ui: &mut Ui| {
        let draw_protect_sheet = |app: &mut SheetApp, ui: &mut Ui| {
            let protected = app.active_sheet_protected();
            let __k = big_button(ui, Icon::Lock, if protected { "Unprotect\nSheet" } else { "Protect\nSheet" }, "Protect Sheet", false);
            control_keytip(app, ui, &__k, "dialog:protectSheet");
            if __k.clicked() {
                app.open_dialog("protectSheet", json!({}));
            }
        };

        let draw_protect_workbook = |app: &mut SheetApp, ui: &mut Ui| {
            let __k = big_button(ui, Icon::Book, "Protect\nWorkbook", "Protect Workbook structure", false);
            control_keytip(app, ui, &__k, "review.protectWorkbook");
            if __k.clicked() {
                crate::backstage::protect_workbook(app);
            }
        };

        let draw_allow_edit_ranges = |app: &mut SheetApp, ui: &mut Ui| {
            if big_button(ui, Icon::Validation, "Allow Edit\nRanges", "Allow Edit Ranges", false).clicked() {
                app.open_dialog("allowEditRanges", json!({}));
            }
        };

        if is_rtl {
            draw_allow_edit_ranges(app, ui);
            draw_protect_workbook(app, ui);
            draw_protect_sheet(app, ui);
        } else {
            draw_protect_sheet(app, ui);
            draw_protect_workbook(app, ui);
            draw_allow_edit_ranges(app, ui);
        }
    };

    if is_rtl {
        draw_protect(app, ui);
        sep(ui);
        draw_comments(app, ui);
        sep(ui);
        draw_proofing(app, ui);
    } else {
        draw_proofing(app, ui);
        sep(ui);
        draw_comments(app, ui);
        sep(ui);
        draw_protect(app, ui);
    }

    if is_rtl {
        let total_width = ui.cursor().min.x - start_x;
        ui.data_mut(|d| d.insert_temp(ribbon_id, total_width));
    }
}