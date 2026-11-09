# Real-time collaboration: status and acceptance evidence

- Date: 2026-11-09
- Branch: `feature/realtime-collaboration` (fork `copoka/gridcraft`), HEAD `bcf9c16d` (code, tests and this page)
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
| §4 undo/redo | per-actor steps (≤200), undo/redo are new room operations, refuse with `conflict` when someone else changed the cell, history survives reconnect while the server runs, not a server restart; a structural step (row or sheet insert/delete) is reversed from whole-sheet images in one operation (`applied.images`, epoch grows, sheet keys kept) and the workbook-level state of the same boundary (`applied.book`: defined names, structure protection, package and workbook kept content with chart sheets, the VBA project, styles, calc) plus each sheet's `preserved` blobs; an image larger than one frame travels in `imagePart` parts (ADR-0013) and one over the 32 MiB operation bound is refused `too_large` before mutation; an undo/redo is journaled like any other version, so a reconnect after it — or a restart that replays the tail — reaches the undone document | `f828c9fb`, `30ebbb73`, `59eb9e06`, `c8e2a3de`, `d36899da`, `8883269d`; collab undo/redo/conflict, structural-image, package-state and replay tests; client `undo_reverses_only_the_actors_own_step` / `redo_puts_the_step_back_on_every_replica` / `structural_undo_restores_rows_and_sheets_on_every_replica` / `structural_undo_restores_the_workbook_metadata_on_every_replica` / `structural_undo_restores_the_package_state_and_vba_on_every_replica` / `an_eight_megabyte_sheet_image_travels_in_parts_and_converges` / `a_peer_that_reconnects_after_an_undo_sees_the_undone_state` / `a_peer_that_reconnects_after_a_structural_undo_gets_the_sheet_image`; server persistence `an_undo_replays_from_the_journal_after_a_restart` / `a_structural_undo_replays_the_workbook_metadata_after_a_restart` / `a_structural_undo_restores_the_package_state_after_a_restart` / `a_snapshot_after_an_undo_keeps_the_step_in_the_base` |
| §5 roles | viewer cannot write; role re-read from the signed JWT on every message; lease and takeover | `4bd61141`, `4396ad93`; server socket tests `a_bad_token_never_joins`, `the_lease_allows_one_session_and_the_owner_takes_over` |
| §6 compat/save | feature `collab` off by default; single-user build unchanged; commit returns the authoritative `.xlsx`, opened by LibreOffice in the acceptance test | `56cff6ae`, `39ac1a28`; server acceptance `a_committed_file_opens_in_libreoffice`; `cargo check -p gridcraft` without the feature |
| §7 server | `apps/gridcraft-collab-server`: HTTP session lifecycle, WS transport, JWT (HS256/ES256/EdDSA), healthcheck, `/metrics`, journal + snapshots + restart + compaction, no cell values or tokens in logs | `4bd61141`, `39ac1a28`, `f2346c30`, `d36899da`; `server.md`; 20 unit + 7 socket + 7 persistence + 5 acceptance |
| §8 tests | unit, two clients, same-cell conflict, structural edits with participants, formulas/dynamic arrays, reconnect/catch-up, viewer, 10 000 cells, commit opens in LibreOffice | counts below; `7ed43c2a` |

## 4. Test run after the journaled-undo slice (2026-11-09)

Run for the journaled-undo commit (`d36899da`), on top of the structural undo and
the image chunking:

```text
cargo +1.95.0 test -p gridcraft-collab                     35 passed
cargo +1.95.0 test -p gridcraft-collab-client               9 converge + 4 image-part tests passed
cargo +1.95.0 test -p gridcraft-collab-server --all-targets 20 unit + 7 socket + 7 persistence + 5 acceptance, all passed
cargo +1.95.0 test -p gridcraft-collab-web -p gridcraft-web 5 + 11 passed
cargo fmt --all -- --check                                 clean
cargo +1.95.0 clippy --workspace --all-targets -- -D warnings
                                                          clean
```

Before this slice the same run counted 31 collab, 7 converge, 19 unit and 5
persistence. The wasm32 check of `collab-web`/`gridcraft-web` is unchanged by
these crates and was last run for the web host-bridge slice.

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

### 4.1. Run after the structural-image slice (`8883269d`)

