//! Draws the Arabic interface (every ribbon tab, dialog and Backstage page, at several window
//! sizes) and checks what comes out: it never panics (an egui layout that meets a NaN rectangle
//! does, and a plain `egui::Grid` in a right-aligned area is one), the window is mirrored, and
//! every label the interface asked to translate has an Arabic entry.

use std::cell::RefCell;
use std::collections::BTreeSet;

use egui::Shape;
use gridcraft_engine::Session;
use serde_json::json;

use super::Language;
use crate::SheetApp;

thread_local! {
    /// Labels asked of [`Language::tr`] in Arabic that have no entry (this thread's test only).
    static MISSING: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
}

pub(super) fn note_missing(text: &str) {
    MISSING.with(|m| {
        m.borrow_mut().insert(text.to_owned());
    });
}

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(language: Language, size: egui::Vec2) -> Harness {
    let mut session = Session::new();
    session.execute("file.new", json!({"sample": "sales"})).expect("the sales sample opens");
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = language;
    let mut h = Harness::builder().with_size(size).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(6);
    h
}

fn collect(shape: &Shape, out: &mut Vec<(String, egui::Rect)>) {
    match shape {
        Shape::Text(t) => out.push((t.galley.text().to_string(), egui::Rect::from_min_size(t.pos, t.galley.size()))),
        Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

/// Every string painted in the last frame, with where.
fn painted(h: &Harness) -> Vec<(String, egui::Rect)> {
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

fn center_x(h: &Harness, text: &str) -> Option<f32> {
    painted(h).into_iter().find(|(t, _)| t.trim() == text).map(|(_, r)| r.center().x)
}

/// Closes whatever a scene opened.
fn reset(h: &mut Harness) {
    let ctx = h.ctx.clone();
    egui::Popup::close_all(&ctx);
    let app = h.state_mut();
    app.dialog = None;
    app.message = None;
    app.backstage = None;
    h.run_steps(2);
}

const DIALOGS: &[&str] = &[
    "about",
    "accessibility",
    "agents",
    "allowEditRanges",
    "analysisToolPak",
    "cfQuick",
    "chartTitle",
    "columnWidth",
    "commandSearch",
    "comment",
    "comments",
    "dataValidation",
    "defaultWidth",
    "defineName",
    "deleteCells",
    "errorChecking",
    "evaluateFormula",
    "find",
    "forecastSheet",
    "formatCells",
    "formatChart",
    "goTo",
    "goToSpecial",
    "goalSeek",
    "headerFooter",
    "insertCells",
    "insertFunction",
    "insertLink",
    "insertPaste",
    "insertPicture",
    "journal",
    "macros",
    "manageRules",
    "moveSheet",
    "nameManager",
    "openPassword",
    "pageSetup",
    "pasteSpecial",
    "pickList",
    "pivotCalculatedField",
    "protectSheet",
    "protectWorkbook",
    "recommendedCharts",
    "removeDuplicates",
    "renameSheet",
    "rowHeight",
    "saveChanges",
    "saveCopy",
    "selectData",
    "selectionPane",
    "series",
    "solver",
    "solverResults",
    "sort",
    "sparkline",
    "spelling",
    "start",
    "statistics",
    "subtotal",
    "textToColumns",
    "unhideSheet",
    "unprotectSheet",
    "unprotectWorkbook",
    "watch",
    "zoom",
];

const RIBBON_TABS: &[&str] = &["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"];

/// Dialog tabs worth opening as well as the first one.
const DIALOG_TABS: &[&str] = &["Alignment", "Font", "Border", "Fill", "Protection", "Replace", "Contributors", "Models", "All Charts"];

/// Draws every scene once. `each` sees the harness after each.
fn every_scene(h: &mut Harness, mut each: impl FnMut(&str, &Harness)) {
    each("main window", h);
    for tab in RIBBON_TABS {
        h.state_mut().ui.ribbon_tab = (*tab).into();
        h.run_steps(3);
        each(&format!("ribbon {tab}"), h);
    }
    h.state_mut().ui.ribbon_tab = "Home".into();
    for name in DIALOGS {
        h.state_mut().open_dialog(name, json!({}));
        h.run_steps(3);
        each(&format!("dialog {name}"), h);
        for tab in DIALOG_TABS {
            if let Some(d) = h.state_mut().dialog.as_mut() {
                d.tab = (*tab).to_string();
                h.run_steps(2);
                each(&format!("dialog {name} / {tab}"), h);
            }
        }
        reset(h);
    }
    for page in crate::backstage::Page::ALL {
        h.state_mut().open_backstage(page);
        h.run_steps(3);
        each(&format!("backstage {}", page.id()), h);
        reset(h);
    }
}

#[test]
fn the_arabic_interface_is_mirrored_and_never_panics() {
    // Wide, ordinary and narrow windows: the ribbon and the sheet tabs scroll in the narrow one.
    for size in [egui::vec2(1440.0, 900.0), egui::vec2(1024.0, 700.0), egui::vec2(640.0, 480.0)] {
        let mut h = harness(Language::Ar, size);
        every_scene(&mut h, |_, _| {});
        // A right-to-left sheet inside the mirrored window, and back.
        h.state_mut().session.execute("view.rightToLeft", json!({"on": true})).expect("sheet direction");
        h.run_steps(3);
        every_scene(&mut h, |_, _| {});
        h.state_mut().session.execute("view.rightToLeft", json!({"on": false})).expect("sheet direction");
        // The language can change while the app runs: the fonts are rebuilt for it.
        for language in [Language::En, Language::Ar, Language::Ja, Language::Ar] {
            h.state_mut().ui.language = language;
            h.run_steps(6);
        }
    }
}

#[test]
fn the_window_is_mirrored_in_arabic_and_not_in_english() {
    let size = egui::vec2(1440.0, 900.0);
    let (ar, en) = (harness(Language::Ar, size), harness(Language::En, size));
    let middle = size.x / 2.0;
    // The File tab and the first sheet tab start the reading side; the status text begins there too.
    let file_ar = center_x(&ar, Language::Ar.tr("File")).expect("File tab, Arabic");
    let file_en = center_x(&en, "File").expect("File tab, English");
    assert!(file_ar > middle && file_en < middle, "File tab at {file_ar} (ar) and {file_en} (en)");
    let sheet_ar = center_x(&ar, "Sales").expect("sheet tab");
    let sheet_en = center_x(&en, "Sales").expect("sheet tab");
    assert!(sheet_ar > middle && sheet_en < middle, "sheet tab at {sheet_ar} (ar) and {sheet_en} (en)");
    let ready_ar = center_x(&ar, Language::Ar.tr("Ready")).expect("status text");
    let ready_en = center_x(&en, "Ready").expect("status text");
    assert!(ready_ar > middle && ready_en < middle, "status at {ready_ar} (ar) and {ready_en} (en)");
    let zoom_ar = center_x(&ar, "100%").expect("zoom");
    let zoom_en = center_x(&en, "100%").expect("zoom");
    assert!(zoom_ar < middle && zoom_en > middle, "zoom at {zoom_ar} (ar) and {zoom_en} (en)");
    // The sheet is a document, not the interface: column A stays on the left.
    let cell_ar = center_x(&ar, "North").expect("a cell");
    let cell_en = center_x(&en, "North").expect("a cell");
    assert!((cell_ar - cell_en).abs() < 1.0, "the sheet is not mirrored by the interface language: {cell_ar} vs {cell_en}");
}

/// Painted words the interface does not translate: names and numbers in the sample workbook and
/// product names. Everything else asked of `tr` in Arabic has an entry.
fn is_data(label: &str) -> bool {
    label.is_empty()
        || !label.chars().any(|c| c.is_alphabetic())
        // Workbook content that passes through `tr` on its way to the screen.
        || ["Sales", "Book1", "Calibri", "Workbook", "Sheet1"].contains(&label)
}

#[test]
fn every_label_the_arabic_interface_asks_for_has_an_entry() {
    MISSING.with(|m| m.borrow_mut().clear());
    let mut h = harness(Language::Ar, egui::vec2(1440.0, 900.0));
    every_scene(&mut h, |_, _| {});
    let missing: Vec<String> = MISSING.with(|m| m.borrow().iter().filter(|l| !is_data(l)).cloned().collect());
    assert!(missing.is_empty(), "no Arabic entry for: {missing:#?}");
}
