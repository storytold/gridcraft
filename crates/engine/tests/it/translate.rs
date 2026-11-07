//! Review › Language › Translate: the pipeline through the engine, undo, XLSX round-trip and the
//! privacy/failure rules. The deterministic fixed backend stands in for a downloaded model.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::translate::{Accelerator, Backend, BackendError, BatchRequest, Cancel, FixedBackend, LangTag};
use serde_json::{Value as Json, json};

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.translate_from = "en".into();
    s.translate_to = "es".into();
    s.translate_backend = Some(Arc::new(FixedBackend::new()));
    s
}

fn run(s: &mut Session, id: &str, params: Json) -> Json {
    s.execute(id, params).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn text(s: &Session, cell: &str) -> String {
    let at = CellRef::parse(cell).expect("cell");
    match s.doc().expect("doc").wb.active().expect("sheet").value(at) {
        Value::Text(t) => t.to_string(),
        other => panic!("{cell} is {other:?}"),
    }
}

fn wait_terminal(s: &mut Session, job: u64) -> Json {
    for _ in 0..400 {
        let status = run(s, "translate.status", json!({"job": job}));
        if matches!(status["state"].as_str(), Some("done" | "cancelled" | "failed")) {
            return status;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("job {job} did not finish");
}

struct FormulaBackend;
impl Backend for FormulaBackend {
    fn model_id(&self) -> &str {
        "formula-test"
    }
    fn accelerator(&self) -> Accelerator {
        Accelerator::Cpu
    }
    fn supports(&self, from: &LangTag, to: &LangTag) -> bool {
        from.language() == "en" && to.language() == "es"
    }
    fn translate(&self, req: &BatchRequest<'_>, _: &Cancel) -> Result<Vec<String>, BackendError> {
        Ok(req.texts.iter().map(|_| "=1+1".to_string()).collect())
    }
}

struct FailingBackend;
impl Backend for FailingBackend {
    fn model_id(&self) -> &str {
        "failing-test"
    }
    fn supports(&self, from: &LangTag, to: &LangTag) -> bool {
        from.language() == "en" && to.language() == "es"
    }
    fn translate(&self, _: &BatchRequest<'_>, _: &Cancel) -> Result<Vec<String>, BackendError> {
        Err(BackendError::Internal("model exploded".into()))
    }
}

struct SlowBackend;
impl Backend for SlowBackend {
    fn model_id(&self) -> &str {
        "slow-test"
    }
    fn supports(&self, from: &LangTag, to: &LangTag) -> bool {
        from.language() == "en" && to.language() == "es"
    }
    fn translate(&self, req: &BatchRequest<'_>, cancel: &Cancel) -> Result<Vec<String>, BackendError> {
        for _ in req.texts {
            std::thread::sleep(Duration::from_millis(200));
            cancel.check()?;
        }
        Ok(req.texts.iter().map(|_| "Hola".to_string()).collect())
    }
}

#[test]
fn run_then_insert_writes_text_and_one_undo_restores_every_cell() {
    let mut s = session();
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello, world!"}));
    run(&mut s, "cell.set", json!({"cell": "A2", "input": "The total is 42 dollars."}));
    run(&mut s, "cell.set", json!({"cell": "A3", "input": "42"}));
    run(&mut s, "cell.set", json!({"cell": "B1", "input": "=LEN(A1)"}));
    let report = run(&mut s, "translate.run", json!({"range": "A1:B4", "from": "en", "to": "es"}));
    assert_eq!(report["stats"]["translated"], 2, "{report}");
    let job = report["job"].as_u64().expect("job");
    let inserted = run(&mut s, "translate.insert", json!({"job": job, "mode": "replace"}));
    assert_eq!(inserted["cells"], 2, "{inserted}");
    assert_eq!(text(&s, "A1"), "Hola, mundo!");
    let a2 = text(&s, "A2");
    assert!(a2.contains("42") && a2.contains("dólares"), "{a2}");
    // Numbers and formula cells were never candidates and are unchanged.
    assert_eq!(s.doc().expect("doc").wb.active().expect("sheet").value(CellRef::parse("A3").expect("a3")), Value::Number(42.0));
    assert!(s.doc().expect("doc").wb.active().expect("sheet").cell(CellRef::parse("B1").expect("b1")).is_some_and(|c| c.formula.is_some()));
    // One undo step restores the exact prior values.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(text(&s, "A1"), "Hello, world!");
    assert_eq!(text(&s, "A2"), "The total is 42 dollars.");
    assert_eq!(inserted["skipped"], json!([]));
}

#[test]
fn a_translation_that_looks_like_a_formula_is_written_as_text() {
    let mut s = session();
    s.translate_backend = Some(Arc::new(FormulaBackend));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello"}));
    let report = run(&mut s, "translate.run", json!({"range": "A1", "from": "en", "to": "es"}));
    let job = report["job"].as_u64().expect("job");
    run(&mut s, "translate.insert", json!({"job": job}));
    let cell = s.doc().expect("doc").wb.active().expect("sheet").cell(CellRef::parse("A1").expect("a1")).cloned().expect("cell");
    assert!(cell.formula.is_none());
    assert_eq!(cell.value, Value::Text("=1+1".into()));
}

#[test]
fn insert_leaves_formula_cells_alone_and_reports_the_reason() {
    let mut s = session();
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello"}));
    run(&mut s, "cell.set", json!({"cell": "B1", "input": "=1+1"}));
    let report = run(&mut s, "translate.run", json!({"range": "A1:B1", "from": "en", "to": "es"}));
    assert_eq!(report["stats"]["skipped"], 1, "{report}");
    let job = report["job"].as_u64().expect("job");
    let values = run(&mut s, "translate.insert", json!({"values": [{"cell": "B1", "text": "Hola"}], "mode": "replace"}));
    assert_eq!(values["cells"], 0);
    assert_eq!(values["skipped"][0]["reason"], "formula");
    assert!(s.doc().expect("doc").wb.active().expect("sheet").cell(CellRef::parse("B1").expect("b1")).is_some_and(|c| c.formula.is_some()));
    run(&mut s, "translate.insert", json!({"job": job}));
}

#[test]
fn empty_only_insert_keeps_non_empty_cells() {
    let mut s = session();
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Keep me"}));
    run(&mut s, "cell.set", json!({"cell": "B1", "input": ""}));
    // A-heading translation into an empty cell, and an attempt to overwrite a filled one.
    let inserted =
        run(&mut s, "translate.insert", json!({"values": [{"cell": "B1", "text": "Hola"}, {"cell": "A1", "text": "Hola"}], "mode": "emptyOnly"}));
    assert_eq!(inserted["cells"], 1, "{inserted}");
    assert_eq!(text(&s, "B1"), "Hola");
    assert_eq!(text(&s, "A1"), "Keep me");
    assert_eq!(inserted["skipped"][0]["reason"], "nonEmpty");
}

#[test]
fn cancelling_a_job_writes_nothing() {
    let mut s = session();
    s.translate_backend = Some(Arc::new(SlowBackend));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello"}));
    let before = s.doc().expect("doc").wb.clone();
    let started = run(&mut s, "translate.start", json!({"range": "A1", "from": "en", "to": "es"}));
    let job = started["job"].as_u64().expect("job");
    let cancelled = run(&mut s, "translate.cancel", json!({"job": job}));
    assert_eq!(cancelled["cancelled"], true);
    let status = wait_terminal(&mut s, job);
    assert_eq!(status["state"], "cancelled");
    assert!(s.execute("translate.insert", json!({"job": job})).is_err());
    assert!(Arc::ptr_eq(&before, &s.doc().expect("doc").wb));
    assert_eq!(text(&s, "A1"), "Hello");
}

#[test]
fn a_timeout_returns_an_error_and_writes_nothing() {
    let mut s = session();
    s.translate_backend = Some(Arc::new(SlowBackend));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello"}));
    let before = s.doc().expect("doc").wb.clone();
    let err = s.execute("translate.run", json!({"range": "A1", "from": "en", "to": "es", "timeoutMs": 20})).unwrap_err().to_string();
    assert!(err.contains("timed out") && err.contains("nothing was written"), "{err}");
    assert!(Arc::ptr_eq(&before, &s.doc().expect("doc").wb));
    assert_eq!(text(&s, "A1"), "Hello");
}

#[test]
fn a_backend_error_writes_nothing() {
    let mut s = session();
    s.translate_backend = Some(Arc::new(FailingBackend));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello"}));
    let before = s.doc().expect("doc").wb.clone();
    let err = s.execute("translate.run", json!({"range": "A1", "from": "en", "to": "es"})).unwrap_err().to_string();
    assert!(err.contains("nothing was written"), "{err}");
    assert!(Arc::ptr_eq(&before, &s.doc().expect("doc").wb));
}

#[test]
fn translated_strings_and_adjacent_formulas_survive_an_xlsx_round_trip_with_no_new_parts() {
    let mut s = session();
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello, world!"}));
    run(&mut s, "cell.set", json!({"cell": "A2", "input": "The total is 42 dollars."}));
    run(&mut s, "cell.set", json!({"cell": "B1", "input": "=UPPER(A1)"}));
    let before = run(&mut s, "file.saveBytes", json!({"format": "xlsx"}));
    let before_parts = package_parts(&before);
    let report = run(&mut s, "translate.run", json!({"range": "A1:A2", "from": "en", "to": "es"}));
    let job = report["job"].as_u64().expect("job");
    run(&mut s, "translate.insert", json!({"job": job}));
    let after = run(&mut s, "file.saveBytes", json!({"format": "xlsx"}));
    assert_eq!(package_parts(&after), before_parts, "translation must not add package parts");
    let mut reopened = Session::new();
    run(&mut reopened, "file.open", json!({"name": "translated.xlsx", "base64": after["base64"]}));
    assert_eq!(text(&reopened, "A1"), "Hola, mundo!");
    assert!(text(&reopened, "A2").contains("42"));
    let formula = reopened.doc().expect("doc").wb.active().expect("sheet").cell(CellRef::parse("B1").expect("b1")).cloned().expect("b1");
    assert_eq!(formula.formula.as_ref().map(|f| f.text.as_str()), Some("UPPER(A1)"));
}

fn package_parts(saved: &Json) -> Vec<String> {
    let bytes = gridcraft_engine::io::base64_decode(saved["base64"].as_str().unwrap_or_default()).expect("base64");
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader).expect("zip");
    let mut names: Vec<String> = (0..zip.len()).filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string())).collect();
    names.sort();
    names
}