```text
cargo +1.95.0 test -p gridcraft-collab                     38 passed
cargo +1.95.0 test -p gridcraft-collab-client               10 converge + 5 image-part tests passed
cargo +1.95.0 test -p gridcraft-collab-server --all-targets 20 unit + 7 socket + 8 persistence + 5 acceptance, all passed
cargo +1.95.0 check --workspace --all-targets              clean
cargo fmt --all -- --check                                 clean
cargo +1.95.0 clippy -p gridcraft-model -p gridcraft-engine -p gridcraft-collab
  -p gridcraft-collab-client -p gridcraft-collab-server --all-targets -- -D warnings
                                                          clean
```

The three new collab tests and the new converge/persistence test are in §9.
`gridcraft-collab-web`/`gridcraft-web` are unchanged by this slice and were not
run again; their source depends on `Applied` only through the transport, which
keeps the new optional fields.

## 5. RFC §13 open questions — where they stand in the implementation

| # | Question | State |
|---|---|---|
| 1 | delta granularity | cells, not blocks: `applied.delta` lists changed cells with values; a 10 000-cell range is one operation and one delta, split into `applied{more:true}` frames when large (ADR-0013) |
| 2 | command size limit | server accepts messages up to 9 MiB, frames up to 1 MiB (`GRIDCRAFT_COLLAB_*` config, `server.md`); a structural undo/redo image over one frame travels in `applied{more:true}` / `imagePart` parts of 512 KiB before base64 (ADR-0013), assembled by the client before it is applied; the parts carry the whole `{book, images}` payload, so the workbook-level state is not repeated per frame; one operation is bounded to 32 MiB (RFC §4.1) and a step image over it is refused `too_large` before mutation; a single *command* above the message limit is still refused — client-side splitting of a large command is not implemented |
| 3 | structural queue | FIFO per document: operations pass one room mutex in arrival order; `structural` must have `baseVersion == head` and bumps `epoch` |
| 4 | snapshot cost | snapshot is the authoritative `.xlsx` written by the real writer, not a JSON model; chunked resync of 768 KiB per frame (RFC §3.3/§4.6/§9) |
| 5 | host contract | `INTEGRATION.md` §6 implemented in the app: `gridcraft:session`, `session-state`, `presence`, `commit`, `committed`; the server-to-server commit callback is implemented too (`COLLAB_CABINET_URL`, §13); the cabinet-side page is outside this repository |
| 6 | several windows | decided and implemented (RFC §13.6, §12 below): one `actorId` per user, one `windowId` per window; participants and presence are per window, undo/redo stay on the actor |
| 7 | CSV/TSV | collaboration is `.xlsx` only, as in `INTEGRATION.md` |

## 6. Known gaps

1. ~~The cabinet's server-to-server commit callback~~ — implemented (§13): the room posts the
   built `.xlsx` to `COLLAB_CABINET_URL`, stores the cabinet's etag with the snapshot and shows
   its version to every subscriber. The cabinet-side endpoint and its deployment remain outside
   this repository.
2. Structural undo (row/sheet insert-delete) is one operation with whole-sheet
   images (`applied.images`) and the workbook-level state of the same boundary
   (`applied.book`: defined names, structure protection, package and workbook kept
   content including chart sheets, the VBA project, styles and calculation
   settings), each sheet carrying its `preserved` blobs; it is refused as
   `conflict` when any operation came after the step, and an image that cannot be
   applied (empty sheet set, a name scope outside the image, a style table with no
   Normal style, unusable calc settings) is refused before the document changes.
   An image over 512 KiB travels in `imagePart` parts (ADR-0013), so the old 8 MiB
   `too_large` bound is gone; the per-operation bound is 32 MiB (RFC §4.1), and a
   step over it is refused `too_large` before mutation (§10). The theme stays with
   the workbook. The undo *history* is still memory-only, so a restarted room
   starts with empty undo stacks (ADR-0010 §6) — the document is right, the steps
   are not undoable a second time.
3. TLS termination is the ingress proxy's job; the process speaks HTTP/WS.
4. ~~Multiple windows of one user share one `actorId`~~: they share the actor (as decided) and
   differ in `windowId`, so the participant list has one row per window (RFC §13.6, §12); a new
   window of a user does not reset or take away the actor's undo stack.
5. One room mutex per process; document isolation is by `fileId`. Many large
   workbooks need separate workers (ADR-0007).
6. Browser transport is in place (`crates/collab-web`, host bridge); a real
   browser run against the cabinet page and the craft-libs pin removal after
   the upstream merge remain.

