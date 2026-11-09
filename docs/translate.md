# Translate (Review › Language › Translate)

GridCraft translates selected cell text **locally, offline, with a user-controlled model**. There
is no cloud service, no account, no telemetry and no network call during inference. The model is
not shipped with GridCraft: the person downloads it after a consent screen (HTTPS, source, size,
licence and SHA-256 shown first) or places the files in the model directory by hand.

This page covers the architecture, the command surface, the model manifest, the privacy
guarantees, the limits and the measured evidence. The command table is also the API the control
channel, `gridcraft-cli run --cmd` and MCP see, because every action is a normal engine command.

## What it does and does not translate

- Translates **text cells** in the selection. A formula, a Python cell, a number, a date, a boolean, an error and an empty cell are skipped, with the reason reported per cell.
- Translated cells keep their cell-level style, but rich-text runs inside one cell are not
  preserved: XLSX import flattens shared-string runs to plain text before translation, so a
  translated insert cannot restore per-run formatting.
- Segments by line and sentence, protects numbers, dates, times, URLs, e-mail addresses and
  placeholder tokens (`{name}`, `{{name}}`, `${name}`, `%s`, `%1$s`), and verifies every marker
  comes back before a translated string is accepted.
- Auto-detects the source language with a pure heuristic (Unicode script plus stopword and
  diacritic scoring). With the shipped Qwen3 backend, `from: auto` reaches the model, which
  translates into the target directly; the detector's answer is reported but never blocks.
  Backends without `auto_source` support instead fail low-confidence strings (`lowConfidence`)
  rather than guessing, and the pane asks for a source language.
- There is no whole-workbook mode, real-time translation, comment/chart/shape translation, or
  translation memory in the file. `TRANSLATE()` / `DETECTLANGUAGE()` remain separate follow-ups
  (see `docs/function-parity.md`).

## Architecture

| Crate | Layer | Role |
|---|---|---|
| `gridcraft-translate` | L1, pure, wasm-clean | BCP-47 tags and aliases, detection, segmentation, protection, skip rules, batch planning, model manifest, the `Backend` / `ModelFetcher` seams, the deterministic test backend (`test-backend` feature, never in a shipped build) |
| `gridcraft-translate-candle` | L6, native | The model backend: verifies model files (size + SHA-256), loads a dense Qwen3 GGUF with candle's `quantized_qwen3`, tokenizes with `tokenizers`, greedy chat-format decoding on the CPU |
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
| `translate.run` | start's params plus `timeoutMs?` | `{job, segments, stats, model, from, to, elapsedMs}` | Blocking convenience for headless callers (CLI, headless MCP); writes nothing. Refused on UI-thread sessions (desktop pane, control channel, MCP connected to the app), which use `translate.start` plus `translate.status` instead. Returns the `job` id as well as the report, so the same result can be passed to `translate.insert` or `translate.copy`; it stays readable while the session keeps it. A timeout is an error. |
| `translate.insert` | `{range?, job? \| values?, mode?: "replace"\|"emptyOnly"}` | `{cells, skipped}` | Journaled, one undo step, all-or-nothing on errors. `values` is `[{cell, text}]` or rows of text from `range`. |
| `translate.copy` | `{job, cells?}` | `{chars, cells}` | Puts the translated lines on the engine clipboard (`Session::translate_clipboard`); the pane copies them to the system clipboard on the explicit Copy action. |
| `translate.models` | `{}` | `{models, active, modelsDir, accelerators}` | Installed/available models, state, licence, hashes and the model directory. |
| `translate.modelImport` | `{path, id?, consent: true}` | model record | Copies and verifies a folder; refuses without consent. On a host session (desktop, CLI) refreshes the backend through the host's opener, so the model is usable right away; a session with no opener (web build) still needs one injected. |
| `translate.modelDownload` | `{id, consent: true}` | model record | URL comes from the shipped manifest, never from the caller; refuses without consent and without a host transport. Same refresh rule as `translate.modelImport`. |
| `translate.modelRemove` | `{id}` | `{removed}` | Removes the model directory; unknown or traversing ids are refused. |

Only `review.translate` is a ribbon-catalog entry, so it is the one that moves from missing to
implemented in `cargo xtask parity` and in the 282 / 296 count. The other ten are registered
engine commands: reachable through the control channel, `gridcraft-cli run --cmd` and MCP, but not
catalog rows (there is no ribbon button of their own). Params are ordinary JSON, so the same calls
work through every surface.

