use super::{SheetApp, theme};
use crate::fonts;

impl SheetApp {
    /// Test-harness setup: mark a deterministic font set (see
    /// [`theme::pinned_font_definitions`]) as already installed, so the first `logic` pass neither
    /// starts the host font scan nor replaces the harness's pinned set. Production uses
    /// [`Self::setup_context`] instead.
    #[doc(hidden)]
    pub fn use_prepared_fonts(&mut self) {
        self.prepared_fonts = true;
        self.fonts_ready = true;
        self.fonts_set = true;
        self.fonts_rebind = false;
        self.fonts_han = Some(theme::HanOrder::of(self.ui.language));
        self.fonts_bound = self.used_fonts();
        self.fonts_seen = self.fonts_key();
    }

    /// The font names the open workbooks use: every style's font plus the theme's heading/body
    /// fonts. These are the families [`fonts`] binds an installed file for. Every workbook's, as
    /// each may be showing in a window of its own.
    pub fn used_fonts(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for d in self.session.documents() {
            names.extend(d.wb.styles.iter().map(|(_, s)| s.font.name.clone()));
            names.push(d.wb.theme.major_font.clone());
            names.push(d.wb.theme.minor_font.clone());
            // Text boxes and shapes name their own typefaces.
            names.extend(d.wb.sheets.iter().flat_map(|sh| sh.shapes.iter().filter_map(|s| s.format.font.clone())));
        }
        names
    }

    /// The open workbooks' (uid, revision), folded: [`Self::used_fonts`] changes only with it.
    fn fonts_key(&self) -> (u64, u64) {
        self.session
            .documents()
            .iter()
            .fold((0u64, 0u64), |(uid, rev), d| (uid.wrapping_mul(31).wrapping_add(d.uid), rev.wrapping_mul(31).wrapping_add(d.revision)))
    }

    /// Installs the text-width closure once, on the first frame.
    pub(super) fn ensure_text_width(&mut self, ctx: &egui::Context) {
        if self.session.text_width.is_some() {
            return;
        }
        let ctx = ctx.clone();
        self.session.text_width = Some(std::sync::Arc::new(move |text, font, exact| theme::cell_text_width(&ctx, text, font, exact)));
    }

    /// Applies the font gates: a language Han-order change, startup/prepared fonts and workbook
    /// font binding. Every branch keeps its one-frame paint gate.
    pub(super) fn apply_font_state(&mut self, ctx: &egui::Context) {
        // A pending synthetic step owns the next real `ui` pass: `ui` returns at the font gate
        // before it handles input, so a font change requested now would consume the step without
        // applying it. Delay the change one frame; the hidden-paint request keeps it coming.
        let defer_font_change = self.synthetic_pending || !self.synthetic.is_empty();
        if !defer_font_change && self.fonts_han.is_some_and(|han| han != theme::HanOrder::of(self.ui.language)) {
            self.fonts_ready = false;
            self.fonts_set = false;
        }
        if !self.fonts_ready {
            self.start_font_setup(ctx);
        } else if self.fonts_rebind {
            // A font-set change made last frame is active now.
            self.fonts_rebind = false;
        } else if !defer_font_change && !self.prepared_fonts && fonts::scan_done() {
            self.rebind_workbook_fonts(ctx);
        }
    }

    /// Applies the pending font set. New fonts apply from the next frame on: paint nothing until
    /// then. Startup always proceeds: without it a queued step would be re-offered forever.
    fn start_font_setup(&mut self, ctx: &egui::Context) {
        if self.fonts_set {
            self.fonts_ready = true;
            return;
        }
        if self.prepared_fonts {
            ctx.set_fonts(theme::pinned_font_definitions());
            theme::apply(ctx, self.ui.dark);
        } else {
            SheetApp::setup_context_for_language(ctx, self.ui.dark, self.ui.language);
        }
        self.fonts_set = true;
        // The rebuilt set has no workbook fonts yet: bind them again once it is active.
        self.fonts_bound.clear();
        self.fonts_seen = (0, 0);
        self.fonts_han = Some(theme::HanOrder::of(self.ui.language));
        ctx.request_repaint();
    }

    /// Binds the workbook's installed fonts once the background scan has finished. Adding fonts
    /// re-applies the font set, which egui activates at the next pass, so skip this frame's paint
    /// (same one-frame cost as startup). Only when an open workbook changed.
    fn rebind_workbook_fonts(&mut self, ctx: &egui::Context) {
        let seen = self.fonts_key();
        if seen == self.fonts_seen {
            return;
        }
        let used = self.used_fonts();
        self.fonts_seen = seen;
        if used == self.fonts_bound {
            return;
        }
        ctx.set_fonts(theme::font_definitions_with(self.ui.language, &used));
        self.fonts_bound = used;
        self.fonts_rebind = true;
        ctx.request_repaint();
    }
}