struct CountingFetcher(Arc<AtomicUsize>);
impl gridcraft_engine::translate::ModelFetcher for CountingFetcher {
    fn fetch(&self, _url: &str) -> Result<Vec<u8>, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err("the network is blocked".into())
    }
}

#[test]
fn inference_never_touches_the_download_transport() {
    let mut s = session();
    let calls = Arc::new(AtomicUsize::new(0));
    s.translate_fetcher = Some(Arc::new(CountingFetcher(calls.clone())));
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello, world!"}));
    let report = run(&mut s, "translate.run", json!({"range": "A1", "from": "en", "to": "es"}));
    assert_eq!(report["stats"]["translated"], 1, "{report}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let job = report["job"].as_u64().expect("job");
    let copied = run(&mut s, "translate.copy", json!({"job": job}));
    assert_eq!(copied["chars"], "Hola, mundo!".chars().count());
    assert_eq!(s.translate_clipboard.as_deref(), Some("Hola, mundo!"));
}

#[test]
fn model_download_requires_consent_and_verifies_every_file() {
    let dir = temp_dir("download");
    let mut s = session();
    s.translate_models_dir = dir.display().to_string();
    let err = s.execute("translate.modelDownload", json!({"id": "opus-mt-en-es"})).unwrap_err().to_string();
    assert!(err.contains("consent"), "{err}");
    // A fetcher that lies about the bytes: refused before anything is written.
    s.translate_fetcher = Some(Arc::new(BadFetcher));
    let err = s.execute("translate.modelDownload", json!({"id": "opus-mt-en-es", "consent": true})).unwrap_err().to_string();
    assert!(err.contains("bytes") || err.contains("SHA-256"), "{err}");
    assert!(!dir.join("opus-mt-en-es").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

struct BadFetcher;
impl gridcraft_engine::translate::ModelFetcher for BadFetcher {
    fn fetch(&self, _url: &str) -> Result<Vec<u8>, String> {
        Ok(vec![0u8; 8])
    }
}

#[test]
fn no_download_transport_and_no_consent_are_actionable() {
    let dir = temp_dir("transport");
    let mut s = session();
    s.translate_models_dir = dir.display().to_string();
    let err = s.execute("translate.modelDownload", json!({"id": "opus-mt-en-es", "consent": true})).unwrap_err().to_string();
    assert!(err.contains("no download transport"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn import_of_truncated_files_is_an_error_not_a_panic() {
    let dir = temp_dir("import");
    let source = dir.join("opus-mt-en-es");
    let _ = std::fs::create_dir_all(source.join("onnx"));
    for file in ["onnx/encoder_model_quantized.onnx", "onnx/decoder_model_quantized.onnx", "source.spm", "target.spm", "vocab.json", "config.json"] {
        let _ = std::fs::write(source.join(file), b"");
    }
    let mut s = session();
    s.translate_models_dir = dir.display().to_string();
    let err = s.execute("translate.modelImport", json!({"path": source.display().to_string(), "consent": true})).unwrap_err().to_string();
    assert!(err.contains("bytes"), "{err}");
    let err = s.execute("translate.modelImport", json!({"path": source.display().to_string()})).unwrap_err().to_string();
    assert!(err.contains("consent"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_and_traversing_model_ids_are_refused() {
    let dir = temp_dir("traversal");
    let mut s = session();
    s.translate_models_dir = dir.display().to_string();
    assert!(s.execute("translate.modelRemove", json!({"id": "../evil"})).is_err());
    assert!(s.execute("translate.modelDownload", json!({"id": "nope", "consent": true})).is_err());
    assert!(s.execute("translate.modelImport", json!({"path": dir.display().to_string(), "id": "../evil", "consent": true})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_translate_command_reports_a_missing_model() {
    let mut s = session();
    s.translate_backend = None;
    run(&mut s, "cell.set", json!({"cell": "A1", "input": "Hello"}));
    for (id, params) in
        [("translate.start", json!({"range": "A1", "from": "en", "to": "es"})), ("translate.run", json!({"range": "A1", "from": "en", "to": "es"}))]
    {
        let err = s.execute(id, params).unwrap_err().to_string();
        assert!(err.contains("no translation model"), "{id}: {err}");
    }
    let pane = run(&mut s, "review.translate", json!({"on": true}));
    assert_eq!(pane["ready"], false);
    assert!(pane["hint"].as_str().unwrap_or_default().contains("no translation model"));
    let models = run(&mut s, "translate.models", json!({}));
    assert_eq!(models["active"], Json::Null);
    assert_eq!(models["models"][0]["id"], "opus-mt-en-es");
    assert_eq!(models["models"][0]["license"], "Apache-2.0");
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-engine-translate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    dir
}
