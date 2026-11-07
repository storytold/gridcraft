# Action Pen: handwriting to text

> **Last reviewed:** 2026-11-07 · **Change:** created with the feature (#1777) · **Target:** Microsoft Excel (Microsoft 365) · Part of [target-app-parity.md](target-app-parity.md) and [hardware-parity.md](hardware-parity.md)

Draw › Action Pen turns handwriting into a cell value. The person picks the tool, writes one
line in a cell, and releases: the stroke is recognised, the writing is typed into the cell under
it (or a `cell` the command names) exactly as if it had been typed — so `42` is a number,
`3/4/2024` is a date, `=SUM(A1:A3)` is a formula — as **one undo step**, with no ink shape left
on the sheet.

The feature is offline, cross-platform and pure Rust. Its model is **not** bundled: GridCraft
works without it (the tool is disabled and says where the model goes), and a person downloads it
once or copies it in by hand. Nothing is fetched at startup, on file open or while a pane is
open, and inference never touches the network.

## The model

| | |
|---|---|
| Model | `Xenova/trocr-small-handwritten` (TrOCR small, handwritten; weights from `microsoft/trocr-small-handwritten`) |
| Revision | `02bea33ca09a70e2e2d91140a7e6e1a729606a5b` |
| Licence | MIT (model card) |
| Source | <https://huggingface.co/Xenova/trocr-small-handwritten> |
| Runtime | `rten` 0.27 (pure Rust, CPU) |
| Total | 250,806,629 bytes (about 251 MB) in 3 files |

| File (inside the model directory) | Bytes | SHA-256 |
|---|---|---|
| `onnx/encoder_model.onnx` | 87,496,990 | `f6a2c3ac23a426c85f8557f2763bf39cce7eeacf3004563762e023050ddc1fd9` |
| `onnx/decoder_model.onnx` | 158,814,912 | `4cf40055fbca767b2fd34f1de4f86be41d8dac670028bb6f65a3d35ebe0b0e5f` |
| `tokenizer.json` | 4,494,727 | `68bcb5468c854362a615f3d2ff6a5e4091a85f4c8198993ed9a30afe0b143737` |

The **fp32** exports are the ones the manifest lists. The quantized exports are smaller but were
not usable: rten 0.27 and onnxruntime disagree on this model's quantized graphs (encoder output
correlation 0.52), while the fp32 files agree, and onnxruntime's own quantized result was worse
(CER 0.056 against 0.0297). The reasoning is recorded in [decisions.md](decisions.md).

No model file is committed, in `assets/` or anywhere else. The repository holds the manifest
above (id, revision, paths, sizes, hashes, licence, URLs) and nothing of the weights.

## Where the model goes

```
<model root>/trocr-small-handwritten/manifest.json      (optional provenance sidecar)
<model root>/trocr-small-handwritten/onnx/encoder_model.onnx
<model root>/trocr-small-handwritten/onnx/decoder_model.onnx
<model root>/trocr-small-handwritten/tokenizer.json
```

`<model root>` is resolved in this order, first hit wins:

1. `$GRIDCRAFT_MODELS`
2. the app's settings folder plus `models/` (`%APPDATA%\GridCraft\models` on Windows,
   `~/Library/Application Support/GridCraft/models` on macOS, `$XDG_CONFIG_HOME/gridcraft/models`
   or `~/.config/gridcraft/models` elsewhere)
3. `models/` beside the executable
4. `models/` in the source checkout (development convenience)

The same layout and the same order serve every Craft app's models.

## Installing it: two ways, both user-initiated

- **Download, from Draw › Action Pen.** The button opens the model panel (name, licence, size,
  source, folder, status) with a Download button. Only that click starts a download: HTTPS to the
  URLs in the manifest, streamed into a `.part` file, checked for its exact size and SHA-256, then
  renamed into place. A damaged or truncated download is deleted, not installed. There is no
  automatic retry, no background refresh and no telemetry.
