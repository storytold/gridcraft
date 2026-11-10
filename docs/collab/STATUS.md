# Real-time collaboration: status and acceptance evidence

- Date: 2026-11-10
- Branch: `feature/realtime-collaboration` (fork `copoka/gridcraft`). The PR head is
  `8208607f`: the slice that closed the fifteen Major findings of the 17:44Z review (`f51237ea`)
  plus the §20 page, both published by the push of 2026-11-10T20:1xZ. The slice that closes the
  six findings of the 20:32Z review is four commits: `8e76f729` (server and client code, tests,
  and the `rfc.md`/`server.md`/`client.md` corrections), `437f19ee` (§21), `942e0007` (two numbers
  in it) and the commit this page sits in (the same reconnect rule in the browser transport, with
  its test). All **local, not pushed** when this revision was written, because pushing this
  branch is the human step §19 names. The branch is then 64 commits on the base `2c58c769`, 84 files,
  +17223/−34. A commit cannot spell its own hash: the bus record `status/gridcraft` quotes it.
  §19 records the push of 2026-11-10T17:19Z, which published `ae4311ee`, `34e54b81`, `46783412`
  and the nine review-fix commits `d91f838a`…`a2251b0e`; §21 records the push of 20:1xZ, which
  published `f51237ea` and `8208607f`.
- Base: `origin/main` = `2c58c769` at the time of the rebase (the PR's `baseRefOid`; upstream has
  moved since)
- PR: <https://github.com/storytold/gridcraft/pull/3613> — open, **not a draft** since
  2026-11-10T14:56:54Z, `mergeable = MERGEABLE`, head `8208607f`, last update 20:32:23Z
- Gate on this head: none yet. The four workflows created on `2a245f91` (`CI`, `House rules`,
  `Nix`, `SonarCloud`, 2026-11-10T17:19:42Z) all sit in `action_required` with no job started;
  the workflows of the 20:1xZ push (`8208607f`) have not started either. CodeRabbit did review
  that head: review `5848894080` at 20:32:17Z, six inline threads and one finding outside the
  diff, all seven closed by §21. The evidence in this page is the host runs of §17, §18.6, §20.2
  and §21.2, not a CI pass.
- Design: `rfc.md`; server run book: `server.md`; client guide: `client.md`
- Session handoff (project-level, outside this repository):
  `docs/collab/STATUS.md` in the cabinet project directory. This page is the canonical record and
  the evidence a GitHub reviewer can open on the branch; the project handoff keeps only session
  state and points here (§19).

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
`storytold/craft-libs`. On 2026-11-10 the merge had not happened: `main`
(`1b34eba4`) lacks the collaboration messages — `compare/7666e2b1...main` is
`diverged` with `behind_by: 1`, and `crates/web-host/src/lib.rs` there has no
`session` message (§6.6, §16).

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
reported one pre-existing deprecation in
`apps/gridcraft-collab-server/src/metrics.rs` (main's code); §15 removes it, so
the `1.99.1` clippy is now clean and the earlier `-A deprecated` is no longer
needed.

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
| 3 | structural queue | FIFO per document: operations pass that document's own mutex in arrival order (one lock per book since §14; the process registry and the file leases are separate); `structural` must have `baseVersion == head` and bumps `epoch` |
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
5. ~~One room mutex per process~~: every open document has its own lock since §14 — the registry
   and the file leases are process-wide, while the room, the sockets, the journal and the snapshot
   version of a book are behind that book's lock, so a recalculation of one workbook does not hold
   up another's operations, commits or metrics. Isolation stays by `fileId`. Many large workbooks
   *on separate hosts* still need separate workers behind sticky routing (ADR-0007 §3): that is a
   deployment concern, not a lock inside this process.
6. ~~Browser transport and a real browser run~~ — the two-client headless run through a
   cabinet-like host page is done (§16): two iframes of the trunk build join one room, 24
   operations are accepted with no rejection, both replicas converge, presence keeps the
   ADR-0006 pace and the owner's Save publishes the built `.xlsx` to `COLLAB_CABINET_URL`.
   That run found and fixed two host-bridge bugs (§16): `session`/`committed` did not request a
   repaint, so an idle pane joined only when unrelated input drew a frame, and presence was
   bound to frames that arrived only on input. The **craft-libs pin stays**: `storytold/craft-libs`
   `main` does not carry the six collaboration messages — the fork commit `7666e2b1` is one
   commit that `main` lacks (`GET /repos/storytold/craft-libs/compare/7666e2b1...main` →
   `diverged`, `behind_by: 1`; `crates/web-host/src/lib.rs` on `main` has no `session` message).
   Both pins move back to `storytold/craft-libs` after that merge. What a person still has to do
   is listed in §16.

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

## 14. One lock per document instead of one per process (ADR-0007 §3, 2026-11-09)

The server no longer serialises every book through one async mutex. The registry — which sessions
are open and which file each leased — sits behind a short lock; the protocol state of a document
(`Gateway`, sockets, participant windows, journal, snapshot version) sits behind that document's
own lock.

- `apps/gridcraft-collab-server/src/lib.rs`: `Doc {meta, gateway, conns, actors, journal,
  snapshot_version}` and `Entry {file_id, doc}` inside the registry `Inner {docs, leases}`.
  `App::doc` clones the `Arc` out of the registry; the caller locks only that book.
- The message loop (`ws.rs`) reads the document handle and touches the lease in the registry, then
  works under the document lock only: role check and apply, metrics, queuing the `applied` parts
  and the journal entry. `record_locked` replaced `record`: the journal is appended under the same
  lock as the operation that produced it, so two operations of one document cannot reach it out of
  order (the previous two-lock sequence could, and a restart then refused the version gap). The
  periodic snapshot is still spawned after the lock is released.
- `create_session`, `restore`, `close_session` and `drop_session` touch the registry and then the
  replaced document's lock — never both held across an `await`, so a takeover of a busy book does
  not stall the others. `flush` still waits for each document's lock (shutdown must not skip a
  book); `/metrics` and `GET /v1/sessions/{id}` take the document lock and report that book at
  that point, without blocking the rest.
- The lease stays process-wide and is checked before the document lock. A takeover between the
  two leaves a detached copy applying one operation to nobody; the new owner's document is
  untouched and the old sockets already received `session-closed`.

Tests:

- `src/lib.rs`: `a_busy_document_does_not_hold_up_another` — the first book's lock is held (a
  long recalculation): a third session opens, the second book reports and commits, and the busy
  one answers only after its lock is released.
- `tests/ws.rs`: `two_books_are_isolated_and_each_converges` — two client pairs on two sessions;
  both first edits are version 1, each replica sees its own value and never the other book's,
  each commit carries one book, and a second edit on one book leaves the other at version 1 with
  its own value.

