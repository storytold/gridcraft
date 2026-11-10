//! Real-handwriting evidence for the Action Pen, run by hand (never in CI: it needs the 251 MB
//! model and a data file, neither of which is committed).
//!
//! ```text
//! GRIDCRAFT_TROCR_MODEL=<models root> \
//! GRIDCRAFT_TROCR_LINES=<data dir> \
//!   cargo test --release -p gridcraft-ink-rten --test real_model -- --ignored --nocapture
//! ```
//!
//! `<models root>` holds `trocr-small-handwritten/` (downloaded or imported by the app).
//! `<data dir>` holds `inputs.bin` (little-endian f32, `3 * 384 * 384` per line, CHW, each value
//! `(pixel / 255 - 0.5) / 0.5`) and `truth.txt` (one ground-truth line per input). Both are made
//! from the MIT validation split of `Teklia/IAM-line` by the script in `docs/action-pen.md`.

use std::path::PathBuf;

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// Levenshtein distance over characters, divided by the truth's length: the CER this test reports.
fn cer(recognised: &str, truth: &str) -> f64 {
    let a: Vec<char> = recognised.chars().collect();
    let b: Vec<char> = truth.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            // Remove a's character (above), insert b's (left), or substitute (diagonal).
            cur.push((prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost));
        }
        prev = cur;
    }
    prev.last().copied().unwrap_or(0) as f64 / b.len().max(1) as f64
}

#[test]
#[ignore = "needs the downloaded model and the IAM line data (see docs/action-pen.md)"]
fn recognises_iam_validation_lines() {
    let model_root = env_path("GRIDCRAFT_TROCR_MODEL").expect("set GRIDCRAFT_TROCR_MODEL (models root)");
    let data = env_path("GRIDCRAFT_TROCR_LINES").expect("set GRIDCRAFT_TROCR_LINES (data dir)");
    let recognizer = gridcraft_ink_rten::RtenRecognizer::load(&model_root).expect("the model loads and verifies");
    let raw = std::fs::read(data.join("inputs.bin")).expect("inputs.bin");
    let truth = std::fs::read_to_string(data.join("truth.txt")).expect("truth.txt");
    let mut values = Vec::with_capacity(raw.len() / 4);
    for chunk in raw.as_chunks::<4>().0 {
        values.push(f32::from_le_bytes(*chunk));
    }
    let per_line = 3 * 384 * 384;
    let lines = values.len() / per_line;
    let truth: Vec<&str> = truth.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines, truth.len(), "inputs.bin and truth.txt disagree on the number of lines");
    assert!(lines > 0, "no lines to check");

    // Native-only test (never built for wasm), so the wall clock is available here.
    #[allow(clippy::disallowed_methods)]
    let start = std::time::Instant::now();
    let mut exact = 0usize;
    let mut cer_sum = 0.0;
    for (i, truth) in truth.iter().enumerate() {
        let input = &values[i * per_line..(i + 1) * per_line];
        let recognised = recognizer.recognize_input(input).expect("recognised");
        let (a, b) = (recognised.to_lowercase(), truth.trim().to_lowercase());
        if a == b {
            exact += 1;
        }
        cer_sum += cer(&a, &b);
        println!("{i:3} truth {b:?} -> {a:?}");
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0 / lines as f64;
    let cer = cer_sum / lines as f64;
    println!("{lines} lines, {exact}/{lines} exact, CER {cer:.4}, {ms:.0} ms/line");
    assert!(cer <= 0.06, "CER {cer:.4} is worse than the measured 0.0297: the runtime or tokenizer regressed");
}
