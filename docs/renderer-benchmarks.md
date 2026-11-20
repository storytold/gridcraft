# Repeatable grid renderer measurements

> **Last updated:** 2026-10-31 · **Change:** major (standalone renderer benchmark workflow)

`cargo xtask renderer-bench` builds and measures the public production grid renderer. It works
on main without a Thai adapter, optional diagnostic feature, private paths or saved workbooks.
Latin is the default. Thai and mixed workloads use an explicitly verified external font.
This is a native developer tool; it does not change application behavior or the web build.

## Quick start

Install Git, the workspace Rust toolchain and the native build prerequisites described in the
project README. Run these commands from the checkout root. The same arguments work in Bash
and PowerShell:

```text
cargo xtask renderer-bench --help
cargo xtask renderer-bench --profile dev --cold 2 --warm 5
cargo xtask renderer-bench --profile release --config all --cold 30 --warm 100
```

The dev-profile command is a setup/smoke run, not release-performance evidence. No external font is
needed for Latin. Do not compare dev and release numbers. `--config` accepts `100x1` (default),
`50x1`, `50x2`, `100x2`, `200x1`, `200x2`, or `all` (zoom percent × pixels per point).
Cold counts are 1–1000 and warm counts 2–1000. Normal benchmark execution initializes no GPU.

Each invocation creates an **exclusive new run directory** beneath `plan/renderer-bench`.
`--out DIR` chooses another parent, which must be gitignored or outside the checkout. The
printed path identifies that run's `report.json`; `REPORT.md`, `samples.json` and
`source-manifest.json` accompany it. A failed run produces no new verified report and cannot
be mistaken for an earlier successful run in the same directory.

## Thai and mixed text

Fonts remain outside this repository. One reproducible fixture is the OFL Noto Sans Thai from
the same public craft-fonts revision used by project CI:

```text
git clone https://github.com/storytold/craft-fonts.git ../craft-fonts
git -C ../craft-fonts checkout b076417b5880bd4154bd22f00695b1d28c6bea83
cargo xtask renderer-bench --script mixed --font ../craft-fonts/fonts/noto-sans-thai/NotoSansThai.ttf --config all
```

The default expected SHA-256 is
`5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8`.
Use `--script thai` for Thai-bearing text without the mixed word list. Alternatively set
`CRAFT_FONTS_DIR` to the external checkout: Bash `export CRAFT_FONTS_DIR=../craft-fonts`,
PowerShell `$env:CRAFT_FONTS_DIR='../craft-fonts'`. Custom fonts require an explicit
`--font-sha256 HEX`. The tool verifies the actual loaded bytes and face-zero glyph coverage
for every workload character; unreadable, wrong-hash or incomplete faces fail explicitly.
Record the font license and keep font binaries outside this repo.

For a diagnostic image, add `--snapshots` to a run (start with `--config 100x1`). This requires
a functioning native wgpu backend. Separate GPU harness setup, rendering and PNG encoding
run after measurements; they are excluded from recorded CPU/UI times. PNGs stay in that run's
ignored directory. Do not use screenshots as proof of all-font shaping or editor correctness.

## What is measured

The workload has 500 unique committed text cells, 20 columns × 25 rows, each 80 Unicode
scalars. Row heights and widths are explicit; Wrap Text is enabled. A fixed word generator
and unique cell prefixes produce reproducible inputs, with a recorded workload hash.

One timed pass includes `SheetApp::logic` and `grid::show` through egui_kittest. It excludes
workbook construction, font setup/validation, viewport setup, coverage/hashing, GPU work and
PNG encoding. Every sample verifies all 500 prefixes against real Grid Geo cell rectangles.
Cold samples each use a fresh Context and record the **first grid pass after font preparation**;
they do not measure whole-app startup or total time until pending layout work completes.

Warmup waits for three unchanged output signatures, capped at 256 passes. Each timed warm
pass must retain that signature. Stable output can include legacy fallback; this check does
not certify internal Thai-adapter completion. Core correctness/counter tests remain separate.
Raw durations are integer nanoseconds, with nearest-rank p50/p95 summaries. Never infer
statistical significance from a smoke run or one fixed run order. Reports fingerprint loaded
font bytes/bindings and actual Grid Geo rectangles; comparison rejects different sets/geometry.

## Source identity and comparisons

The runner inventories actual tracked and nonignored untracked checkout bytes (including
deletions), embeds their digest and HEAD into the example build, and checks both before/after
building and measurement. The executable verifies its embedded identity at startup and the
runner also hashes it before/after execution. It records current UTC through chrono, actual
toolchain versions, profile, OS/architecture, compiler-wrapper/profile/flag environment, font
and workload identity. It accepts
no caller-supplied revision/manifest label. A stale binary or changed checkout is rejected.

This guards accidental stale-source results, not malicious build-tool tampering or a complete
reproducible-build attestation. External dependencies are pinned by Cargo.lock; compiled build
scripts, toolchains and drivers are trusted. Symlinks/submodules are refused. Inventory is
bounded to 20,000 files, 32 MiB/file and 256 MiB total. Checkout line endings can give different
tree hashes at the same commit on different platforms. Do not edit the checkout during a run.
Keep outputs ignored so they do not
alter the measured source inventory. Use separate target directories for simultaneous runs.

Run base and head on the same idle host, with the same toolchain/profile/font, counts and
configurations, and alternate run order. Use the concrete report paths printed by each run:

```text
cargo xtask renderer-bench compare BASE/report.json HEAD/report.json plan/comparison.md
```

Comparison refuses mismatched profiles, toolchains, platform, RUSTFLAGS, workload, font,
configuration or sample counts. The output file must be new. Host hardware, thermal state and
background load still require operator control. Tables use the reports' actual source IDs;
there is no historical baseline name or automatic performance-target verdict.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-31 | major | Native public-renderer benchmark, source guards, portable font fixture and comparison workflow |
