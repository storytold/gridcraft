//! Rendered charts kept between frames.
//!
//! Resolving a chart's series and laying it out is O(points). Doing it for every visible chart on
//! every frame kept the UI thread busy while scrolling a workbook with a few big charts (#1139,
//! part of #1098). The primitives a chart renders to depend on the workbook, the chart's place in
//! it, the zoom, the screen's pixels-per-point and the fonts text is measured with, so those are
//! what an entry is checked against.
//!
//! **Why "the workbook is unchanged" can be known without a change counter.** A document's
//! workbook is an `Arc<Workbook>` and its content is only reachable for writing through
//! `Arc::make_mut` (or by replacing the `Arc`), because the model has no interior mutability and
//! the code base forbids `unsafe`. While a `Weak` to the allocation exists, `make_mut` never
//! mutates it in place: it clones (other strong owners, e.g. an undo snapshot) or moves the value
//! to a new allocation (the `Weak` is cut loose). So if the document's current `Arc` still points
//! at the allocation the cache holds a `Weak` to, nothing was written since the entries were
//! built, however the write would have been made (command, undo, redo, recalculation, file load,
//! a direct edit from the UI, a future code path). The `Weak` also keeps the allocation from being
//! freed and reused, so a pointer match cannot be a stranger that landed on the same address.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use gridcraft_chart::Prim;
use gridcraft_engine::model::Workbook;

/// Most charts kept, over all documents.
const MAX_ENTRIES: usize = 64;
/// Most memory kept (estimated from the primitives), over all documents.
const MAX_BYTES: usize = 48 << 20;

struct Entry {
    /// Zoom and pixels-per-point the primitives were measured for (`f32::to_bits`).
    z: u32,
    ppp: u32,
    prims: Arc<[Prim]>,
    bytes: usize,
    /// Tick of the last use; the smallest is evicted first.
    used: u64,
}

struct DocCharts {
    /// The workbook the entries were built from.
    wb: Weak<Workbook>,
    /// (sheet index, index in the sheet's `charts`). The workbook is unchanged while the entries
    /// live, so the pair names one chart.
    charts: HashMap<(usize, usize), Entry>,
}

#[derive(Default)]
pub(crate) struct ChartCache {
    docs: HashMap<u64, DocCharts>,
    tick: u64,
    bytes: usize,
    /// Charts built (resolved and rendered) so far; for tests.
    #[cfg(test)]
    pub(crate) builds: usize,
}

fn prim_bytes(p: &Prim) -> usize {
    std::mem::size_of::<Prim>()
        + match p {
            Prim::Line { pts, .. } | Prim::Polygon { pts, .. } => pts.len().saturating_mul(std::mem::size_of::<[f32; 2]>()),
            Prim::Text { text, .. } => text.len(),
            _ => 0,
        }
}

impl ChartCache {
    /// The primitives of chart `index` of `sheet` in document `uid` whose workbook is `wb`, for
    /// zoom and pixels per point `(z, ppp)`. `build` renders the chart when there is no valid entry.
    pub(crate) fn get(
        &mut self,
        uid: u64,
        wb: &Arc<Workbook>,
        sheet: usize,
        index: usize,
        (z, ppp): (f32, f32),
        build: impl FnOnce() -> Vec<Prim>,
    ) -> Arc<[Prim]> {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        let doc = self.docs.entry(uid).or_insert_with(|| DocCharts { wb: Arc::downgrade(wb), charts: HashMap::new() });
        if !std::ptr::eq(doc.wb.as_ptr(), Arc::as_ptr(wb)) {
            // The document changed (or is another one with this uid): nothing built from the old
            // workbook can be trusted.
            let dropped: usize = doc.charts.values().map(|e| e.bytes).sum();
            doc.charts.clear();
            doc.wb = Arc::downgrade(wb);
            self.bytes = self.bytes.saturating_sub(dropped);
        }
        let (zb, pb) = (z.to_bits(), ppp.to_bits());
        if let Some(e) = doc.charts.get_mut(&(sheet, index))
            && e.z == zb
            && e.ppp == pb
        {
            e.used = tick;
            return e.prims.clone();
        }
        #[cfg(test)]
        {
            self.builds += 1;
        }
        let prims: Arc<[Prim]> = build().into();
        let bytes = prims.iter().map(prim_bytes).fold(0usize, usize::saturating_add);
        let Some(doc) = self.docs.get_mut(&uid) else { return prims };
        if let Some(old) = doc.charts.remove(&(sheet, index)) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        // A chart that alone would fill a quarter of the budget is not worth keeping.
        if bytes <= MAX_BYTES / 4 {
            doc.charts.insert((sheet, index), Entry { z: zb, ppp: pb, prims: prims.clone(), bytes, used: tick });
            self.bytes = self.bytes.saturating_add(bytes);
            self.evict();
        }
        prims
    }