- **By hand.** Download the three files from the Hugging Face page in a browser and put them in
  the folder (or copy them from another machine). `Import…` in the same panel copies them through
  a temporary file and verifies each one; copying them into the folder directly works too — the
  app verifies sizes at startup and hashes them when it loads them.

When the files are present the recognizer loads on a worker thread (hashing 251 MB and parsing
two ONNX graphs takes about a second) and the tool becomes available. When they are missing,
incomplete or refused, the tool stays disabled and says so, and every command returns a
structured error naming the folder to fill.

## Commands

| Command | Kind | What it does |
|---|---|---|
| `draw.actionPen` | toggle (no undo) | `{on?: bool}` turns the tool on or off. Turning it on without a model is refused with the folder to install it in. |
| `draw.actionPenInk` | edit (one undo step) | `{points: [[x,y],…], cell?}` recognises one stroke in sheet points and types the text into the cell under the stroke (or `cell`). Returns `{cell, text, value}`. |
| `ink.status` | query | `{accelerator?}` reports `installed`/`filesPresent`, `ready`, the model id, name, revision, licence, source, size, runtime, the folder, missing files, a one-line hint, and the accelerator fields below. |
| `ink.modelImport` | native | `{path}` copies one model file or a folder of them into the model folder, verifying each, and reports what is still missing. |
| `ink.modelRemove` | native | `{}` deletes the model folder, turns the tool off and detaches the recognizer. |

The engine never loads a model itself: `Session::ink` holds an `Arc<dyn gridcraft_ink::Recognizer>`
and `Session::models_dir` the folder, both set by the host. CLI and MCP sessions have neither, so
they can still report `ink.status` and get a structured error from the tool.

## How a stroke becomes text

1. **Rasterise.** `gridcraft-ink` computes the stroke's bounding box and draws it anti-aliased
   into a grey line image: paper white, ink black, the pen's width from the existing `ink_width`
   encoding (0.5–40 pt), paper padding all round, at most 1024 px on the long side and with a
   bounded number of segments and pixel visits, so a hostile stroke cannot make the rasteriser
   expensive.
2. **Normalise.** The image is stretched (not letterboxed) to 384 × 384, three channels in CHW
   order, each value `(pixel / 255 - 0.5) / 0.5`: -1 is ink, +1 is paper.
3. **Encode and decode.** `gridcraft-ink-rten` runs the encoder once, then greedy-decodes up to 32
   steps from start token 2, stopping at 2 (or padding), batch 1.
4. **Detokenise.** The model's own `tokenizer.json`, loaded with the `tokenizers` crate (default
   features off; `fancy-regex` instead of the C `onig`), decodes the ids.
5. **Type it.** The text goes through `cmd::edit::input_to_cell`, the same path as typing: numbers,
   dates, booleans and formulas get their typed value and automatic number format. One `edit`, one
   undo step, `cx.touch` for the cell.

The rasteriser's shape and ink coverage are pinned by tests in `crates/ink/src/raster.rs` (a
100-point stroke with a 2 pt pen comes out about 400 px long and 8 px thick, inside paper
padding, with the ink covering 3–15% of the image).

## Accuracy, measured

