/// Levels with fewer formulas than this are calculated on the main thread: starting threads
/// would cost more than they save. `GRIDCRAFT_PARALLEL_MIN` overrides it (tests thread tiny
/// levels to shake the worker path).
const PARALLEL_MIN: usize = 512;

/// Most calculation threads used, whatever the processors present: Excel's specifications and
/// limits list "Processor Cores: 64"
/// (<https://support.microsoft.com/en-us/office/excel-specifications-and-limits-1672b34d-7043-467e-8e27-269d656771c3>).
/// A ceiling, not a target: scaling past 64 threads has not been measured.
pub(crate) const MAX_AUTO_THREADS: usize = 64;

/// The processors [`thread_count`] counts out of `present`: at least one, at most
/// [`MAX_AUTO_THREADS`].
pub(crate) fn processors_used(present: usize) -> usize {
    present.clamp(1, MAX_AUTO_THREADS)
}

/// Calculation threads for `calc`: one on wasm, when multi-threaded calculation is off, or when
/// the machine has one processor; else the manual count or, for 0, the automatic count (the
/// processors present), at most the processors present and [`MAX_AUTO_THREADS`].
/// `GRIDCRAFT_CALC_THREADS` in the environment is a testing override (to time or rule out
/// threading without editing the file): it replaces the workbook's settings, multi-threaded
/// calculation off included, still within the processors present and [`MAX_AUTO_THREADS`] and one
/// on wasm; 0 means the automatic count there too.
pub fn thread_count(calc: &gridcraft_model::CalcSettings) -> usize {
    static OVERRIDE: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    if cfg!(target_arch = "wasm32") {
        return 1;
    }
    // A count never exceeds the processors present: the workbook's setting travels with the
    // file, and 64 workers on a 2-core machine would only cost memory.
    // Read once: on Linux each call opens the cgroup files.
    static CORES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let cores = *CORES.get_or_init(|| processors_used(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)));
    if let Some(n) = OVERRIDE.get_or_init(|| std::env::var("GRIDCRAFT_CALC_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok())) {
        return if *n == 0 { cores } else { (*n).min(cores) };
    }
    if !calc.multi_threaded {
        return 1;
    }
    if calc.threads > 0 { (calc.threads as usize).min(cores) } else { cores }
}

/// [`PARALLEL_MIN`], or `GRIDCRAFT_PARALLEL_MIN` from the environment.
pub(super) fn parallel_min() -> usize {
    static MIN: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *MIN.get_or_init(|| std::env::var("GRIDCRAFT_PARALLEL_MIN").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(PARALLEL_MIN))
}