A headless run hands its `job` id straight to the follow-up commands:

```jsonc
// 1. translate the selection; the report comes back with the id
{"id": "translate.run", "params": {"range": "A1:B20", "from": "en", "to": "es"}}
// → {"job": 7, "segments": [...], "stats": {...}}

// 2. write that result (one undo step, all-or-nothing)
{"id": "translate.insert", "params": {"job": 7, "mode": "replace"}}
// or take the lines instead of writing them
{"id": "translate.copy", "params": {"job": 7}}
```

## Model, licence and hashes

The working v1 model is **Qwen3 1.7B**, dense and Apache-2.0, quantized to Q8_0. Qwen3.5 is a
hybrid linear-attention architecture (`qwen3_5`, linear-attention layers with convolution state)
that candle does not implement; only dense `qwen3`/`quantized_qwen3` exists, so Qwen3.5 is not
used. The model is multilingual (119 languages), so the same weights serve several pairs.

| File | Bytes | SHA-256 |
|---|---:|---|
| `Qwen3-1.7B-Q8_0.gguf` | 1,834,426,016 | `061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a` |
| `tokenizer.json` | 11,422,654 | `aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4` |

- Weights: `Qwen/Qwen3-1.7B-GGUF` at `90862c4b9d2787eaed51d12237eafdfe7c5f6077`.
- Tokenizer: `Qwen/Qwen3-1.7B` at `70d244cc86ccca08cf5af4e1e306ecf908b1ad5e` (the GGUF repository
  does not ship it, so the manifest carries one pinned URL per file).
- Licence: **Apache-2.0** (weights and tokenizer model cards), recorded in the manifest and shown
  before download.
- Total download: 1,845,848,670 bytes (~1.85 GB, ~1.72 GiB). This is a large model: the pane shows
  the size and the 3 GB memory hint (`minRamMb`) honestly. There is no separate per-model cap:
  each file is bounded by its own manifest size, and the person sees the total and consents before
  anything is fetched.
- Inference is one segment at a time: the chat prompt is capped at 2,048 tokens and new tokens at
  256; the 1.8 GB GGUF loads once, on the worker's first translation.
- Nothing is downloaded, read or verified at startup or when the pane opens.

### Pairs verified in v1

`en → es`, `en → de`, `en → ja` and `zh → en` are in the manifest and were run end to end on this
machine (see Measured evidence). The model can translate more languages, but a pair is only listed
as supported after it is run and recorded; do not claim a pair from the model card alone. With
`from: auto` the heuristic detector's answer is shown but the model performs the translation, so
an uncertain short string still translates.

The same manifest ships **Qwen3 0.6B** (`qwen3-0.6b`): the same four pairs,
`Qwen3-0.6B-Q8_0.gguf` at 639,446,688 bytes plus the same tokenizer, with a ~1.5 GB resident
requirement. It is a smaller, faster, less precise alternative on the same backend, not a second
backend: `CandleBackend::open(root, "qwen3-0.6b")` verifies and loads it, and the weights path
always comes from the manifest's `weights` field. The desktop app and the CLI inject only
`qwen3-1.7b` at startup (`installed_backend`), so 0.6B is an embedder-only option today: a caller
that opens `CandleBackend::open(root, "qwen3-0.6b")` itself gets it, but neither the app nor the
CLI picks it up automatically.

Other models considered: OPUS-MT per-pair models were implemented first (small, ~116 MB) but
produced narrower output and needed one download per pair; candle + Qwen3 gives broad coverage
with one model. `Qwen3-4B` (Q4_K_M) remains a quality fallback for a machine that can spare the
memory.

## Model directory and lifecycle

```
<model root>/qwen3-1.7b/manifest.json   provenance sidecar written after verification
<model root>/qwen3-1.7b/<files…>        exactly the files the manifest lists
```

The root is the first of:

1. `$GRIDCRAFT_MODELS`;
2. the platform data directory: `%APPDATA%\GridCraft\models`, `~/Library/Application Support/GridCraft/models`,
   or `$XDG_DATA_HOME/gridcraft/models` (falling back to `~/.local/share`);
3. `models/` beside the executable;
4. `models/` in a source checkout.