## 7. Sheet images in parts (ADR-0013, 2026-11-09)

A structural undo/redo carries every sheet of its step and the workbook-level
state of the same boundary. The old room refused an image over 8 MiB as
`too_large`; it now travels in parts, and the limit is gone:

- `crates/collab/src/parts.rs`: `split_applied(applied, budget)` serialises the
  whole `{book, images}` payload once and cuts the bytes into
  `imagePart{seq,total,data}` frames of 512 KiB before base64 (about 683 KiB on
  the wire, under the 1 MiB frame limit). `version`, `epoch`, `opId`, `sheets`
  and `result` repeat on every part; `book` and `images` only travel inside the
  payload, so the workbook-level state is not repeated per frame; `more` is true
  until the last one.
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

The parts are the wire form only: since `d36899da` a structural undo/redo is a
journal entry of its own (§8), so the whole image is also one line of
`journal.jsonl`, and the next snapshot (operation count, timer or shutdown)
compacts it away with the rest of the tail.

## 8. Undo and redo in the journal (2026-11-09)

An undo takes a version like any other operation, but the server only journaled
`op` messages: a client that reconnected between an undo and the next snapshot
was told it was caught up while the undone cell was still in its replica, and a
restart silently dropped the step the base did not cover. It also put a gap in
the version sequence of the journal, which ADR-0003 §1/§5 require to be
contiguous.

- `JournalEntry.op` is now a `JournalOp`: `Op { op }`, `Undo` or `Redo`
  (`apps/gridcraft-collab-server/src/store.rs`). A client operation is replayed
  through `Room::apply`, so the engine rules and its step of the history run
  again; a reversal is replayed from the answer it produced.
- `Room::replay_reversal` restores that answer: `applied.images` replaces the
  sheets (and revives their keys) or `applied.delta` restores the cells, then the
  counters come from the entry. The undo *history* is never written to the
  volume (ADR-0010 §6): the answer already carries every cell or sheet the
  operation put back.
- Replay is strict about the sequence: the first entry after `base_version` must
  be `base_version + 1`, every next one `+1`, and a reversal needs the room's
  next version. `meta.version` is no longer compared with the replayed head: it
  is the head at the last snapshot, so after a crash it sits at the base while
  the journal is ahead — the old check refused such a directory and dropped the
  session instead of replaying it.
- `Room::forget_history` clears the per-actor stacks after a replay: replaying the
  operations rebuilds something close to them, but a step a later undo took away
  is not in the volume, so a restarted room starts with empty stacks rather than
  offering an undo the document would refuse.
- `ws.rs` journals `undo`/`redo` frames through the same `record` and counts them
  as operations (session `updatedAt`, the `ops` metric): they change the document
  and take a version.
- A structural reversal's journal entry is large: it stores the answer, which for
  a structural step is the whole sheet image (the same bytes `applied` sends,
  chunked on the wire). Compaction matters more with them; the snapshot on the
  operation count, on the timer and on shutdown trims them as before.

Tests:

- `crates/collab/src/tests.rs`: `a_journaled_undo_and_redo_replay_from_their_answers_after_a_restart`
  (a fresh room over the workbook the edit left replays the undo and the redo;
  the counters land on 2 and 3), `a_journaled_structural_undo_replays_its_sheet_image`
  (the inserted sheet is gone, the restored sheet keeps its old key, the epoch
  follows), `a_replayed_undo_is_refused_when_the_journal_skips_a_version`,
  `a_restored_room_starts_with_empty_undo_stacks`.
- `crates/collab-client/tests/converge.rs`:
  `a_peer_that_reconnects_after_an_undo_sees_the_undone_state` (two replicas: the
  peer drops at v1, the author undoes, the peer comes back at v1 and finds the
  cell empty, then a redo reaches both) and
  `a_peer_that_reconnects_after_a_structural_undo_gets_the_sheet_image` (the same
  for a sheet insert undone from an image). Both fail without the journal change:
  the peer keeps the cell or the extra sheet and is told it is caught up.
- `apps/gridcraft-collab-server/tests/persistence.rs`:
  `an_undo_replays_from_the_journal_after_a_restart` (a snapshot compacts the edit
  into the base, the undo is the only journal entry, the server is stopped
  without a flush: the restart serves version 2, commits the undone cell and
  catches a client at v1 up from the journal, not from the workbook) and
  `a_snapshot_after_an_undo_keeps_the_step_in_the_base` (the other direction:
  `flush` compacts the undo into the base, the journal is empty, a restart still
  serves the undone-then-redone value).
