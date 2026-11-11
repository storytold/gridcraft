use super::{CellRef, SheetApp, SheetView};

impl SheetApp {
    /// The active workbook and sheet: what drafts, text boxes and picture targets belong to.
    pub fn view_key(&self) -> Option<(u64, usize)> {
        self.session.active().map(|d| (d.uid, d.wb.active_sheet()))
    }

    /// The active window and sheet: each window scrolls on its own.
    fn scroll_key(&self) -> Option<(u64, u64)> {
        Some((self.session.active_window()?, self.session.active()?.wb.active()?.uid.get()))
    }

    pub fn view(&self) -> SheetView {
        self.scroll_key().and_then(|k| self.views.get(&k).copied()).unwrap_or_else(|| self.saved_view())
    }

    pub fn view_mut(&mut self) -> Option<&mut SheetView> {
        let k = self.scroll_key()?;
        // Only for a sheet this window hasn't scrolled yet: the saved position walks every sized
        // or hidden row above it, and the grid asks for its view every frame.
        if !self.views.contains_key(&k) {
            let saved = self.saved_view();
            self.views.insert(k, saved);
        }
        self.views.get_mut(&k)
    }

    /// The view a sheet opens with: scrolled to the file's saved first visible cell (`topLeftCell`).
    fn saved_view(&self) -> SheetView {
        let Some(sh) = self.session.active().and_then(|d| d.wb.active()) else { return SheetView::default() };
        if sh.view != gridcraft_engine::model::SheetViewMode::Normal {
            return SheetView::default();
        }
        let (fr, fc) = sh.freeze.unwrap_or((0, 0));
        let top = sh.view_top_left;
        let y = sh.row_top(top.row.max(fr)) - sh.row_top(fr);
        let x = sh.col_left(top.col.max(fc)) - sh.col_left(fc);
        SheetView { scroll: egui::vec2(x.max(0.0) as f32, y.max(0.0) as f32) }
    }

    /// Hands the active document's scroll positions (first visible cell per sheet) and active
    /// cell to the engine so a save writes them (`sheetView topLeftCell`). Normal view only:
    /// Page Layout view scrolls over pages, not cells.
    pub fn store_view_state(&mut self) {
        let window = self.session.active_window();
        let Some(d) = self.session.active_mut() else { return };
        let mut tops = Vec::new();
        for (si, sh) in d.wb.sheets.iter().enumerate() {
            let Some(v) =
                window.and_then(|w| self.views.get(&(w, sh.uid.get()))).filter(|_| sh.view == gridcraft_engine::model::SheetViewMode::Normal)
            else {
                continue;
            };
            let (fr, fc) = sh.freeze.unwrap_or((0, 0));
            // A hair past the edge so a row that starts exactly at the scroll offset is the one found.
            let row = sh.row_at(sh.row_top(fr) + v.scroll.y.max(0.0) as f64 + 0.01);
            let col = sh.col_at(sh.col_left(fc) + v.scroll.x.max(0.0) as f64 + 0.01);
            tops.push((si, CellRef::new(row, col)));
        }
        d.store_view(&tops);
    }

    /// Drops the scroll positions of windows that have closed: window ids are never reused.
    pub(super) fn prune_views(&mut self) {
        let session = &self.session;
        self.views.retain(|&(w, _), _| session.window(w).is_some());
    }
}
