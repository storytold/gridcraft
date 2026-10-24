# Unused IFERROR fallbacks during suspended recalculation

> **Last updated:** 2026-10-24

This is a follow-up to PR #1340's bounded-stack recalculation, now merged upstream.
The branch is aligned with `main` at `3099df9`; it preserves the parent's batched
dependencies, waiting guards and deduplication. The remaining change is the range
batching guard and the generated regression coverage described below.

At the nesting limit, a formula read temporarily returns `#CALC!` while recalculation
unwinds to evaluate a pending dependency. An enclosing IFERROR can then read its
fallback even though the primary expression will succeed after resumption. The
parent scheduler also queued that fallback as a dependency. If the fallback points
back to the suspended chain, it reports a false `#CIRC!`.

Batch collection now belongs to range reads that started before suspension, at the
nesting limit. Reads started after suspension do not add new batched dependencies.
This keeps the wide-range batching needed by #1340 without treating an unused
fallback as a genuine dependency. Both streamed and materialized range reads use
the same guard.

## Generated regression

`crates/calc/tests/unused_error_fallback.rs` builds a fictional workbook:

- `Sheet1!A1:A63` each reads the next cell and adds 1.
- `A64 = IFERROR(SUM(Data!B1:B128), Data!D1)`.
- Each source cell in `Data!B1:B128` is the formula `=1`.
- `Data!D1 = Sheet1!A1+1`.

The primary expression succeeds: A1 must be 191 and D1 192, with no circular
diagnostic. The same assertions cover `SUM(Data!D1:D2)` and
`SUMPRODUCT(Data!D1:D2)` fallbacks. Controls make the fallback genuinely necessary,
introduce a real ancestor back-edge or a cycle between batched siblings, and remove
those cycles to verify recovery. An acyclic used fallback still returns its value.

```sh
cargo test -p gridcraft-calc --test unused_error_fallback
cargo test -p gridcraft-calc --lib wide_read_at_the_nesting_limit_suspends_once
cargo xtask ci
```

The existing 3,000-cell suspension-count regression remains the performance guard.
No elapsed-time speedup is claimed for this change. All source data and formulas
are generated in Rust; no private workbook or Excel-produced fixture is included.

## Revision history

| Date | Change |
|---|---|
| 2026-10-24 | Align with main at `3099df9` after #1340 merged, preserving the parent scheduler, its 3,000-cell regression and subsequent upstream hardening |
| 2026-10-21 | Guard range dependency batching against unused IFERROR fallback reads; cover real cycles and recovery |
