# Translate (Review › Language › Translate)

GridCraft translates selected cell text **locally, offline, with a user-controlled model**. There
is no cloud service, no account, no telemetry and no network call during inference. The model is
not shipped with GridCraft: the person downloads it after a consent screen (HTTPS, source, size,
licence and SHA-256 shown first) or places the files in the model directory by hand.

This page covers the architecture, the command surface, the model manifest, the privacy
guarantees, the limits and the measured evidence. The command table is also the API the control
channel, `gridcraft-cli run --cmd` and MCP see, because every action is a normal engine command.

## What it does and does not translate

- Translates **text cells** in the selection. A formula, a Python cell, a number, a date, a
  boolean, an error and an empty cell are skipped, with the reason reported per cell.
- Never touches formula syntax and never writes a formula: `translate.insert` writes
  `Value::Text` directly, so a translation that begins `=`, `+`, `-` or `@` stays text.
- Segments by line and sentence, protects numbers, dates, times, URLs, e-mail addresses and
  placeholder tokens (`{name}`, `{{name}}`, `${name}`, `%s`, `%1$s`), and verifies every marker
  comes back before a translated string is accepted.
- Auto-detects the source language with a pure heuristic (Unicode script plus stopword and
  diacritic scoring). A short or ambiguous string comes back with low confidence and is **not**
  translated: the pane asks for a source language instead of guessing.
- There is no whole-workbook mode, real-time translation, comment/chart/shape translation, or
  translation memory in the file. `TRANSLATE()` / `DETECTLANGUAGE()` remain separate follow-ups
  (see `docs/function-parity.md`).

## Architecture

| Crate | Layer | Role |
|---|---|---|
| `gridcraft-translate` | L1, pure, wasm-clean | BCP-47 tags and aliases, detection, segmentation, protection, skip rules, batch planning, model manifest, the `Backend` / `ModelFetcher` seams, the deterministic test backend (`test-backend` feature, never in a shipped build) |
| `gridcraft-translate-rten` | L6, native | The model backend: verifies model files (size + SHA-256), loads the ONNX encoder/decoder with `rten`, builds the sentencepiece tokenizers with `tokenizers`, greedy decoding, CPU only |
| `gridcraft-engine` (`cmd/translate.rs`) | L5 | The eleven commands, the cell snapshot, job state on a worker thread, `translate.insert`'s one-undo-group write, model install/remove and verification |
| `gridcraft-ui-egui` (`panes.rs`, `ribbon/review.rs`) | L6 | The Translate task pane, Review › Language › Translate and keytip `RL`; strings in ten interface languages |

Inference **never runs on `Session::execute`'s synchronous path**: `translate.start` snapshots the
selection (text and kind, not the workbook), spawns a native worker thread and returns a job id.
The workbook is never borrowed while the model runs. `translate.run` is the blocking form for
headless callers. A job's state lives in `Arc<Mutex<JobData>>` on the session and is polled with
`translate.status`; the pane never writes cells except through `translate.insert`.

Model-file handling is native-only. The web build reports every model command as unavailable; no
WASM backend ships in v1.

## Commands

| Command | Params | Result | Notes |
|---|---|---|---|
| `review.translate` | `{on?, from?, to?}` | `{on, from, to, model, ready, hint?}` | Toggles the pane; a query, not journaled. `from`/`to` remember the pair. |
| `translate.start` | `{range?, sheet?, from?: "auto"\|tag, to?, maxCells?}` | `{job, cells, skipped}` | Snapshots and starts a worker; never writes. `to` defaults to the remembered target. |
| `translate.status` | `{job}` | `{state, progress, segments?, skipped?, stats?, error?}` | `state`: `queued`, `running`, `done`, `cancelled`, `failed`. |
| `translate.cancel` | `{job}` | `{cancelled}` | Idempotent; a cancelled job writes nothing. |
| `translate.run` | start's params plus `timeoutMs?` | `{segments, stats}` | Blocking convenience for CLI/MCP; writes nothing. A timeout is an error. |
| `translate.insert` | `{range?, job? \| values?, mode?: "replace"\|"emptyOnly"}` | `{cells, skipped}` | Journaled, one undo step, all-or-nothing on errors. `values` is `[{cell, text}]` or rows of text from `range`. |
| `translate.copy` | `{job, cells?}` | `{chars, cells}` | Puts the translated lines on the engine clipboard (`Session::translate_clipboard`); the pane copies them to the system clipboard on the explicit Copy action. |
| `translate.models` | `{}` | `{models, active, modelsDir, accelerators}` | Installed/available models, state, licence, hashes and the model directory. |
| `translate.modelImport` | `{path, id?, consent: true}` | model record | Copies and verifies a folder; refuses without consent. |
| `translate.modelDownload` | `{id, consent: true}` | model record | URL comes from the shipped manifest, never from the caller; refuses without consent and without a host transport. |
| `translate.modelRemove` | `{id}` | `{removed}` | Removes the model directory; unknown or traversing ids are refused. |

