//! Deterministic large-workbook CPU benchmark (no GPU/present time).
//! cargo run --release -p gridcraft-ui-egui --example large_workbooks -- A 100000 7
//! A: values, B: 3 formulas/row, C: mixed/styled, D/E: simple 500k/1M,
//! F: sparse, G: dependency chains, H: high/low-selectivity filters.
#![allow(clippy::disallowed_methods)] // Native-only benchmark clocks.

use egui::{Rect, pos2, vec2};
use gridcraft_engine::{
    DocState, Session,
    core::{CellRef, MAX_ROWS, Value},
    model::{Cell, Formula, LineInfo, Style, Workbook},
};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;
use std::{hint::black_box, sync::Arc, time::Instant};

fn measure(mut f: impl FnMut() -> Result<(), Box<dyn std::error::Error>>, n: usize) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let mut times = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        f()?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    Ok(times)
}

fn report(name: &str, times: &[f64]) {
    let at = |p: usize| times.get((times.len().saturating_mul(p).div_ceil(100)).saturating_sub(1)).copied().unwrap_or(0.0);
    println!("{name}: n={} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3}", times.len(), at(50), at(95), at(99));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dataset = args.get(1).map(String::as_str).unwrap_or("A");
    if !["A", "B", "C", "D", "E", "F", "G", "H"].contains(&dataset) {
        return Err("dataset must be A..H".into());
    }
    let default_rows = match dataset {
        "D" => 500_000,
        "E" => 1_000_000,
        _ => 100_000,
    };
    let rows = args.get(2).map(|s| s.parse::<u32>()).transpose()?.unwrap_or(default_rows);
    let repeats = args.get(3).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(7);
    if rows == 0 || rows >= MAX_ROWS || !(1..=100).contains(&repeats) {
        return Err("rows must be 1..MAX_ROWS-1 and repeats 1..100".into());
    }
    println!("dataset={dataset} rows={rows} repeats={repeats} profile=release-if-built-with---release");
    let t = Instant::now();
    let mut wb = Workbook::new();
    let style = Style { num_fmt: gridcraft_engine::model::NumFmt::new("yyyy-mm-dd"), ..Default::default() };
    let date_style = wb.styles.intern(style);
    let sh = wb.sheet_mut(0).ok_or("missing sheet")?;
    for col in 0..12 {
        sh.set_cell(CellRef::new(0, col), Cell::value(format!("Column {col}").into()));
    }
    let cols = if ["D", "E", "F", "G"].contains(&dataset) { 2 } else { 12 };
    for row in 1..=rows {
        if dataset == "F" && row % 1000 != 0 {
            continue;
        }
        for col in 0..cols {
            let mut cell = if (dataset == "B" && col >= 9) || (dataset == "G" && col == 1) {
                let formula = if dataset == "G" && row > 1 { format!("=B{row}+A{}", row + 1) } else { format!("=A{}*2+{}", row + 1, col) };
                Cell::formula(Formula::new(&formula))
            } else if dataset == "C" && col % 3 == 1 {
                Cell::value(format!("Group {}", row % 97).into())
            } else {
                Cell::value(Value::Number(if dataset == "C" && col % 3 == 2 {
                    45000.0 + f64::from(row % 365)
                } else {
                    f64::from(row % 100) + f64::from(col)
                }))
            };
            if dataset == "C" && col % 3 == 2 {
                cell.style = date_style;
            }
            sh.set_cell(CellRef::new(row, col), cell);
        }
        // XLSX commonly stores explicit heights for every populated row.
        if dataset == "C" {
            sh.rows.insert(row, LineInfo { size: Some(20.0 + (row % 3) as f32), ..Default::default() });
        }
    }
    println!("generate_ms={:.3} cells={}", t.elapsed().as_secs_f64() * 1000.0, sh.cells.len());
    report(
        "used_range_2_queries",
        &measure(
            || {
                black_box(wb.active().and_then(|s| s.used_range()));
                black_box(wb.active().and_then(|s| s.used_range()));
                Ok(())
            },
            repeats * 10,
        )?,
    );
    report(
        "geometry",
        &measure(
            || {
                if let Some(sh) = wb.active() {
                    black_box(Geo::new(sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0)), vec2(0.0, 800000.0)));
                }
                Ok(())
            },
            repeats * 10,
        )?,
    );
    let mut geometry_cache = gridcraft_ui_egui::grid::GeometryCache::default();
    report(
        "geometry_cached",
        &measure(
            || {
                if let Some(sh) = wb.sheets.first() {
                    black_box(geometry_cache.geometry(sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0)), vec2(0.0, 800000.0)));
                }
                Ok(())
            },
            repeats * 10,
        )?,
    );
    let t = Instant::now();
    let mut session = Session::new();
    session.add_document(DocState::new(wb, None, "Benchmark".into()));
    println!("initial_graph_and_calc_ms={:.3} formulas={}", t.elapsed().as_secs_f64() * 1000.0, session.doc()?.calc.last_recalc_cells);
    session.execute("data.filter", json!({"range":format!("A1:L{}",rows+1)}))?;
    for threshold in [10, 90] {
        report(
            &format!("filter_gt_{threshold}"),
            &measure(
                || {
                    session.execute("data.filterBy", json!({"column":"A","custom":{"op":">","value":threshold}}))?;
                    println!("filter_recalc_cells={}", session.doc()?.calc.last_recalc_cells);
                    Ok(())
                },
                repeats,
            )?,
        );
    }
    session.execute("data.clearFilter", json!({}))?;
    let mut app = SheetApp::new(session, Default::default());
    let ctx = egui::Context::default();
    let mut frame = 0;
    let mut run_frame = || -> Result<(), Box<dyn std::error::Error>> {
        if let Some(v) = app.view_mut() {
            v.scroll.y = (frame % 80) as f32 * 20000.0;
        }
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0))),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        black_box(ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        }));
        frame += 1;
        Ok(())
    };
    for _ in 0..5 {
        run_frame()?;
    }
    report("scroll_cpu_frame", &measure(run_frame, repeats * 20)?);
    let wb: Arc<Workbook> = app.session.doc()?.wb.clone();
    report(
        "xlsx_save",
        &measure(
            || {
                black_box(gridcraft_engine::io::save_bytes(&wb, "benchmark.xlsx")?);
                Ok(())
            },
            repeats,
        )?,
    );
    let bytes = gridcraft_engine::io::save_bytes(&wb, "benchmark.xlsx")?;
    println!("xlsx_bytes={}", bytes.len());
    report(
        "xlsx_read_without_calc",
        &measure(
            || {
                black_box(gridcraft_engine::io::open_bytes("benchmark.xlsx", &bytes)?);
                Ok(())
            },
            repeats,
        )?,
    );
    report(
        "full_recalc",
        &measure(
            || {
                let mut w = (*wb).clone();
                let mut calc = gridcraft_engine::calc::Calc::new();
                calc.recalc_all(&mut w);
                black_box(calc.last_recalc_cells);
                Ok(())
            },
            repeats,
        )?,
    );
    if dataset == "H" {
        // Isolate the exact previous/current value-list predicate, including conversion
        // and preparation, on the same 100k cells. This is predicate CPU time, not a
        // whole-command benchmark and must be reported separately.
        let values: Vec<String> = (0..100).map(|n| n.to_string()).collect();
        let sh = wb.active().ok_or("missing sheet")?;
        report(
            "value_list_predicates_legacy",
            &measure(
                || {
                    let visible: Vec<bool> = (1..=rows)
                        .map(|row| {
                            let c = CellRef::new(row, 0);
                            let v = sh.value(c);
                            let text = gridcraft_engine::display::cell_text(&wb, sh, c);
                            values.iter().any(|x| x.eq_ignore_ascii_case(&text) || x.eq_ignore_ascii_case(&v.display()))
                        })
                        .collect();
                    if !visible.iter().all(|v| *v) {
                        return Err("legacy membership mismatch".into());
                    }
                    black_box(visible);
                    Ok(())
                },
                repeats,
            )?,
        );
        report(
            "value_list_predicates_prepared",
            &measure(
                || {
                    let set: std::collections::HashSet<String> = values.iter().map(|s| s.to_ascii_lowercase()).collect();
                    let visible: Vec<bool> = (1..=rows)
                        .map(|row| {
                            let c = CellRef::new(row, 0);
                            let v = sh.value_ref(c).unwrap_or(&Value::Empty);
                            set.contains(&v.display().to_ascii_lowercase())
                                || set.contains(&gridcraft_engine::display::cell_text(&wb, sh, c).to_ascii_lowercase())
                        })
                        .collect();
                    if !visible.iter().all(|v| *v) {
                        return Err("prepared membership mismatch".into());
                    }
                    black_box(visible);
                    Ok(())
                },
                repeats,
            )?,
        );
    }
    Ok(())
}