- `apps/gridcraft-collab-server/src/store.rs`: a reversal entry round-trips and
  its line carries no step and no before-image, so the history is not in the
  volume.

## 9. Workbook metadata and kept parts in a structural image (2026-11-09)

A structural step already restored whole sheets, but the boundary it belongs to is
bigger than the sheets. Deleting a sheet drops or rescopes its defined names, and
`Sheet::preserved` (form controls, OLE objects, query tables, VML) is skipped in
the sheet's JSON because it is not part of the workbook format: the author's room
kept it in memory, but a peer and a journal replay lost it. The image now carries
it, so an undo/redo after a sheet or row delete restores what the document had at
that boundary on every replica and after a restart.

What the image holds now:

| Piece | Where | Notes |
|---|---|---|
| sheets (cells, formulas, styles, merges, tables, charts, view) | `applied.images[].sheet` | as before |
| a sheet's kept file content | `applied.images[].preserved` | new; base64 blobs of `Preserved` |
| defined names (name, scope, formula) | `applied.book.names` | new; scopes index the restored sheets |
| structure protection + its password hash | `applied.book.protected_structure`, `.structure_password` | new; the hash never the password |
| step boundary state for redo | `Step::before_book` / `after_book` | in the room's memory, bounded as the step history |

- `crates/model`: `WorkbookMeta` (names, structure protection, password hash) and
  `Workbook::workbook_meta()`; `Preserved` and its parts are now serialisable
  (the sheet field itself stays `#[serde(skip)]`, so the workbook format is
  unchanged) and a kept part's bytes go through a shared base64 module.
- `crates/engine`: `Session::restore_structural(sheets, meta)` replaces the
  sheets and, in the same call, the workbook-level state, then rebuilds and
  recalculates once. It validates the image before touching the document: an
  empty sheet set or a name scope outside the image is refused and the caller
  still holds the old document. `restore_sheets` delegates with `meta: None`, so
  old callers and old journal entries keep working.
- `crates/collab`: `Applied.book`, `Step::before_book`/`after_book`,
  `SheetImage::preserved` and `SheetImage::restored()`. `reverse_sheets` restores
  sheets and metadata together; it still refuses as `conflict{retryable:true}` (and
  before any mutation) when the room is not at the step's version.
  `Room::replay_reversal` restores `applied.book` too, so a journaled undo after
  a restart reaches the same names and kept parts.
- `crates/collab-client`: the parts reassemble into an `ImageBundle` and the
  replica applies sheets and metadata through `restore_structural`; if the
  image does not fit the replica the document is left alone and a resync from the
  version before the operation is requested.
- Chunking is unchanged in shape but the payload is the bundle: when the image
  does not fit one frame, the `imagePart` parts carry `{book, images}`, and a
  client that misses a part still applies nothing (ADR-0013).

What deliberately stays outside the image: the theme (a structural step does not change it and
restoring it from a step image would let an undo overwrite settings the step never touched). It is
the only piece of the workbook-level state the image still leaves alone; the package content, VBA
project, chart sheets, styles and calculation settings joined it in §10.

Tests:

- `crates/collab/src/tests.rs`:
  `a_sheet_delete_image_carries_the_workbook_meta_and_the_kept_parts` (a name
  scoped to the deleted sheet and a workbook-scope name come back on undo; the
  sheet's kept part survives an `Applied` round-trip through JSON and a
  `replay_reversal` on a room restored at the step's version),
  `a_row_delete_undo_keeps_the_structure_protection_of_the_boundary`, and
  `a_sheet_delete_undo_after_a_foreign_edit_changes_nothing` (the refusal is
  `conflict`; sheets, names and cells are untouched).
- `crates/collab-client/tests/converge.rs`:
  `structural_undo_restores_the_workbook_metadata_on_every_replica` (author and
  peer see the same defined names before the delete, after it, after undo and
  after redo; structure protection survives a row undo on both).
- `crates/collab-client/tests/parts.rs`:
  `a_chunked_image_carries_the_book_metadata_and_the_kept_parts_to_both_replicas`
  (a 2 MiB image with a kept part, a name and structure protection: two replicas
  apply nothing until the last part and then all three together).
- `apps/gridcraft-collab-server/tests/persistence.rs`:
  `a_structural_undo_replays_the_workbook_metadata_after_a_restart` (a snapshot
  compacts the insert and the name into the base; the delete and its undo are the
  journal; a crash and a restart replay the undo, and the committed `.xlsx` opens
  with `ExtraRef` scoped to `Extra` at the same formula).

Limits: an image in the journal is one JSON line and can be large with kept
parts (the same compaction rules as before apply); the wire chunks it, the
volume does not. The `defined names` of a step are bounded only by the workbook
itself; a workbook with tens of thousands of names makes the journal line and
the `book` part of the image proportionally large.

## 10. Package content, VBA, chart sheets, styles and calc in a structural image (2026-11-09)

The image of a structural step is the whole boundary, and §9 still left the package level out: a
workbook's unmodelled `preserved` content (package and workbook relationships, kept parts, VML,
chart sheets), the VBA project, the style table and the calculation settings travelled only with the
workbook, never with a step. A replica or a replayed journal that had to restore the boundary
therefore restored the sheets and the names but not the rest of it.

