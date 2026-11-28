use super::{Json, SheetApp, json, theme};
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
        self.fonts_generation = fonts::added_generation();
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

    /// `font.add`: `{data: base64 TTF/OTF/TTC, name?: file name}` → `{faces: [{family, weight,
    /// italic}]}`. The faces join the installed fonts for the session ([`fonts::add_font`]); the
    /// workbook's cells set in them draw with them from the next frame on.
    pub(super) fn add_font(&mut self, p: &Json) -> Result<Json, String> {
        let Some(data) = p.get("data").and_then(Json::as_str) else {
            return Err("font.add: missing `data` (a base64 TTF, OTF or TTC file)".into());
        };
        // Refused before decoding: base64 is 4 characters for every 3 bytes.
        if data.len() / 4 * 3 > fonts::MAX_ADDED_FILE + 3 {
            return Err(format!("font.add: {}", fonts::AddFontError::TooLarge));
        }
        let bytes = gridcraft_engine::io::base64_decode(data).ok_or("font.add: `data` is not valid base64")?;
        let name = p.get("name").and_then(Json::as_str).unwrap_or("font");
        let faces = fonts::add_font(name, bytes).map_err(|e| format!("font.add: {e}"))?;
        let faces: Vec<Json> = faces.iter().map(|f| json!({"family": f.family, "weight": f.weight, "italic": f.italic})).collect();
        Ok(json!({ "faces": faces }))
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
            self.chart_cache.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
        } else if !defer_font_change && !self.prepared_fonts && fonts::scan_done() {
            self.rebind_workbook_fonts(ctx);
        }
    }

    /// Applies the pending font set. New fonts apply from the next frame on: paint nothing until
    /// then. Startup always proceeds: without it a queued step would be re-offered forever.
    fn start_font_setup(&mut self, ctx: &egui::Context) {
        if self.fonts_set {
            self.fonts_ready = true;
            // The new set is active now: chart text was measured with the old one.
            self.chart_cache.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
            return;
        }
        if self.prepared_fonts {
            craft_ui::fonts::load(ctx, theme::pinned_font_definitions());
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
    /// (same one-frame cost as startup). Only when an open workbook changed, or a font was added
    /// with `font.add`.
    fn rebind_workbook_fonts(&mut self, ctx: &egui::Context) {
        let seen = self.fonts_key();
        let generation = fonts::added_generation();
        if seen == self.fonts_seen && generation == self.fonts_generation {
            return;
        }
        let used = self.used_fonts();
        self.fonts_seen = seen;
        let font_added = generation != self.fonts_generation;
        self.fonts_generation = generation;
        if used == self.fonts_bound && !font_added {
            return;
        }
        craft_ui::fonts::load(ctx, theme::font_definitions_with(self.ui.language, &used));
        self.fonts_bound = used;
        self.fonts_rebind = true;
        ctx.request_repaint();
    }
}
