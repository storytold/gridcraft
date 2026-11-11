use super::controls::{RibbonSections, act, anchor_keytip, control_keytip, group_end, menu_items, section, sep};
use super::{Icon, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_button, json, small_button};

pub(super) fn review(app: &mut SheetApp, ui: &mut Ui) {
    // Excel's measured Review rows, in client points: Proofing stacks its three buttons between
    // 819 and 1280 and is one row (or its compact row below 819) either side; Comments stacks its
    // six between 880 and 1280, one row from 1280 and compact below 880.
    RibbonSections::breaks(ui, "Proofing", &[(1280.0, 1), (819.0, 3)]);
    RibbonSections::breaks(ui, "Comments", &[(1280.0, 1), (880.0, 3)]);
    section(ui, app, "Proofing", Icon::Spell, |app, ui| {
        if RibbonSections::level(ui, "Proofing") == 3 {
            ui.vertical(|ui| {
                proofing_spelling(app, ui, false);
                proofing_statistics(app, ui, false);
                proofing_accessibility(app, ui, false);
            });
        } else {
            proofing_spelling(app, ui, true);
            proofing_statistics(app, ui, true);
            proofing_accessibility(app, ui, true);
        }
    });
    sep(ui, "Proofing");
    section(ui, app, "Comments", Icon::Comment, |app, ui| {
        if RibbonSections::level(ui, "Comments") == 3 {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    comment_new(app, ui, false);
                    comment_delete(app, ui, false);
                });
                ui.horizontal(|ui| {
                    comment_previous(app, ui);
                    comment_next(app, ui);
                });
                ui.horizontal(|ui| {
                    comment_show(app, ui, false);
                    comment_notes(app, ui, false);
                });
            });
        } else {
            comment_new(app, ui, true);
            comment_delete(app, ui, true);
            ui.vertical(|ui| {
                comment_previous(app, ui);
                comment_next(app, ui);
            });
            comment_show(app, ui, true);
            comment_notes(app, ui, true);
        }
    });
    sep(ui, "Comments");
    section(ui, app, "Protect", Icon::Lock, |app, ui| {
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
    });
    group_end(ui, "Protect");
}

/// Proofing's button, large on the wide one-row form and small in the stacked three-row form.
fn review_button(ui: &mut Ui, big: bool, icon: Icon, label: &str, tip: &str, dropdown: bool) -> egui::Response {
    if big { big_button(ui, icon, label, tip, dropdown) } else { small_button(ui, icon, label, tip, dropdown) }
}

fn proofing_spelling(app: &mut SheetApp, ui: &mut Ui, big: bool) {
    let r = review_button(ui, big, Icon::Spell, "Spelling", "Spelling (F7)", false);
    control_keytip(app, ui, &r, "dialog:spelling");
    if r.clicked() {
        app.open_dialog("spelling", json!({}));
    }
}

fn proofing_statistics(app: &mut SheetApp, ui: &mut Ui, big: bool) {
    let r = review_button(ui, big, Icon::Calc, if big { "Workbook\nStatistics" } else { "Workbook Statistics" }, "Workbook Statistics", false);
    control_keytip(app, ui, &r, "statistics");
    if r.clicked() {
        app.open_dialog("statistics", json!({}));
    }
}

fn proofing_accessibility(app: &mut SheetApp, ui: &mut Ui, big: bool) {
    let r = review_button(ui, big, Icon::Validation, if big { "Check\nAccessibility" } else { "Check Accessibility" }, "Check Accessibility", false);
    control_keytip(app, ui, &r, "dialog:accessibility");
    if r.clicked() {
        app.open_dialog("accessibility", json!({}));
    }
}

fn comment_new(app: &mut SheetApp, ui: &mut Ui, big: bool) {
    let r = review_button(ui, big, Icon::Comment, if big { "New\nComment" } else { "New Comment" }, "New Comment", false);
    control_keytip(app, ui, &r, "dialog:comment");
    if r.clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
}

fn comment_delete(app: &mut SheetApp, ui: &mut Ui, big: bool) {
    let r = review_button(ui, big, Icon::Delete, "Delete", "Delete Comment", false);
    control_keytip(app, ui, &r, "review.deleteComment");
    if r.clicked() {
        act(app, "review.deleteComment", json!({}));
    }
}

fn comment_previous(app: &mut SheetApp, ui: &mut Ui) {
    if small_button(ui, Icon::Left, "Previous", "Previous Comment", false).clicked() {
        act(app, "review.previousComment", json!({}));
        app.grid.ensure_visible = true;
    }
}

fn comment_next(app: &mut SheetApp, ui: &mut Ui) {
    if small_button(ui, Icon::Right, "Next", "Next Comment", false).clicked() {
        act(app, "review.nextComment", json!({}));
        app.grid.ensure_visible = true;
    }
}

fn comment_show(app: &mut SheetApp, ui: &mut Ui, big: bool) {
    let shown = app.session.comments_pane;
    let r = review_button(ui, big, Icon::Comment, if big { "Show\nComments" } else { "Show Comments" }, "Comments pane", false);
    if shown {
        ui.painter().rect_stroke(r.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if r.clicked() {
        act(app, "review.showComments", json!({"on": !shown}));
        if app.session.comments_pane {
            app.grid.pane = None;
        }
    }
}

fn comment_notes(app: &mut SheetApp, ui: &mut Ui, big: bool) {
    let notes = review_button(ui, big, Icon::Note, "Notes", "Notes", true);
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
}