    /// Drops least recently used entries until the caps hold.
    fn evict(&mut self) {
        loop {
            let count: usize = self.docs.values().map(|d| d.charts.len()).sum();
            if count <= MAX_ENTRIES && self.bytes <= MAX_BYTES {
                return;
            }
            let oldest = self.docs.iter().flat_map(|(uid, d)| d.charts.iter().map(move |(k, e)| (e.used, *uid, *k))).min();
            let Some((_, uid, key)) = oldest else { return };
            if let Some(e) = self.docs.get_mut(&uid).and_then(|d| d.charts.remove(&key)) {
                self.bytes = self.bytes.saturating_sub(e.bytes);
            }
        }
    }

    /// Forgets the documents that are not in `live` (closed workbooks).
    pub(crate) fn retain_docs(&mut self, live: impl Iterator<Item = u64>) {
        let live: Vec<u64> = live.collect();
        let bytes = &mut self.bytes;
        self.docs.retain(|uid, d| {
            let keep = live.contains(uid);
            if !keep {
                *bytes = bytes.saturating_sub(d.charts.values().map(|e| e.bytes).sum());
            }
            keep
        });
    }

    /// Forgets everything (the fonts text was measured with changed).
    pub(crate) fn clear(&mut self) {
        self.docs.clear();
        self.bytes = 0;
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.docs.values().map(|d| d.charts.len()).sum()
    }

