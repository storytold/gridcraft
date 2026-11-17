//! Format › Font… on macOS: the system font panel, as WordCraft has (storytold/wordcraft#6865).
//! It has no shortcut: ⌘D is Fill Down and ⌘T Insert Table, as in Excel. The panel sends `changeFont:` to the font manager's target, an object
//! of ours here, which converts the selection's font the way the panel says and hands what changed
//! (family, size, bold, italic) to the app, which formats the selected cells with it.
//!
//! An exception to the workspace's no-`unsafe` rule (AGENTS.md): receiving `changeFont:` needs an
//! Objective-C class (`define_class!`), and setting the manager's target is marked unsafe.

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender, channel};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSFont, NSFontManager, NSFontPanel, NSFontTraitMask};
use objc2_foundation::NSString;

use gridcraft_ui_egui::SheetApp;

/// What the user changed in the panel. Only that is applied, so a new size keeps each cell's own
/// family, bold and italic in a mixed selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pick {
    pub family: Option<String>,
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
}

/// A font as the app formats cells with it.
#[derive(Debug, Clone, PartialEq)]
struct Face {
    family: String,
    size: f64,
    bold: bool,
    italic: bool,
}

impl Pick {
    fn between(before: &Face, after: &Face) -> Self {
        Pick {
            family: (after.family != before.family).then(|| after.family.clone()),
            size: (after.size != before.size).then_some(after.size),
            bold: (after.bold != before.bold).then_some(after.bold),
            italic: (after.italic != before.italic).then_some(after.italic),
        }
    }
}

struct Ivars {
    picks: Sender<Pick>,
    /// The font the panel converts: the active cell's, then each pick.
    current: RefCell<Retained<NSFont>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `FontTarget` doesn't implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GridCraftFontTarget"]
    #[ivars = Ivars]
    struct FontTarget;

    impl FontTarget {
        // SAFETY: `changeFont:` takes the sender (the font manager) and returns nothing.
        #[unsafe(method(changeFont:))]
        fn change_font(&self, _sender: Option<&AnyObject>) {
            let manager = NSFontManager::sharedFontManager(self.mtm());
            let before = self.ivars().current.borrow().clone();
            let font = manager.convertFont(&before);
            let pick = Pick::between(&face(&manager, &before), &face(&manager, &font));
            if pick != Pick::default() {
                let _ = self.ivars().picks.send(pick);
            }
            self.ivars().current.replace(font);
        }
    }
);

#[allow(unsafe_code)]
impl FontTarget {
    fn new(mtm: MainThreadMarker, picks: Sender<Pick>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { picks, current: RefCell::new(NSFont::systemFontOfSize(12.0)) });
        // SAFETY: NSObject's designated initializer, on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

fn face(manager: &NSFontManager, font: &NSFont) -> Face {
    let traits = manager.traitsOfFont(font);
    Face {
        family: font.familyName().map(|f| f.to_string()).unwrap_or_default(),
        size: font.pointSize(),
        bold: traits.contains(NSFontTraitMask::BoldFontMask),
        italic: traits.contains(NSFontTraitMask::ItalicFontMask),
    }
}

/// Where Format › Font… opens the panel and the picks arrive. Lives as long as the app: the font
/// manager doesn't keep its target alive.
pub struct FontPanel {
    target: Retained<FontTarget>,
    picks: Receiver<Pick>,
    /// The font the panel shows, so it changes only when the selection's does.
    shown: RefCell<Option<Face>>,
}

impl FontPanel {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let (tx, picks) = channel();
        FontPanel { target: FontTarget::new(mtm, tx), picks, shown: RefCell::new(None) }
    }

    /// Shows the panel on the active cell's font.
    pub fn show(&self, app: &SheetApp) {
        self.show_face(active_face(app));
    }

    fn show_face(&self, face: Face) {
        let mtm = self.target.mtm();
        self.select(face);
        set_target(&NSFontManager::sharedFontManager(mtm), &self.target);
        NSFontPanel::sharedFontPanel(mtm).orderFront(None);
    }