All eleven are registered in `cargo xtask parity`'s catalog, so `review.translate` moves from
missing to implemented. Params are ordinary JSON, so the same calls work through the control
channel, the CLI and MCP.

## Model, licence and hashes

The working v1 pair is **OPUS-MT English → Spanish**, the ONNX conversion published by Xenova.
The candle conversion at `refs/pr/4` was evaluated and rejected (it produced garbage for ordinary
sentences); the ONNX files reproduce onnxruntime token-for-token under `rten` (see evidence
below). The manifest records the model card's real licence per pair.

| File | Bytes | SHA-256 |
|---|---:|---|
| `onnx/encoder_model_quantized.onnx` | 52,899,742 | `03434edd1147e6c49e22066e9dc5888235e99cc35a6711d1c4ea5b06455d5da9` |
| `onnx/decoder_model_quantized.onnx` | 59,842,102 | `2b45074cc819f1d901e40510d7bc0781d99327d16c7eca04518c4ee0c3d6ba10` |
| `source.spm` | 801,636 | `4dd547c24816a335e7b0b2e63376a8f1b3cbfc671eda5ab808dd44fdadaa8791` |
| `target.spm` | 825,924 | `e236ee6d866b635c0142114f8647f39831f9d92534aa2aad75c942f6a78ad0e3` |
| `vocab.json` | 1,720,044 | `b074b4cca0036ade5a39ea97faabd534e1015482c480fc2cb02c6481983eb163` |
| `config.json` | 1,468 | `499eae1f97eeba63172fc884f92fd727ae803b7ceb79c0f653aa4b011152af2f` |

- Source: `Xenova/opus-mt-en-es`, pinned revision
  `4b002a4c7edd54a7ced58877258b87f7efd3f892` (immutable commit, not a moving ref).
- Licence: **Apache-2.0** (model card `Xenova/opus-mt-en-es`; the weights convert
  `Helsinki-NLP/opus-mt-en-es`, whose card is also Apache-2.0). Recorded in the manifest and shown
  before download.
- Total download: 116,090,916 bytes (~116 MB, ~111 MiB).
- The Xenova `vocab.json` differs from the Helsinki main-branch one; all six files must come from
  the same revision. Mixing them produced different token ids.
- Minimum memory hint: 512 MB. Nothing is downloaded, read or verified at startup or when the pane
  opens.

### Pairs considered for later rows

OPUS-MT licences vary by model card, so each row records its own:

| Pair | Repository | Licence |
|---|---|---|
| en → de | `Helsinki-NLP/opus-mt-en-de` | CC-BY-4.0 |
| en → fr | `Helsinki-NLP/opus-mt-en-fr` | Apache-2.0 |
| en → ru | `Helsinki-NLP/opus-mt-en-ru` | Apache-2.0 |
| en → zh | `Helsinki-NLP/opus-mt-en-zh` | Apache-2.0 |
| zh → en | `Helsinki-NLP/opus-mt-zh-en` | CC-BY-4.0 |
| ar → en | `Helsinki-NLP/opus-mt-ar-en` | Apache-2.0 |

`facebook/nllb-200` is CC-BY-NC-4.0 and is not used. An OPUS-MT pair becomes supported by adding
a manifest row, its ONNX files and a config in the backend; the command and pane surfaces do not
change.

## Model directory and lifecycle

```
<model root>/opus-mt-en-es/manifest.json   provenance sidecar written after verification
<model root>/opus-mt-en-es/<files…>        exactly the files the manifest lists
```

The root is the first of:

1. `$GRIDCRAFT_MODELS`;
2. the platform data directory: `%APPDATA%\GridCraft\models`, `~/Library/Application Support/GridCraft/models`,
   or `$XDG_DATA_HOME/gridcraft/models` (falling back to `~/.local/share`);
3. `models/` beside the executable;
4. `models/` in a source checkout.

Install is staged in a temporary directory: every file's path, byte size and SHA-256 is checked,
the sidecar is written, then the directory is renamed into place. A failure removes the staging
directory and leaves any previously installed model untouched. Model files are treated as
untrusted input: paths are relative, bounded and free of `..`, drives and backslashes; a name that
does not match a manifest row is refused.

## Accelerators and platforms