    #[cfg(test)]
    pub(crate) fn docs(&self) -> usize {
        self.docs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SheetApp;
    use crate::chartview::tests::RENDERS;
    use egui::{Event, vec2};
    use gridcraft_chart::ApproxMeasure;
    use gridcraft_engine::Session;
    use serde_json::json;

    type Harness = egui_kittest::Harness<'static, SheetApp>;

    fn wb() -> Arc<Workbook> {
        Arc::new(Workbook::new())
    }

    fn rect_prim(w: f32) -> Vec<Prim> {
        vec![Prim::Rect { x: 0.0, y: 0.0, w, h: 1.0, fill: None, stroke: None, radius: 0.0 }]
    }

    fn line_prim(points: usize) -> Vec<Prim> {
        vec![Prim::Line { pts: vec![[0.0, 0.0]; points], color: [0, 0, 0, 255], width: 1.0, dash: false }]
    }

    #[test]
    fn a_workbook_written_after_caching_is_never_taken_for_the_cached_one() {
        let mut cache = ChartCache::default();
        let mut w = wb();
        let build = || rect_prim(1.0);
        cache.get(1, &w, 0, 0, (1.0, 1.0), build);
        cache.get(1, &w, 0, 0, (1.0, 1.0), build);
        assert_eq!(cache.builds, 1, "same workbook: a hit");
        // Sole owner of the Arc: without the Weak this write would happen in place, at the same address.
        Arc::make_mut(&mut w).active_sheet = 0;
        cache.get(1, &w, 0, 0, (1.0, 1.0), build);
        assert_eq!(cache.builds, 2, "a write through make_mut moved the workbook to a new allocation");
        // Shared with an undo snapshot: make_mut clones, the pointer changes as well.
        let snapshot = w.clone();
        Arc::make_mut(&mut w).active_sheet = 0;
        cache.get(1, &w, 0, 0, (1.0, 1.0), build);
        assert_eq!(cache.builds, 3);
        // Replacing the Arc with an equal workbook (a reload) is a change too.
        w = Arc::new((*snapshot).clone());
        cache.get(1, &w, 0, 0, (1.0, 1.0), build);
        assert_eq!(cache.builds, 4);
        // The old allocation is still held by the cache's Weak while its owner is gone, so a new
        // workbook can't be mistaken for it by landing on the same address.
        drop(snapshot);
        cache.get(1, &w, 0, 0, (1.0, 1.0), build);
        assert_eq!(cache.builds, 4);
    }

    #[test]
    fn zoom_scale_chart_and_document_are_part_of_the_key() {
        let mut cache = ChartCache::default();
        let w = wb();
        let build = || rect_prim(1.0);
        cache.get(1, &w, 0, 0, (1.0, 1.0), build);
        cache.get(1, &w, 0, 0, (1.25, 1.0), build);
        cache.get(1, &w, 0, 0, (1.25, 2.0), build);
        cache.get(1, &w, 0, 1, (1.25, 2.0), build);
        cache.get(1, &w, 1, 0, (1.25, 2.0), build);
        cache.get(2, &w, 0, 0, (1.25, 2.0), build);
        assert_eq!(cache.builds, 6);
        cache.get(1, &w, 0, 1, (1.25, 2.0), build);
        cache.get(2, &w, 0, 0, (1.25, 2.0), build);
        assert_eq!(cache.builds, 6, "each of those is cached");
    }

    #[test]
    fn the_cache_is_capped_in_entries_and_bytes_and_drops_the_least_recently_used() {
        let mut cache = ChartCache::default();
        let w = wb();
        for i in 0..MAX_ENTRIES + 10 {
            cache.get(1, &w, 0, i, (1.0, 1.0), || rect_prim(1.0));
            // Chart 0 is used all the time, so it survives.
            cache.get(1, &w, 0, 0, (1.0, 1.0), || rect_prim(1.0));
        }
        assert_eq!(cache.len(), MAX_ENTRIES);
        let before = cache.builds;
        cache.get(1, &w, 0, 0, (1.0, 1.0), || rect_prim(1.0));
        assert_eq!(cache.builds, before, "the busy chart is still there");
        cache.get(1, &w, 0, 1, (1.0, 1.0), || rect_prim(1.0));
        assert_eq!(cache.builds, before + 1, "the idle one was evicted");

        // Bytes: each of these is ~4 MiB of points; only a handful fit.
        let mut cache = ChartCache::default();
        let points = (4 << 20) / std::mem::size_of::<[f32; 2]>();
        for i in 0..40 {
            cache.get(1, &w, 0, i, (1.0, 1.0), || line_prim(points));
        }
        assert!(cache.bytes <= MAX_BYTES && cache.len() < 40, "{} entries, {} bytes", cache.len(), cache.bytes);
        // One that would fill a quarter of the budget is not kept, and is still returned.
        let mut cache = ChartCache::default();
        let big = cache.get(1, &w, 0, 0, (1.0, 1.0), || line_prim(MAX_BYTES / 4 / std::mem::size_of::<[f32; 2]>() + 1000));
        assert!(!big.is_empty());
        assert_eq!((cache.len(), cache.bytes), (0, 0));
    }

    #[test]
    fn closed_documents_are_forgotten() {
        let mut cache = ChartCache::default();
        let w = wb();
        cache.get(1, &w, 0, 0, (1.0, 1.0), || line_prim(10));
        cache.get(2, &w, 0, 0, (1.0, 1.0), || line_prim(10));
        cache.retain_docs([2].into_iter());
        assert_eq!((cache.docs(), cache.len()), (1, 1));
        cache.retain_docs(std::iter::empty());
        assert_eq!((cache.docs(), cache.len(), cache.bytes), (0, 0, 0));
    }

    /// A workbook with charts of several kinds over numbers, dates and categories.
    fn corpus() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Date", "Sales", "Cost", "Share"]]})).unwrap();
        for i in 0..40u32 {
            let date = format!("2024-{:02}-{:02}", 1 + i / 28, 1 + i % 28);
            s.execute("cell.set", json!({"cell": format!("A{}", i + 2), "input": date})).unwrap();
            s.execute("cell.set", json!({"cell": format!("B{}", i + 2), "input": format!("{}", (i * 37) % 101)})).unwrap();
            s.execute("cell.set", json!({"cell": format!("C{}", i + 2), "input": format!("=B{}/2+{}", i + 2, i)})).unwrap();
            s.execute("cell.set", json!({"cell": format!("D{}", i + 2), "input": format!("{}", (i % 5) + 1)})).unwrap();
        }
        for (i, (kind, range)) in [
            ("column", "A1:C41"),
            ("line", "A1:C41"),
            ("scatter", "B1:C41"),
            ("pie", "A1:B6"),
            ("combo", "A1:C41"),
            ("area", "A1:B41"),
            ("bar", "A1:D41"),
            ("doughnut", "A1:B6"),
        ]
        .into_iter()
        .enumerate()
        {
            s.execute("insert.chart", json!({"range": range, "type": kind, "at": format!("H{}", 2 + i * 16), "width": 320, "height": 220})).unwrap();
        }
        s
    }