What the image holds now:

| Piece | Field | Notes |
|---|---|---|
| sheets (cells, formulas, styles, merges, tables, charts, view) | `applied.images[].sheet` | as before |
| a sheet's kept file content | `applied.images[].preserved` | §9 |
| defined names, structure protection | `applied.book.names`, `.protectedStructure`, `.structurePassword` | §9 |
| package and workbook kept content, including chart sheets | `applied.book.preserved` | new; the workbook's own `Preserved` |
| VBA project | `applied.book.vbaProject` | new; base64 of the blob, never run |
| style table | `applied.book.styles` | new; so every replica resolves the restored cells' style ids the same way |
| calculation settings | `applied.book.calc` | new; mode, iterative limits, precision as displayed, R1C1 |

- `crates/model`: `WorkbookMeta` gains `preserved`, `vba_project`, `styles` and `calc`;
  `Workbook::workbook_meta()` fills them. A missing field means "not carried" — an image written
  before this slice, or a boundary without that piece — and a restore leaves the document's value
  alone, so old journal entries keep replaying. `StyleTable` now compares by its styles: the
  interned-id index is a cache, `#[serde(skip)]`, and a table read back from an image is the same
  table even though its cache starts empty.
- `crates/engine`: `Session::restore_structural` applies the new fields with the sheets. It still
  validates before any mutation and refuses whole (the caller keeps the old document): a name scope
  outside the image, a style table with no Normal style, and calculation settings that cannot be
  used (`maxIterations` 0 or over 32767, a non-finite or negative `maxChange`).
- `crates/collab`: `Room::reverse_sheets` serialises the whole `{book, images}` bundle **before**
  it calls the engine and compares it with `IMAGE_LIMIT` (32 MiB, the per-operation bound of
  RFC §4.1 / ADR-0013 §5). Over the bound the undo/redo is refused `too_large`, not retryable,
  with the document and the version untouched — the same refusal a large sheet image got before the
  parts existed. An image that cannot be serialised at all is refused `internal` the same way.
- `crates/collab-client`: a sequence that announces more parts than the bound allows, or whose
  assembled bytes pass it, is dropped before it is applied; the document and its version stay as
  they were and the replica asks for a catch-up from the version before the first part. Assembly is
  memory only: a broken sequence leaves no temporary file and no partial document.

The `imagePart` payload is unchanged in shape: the parts carry the whole `{book, images}` bundle, so
the new fields do not repeat per frame and a missed part still applies nothing (ADR-0013).

Tests:

- `crates/collab/src/tests.rs`:
  `a_sheet_delete_image_carries_the_package_parts_vba_styles_and_calc` (a chart sheet, its part and
  relationship, a package relationship, a VBA project, a non-default style and iterative calc are
  in the boundary; the undo's `book` equals the boundary, survives the JSON round trip of a journal
  entry, and a replayed undo restores them onto a base stripped of all of it);
  `an_image_over_the_operation_limit_is_refused_before_the_document_changes` (a lowered bound:
  `too_large`, not retryable, no version, no epoch, the inserted sheet still there);
  `a_sheet_delete_undo_after_a_foreign_edit_changes_nothing` now also keeps a chart sheet and a VBA
  project and asserts the refusal leaves them untouched.
- `crates/engine/src/tests.rs`: `restore_structural_refuses_an_image_it_cannot_apply` (a name scope
  outside the image, a style table with no styles, invalid calc: all refused, the document equal to
  what it was).