| Platform | Runtime | Status |
|---|---|---|
| Windows x86-64 | rten 0.27, pure-Rust CPU | Default and tested here |
| macOS (Intel/Apple silicon) | rten CPU | Default; no required accelerator |
| Linux x86-64/ARM64, BSD | rten CPU | Default |
| Web (wasm) | — | Translation disabled in v1; no model backend ships |

`translate.models` reports `accelerators: ["cpu"]`. `gridcraft_translate::Accelerator` is the seam
for a future optional OpenVINO backend on Intel x86 (Windows/Linux): the enum accepts `openvino`,
the manifest carries a runtime hint, and a request for an unavailable accelerator returns the
clear message "this build runs translation on the CPU; the OpenVINO NPU/GPU backend is not
available". CPU behaviour is identical whether or not that backend exists. No C/C++ runtime is a
dependency of this PR.

## Limits

| Limit | Default |
|---|---:|
| Cells per request | 10,000 |
| Characters per request | 2,000,000 |
| Model tokens per batch (estimated) | 512,000 |
| Characters per cell | 32,000 |
| Batch timeout (`translate.run`) | 600,000 ms (10 min), capped at 1 h |
| Decoder steps | 128 (greedy; stops at EOS) |
| Model file (sanity bound before the manifest check) | 512 MB |

Limits are checked before a batch is allocated; they can be lowered with command params
(`maxCells`, `timeoutMs`), never raised above the manifest caps. Identical sentences are
translated once; batches are packed by estimated token budget.

## Privacy and failure guarantees

- Inference is offline. The only network operation is a user-initiated model download through the
  host's single HTTPS transport (`ModelHost`/`ModelFetcher`); inference holds no client.
- No telemetry, analytics or crash payload. Cell text never appears in a log, an error string or a
  job's `error` field: diagnostics carry counts, durations, language tags and model ids only.
- Cell text lives in job memory for the session; nothing is written to disk by this feature. The
  workbook changes only through `translate.insert`.
- A missing, incomplete, corrupt or hash-mismatched model disables the feature with an actionable
  message and writes nothing. Truncated, zero-byte and wrong-format files are structured errors,
  never panics.
- Cancel and timeout discard the job's results; `translate.insert` refuses a job that is not
  `done`. `translate.insert` validates every target before writing and returns an error (leaving
  the workbook unchanged) rather than writing part of a batch.
- Clipboard access happens only on the explicit Copy action.

## XLSX

Translation results are ordinary cell string values. There is no new file format, custom XML
part, hidden sheet or translation memory: saving and reopening preserves the translated strings,
their styles, adjacent formulas and every untouched part. A round-trip test asserts that the
package's part names are identical before and after a translated insert.

## Retrieval of results, not of the model

`translate.status` returns the translated text to the caller that asked for the job, exactly as
reading a cell does. `translate.copy`/`translate.insert` are explicit actions. Nothing is
summarised into an error or a log.

## Measured evidence (2026-11-07, Windows x64, CPU, test profile with optimised dependencies)

Model: `Xenova/opus-mt-en-es` at `4b002a4c7edd54a7ced58877258b87f7efd3f892`, quantized ONNX,
rten 0.27, greedy decoding.

| Input | Output | Time |
|---|---|---|
| The total is 42 dollars. | `El total es de 42 dólares.` | ~1.21 s for both sentences including the 116 MB model load and verification |
| Good morning, the report is ready for review. | `Buenos días, el informe está listo para su revisión.` | included above |

The first sentence's encoder ids were `[42, 447, 31, 3222, 5624, 3, 0]` and the decoder produced
`[65000, 54, 447, 43, 4, 3222, 753, 3, 0]`, matching onnxruntime. The number `42` is preserved
through protection and restoration.

Reproduce:

```sh
GRIDCRAFT_TRANSLATE_MODEL=<model dir> cargo test -p gridcraft-translate-rten \
  --test real_model -- --ignored --nocapture
```

## Tests

- `crates/translate/src/*`: tags/aliases, detection (including short/ambiguous text), CJK/RTL
  segmentation with no text loss, protection round-trip and malformed placeholders, skip rules,
  caps, batching, the pipeline with the deterministic backend.
- `crates/engine/tests/it/translate.rs`: run → insert, one undo step, formula cells untouched,
  translated `=…` stays text, cancel/timeout/backend error write nothing, XLSX round-trip with no
  new package parts, consent/permission/hash/path/truncation failures.
- `crates/translate-rten`: file verification (size, hash, traversal, zero-byte, truncated), a
  structured wrong-format error, and ignored real-model tests reading `GRIDCRAFT_TRANSLATE_MODEL`.