    #[test]
    fn cached_output_equals_a_fresh_render_for_a_corpus_of_charts() {
        let s = corpus();
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        assert_eq!(sh.charts.len(), 8);
        let mut cache = ChartCache::default();
        for (i, ch) in sh.charts.iter().enumerate() {
            let fresh = || {
                let data = gridcraft_chart::resolve(&d.wb, d.wb.active_sheet, ch);
                gridcraft_chart::render(ch, &data, ch.anchor.width, ch.anchor.height, &ApproxMeasure)
            };
            let expect = fresh();
            assert!(expect.len() > 3, "{:?} renders something", ch.kind);
            let first = cache.get(d.uid, &d.wb, d.wb.active_sheet, i, (1.0, 1.0), fresh);
            let second = cache.get(d.uid, &d.wb, d.wb.active_sheet, i, (1.0, 1.0), || panic!("must be a hit"));
            assert_eq!(&*first, &expect[..], "{:?}: built entry", ch.kind);
            assert_eq!(&*second, &expect[..], "{:?}: cached entry", ch.kind);
        }
        assert_eq!(cache.builds, 8);
    }

    // --- frames ---

    fn harness(s: Session) -> Harness {
        let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 900.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            SheetApp::new(s, Default::default()),
        );
        h.run_steps(8);
        h
    }

    /// Two charts side by side, both in view, over A1:B12 (B3 is `=B2*2`).
    fn two_charts() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Day", "Sales"]]})).unwrap();
        for i in 2..=12u32 {
            let input = if i == 3 { "=B2*2".to_string() } else { format!("{}", i * 3 % 17) };
            s.execute("cell.set", json!({"cell": format!("A{i}"), "input": format!("2024-03-{i:02}")})).unwrap();
            s.execute("cell.set", json!({"cell": format!("B{i}"), "input": input})).unwrap();
        }
        s.execute("insert.chart", json!({"range": "A1:B12", "type": "column", "at": "D2", "width": 300, "height": 200})).unwrap();
        s.execute("insert.chart", json!({"range": "A1:B12", "type": "scatter", "at": "L2", "width": 300, "height": 200})).unwrap();
        s
    }

    fn builds() -> usize {
        RENDERS.with(|c| c.get())
    }

    /// Runs `change`, lets a few frames pass and returns how many charts were rendered meanwhile.
    fn rendered_after(h: &mut Harness, change: impl FnOnce(&mut Harness)) -> usize {
        let before = builds();
        change(h);
        h.run_steps(4);
        builds() - before
    }

    fn exec(h: &mut Harness, id: &str, params: serde_json::Value) {
        h.state_mut().session.execute(id, params).unwrap();
    }

    #[test]
    fn frames_without_a_change_render_each_chart_once() {
        let mut h = harness(two_charts());
        assert!(builds() >= 2, "the charts were drawn");
        let n = rendered_after(&mut h, |h| h.run_steps(30));
        assert_eq!(n, 0, "30 idle frames render nothing again");
        // Pointer movement and wheel scrolling (view state only) change no workbook.
        let scroll_before = h.state().view().scroll;
        let n = rendered_after(&mut h, |h| {
            for i in 0..10 {
                h.input_mut().events.push(Event::PointerMoved(egui::pos2(300.0 + i as f32 * 7.0, 400.0)));
                h.input_mut().events.push(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -2.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                });
                h.run_steps(1);
            }
        });
        assert_ne!(h.state().view().scroll, scroll_before, "the sheet did scroll");
        assert_eq!(n, 0, "hover and scrolling reuse the rendered charts");
        assert_eq!(h.state().chart_cache.lock().unwrap().len(), 2);
    }

    #[test]
    fn every_kind_of_change_renders_the_charts_again_exactly_once() {
        let mut h = harness(two_charts());
        let cases: Vec<(&str, Box<dyn Fn(&mut Harness)>)> = vec![
            ("cell edit", Box::new(|h| exec(h, "cell.set", json!({"cell": "B5", "input": "99"})))),
            ("recalc through a dependency", Box::new(|h| exec(h, "cell.set", json!({"cell": "B2", "input": "7"})))),
            ("calculate now", Box::new(|h| exec(h, "formulas.calculateNow", json!({})))),
            ("undo", Box::new(|h| exec(h, "edit.undo", json!({})))),
            ("redo", Box::new(|h| exec(h, "edit.redo", json!({})))),
            ("row hide", Box::new(|h| exec(h, "home.hideRows", json!({"rows": "6:7"})))),
            ("chart resize", Box::new(|h| exec(h, "chart.set", json!({"chart": 1, "width": 360, "height": 240})))),
            ("format change", Box::new(|h| exec(h, "home.bold", json!({"range": "A1:B1"})))),
            ("number format", Box::new(|h| exec(h, "home.numberFormat", json!({"range": "B2:B12", "format": "0.00"})))),
            (
                "document theme",
                Box::new(|h| {
                    exec(
                        h,
                        "pageLayout.themeColors",
                        json!({"colors": ["#000000", "#FFFFFF", "#44546A", "#E7E6E6", "#C00000", "#ED7D31", "#A5A5A5", "#FFC000", "#5B9BD5", "#70AD47", "#0563C1", "#954F72"]}),
                    )
                }),
            ),
        ];
        for (what, change) in cases {
            let n = rendered_after(&mut h, |h| change(h));
            assert_eq!(n, 2, "{what}: both visible charts rendered once");
            let n = rendered_after(&mut h, |h| h.run_steps(5));
            assert_eq!(n, 0, "{what}: and then nothing more");
        }
        // Zoom is part of the key too.
        let n = rendered_after(&mut h, |h| exec(h, "view.zoom", json!({"percent": 150})));
        assert_eq!(n, 2, "zoom");
    }

    #[test]
    fn switching_sheets_and_loading_files_render_the_new_charts_once() {
        let mut h = harness(two_charts());
        exec(&mut h, "home.insertSheet", json!({"before": 1}));
        h.run_steps(4);
        // The new sheet has no charts; coming back renders this sheet's two once.
        let n = rendered_after(&mut h, |h| exec(h, "sheet.activate", json!({"sheet": 0})));
        assert_eq!(n, 2, "sheet switch");
        let n = rendered_after(&mut h, |h| h.run_steps(5));
        assert_eq!(n, 0);

        // File load: the same workbook opened from bytes is a new document with its own charts.
        let bytes = h.state_mut().session.execute("file.saveBytes", json!({})).unwrap();
        let b64 = bytes["base64"].as_str().unwrap().to_string();
        let n = rendered_after(&mut h, |h| exec(h, "file.open", json!({"name": "again.xlsx", "base64": b64})));
        assert_eq!(n, 2, "file load");
        assert_eq!(h.state().session.documents().len(), 2);
        let n = rendered_after(&mut h, |h| h.run_steps(5));
        assert_eq!(n, 0);
        assert_eq!(h.state().chart_cache.lock().unwrap().docs(), 2, "both open documents are cached");

        // Back in the first document nothing is rendered: its entries were kept.
        let n = rendered_after(&mut h, |h| h.state_mut().session.set_active(0));
        assert_eq!(n, 0, "the first document's charts were still cached");
        // Closing a document forgets its entries; the other one is shown (and rendered) now.
        let n = rendered_after(&mut h, |h| exec(h, "file.close", json!({"force": true})));
        assert_eq!(h.state().session.documents().len(), 1);
        assert_eq!(h.state().chart_cache.lock().unwrap().docs(), 1);
        assert_eq!(n, 0, "the remaining document's charts were cached too");
    }
}
