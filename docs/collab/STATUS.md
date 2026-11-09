# Real-time collaboration: status and acceptance evidence

- Date: 2026-11-09
- Branch: `feature/realtime-collaboration` (fork `copoka/gridcraft`), HEAD `c8e2a3de` (code; this page is the docs commit on top)
- Base: `origin/main` = `2c58c769` (rebased 2026-11-08)
- PR: <https://github.com/storytold/gridcraft/pull/3613> (draft)
- Design: `rfc.md`; server run book: `server.md`; client guide: `client.md`
- Session handoff (project-level, outside this repository):
  `docs/collab/STATUS.md` in the cabinet project directory; this page is the
  in-repo evidence record visible to a GitHub reviewer.

## 1. Rebase onto current `main`

The branch stood on `6a8eb4ff`; `origin/main` had moved to `2c58c769` (121 commits,
mostly the calc spill-index/unblock performance work, #2683/#2696).

- 15 commits replayed; one conflict, in `crates/calc/src/recalc.rs` (commit
  "engine: apply a remote command without this session's history").
- `main` added `Calc::spill_errors` and the `SpillErrors` index of `#SPILL!` cells
  with `unblock_candidates`; this branch added `Calc::last_evaluated` with
  `take_evaluated`. The resolution keeps both: `Calc` carries `spill_errors` and
  `last_evaluated` (both initialised in `Default`), and the `impl` block keeps
  `unblock_candidates` and `take_evaluated`.
- No CI file is touched: `git diff --name-only origin/main...HEAD -- .github` is empty.
- Pushed to the fork only (`copoka/gridcraft`); nothing is pushed to `storytold/*`.

## 2. Web host bridge (`INTEGRATION.md` §6, ADR-0016 §3)

The browser build now opens a collaboration session through the page that hosts
it, with the same protocol as the desktop:

- `gridcraft:session` → `HostSession` (`sessionId`, `wsUrl`, JWT, role) in the
  app; the token goes in the first `hello` frame, never in the URL, a log or a
  preference, and every debug print redacts it.
- `crates/collab-web` dials the URL with the page's `WebSocket` and drives the
  shared, transport-free `Synced` (the desktop client now talks to a `Link`
  trait): preview, authoritative delta, rollback, sheet keys, snapshot and
  per-user undo/redo are the same code on both transports. Unconfirmed
  operations keep their ids and are resent after the next welcome; a close
  reconnects with the desktop backoff (250 ms doubling to 10 s).
- `gridcraft:session-state` is posted on a status or participant change,
  `gridcraft:presence` at 10 Hz; `gridcraft:committed` records the cabinet's
  version and etag.
- Save in a live session posts `gridcraft:commit` (no bytes: the cabinet owns
  the assembled file) instead of downloading a local copy; Save As and exports
  stay local.
- Without a `gridcraft:session` message the web build makes no network call and
  stays single-user.

`craft-web-host`/`craft-web-api` are pinned to `copoka/craft-libs` revision
`7666e2b100d8cf12998f9b57d3b036964cc3189c` (the branch with the six
collaboration messages), the single deviation ADR-0016 §3 allows. Manifest and
`Cargo.lock` both record it; other craft-libs crates stay on their upstream
revision. After the craft-libs change merges, both pins move back to
`storytold/craft-libs`.

Evidence:

```text
cargo check -p gridcraft-web --target wasm32-unknown-unknown   ok
trunk build (0.22.1, apps/gridcraft-web)                       ok, dist/web
cargo test -p gridcraft-collab-web                             5 passed
cargo test -p gridcraft-web                                   11 passed
cargo test -p gridcraft-collab-client                          5 passed (converge)
cargo clippy -p gridcraft-collab-web -p gridcraft-web
  --all-targets -- -D warnings (native and wasm32, --no-deps)   clean
cargo xtask layers                                             OK: 21 crates
```

The two-client web test is headless: a room applies a third participant's
commands and both browser replicas are driven with the server frames through
the same `on_text` path the socket uses, then the engine cells and versions
must match. A real browser session needs the cabinet page; the contract now
has both sides implemented here.

## 3. Acceptance matrix (`TASK.md` §1–8)

| `TASK.md` | Implemented | Evidence |
|---|---|---|
| §1 session model | one session per file, owner/editor/viewer, presence (sheet, cell/range, selection, viewport), join/leave/reconnect | `4396ad93`, `56cff6ae`; `cargo test -p gridcraft-collab` (21) — `hub` presence/role tests; server socket tests `two_editors_converge_on_one_book`, `a_viewer_is_refused_before_the_engine` |
| §2 book sync | op unit = one user action (`command`/`structural`), sheet keys stable across restarts, authoritative recalculation on one engine `Session`, delta of changed cells (spills included) instead of the workbook, `baseVersion`/`epoch` conflict rules | `5a307189`, `12628770`, `f828c9fb`; collab delta/dedup/epoch tests; server acceptance `a_dynamic_array_spills_and_the_spill_survives_a_commit`, `a_large_range_travels_and_commits` (A1:E2000, 10 000 cells); client `a_structural_command_lands_on_both_replicas` |
| §3 presence/UX | overlay of other participants' selections, participant list, status bar, viewer refused locally and by the server | `56cff6ae`; `crates/ui-egui/src/collab.rs`, `apps/gridcraft/src/collab_ui.rs`; client `a_viewer_is_refused_locally_and_still_receives`; server `a_viewer_is_refused_before_the_engine`; server `a_viewer_is_refused_before_the_engine` |
| §4 undo/redo | per-actor steps (≤200), undo/redo are new room operations, refuse with `conflict` when someone else changed the cell, history survives reconnect while the server runs, not a server restart; a structural step (row or sheet insert/delete) is reversed from whole-sheet images in one operation (`applied.images`, epoch grows, sheet keys kept); an image larger than one frame travels in `imagePart` parts (ADR-0013) | `f828c9fb`, `30ebbb73`, `59eb9e06`, `c8e2a3de`; collab undo/redo/conflict and structural-image tests; client `undo_reverses_only_the_actors_own_step` / `redo_puts_the_step_back_on_every_replica` / `structural_undo_restores_rows_and_sheets_on_every_replica` / `an_eight_megabyte_sheet_image_travels_in_parts_and_converges` |
| §5 roles | viewer cannot write; role re-read from the signed JWT on every message; lease and takeover | `4bd61141`, `4396ad93`; server socket tests `a_bad_token_never_joins`, `the_lease_allows_one_session_and_the_owner_takes_over` |
| §6 compat/save | feature `collab` off by default; single-user build unchanged; commit returns the authoritative `.xlsx`, opened by LibreOffice in the acceptance test | `56cff6ae`, `39ac1a28`; server acceptance `a_committed_file_opens_in_libreoffice`; `cargo check -p gridcraft` without the feature |
| §7 server | `apps/gridcraft-collab-server`: HTTP session lifecycle, WS transport, JWT (HS256/ES256/EdDSA), healthcheck, `/metrics`, journal + snapshots + restart + compaction, no cell values or tokens in logs | `4bd61141`, `39ac1a28`, `f2346c30`; `server.md`; 19 unit + 7 socket + 5 persistence + 5 acceptance |
| §8 tests | unit, two clients, same-cell conflict, structural edits with participants, formulas/dynamic arrays, reconnect/catch-up, viewer, 10 000 cells, commit opens in LibreOffice | counts below; `7ed43c2a` |

## 4. Test run after the rebase and the structural-undo slice (2026-11-09)

Run for the structural-undo commit (`59eb9e06`) and the image-chunking slice (`c8e2a3de`):

```text
cargo +1.95.0 test -p gridcraft-collab                     31 passed
cargo +1.95.0 test -p gridcraft-collab-client               7 converge + 4 image-part tests passed
cargo +1.95.0 test -p gridcraft-collab-server --all-targets 19 unit + 7 socket + 5 persistence + 5 acceptance, all passed
cargo test -p gridcraft-collab-web -p gridcraft-web         5 + 11 passed
cargo check -p gridcraft-collab-web -p gridcraft-web --target wasm32-unknown-unknown  ok
cargo fmt --all -- --check                                 clean
cargo +1.95.0 clippy --workspace --all-targets -- -D warnings
                                                          clean
```

The clippy run uses `1.95.0`, the toolchain the branch is built with, where
`fetch_update` is not deprecated. Under the host's default `1.99.1` the same run
reports that one pre-existing deprecation in
`apps/gridcraft-collab-server/src/metrics.rs` (main's code, untouched here), so
the earlier record excluded it with `-A deprecated`.

Earlier, after the rebase (before this slice), the same branch passed
`cargo +1.95.0 test -p gridcraft-engine --lib` (898) and the full `cargo
+1.95.0 clippy --all-targets -- -D warnings` (collab, client, server, engine,
ui-egui); `cargo xtask layers` and the single-user / `--features collab`
checks are unchanged by this slice.

The earlier "no wasm32 std" note no longer applies: the remote build host has
the wasm32 target and trunk 0.22.1. The native transport is still not compiled
for WASM; the browser uses `crates/collab-web` and the host bridge.

## 5. RFC §13 open questions — where they stand in the implementation

| # | Question | State |
|---|---|---|
| 1 | delta granularity | cells, not blocks: `applied.delta` lists changed cells with values; a 10 000-cell range is one operation and one delta, split into `applied{more:true}` frames when large (ADR-0013) |
| 2 | command size limit | server accepts messages up to 9 MiB, frames up to 1 MiB (`GRIDCRAFT_COLLAB_*` config, `server.md`); a structural undo/redo image over one frame travels in `applied{more:true}` / `imagePart` parts of 512 KiB before base64 (ADR-0013), assembled by the client before it is applied; a single *command* above the message limit is still refused — client-side splitting of a large command is not implemented |
| 3 | structural queue | FIFO per document: operations pass one room mutex in arrival order; `structural` must have `baseVersion == head` and bumps `epoch` |
| 4 | snapshot cost | snapshot is the authoritative `.xlsx` written by the real writer, not a JSON model; chunked resync of 768 KiB per frame (RFC §3.3/§4.6/§9) |
| 5 | host contract | `INTEGRATION.md` §6 implemented in the app: `gridcraft:session`, `session-state`, `presence`, `commit`, `committed`; the cabinet-side page and the server-to-server commit callback are outside this repository |
| 6 | several windows | one `actorId` per user, presence shows one window (RFC §13.6); `windowId` is a possible follow-up |
| 7 | CSV/TSV | collaboration is `.xlsx` only, as in `INTEGRATION.md` |

## 6. Known gaps

1. The cabinet's server-to-server commit callback: the HTTP `commit` endpoint exists,
   the outgoing call is integration work.
2. Structural undo (row/sheet insert-delete) is one operation with whole-sheet
   images (`applied.images`), refused as `conflict` when any operation came after
   the step; an image over 512 KiB travels in `imagePart` parts (ADR-0013), so the
   old 8 MiB `too_large` bound is gone. Two limits remain: the image does not
   carry workbook-level metadata (defined names, protection of structure) or a
   sheet's `preserved` blobs; and undo/redo effects are still not journaled, so a
   reconnect between an undo and the next snapshot can miss it (the same gap the
   cell undo has).
3. TLS termination is the ingress proxy's job; the process speaks HTTP/WS.
4. Multiple windows of one user share one `actorId`.
5. One room mutex per process; document isolation is by `fileId`. Many large
   workbooks need separate workers (ADR-0007).
6. Browser transport is in place (`crates/collab-web`, host bridge); a real
   browser run against the cabinet page and the craft-libs pin removal after
   the upstream merge remain.
7. The server binary does not initialise a logger, so `log::*` from the library is
   dropped; `eprintln!` at start and shutdown work, metrics and `/healthz` are not
   affected.

## 7. Sheet images in parts (ADR-0013, 2026-11-09)

A structural undo/redo carries every sheet of its step. The old room refused an
image over 8 MiB as `too_large`; it now travels in parts, and the limit is gone:

- `crates/collab/src/parts.rs`: `split_applied(applied, budget)` serialises
  `images` once and cuts the bytes into `imagePart{seq,total,data}` frames of 512
  KiB before base64 (about 683 KiB on the wire, under the 1 MiB frame limit).
  `version`, `epoch`, `opId`, `sheets` and `result` repeat on every part;
  `more` is true until the last one.
- `apps/gridcraft-collab-server/src/ws.rs` writes the parts back to back: the
  whole sequence is queued for the peers while the room lock is still held, and
  only then does the task await a socket write, so neither another operation nor
  a second pump on another worker thread can land between two parts (ADR-0013
  §2). A journal replay during `resync` uses the same splitter.
- The client (`Synced::accept_image_parts`) buffers the parts and applies the
  operation only after `more:false`; out-of-order, missing or undecodable parts,
  another operation in the middle, or a disconnect drop the buffer, leave the
  document and its version untouched, and ask for a catch-up from the version
  before the first part. `note_applied` advances the version only on the last
  part, so the reconnect `hello` reports the version before the sequence.

Tests:

- `crates/collab/src/parts.rs`: a 9 MiB image is cut into parts, each frame
  serialises to ≤ 1 MiB, the flags/`opId`/version/epoch repeat, and the parts
  rebuild the exact image bytes.
- `crates/collab/src/tests.rs`:
  `a_structural_undo_with_an_image_over_eight_megabytes_is_not_refused`.
- `crates/collab-client/tests/converge.rs`:
  `an_eight_megabyte_sheet_image_travels_in_parts_and_converges` — two replicas,
  a real server, twelve 700 KiB cells, `home.insertSheet`, undo and redo; both
  replicas converge, the caller sees one completed `applied` per operation, and
  its image is over 8 MiB.
- `crates/collab-client/tests/parts.rs`: two replicas fed the same parts (9 MiB
  image) apply nothing until the last part; a disconnect mid-sequence and a
  missing part both leave the document intact and request a resync from the
  version before the first part.