    /// While the panel is open, it shows the active cell's font as that changes (another cell, or
    /// formatting from the ribbon), so a pick converts the cell's own font.
    pub fn follow(&self, app: &SheetApp) {
        let mtm = self.target.mtm();
        if !NSFontPanel::sharedFontPanelExists(mtm) || !NSFontPanel::sharedFontPanel(mtm).isVisible() {
            return;
        }
        let face = active_face(app);
        if self.shown.borrow().as_ref() != Some(&face) {
            self.select(face);
        }
    }

    fn select(&self, face: Face) {
        let manager = NSFontManager::sharedFontManager(self.target.mtm());
        let mut traits = NSFontTraitMask::empty();
        traits.set(NSFontTraitMask::BoldFontMask, face.bold);
        traits.set(NSFontTraitMask::ItalicFontMask, face.italic);
        // A family the system doesn't have (a workbook's, a font GridCraft carries) shows the system
        // font, bold and italic as the cell is.
        let font = manager
            .fontWithFamily_traits_weight_size(&NSString::from_str(&face.family), traits, 5, face.size)
            .unwrap_or_else(|| manager.convertFont_toHaveTrait(&NSFont::systemFontOfSize(face.size), traits));
        self.target.ivars().current.replace(font.clone());
        manager.setSelectedFont_isMultiple(&font, false);
        self.shown.replace(Some(face));
    }

    /// Shows the panel on a given font, for `tests/mac_font_panel.rs`.
    #[allow(dead_code)]
    pub fn show_for_test(&self, family: &str, size: f64, bold: bool, italic: bool) {
        self.show_face(Face { family: family.into(), size, bold, italic });
    }

    /// What the panel does when the user picks a font, for `tests/mac_font_panel.rs`.
    #[allow(dead_code)]
    pub fn change_font_for_test(&self) {
        self.target.change_font(objc2::sel!(changeFont:), None);
    }

    /// The picks since the last call, oldest first. The panel then shows the active cell's font
    /// again at the next [`follow`](Self::follow), as a pick the app refused leaves the cell as it was.
    pub fn picks(&self) -> Vec<Pick> {
        let picks: Vec<Pick> = self.picks.try_iter().collect();
        if !picks.is_empty() {
            self.shown.replace(None);
        }
        picks
    }
}

/// The active cell's font, as the ribbon's Font group shows it.
fn active_face(app: &SheetApp) -> Face {
    let font = app.session.active().and_then(|d| Some(d.wb.styles.get(d.wb.active()?.style_id(d.selection.active)).font.clone())).unwrap_or_default();
    Face { family: font.name, size: f64::from(font.size), bold: font.bold, italic: font.italic }
}

/// Formats the selected cells with what the user changed, as the ribbon's Font, Size, Bold and
/// Italic do.
pub fn apply(app: &mut SheetApp, pick: &Pick) {
    let runs = [
        pick.family.as_ref().map(|name| ("home.fontName", serde_json::json!({"name": name}))),
        pick.size.map(|size| ("home.fontSize", serde_json::json!({"size": size}))),
        pick.bold.map(|on| ("home.bold", serde_json::json!({"on": on}))),
        pick.italic.map(|on| ("home.italic", serde_json::json!({"on": on}))),
    ];
    for (id, params) in runs.into_iter().flatten() {
        app.run_or_alert(id, params);
    }
}

#[allow(unsafe_code)]
fn set_target(manager: &NSFontManager, target: &FontTarget) {
    // SAFETY: the target responds to the manager's action (`changeFont:`), and `FontPanel` keeps it
    // alive for as long as the app runs, so the manager's unretained reference stays valid.
    unsafe { manager.setTarget(Some(target)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_changed_is_picked() {
        let before = Face { family: "Helvetica".into(), size: 12.0, bold: true, italic: false };
        let bigger = Face { size: 18.0, ..before.clone() };
        assert_eq!(Pick::between(&before, &bigger), Pick { size: Some(18.0), ..Pick::default() }, "bold stays each run's own");
        let plain = Face { bold: false, ..before.clone() };
        assert_eq!(Pick::between(&before, &plain), Pick { bold: Some(false), ..Pick::default() });
        assert_eq!(Pick::between(&before, &before), Pick::default());
    }
}
