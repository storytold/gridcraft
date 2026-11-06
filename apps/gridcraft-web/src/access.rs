//! Access modes for a host page: `?mode=view|comment|edit` when the page loads (see the `host`
//! module). `set_mode` applies a new mode at once, for a host message that may carry it later.
//!
//! In view mode nothing in a workbook may change. In comment mode only notes and comments may,
//! and only the user's own (the author named by `?author=`), plus replies of theirs added to
//! someone else's thread. Edit mode is GridCraft as it is.
//!
//! The rule is applied after every frame to whatever that frame changed, so it holds however the
//! change was made (typing, paste, the ribbon, menus, keyboard shortcuts, undo, dialogs): a change
//! the mode does not allow is put back, and a short notice says why. What a person only looks at
//! (the active sheet, zoom, scroll position, page view) is never put back. The host still decides
//! what it stores; this keeps the screen honest about what the person may do.

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_engine::model::{Comment, Workbook};
use gridcraft_ui_egui::SheetApp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Edit,
    Comment,
    View,
}

impl Mode {
    pub fn parse(value: &str) -> Option<Mode> {
        match value {
            "edit" => Some(Mode::Edit),
            "comment" => Some(Mode::Comment),
            "view" => Some(Mode::View),
            _ => None,
        }
    }

    /// Whether Save hands the workbook to the host (comment mode saves comments).
    pub fn saves(self) -> bool {
        self != Mode::View
    }
}

pub const VIEW_NOTICE: &str = "You can view this workbook, but not change it.";
pub const COMMENT_NOTICE: &str = "You can add comments here, but not change the workbook's content.";

/// Remembers, per open workbook, its content as last allowed.
#[derive(Default)]
pub struct Gate {
    mode: Mode,
    allowed: Vec<(u64, Arc<Workbook>)>,
}

impl Gate {
    pub fn new(mode: Mode) -> Gate {
        Gate { mode, allowed: Vec::new() }
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// A new mode applies at once; what is on screen now is its starting point.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.allowed.clear();
    }

    /// After a frame: puts back whatever the mode does not allow. True when something was.
    pub fn enforce(&mut self, app: &mut SheetApp) -> bool {
        if self.mode == Mode::Edit {
            self.allowed.clear();
            return false;
        }
        // Typing into a cell is never allowed outside edit mode, so its editor never stays open.
        app.editor = None;
        if self.mode == Mode::View {
            app.text_box_editor = None;
        }
        let author = app.session.prefs.user_name.clone();
        let mode = self.mode;
        let Some(doc) = app.session.active_mut() else { return false };
        let Some(index) = self.allowed.iter().position(|(uid, _)| *uid == doc.uid) else {
            self.allowed.push((doc.uid, doc.wb.clone()));
            return false;
        };
        let before = self.allowed[index].1.clone();
        if Arc::ptr_eq(&before, &doc.wb) {
            return false;
        }
        let permitted = permitted(&before, &doc.wb, mode, &author);
        if permitted == *doc.wb {
            self.allowed[index].1 = doc.wb.clone();
            return false;
        }
        // Reuse the saved or previous workbook when the content is the same, so "unsaved
        // changes" is exactly what it was.
        let restored = if permitted == *doc.saved {
            doc.saved.clone()
        } else if permitted == *before {
            before
        } else {
            Arc::new(permitted)
        };
        doc.wb = restored.clone();
        doc.revision = doc.revision.wrapping_add(1);
        self.allowed[index].1 = restored;
        let notice = if mode == Mode::View { VIEW_NOTICE } else { COMMENT_NOTICE };
        app.toast = Some((notice.into(), gridcraft_ui_egui::now_ms()));
        true
    }
}

/// `before` with what `now` may keep in `mode`: the view settings, and in comment mode the
/// author's own comment changes.
fn permitted(before: &Workbook, now: &Workbook, mode: Mode, author: &str) -> Workbook {
    let mut out = before.clone();
    if before.sheets.len() != now.sheets.len() {
        return out;
    }
    out.active_sheet = now.active_sheet.clone();
    out.view = now.view;
    for ((sheet, was), is) in out.sheets.iter_mut().zip(&before.sheets).zip(&now.sheets) {
        let same_view = was.zoom == is.zoom && was.view_active == is.view_active && was.view_top_left == is.view_top_left && was.view == is.view;
        let comments = if mode == Mode::Comment { merged_comments(&was.comments, &is.comments, author) } else { None };
        if same_view && comments.is_none() {
            continue;
        }
        let sheet = Arc::make_mut(sheet);
        sheet.zoom = is.zoom;
        sheet.view_active = is.view_active;
        sheet.view_top_left = is.view_top_left;
        sheet.view = is.view;
        if let Some(comments) = comments {
            sheet.comments = comments;
        }
    }
    out
}

/// The comments `author` may leave: theirs as they are now, others' as they were (with the
/// author's replies added at the end). None when nothing changed.
fn merged_comments<K: Ord + Clone>(was: &BTreeMap<K, Comment>, is: &BTreeMap<K, Comment>, author: &str) -> Option<BTreeMap<K, Comment>> {
    if was == is {
        return None;
    }
    let mut out = BTreeMap::new();
    for (cell, before) in was {
        let after = is.get(cell);
        if before.author == author || after.is_some_and(|after| only_own_replies(before, after, author)) {
            if let Some(after) = after {
                out.insert(cell.clone(), after.clone());
            }
        } else {
            out.insert(cell.clone(), before.clone());
        }
    }
    for (cell, after) in is {
        if !was.contains_key(cell) && after.author == author {
            out.insert(cell.clone(), after.clone());
        }
    }
    Some(out)
}

/// `after` is `before` with only replies by `author` added at the end.
fn only_own_replies(before: &Comment, after: &Comment, author: &str) -> bool {
    if after.replies.len() < before.replies.len() {
        return false;
    }
    let mut head = after.clone();
    head.replies.truncate(before.replies.len());
    head.dates.truncate(before.dates.len());
    head == *before && after.replies[before.replies.len()..].iter().all(|(who, _)| who == author)
}

#[cfg(test)]
mod tests;