- `crates/collab-client/tests/converge.rs`:
  `structural_undo_restores_the_package_state_and_vba_on_every_replica` — a real server opened from
  a written `.xlsm` with a VBA project, a chart sheet (drawing and chart included), a non-default
  style and iterative calc; two replicas start from it, insert a sheet, delete it, undo and redo,
  and compare the VBA project, chart sheets, kept parts, calculation settings and style-table size
  at every step.
- `apps/gridcraft-collab-server/tests/persistence.rs`:
  `a_structural_undo_restores_the_package_state_after_a_restart` — a snapshot compacts the insert
  and the name into the base; the delete and its undo are the journal; a crash without a flush and a
  restart replay the undo, and the committed `.xlsx` opens with the VBA project, `Chart1` and the
  boundary's calculation settings and the restored sheet.

Run (2026-11-09, remote build host):

```text
cargo +1.95.0 test -p gridcraft-collab                       40 passed
cargo +1.95.0 test -p gridcraft-collab-client                 11 converge + 5 parts passed
cargo +1.95.0 test -p gridcraft-collab-server --all-targets   20 unit + 7 socket + 9 persistence + 5 acceptance passed
cargo +1.95.0 test -p gridcraft-engine --lib restore_structural_refuses_an_image_it_cannot_apply
                                                              1 passed
cargo +1.95.0 clippy -p gridcraft-model -p gridcraft-engine
  -p gridcraft-collab -p gridcraft-collab-client
  -p gridcraft-collab-server --all-targets -- -D warnings     clean
cargo fmt --all -- --check                                    clean
```

The full `cargo test -p gridcraft-engine --lib` run has one failure,
`io::tests::save_keeps_the_destination_access_restrictions`, which checks the permission bits of a
file under the build host's temporary directory and is unrelated to this slice (the engine I/O code
is untouched); the new engine test passes on its own.

Limits: the style table, kept package parts and the VBA project are now part of every structural
step image, so a step of a macro-heavy workbook is larger on the wire and in the journal; the
32 MiB operation bound is where such a step is refused rather than truncated. The theme stays with
the workbook. The undo history is still memory-only (ADR-0010 §6), so a restarted room starts with
empty stacks.

## 11. Logging in the binary (2026-11-09, TASK.md §6 п.7 / §7)

`log::*` from the library used to be dropped: the binary never installed a logger. It does now
(`apps/gridcraft-collab-server/src/logging.rs`).

- `env_logger` with default features off except `regex`: no colour stack, timestamps from `chrono`
  (already a dependency). One line per record — `2026-11-09T12:54:32.659Z INFO
  gridcraft_collab_server: session opened fileId=1 session=… fresh=true`; no ANSI escape on a pipe
  or in a file; a multiline message is collapsed onto its line.
- Filter from `RUST_LOG`. When unset: `warn,gridcraft_collab_server=info,gridcraft_collab=info`, so
  a start says what the process does while `axum`/`hyper`/`tungstenite` stay at `warn`.
  `RUST_LOG=info` turns everything up, `RUST_LOG=off` silences a healthy run.
- The lifecycle records the library already emitted (`session opened`, `session restored`,
  `session closed`, `periodic snapshot: N sessions`, `snapshot failed`, `journal append failed`,
  `store scan failed`, `session not persisted/restored`) now reach stderr. `main` logs `listening
  on {addr}` with the address after `bind` (`ADDR=127.0.0.1:0` prints the real port) and
  `N sessions snapshotted` at shutdown instead of `eprintln!`. The fatal startup error stays on
  stderr, before and independent of the filter.
- No secret is added to a record: the existing lines carry `fileId`, `sessionId`, `actorId`, a
  version, a code and `storage io/json/xlsx` causes — never the token, the signing key, cell values
  or a path outside `fileId`. `/healthz` and `/metrics` are untouched (`healthz` answered `ok` in
  the run below).

Evidence (remote host, 2026-11-09):

```text
cargo +1.95.0 test -p gridcraft-collab-server -j 8   24 unit + 7 socket + 9 persistence + 5 acceptance + 2 logging
cargo +1.95.0 clippy -p gridcraft-collab-server --all-targets -j 8 -- -D warnings   clean
cargo fmt --all -- --check                           clean

2026-11-09T12:54:32.659Z INFO  gridcraft_collab_server: gridcraft-collab-server listening on 127.0.0.1:18787 (0 sessions restored)
2026-11-09T12:54:34.172Z INFO  gridcraft_collab_server: session opened fileId=1 session=<sessionId> fresh=true
2026-11-09T12:54:34.474Z INFO  gridcraft_collab_server: gridcraft-collab-server: 0 sessions snapshotted
```

