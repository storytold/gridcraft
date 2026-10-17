//! A host adds fonts while the app runs (`app.font.add`): the web build has no system fonts, so
//! this is how CJK text gets glyphs there. The added fonts are process-wide, so this file keeps
//! its own process, and its tests take turns: a font one test adds makes every running app
//! rebuild its fonts at its next frame, which would land in the middle of another test.

use gridcraft_engine::Session;
use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::{Value as Json, json};

static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Held for a whole test, so no other test here adds a font meanwhile.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Services::default()),
    );
    h.run_steps(4);
    h
}

/// Ubuntu Light, base64-encoded, with table `from` renamed to `to` so nothing finds it.
fn ubuntu_renaming(from: &[u8; 4], to: &[u8; 4]) -> String {
    let mut font = epaint_default_fonts::UBUNTU_LIGHT.to_vec();
    let tables = u16::from_be_bytes([font[4], font[5]]) as usize;
    for at in (0..tables).map(|i| 12 + 16 * i) {
        if &font[at..at + 4] == from {
            font[at..at + 4].copy_from_slice(to);
        }
    }
    gridcraft_engine::io::base64_encode(&font)
}

fn add(h: &mut egui_kittest::Harness<'static, SheetApp>, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match control::handle(h.state_mut(), &ctx, "engine.execute", &json!({"command": "app.font.add", "params": params})) {
        Outcome::Done(reply) => reply,
        Outcome::Screenshot { .. } => panic!("a command replies at once"),
    }
}

#[test]
fn an_added_font_joins_every_family_of_the_running_app() {
    let _turn = turn();
    let mut h = harness();
    let ubuntu = gridcraft_engine::io::base64_encode(epaint_default_fonts::UBUNTU_LIGHT);
    let reply = add(&mut h, json!({"name": "Ubuntu Light", "base64": ubuntu}));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["result"], json!({"name": "Ubuntu Light", "added": true, "covers": {"hangul": false, "han": false, "kana": false}}));
    h.run_steps(3);
    // The font set the app draws with now ends every family with it.
    let fonts = h.ctx.fonts(|f| f.definitions().clone());
    assert!(fonts.font_data.contains_key("extra:Ubuntu Light"));
    for (family, chain) in &fonts.families {
        assert_eq!(chain.last().map(String::as_str), Some("extra:Ubuntu Light"), "{family:?}: {chain:?}");
    }
    // Adding the same name again changes nothing, and says so.
    let again = add(&mut h, json!({"name": "Ubuntu Light", "base64": gridcraft_engine::io::base64_encode(b"not a font")}));
    assert_eq!(again["result"], json!({"name": "Ubuntu Light", "added": false, "covers": {"hangul": false, "han": false, "kana": false}}), "{again}");
    // The command is listed with its parameters.
    let ctx = h.ctx.clone();
    let Outcome::Done(listed) = control::handle(h.state_mut(), &ctx, "engine.commands", &json!({"search": "font.add"})) else {
        panic!("command discovery replies at once");
    };
    assert!(
        listed["result"]
            .as_array()
            .is_some_and(|c| c.iter().any(|c| c["id"] == "app.font.add" && c["params"].as_str().is_some_and(|p| p.contains("base64"))))
    );
}

#[test]
fn a_bad_font_is_refused_and_the_app_keeps_drawing() {
    let _turn = turn();
    let mut h = harness();
    let ubuntu = gridcraft_engine::io::base64_encode(epaint_default_fonts::UBUNTU_LIGHT);
    for params in [
        json!({}),
        json!({"name": "No Bytes"}),
        json!({"name": "Not Base64", "base64": "!!"}),
        json!({"name": "Garbage", "base64": gridcraft_engine::io::base64_encode(&[0xAB; 64])}),
        json!({"name": "Bad Index", "base64": ubuntu, "index": -1}),
        json!({"name": "Huge Index", "base64": ubuntu, "index": 4_294_967_296_u64}),
        json!({"name": "Missing Face", "base64": ubuntu, "index": 3}),
        json!({"name": "", "base64": ubuntu}),
        // Readable, but nothing egui could draw with: no outlines, no units per em.
        json!({"name": "No Outlines", "base64": ubuntu_renaming(b"glyf", b"gly_")}),
        json!({"name": "No Head", "base64": ubuntu_renaming(b"head", b"hea_")}),
    ] {
        let reply = add(&mut h, params.clone());
        assert_eq!(reply["ok"], false, "{params}: {reply}");
    }
    h.run_steps(3);
    let fonts = h.ctx.fonts(|f| f.definitions().clone());
    for name in ["No Bytes", "Garbage", "Bad Index", "Huge Index", "Missing Face", "No Outlines", "No Head"] {
        assert!(!fonts.font_data.contains_key(&format!("extra:{name}")), "{name} was refused");
    }
    assert!(h.state().fonts_ready, "the app still draws");
}
