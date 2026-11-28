//! `font.add`: a font handed over at run time (the web build has no font folders) is bound for the
//! workbook's cells set in it, as an installed font is.

use egui::vec2;
use gridcraft_engine::Session;
use gridcraft_engine::io::base64_encode;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::fonts::{self, role_key};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn app_with_cell_in(font: &str) -> Harness {
    // A native build binds the workbook's fonts once the host scan is done (the desktop app starts
    // it at launch; a wasm build has none to wait for).
    fonts::catalog();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B2", "input": "سلام"})).unwrap();
    session.execute("home.formatCells", json!({"range": "B2", "style": {"font": {"name": font, "bold": true}}})).unwrap();
    let mut h = Harness::builder().with_size(vec2(900.0, 600.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(2);
    h
}

/// Steps the app a few frames, until `role` is bound.
fn step_until_bound(h: &mut Harness, role: &str) -> bool {
    for _ in 0..8 {
        if fonts::is_bound(role) {
            return true;
        }
        h.run_steps(1);
    }
    fonts::is_bound(role)
}

#[test]
fn an_added_font_binds_the_cells_set_in_it_and_bad_files_are_refused() {
    let mut h = app_with_cell_in("Hack");
    // The machine running the test may not have Hack installed: the font only comes from font.add.
    let installed = !fonts::faces_of("Hack").is_empty();
    let regular = role_key("Hack", false, false);
    let bold = role_key("Hack", true, false);
    if !installed {
        h.run_steps(4);
        assert!(!fonts::is_bound(&bold), "Hack is not bound before it is added");
    }

    let data = base64_encode(epaint_default_fonts::HACK_REGULAR);
    let r = h.state_mut().run("font.add", json!({"data": data, "name": "Hack-Regular.ttf"})).expect("font.add");
    assert_eq!(r, json!({"faces": [{"family": "Hack", "weight": 400.0, "italic": false}]}));
    assert!(fonts::families().iter().any(|f| f == "Hack"), "the font list offers it");
    assert!(step_until_bound(&mut h, &bold), "the cell's bold role is bound after font.add");
    assert!(fonts::is_bound(&regular));
    // A family with no bold face of its own is thickened when drawn bold, as an installed one is.
    assert!(fonts::is_emboldened("Hack", false));

    // The same file again, and files that are no fonts, are refused and change nothing.
    let generation = fonts::added_generation();
    let again = h.state_mut().run("font.add", json!({"data": data})).unwrap_err();
    assert!(again.contains("already added"), "{again}");
    let not_font = h.state_mut().run("font.add", json!({"data": base64_encode(b"not a font at all")})).unwrap_err();
    assert!(not_font.contains("not a font"), "{not_font}");
    let bad = h.state_mut().run("font.add", json!({"data": "@@@"})).unwrap_err();
    assert!(bad.contains("base64"), "{bad}");
    let missing = h.state_mut().run("font.add", json!({})).unwrap_err();
    assert!(missing.contains("missing `data`"), "{missing}");
    let too_big = "A".repeat((fonts::MAX_ADDED_FILE + 8) / 3 * 4);
    let too_big = h.state_mut().run("font.add", json!({"data": too_big})).unwrap_err();
    assert!(too_big.contains("larger than"), "{too_big}");
    assert_eq!(fonts::added_generation(), generation, "a refused file is not kept");
}