`tests/logging.rs` spawns the real binary: with `RUST_LOG=info` stderr gets `listening on`,
`session opened` (the line holds no service token) and, after one operation, `periodic snapshot: 1
sessions`; with `RUST_LOG=off` a running process writes nothing. Unit tests in `src/logging.rs`
hold the default filter (collab at `info`, `axum`/`hyper`/`gridcraft_engine` at `warn`), the
`RUST_LOG` override, the one-line shape and the newline collapse.

## 12. Several windows of one user (RFC §13.6, 2026-11-09)

A person is an actor; a window is a connection of that person. The decision is one `actorId` per
user and a random `windowId` per window, and the implementation keeps operations and undo exactly
where they were: the wire of `op`/`applied`/`undo`/`redo` and the room's per-actor stack are
untouched. Presence and the participant list become per window.

- `crates/collab/src/hub.rs`: `Participant.window_id` (wire `windowId`, omitted when empty);
  the hub keys participants by `(actor, window_id)`, so joining the same window twice is one row
  and another window of the same actor is a second one. `presence(doc, actor, window, …)` writes
  that window's cursor only. `apply`/`undo`/`redo` still take the actor and check the role of any
  of its windows.
- `crates/collab/src/gateway.rs`: `ClientMessage::Hello` carries `windowId` (default empty — an
  older client has one implicit window); `Connection.window_id` fixes the window at the
  handshake; `Leave` and `Presence` address that window.
- `apps/gridcraft-collab-server/src/ws.rs`: the connection table counts `(actor, window)` pairs,
  so the row of a window is removed only when its last connection is gone; a reconnect that
  overlaps the old socket keeps the row. `maxParticipants` counts distinct actors, not windows,
  so a second or third window of somebody already in the session is allowed (RFC §13.6). A
  presence answer is queued for the sender through the same per-connection queue as the peers'
  frames, so every frame is applied in the order the room produced it and an older frame cannot
  blank a window's row on the sender.