Install is staged in a temporary directory: each file is streamed into it under the manifest's
size limit (so a 1.8 GB download is never held in memory), then every file's path, byte size and
SHA-256 is checked, the sidecar is written, and the directory is renamed into place. A failure
removes the staging directory and leaves any previously installed model untouched. Model files are
treated as untrusted input: paths are relative, bounded and free of `..`, drives and backslashes; a
name that does not match a manifest row is refused.

## Accelerators and platforms

| Platform | Runtime | Status |
|---|---|---|
| Windows x86-64 | candle 0.11, pure-Rust CPU | Default and tested here |
| macOS (Intel/Apple silicon) | candle CPU | Default; no required accelerator |
| Linux x86-64/ARM64, BSD | candle CPU | Default |
| Web (wasm) | — | Translation disabled in v1; no model backend ships |

`translate.models` reports `accelerators: ["cpu"]`. `gridcraft_translate::Accelerator` is the seam
for a future optional OpenVINO backend on Intel x86 (Windows/Linux): the enum accepts `openvino`,
the manifest carries a runtime hint, and a request for an unavailable accelerator returns the
clear message "this build runs translation on the CPU; the OpenVINO NPU/GPU backend is not
available". CPU behaviour is identical whether or not that backend exists. No C/C++ runtime is a
dependency of this PR: `tokenizers` is built with `default-features = false` and `fancy-regex`, so
the default `onig`/`esaxx` native builds are off.

## Limits

| Limit | Default |
|---|---:|
| Cells per request | 10,000 |
| Characters per request | 2,000,000 |
| Model tokens per batch (estimated) | 512,000 (each segment is one model call, prompt ≤ 2,048) |
| Characters per cell | 32,000 |
| Job timeout (`translate.run`, default) | 1,800,000 ms (30 min), capped at 1 h; cancellation is authoritative |
| New tokens per segment | 256 (greedy; stops at `<\|im_end\|>`/EOS; a decode that reaches 256 without one is an error, so a truncated sentence is never accepted) |
| Model file | The manifest's exact byte size for that file: it is the download bound (a larger `content-length` or body is refused) and the size check after it |

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

Model: `Qwen3-1.7B-Q8_0.gguf` at the pinned revision, candle 0.11 `quantized_qwen3`, greedy,
thinking forced off with the empty `<think></think>` block. Times include protection,
tokenisation and decode for one cell; the first call also includes loading and verifying the
1.8 GB file. Machine load varied during the run, so treat them as an honest range, not a
benchmark: 7.5–45.5 s per cell, with most short sentences at 20–30 s.

| Input | Output | Time |
|---|---|---|
| The total is 42 dollars. | `El total es 42 dólares.` | 45.5 s (first call, includes model load); 30.3 s on a later repeat |
| Good morning, the report is ready for review. | `Guten Morgen, der Bericht ist für die Prüfung bereit.` | 21.2 s |
| The meeting is on Friday. | `会は金曜日に開催されます。` | 20.0 s |
| 会议在星期五。 | `The meeting is on Friday.` | 7.6 s |
| The total is 42 dollars. (auto source) | `El total es 42 dólares.` | 30.3 s |

The number `42` survives protection and restoration in every Spanish output. Reported by the
maintainer on the same day, also under build load: 1.7B `58 s / 102 s / 47 s` for
en→es, en→de and en→ja; 0.6B `36 s / 50 s / 27 s` for the same shape, with outputs
`Total: 42 dólares.`, `Guten Tag, der Bericht ist für die Überprüfung bereit.` and `近い駅の位置？`.
The 0.6B model is faster and less precise; both are Apache-2.0.

Reproduce:

```sh
GRIDCRAFT_TRANSLATE_MODEL=<qwen3-1.7b dir> cargo test -p gridcraft-translate-candle \
  --test real_model -- --ignored --nocapture
```

## Tests

- `crates/translate/src/*`: tags/aliases, detection (including short/ambiguous text), CJK/RTL
  segmentation with no text loss, protection round-trip and malformed placeholders, skip rules,
  caps, batching, the pipeline with the deterministic backend.
- `crates/engine/tests/it/translate.rs`: run → insert, one undo step, formula cells untouched,
  translated `=…` stays text, cancel/timeout/backend error write nothing, XLSX round-trip with no
  new package parts, consent/permission/hash/path/truncation failures.
- `crates/translate-candle`: file verification (size, hash, traversal, zero-byte, truncated), a
  structured wrong-format error, and ignored real-model tests reading `GRIDCRAFT_TRANSLATE_MODEL`.
