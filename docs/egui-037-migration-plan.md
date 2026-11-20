# GridCraft — egui 0.37 migration plan

Status (2026-10-25): the local migration incorporates main
`52fa4406c1ebff8b260a2922472f03aba198b9cf`. Canonical pins are egui
`6d518fc799681571963842f95b55a6b68431b15a`, winit
`a517b3846ee8457d84ee4975110bfda6797c44dc`, craft-ui/craft-print
`25da1314d443171b4adeaad2cd9454b273f35c6f` and craft-fonts
`b076417b5880bd4154bd22f00695b1d28c6bea83`. The regenerated lock SHA256 is
`426a766291a8246573bf57e3ab3f5f17ff812487582254a32ff5a2ec0db39f27` and passed the dependency-source audit.
`egui037-grid-final-main-ci-20261025-0854` passed all ten stages: i18n-report,
fmt, strict workspace all-target clippy, workspace tests, doctest, assets, layers,
WASM, parity and keytips. Actual totals are 2,463 passed, none failed and one ignored across 35 summaries.
Independent review verified the merged main behavior and the final scroll/accessibility fixes.
The local merge preserves both migration history and the reviewed upstream changes;
final code and dependency bytes match the passing CI. Subsequent status edits are prose only.
Main advanced to `17b9f6ff0125209fe707efc919690632ff49cb0b` after that CI run; this tree does
not include it. The pass above therefore does not qualify the later integration, and PR CI must
pass on the combined tree before it is described as validated.
Native/platform/performance and application E21b qualification remain open; canonical
pins and full CI do not imply that every native contract is qualified.
Human review and explicit publication approval are required. Earlier compiler logs,
source anchors, baseline inventories and proposed steps below are historical records;
this status governs current pins and acceptance. Final commit/source/CI bindings are in
`final-landing/grid-publication0865` in the migration task evidence.

Scope: `storytold/gridcraft` only. The shared `storytold/craft-libs`, egui fork and winit fork are
dependencies of this plan, not files this plan changes. Application document models, commands, undo
semantics, XLSX persistence and UI policy remain owned by GridCraft.

## 1. Baseline, source identity and targets

### 1.1 Exact source baseline

| Item | Revision / value |
| --- | --- |
| GridCraft checkout | `/home/codex/work/CraftApps/egui037-migration-20261022/gridcraft-implementation` |
| GridCraft baseline SHA | `b8771d51fb4da60bd48c13cf313d6d6014dc5e66` (current `main` merge base for this migration; the earlier `ac8da7d4` release baseline is retained as historical evidence below) |
| Workspace version / edition / MSRV | `0.11.1`, edition 2024, declared `rust-version = "1.95"` |
| Lockfile | `Cargo.lock`, format version 4 |
| Current egui family | selected packages at 0.37.0 from canonical6d518fc7; `egui_extras` declared at 0.37 but unused |
| Current winit / wgpu | winit 0.30.13 at canonical a517b384; wgpu 30.0.1 |
| Current accesskit / kittest | accesskit 0.25.1, accesskit_consumer 0.39.1, kittest 0.5.0, accesskit_winit 0.34.1 |
| Current skrifa | 0.44.0, shared by `epaint` and `gridcraft-ui-egui` |
| Shared UI dependency | craft-ui and craft-print both pin canonical 25da1314; the complete 0.37 UI/capture tree is included |
| Shared capture helper | `craft-ui` CL-2 provisional capture slot; GridCraft/DesignCraft adapters are the pilot and precede CL-2F. CL-1 and CL-2F are the shared gate for the eleven craft-ui consumers |
| Target upstream release | egui annotated tag `0.37.0`, peeled commit `841a328ba2dd8f6996beb7189ebb7fc00e668a2a` (2026-10-15) |
| Historical review candidate egui (superseded by current immutable pin) | `aiongg/egui` `craft/0.37` at `a4f9a17f64a6bae61350fb85804c9df0d9865916`; inspect only, never publish this branch |
| Historical review candidate winit (superseded by current immutable pin) | `aiongg/winit` `craft/0.30.13-lang` at `8b35388bd719e8cc86a95971ba0bc026c9a8f058`; published base `storytold/winit` `craft/0.30.13` at `ffae5198770d7730e718043f035a772b7dbc3b1b` |
| Current implementation egui/winit pins |173cc7375d35dc5fbc5b5d0994fdf787f547c5d1 / ae48c756a3b81125eb796c0cc6dccf60990d1181; full P8/E21b and platform/performance acceptance remain separate, explicitly unqualified gates |
| Stock comparison | egui 0.36.2 `49682f8baa058bf49e011035cfbd6e825f88a5ef`, 0.37.0 `841a328b…`; stock 0.37 is a measurement control only, never a production stage or pin |
| Existing shared update | `storytold/craft-libs` PR 199, branch head `b68de8c5f4f1c060904d211a7b1320aa0f7bce78`; open and conflicting at inspection |
| Historical diagnostics | `/home/codex/work/ssd2/task-evidence/egui037-audit-20261022/{fork,stock}/gridcraft.errors` |

The existing audit merge `ec8840af8c5fda695cf195cdd05e5ae078415c07` is not a base: its
`Cargo.lock` still resolves egui 0.36.2 while the source expects 0.37. The PR-199 branch itself has a
0.37 registry lock but no fork/winit pin. Neither is accepted evidence for GridCraft.