- `crates/collab-client`: `Config.window_id` (empty asks the client to make one:
  process id + a per-process counter + Rust's per-process random hash seed); the client sends it
  in `hello` and reuses it after every reconnect, `State.window_id` is how the UI tells its own
  window from the other one of the same actor. `Synced::new` uses it as the operation-id prefix
  (two windows of one actor never reuse an `opId`) and the `own` undo view still keys on
  `applied.actor`, so all windows of one person see the same undo stack.
- `crates/collab-web`: the page's `clientId` is the window id (`hello.windowId`, `State.window_id`);
  `host_participants` emits one row per window with `clientId = windowId` (falling back to the
  actor when a client sent none), so the cabinet's list shows two rows for two windows.
- `crates/ui-egui`: `PresenceView.window_id` and `PresenceView::is_window_of`; the desktop and web
  adapters filter by `(actor, window)` instead of actor, so the other window of the same user is
  painted and listed. The colour stays per actor (RFC §5); the rows are told apart by `windowId`.

Tests:

```text
cargo +1.95.0 test -p gridcraft-collab -j 8          45 passed
  two_windows_of_one_actor_are_two_participants        two rows, presence per window, version stays 0
  a_reconnecting_window_keeps_its_row_and_a_client_without_one_keeps_a_single_row
  a_new_window_does_not_take_the_actors_undo_stack_away
  two_connections_of_one_actor_are_two_windows_in_the_gateway
  a_participant_row_carries_its_window_id_on_the_wire   windowId on the wire, old frame defaults
cargo +1.95.0 test -p gridcraft-collab-client -j 8   13 converge + 5 parts
  two_windows_of_one_actor_are_two_rows_and_share_the_undo_stack
  a_reconnecting_window_keeps_its_row                 same window id: still two rows, never a third
cargo +1.95.0 test -p gridcraft-collab-server -j 8   24 unit + 7 socket + 9 persistence + 5 acceptance + 2 logging
cargo +1.95.0 test -p gridcraft-collab-web -j 8      6 passed
  two_windows_of_one_actor_are_two_host_participants  host list row per window, own other window painted
cargo +1.95.0 clippy -p gridcraft-collab -p gridcraft-collab-client -p gridcraft-collab-server
  -p gridcraft-collab-web -p gridcraft-ui-egui --all-targets -j 8 -- -D warnings   clean
cargo +1.95.0 clippy -p gridcraft --features collab --all-targets -j 8 -- -D warnings   clean
cargo fmt --all -- --check                           clean
cargo check -p gridcraft-web --target wasm32-unknown-unknown   ok
```

No CI file is touched and nothing is pushed to `storytold/*`.

## 13. Commit callback to the cabinet (M87, 2026-11-09)

A commit no longer stops at this process when a cabinet endpoint is configured: the room builds
the authoritative `.xlsx`, posts it to the cabinet and records the version the cabinet stores.

- `apps/gridcraft-collab-server/src/cabinet.rs`: the callback client. Only `http://` (TLS is the
  ingress's job), one request per attempt, `Connection: close`; the body is the shared contract
  `{fileId, bytes(base64), sha256, version, expectedVersion?}`. `{sessionId}` and `{fileId}` in
  the URL are substituted. The token is a secret: `CabinetConfig`'s `Debug` redacts it, and
  neither it nor the document reaches a log line.
- Status handling follows the contract: `200 {version, etag}` is a stored version; `409` is an
  external edit and is never retried; `401`/`403` and `413` are refused at once; `408`/`425`/
  `429`/`5xx`/a lost answer are retried with the same body after 200 ms and 800 ms
  (`COLLAB_CABINET_ATTEMPTS`, default 3).
- `COLLAB_CABINET_URL`, `COLLAB_CABINET_TOKEN`, `COLLAB_CABINET_TIMEOUT_MS` and
  `COLLAB_CABINET_ATTEMPTS` are read at start-up (the `GRIDCRAFT_COLLAB_`-prefixed aliases are
  accepted). Without a URL the old behaviour is unchanged: `CommitReply` has no `cabinet` field
  and subscribers receive nothing extra.
- On `200` the answer is returned in `CommitReply.cabinet`, written to `meta.json` as
  `cabinet: {version, etag}` and broadcast to every socket as
  `committed{version, sha256, cabinet}` (`crates/collab`, `crates/collab-client`,
  `crates/collab-web`). The etag is what the next commit sends as `expectedVersion`; it survives
  a restart and a failure leaves it alone.
- `409` answers the caller `409 cabinetConflict` and sends every subscriber a retryable
  `error{code:"cabinetConflict"}`; the file is not overwritten. A wrong token is
  `403 cabinetUnauthorized`, a refused size `413 cabinetTooLarge`, a spent retry
  `502 cabinetUnavailable`.
- New metrics: `cabinet_published_total`, `cabinet_conflicts_total`, `cabinet_errors_total`.

Tests (`tests/commit_callback.rs`, a loopback cabinet, no real service):

```text
a_commit_is_published_to_the_cabinet_and_every_peer_sees_its_version
a_cabinet_conflict_is_not_retried_and_leaves_the_stored_etag_alone
a_transient_cabinet_failure_is_retried_with_the_same_body
a_refused_token_and_a_size_refusal_keep_their_meaning
a_spent_retry_is_a_gateway_failure_and_publishes_nothing
the_etag_survives_a_restart_so_the_next_commit_still_checks_it
```

`cabinet.rs` unit tests cover the status classes, the exact body, the URL substitution, a
chunked answer and that a `409` is a single attempt. `store.rs` covers the etag round-trip and
a `meta.json` written before this slice.

Run (remote host, `+1.95.0`):

```text
cargo +1.95.0 test -p gridcraft-collab                       45 passed
cargo +1.95.0 test -p gridcraft-collab-client                 13 converge + 5 parts passed
cargo +1.95.0 test -p gridcraft-collab-server                 33 unit + 7 socket + 9 persistence + 5 acceptance + 6 callback + 2 logging
cargo +1.95.0 test -p gridcraft-collab-web                    6 passed
cargo +1.95.0 clippy -p gridcraft-collab -p gridcraft-collab-client
  -p gridcraft-collab-server -p gridcraft-collab-web --all-targets -j 8 -- -D warnings   clean
cargo fmt --all -- --check                                    clean
cargo +1.95.0 check -p gridcraft --features collab --all-targets                        ok
cargo +1.95.0 check -p gridcraft-collab-web -p gridcraft-web --target wasm32-unknown-unknown  ok
```

No CI file is touched and nothing is pushed to `storytold/*`.
