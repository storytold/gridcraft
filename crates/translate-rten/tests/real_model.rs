//! End-to-end check against the real model. Ignored by default: it needs the user-downloaded
//! weights (never committed).
//!
//! ```text
//! GRIDCRAFT_TRANSLATE_MODEL=<model root or opus-mt-en-es dir> \
//!   cargo test -p gridcraft-translate-rten --test real_model -- --ignored --nocapture
//! ```

use std::time::Instant;

use gridcraft_translate::{Backend, BatchRequest, Cancel, LangTag};
use gridcraft_translate_rten::RtenBackend;

fn backend() -> RtenBackend {
    let path = std::env::var("GRIDCRAFT_TRANSLATE_MODEL").expect("set GRIDCRAFT_TRANSLATE_MODEL to the model directory");
    let path = std::path::PathBuf::from(path);
    if path.join("onnx").is_dir() {
        // A directory whose name is not the manifest id (a developer download) still works when
        // the caller names the model explicitly.
        RtenBackend::with_manifest(path, &gridcraft_translate::manifest::OPUS_MT_EN_ES)
    } else {
        RtenBackend::open(&path, "opus-mt-en-es").expect("open model")
    }
}

fn lang(code: &str) -> LangTag {
    LangTag::parse(code).expect("tag")
}

#[test]
#[ignore = "needs the user-downloaded opus-mt-en-es model (GRIDCRAFT_TRANSLATE_MODEL)"]
// Native-only integration test: it never builds or runs for wasm, where the clock is unavailable.
#[allow(clippy::disallowed_methods)]
fn translates_en_to_es_with_numbers_preserved() {
    let backend = backend();
    let started = Instant::now();
    let texts = vec!["The total is 42 dollars.".to_string(), "Good morning, the report is ready for review.".to_string()];
    let out = backend.translate(&BatchRequest { from: &lang("en"), to: &lang("es"), texts: &texts }, &Cancel::new()).expect("translate");
    println!("{}", out[0]);
    println!("{}", out[1]);
    println!("elapsed: {:?}", started.elapsed());
    assert_eq!(out[0], "El total es de 42 dólares.");
    assert!(out[0].contains("42"));
    assert!(!out[1].trim().is_empty());
}

#[test]
#[ignore = "needs the user-downloaded opus-mt-en-es model (GRIDCRAFT_TRANSLATE_MODEL)"]
fn refuses_the_reverse_pair() {
    let backend = backend();
    assert!(!backend.supports(&lang("es"), &lang("en")));
}
