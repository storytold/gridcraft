//! End-to-end check against the real Qwen3 model. Ignored by default: it needs the
//! user-downloaded weights (never committed).
//!
//! ```text
//! GRIDCRAFT_TRANSLATE_MODEL=<model root or qwen3-1.7b dir> \
//!   cargo test -p gridcraft-translate-candle --test real_model -- --ignored --nocapture
//! ```

use std::time::Instant;

use gridcraft_translate::{Backend, BatchRequest, Cancel, LangTag};
use gridcraft_translate_candle::CandleBackend;

fn backend() -> CandleBackend {
    let path = std::env::var("GRIDCRAFT_TRANSLATE_MODEL").expect("set GRIDCRAFT_TRANSLATE_MODEL to the model directory");
    let path = std::path::PathBuf::from(path);
    if path.join("Qwen3-1.7B-Q8_0.gguf").is_file() {
        CandleBackend::with_manifest(path, &gridcraft_translate::manifest::QWEN3_1_7B)
    } else {
        CandleBackend::open(&path, "qwen3-1.7b").expect("open model")
    }
}

fn lang(code: &str) -> LangTag {
    LangTag::parse(code).expect("tag")
}

// Native-only helper: this test binary never builds for wasm.
#[allow(clippy::disallowed_methods)]
fn translate(backend: &CandleBackend, from: Option<&LangTag>, to: &str, text: &str) -> String {
    let started = Instant::now();
    let out = backend.translate(&BatchRequest { from, to: &lang(to), texts: &[text.to_string()] }, &Cancel::new()).expect("translate");
    println!("{text:?} -> {:?} in {:?}", out.first().map(String::as_str), started.elapsed());
    out.into_iter().next().unwrap_or_default()
}

#[test]
#[ignore = "needs the user-downloaded qwen3-1.7b model (GRIDCRAFT_TRANSLATE_MODEL)"]
// Native-only integration test: it never builds or runs for wasm, where the clock is unavailable.
#[allow(clippy::disallowed_methods)]
fn translates_the_required_pairs_and_preserves_numbers() {
    let backend = backend();
    let en = lang("en");
    let es = translate(&backend, Some(&en), "es", "The total is 42 dollars.");
    assert!(es.to_lowercase().contains("42"), "{es}");
    assert!(es.to_lowercase().contains("dólares") || es.to_lowercase().contains("dolares"), "{es}");
    let de = translate(&backend, Some(&en), "de", "Good morning, the report is ready for review.");
    assert!(!de.trim().is_empty());
    let ja = translate(&backend, Some(&en), "ja", "The meeting is on Friday.");
    assert!(!ja.trim().is_empty());
    let zh = lang("zh");
    let en_back = translate(&backend, Some(&zh), "en", "会议在星期五。");
    assert!(!en_back.trim().is_empty());
    // Auto-detect reaches the model itself.
    let auto = translate(&backend, None, "es", "The total is 42 dollars.");
    assert!(auto.contains("42"), "{auto}");
}

#[test]
#[ignore = "needs the user-downloaded qwen3-1.7b model (GRIDCRAFT_TRANSLATE_MODEL)"]
fn refuses_a_target_outside_the_manifest() {
    let backend = backend();
    assert!(!backend.supports_target(&lang("fr")));
}
