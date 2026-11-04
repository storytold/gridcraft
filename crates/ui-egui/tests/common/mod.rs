//! The shared strict test harness: bundled faces plus the pinned craft-fonts input, never the
//! host's installed fonts.
//!
//! `egui_kittest` keeps `MissingGlyphPolicy::Panic` and accessibility checks on. The production
//! font set probes system fonts for scripts a workstation may lack; the suite must not, or a
//! minimal CI image would fail for a reason unrelated to the change under test. `CRAFT_FONTS_DIR`
//! is the pinned input (never a font copy in this repository); without it only egui's bundled
//! faces are installed and the non-Latin language cases are skipped explicitly in the one test
//! that renders every catalog.

use egui_kittest::HarnessBuilder;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};

pub type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A kittest builder with the deterministic font set and GridCraft's visuals installed.
pub fn builder<State>() -> HarnessBuilder<State> {
    egui_kittest::Harness::builder().with_context_setup(|ctx| {
        ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
        gridcraft_ui_egui::theme::apply(ctx, false);
    })
}

/// A test app whose first `logic` pass keeps the harness's pinned font set.
pub fn app(session: Session) -> SheetApp {
    prepare(SheetApp::new(session, Services::default()))
}

/// Mark `app` as already using the harness's pinned font set.
pub fn prepare(mut app: SheetApp) -> SheetApp {
    app.use_prepared_fonts();
    app
}

/// Drive `app`'s `logic` then `ui` each frame in a harness built with `builder`.
///
/// Tests that need a non-standard size, step timing, OS or pixel ratio configure the builder
/// here so those details stay explicit; the shared font and lifecycle setup lives in `builder`
/// and `app`.
pub fn harness_with(builder: HarnessBuilder<SheetApp>, app: SheetApp, steps: usize) -> Harness {
    let mut h = builder.build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(steps);
    h
}

/// A 1200×800 harness that reports `os` to egui, for platform-specific input.
pub fn os_harness(session: Session, os: egui::os::OperatingSystem) -> Harness {
    harness_with(builder().with_os(os).with_size(egui::vec2(1200.0, 800.0)), app(session), 4)
}