`rust-version = "1.90"` is already inconsistent with the current eframe 0.36.2, whose manifest says
Rust 1.95. Cargo only checks the active compiler, so this has not surfaced. The migration must align
the workspace `rust-version` and CI toolchain to 1.95 (or the higher of the selected fork's MSRV),
because 0.37.0's egui/epaint/eframe manifests declare 1.95.

### 1.2 Build targets and entry points

| Target | Package | UI path | Notes |
| --- | --- | --- | --- |
| Desktop native | `gridcraft` (`apps/gridcraft`) | `App::logic` / `App::ui` wrapping `SheetApp` | eframe trait: `fn logic(&mut self, ctx: &Context, frame: &mut Frame)` and `fn ui(&mut self, ui: &mut Ui, frame: &mut Frame)` (plus `raw_input_hook`); `SheetApp::logic(&Context)`/`ui(&mut Ui)` take no `Frame`. The split exists on 0.36.2 and is **not** 0.37-only |
| Web | `gridcraft-web` (`apps/gridcraft-web`) | `WebShell::logic` / `WebShell::ui` wrapping `SheetApp` | wasm32, eframe web runner, WebGPU with WebGL2 fallback, rfd async picker, no TCP control channel |
| UI library/tests | `gridcraft-ui-egui` | `SheetApp::logic`, `SheetApp::ui`, `examples/snapshot.rs` | 85 integration test files (750 `#[test]` in `tests/`) plus 153 in-crate unit tests: 903 `#[test]` attributes in the crate by static count |
| CLI/headless | `gridcraft-cli` | none | no egui dependency; must keep building/tests |
| Shared controls | `craft-ui` (`printing` only) | `printing::State::show` inside dialogs | GridCraft does not use craft-ui docking/tabs/buttons |
| macOS native menu test | `apps/gridcraft/tests/mac_menu_appkit.rs` | AppKit/muda, no egui | must still run on macOS |
| Tooling | `xtask` | `ci`, `layers`, `wasm`, `assets`, `parity`, `keytips` | `cargo xtask ci` is the repository gate |

There are no child `ViewportId`s, deferred viewports, docking areas, audio/playback queues, or
nested workspaces in GridCraft. The single root viewport is the only egui window. `view.newWindow`
is an engine document command, not an egui child window. The plan therefore does **not** add
viewport routing or docking migrations that GridCraft does not use.

### 1.3 Historical pre-migration manifest/feature inventory

The following inventory describes the baseline before implementation. The current root adds
canonical family/winit patches, both shared-library pins are c488caa9, native clipboard is enabled,
and the current lock/pins are recorded above. Baseline absence of patches is not current state.

- Root `Cargo.toml`: workspace resolver 3; `egui`, `egui_extras`, `eframe`, `egui_kittest`,
  `epaint_default_fonts` pins; `skrifa = 0.44`; no `[patch]` section anywhere in the repository.
- `crates/ui-egui/Cargo.toml`: `egui` (workspace, default features), `craft-ui` git revision,
  `skrifa`, Linux-only `zbus = "=5.19.0"`, dev `egui_kittest` with `wgpu` + `snapshot`, dev
  `epaint_default_fonts`, `gridcraft-xlsx` test support, wasm `web-sys`.
- `apps/gridcraft/Cargo.toml`: eframe `default-features = false` plus `default_fonts`, `wgpu`,
  `accesskit`, `wayland`, `x11` at baseline; G1 adds native `clipboard` to preserve the
  0.36 integration-forced OS clipboard. `craft-print` git revision; desktop `rfd`, `arboard`
  (`wayland-data-control` on Linux/FreeBSD), Windows `pollster`, macOS `muda` and
  `fmv-macos-events`, macOS AppKit dev-dependencies.
- `apps/gridcraft-web/Cargo.toml`: `egui` workspace; wasm-only eframe `default_fonts`, `wgpu`,
  `accesskit`; rfd, wasm-bindgen, js-sys, web-sys feature list.
- All other crates: no egui dependency. `xtask/src/layers.rs` already classifies
  `gridcraft-ui-egui` as the only crate allowed `egui`/`wgpu`.
- `.cargo/config.toml` only aliases `xtask`; no rustflags, target overrides or patches.
- No `vendor/` directory, no `[patch]` entries, no nested workspace, no examples or benches with
  egui except `crates/ui-egui/examples/snapshot.rs`.
- CI: `.github/workflows/ci.yml` runs `cargo xtask ci` (fmt, clippy `-D warnings`, workspace tests,
  assets, layers, wasm, parity, keytips). FreeBSD, Nix-evaluation, packaging-lint, release and
  Windows ARM64 workflows exist; none runs an egui snapshot job today.

The 0.37 graph also changes the egui feature contract:

- `egui` and `epaint` no longer enable `default_fonts` by default (`#8583`).
  `gridcraft-ui-egui` must explicitly request `egui/default_fonts` (and update
  `epaint_default_fonts` to 0.37) to make its production fallback policy explicit.
  `egui_kittest`'s normal `egui` dependency inherits `default-features = false`. Only
  kittest's own dev-dependencies enable `default_fonts`/`monochrome_emoji_fonts`, and
  dev-dependencies do not propagate to consumers. Package-scoped kittest/snapshot builds are
  therefore font-empty unless `gridcraft-ui-egui` itself enables `egui/default_fonts` or the
  harness installs pinned fonts. Inspect the resolved feature graph for each production and
  test root; do not infer fonts from kittest's dev-dependencies.
- `egui_kittest` must move to 0.37/`kittest` 0.5; its `wgpu` and `snapshot` features remain.
- `egui_extras` is declared but unused; keep it out of the graph or pin it consistently, but do not
  delete the workspace entry as part of an unrelated cleanup.

## 2. Historical diagnostics and what they already prove

The audit logs from 2026-10-22 (not current acceptance) list five fork errors in GridCraft:

1. `control.rs`: `egui::UserData` no longer exists.
2. `control.rs`: `egui::Event::Screenshot` no longer exists.
3. `dialogs.rs:1969`: `Role::SelectableLabel` no longer exists.
4. `ribbon.rs:146`: `Role::Checkbox` no longer exists.
5. `theme.rs:282`: `FontData::font` is no longer `Cow<[u8]>`; it is `Arc<dyn AsRef<[u8]> + Send + Sync>`.

The stock log adds the same font-family API errors at `theme.rs` 993/1065/1098/1104/1119 (comparison,
`to_vec`, `len`, `Cow` construction). The current source confirms those anchors. These errors are
only the first compilation wall; they do not cover the behavioural work in sections 3–9.

## 3. Actual incompatibilities and design decisions

### 3.1 Root `Ui`, lifecycle and hidden-window logic

GridCraft already uses the correct 0.36/0.37 shape:

- `crates/ui-egui/src/lib.rs:1148` `SheetApp::logic(&Context)` handles control polling, font
  readiness, desktop theme, zoom, close guard and the asynchronous file inbox; the screenshot
  deadline/slot drain moves here in 3.5.
- `crates/ui-egui/src/lib.rs:1270` `SheetApp::ui(&mut Ui)` lays out the title bar, ribbon,
  backstage, formula bar, status bar, tabs, panes and central grid.
- `apps/gridcraft/src/main.rs` `impl eframe::App for App` calls `self.0.logic(ctx)` in `logic` and
  `self.0.ui(ui)` in `ui`; `apps/gridcraft-web/src/web.rs` does the same for `WebShell`.
- `egui_kittest` harnesses call `app.logic(ui.ctx()); app.ui(ui)` through
  `Harness::build_ui_state` (for example `tests/interaction.rs:10`). `examples/snapshot.rs` uses the
  same closure and `harness.render()`.

Do **not** rewrite this into `Context::run_pass`/`root_ui` calls. `Context::root_ui` panics while
`App::ui`/`run_ui` already holds the root `Ui` (`#8535`). Panels are already created on the passed
root `Ui`; `CentralPanel::show(ui, …)` in `SheetApp::ui` stays. Tests keep `Context::run_ui`
(`#8535` keeps it as a wrapper over `run_pass` + `root_ui`).

**Actual lifecycle contract.** Both native (`eframe/src/native/epi_integration.rs::update`) and web
(`eframe/src/web/app_runner.rs::logic`) call `App::logic` and then `App::ui` *inside* the
`Context::run_ui` closure, once per layout pass. A multi-pass frame therefore calls `logic` more
than once. When nothing is painted, eframe calls `App::logic` through `Context::run_logic` instead;
that call leaves events, input time and layout stale and must not consume UI input. After a normally
completed `run_ui`, `current_pass_index()` is zero even after several passes (`end_pass` takes the
output and only `run_dyn` restores the count before the next pass), so it cannot mark hidden work.
`ViewportInfo::visible()` reports minimized/occluded (`None` unknown), not the phase: an
**issued** screenshot command or `ViewportCommand::RequestPaintWhileHidden` forces a full hidden UI
pass and paint. A screenshot merely queued in `logic` has not reached that point (section 3.5).
Use consumed queues, request/slot identity and monotonic deadlines, not a visibility flag or pass
index, to decide background work; input handling belongs in `ui`.

Only these background paths move into `logic`: the control poll (already there), printing feedback
polling, screenshot result drain/deadlines, autosave, the window title and the existing queued file
inbox. Do not move `ribbon_keys`, pointer handling, `ctx.text_edit_focused()` or event-list scans.

Two lifecycle defects must be fixed while the graph changes:

1. **Autosave currently runs in `ui`** (`lib.rs:1331`). Move the decision into `logic`, keep
   `store_view_state()`/`file.save` there, and call `ctx.request_repaint_after(2 s)` while AutoSave
   is on and the active workbook is dirty and has a path. Keep one monotonic `last_autosave` check
   so a forced hidden UI pass (logic called again) cannot save twice. Preserve the existing guards:
   no cell editor, no text-box editor, `.xlsx` path, `last_autosave > 2000 ms`.
2. **Printing feedback is polled twice per frame, and neither path is `SheetApp::logic`.**
   `apps/gridcraft/src/main.rs:43` calls `self.0.poll_printing(ctx)` immediately after
   `self.0.logic(ctx)`, and `SheetApp::ui` calls `poll_printing` again at `lib.rs:1272`. Move the one
   poll into `SheetApp::logic`, delete the `ui` call and delete the desktop app's extra call. The web
   shell has no extra poll. `printing::State::poll` and `print_feedback_seen` remain idempotent; the
   direct calls in `tests/native_print.rs:40,121,124` move to a real `logic` drive, or stay as
   explicit helper tests with a name that says they bypass the lifecycle.

Also move the window-title update (`lib.rs:1342`) into `logic`, next to the close/autosave state.
`ViewportCommand::Title` is valid outside a pass. This makes a control-channel rename while hidden
update the native title when the window returns. Keep `last_title` in `GridState`/`SheetApp` so it
is sent only on change.

**Multiple UI passes and synthetic input.** `Context::run_dyn` takes `RawInput` fields once per
frame, so a second pass sees no pointer/key events and must not replay a click. It can still re-run
non-input side effects. `App::raw_input_hook` runs once per frame, before `run_ui` or `run_logic`,
not once per pass; in a hidden logic-only tick it can run with no UI to consume its events. Do not
dequeue-and-discard a gesture in the hook. Keep one unapplied gesture step in app state and carry it
across repeated hidden logic ticks until a full `ui` pass consumes it (eframe also retains an
unconsumed `RawInput` for the next pass); do not offer the next step while one is unapplied, and
keep press and release as distinct steps rather than collapsing them. While a step is unapplied,
`logic` sends `ViewportCommand::RequestPaintWhileHidden` every frame (the command holds for one
frame) so a UI pass runs. An issued screenshot command requests paint for its own capture, but it
does not make synthetic input run, nor does it issue itself. Audit `SheetApp::ui` for
pass-dependent effects:

- `control::issue_screenshots` runs after every widget but before `collect_screenshots`, autosave
  and the title update; it is the last UI-only command issuer, so no GridCraft widget can request a
  discard after it. It issues once per admitted request (section 3.5). Do not gate it on
  `will_discard()`: that query only reports a discard already requested and cannot predict a later
  one.
- `control::collect_screenshots` is replaced by a logic-side slot drain (section 3.5).
- `poll_printing` moves to `logic` (above).
- `ctx.send_viewport_cmd(ViewportCommand::Title)` and `store_view_state`/autosave become `logic`
  calls.
- `backstage`, dialog and popup state changes are input-driven; the second pass has no events, so
  they do not repeat.
- `grid::show` writes `grid.rect`, `grid.geo`, `grid.cells_rect` and scroll state. These are
  recomputed on every pass from the same `SheetApp`/session; a discarded pass may leave values from
  the discarded layout, but the final pass overwrites them.

The discard-injection hook cannot be a `#[cfg(test)]` field used by `crates/ui-egui/tests/*`:
integration tests compile the library as a normal dependency, so `cfg(test)` is off there. Put the
private `force_discard_once` field and the discard tests inside a `#[cfg(test)] mod` in
`crates/ui-egui/src/lib.rs`, where unit tests can set it and call `ui`/`control` directly; keep
integration tests to observable commands without the hook. Tests:
- click a ribbon command, then a hook requests `request_discard` **after** screenshot issuance in
  that pass → exactly one journal entry/undo step, one title command, one autosave, and the
  screenshot request completes exactly once (its command is appended to the combined pass output;
  no duplicate command is issued);
- a forced hidden UI pass after a completed multi-pass frame still runs autosave/printing once and
  applies queued synthetic input, while a plain hidden logic tick runs no `ui`;
- repeated hidden logic-only ticks with a queued gesture retain one unapplied step (press and
  release still distinct) and a later full UI pass applies it exactly once; a queued-but-not-issued
  hidden/occluded screenshot receives `RequestPaintWhileHidden` and completes once a current size
  is reported (a minimized request waits for restore or reports its timeout);
- `logic` only (no `ui`) with a dirty autosave workbook → exactly one save after the delay and no
  duplicate when a forced `ui` pass follows.

### 3.2 `WidgetType` → `accesskit::Role`

Sixteen call sites use the deprecated `WidgetType` alias:

| File | Sites | Mapping |
| --- | --- | --- |
| `crates/ui-egui/src/backstage.rs` | 5 | `WidgetType::Button` → `Role::Button` |
| `crates/ui-egui/src/border_preview.rs` | 2 | `Button` and `Label` → `Role::Button`, `Role::Label` |
| `crates/ui-egui/src/dialogs.rs:1969` | 1 | `SelectableLabel` → `Role::Button` with `WidgetInfo::selected(Role::Button, enabled, selected, label)`; keep and assert the selected-state claim |
| `crates/ui-egui/src/grid.rs:3421` | 1 | `Button` |
| `crates/ui-egui/src/ribbon.rs` | 6 | `Checkbox` → `Role::CheckBox`; the rest `Button` |
| `crates/ui-egui/src/tabs.rs:73` | 1 | `Button` |

Use `egui::Role` (re-export of `accesskit::Role`), not a local enum. Preserve the existing
`selected`/`enabled` arguments and labels; the border-style items in `dialogs.rs` use
`WidgetInfo::selected` so their checked state survives. Add contract tests against the actual
controls: `Role::CheckBox` with checked state on Dark Mode (the only `WidgetType::Checkbox` site;
the custom-painted ribbon/tab controls are not checkboxes), `Role::Button` plus selected state on
Format Painter and a selected Backstage row, `Role::Button` on `All sheets` and on each sheet tab,
and `Role::Label` on the border preview. The custom-painted sheet tabs currently expose no widget
info, so G1 gives each one `WidgetInfo::selected(Role::Button, …)` with its sheet name before
asserting selection. Compilation alone is not evidence of a role change.

### 3.3 Deprecated IDs

`Id::new` and `Ui::id` still compile in 0.37 but warn, and `cargo xtask ci` denies warnings. Replace
all of them mechanically; the values are identical:

- 46 production `Id::new(` occurrences and 10 test occurrences (excluding `FontId::new`/
  `LayerId::new`), across `grid.rs` (17), `formula_bar.rs` (6), `panes.rs` (5), `paper_view.rs`
  (4), `widgets.rs` (3), `ribbon.rs` (3), and one each in `cell_pictures.rs`, `credits.rs`,
  `dialogs.rs`, `floating_pictures.rs`, `i18n.rs`, `printing.rs`, `tabs.rs`, `text_box.rs`.
- 9 `ui.id()` uses: `tabs.rs:98`, `widgets.rs:149,150,246,327`, `paper_view.rs:470`,
  `ribbon.rs:44`, `grid.rs:867,920` (nine lines; `widgets.rs` contributes four).
- No `UiBuilder::id` or `AsId` uses.

Replace with `Id::unique` and `Ui::scope_id`. Do not change the source values or add salts. The
debug ID tracker in 0.37 is stricter about unstable IDs; a `ui.scope_id().with(...)` under a
reordered row is stable only if the salt is stable. Existing salts already include row/column/
document identity; keep them. Add a focus-retention test: focus the name box, switch language,
open/close the comments pane, and verify focus stays on the name box (not on another field).

### 3.4 Font data API

`epaint::text::FontData.font` changed from `Cow<'static, [u8]>` to
`Blob = Arc<dyn AsRef<[u8]> + Send + Sync>`. `FontData` still derives `Clone` and implements
`PartialEq`/`Debug` itself; the `Blob` type has no `PartialEq`. Tests that want pointer identity
must therefore compare the blobs with `Arc::ptr_eq`, not rely on `FontData` equality.

Concrete changes in `crates/ui-egui/src/theme.rs`:

- `try_load` (`theme.rs:244`): build `FontData::from_blob(blob, *index)` or
  `FontData { index, ..FontData::from_static(bytes) }`.
- `font_file` (`theme.rs:263`): cache `Blob` rather than `&'static [u8]` in the existing
  `Mutex<BTreeMap<...>>`. Return `Option<Blob>`; clone the `Arc` for each `FontData`. This preserves
  the existing “one file loaded once” memory intent and makes `Arc::ptr_eq` meaningful.
- `drop_unparseable_fonts` (`theme.rs:281`): `font_parses(fd.bytes(), fd.index)`.
- `font_definitions_with` (`theme.rs:457`) and `load_fallback` continue to insert `Arc::new(fd)`;
  `font_data` is still `BTreeMap<String, Arc<FontData>>`.
- `crates/ui-egui/src/fonts.rs:355` `bind_family` uses `FontData::from_owned(bytes)`; unchanged,
  except any `.font` field access must use `.bytes()`.

Tests in `theme.rs` must change, not be deleted:

- `fallback_faces_load_each_file_once`: compare `!Arc::ptr_eq(&a.font, &b.font)` instead of
  `a.font != b.font`.
- `unparseable_font_files_are_rejected_not_fatal`: `fd.bytes().to_vec()`, `fd.bytes().len()`.
- `interface_fonts_are_read_once_and_shared_with_egui`: with the `Blob` cache, assert
  `Arc::ptr_eq(&first.font, &second.font)` and `first.bytes() == epaint_default_fonts::UBUNTU_LIGHT`.
  This is stronger than the old `Cow::Borrowed` pointer check and proves egui shares the bytes.
- Keep `drop_unparseable_fonts` as the last line of defence. It is not a vendored patch; it is the
  app's protection against epaint panicking on a corrupt system font (#226), and 0.37 still parses
  faces during `set_fonts`.

Add `egui = { workspace = true, features = ["default_fonts"] }` to
`crates/ui-egui/Cargo.toml` (or enable it on the workspace dependency) and update
`epaint_default_fonts` to 0.37. Without this, `FontDefinitions::default()` is empty in
`font_definitions_with` in a production library graph without another font-enabling dependency.
The consumer's `egui_kittest` dev-dependency does **not** propagate kittest's own
`default_fonts`/`monochrome_emoji_fonts` dev-features, so the test harness must enable
`egui/default_fonts` itself or install pinned fonts.

### 3.5 Screenshot callback — exact lifecycle

Current code (`crates/ui-egui/src/control.rs`):

- `Shots { token, queued: Vec<(token, at_ms, frames)>, pending: Vec<(token, path, Sender, deadline)> }`
  (`control.rs:70`).
- `poll` enqueues a screenshot request and a pending reply (`control.rs:450`).
- `issue_screenshots` (`control.rs:477`) waits 100 ms and three UI frames, then sends
  `ViewportCommand::Screenshot(UserData::new(token))`.
- `collect_screenshots` (`control.rs:493`) scans `Event::Screenshot` events in the next input,
  writes/replies, and expires pending requests after 8 s.
- `selection_glide` and `grid` use `app.shots.in_flight()` to freeze animation while a capture is
  pending.

0.37 replaces both the input event and `UserData` with a one-shot callback. GridCraft adopts the
shared CL-2 completion slot; it does not add a local `SyncSender` channel or a second token map.
The selected topology is CL-2 provisional slot API -> GridCraft and DesignCraft adapters against
that exact revision -> CL-2F API freeze after joint review. The slot owns one result and the
request record owns the slot; the screenshot callback holds only a `Weak` reference to it:

```rust
// Sketch of the shared CL-2 ownership, not an existing API.
let weak = Arc::downgrade(&request.slot); // app owns the strong slot and its waker
let callback = egui::ScreenshotCallback::new(move |image: Arc<egui::ColorImage>| {
    // Short lock only; never encode, write, reply, run commands or touch app state here.
    let Some(slot) = weak.upgrade() else { return }; // canceled/dropped: release image now
    let wake = slot.offer(token, image);            // one result, replaces nothing
    drop(slot);                                     // release the lock before waking
    if let Some(wake) = wake { wake.wake(); }       // slot-owned UI waker, after unlock
});
ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
```

Design:

- `Shots` keeps request identity, path/reply routing, settle counter, 8 s deadline and the
  `in_flight()` query. Replace the `queued`/`pending` vectors with one slot per admitted request
  from `craft_ui::capture` (CL-2 provisional revision); do not add a second channel. The app owns
  the strong slot and the waker; the slot stores at most one `Arc<ColorImage>`, records the
  issuance-time UI logical geometry and geometry epoch, and is released on drop/cancel/timeout.
  The callback holds only `Weak`, unlocks before invoking the slot's waker, and holds no strong
  `Context`; it does not encode, write, reply or request its own repaint. A late callback after
  cancel or app shutdown upgrades to nothing and drops the image without a file or reply.
  Correlation is slot ownership plus the request token; callback order is irrelevant and a late
  image cannot bind to a new request.
- Admission is bounded by **count and aggregate bytes before issuance**, with checked arithmetic,
  not just by the number of slots. Adopt the shared capture defaults: 4 outstanding requests,
  128 MiB per image and 256 MiB aggregate reserved for in-flight/completed images; an app may
  choose stricter explicit limits. Reserve checked `width * height * 4` bytes for the expected
  capture from the current viewport logical size and `pixels_per_point`, rounded up and capped by
  the surface/`max_texture_side` **before issuing**; reject with a clear “screenshot budget
  exceeded” reply when count or bytes would exceed the budget, without evicting earlier requests.
  The reservation and the `CaptureSlot` per-image bound are the largest image `validate_image`
  accepts (one pixel of rounding per axis), so the aggregate bound is strict for retained bytes;
  a viewport with no size yet reserves the per-image maximum and waits for a real size or the
  deadline instead of being refused as an overflow.
  Revalidate the geometry immediately before issuing. Record the issuance-time UI logical
  content/screen geometry and a geometry epoch and bind them to that image; cancel or retry on
  **any** intervening resize/DPI/zoom/surface change before save, even if the value changes back
  to its original. The callback supplies pixels only, so derive scale from the returned image
  dimensions and the recorded logical viewport, and verify aspect/rounding. On completion reject
  an oversized, stale or epoch-mismatched image, release its bytes and reservation, and keep
  retained slot bytes back at zero after drain/shutdown. The renderer/readback transient
  allocation is recorded separately in peak RSS; a post-callback size check alone does not prove
  the peak is bounded.
- `issue_screenshots` runs after every widget but before autosave and the title update, so no
  GridCraft widget can request a discard later in that pass. Issue once per admitted request (one
  callback, one command); `Context::run_dyn` appends each pass's output before it checks
  `requested_discard()`, so a command issued in a discarded pass still reaches the integration.
  Settling counts one real pass per request (`cumulative_pass_nr`, so repeated `ui` calls in one
  pass count once) and skips a pass that has already asked to be discarded; a discard requested
  after issuance can still cost one counted pass, but the image is geometry-validated at drain.
  `request_repaint_after`
  alone wakes `logic` only, and a queued request has not yet issued its paint-requesting command.
  Therefore, while a request is queued or settling **with a capturable size**, `logic` sends
  `ViewportCommand::RequestPaintWhileHidden` on each frame until `issue_screenshots` runs; once
  issued, the `Screenshot` command's `wants_paint` forces the painted frame, so no further hidden
  paint request is sent. A minimized request has no capturable size and sends none: its hidden
  full-frame wait cannot reach issuance, and the restore-triggered pass issues it instead. Also
  keep the 16 ms `request_repaint_after` from both `ui` and `logic` for completion and timeout.
- `collect_screenshots` becomes `drain_screenshots(app, ctx, now)` called from `logic` (not `ui`).
  G1 includes the drain, deadline/timeout handling and the basic exactly-one reply, so its own
  screenshot smoke can pass; G2 owns the adversarial byte-budget/discard stress. For each completed
  slot: validate request identity, size, geometry/epoch and deadline, take the image once, then
  encode/write/reply outside the slot lock. A completed request with no live record drops its image
  without a reply.
- `logic` checks deadlines and replies with the existing error text (`control.rs:521`), updated if
  it still tells users to make the window visible (an issued screenshot command paints a hidden
  window; a queued one reaches issuance through `RequestPaintWhileHidden`). It must not hold a
  slot lock while calling `reply.send`.
- Request IDs use `wrapping_add(1)` and skip zero; never index arrays by token. A reused viewport
  or document does not matter because the slot dies with its request record.
- `in_flight()` remains true while any request record has an unfinished slot, so animation stays
  frozen through a late/queued capture.
- If CL-2's stress tests expose a weakness, fix it in CL-2/CL-2F with the DesignCraft adapter in
  the same review; a GridCraft-local channel is not a fallback.

Tests (`crates/ui-egui/src/control.rs` unit tests or `tests/screenshot_callback.rs`):

1. Build a `Context`, run `issue_screenshots` for a request and inspect the
   `ViewportCommand::Screenshot(callback)` from `FullOutput`; complete it with a 2×2 red image;
   run `logic`; assert one PNG/path reply with `width/height`, and the file bytes decode.
2. Two requests with distinct slots: complete out of order; each reply gets its own image/path.
3. Timeout/cancel/drop: inject a fake clock, cancel one request and time out another, then complete
   both callbacks; assert no second reply, no file write and retained bytes return to zero.
4. Count and byte budgets separately: with an aggregate budget below four 4K captures and a
   per-image budget below one oversized capture, admission stops on bytes before the count; extras
   are rejected once with an error; completed/oversized images release their reservation. Assert
   peak retained bytes, not only slot count.
5. Discard after issuance: a unit-test hook calls `request_discard` immediately after
   `issue_screenshots`; assert exactly one command was issued, the capture completes exactly once,
   and the reply/file is written once. Do not require a post-pass API.
6. Resize/DPI/zoom: change viewport size/scale between admission and issuance, and again between
   issuance and completion including a change back to the original values; assert the geometry
   epoch mismatch retries within the deadline or returns the size/geometry error and releases the
   reservation; never combine old pixels with current geometry. The identity intentionally keeps
   `zoom`/visibility/occlusion, so a bounded retry failure is acceptable instead of comparing only
   the expected physical size.
7. End-to-end `ui.screenshot` through `ControlRequest`/`handle` in a kittest harness with wgpu:
   file and base64 forms, invalid path error, hidden/occluded-window capture with a current size
   (queued admission sends `RequestPaintWhileHidden`, then the issued command paints the hidden
   window), a minimized request that waits for restore and otherwise times out, and no reply
   after drop.

### 3.6 `MouseWheel` event source

0.37 adds the required `source: MouseWheelSource` field to `Event::MouseWheel` (`#8665`). Every
literal constructor must add `source`, preserving the test's intent:

- `crates/ui-egui/tests/it/wheel_zoom.rs`: line events use `source: MouseWheelSource::Unknown`
  (simulated wheel); point/page units and the `Wheel`, `Trackpad` and `Momentum` sources are now
  covered, including a gesture with no `TouchPhase::End`.
- `crates/ui-egui/tests/it/sheet_tabs.rs` and `tests/it/filter_list_scroll.rs`: `Unknown`.

Production matches (`keytips.rs:64`, `grid.rs:2945`) use `..` or a wildcard and need no field
change. Add tests for `Wheel`, `Trackpad` and `Momentum` as well, because the 0.37 wheel smoothing
changes (`#8662`, `#8678`) and the winit W13 source reporting affect the notch counting in
`grid::show` (`grid.rs:591-637`) and Ctrl+wheel zoom. The expected zoom ladder
(`grid::wheel_zoom_step`) and scroll distance must not change silently. `Event::PasteImage` and
`Event::MiddleClickPaste` are new variants; no exhaustive match exists today, but new handlers must
choose explicitly (section 6.4).

### 3.7 Style and geometry pinning

0.37 changes default widget styling (`#8617`, `#8445`): widget `corner_radius` rises from 2/3 to
4/6, button padding from 4/1 to 8/2, and `interact_size.y` from 18 to 20. GridCraft already sets
`button_padding = (6,3)` and `interact_size.y = 22` in `theme::apply` (`theme.rs:804`, values at
831–832), but does not
set widget corner radii. To avoid an unintended restyle:

- In `theme::apply`, set `v.widgets.noninteractive.corner_radius = CornerRadius::same(2)`,
  `inactive = 2`, `hovered = 3`, `active = 2`, `open = 2` (the 0.36 defaults), in both light and
  dark themes. Keep the existing `window_corner_radius = 10`, `menu_corner_radius = 8`, popup
  shadow, and all token colours.
- Do not rely on `Visuals::dark()`/`light()` defaults for anything GridCraft paints with egui
  widgets. GridCraft's custom-painted ribbon/grid already set their own radii.
- `TextEdit::horizontal_align(Align)` is deprecated (`paper_view.rs:489`). Replace with
  `TextEdit::align(Align2)`, preserving vertical top alignment:
  `[LEFT_TOP, CENTER_TOP, RIGHT_TOP][e.section.min(2)]`. Do not use `Align2::LEFT_CENTER`, which
  would move the header/footer text vertically.
- 0.37 TextEdit changes (`#8628` unclipped growth, `#8420` min size, `#8763` focus surrender) affect
  the formula bar, in-cell editor, header/footer editor, dialogs and tab rename. Review
  `tests/in_cell_editor_size.rs`, `in_cell_editor_wrap.rs`, `committed_text_fields.rs`,
  `formula_bar` tests, `page_layout_view.rs`, `sheet_tab_rename.rs` and add a click-away selection
  regression (section 8).

## 4. Root-Ui, multi-pass and logic-only acceptance

| Risk | Observable assertion |
| --- | --- |
| Hidden logic stops | With the window hidden/minimized and AutoSave on, a dirty saved `.xlsx` is saved once after the delay; control `ui.inspect` still answers; a plain hidden tick runs logic only (a forced hidden UI pass is covered separately) |
| Duplicate side effects | A unit-test hook requests `request_discard` after a command **and after screenshot issuance**: one journal entry/undo step; one title command; one autosave; screenshot completes once |
| Root Ui borrow | No `Context::root_ui` call inside `SheetApp::ui`; the passed `&mut Ui` is used for every panel; a debug build does not panic with the root-Ui borrow message |
| Panel order | Title bar, ribbon/backstage, notice, formula bar, status bar, sheet tabs, pivot pane, panes, central panel reserve space in that order at 1200×800 and 640×420; `grid.rect` equals the central panel's inner rect |
| Clip/hit agreement | `Geo` cell positions, `grid.cells_rect`, scrollbars and `ui.cell.click` hit the same cells after resize, zoom (75/100/200%), right-to-left sheets and frozen panes |
| Multi-pass geometry | A pass discarded by a `Grid` sizing pass leaves final `grid.geo` equal to a single-pass render |
| Logic input | A control `ui.key`/`ui.click` while hidden is applied in forced hidden UI passes; one unapplied gesture step is retained across repeated hidden logic ticks with press/release distinct, `logic` sends `RequestPaintWhileHidden` until a full `ui` pass consumes it, and `logic` never scans `Event::Key`/`Pointer` |

Test anchors: `tests/interaction.rs` (harness shape), `tests/interface_scale.rs`, `tests/host_chrome.rs`,
`tests/right_to_left.rs`, `tests/wheel_zoom.rs`, `tests/scroll_position.rs`,
`tests/selection_scroll.rs`, `tests/freeze_handles.rs`.

## 5. Dependency graph, fork and vendoring decisions

### 5.1 Intended GridCraft graph

For suite validation, GridCraft must resolve one copy of each egui-family crate and one winit:

```text
gridcraft (eframe + egui + egui-wgpu + egui-winit + epaint)
  -> eframe 0.37 (approved storytold rev)
  -> egui 0.37 (approved storytold rev)
  -> egui-wgpu 0.37 (approved storytold rev, E2+E3)
  -> egui-winit 0.37 (approved P8 rev: E4/P2b W15P routing + E7/E19 held-key + E21 + E21b/P7b)
  -> epaint 0.37 (approved P8 rev, E5/E8/E11/E12/E13/E14/E15/E16/E20)
  -> ecolor/emath 0.37 (approved P8 rev)
  -> winit 0.30.13 (approved completed rev, W17-W27 + W15P + W27 lifecycle)
  -> craft-ui / craft-print (craft-libs approved rev -> egui 0.37 fork)
```

G1 explicitly enables native `eframe/clipboard`; verify `egui-winit/clipboard` and
`arboard` in the native feature graph. Web features are unchanged. Cross-application
copy/paste must work in the grid, formula bar and in-cell TextEdit, including text from
an independent process and copy back to it; an in-process clipboard is not acceptance.

Add a root `[patch.crates-io]` in `gridcraft/Cargo.toml` for every egui-family package GridCraft
actually resolves: `egui`, `epaint`, `ecolor`, `emath`, `eframe`, `egui-wgpu`, `egui-winit`,
`egui_kittest`, `epaint_default_fonts`, plus `winit`. Use full revision SHAs, never branches. A
dependency's patch section does not propagate; this root patch is required. `egui_extras` can be
patched too if it stays in the graph, but it is currently unused.

Publication rules:

- No `path =` overrides in the published tree; no machine-local patches; no moving branch.
- `cargo metadata --locked` and `cargo tree -i egui`/`-i winit` must show exactly one source per
  package for native and wasm feature sets.
- `cargo check --locked` must work in a clean checkout with network access to the public fork.
- `aiongg` is a review candidate only. The production graph must use the complete
  reviewed/published `storytold` egui **P8** release (P2b async W15P routing, P7 held-key pairing,
  P7b/E21b, P7c's selected #8763/#8748 fixes and justified performance fixes) and the completed
  winit series (W15P and W27 lifecycle). No stock 0.37 or intermediate P stage is a production
  candidate. The final SHAs are **not selected yet**; do not pin a candidate head, branch, old SHA
  or invented SHA. S2 and G1 stay blocked until the approved full SHAs are recorded in 1.1 and
  checked in a clean `--locked` build.
- Keep the old coherent set (current `Cargo.lock` + craft-libs `d27646a5`) as the rollback point.

### 5.2 `craft-libs` PR 199 and issue 198

GridCraft consumes only `craft_ui::printing` today; G1 adds `craft_ui::capture` (CL-2) for
screenshots. PR 199's role/ID/docking/ellipsis edits are relevant to craft-libs itself, not to
GridCraft's source. Integration decisions:

- Do **not** rebase or merge PR 199's tree. Its head `b68de8c` and the audit merge `ec8840a` are
  stale relative to `49190884278f16c9a9f5823d7698a8ffb5deb062`: they omit `craft-platform` and
  revert security, settings, docking and printing fixes. Port only the API-port hunks from
  `8614e4a04fae516fba16c22b31ff1008cb6be10b` plus the relevant README change from
  `1764bca` onto current `4919088`, preserving intervening commits; gate the result with a diff
  check against `4919088` that shows only those intended hunks.
- CL-1 must add the winit patch and a publication-compatible check (registry egui 0.37 build of the
  packaged crate). GridCraft needs the fork pin separately.
- CL-1 and CL-2F are the shared gate for all eleven `craft-ui` consumers at the P8 stage.
  GridCraft and DesignCraft are the CL-2 provisional pilot; both adapters are reviewed before
  CL-2F freezes the API. A consumer already bumped to an incompatible `craft-ui` revision must
  land that bump in the same atomic dependency stage as the egui P8 landing; no mixed
  `craft-ui`/egui graph is acceptable.
- Bump `craft-ui` **and** `craft-print` to one approved craft-libs revision. Leaving `craft-print`
  at `d27646a5` while `craft-ui` moves creates two git checkouts of the same repository and an
  incoherent graph.
- Issue 198 is a language-support proposal, not a merged dependency. Do not import its locale or
  formatting roadmap. GridCraft keeps its own `i18n` table and formula locale; only consume the
  fork APIs the suite actually selects (for example `Context::set_language` on the fork, E17).
- If craft-libs' `printing::State` API changes in CL-1, update
  `crates/ui-egui/src/printing.rs` and `apps/gridcraft/src/print.rs`; the current wrapper already
  isolates GridCraft from most of craft-ui.

### 5.3 Vendored fixes and provenance

GridCraft has no vendor directory. Its current root uses canonical egui/winit Cargo patches;
those dependency selections do not replace these app-local workarounds, which must be preserved:

| Fix | Provenance | Decision |
| --- | --- | --- |
| `graphics.rs` backend selection, no Vulkan by default, FXC for DX12, GL fallback | GridCraft #120, #339, #354 | Keep in `apps/gridcraft/src/graphics.rs`; E2/E3 fork errors make the existing fallback path effective |
| `theme.rs` font parse guard and `drop_unparseable_fonts` | GridCraft #226 | Keep and adapt to `Blob`; add a corrupt-font regression |
| Custom `text_box.rs` wrapper and selection glide | GridCraft UI | Keep the wrapper, its state sync and selection glide; its inner field is built-in `TextEdit` and inherits E21b. Do not replace the wrapper with a bare `TextEdit` |
| `cell_pictures` 64 MiB/4096-entry texture cache, `floating_pictures` size cap | GridCraft image work | Keep; verify texture IDs/uploads under 0.37 |
| macOS AppKit exit workaround in `apps/gridcraft/src/main.rs` `on_exit` | GridCraft #117 | Keep; re-test on macOS; the 0.37 AccessKit adapter may change the teardown |
| `fmv-macos-events`, `muda` pins | macOS file-open/menu | Keep; winit remains 0.30.13 |

Relevant fork patches by provenance:

- **E2+E3** (`9dd799ab`): surface fit within `max_texture_dimension_2d` and returned
  `WgpuError::ConfigureSurface`/`CreateRenderer`; provenance is PdfCraft's vendored egui-wgpu
  (pdfcraft#609, #617, #1042), now in the shared fork. Required by GridCraft #761 and by the
  existing `graphics.rs` fallback/retry logic. Without it, a too-large window or a GL surface
  timeout can panic before the app exists and the fallback never runs.
- **E4** (`66bacf23`) is only the regular-clipboard half: it initializes arboard, leaves
  `smithay: None` and routes text through winit; the candidate keeps arboard for PRIMARY. The
  selected completion is winit **W15P**: SCTK 0.19.2 `PrimarySelectionManagerState` on the existing
  connection/event queue, with per-seat PRIMARY device/source/offer objects and no second regular
  `wl_data_device`. E4's `get_primary_text`/`set_primary_text` route through W15P in the separate
  P2b stage. GridCraft's own arboard handle is used for HTML (`wayland-data-control`); plain-text
  and PRIMARY routing must follow the single-path design in 6.4, with real compositor evidence.
- **E5/E8/E12/E13/E14/E15/E16** (epaint/egui): bidi shaping/order, TextEdit clipping, grapheme
  caret/delete, fallback alignment, line breaking, visual cursor. GridCraft draws CJK/Thai/Hebrew/
  Arabic workbook text and uses an app-managed in-cell editor built on `TextEdit`; preserve and
  test.
- **E7/E19/E10** (keyboard layout): letter keys from Latin layout/physical code. E19 needs winit
  W25 (`KeyEventExtLatinLayout`). GridCraft's keytips and shortcuts must keep working on
  Russian/Arabic/Hebrew layouts and web keyboard events.
- **E17/E18/E20** (language): `Context::set_language`, `UiBuilder::language`, accessible language,
  widget text language. GridCraft's `UiState.language` should drive these; map `PtBr` to `pt-BR`
  rather than the current short `code()` string, and do not duplicate RTL processing in app text.
- **E21** (`a70470b0`): `ImeEvent::DeleteSurroundingBytes` and exact-byte deletion. GridCraft's cell
  editor undo transaction must remain one step; pair with W26/W27.
- **E21b/P7b** (new prerequisite, no candidate commit yet): bounded outgoing IME surrounding text in
  `IMEOutput`, produced by the built-in `TextEdit` and sent by egui-winit through W27
  `set_ime_surrounding_text` on Wayland. GridCraft's acceptance is the complete shared E21b/W27
  contract, not a payload restatement: the owner/context generation is present even when text is
  `Unavailable`; an available-empty document is distinct and sent as an explicit
  `set_surrounding_text("", 0, 0)` plus commit; an owner/unavailable transition clears caches and
  commits the disable/enable boundary before the new owner; the final completed pass, including
  an authoritative `None`, replaces the old `ime.or(self.ime)` accumulation; a current-epoch
  `done` serial mismatch applies incoming edits once and defers outgoing state until a matching
  serial, while an obsolete epoch invalidates the batch; the new <=4,000-byte payload extraction
  must not allocate a full-document clone (existing TextEdit change-detection/undo clones are
  baseline, not this requirement). Preedit is excluded and cursor/anchor are valid UTF-8 byte
  offsets; password input never supplies text. GridCraft's formula bar
  (`formula_bar.rs:321-323`) and text-box/in-cell editors (`text_box.rs:151`) use built-in
  `TextEdit`, so they inherit the producer; they still need focus, deletion+commit,
  deletion-only, preedit, reversed-selection, emoji/combining/RTL, empty-vs-unavailable,
  final-pass `None`, both serial cases, owner generation, committed clear and two-surface tests.
  Custom painters that synthesize `IMEOutput` would need their own model adapter; GridCraft
  currently has none, and must not set the new field to `None` as a port.
- **E1** (`ViewportHooks`): not used by GridCraft (single viewport); do not add a hook.
- **E6** (`request_focus` during IME): relevant to the in-cell editor; test.
- **E9** (`Style::right_to_left`): GridCraft's sheet mirroring is app-level; do not turn on global
  RTL UI style. It would mirror the ribbon/dialogs incorrectly.
- **E11** (vello multithreading): not enabled by GridCraft; no action unless a dependency turns it on.

## 6. Behaviour-specific refactors and tests

### 6.1 Commands, undo and persistence

The engine and command registry do not change. Required checks:

- Every UI action still goes through `SheetApp::run`/`Session::execute`; no direct model mutation
  from the 0.37 API changes.
- `home.bold`, `cell.set`, `edit.copy`/`cut`/`paste`, `edit.undo`/`redo`, `file.save`,
  `file.saveAs` and `file.open` produce the same journal and undo labels.
- A UI click and the control-channel command produce the same one undo step.
- Programmatic empty params never open a dialog (`UiRequest::Dialog` only from menu-style
  invocation).
- Save → close → reopen a synthetic workbook: values, formulas, number formats, styles, merges,
  tables, frozen panes, right-to-left flag, print settings and `UiState` prefs round-trip. Compare
  `sheet.read`, `document.inspect`, and raw XLSX package parts/hashes where stable.
- Autosave in `logic` writes exactly once per change after 2 s; saving while hidden does not touch
  UI state.
- `engine.journal` replay still works; no password appears in journal/errors.

Anchors: `crates/engine/src/cmd/*` (unchanged), `crates/ui-egui/src/lib.rs::run_command`,
`tests/autosave_persist.rs`, `tests/interaction.rs`, `crates/engine/tests/command_sequence_fuzz.rs`.

### 6.2 Fonts, glyphs and text layout

- Font definitions: `theme::font_definitions_with` must still build UI, UI-bold, cell, cell-bold,
  cell-italic, cell-bold-italic, serif, mono, plus loadable script fallbacks and workbook families.
- `FontData` API refactor as in 3.4; keep the `Blob` cache and `drop_unparseable_fonts`.
- 0.37 `MissingGlyphPolicy`: the production `Context` default is Tofu; `egui_kittest` defaults to
  `MissingGlyphPolicy::Panic` and `check_accessibility = true`. GridCraft has 76
  `Harness::builder`/`build_ui_state`/`build_state` call sites in 73 test files (plus the snapshot
  example) and no shared harness module, so the default would make CI depend on installed fonts and
  on names that do not exist yet. `egui_kittest`'s own `default_fonts`/`monochrome_emoji_fonts`
  dev-dependencies do not propagate into GridCraft, so the harness must enable
  `egui/default_fonts` itself or install pinned fonts. G1 must make the suite green **without
  globally disabling the checks**:
  - `crates/ui-egui/tests/common/mod.rs` is the one harness constructor now used by the `tests/it`
    binary and `keytips.rs`. It installs `theme::pinned_font_definitions()` (egui's bundled faces
    plus the pinned craft-fonts input, never the host system scan) through
    `HarnessBuilder::with_context_setup`, marks the test `SheetApp` with
    `SheetApp::use_prepared_fonts()` so the first `logic` pass does not replace the pinned set, and
    leaves `check_accessibility = true` and `MissingGlyphPolicy` at `Panic`. The snapshot example
    and `header_autofit_fonts.rs` deliberately keep the production/system-font path where the
    subject of the test is installed-font behavior.
  - The CI/acceptance font contract is the existing pinned input: set `CRAFT_FONTS_DIR` to the
    craft-fonts checkout (and `CRAFT_FONTS_REQUIRED=1` for release/acceptance). A minimal image
    without it runs the deterministic bundled-face harness; the one test that renders every
    catalog skips `Ja`/`Ko`/`Zh` in that mode and records the gap, rather than relaxing
    `MissingGlyphPolicy` or accessibility globally.
  - Complete all accessible names/roles that the existing test suite exercises as part of G1 (the
    form fields, Name Box/formula bar, Font Size box, tab rename, dialog labels and any other
    unnamed input the strict harness reports). G3 adds the remaining dialog/grid/popup names and
    semantic assertions; it does not own "make G1 green".
  - `allow_missing_glyphs()` or `with_accessibility_check(false)` is not a suite-wide default. A
    specific test may opt out only with a recorded reason, an owner-facing follow-up gate and a
    comment at that call site; it must not be hidden in the shared constructor.
  - `tests/symbol_glyphs.rs` is the installed-face coverage test and is strict in both modes
    (below); do not weaken it.
  - Do not change production font policy from Tofu.
- Fonts come from pinned, OFL-licensed faces in `storytold/craft-fonts`, never from a copy in this
  repository: `BIZ UDPGothic` Regular/Bold (`Jpan,Latn`), `NanumGothic` Regular (`Hang,Kore`),
  `Noto Sans Symbols` Regular and `Noto Sans Symbols 2` Regular (`Zsym`).
  `crates/ui-egui/src/craft_fonts_policy.rs` is the single policy the build script and the tests
  share: exact family/style plus manifest script tags, the barred-family filter (`Adobe*`,
  `Source Han*`, `Source Sans*`, `Source Serif*`, `Noto Sans CJK*`, `Noto Serif CJK*`, with no
  broad `Noto Sans` prefix that would reject the permitted Symbols faces), the required-face check
  and the web allowlist. `CRAFT_FONTS_DIR` is the only input (no fixture path lives in Cargo
  configuration), and without it the generated table is empty and the build is unchanged.
  `CRAFT_FONTS_REQUIRED=1` makes a missing checkout, manifest, row, font file or required face a
  build error.
- Two symbol modes, each asserted exactly:
  - no-font/default: bundled faces must supply `↘ ◀ ↑ 📋` and the rest of the interface set. `✕`,
    `✓` and `⌃` have no bundled egui face, so chrome paints `Icon::Close`/`Icon::Check` and the
    Control-key tooltip falls back to `^`; workbook text still refuses to fake a glyph.
  - extended acceptance (`CRAFT_FONTS_DIR`, `CRAFT_FONTS_REQUIRED=1`): the exact union
    `↘ ✕ ◀ ↑ 📋 ✓ ⌃` in both `Proportional` and `CELL`, plus the Korean interface labels, must
    render; a loaded or required set that lacks a face fails instead of skipping.
- Native builds embed **every non-barred row** of `fonts/manifest.txt`, not only the five required
  faces: permitted `BIZ UDMincho`, `Shippori Mincho`, `Noto Sans Arabic`, `Noto Sans Georgian`,
  `Noto Sans Hebrew` and `Noto Sans Thai` rows are also present when the pinned checkout carries
  them. All are OFL/non-barred, so the asset policy holds; the policy and size gate must describe
  the actual set, not claim five faces.
- Simplified Chinese is a native-only opt-in: the OFL `LXGW WenKai Lite` row from craft-fonts'
  `fonts/simplified-chinese-manifest.txt` (the face DeckCraft uses). It is outside `WEB_ALLOWED`
  and never enters the wasm bundle; `CRAFT_FONTS_REQUIRED=1` native builds require the manifest,
  while ordinary builds tolerate an older checkout without it. System Han faces still lead the
  chain when present. The native font set (including this face) needs its own raw/compressed size
  measurement; the current measured figure is the five-file web allowlist, 12,970,840 bytes raw.
- Web (`wasm32`) embeds only `BIZ UDPGothic` Regular/Bold, `NanumGothic` Regular and the two
  Symbols faces. Raw font bytes, raw wasm and compressed wasm still need measuring against the
  consumer's real hosting limit; do not record "within budget" before that measurement.
- Font rebuild gate: 0.37 applies `set_fonts` immediately when no text has been laid out this pass
  (`#8602`, `#8609`). GridCraft's one-frame `fonts_ready` skip remains correct but may now skip a
  frame unnecessarily. Re-test language switch, workbook-font discovery and installed-font rebind;
  if the extra frame is observable, gate only when text already exists.
- 0.37 `#8512` system-font discovery/provider: GridCraft has its own `fonts::start_scan` and
  `theme` candidate probing. Do not switch to `egui_system_font_provider` without measuring
  startup, preserving workbook font names, and keeping fallback order. This is a blocking
  comparison, not an automatic adoption.
- Text shaping: E5/E12/E13/E16 change RTL rows, caret/selection and grapheme deletion. Add tests for
  mixed LTR/Arabic/Hebrew in a cell, visual order, caret at the visual end, Delete/Backspace over a
  combining cluster, and formula-bar caret. Use installed fallback faces and skip only when the face
  is absent, as `theme::fallback_covers_its_script` already does.
- E14 fallback alignment and E15 line breaking: the defaults are intended to preserve the old
  rules, but compare wrapped cells, CJK/Indic fallback rows and vertical alignment against the
  baseline. A changed fallback baseline must be explained, not accepted as a new golden.
- Screenshot/font fixtures: record the exact font files and hashes used for every visual evidence
  run. Do not commit font files or screenshots of Microsoft software.

### 6.3 Accessibility and input

0.37 makes unnamed input widgets a kittest failure (`#8589`) and improves popup/menu/panel trees
(`#8581`, `#8607`, `#8608`, `#8612`). GridCraft has many custom-painted controls and several
TextEdits without labels. The names and roles exercised by the existing harnesses land in **G1**
with the shared strict harness; the broader coverage below lands in G3. Required work:

- `crates/ui-egui/src/text_box.rs`'s text-box editor uses the translated `Text Box` accessible
  name in every catalog; the `i18n` unit test checks it alongside the Name Box, Cell Editor, Font
  Size and Sheet Name names.
- `crates/ui-egui/src/dialogs.rs::form_fields`: wrap each label + field in
  `ui.label_inputs_by(label.id, |ui| …)` or call `.labelled_by(label.id)` on the field. Do this for
  text, password and number fields.
- `crates/ui-egui/src/formula_bar.rs`: give the Name Box and formula bar explicit
  `accessible_name`/`labelled_by`; keep their existing `Id`s.
- `crates/ui-egui/src/ribbon.rs::size_combo`: the TextEdit has no label; add
  `.accessible_name("Font Size")` (translated) and keep `control_keytip`.
- `crates/ui-egui/src/tabs.rs`: the tab rename TextEdit gets the sheet name as its accessible name.
- `crates/ui-egui/src/widgets.rs`: icon-only controls already use `on_hover_text`, which 0.37 maps
  to a description/name; verify with `unnamed_widgets()` rather than assuming.
- `crates/ui-egui/src/grid.rs`: scrollbars, freeze handles, header dividers and fill-handle areas
  are custom interactions. Give each a role/name only where it is a real control (scrollbar
  `Role::ScrollBar`; other clickable overlays `Role::Button` with a name), and keep decorative
  overlays out of the tree. Do not invent `Role::Unknown` nodes for every painted rect.
- Popup/menu roles: 0.37 exposes menu items/submenus; ensure context menus, filter menus, fill menu
  and cell context menu have correct names and that hidden popups disappear from the tree.
- Focus/keyboard: Tab/Shift+Tab, Enter/Space, Escape and arrow keys must trigger the same commands
  as pointer clicks; focus must not move when language/theme changes or a pane is hidden.
- IME: preedit, commit, cancel and replacement produce one edit/undo step. Test E21 incoming
  delete-surrounding and the complete E21b contract (section 5.3) in the formula bar and
  `text_box.rs`: owner/context generation even when unavailable, available-empty vs unavailable,
  committed cache clear/disable-enable on owner transition, final-pass `None` authoritative,
  current-epoch serial mismatch applying incoming edits once while outgoing state is deferred, and
  obsolete epochs invalidating the batch; plus deletion-only batches, delete+commit, preedit
  exclusion, reversed selection, emoji/combining/RTL byte indices, focus loss/password clearing and
  two windows. Native IME behavior (including a Wayland Vietnamese/predictive trace) still needs
  real Windows/macOS/Linux evidence; E21 alone is not surrounding-text acceptance.

Accessibility acceptance:

- `Harness::unnamed_widgets()` is empty for the main window and each reviewed dialog.
- Named inputs expose role, value, selected/checked, disabled and focus states.
- A deliberate mutation (remove one name) makes the check fail; revert it before landing.
- Screen-reader native smoke test on Windows/Linux (Orca/AT-SPI) if available; otherwise record the
  gap.

### 6.4 Clipboard, PRIMARY and files

- **One routing design for two clipboard owners.** GridCraft's `Services` owns an arboard handle
  for HTML and plain text (`apps/gridcraft/src/main.rs:150-163`; `wayland-data-control` on
  Linux/FreeBSD), while egui/egui-winit owns the text
  clipboard used by `TextEdit` copy/paste. After E4/W15P both must not compete for the same Wayland
  selection:
  - Grid copy/cut keeps using the one arboard data-control handle, because winit's selected
    clipboard API has no multi-MIME HTML offer; it publishes HTML+plain text. TextEdit copy/paste
    and PRIMARY go through egui/egui-winit (and W15P for PRIMARY). GridCraft must not create a
    second arboard handle or a second `wl_data_device` for the plain-text path.
  - Paste reads HTML first through arboard, then the current plain text (arboard or the egui
    clipboard), with one defined owner order and no stale re-read. A grid copy followed by a
    TextEdit paste, and a TextEdit copy followed by a grid paste, must both work on Wayland.
  - Create the arboard handle lazily and keep it owned by `Services` for the app's lifetime; record
    which protocol each path uses, and prove that an HTML paste from a browser and a plain-text
    paste from another app each work after the other path has taken ownership.
  - Password fields never publish text or PRIMARY; no unchanged-selection reclaim; canceled/slow
    transfers must not leave a half-published owner.
- PRIMARY (W15P): `#8428` makes Label/TextEdit selections publish PRIMARY and middle-click paste
  into a TextEdit. The contract is the winit bounded-transfer one: a fresh triggering serial from
  **either** a pointer press/release or keyboard input on the focused seat (never a keyboard-only
  serial rule and never an arbitrary first seat); synchronous ownership claim on that input turn
  while only later serving/reads are asynchronous; bounded async reads with a 2 s total deadline,
  16 MiB cap, at most 4 read workers, 2 writer threads and queue depth 8; a live owner-specific
  completion waker; canceled or stale results discarded; password fields never publish. Test:
  select formula text → middle-click in the in-cell editor at a position → text is inserted
  there, one undo step; pointer and keyboard triggers, focus movement, owner/seat changes,
  empty/absent offers, self-paste, trickle/oversized/canceled transfers and event-loop
  responsiveness; non-PRIMARY platforms unaffected. Native gates are wlroots/Hyprland, KWin and
  Mutter, not one compositor only.
- `Event::PasteImage` and `Event::MiddleClickPaste` are new. GridCraft deliberately ignores both in
  the grid: there is no image-clipboard pipeline or cell-picture insert-from-clipboard command, and
  inserting at an unchosen anchor/size would be a new feature. A clipboard with a text
  representation still arrives as `Event::Paste`; PRIMARY middle-click paste belongs to egui's
  TextEdit. The match arms are explicit and commented, so a future image-paste feature must choose
  behavior rather than inherit a silent no-op.
- File dialogs (rfd), drag-and-drop and `fmv-macos-events` are native workflows; kittest cannot
  prove them. Use the Windows clone and a macOS session with real evidence. The web picker uses the
  async inbox; test cancel/retry with two dialogs so an inbox cannot cross-deliver.

### 6.5 Viewports, GPU and screenshots

- One root viewport only. No `show_viewport_deferred`, no child-window title/focus/close tests.
- `ViewportCommand::Screenshot` is the only viewport command affected by the callback change; the
  other commands (`InnerSize`, `Focus`, `Close`, `CancelClose`, `Title`, `StartDrag`, `Maximized`)
  stay.
- GPU: preserve `cell_pictures`/`floating_pictures` texture caches. 0.37's texture manager and
  `max_texture_side` behavior must not change image sizes or leak textures. Test texture IDs,
  upload deltas, eviction at 64 MiB, images larger than `max_texture_side`, and drop on workbook
  close.
- E2/E3: verify the Windows fallback still triggers on a failed renderer/surface. The fork's
  `surface_fit` test is a dependency test; GridCraft adds a visible large-window/resize workflow on
  the owned Windows clone where the adapter limit permits it.
- 0.37 color emoji changes texture sampling. Keep `TextureOptions` semantics for cell pictures and
  icons (nearest/linear, wrap mode); do not accept a clamp-only optimisation as a replacement for
  Repeat/MirroredRepeat.
- An **issued** screenshot command requests a real paint of a hidden window: the candidate marks
  `ActionRequested::Screenshot` (and `RequestPaintWhileHidden`) as `wants_paint`, and eframe treats
  that as visible for painting. But issuance is UI-only, so a hidden window must first be made to
  run `ui`: while a request is queued or settling **with a capturable size**, `logic` sends
  `ViewportCommand::RequestPaintWhileHidden`; only then does `issue_screenshots` emit the
  paint-requesting `Screenshot` command, after which no further hidden paint request is needed. A
  hidden or occluded window that still reports its current content size can complete this way; a
  minimized window has no capturable size, so it sends none and its request stays queued until
  restore and otherwise reports the 8 s timeout. Do not reuse an old
  rect for a minimized window: issuance and drain fail closed on the fresh geometry. Do not assert
  a hidden/occluded screenshot timeout. Test the real path: issue `ui.screenshot` while
  hidden/occluded and assert one image/reply; then minimize, issue another request, restore, and
  assert it completes (or, without a restore, assert the documented timeout). Use `ui.focus` only
  when focus itself is part of the workflow. A timeout covers "no frame was presented" (event loop
  stalled/closing) or "no capturable size", not "window hidden". Update the existing error text if
  it tells users to make the window visible. Synthetic input also runs only in a UI pass and
  follows the one-unapplied-step rule in section 3.1.
- Capture identity stays strict across resize/DPI/zoom/visibility/occlusion. Keeping `zoom` and the
  occlusion flags in the identity is intentional even when the expected physical size is unchanged:
  a bounded retry (two attempts) followed by an explicit error is preferable to combining pixels
  from different layout generations. This bounded failure is accepted behaviour, not a relaxation
  target.
- Native qualification for a real hidden/occluded capture and a minimized/restore request remains
  required. This plan describes the intended behaviour; no native hidden/minimize/restore run has
  been performed for this source.

### 6.6 Printing: keep the existing synchronous construction, measure it

`SheetApp::submit_print` runs `file.print` synchronously (`lib.rs:595-611`), and the engine builds
the whole PDF in `build_pdf` before returning bytes (`crates/engine/src/cmd/print.rs:1600-1621`).
This is existing baseline behavior, not a 0.37 regression: opening Print settings is lazy (a menu
`file.print` opens the backstage when a native printer is configured), while the Print button and
`ui.printSubmit` block the UI while the PDF is generated and handed to the worker. The migration
does **not** move PDF generation to a background engine unless new behavior requires it or a
predeclared budget says so. Required gate:

- Measure the three phases separately on the same workbook: settings-open latency (no PDF), PDF
  generation (`file.print`/`build_pdf`) and submission (`printing.begin`/worker hand-off), plus the
  frame that contains them. Use the 100k-row fixture and a representative print area.
- Define the candidate acceptance as no regression against the recorded baseline within 9.3's
  thresholds; a regression is fixed in the print path or explicitly budgeted and reviewed.
- Test a hidden/minimized print submission and feedback polling once through `logic`; print
  discovery/worker completion must not depend on a painted `ui`.
- If a future requirement moves generation off-thread, it needs its own command/undo/worker
  ownership design, cancellation and a regression test; it is not part of this migration.

## 7. Native, web and platform workflows

### 7.1 Native Linux

- Build/test via `task.py run --kind build` on a fresh allocation; record source SHA, lock hash,
  feature set, rustc, font package versions and evidence path.
- Native measurement is staged, not assumed: (1) managed build/test and offscreen/kittest
  render, (2) provisioned X11 with a named window manager or Wayland with a named compositor,
  (3) interactive workflow with real input. No `DISPLAY`/`WAYLAND_DISPLAY` is inherited on this
  server, and the host's i915/`/dev/dri/renderD128` access is availability evidence only, not a
  qualified display or GPU backend. Record the chosen backend and driver per row.
- In the provisioned session run `gridcraft --sample sales --control 7979`, drive the control
  channel, take native screenshots, exercise wheel/trackpad, Ctrl+wheel zoom, clipboard/PRIMARY,
  IME (if installed), file picker cancel and window resize. Record compositor and scale factor.
- FreeBSD: run the existing FreeBSD workflow/row; verify the Linux/FreeBSD `arboard`
  `wayland-data-control` cfg and winit configuration do not change the build or clipboard behavior.
- Software renderer (llvmpipe) and real GPU are separate rows; neither is accepted until measured
  with identified adapter/driver.

### 7.2 Windows

- Allocate one uniquely named `craft-windows` clone; use its own SSH/VNC ports. Check allocator
  capacity first: a full copy needs its reservation, and an overlay is allowed only with an
  adequate budget. If neither fits, the row stays blocked and no existing VM is stopped or moved.
  Build the exact task checkout with a separate `CARGO_TARGET_DIR` and run
  `cargo test --workspace` (or the repository's build/test script for the checkout) through
  `guest-exec.py`, submitting each guest command with `task.py run --kind vm` under the clone's
  task ID.
- Use the interactive scheduled-task launcher for the GUI; verify the app is on the active console
  with Explorer. Drive `--sample sales --control 7979`, edit/save/reopen, open a password-protected
  fixture, use the printer settings (discovery may have no printer), resize/minimize, and capture
  screenshots.
- Measure renderer construction separately from first frame because of `#8736` (FXC DX12 shader
  compile). GridCraft forces FXC, so this is on its default path. Time a fresh-process
  `egui_wgpu::Renderer::new` probe (or the `wgpu_core=trace` bracket between
  `create_pipeline_layout` and `create_render_pipeline`) separately from the first presented
  frame; record both.
- The baseline VM is software-rendered (Microsoft Basic Render Driver); report it as such, not as
  a physical-GPU result. Keep the Windows ARM64 packaging workflow in the matrix; it is not a
  substitute for a native x64 GUI/IME run.

### 7.3 macOS

- Build/test on a real macOS host with the macOS CI gate; cross-compilation is not acceptance.
- Cover AppKit native menu, `fmv-macos-events` double-click/Open With, close/quit with unsaved work,
  `on_exit` workaround, Command shortcuts, IME (Korean/Japanese/Chinese), trackpad wheel/momentum,
  Retina scale 1x/2x and Metal/wgpu. This environment is not available on the server; record it as a
  rollout blocker until a real run exists.

### 7.4 Web

- Staged browser evidence: (1) wasm compile in CI, (2) a pinned headless browser run where a
  pinned Chromium/Firefox is provisioned, (3) interactive WebGPU/WebGL2 with recorded browser and
  adapter identity. No host browser is inherited at planning time; an unavailable runtime is an
  explicit blocker, not a compile-only pass.
- `cd apps/gridcraft-web && trunk build --release` and `trunk serve --release`; test in at least
  Chromium and Firefox with WebGPU where available and `?webgl` forcing WebGL2.
- Workflows: `?sample=sales`, edit/save/download/reopen, browser file picker cancel/retry, dropped
  file, paste HTML/plain text, language switch (including CJK tofu behavior), interface scale,
  resize, and keyboard layout (`E10`).
- `cargo xtask wasm` only proves compilation. Browser behavior, IME and clipboard require a real
  browser run; record versions and screenshots/logs.
- Record raw and compressed `.wasm` size; the 0.37 family may change code size.

### 7.5 CI and commands

Baseline and candidate commands (all new jobs use `maintenance/task.py run/status/finish`, a unique
`--task`, and a fresh `--job-id`). Example for the repository gate:

```sh
python3 /home/codex/work/maintenance/task.py run \
  --job-id gridcraft-egui037-ci-01 --kind build --task gridcraft-egui037 \
  --source /home/codex/work/CraftApps/egui037-migration-20261022/gridcraft \
  --evidence /home/codex/work/ssd2/task-evidence/egui037-migration-20261022/gridcraft \
  -- bash -c 'cargo xtask ci'
```

Inside that managed job (never as a direct host command), the candidate checks are:

```sh
cargo xtask ci
cargo test --workspace --locked
cargo test -p gridcraft-ui-egui --all-targets
cargo check -p gridcraft-web --target wasm32-unknown-unknown --locked
cargo run -p gridcraft-ui-egui --example snapshot -- --sample sales out.png
cargo tree -i egui
cargo tree -i winit --target all
cargo metadata --format-version 1 --locked
```

`cargo xtask ci` already runs fmt, clippy `-D warnings`, workspace tests, assets, layers, wasm,
parity and keytips. Add a dependency-identity check to CI (a small script or xtask step) that fails
if any egui-family package or winit comes from a second source/revision, or if a branch/path patch
exists. Test the check with a deliberately wrong pin.

## 8. User-observable regression assertions

### 8.1 Geometry and hit testing

Use `Geo::new`, `grid.rect`, `grid.cells_rect`, `screen_cell_rect` and `ui.inspect`:

- 1200×800 and 640×420, ppp 1.0 and 2.0, light/dark, en/es/ja/zh, RTL on/off.
- Click the centre of A1/B2 and drag C3:F7; selection matches; frozen panes and split panes keep
  headers fixed; scrollbar thumbs map to the same scroll.
- Ribbon tab/height, formula bar, status bar, tabs and central grid boundaries stay within 0.5 pt of
  the baseline where the style pin is intended to preserve them.
- Right-to-left sheet mirrors columns and hit tests; outline buttons and ruler drags keep sides.
- Interface scale 75/100/200% keeps `ctx.zoom_factor()` and geometry consistent.

### 8.2 Fixtures

Fixtures are built in code and never committed as large binaries:

- `Session::new()` + `file.new {sample: "sales"|"budget"|"grades"}` for the three samples.
- A 100,000-row × 6-column workbook built through `range.setValues` in 10,000-row chunks, plus
  formulas `G=B*C+D`, a chained running sum, `SUM`/`SUMIFS`/`XLOOKUP` and a sort, mirroring
  `crates/engine/examples/perf.rs`.
- A two-sheet workbook with a formula being pointed across sheets, a right-to-left sheet, frozen
  panes, merged cells, a table, a chart, comments/notes, validation and a cell picture.
- Password-protected and corrupt-font fixtures through `gridcraft-xlsx`'s `test-support` feature
  and a temporary file; no password is written to logs or evidence.
- 128 small generated PNGs for the picture cache, plus one image larger than `max_texture_side`.
- Pinned UI text in en/es/ja/zh (and Greek/Cyrillic/Hebrew where a face exists) so snapshots and
  accessibility names are deterministic.

Record the fixture builder's source hash and the exact parameters in every evidence run.

### 8.3 Screenshots and pixel probes

- Produce before/after PNGs with `examples/snapshot` at 1440×900, scale 1 and 2, light/dark,
  Home/Insert/View, `sales`/`budget`/`grades`, formula autocomplete (`GRIDCRAFT_LOCALE_SCREENSHOT`),
  table headings (`GRIDCRAFT_TABLE_HEADINGS_SCREENSHOT`) and representative dialogs.
- Store them in durable evidence, not as unreviewed repo assets. Record font files/hashes, OS,
  renderer, DPI and viewport.
- Add a small kittest render test with fixed harness size/ppp/theme that probes known pixels:
  window background, ribbon fill, selected cell fill, grid line, header fill, selection outline,
  formula bar background. Use exact RGB for flat tokens and a small tolerance for antialiased
  edges. The test must fail if a probe is moved by 3 px or a token colour changes; verify with a
  deliberate mutation, then revert.
- Do not bulk-refresh any golden. Review every changed image and explain it (style pin, font,
  renderer, or a real regression).

### 8.4 Accessibility

- `unnamed_widgets()` empty for main window, Backstage, Find/Replace, Format Cells, Sort, Data
  Validation, print settings and Paste Special.
- Query roles/names/values/selected/disabled/focus for: ribbon buttons, tab list, sheet tabs, name
  box, formula bar, font size box, dark-mode checkbox, dialog fields, OK/Cancel, filter menu.
- Keyboard: Tab order reaches every input; Enter/Space activate the same command; Escape closes the
  right popup; focus survives language/theme change and pane show/hide.
- A deliberate missing name or wrong role makes the assertion fail.

### 8.5 Input, IME and text

- `Event::Text`, `Key`, `PointerButton`, `MouseWheel` (with `MouseWheelSource::Unknown`, `Wheel`,
  `Trackpad`, `Momentum`), touch pan and pinch zoom.
- Wheel zoom ladder (`grid::wheel_zoom_step`) at 100/121/99/400%, fine-grained trackpad fractions,
  Ctrl held vs released, momentum phase; no latch from a missing `TouchPhase::End`.
- In-cell editor: type, Enter/Tab/arrow, Escape, multiline Alt+Enter, click-away commit, formula
  point mode, F4, autocomplete; one undo step per commit.
- Regression for `#8763`: select text in the formula bar, then right-click → Copy (or click a
  custom toolbar action that reads the text selection); the action must see the selection that
  existed when the click started (or the app must restore it). The final P8 fork's P7c fixes
  (selected #8763/#8748) must satisfy this case. Only add a local workaround for a proven
  app-specific difference, with a test and removal condition; PR #8764 itself is open/unmerged.
- Bidi/grapheme: Arabic/Hebrew mixed cells, combining marks, emoji clusters, selection and caret.
- IME: preedit/commit/cancel/replacement in the formula bar and in-cell editor produce one undo
  step; E21 delete-surrounding and the complete E21b contract are checked together; a deletion-only
  batch, delete+commit, current-epoch serial mismatch vs obsolete epoch, owner-generation change,
  empty-vs-unavailable, committed clear, final-pass `None`, password clearing and two windows are
  separate cases.
- Clipboard/PRIMARY as in 6.4.

### 8.6 Persistence and lifecycle

- Edit → save → close → reopen; compare values/formulas/styles/merges/tables/freeze/RTL/print and
  `UiState`/`Session.prefs`.
- Undo/redo after reopen? (Undo history is not persisted; assert it is empty as today.)
- Hidden/minimized autosave; close with unsaved work asks; `app.quit` does not ask; printing busy
  cancels close.
- Control `ui.screenshot` and `ui.inspect` across resize/minimize/focus; a hidden/occluded
  queued screenshot reaches issuance through `RequestPaintWhileHidden`, then completes from the
  issued command's own paint request when a current size is reported (no artificial timeout); a
  minimized request stays queued until restore and otherwise reports the 8 s timeout.
- Window title updates on document rename/dirty change, including while hidden.

## 9. Performance measurement and gates

### 9.1 Baseline before production change

Record the baseline at `b8771d51fb4da60bd48c13cf313d6d6014dc5e66` with the current lock and the reviewed `craft-libs` base
`3832c47`; the earlier `ac8da7d4`/`d27646a5` pair stays as historical evidence only. No candidate
numbers may be reported without a paired current baseline. Capture exact SHA, lockfile hash, features, fonts, fixture,
OS, renderer/driver, DPI, viewport, profile, rustc, hardware and load. Use paired runs, alternate
order, 10 warmups and at least 30 timed iterations for steady operations, 20 fresh processes for
startup, and at least 1,000 frames for frame distributions. Report raw samples and the paired
median with a confidence interval for 30-iteration operations; a 30-sample p95 is the 29th
observation and is not a tail estimate, so do not use it as an acceptance threshold. Use p95 from
an expanded tail run (at least 1,000 samples) and p99 only from at least 1,000 frames; report the
sample count with every percentile. The first baseline report and every investigated regression
include an equivalent stock-0.37 control, measured in the same harness alongside 0.36 and the
fork. Stock 0.37 is a measurement control, never a production stage; where an operation cannot
compile unchanged on stock, use a pinned minimal adapter and record the equivalence limit. Also
record peak RSS, texture uploads/allocation counts where practical, and idle CPU/repaint count.
Never report compile duration as UI performance.

### 9.2 Scenes

| Scene | Operation | Measurement |
| --- | --- | --- |
| Startup | fresh process → first painted frame / control channel ready | wall time, split font setup, wgpu renderer creation, first frame |
| Grid scroll | `sales`/100k-row synthetic workbook, 1,000 frames of vertical/horizontal scroll | frame distribution, `grid_ms`, tessellation time, vertices/indices, uploads |
| Cell edit | type/commit 1,000 cells via control channel | command latency, frame CPU, undo memory |
| Recalc | `crates/engine/examples/perf` on the same workbook | engine-only latency, recalc cell count (not a UI number) |
| Dialog/ribbon | open/close Format Cells, switch tabs, type filter | CPU frame time, paint counts |
| Text tessellation | fixed 120-ASCII-label scene + mixed color-glyph row | per-tessellate time/instructions; reproduce #8731 |
| Fonts | 20 cold starts with the same font set | discovery/build/set_fonts time, peak RSS |
| Memory | 100k-row workbook + 128 pictures | peak RSS, texture bytes, cache evictions |
| Idle | visible and hidden, 10 s | logic-tick and painted-frame counts, CPU, no busy loop |
| Print | settings open, `file.print` PDF generation, submission | three separate latencies plus the containing frame; 100k-row fixture |
| Windows startup | renderer construction vs first frame | #8736, FXC DX12, software WARP separately |
| Web | release wasm load + first frame + 1,000 scroll frames | raw/compressed wasm, browser frame timing |

A benchmark harness should be added before the migration, not after:

- `crates/ui-egui/examples/frame_bench.rs`: build a deterministic `SheetApp` with a synthetic
  workbook, call `logic`/`ui` in `ctx.run_ui`, then time `ctx.tessellate` separately; emit JSON
  samples and vertex/index counts. No GPU required.
- `apps/gridcraft` opt-in `GRIDCRAFT_FRAME_LOG=<path>`: a small logger called from both
  `SheetApp::logic` and `SheetApp::ui` appends one JSON line per invocation with an explicit
  `phase` (`"logic"` or `"ui"`), timestamp, `perf.frame_ms`/`grid_ms` where available, viewport
  size, scale and `visible()` as data (never as a phase inference); the app adds
  `eframe::Frame::info().cpu_usage` when it has a `Frame`. Hidden logic-only ticks therefore have
  samples, and a separate 10 s idle probe records process CPU and tick counts. No behavior change
  when unset.
- `egui_kittest` wgpu render loop for offscreen GPU frame time and texture upload deltas; record
  adapter info from a small `wgpu::Instance` probe.

### 9.3 Thresholds and noise policy

Freeze these before candidate runs (stricter repo budgets win):

- Investigate a repeatable >5% median regression (paired CI above the noise floor excludes noise)
  in any scene, or >10% p95 only where p95 comes from the expanded tail run; >5% peak-RSS
  increase; any new idle busy-loop or printing underrun; any loss of an existing absolute budget.
  A 30-sample p95 is reported but never used as a gate.
- For sub-millisecond measurements, require the relative increase to also exceed 0.05 ms or
  corroborate with allocation/instruction counts.
- Text tessellation: compare 0.36.2, stock 0.37.0 and the fork on the same scene. If #8731
  reproduces (expected from the upstream report), the row-level color-glyph fast path must land in
  the fork before GridCraft accepts the migration; do not accept a 60% text-tessellation regression
  as “expected 0.37 cost”.
- Windows startup: measure renderer construction separately. If #8736's reported cost reproduces
  as a repeatable >5% total startup regression, evaluate the fork shader fix with equivalent wrap
  arithmetic; clamp-only is not valid. The upstream ~40 ms figure is a report, not our measurement.
- Reject noisy comparisons as inconclusive and rerun; never auto-widen a threshold or tolerance
  after seeing a failure. Document any pre-measurement exception and its review.

## 10. Publication stages, dependency DAG and rollback

### 10.1 Stages

| Stage | Content | Depends on | Rollback |
| --- | --- | --- | --- |
| P0 | This plan; evidence notes | none | delete plan branch |
| S1 | Shared craft-libs CL-1 for the eleven `craft-ui` consumers: port only `8614e4a` API hunks + `1764bca` README onto `4919088`, final approved egui/winit pins (blocked until those refs exist), registry compatibility, release SHA | P0 review, final refs selected | revert CL-1; keep old `d27646a5` |
| S2 | Complete reviewed/published `storytold` egui **P8** (E1–E21 + E21b, P2b async W15P routing, P7 held-key pairing, P7b/E21b, P7c selected #8763/#8748 fixes, justified perf fixes) and completed winit (W17–W27 + W15P + W27 lifecycle), immutable approved SHAs. No stock 0.37 or intermediate stage | P0 review, fork review, final refs selected and published | revert fork pins; keep 0.36.2/0.36 craft set |
| CL-2 | craft-libs provisional capture-slot API in `crates/ui/src/capture.rs`; cancellation/out-of-order/shutdown stress; lazy-print tests | P0 review | revert helper with its adapters |
| CL-2F | Shared capture API freeze for the eleven `craft-ui` consumers after reviewing the DesignCraft and GridCraft CL-2 pilot adapters | CL-2, G1 (Grid adapter), DesignCraft adapter | keep CL-2 revision; no app change |
| G0 | Baseline harnesses on `ac8da7d4`: frame bench, phase-tagged frame log, 0.36-compatible geometry/screenshot tests, benchmark evidence (0.37-only assertions arrive in G1/G3) | P0 | remove harnesses; no production behavior change |
| G1 | Atomic dependency/compile migration: root patches, manifests (native eframe `clipboard`), lock, `craft-ui`/`craft-print` bump, CL-2 slot adapter, E21b/W15P consumption, Role/ID/FontData/screenshot-callback source changes including `drain_screenshots`, timeout and the basic reply, strict shared harness with required names/fonts, style pin | S1+S2+CL-2, G0 baseline | revert G1; restore baseline lock/manifests |
| G2 | Lifecycle: autosave/title/printing in logic, adversarial screenshot byte-budget/discard/timeout/epoch tests, hidden synthetic-input paint | G1 | revert G2; G1 still builds |
| G3 | Remaining accessibility names/roles, pixel probes, focus/ID tests, broader IME/RTL coverage | G1 (G2 if same files) | revert G3 |
| G4 | Platform/performance acceptance (staged): Linux X11/Wayland, Windows VM, browser, macOS; print and CL-2 evidence; fix findings | G2+G3+CL-2F | revert G4 fixes; retain evidence |
| G5 | Docs: architecture, ui-parity, control protocol notes, release notes | G4 | revert docs |

Implementation does not start until P0 has a fresh independent adversarial review, the complete
S1/S2 revisions are fixed and published, and CL-2's provisional API exists. The final production
`storytold` SHAs for egui P8/winit are unknown until that approved implementation; their absence
blocks S2/G1 rather than being filled with a candidate SHA or a stock/intermediate stage. Each app
PR is reviewable on its own; do not land an intermediate dependency bump that leaves `main`
unbuildable. A consumer already carrying an incompatible `craft-ui` bump must put that bump in the
same atomic dependency stage as the egui P8 landing. G1 is the only atomic “must compile and pass
CI” PR: it includes the shared strict harness, every accessible name and pinned font needed by the
existing suite (no suite-wide `allow_missing_glyphs`/`with_accessibility_check(false)`), and the
CL-2 adapter. G2/G3 can be separate commits/PRs once G1
is green. CL-2F freezes the shared capture API only after both initial adapters are reviewed. G4 is
validation plus targeted fixes, not a feature grab; rerun the affected performance and visual checks
after any fix.

### 10.2 Compatibility pin and lockfile checks

Before each publication:

- `cargo metadata --locked` and `cargo tree` show one egui-family source/revision and one winit.
- Root `[patch.crates-io]` entries use full SHAs; no branch/path.
- `craft-ui` and `craft-print` resolve to one approved craft-libs SHA that contains CL-1, CL-2's
  provisional-or-frozen capture slot and any CL-3 contract fixes GridCraft consumes. CL-1 and
  CL-2F are published for the eleven sharing consumers; GridCraft/DesignCraft use the CL-2
  provisional pilot and their adapters precede CL-2F. Do not add a GridCraft-local capture
  channel/token map while the shared slot is the selected design, and do not leave a mixed
  `craft-ui`/egui stage.
- The lockfile has no second `egui`/`eframe` version and no stale accesskit_consumer duplicates
  except ones proven necessary by an unrelated dependency.
- `cargo check --locked` in a clean checkout works; CI's dependency check fails on a missing patch.
- `docs/` version statements that mention 0.36 are updated only after validation; do not rewrite
  historical citations.

### 10.3 Rollback

Rollback restores the entire coherent set: `Cargo.toml`, all manifests, `Cargo.lock`, root patches,
source adaptations, `craft-ui`/`craft-print` revision and any test harness changes. Do not “roll
back” by changing only the egui version string. Keep the old lockfile, the 0.36.2 registry crates and
craft-libs `d27646a5` reachable. No persistence format changes are planned; `ui.json`, `prefs.json`, XLSX and
control JSON remain compatible. If a persistence change becomes necessary, specify backward
compatibility and preserve unreadable data.

## 11. Original planning risks and remaining acceptance criteria

Current disposition (2026-10-24): exact canonical revisions and the coherent capture API in
items 1–2 are selected as recorded at the top of this plan. The representative strict harness
checks in item 9 passed in final full CI07. Earlier candidate/PR states below are planning history,
not current pin blockers. Native/platform/IME, screen-reader and performance acceptance remain
separate; the final qualification update records what actually passed.

1. **Final production fork revisions.** `aiongg/egui`/`aiongg/winit` are review candidates, not
   production sources. The exact immutable `storytold` egui **P8** (including P2b async W15P
   routing, P7 held-key pairing, P7b/E21b, P7c's selected #8763/#8748 fixes and justified perf
   fixes) and completed winit (W15P + W27 lifecycle) SHAs are not selected at planning time.
   Pinning is blocked until the approved implementation publishes them; record the full SHAs in
   1.1 and verify `--locked` before S2/G1. No stock 0.37 or intermediate stage is accepted. Without
   E2/E3, #761's crash/fallback behavior is not qualified; without E4/W15P and E19/E21/E21b,
   Wayland clipboard, PRIMARY, non-Latin and IME behavior regress.
2. **craft-libs CL-1 and CL-2 exact revisions.** PR 199's tree is stale; CL-1 is the hunk port onto
   `4919088`, not a rebase. GridCraft cannot bump `craft-ui`/`craft-print` until CL-1 and CL-2's
   provisional capture slot exist with clean tests and a coherent lock. CL-1 and CL-2F are the
   shared gate for the eleven `craft-ui` consumers; GridCraft/DesignCraft are the CL-2 pilot and
   their adapters are reviewed before CL-2F freezes the API. Any incompatible existing `craft-ui`
   bump must land in the same atomic dependency stage.
3. **PRIMARY and HTML coexistence.** W15P must land in the approved winit revision, and the 6.4
   routing must be proven on wlroots/KWin/Mutter: grid HTML+plain-text copy through the one
   arboard data-control handle, TextEdit copy/paste and PRIMARY through egui-winit/W15P, no second
   arboard handle or `wl_data_device`, password exclusion and owner handoff intact. Native evidence
   is required.
4. **E21b outgoing IME text.** The final canonical fork includes the shared producer and Wayland
   adapter; the original candidate lacked them. GridCraft-specific acceptance still requires the
   complete shared contract: owner generation present even when unavailable, available-empty
   distinct and explicitly sent as an empty payload, cache clear plus committed disable/enable on
   owner/unavailable transition, final completed pass including `None` authoritative, a
   current-epoch serial mismatch applying incoming edits once and deferring outgoing state until a
   match while obsolete epochs invalidate, and no full-document allocation for the new bounded
   payload. Formula-bar/in-cell surrounding text, W27 delivery, deletion+commit, preedit exclusion,
   focus/password clearing and two-window isolation remain gates; E21 alone is not enough.
5. **#8763/#8748 TextEdit selection and hidden-child regressions.** 0.37 collapses a focused
   TextEdit's selection before the clicked widget runs. The final egui P8's P7c owns the selected
   #8763/#8748 fixes and regression coverage; GridCraft verifies its formula-bar/context-action
   paths under section 8.5. A local workaround requires a demonstrated app-specific need and
   removal condition; PR #8764 itself is open/unmerged.
6. **#8731 text tessellation.** Stock 0.37 is reported ~60% slower for plain text; stock is the
   measurement control. If reproduced, the fork needs the justified row-level fast path in P8
   before GridCraft accepts. GridCraft renders a large text grid, so this is a release blocker,
   not a note.
7. **#8736 DX12 shader compile.** FXC is GridCraft's default. Measure renderer construction and
   first frame; decide on the fork shader fix without weakening wrap modes. The upstream figure is
   a report, not our measurement.
8. **#8512 system fonts.** Do not adopt the new provider without a measured startup/coverage
   comparison; GridCraft's per-workbook font binding and script fallback must remain.
9. **Strict harness names/fonts.** G1 must pass `egui_kittest`'s Panic/accessibility defaults using
   pinned fonts and complete accessible names, not a global opt-out. If a specific fixture cannot
   be pinned, record the gap and the per-test exception; do not make CI depend on host fonts.
10. **Staged platform evidence.** Native Linux needs a provisioned X11/Wayland display and
    compositor; Windows needs an owned clone that fits the allocator budget and remains
    software-rendered until a qualified GPU run; browser needs a pinned runtime; macOS and physical
    Windows GPU/audio need real hosts. Host `/dev/dri/renderD128` access is not display/GPU
    acceptance. These stay explicit rollout blockers until recorded.
11. **AccessKit tree changes.** Role/tree/name changes may alter native screen-reader output.
    kittest assertions are necessary but not sufficient; native AT-SPI/UIA/VoiceOver smoke tests are
    required where available.
12. **Print scope.** PDF construction remains synchronous on the UI path as it is today. A measured
    candidate regression is fixed or explicitly budgeted; moving generation to a background engine
    is a separate, designed change.
13. **No generic compatibility layer.** craft-ui owns the capture slot because multiple apps need
    the same completion lifecycle. GridCraft keeps its timeout, paths, tokens, JSON and animation
    policy and does not add a local channel alternative.

Acceptance for G1 is: `cargo xtask ci` passes at the exact approved P8/winit revisions with actual
test counts and the strict shared harness (no global glyph/accessibility opt-out); `cargo check
--locked` passes for native and wasm; one egui/winit source; E21b/W15P/CL-2 APIs are consumed;
`drain_screenshots`, timeout and the basic screenshot reply work, including a hidden/occluded
queued capture admitted through `RequestPaintWhileHidden` (a minimized request waits for restore or
times out); the historical compile errors are gone; and the
G1 smoke tests (open/edit/undo/save/reopen, screenshot, language switch, resize) pass. Acceptance for the migration as a whole adds G2–G4 evidence, the
visual/geometry/accessibility checks, paired performance results against the recorded baseline,
print and capture measurements, and no unresolved blocking unknown above.


## Local implementation update — 2026-10-23

The user clarified that reviewed plans remain local and implementation proceeds before publication. The earlier requirement to publish/land this plan first is superseded; pushes and PR publication still require explicit approval.

Canonical `storytold/gridcraft` `main` for this migration is `b8771d51fb4da60bd48c13cf313d6d6014dc5e66` (the checkout in
`/home/codex/work/CraftApps/egui037-migration-20261022/gridcraft-implementation`). The earlier
`bdf8b8cc…` and `ac8da7d4…` SHAs and line anchors above are retained as historical review
evidence; use `b8771d51fb4da60bd48c13cf313d6d6014dc5e66` and its exact lockfile for current-main integration and paired
measurements. Shared `craft-libs` integration now pins canonical 25da1314; 3832c47 is its
historical integration base. The original refs remain reachable. These unpublished immutable
pins still require an approved publication and clean remote-fetch verification.

Add Ctrl+8 outline symbols/headings combinations to geometry and hit tests; consume reveal_from once; include Auto Fill Options, accounting fill and Ctrl+click hyperlink regressions. Pin fonts when using the now host-font-dependent snapshot example.

The complete static delta review and exact old/new object identities are retained in the task evidence: `canonical-diffs-refresh-20261023.json` and `overnight-consumer-review-20261023.md`. This amendment claims no new build, native GUI or performance acceptance.


## Explicit accessibility qualification update — 2026-10-24

Full CI07 is successful evidence for the final formatted runtime source, including the seven
representative integration tests that explicitly call `check_accessibility()` on rendered
ribbon/locale/pane/menu/dialog/editor/backstage states. The real SheetApp capture Harness also
passed checks before issuance and after manual callback drain/repaint, preserving callback count,
request token, one-reply and released-reservation assertions. The CI06 unnamed controls were
repaired with visible localized label relationships or precise semantic names; roles and behavior
assertions were preserved. The four source-file differences from the fresh accessible-name review
are formatter-only. Full CI07 passed all ten stages with 2,249 passed, 0 failed and the existing
release timing experiment ignored. CI05 remains historical pre-repair evidence; CI06 remains a
failed historical run.

This is callback and representative UI-tree coverage, not GPU/native hidden-window, native
screen-reader or every-state accessibility acceptance. The canonical egui fork includes outgoing
IME surrounding-text support and its Wayland adapter; application-specific formula-bar, in-cell
and text-box owner transitions, stale deletion/commit handling and native composition remain
separate qualification gates. Native open/edit/undo/save/reopen, language/resize, clipboard/PRIMARY,
hidden/minimize/restore/readback and browser/macOS/Windows workflows remain open, as do paired
performance, renderer/print and font/WASM size acceptance. Required CI success permits local
commit review; it does not authorize a push, tag or PR publication. Every publication still needs
fresh explicit human approval, followed by clean remote-fetch verification of the exact pins.

Revision: 2026-10-24 — bound CI07, review union and final canonical source; no production changes.
