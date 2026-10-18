//! Read-only real-workbook benchmark. No source workbook is written or altered.
//! cargo run --release -p gridcraft-ui-egui --example workbook_file_perf -- <path.xlsx> [repeats]
#![allow(clippy::disallowed_methods)]

use egui::{Rect, pos2, vec2};
use gridcraft_engine::{
    DocState, Session,
    core::{CellRef, RangeRef, col_to_letters},
};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;
use std::{hint::black_box, time::Instant};

fn report(name: &str, mut times: Vec<f64>) {
    times.sort_by(f64::total_cmp);
    let at = |p: usize| times.get(times.len().saturating_mul(p).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0.0);
    println!("{name}: n={} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3}", times.len(), at(50), at(95), at(99));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).ok_or("expected XLSX path")?;
    let repeats = args.get(2).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(5);
    if !(1..=100).contains(&repeats) {
        return Err("repeats must be 1..100".into());
    }
    let mut open = Vec::new();
    let mut read = Vec::new();
    let mut calc = Vec::new();
    let mut session = Session::new();
    for _ in 0..repeats {
        // Exclude closing the previous document from the next open measurement.
        session = Session::new();
        let t = Instant::now();
        let bytes = std::fs::read(path)?;
        let (wb, warnings) = gridcraft_engine::io::open_bytes(path, &bytes)?;
        read.push(t.elapsed().as_secs_f64() * 1000.0);
        let c = Instant::now();
        session.add_document(DocState::new(wb, Some(path.clone()), "Benchmark".into()));
        calc.push(c.elapsed().as_secs_f64() * 1000.0);
        open.push(t.elapsed().as_secs_f64() * 1000.0);
        println!("bytes={} warnings={} formulas={}", bytes.len(), warnings.len(), session.doc()?.calc.last_recalc_cells);
    }
    report("file_read_and_import", read);
    report("initial_graph_and_calc", calc);
    report("open_complete", open);
    for (index, sh) in session.doc()?.wb.sheets.iter().enumerate() {
        println!(
            "sheet={index} stored_cells={} used_range={:?} explicit_rows={} filters={} tables={}",
            sh.cells.len(),
            sh.used_range().map(|r| r.a1()),
            sh.rows.len(),
            sh.autofilter.is_some(),
            sh.tables.len()
        );
    }
    // Largest populated sheet, rather than assuming the saved active sheet is the data.
    let si = session.doc()?.wb.sheets.iter().enumerate().max_by_key(|(_, sh)| sh.cells.len()).map(|(i, _)| i).ok_or("no sheets")?;
    session.doc_mut()?.wb = {
        let mut wb = (*session.doc()?.wb).clone();
        wb.active_sheet = si;
        std::sync::Arc::new(wb)
    };
    let sh = session.doc()?.wb.sheet(si).ok_or("missing sheet")?;
    let used = sh.used_range().ok_or("empty workbook")?;
    let range = sh.autofilter.as_ref().map(|f| f.range).unwrap_or(used);
    let row = range.start.row.saturating_add(1);
    let col = (range.start.col..=range.end.col).find(|col| sh.value(CellRef::new(row, *col)).as_f64().is_some()).ok_or("no numeric filter column")?;
    let data = RangeRef::new(CellRef::new(row, col), CellRef::new(range.end.row, col));
    let mut nums: Vec<f64> = sh.cells.iter_range(data).filter_map(|(_, c)| c.value.as_f64()).filter(|n| n.is_finite()).collect();
    nums.sort_by(f64::total_cmp);
    let thresholds = [nums.get(nums.len() / 10).copied().unwrap_or(0.0), nums.get(nums.len() * 9 / 10).copied().unwrap_or(0.0)];
    println!("benchmark_sheet={si} filter_range={} numeric_column={}", range.a1(), col_to_letters(col));
    session.execute("data.filter", json!({"range":range.a1()}))?;
    // data.filter toggles an existing filter off; restore only when needed.
    if session.doc()?.wb.sheet(si).is_none_or(|s| s.autofilter.is_none()) {
        session.execute("data.filter", json!({"range":range.a1()}))?;
    }
    for (i, threshold) in thresholds.into_iter().enumerate() {
        let mut times = Vec::new();
        for _ in 0..repeats {
            let t = Instant::now();
            let result = session.execute("data.filterBy", json!({"column":col_to_letters(col),"custom":{"op":">","value":threshold}}))?;
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            println!("filter={i} hidden_rows={} recalc_cells={}", result["hiddenRows"], session.doc()?.calc.last_recalc_cells);
        }
        report(&format!("filter_selectivity_{i}"), times);
    }
    session.execute("data.clearFilter", json!({}))?;
    let mut app = SheetApp::new(session, Default::default());
    let ctx = egui::Context::default();
    let mut frames = Vec::new();
    for frame in 0..(repeats * 20 + 5) {
        if let Some(view) = app.view_mut() {
            view.scroll.y = (frame % 80) as f32 * 20000.0;
        }
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        let t = Instant::now();
        black_box(ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        }));
        if frame >= 5 {
            frames.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    report("scroll_cpu_frame", frames);
    let wb = app.session.doc()?.wb.clone();
    let mut times = Vec::new();
    for _ in 0..repeats {
        let t = Instant::now();
        black_box(gridcraft_engine::io::save_bytes(&wb, "benchmark.xlsx")?);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    report("save_to_memory", times);
    Ok(())
}