Windows 11 x64, CPU only, release build, 2026-11-07. The inputs are the first 40 lines of the
**validation** split of [`Teklia/IAM-line`](https://huggingface.co/datasets/Teklia/IAM-line)
(MIT), preprocessed exactly as the model expects (RGB, bicubic resize to 384 × 384, `/255`,
mean/std 0.5, NCHW) and run through this crate's encoder, decoder and tokenizer:

| Run | Lines | Exact | CER | ms/line |
|---|---|---|---|---|
| Reference pre-flight (rten 0.27, fp32) | 40 | 22 / 40 | 0.0297 | ~274 |
| `crates/ink-rten/tests/real_model.rs`, this branch | 40 | 22 / 40 | 0.0297 | 629 |
CER is Levenshtein distance over characters divided by the truth's length, lowercased on both
sides; "exact" is an identical lowercased line. The 629 ms/line is from a run while five other
builds shared the machine; the same binary takes about 0.27 s per line when the CPU is otherwise
idle (the reference row), and the difference is scheduling, not the implementation.

**Limitation, stated plainly.** These are photographs of handwriting from IAM, which is the
distribution TrOCR was trained on. The number proves the runtime, the preprocessing and the
tokenizer reproduce the reference pipeline; it does **not** measure the Action Pen's real input,
which is ink rasterised from stroke points (a different pen, a different width, no paper texture
or slant). Accuracy on rasterised ink, on several lines in one cell, on non-Latin scripts and on
mathematics is not yet measured; [gaps.md](gaps.md) §17 tracks it.

### Reproducing the measurement

The check is an ignored test, so CI never needs the model or the data:

```text
GRIDCRAFT_TROCR_MODEL=<models root> \
GRIDCRAFT_TROCR_LINES=<data dir> \
  cargo test --release -p gridcraft-ink-rten --test real_model -- --ignored --nocapture
```

`<data dir>` holds `inputs.bin` (little-endian f32, `3 * 384 * 384` values per line, CHW,
`(pixel / 255 - 0.5) / 0.5`) and `truth.txt` (one ground-truth line per input). They are made
from the validation parquet with, in outline:

```python
rows = pyarrow.parquet.read_table("validation.parquet").to_pylist()[:40]
for r in rows:
    im = PIL.Image.open(io.BytesIO(r["image"]["bytes"])).convert("RGB").resize((384, 384), PIL.Image.BICUBIC)
    x = (numpy.asarray(im, dtype="float32") / 255.0 - 0.5) / 0.5
    inputs.append(x.transpose(2, 0, 1))
inputs.tofile("inputs.bin")
open("truth.txt", "w").write("\n".join(r["text"] for r in rows) + "\n")
```

The test prints each line's truth and recognition, then the summary, and fails if CER is worse
than 0.06.

## Backend and accelerator matrix

Inference is **pure-Rust CPU** by default, on every platform GridCraft ships. No accelerator is
required, and no C or C++ runtime is added for it (the `tokenizers` build uses `fancy-regex`,
not `onig`).

| Platform | Default backend | Optional accelerator |
|---|---|---|
| Windows x86-64 / ARM64 | CPU (`rten`) | none in this PR; an Intel OpenVINO backend (NPU/GPU, on x86) is being measured and would land as its own optional crate with CPU fallback |
| macOS (Intel and Apple silicon) | CPU (`rten`) | none in this PR; an Apple Neural Engine backend would be its own optional crate |
| Linux x86-64 / ARM64 | CPU (`rten`) | as Windows (OpenVINO on x86) |
| FreeBSD and other BSDs | CPU (`rten`) | none |
| Web (wasm) | disabled: no model directory, no threads | none |

The seam is in place so an accelerator never changes the feature:

- `gridcraft_ink::Accelerator` (`Cpu`, `OpenVino`, `Apple`) with `as_str()`/`parse()`.
- `Recognizer::accelerator()` and `Recognizer::accelerators()`, defaulting to CPU only.
- `Manifest::runtime` (the runtime and version the hashes belong to) and
  `Manifest::accelerators` (CPU always; a manifest without CPU is refused).
- `ink.status` reports `runtime`, `accelerator`, `accelerators`, and, when its `accelerator`
  parameter asks for a backend the build does not have, `requestedAccelerator` plus
  `acceleratorMessage` — "Handwriting runs on CPU; OpenVINO NPU/GPU is not available in this
  build, so recognition stays on CPU." — instead of failing. The feature behaves identically on
  CPU.

## Privacy and safety

- No network call at startup, on file open or on pane open. A download happens only after the
  Download button is pressed, over HTTPS, from the manifest's URLs.
- Zero network calls during inference.
- Model files are untrusted input: manifest paths that could escape the model directory are
  refused, sizes are checked before parsing, files are hashed before use, and an import or
  download writes through a temporary file so a failure leaves no half-written model.
- Documents are never sent anywhere. Cell text, stroked points and recognised text stay in the
  process; nothing about them is logged.
