//! Insert ▸ Library: the shared Library pane over GridCraft's `library.*` commands. The Insert
//! tab's Library button opens it beside the sheet; its pictures fill in as thumbnails, a search
//! narrows them, and a click puts one on the sheet as one undo step.

use std::io::Cursor;
use std::path::PathBuf;

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use gridcraft_ui_egui::library_pane::{LABELS, PANE};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

use crate::common::Harness;
use crate::support::{app_harness, blank_harness, blank_session, services_harness};

/// A folder of its own under the system's temporary folder, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn path(&self) -> String {
        self.0.to_str().unwrap().to_string()
    }
}

fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(w, h, image::Rgba([rgb[0], rgb[1], rgb[2], 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// Two pictures and a file that only claims to be one.
fn pictures(tag: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("gridcraft-library-pane-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    std::fs::write(dir.join("blue-logo.png"), png(80, 40, [30, 90, 200])).unwrap();
    std::fs::write(dir.join("red-swatch.png"), png(40, 40, [200, 40, 40])).unwrap();
    std::fs::write(dir.join("broken.png"), b"not a picture").unwrap();
    Dir(dir)
}

fn images(h: &Harness) -> usize {
    h.state().session.active().unwrap().wb.active().unwrap().images.len()
}

/// Open the pane on `dir` and let it read every file and draw the thumbnails in view.
fn open_on(h: &mut Harness, dir: &Dir) {
    h.state_mut().run("library.addFolder", json!({"path": dir.path()})).unwrap();
    h.state_mut().run("insert.library", json!({"on": true})).unwrap();
    for _ in 0..40 {
        h.step();
        if h.state().library.ready() {
            break;
        }
    }
    h.run_steps(6);
}

fn pane_open(h: &Harness) -> bool {
    h.state().grid.pane.as_deref() == Some(PANE)
}

#[test]
fn the_insert_tabs_library_button_opens_and_closes_the_pane() {
    let mut h = blank_harness(4);
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Library").click();
    h.run_steps(3);
    assert!(pane_open(&h));
    // With no folder yet, it says what to do and offers Add Folder….
    assert!(h.query_by_label_contains("Add a folder of PNG or JPEG pictures").is_some());
    assert!(h.query_by_label("Add Folder…").is_some());
    // The shared tab strip with GridCraft's tab, beside the ribbon's own Pictures button.
    assert_eq!(h.query_all_by_label("Pictures").count(), 2);
    // The same button closes it.
    h.get_by_role_and_label(Role::Button, "Library").click();
    h.run_steps(3);
    assert!(!pane_open(&h));
    assert!(h.query_by_label("Add Folder…").is_none());
}

#[test]
fn pictures_fill_in_and_a_click_inserts_one() {
    let dir = pictures("click");
    let mut h = blank_harness(4);
    open_on(&mut h, &dir);
    // Every file's one picture in one grid, named under its thumbnail, with no heading per file:
    // listing reads no file, so the broken one is a cell too until its thumbnail is drawn.
    assert!(h.query_by_label("blue-logo").is_none());
    let cell = h.get_by_role_and_label(Role::Button, "Picture: blue-logo.png");
    assert!(h.query_by_role_and_label(Role::Button, "Picture: red-swatch.png").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Picture: broken.png").is_some());
    assert_eq!(images(&h), 0);
    cell.click();
    h.run_steps(3);
    assert_eq!(images(&h), 1, "a click puts the picture on the sheet");
    let wb = &h.state().session.active().unwrap().wb;
    let img = wb.active().unwrap().images.first().unwrap();
    assert_eq!(img.anchor.cell.a1(), "A1", "at the active cell");
    assert_eq!((img.anchor.width, img.anchor.height), (80.0, 40.0));
    // One undo step.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(images(&h), 0);
}

#[test]
fn a_search_narrows_the_pictures() {
    let dir = pictures("search");
    let mut h = blank_harness(4);
    open_on(&mut h, &dir);
    h.state_mut().library.query = "swatch".into();
    // The search runs a moment after the last key.
    for _ in 0..30 {
        h.step();
        if h.state().library.hits().is_some() {
            break;
        }
    }
    h.run_steps(3);
    assert_eq!(h.state().library.hits(), Some(1));
    assert!(h.query_by_role_and_label(Role::Button, "Picture: red-swatch.png").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Picture: blue-logo.png").is_none());
    h.state_mut().library.query = "nothing like it".into();
    h.run_steps(30);
    assert!(h.query_by_label_contains("Nothing matches").is_some());
}

#[test]
fn add_folder_uses_the_folder_picker() {
    let dir = pictures("picker");
    let picked = dir.path();
    let services = Services { pick_folder: Some(Box::new(move || Some(picked.clone()))), ..Default::default() };
    let mut h = services_harness(services, 4);
    h.state_mut().run("insert.library", json!({})).unwrap();
    h.run_steps(3);
    h.get_by_label("Add Folder…").click();
    h.run_steps(4);
    assert_eq!(h.state().session.prefs.library_folders, [dir.path()]);
    assert_eq!(h.state().library.folder.as_deref(), Some(dir.path().as_str()), "the new folder is shown");
    // Remove forgets it (the files stay where they are).
    h.get_by_label("Remove").click();
    h.run_steps(3);
    assert!(h.state().session.prefs.library_folders.is_empty());
    assert!(dir.0.join("blue-logo.png").is_file());
}

#[test]
fn without_a_picker_a_path_is_typed() {
    let dir = pictures("typed");
    let mut h = app_harness(crate::common::prepare(SheetApp::new(blank_session(), Services::default())), 4);
    h.state_mut().run("insert.library", json!({})).unwrap();
    h.run_steps(3);
    h.get_by_label("Add Folder…").click();
    h.run_steps(3);
    assert!(h.state().library.adding, "the path field shows");
    h.state_mut().library.new_folder = dir.path();
    h.run_steps(2);
    h.get_by_label("Add").click();
    h.run_steps(4);
    assert_eq!(h.state().session.prefs.library_folders, [dir.path()]);
    assert!(!h.state().library.adding);
}

#[test]
fn opening_the_library_closes_the_comments_pane() {
    let mut h = blank_harness(4);
    h.state_mut().run("review.showComments", json!({"on": true})).unwrap();
    h.run_steps(2);
    assert!(h.state().session.comments_pane);
    let on = h.state_mut().run("insert.library", json!({})).unwrap();
    assert_eq!(on, json!({"on": true}));
    h.run_steps(3);
    assert!(pane_open(&h));
    assert!(!h.state().session.comments_pane, "one task pane at a time");
    assert_eq!(h.state_mut().run("insert.library", json!({})).unwrap(), json!({"on": false}));
    h.run_steps(2);
    assert!(!pane_open(&h));
    // The words are GridCraft's.
    assert_eq!(LABELS.nouns.many, "pictures");
}

#[test]
fn the_pane_speaks_the_interface_language() {
    let dir = pictures("german");
    let mut h = blank_harness(4);
    h.state_mut().ui.language = gridcraft_ui_egui::i18n::Language::De;
    open_on(&mut h, &dir);
    assert!(h.query_by_label("Ordner hinzufügen…").is_some(), "the pane's own words");
    assert!(h.query_by_label("Bilder").is_some(), "GridCraft's tab");
    assert!(h.query_by_role_and_label(Role::Button, "Bild: blue-logo.png").is_some(), "an item's name");
    assert!(h.query_by_label("Add Folder…").is_none());
    assert!(h.query_by_label("Bibliothek").is_some(), "the pane's title");
}