Run (remote host, `+1.95.0` where noted):

```text
cargo test -p gridcraft-collab-server --all-targets -j 8
                                      34 unit + 8 socket + 9 persistence + 5 acceptance + 6 callback + 2 logging
cargo +1.95.0 clippy -p gridcraft-collab-server --all-targets -j 8 -- -D warnings   clean
cargo fmt --all -- --check                                 clean
```

The crates below the server are unchanged, so their gates from §13 still stand.

## 15. Host gate under Rust 1.99.1: `try_update` in the metrics histogram (2026-11-09)

`cargo clippy --workspace --all-targets -- -D warnings` on the build host's default toolchain
(`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, the same edition of the compiler CI uses) failed on one
line that predates this branch:

```text
error: use of deprecated method `std::sync::atomic::Atomic::<u64>::fetch_update`:
       renamed to `try_update` for consistency
  --> apps/gridcraft-collab-server/src/metrics.rs:46:33
   = note: `-D deprecated` implied by `-D warnings`
```

`Histogram::observe` accumulates the `_sum` of the op latency histogram; the accumulation must
saturate rather than wrap. The method is now `try_update`, with the same signature and the same
`Result<previous, current>` answer:

```rust
let updated = self.sum_micros.try_update(Ordering::Relaxed, Ordering::Relaxed, |sum| Some(sum.saturating_add(micros)));
debug_assert!(updated.is_ok(), "the sum_micros closure always returns Some");
```

The closure always returns `Some`, so the update cannot be refused: `try_update` yields `Ok(previous)`.
The `Err(current)` arm is unreachable and the check makes that explicit instead of discarding the
answer. In a release build the behaviour is identical to `fetch_update` before (`Relaxed`,
`saturating_add`, one atomic RMW); the existing unit test pins it: `latency_buckets_are_cumulative`
observes 500 µs + 3 ms + 700 ms and requires `latency_seconds_sum 0.703500` (703 500 µs).

Run on the remote host with the default `1.99.1` (2026-11-09):

```text
cargo fmt --all -- --check                                        exit 0
cargo clippy --workspace --all-targets -j 8 -- -D warnings        exit 0, Finished (no errors)
cargo test --workspace -j 8 --no-fail-fast                        every target ok except three
                                                                  host-environment tests below
```

Test totals: `gridcraft-collab` 45, `gridcraft-collab-client` 13 converge + 5 parts,
`gridcraft-collab-server` 34 unit + 8 socket + 9 persistence + 5 acceptance + 6 callback + 2
logging, `gridcraft-collab-web` 6; engine 898 unit + 108 integration, ui-egui 304 unit + 1180
integration, xlsx 364, calc 116, chart 80, functions 164, model 43, numfmt 39, math 23, formula
35, core 40, cli 27 + 11, web 11, pdf 19, mcp 25, xtask 25 and the remaining crates — all passed.

The three failures are not caused by this change (the diff is one file, `metrics.rs`) and are
host-environment tests that also failed before it:

1. `gridcraft-engine --lib`, `io::tests::save_keeps_the_destination_access_restrictions`: the
   build host's temporary directory yields mode `664`, the test wants the destination's
   restrictions (§10). 898 other engine tests pass.
2. `gridcraft-ui-egui --test it`, `interaction::column_autocomplete_folds_case_beyond_ascii` and
   `interaction::language_switch_renders_the_ribbon_and_persists`: the patched egui panics on a
   glyph the host's fonts lack (`ẞ` U+1E9E and `ẻ` U+1EBB); the other 1180 integration tests pass.

No CI file is touched, nothing is pushed to `storytold/*`; the commit stays on
`feature/realtime-collaboration`.

## 16. Two-client host run against a cabinet-like page (2026-11-10)

**Code head `6a9b143c`** on `feature/realtime-collaboration` carries the two host-bridge fixes
this run found. The docs commit that recorded the run is `0b7d6aff`, which was the pushed head of
PR #3613 and of `fork/feature/realtime-collaboration` at the time (superseded by the push of
2026-11-10T17:19Z, §19). The text below was written as a **local** docs commit on top of
`0b7d6aff`, not pushed then; it changed only this file, so the code of the branch was still
`6a9b143c`.

The browser half of §2 and §6.6, run the way `INTEGRATION.md` §6 describes it: a page frames two
GridCraft clients, gives each its room token, opens the file the cabinet holds, and takes the
commit. The page is a local stand-in for the cabinet (it serves the trunk build, asks the room to
build the file and answers the room's callback); the two clients are real browser tabs of the
real web build, driven through `window.gridcraft` (`craft-web-api`), not through the protocol.

**Environment.** Chrome `154.0.8037.92` headless (SwiftShader, `--use-angle=swiftshader`,
`--enable-unsafe-swiftshader`), 560×360 at device pixel ratio 1, `trunk build --release`
(trunk 0.22.1, 28.5 MB wasm) in `apps/gridcraft-web`. `gridcraft-collab-server` on
`127.0.0.1:8087` with a fresh store, HS256 development tokens (`aud:"office-sync"`,
`fileId:7`, `app:"gridcraft"`, one `sessionId` for both clients, roles `owner` and `editor`),
and `COLLAB_CABINET_URL=http://127.0.0.1:8090/api/v1/internal/office/sessions/{sessionId}/commit`
with `COLLAB_CABINET_TOKEN`. The page opened the session through `POST /v1/sessions` and asked
for the file once before the edits (`POST /v1/sessions/{id}/commit`); the room built the blank
workbook (4004 bytes, `PK\x03\x04`) and posted it to the callback, which answered
`{version:41, etag:"gridcraft-cabinet-etag-41"}`. Each pane is
`/index.html?host=parent&webgl&author=Alice|Bob` and received that workbook as `gridcraft:open`
— the cabinet's bytes, not an app-made blank.

**Result.**

| Step | What was observed |
|---|---|
| `gridcraft:ready` | both panes posted it; the page answered `host` and `open` with the cabinet's 4004-byte `.xlsx`; both apps report the workbook through `document.inspect` |
| join | `session-state` `pending` then `synced` in both panes, both listing two windows (`web-…687636177` owner, `web-…231118710` editor) |
| operations | 24 accepted, 0 refused: 12 `cell.set` per pane through `window.gridcraft.execute`, each confirmed by the room's version before the next; room `version=24`; `ops_total 24`, `applied_total 24` |
| convergence | after the last operation both replicas read the same `A1:L2`: every `Alice 1…12` and `Bob 1…12` cell, byte-identical sheets |
| presence | posted on the 100 ms gate of ADR-0006: 8.36 Hz per pane (median interval 110.5 ms) over a 3 s quiet window after the edits; 6.7–6.9 Hz while the operations were being applied (SwiftShader's frame cost bounds the pace on this host) |
| commit | the owner's Save posts `gridcraft:commit {sessionId}` with no bytes; the page calls the room's `POST /v1/sessions/{id}/commit`; the room builds the 4261-byte `.xlsx` (`sha256 614bf3b6…`) and posts it to `COLLAB_CABINET_URL` with `expectedVersion:"gridcraft-cabinet-etag-41"`; the cabinet stores it as version 42 |
| `committed` | both panes show `Committed as version 42 · etag "gridcraft-cabinet-etag-42"`; `commits_total 2`, `cabinet_published_total 2`, `cabinet_conflicts_total 0`, `cabinet_errors_total 0` |

The second callback is the contract's chain: the room sent the etag of the first accepted commit
as `expectedVersion`, so the cabinet could refuse a file changed outside the session.

**Two host-bridge bugs this run found (fixed).**

1. **A host message did not schedule a frame.** `gridcraft:session` and `gridcraft:committed`
   were queued for the next frame but nothing asked for one; an idle app draws no frame, so a pane
   joined only if unrelated input happened to paint. With two panes, one often stayed single-user
   for the whole run (`session-state` never posted), and re-sending the message replaced a live
   transport. `apps/gridcraft-web/src/host.rs` now calls `ctx.request_repaint()` for both
   messages. The join test went from 1 of 2 panes on a bad run to 6 of 6.
2. **Presence was bound to input-driven frames.** The UI posted `presence` from `ui`, and egui
   drew only on input, so the browser pace was ~2 Hz. `SheetApp::logic` now schedules
   `ctx.request_repaint_after(100 ms)` while a session is live (`crates/ui-egui/src/app.rs`), the
   same way the word processor's session does; the measured pace is the ADR-0006 one (table
   above).

The failing pre-fix runs also logged one `Error: closure invoked recursively or after being
dropped` from the wasm glue: dropping a `Transport` closed its socket but left the `on*` closures
registered, and the asynchronous `close` event reached freed wasm code.
`crates/collab-web/src/socket.rs` now takes the callbacks off the socket and drops the closures
in `Drop`; the run above has no page error (only a 404 for the page's missing favicon).

**Gates** (build host `agents`, `scripts/remote-build.sh`): `cargo fmt --all -- --check` exit 0;
`cargo test -p gridcraft-collab-web -p gridcraft-web` green (11 web tests, the collab-web suite);
`cargo clippy -p gridcraft-collab-web -p gridcraft-web -p gridcraft-ui-egui --all-targets --
-D warnings` clean. The wasm32 `clippy -D warnings` still stops on the pre-existing `dead_code`
warning in `crates/ui-egui/src/backstage.rs` (`Modified`), untouched by this slice and present
before it. The branch was pushed at that point: `0b7d6aff` was the head of PR #3613 and of
`fork/feature/realtime-collaboration`, and the PR records that push as an update on
2026-11-09T23:25:26Z (it was superseded on 2026-11-10T17:19Z, §19). The workflows created on that
head (`CI`, `House rules`, `Nix`, `SonarCloud`, 2026-11-09T23:25:33Z) did not run — they sit in
`action_required` with no job started — so that head carries the gates above as run on the build
host, not as a CI pass. The local docs commit that carried this text was not pushed then and
changed nothing outside `docs/collab/STATUS.md`.

**What remains for a person.**

1. Two real machines or profiles over a network, with the real cabinet page (its own tokens,
   iframe policy and `wss://`). This run used one headless browser and two same-origin iframes on
   loopback.
2. The cabinet's own storage: real version/etag numbering, antivirus and the ticket that carries
   the numeric `fileId` in `doc`; the same service token in `COLLAB_CABINET_TOKEN` and in the
   cabinet's callback configuration. A lost `200` is still a `409` until the cabinet makes the
   callback idempotent.
3. The craft-libs pin removal, which waits for `storytold/craft-libs` to merge the six
   collaboration messages (§6.6): on 2026-11-10 `main` did not contain `7666e2b1`.

## 17. A red gate on the eight-megabyte test and what closed it (2026-11-10)

The gate on `0b7d6aff` (run `42720341961`) failed at `cargo nextest run --workspace` on one test:
`gridcraft-collab-client::converge`,
`an_eight_megabyte_sheet_image_travels_in_parts_and_converges`, assertion at
`crates/collab-client/tests/converge.rs:492`, "one applied per edit on the author; rejected []",
`left: 0`, `right: 12`. The author received none of the twelve `applied` frames; the peer received
all twelve; the other twelve tests of the file passed; `fmt` and `clippy` were green.

**The cause was in the test, not in the server.** The test sent twelve 700 KiB `cell.set`
operations, then waited with `settle`, which polls the connection and **throws the events away**,
and only then called `settle_events`, which collects them. `settle` returns when two polls 25 ms
apart see nothing, which is shorter than it takes a loaded runner to hand twelve large frames to
both connections: whatever arrived during that wait was dropped before the assertion counted it.

Reproduced on the `agents` host with the test alone: 5 of 40 runs failed with the same assertion.
Instrumenting `settle` showed the failing runs had already consumed 4 of the author's 12 applied
frames (`applied a=4 b=0`), so the collection that followed saw 8 and the assertion said
`left: 8, right: 12`; on the CI runner the wait consumed all twelve, hence `left: 0`. The frames
themselves were delivered and applied by the client — the transport advances
`state().version` when it queues an `applied` — so nothing on the server or client side was wrong.

**The fix (`34e54b81`).** `settle_events` now takes the version the operation must reach, collects
every frame on the way, and only then requires one quiet round:

- it polls both connections and appends whatever arrived until neither is busy;
- it waits until both replicas are at `version` (12 after the twelve edits, 13 after the insert,
  14 after the undo, 15 after the redo) instead of trusting a fixed quiet window to be long
  enough for the first frame;
- the quiet round after the version catches a trailing frame, so a duplicate `applied` is still
  counted.

The test no longer calls `settle` before collecting, and the assertions are unchanged: twelve
`applied` per replica for the twelve edits, and exactly one for the undo and one for the redo, on
both the author and the peer.

**Evidence (remote host, `34e54b81`).**

```text
converge: an_eight_megabyte_sheet_image_travels_in_parts_and_converges alone, 60 runs   60 ok, 0 failed
converge: whole file, 12 runs with 8 busy CPU hogs                                      13 passed each, 0 failed
cargo test -p gridcraft-collab-client                                    13 + 5 passed, 0 failed, EXIT 0
cargo test -p gridcraft-collab                                           45 passed, 0 failed, EXIT 0
cargo test -p gridcraft-collab-server --all-targets                      34+5+6+2+9+8 passed, EXIT 0
cargo fmt --all -- --check                                               clean, EXIT 0
cargo clippy --workspace --all-targets -j 4 -- -D warnings               clean, EXIT 0
```

Before the fix the same host failed the single test 5 of 40 runs. The fix was a **local** commit on
top of `ae4311ee` when it was written (the PR head was `0b7d6aff` then); the push of
2026-11-10T17:19Z published it in `2a245f91` (§19).

## 18. CodeRabbit's fourteen Major findings on `46783412` (2026-11-10)

Review `5844270617` landed at 15:17Z on the then PR head `46783412`: fourteen comments, all Major.
Every one was checked against the code before anything changed. All fourteen were still valid, so
all fourteen are fixed — each by a commit on top of `46783412` on
`feature/realtime-collaboration`. The commits were local when this section was written; `main` is
untouched and no CI file is changed. The push of 2026-11-10T17:19Z published them in `2a245f91`
(§19).

| # | Finding | What the code did | Commit |
|---|---|---|---|
| 1 | `apps/gridcraft-collab-server/src/lib.rs:466` | `snapshot()` had no per-document exclusion: a capture at v5 could write its base after a capture at v10 had trimmed the journal to 11, and `load_stored` then refuses the directory ("the journal jumps") | `d35fa3e2` |
| 2 | `apps/gridcraft-collab-server/src/store.rs:300` | every write used `{name}.tmp`; two writers of one file (snapshot `meta.json` and a commit's, two commits) could interleave bytes or fail each other's `rename` | `d35fa3e2` |
| 3 | `apps/gridcraft/src/collab_ui.rs:118` | `classify(id)?` answers `None` for `edit.undo`/`edit.redo`, so `Adapter::execute` returned `None` and the engine's local history undid a preview on this replica only | `b4079dae` |
| 4 | `crates/collab-client/src/client.rs:280` | the transport's resend queue kept every operation after `applied`/`rejected`; a reconnect re-sent answered operations, and one outside the dedup window would be applied again | `0ea76265` |
| 5 | `crates/collab-client/src/synced.rs:312` | a refused preview was rolled back with `session.execute("edit.undo")`, which undoes the *newest* local step: with two previews pending, a refusal of the first removed the second | `282f3da9` |
| 6 | `crates/collab-client/src/synced.rs:324` | a snapshot replaced the whole `Session` (`*session = Session::new()`), closing the other open workbooks and resetting their preferences | `1878b640` |
| 7 | `crates/collab-client/src/synced.rs:517` | `target`, remote application and presence all used the *active* workbook; with two open, a local edit went to the room as an operation on a same-named sheet, and a remote operation could be written into the other file | `1878b640` |
| 8 | `crates/collab-web/src/lib.rs:250` | `Synced`'s actor came from the page's display name, not from the server's welcome: with a display name ≠ JWT subject the undo stack stayed empty, and another window of the same actor's command was replayed here | `1878b640` |
| 9 | `crates/collab-web/src/lib.rs:373` | the same `classify` gate as #3, in the web adapter | `b4079dae` |
| 10 | `crates/collab/src/ops.rs:29` | the engine marks a sort structural (`cx.structural = true`) and moves cells, styles, row heights and object anchors, but `classify` ordered it as a merge edit | `d91f838a` |
| 11 | `crates/collab/src/room.rs:233` | a step kept every cell in the delta, dependents the recalculation evaluated included; an undo then required a dependent to still hold its value and refused an edit of a cell nobody touched | `deffb80f` |
| 12 | `crates/collab/src/room.rs:321` | a cell undo's `applied` carried only the step's own list; `restore_cells` recalculated without recording it, so peers patched values only and kept no restored formula or style | `deffb80f` |
| 13 | `crates/engine/src/lib.rs:446` | `CellState` kept value, formula text and style: an undo of a Python cell wrote a constant, and an array formula came back as a single-cell one | `deffb80f` |
| 14 | `crates/ui-egui/src/app/commands.rs:75` | the editor's commit (`cell.set`, `range.fill`) and a form control's value called `session.run` directly: in a live session they stayed local and never reached the room | `5d8cc3c4` |

### 18.1 Server: one snapshot per document, one temporary file per write

`Doc` gains `snapshot_lock: Arc<tokio::sync::Mutex<()>>` and `persisted: bool`. `snapshot()` takes
that lock before it captures the workbook and holds it through the blocking write, the journal
trim and `snapshot_version`; a capture with nothing newer than the base on disk returns without
writing (`persisted && state.version <= snapshot_version`). `write_meta` takes the same lock, so a
snapshot that captured older metadata cannot overwrite a takeover's session id or a commit's
cabinet etag. The lock is separate from the document lock, so the file write still does not stop
the message loop of that document.

`Store::atomic` writes to `{name}.{pid}.{seq}.tmp`, calls `sync_all` on it and then renames: two
writers of one file cannot collide, and a crash still leaves either the old file or the complete
new one.

Tests: `a_snapshot_with_nothing_new_does_not_rewrite_the_base` (a fresh session writes its base
once; a second snapshot with no operation in between writes nothing),
`concurrent_writes_of_one_file_do_not_collide` (8 threads × 20 writes of `seen.json`; every write
returns `Ok` and the file decodes to one writer's value).

### 18.2 Routing: undo/redo, the resend queue, and edits the UI made by itself

`edit.undo` and `edit.redo` stay in the allowlist only to be refused as *commands*; they are
operations of the room. Both adapters now skip the `classify` gate for exactly those two ids, so
`Synced::execute` reaches its `Undo`/`Redo` branch instead of the engine's local history. The
transport drops an operation from its resend queue when the server answers it, with `applied` or
with `rejected`.

The editor's commit and a form control's value go through a new `SheetApp::run_shared`, which asks
the collaboration layer first and falls back to `Session::run` — the same dispatch the ribbon uses.
Draft handling, gesture undo and the local path for non-shared commands are unchanged.

Tests: `undo_and_redo_reach_the_session_not_the_local_history`,
`an_answered_operation_leaves_the_resend_queue`,
`a_committed_cell_edit_goes_through_the_session`,
`a_fill_committed_from_the_editor_goes_through_the_session`.

### 18.3 Client: one document, one person, and a rollback that is the rollback of the refusal

`Synced` records the `DocState::uid` it joined with, resolves its target through that document,
applies every remote operation to it (switching the session there and back when another workbook is
in front), and reports presence for it. An edit made in another workbook returns `None` from the
adapter, so it stays local. A snapshot replaces only that document through the new
`Session::swap_document(uid, state)`, which keeps the `uid` and the window and starts the document's
history empty; other workbooks, their unsaved state and `session.prefs` stay.

The actor comes from the server's welcome (`State::actor`, the JWT subject) and is adopted on the
first poll; whether a command still has to run is decided by the operation id's window prefix
(`{windowId}-{seq}`), not by the actor alone, so another window of the same person is not mistaken
for this one while this window's own preview is not replayed.

A refused preview is rolled back only when it is the newest preview. If a later preview is pending,
the engine's undo would take that one back instead, so the replica asks for the document
(`Resync`) and says so.

Tests: `an_edit_in_another_workbook_stays_local`, `a_remote_operation_lands_in_the_shared_workbook`,
`a_snapshot_replaces_only_the_shared_workbook`, `a_command_less_undo_restores_the_formula_on_a_peer`,
`a_refused_earlier_preview_does_not_undo_the_later_one`,
`a_refused_last_preview_is_rolled_back_locally`, `the_actor_and_the_window_come_from_the_server`.

### 18.4 Undo steps: what the command wrote, and an answer that carries the document

The engine now reports two lists: `take_changed` (as before, the cells whose displayed values moved,
including dependents and spills) and `take_written` (only the cells the command itself wrote). The
room snapshots a step from `take_written` and builds its delta from `take_changed`. An undo
therefore validates only the cells the participant's command wrote — a volatile formula or a spill
that somebody else's edit re-evaluated no longer refuses it — while the answer still carries every
cell whose value moved.

`restore_cells` leaves the restored cells and every formula it re-evaluated in `last_changed`, so the
undo's `applied` is built from the document rather than from the step's own list, and the client
applies a command-less `applied` through `restore_cells` (formula, array flags, Python code and
style) instead of patching values. Spilled children still go through `apply_values`, because their
value lives in the sheet's spill map and not in a cell of their own.

`CellState` and `CellChange` gain `py`, `array` and `dynamic` (all `#[serde(default)]`, so old
journal entries and old deltas still read): a Python cell's code and an array formula's range come
back with the undo, on the author, on every peer and after a restart.

Tests: `an_undo_does_not_wait_for_a_recalculated_dependent`,
`an_undo_delta_carries_the_restored_formula_and_the_dependents`,
`a_cell_state_puts_python_code_and_an_array_formula_back`.

### 18.5 A sort is a structural edit

`data.sort` (and `sortAscending`/`sortDescending`) is in `STRUCTURAL_PREFIXES`. The engine already
treats it as structural — it rewrites formulas, moves row heights and re-anchors objects, then
recalculates — so the room now requires `base_version == head` for it and bumps the epoch, instead
of ordering it against concurrent cell edits addressed with the pre-sort positions.

Test: `the_allowlist_separates_structural_from_merge_and_denies_code` gains the three prefixes.

### 18.6 Gates (remote host `agents`, 2026-11-10)

Every crate the fourteen fixes touch was tested on the build host, on **`a2251b0e`** — the code head
after the formatting pass; the commit on top of it changes only this page. No CI run covered these
commits when the run was made (they were local then; the push of 2026-11-10T17:19Z put them on the
PR head `2a245f91`, whose runs have not started, §19), so this host run is the evidence for them:

```text
cargo test -p gridcraft-collab                                48 passed, 0 failed, EXIT 0
cargo test -p gridcraft-collab-client --all-targets           1 lib + 13 converge + 4 document
                                                              + 5 parts + 2 rollback, 0 failed, EXIT 0
cargo test -p gridcraft-collab-server --all-targets           36 unit + 8 socket + 9 persistence
                                                              + 5 acceptance + 6 callback + 2 logging,
                                                              0 failed, EXIT 0
cargo test -p gridcraft-collab-web                            8 passed, 0 failed, EXIT 0
cargo test -p gridcraft-ui-egui --test it collab_dispatch     2 passed, 1182 filtered out, EXIT 0
cargo test -p gridcraft-engine --lib                          898 passed, 1 failed, EXIT 101
cargo fmt --all -- --check                                    clean, EXIT 0
cargo clippy -p gridcraft-collab -p gridcraft-collab-client
  -p gridcraft-collab-web -p gridcraft-collab-server
  -p gridcraft-engine -p gridcraft-ui-egui --all-targets
  -j 8 -- -D warnings                                         clean, EXIT 0
```

The one engine failure is the host-environment test §10 and §15 already record:
`io::tests::save_keeps_the_destination_access_restrictions` (it checks permission bits under the
build host's temporary directory, which yields mode `664`). The engine code it measures is not
touched by this slice; the other 898 unit tests pass, including every collaboration-facing one
(`restore_cells`, `last_written`, the array/Python round trip).

The whole `--workspace` run was not repeated: the crates above are the ones the diff changes
(`crates/collab`, `crates/collab-client`, `crates/collab-web`, `crates/engine`,
`crates/ui-egui`, `apps/gridcraft`, `apps/gridcraft-collab-server`), and `apps/gridcraft` has no
test binary of its own — its adapter change is exercised through `collab_web`'s and
`collab-client`'s tests of the same code they share.

### 18.7 Answering the review

No finding was rejected: each one named a real path in the code and each is fixed above. All
fourteen GitHub review threads were answered on 2026-11-10 (one reply per comment id, quoting the
fix and its test). When the replies were written the PR head was still `46783412` and the replies
said plainly that the commit hashes were local until the branch was pushed; a reply on a line of an
older commit still shows the discussion resolved by the commit named in its text.

The branch was pushed afterwards (§19): the PR head moved from `46783412` to `2a245f91`, which
carries the nine review-fix commits and this page. The gate on that head has not started — the four
workflows sit in `action_required` — so the gates in §18.6 remain the evidence, not a CI pass.

## 19. The push of 2026-11-10T17:19Z and the state of the runs on `2a245f91`

What §16–§18 recorded as local is published. The PR head and `fork/feature/realtime-collaboration`
are one commit, `2a245f91abab4aeaca1ec6296d3654c93a2d4da2`, and the tree was clean at it. The
branch is 57 commits on `origin/main` (`2c58c769`), 84 files, +16130/−34, and the PR's
`baseRefOid` is the same `2c58c769`.

- **PR state.** Open, `isDraft = false` (ready for review at 2026-11-10T14:56:54Z),
  `mergeable = MERGEABLE`, last update 17:19:57Z. Nothing in `.github/` is touched by the branch;
  `main` is unchanged.
- **Workflows on the head.** `CI`, `House rules`, `Nix` and `SonarCloud` were created at
  17:19:42Z and all four are `completed / action_required`: the fork's pull request runs wait for
  a maintainer to approve them, so no job has started and no check has a conclusion.
- **CodeRabbit.** The review on the new head was still `in_progress` when this page was written
  (started 17:19:59Z). Its previous review — 14 Major findings on `46783412` — is answered and
  fixed above (§18); the PR carries 30 review comments in total.
- **What that means for acceptance.** There is **no CI gate on `2a245f91`**: green here is the
  host evidence of §17 and §18.6 plus the earlier CI run on `0b7d6aff` (`42720341961`, red on the
  converge test that `34e54b81` fixed). A maintainer's approval of the four runs is a manual step,
  as is the CodeRabbit review of the new head; `mergeStateStatus = UNSTABLE` because no check on
  the head has a conclusion, not because a test failed.
- **For a person.** Push this docs commit (it changes only this page), approve the four workflows
  on the fork PR (or push the branch to a branch inside `storytold/gridcraft`, if the maintainer
  prefers that), wait for CodeRabbit's new review and triage it, and only then ask for the merge.
- **Where the record lives.** This page is the canonical record of the work: model, protocol,
  slices, gates and reviews, and a GitHub reviewer can open it on the branch. The project-level
  handoff in the cabinet directory (`docs/collab/STATUS.md`, outside this repository) was reduced to
  a pointer plus session state (run instructions, what is left, the bus log, the host environment)
  on 2026-11-10T17:36Z, so the two pages no longer diverge.

## 20. CodeRabbit's fifteen Major findings on `2a245f91` (2026-11-10)

Review `5846575558` landed at 17:44Z on the PR head `2a245f91`: fifteen inline comments, all Major —
eleven labelled quick win, four heavy lift. Every one was checked against the code before anything
changed, and all fifteen were still valid. All fifteen are fixed by `f51237ea` on
`feature/realtime-collaboration`; none was rejected, and no finding needed an answer that argues it
away. `main` is untouched and `.github/` is not in the diff.

| # | Finding | Quick win / heavy | What was wrong | Where it is fixed |
|---|---|---|---|---|
| 1 | `store.rs:224` | quick win | `read_journal` ignored a torn last line but left the bytes: the next append joined them into one malformed line that is no longer last, and the restart after it refused the whole journal | `Store::read_journal_repairing` rewrites the journal with the entries that parse; `load_stored` calls it before the session appends |
| 2 | `store.rs:243` | quick win | one unreadable `meta.json` made `sessions()` fail, so `App::restore` logged "store scan failed" and restored nothing | the scan logs the directory and moves on |
| 3 | `store.rs:285` | **heavy** | each file was atomic but the snapshot was not: a crash between `base.xlsx` and `seen.json`/`meta.json` left a base a restart would replay onto a second time | `base.<V>.xlsx`, `keys.<V>.json`, `seen.<V>.json` carry their version, `meta.json` is written last as the commit point, readers take the version from it, older files are pruned after it lands |
| 4 | `ws.rs:202` | quick win | the journal tail was read under the document lock, the lock was released, and the connection was re-registered under it again: an operation applied in between went to the peers and the journal only, while `resynced` told the client a head it never received | `catch_up` reads the tail and inserts the connection in one critical section; a tail trimmed past the client's version falls back to the workbook |
| 5 | `web.rs:247` | **heavy** | a repeated `gridcraft:session` built a new client, so operations the room never confirmed could not be resent | `HostSession::same_room` (same session, URL and role) keeps the client and its queue and applies only a new JWT through `renew_token` |
| 6 | `web.rs:245` | quick win | a failed dial cleared `host_session_id` while the previous session stayed live, so Save posted the local replica as `gridcraft:save` and the bridge marked a stale copy saved | a failed dial keeps the session and its id; only the message says the new room was not opened |
| 7 | `collab_ui.rs:36` | **heavy** | a replica joining a room opened from the cabinet's bytes at version 0 was sent nothing — the journal held nothing to replay — so it kept its own workbook while both sides claimed version 0 | `Doc::seeded` records that the room's document came from outside; the transport sends it to a version-0 join |
| 8 | `synced.rs:390` | **heavy** | the rollback of a refused preview with a later preview on top asked for a resync from the current version, and the tail cannot contain an operation that never happened, so the preview stayed | `ClientMessage::Snapshot` asks for the authoritative workbook; the replication path is `resync(from = head + 1)`, which already sends one |
| 9 | `ops.rs:32` | quick win | `data.subtotal` writes total rows and `data.removeDuplicates` compacts rows upwards, both classified as merge edits | both are structural (`STRUCTURAL_IDS`) |
| 10 | `engine/src/lib.rs:1243` | quick win | `restore_cells` rebuilt the whole graph and recalculated the workbook, so one cell undo carried every formula in the book into the delta, the journal line and every peer | it calls `calc.cells_changed` with the restored keys, as a command's commit does |
| 11 | `rfc.md:75` | quick win | the documented allowlist named `format.*` and used `format.bold` as its example, and `format.*` is not allowed — that example is refused `forbidden` | the allowlist, the structural list and the denylist are the ones in `ops.rs`; the example is `home.bold` |
| 12 | `rfc.md:151` | quick win | the message table and the `applied` example described flat fields, while the wire nests the operation under `op` and the answer under `applied`, and `result.delta` is an array | table and examples match the serialized types |
| 13 | `rfc.md:156` | quick win | the table listed `commit`/`commit-result` as WebSocket messages; there are none, such a frame is refused as invalid | the commit is the cabinet's HTTP endpoint, the notification is `committed` |
| 14 | `rfc.md:293` | quick win | `expectedVersion` was documented as the cabinet's file version; the HTTP query checks the room's document version, and the callback sends the previous cabinet etag under the same name | the two values are distinguished |
| 15 | `server.md:28` | quick win | nothing said that the callback URL must be `http://` (an `https://` one stops the start) or that the token and the workbook travel without TLS | the run book states the restriction and the protection the channel needs |

### 20.1 The four heavy lifts

**Snapshot commit point (#3).** The write order was base, keys, seen, meta, each file replaced
atomically. A crash after the base left a new workbook with the old `seen.json`: the restart
replayed the journal onto a document that already contained it, and the dedup window could not
catch the ids because it was still at the old version. The three files now carry the version they
describe and `meta.json` is written last; `load_stored` reads the files `meta.base_version` names,
so a half-written snapshot is invisible. Pruning runs after the metadata is on disk and only
removes the versions it no longer names. The volume from an older build (plain `base.xlsx`) is read
through the fallback until the first snapshot of this build replaces it.

Test: `the_meta_file_is_the_commit_point_of_a_snapshot` writes version 5, simulates the crash by
writing only a version-7 base, and checks that the restart still reads version 5 and that the next
complete snapshot prunes it.

**Torn journal tail (#1).** The reader's tolerance was not enough: the bytes stayed, so the append
after them produced one malformed line with an empty remainder after it, which the reader refuses
because the broken line is no longer last. The repair is a rewrite of the same file from the
entries that parse, done on the load path before the room takes new appends.
Test: `a_torn_journal_tail_is_dropped_before_the_next_append`.

**A repeated session (#5).** The decision is a comparison of the room a window already dialed with
the one the cabinet just sent: session, socket URL and role. When they match, the client is kept and
only the JWT is replaced (`renew_token`, which re-dials with the same window id and resends what the
room has not answered); a different room is a new session. `HostSession::same_room` is the tested
part (`a_repeated_session_names_the_same_room_only_for_the_same_session_url_and_role`); the shell
uses it before it would build a new client, so the pending queue cannot be dropped by construction.

**A version-0 join of a room that has a document (#7).** The room knows something the joining
replica does not: whether its document came from outside. `Doc::seeded` is set when the session was
created from `initialBytes`, when it continued a stored one, and when it was restored from the
volume; it stays false for a room a participant opened as an empty workbook. A join at version 0
into a seeded room takes the ordinary catch-up with `from = 0`, which now answers with the workbook
when the journal has nothing to send. An unseeded room sends nothing, as before, so the first
participant's document still seeds it.

Tests: `a_join_of_a_session_opened_from_bytes_receives_the_workbook` and
`a_join_of_a_room_that_started_empty_gets_no_workbook` in
`apps/gridcraft-collab-server/tests/ws.rs`;
`a_structural_undo_restores_the_package_state_after_a_restart` in `persistence.rs` now consumes the
workbook before its first operation.

**A refused preview the tail cannot repair (#8).** `ClientMessage::Snapshot` is a new client
message: the transport answers it with the base workbook at the head and then `resynced`, through
the same path it takes when a client is outside `[base, head]`. The client uses it exactly where a
preview has to be shed and the operation that has to go may never have reached the journal — a
refusal with a later preview on top. A plain catch-up still uses `Resync`.
Tests: `a_refused_earlier_preview_does_not_undo_the_later_one` (now asserts the snapshot request)
and `the_workbook_a_client_asked_for_replaces_the_refused_preview` in
`crates/collab-client/tests/rollback.rs`; `a_client_can_ask_for_the_whole_workbook` on a socket.

### 20.2 Gates (remote host `agents`, 2026-11-10, on `f51237ea`)

Every crate the fifteen fixes touch was built and tested on the build host. No CI run covers the
commit yet: the four workflows on the PR head wait for a maintainer's approval (§19), so this host
run is the evidence for this slice.

```text
cargo test -p gridcraft-collab                                49 passed, 0 failed, EXIT 0
cargo test -p gridcraft-collab-client --all-targets           1 lib + 13 converge + 4 document
                                                              + 5 parts + 3 rollback, 0 failed, EXIT 0
cargo test -p gridcraft-collab-server --all-targets           39 unit + 5 acceptance + 6 callback
                                                              + 2 logging + 9 persistence + 11 socket,
                                                              0 failed, EXIT 0
cargo test -p gridcraft-collab-web                            9 passed, 0 failed, EXIT 0
cargo test -p gridcraft-ui-egui --test it collab_dispatch     2 passed, 1182 filtered out, EXIT 0
cargo test -p gridcraft-engine --lib                          898 passed, 1 failed, EXIT 101
cargo check -p gridcraft-web --target wasm32-unknown-unknown  ok, EXIT 0
cargo check -p gridcraft --features collab                    ok, EXIT 0
cargo fmt --all -- --check                                    clean, EXIT 0
cargo clippy -p gridcraft-collab -p gridcraft-collab-client
  -p gridcraft-collab-web -p gridcraft-collab-server
  -p gridcraft-engine -p gridcraft-ui-egui --all-targets
  -j 8 -- -D warnings                                         clean, EXIT 0
cargo clippy -p gridcraft-web -p gridcraft-collab-web
  --target wasm32-unknown-unknown --no-deps -- -D warnings    clean, EXIT 0
cargo clippy -p gridcraft --features collab --no-deps
  -j 8 -- -D warnings                                         clean, EXIT 0
```

The one engine failure is the host-environment test §10 and §15 already record:
`io::tests::save_keeps_the_destination_access_restrictions` (permission bits under the build host's
temporary directory are `664`). The engine code it measures is not touched here; the other 898
tests pass, including every restore path this slice changes. The `--workspace` run was not
repeated: the crates above are the ones the diff changes.

### 20.3 Answering the review

All fifteen GitHub review threads were answered on 2026-11-10T18:46Z — one reply per comment id,
quoting the change and its test, and each reply says the commit is local until the branch is pushed
(the same honest wording §18.7 used). Where a finding asked for a mechanism the protocol did not
have — the authoritative workbook on demand — the reply names it: `ClientMessage::Snapshot`, its
server path and the three tests; where a fix is a lock scope rather than a behaviour the reply says
so and names the suites that cover the paths around it instead of claiming a test that cannot be
written deterministically. `rfc.md` and `server.md` were corrected in the same commit, so
the documents a client is built from now describe the wire that exists.

`f51237ea` was local when this section was written, and this section was one docs commit on top of
it that changes only this page. Both were published by the push of 20:1xZ (§21); approving the
workflows on the new head is still the human step, so the gate on it is the host run of §20.2, not
a CI pass.

## 21. CodeRabbit's six findings on `8208607f` (2026-11-10T20:32Z)

The push the human step named landed at 2026-11-10T20:1xZ: the fork and PR #3613 moved to
`8208607f` (the code slice `f51237ea` plus the §20 page on top), `mergeable = MERGEABLE`, not a
draft. CodeRabbit then reviewed the head its own way: review `5848894080` at 20:32:17Z, six
inline threads and one finding outside the diff (7 in total: five Major inline, one Major on
`client.rs`, one Minor on this page). Every one was checked against the code at `8208607f` before
anything changed; all seven held. This section records what closed each of them and the runs that
show it.

The slice that closes them is `8e76f729` (code and the `rfc.md`/`server.md`/`client.md`
corrections) and this page is one docs commit on top of it. Both are **local** when this is
written: pushing the branch is the same human step §19 and §20.3 name, and the fork stays at
`8208607f` until it runs. A commit cannot spell its own hash; the bus record `status/gridcraft`
quotes the docs commit.

### 21.1 What was wrong, and what changed

**A version-zero joiner of a seeded room with edits (`ws.rs:179`, Major).** The first fix on
`2a245f91` sent the workbook only when the journal was empty (`from == 0 && head == 0 && seeded`).
One operation later the journal is not empty, so a replica that holds a different document still
got the tail — the edits replayed onto its own workbook, and both sides then claimed the room's
version. A version-zero joiner of a seeded room now takes the workbook whatever the head is: the
journal only describes what changed *after* the seed, and a replica without the seed cannot apply
it to anything. Test:
`a_version_zero_join_of_a_seeded_room_with_edits_receives_the_workbook` in
`apps/gridcraft-collab-server/tests/ws.rs` (fails on the old condition: the joiner received
`applied` and `resynced`, not a `snapshot`).

**An empty tail below the head (`ws.rs:237-243`, Major).** `resync` caught a tail that *started*
above `sent + 1`, but not a tail a snapshot had trimmed to nothing between `unregister` and
`catch_up`. Then `journal_after(sent)` is empty, the connection is registered and told
`resynced { version: head }` with versions it never received — and no version gap to notice. The
tail and the head are now read under the same lock, and an empty tail below the head is a
`CatchUp::Trimmed` that asks for the base; `u64::MAX` (a client asking for the workbook outright)
is above any head, so `reject_resync` still registers. The decision is a pure function,
`missed(tail_empty, head, sent)`, tested by
`an_empty_tail_below_the_head_is_a_trimmed_journal` in `ws.rs`. The race itself cannot be driven
from a socket test without a hook between the two calls; the state it decides on can.

**A preview made while the requested workbook was in transit (`synced.rs:381-385`, Major).** The
replica asks for `ClientMessage::Snapshot` during normal editing (a refused preview under a later
one, §20.1 #8). The socket is handled in order, so the server runs that request before a preview
the user makes while it travels, and the workbook does not contain that preview. On arrival,
`Event::Snapshot` replaced the document and *cleared* `pending`; the `applied` for the preview then
saw only the operation-id prefix, took itself for a live preview and patched values — a formula or
a style stayed a constant on that replica while every other replica and the room held the formula.
The snapshot no longer clears the queue: it marks every entry `previewed = false`, and
`apply_here` treats an operation as this replica's own only when its pending entry is still
previewed. An operation the transport never queued here keeps the old behaviour (nothing was
previewed). Test: `a_preview_made_while_the_workbook_was_in_transit_is_replayed` in
`crates/collab-client/tests/rollback.rs` — the preview is a formula, the delta carries only its
value, and the assertion is on the formula after the answer.

**The serialized cell value in the RFC (`rfc.md:103`, Major).** The example sent
`"value":45`; `delta_for` serializes `Value` as `#[serde(tag = "t", content = "v")]`, so a number
is `{"t":"Number","v":45.0}`. A client built from the example reads `f64` and fails on every
authoritative delta. The example now shows the real shape, the mapping for text, bool, error and
empty is written next to it, and the paragraph says that unset `Applied` fields are omitted rather
than sent as `null` (`skip_serializing_if`) — the same block also spelled the delta's object lists
`objectsDeleted`, which the wire does not.

**The exception to "a delta or a command is present" (`rfc.md:109-110`, Major).** `Room::reverse_sheets`
answers a structural undo/redo with `Outcome { command: None, delta: None }` and the restored state
in `images` and `book`; the RFC stated the invariant unconditionally, so a client enforcing it would
reject a valid answer and diverge. The rule now carries the exception, says what such an `applied`
holds (`images`, `book`, `sheets`) and that the client must replace the sheets from the images, not
treat the frame as an error; the "delta is mandatory" paragraph names the same exception.

**The reconnect cursor (`client.rs:121-130`, Major, outside the diff).** `State::version` was
initialised from `welcome.version` — the room's head, delivered *before* any catch-up frame — and
the reconnect `hello` sends `state.version` as `lastVersion`. A socket that dropped after welcome
but before the catch-up therefore advertised a version whose operations this replica never got; the
server saw a replica at the head, sent neither tail nor snapshot, and the document stayed behind.
The cursor now starts at `cfg.last_version` (what the replica holds) and only `applied`, `snapshot`
and `resynced` move it; a `welcome` sets the epoch, sheets and participants but not the cursor.
Test: `a_welcome_does_not_move_the_reconnect_cursor` in `client.rs` (a welcome at version 9 leaves
a replica at 3 where it stands and takes epoch 4). The browser transport had the same hole —
`crates/collab-web/src/link.rs` took `state.version.max(welcome.version)` — so it gets the same rule
and its own `a_welcome_does_not_move_the_reconnect_cursor` in `collab-web`, which applies one
operation and then feeds a welcome at version 9: the cursor stays at 1.

**The workflow wording on this page (`STATUS.md:16`, Minor).** The line said the workflows of
`f51237ea` existed but had not started, while two other places said the commit was not pushed. It
now says the workflows are created by the push, which was still the human step when §20 was
written; the push itself is recorded at the top of §21.

### 21.2 Gates (remote host `agents`, 2026-11-10, on `8e76f729`)

```text
cargo xtask ci                                fmt ok, clippy ok, i18n-report ok,
                                              test FAIL: the one host-environment engine test
cargo test -p gridcraft-collab                49 passed, 0 failed, EXIT 0
cargo test -p gridcraft-collab-client --all-targets   2 lib + 13 converge + 4 document
                                              + 5 parts + 4 rollback, 0 failed, EXIT 0
cargo test -p gridcraft-collab-server --all-targets   40 unit + 5 acceptance + 6 callback
                                              + 2 logging + 9 persistence + 12 socket,
                                              0 failed, EXIT 0
cargo test -p gridcraft-collab-web            10 passed, 0 failed, EXIT 0
cargo fmt --all -- --check                    clean, EXIT 0
cargo clippy -p gridcraft-collab -p gridcraft-collab-client
  -p gridcraft-collab-server -p gridcraft-collab-web
  -p gridcraft-web --all-targets -j 8 -- -D warnings    clean, EXIT 0
cargo check -p gridcraft-collab-web -p gridcraft-web
  --target wasm32-unknown-unknown             ok, EXIT 0
```

`cargo xtask ci` stopped at `cargo test --workspace --lib --bins --tests` on
`io::tests::save_keeps_the_destination_access_restrictions` (`mode 664`, the build host's
temporary directory) — the host-environment failure §10, §15 and §20.2 already record, in a file
this slice does not touch; the other 898 engine tests pass. Counts over the four collaboration
packages: 161 passed, 0 failed; before this slice 156 — the five new tests are this slice's. No CI
run covers `8e76f729` yet: the workflows on `8208607f` sit in `action_required` until a maintainer
approves them, and the new head is not on the PR at all until the push.

### 21.3 Answering the review

All six threads were answered on 2026-11-10T20:4xZ, one reply per comment id, each naming the
commit and the test or the exact paragraph, and each saying the commit is local until the branch
is pushed — the wording §18.7 and §20.3 used, which was still true when they were posted; the
push, if it comes before the next pass, is recorded in the bus thread `status/gridcraft`. The
finding outside the diff (`client.rs`) has no thread of its own; its reply is the one on the
`synced.rs` thread, which lists the four code fixes with their tests. Where a finding could not be reproduced by a socket
test (the trimmed-journal race) the reply says so and names the pure decision the test covers
instead of claiming a test that cannot be written deterministically.
