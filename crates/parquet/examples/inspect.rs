//! Local import timing without printing any cell contents.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: inspect file.parquet [--owned]")?;
    let owned = std::env::args().any(|arg| arg == "--owned");
    let bytes = std::fs::read(path)?;
    // Native-only timing; std clocks trap on wasm (see clippy.toml).
    #[allow(clippy::disallowed_methods)]
    let start = std::time::Instant::now();
    let input_bytes = bytes.len();
    let (wb, report) = if owned { gridcraft_parquet::read_owned(bytes)? } else { gridcraft_parquet::read(&bytes)? };
    if let Some(sheet) = wb.sheet(0) {
        println!(
            "input_bytes={} cells={} used_range={} seconds={:.3} warnings={}",
            input_bytes,
            sheet.cells.len(),
            sheet.used_range().map(|r| r.a1()).unwrap_or_default(),
            start.elapsed().as_secs_f64(),
            report.warnings.len()
        );
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() {}
