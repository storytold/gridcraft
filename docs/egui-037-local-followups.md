# egui 0.37 local port — follow-ups and gates

Status: local implementation on GridCraft `b8771d51fb4da60bd48c13cf313d6d6014dc5e66` with canonical
immutable dependency pins. Final managed full CI
`egui037-gridcraft-candidate-20261024-full-ci-queue-07` succeeded at 2026-10-24 05:21 UTC after both
accessibility repairs: all ten stages passed, with 2,249 tests passed, none failed and one existing
release timing experiment ignored. The representative explicit accessibility/capture checks
passed. The required local precommit CI gate is complete for the formatted runtime source;
this final refresh changes only the two migration documents. Native/platform/performance
acceptance remains separate. No code
push or PR publication is authorized; each requires fresh human approval of contents and validation.

## Current integration inputs

- Root Cargo patches select the egui family at `173cc7375d35dc5fbc5b5d0994fdf787f547c5d1`
  and winit at `ae48c756a3b81125eb796c0cc6dccf60990d1181`. Selected egui packages are 0.37.0;
  there is no machine-local path override or second egui 0.36 source.
- `craft-ui` and `craft-print` both pin `c488caa9fbc2fb1a86e4674887e9b80b598ebbfa`.
  The shared capture slot is consumed through that revision. Canonical Cargo.lock SHA256 is
  `98209e84412b8ce08bf290ab706c20909389ca6d1e6668aca929f717ad55ea2f`.
- CI, release and FreeBSD select craft-fonts `b076417b5880bd4154bd22f00695b1d28c6bea83`.
  Local validation uses command-scoped private Git mirrors for unpublished immutable inputs;
  clean remote fetching after approved publication remains a separate gate.
- The shared capture contract fits GridCraft's requirements without changes: one slot per request,
  `take()` at most once, `cancel()`/drop makes late callbacks inert, a per-image byte bound, and a
  viewport waker that runs after the slot lock is released. GridCraft adds only its admission
  budget, request records, geometry epoch and output routing around it.
- The `CaptureSlot` callback holds only a weak reference and never encodes, writes a file or sends
  a control reply; GridCraft drains the slot from `logic` and writes outside the slot.

## Capture lifecycle repair (2026-10-23)

The previous managed run of this checkout (`egui037-gridcraft-candidate-20261023-tests-01`)
finished with 154 passed and 8 failed: five screenshot/lifecycle tests dropped an unapplied
`TexturesDelta`, two more lifecycle tests failed on the same atlas delta, and
`formula_bar::spill_tests::formula_bar_click_and_theme_presentation` reported the Name Box
`TextInput` without an accessible name. Those failures are historical; final full CI07 passed the repaired lifecycle and
accessibility tests.

The local source now addresses the fresh review's capture findings:

- A completion that arrives after the request deadline is refused with the timeout error before
  any encoding or file write; timeouts, geometry mismatches, oversize images and removed
  requests all release their reservation. Every screenshot reply carries its `request` token.
- Screenshot deadlines and AutoSave pacing use a monotonic clock (`std::time::Instant` natively,
  `performance.now()` on wasm). `now_ms()` stays the wall clock for user-visible timestamps such
  as Backstage's "last changed".
- `CaptureGeometry::current` reads `ViewportInfo` (refreshed by `run_logic`) for the viewport
  `inner_rect` and `native_pixels_per_point * zoom`, falling back to the last screen rect; a
  hidden resize is therefore seen without a UI pass. Issuance and drain update the geometry and
  its epoch through one helper, and the slot is created at issuance with a per-image bound equal
  to the aggregate reservation (which includes the accepted one-pixel rounding).
- A request admitted while the window has no size (minimized or not laid out) reserves
  `UNKNOWN_CAPTURE_BYTES` and waits for a real size; it times out with a size-specific message.
  Drain runs before admission, and settling counts one non-discarded egui pass per request.
- The geometry identity intentionally includes `zoom`, `visible` and `occluded` even when the
  expected physical size is unchanged. A capture is bound to one layout generation; the two-retry
  budget followed by an explicit error is the accepted bounded failure, not a reason to compare
  only the expected size.
- A synthetic step consumed by a font-gated `ui` early return is offered again on the next pass
  (the hook tracks the `cumulative_pass_nr` at injection), and font-set changes wait while an
  input step is pending. Re-offering the step is safe: the early-returning pass handled no input,
  so egui's state changes from those events (keys down, pointer down) have not applied a step.
- The Name Box and the formula-bar/in-cell editors carry explicit accessible names, so the strict
  kittest accessibility check passes without disabling it or the missing-glyph policy.

### Shared eframe web follow-up: `RequestPaintWhileHidden`

Native `egui-winit` handles `ActionRequested::wants_paint()` for `RequestPaintWhileHidden` and
paints the hidden viewport. The pinned `eframe` web runner does not: a hidden tab runs only
`run_logic`, and `handle_viewport_commands` handles `Screenshot`/`SetTheme` while every other
command — including `RequestPaintWhileHidden` — falls into its unhandled-command log. A queued
`ui.screenshot` or a queued `ui.key`/`ui.text` step in a hidden browser tab therefore cannot reach
its `ui` pass until the tab is visible; the screenshot request reports its timeout instead. The
fix belongs in the shared eframe web runner (paint a hidden tab on request, with the same
queued-work trigger GridCraft sends from `logic` on native), not in a GridCraft-local channel.
GridCraft therefore sends `RequestPaintWhileHidden` only off wasm (a visible web tab still gets
`request_repaint`). The control protocol documents this web limitation instead of claiming hidden
web screenshots work.

## E21b — application qualification remains open

The final canonical egui fork includes `IMEOutput::surrounding_text`, owner/context generation
and the Wayland outgoing adapter; the earlier statement that no shared producer/adapter exists is
superseded. GridCraft must qualify its actual formula-bar, in-cell and text-box editing paths,
including available-empty versus unavailable, stale epoch/serial deletion and commit, deletion-only
batches, composition ownership and two-surface isolation. Full CI07 is not a native IME or complete
E21b/W27 acceptance result. No application-local API stub is introduced.

## Review repair (2026-10-24)

- `RequestPaintWhileHidden` is sent only for a queued/settling request that has a capturable size;
  an issued capture relies on the `Screenshot` command's `wants_paint`, and a minimized request no
  longer forces full hidden frames while it waits.
- A slot-level per-image refusal (an image larger than the reservation) now spends the same bounded
  retry as a smaller image, instead of failing only when the window grew.
- The shared deterministic harness, the `Text Box` accessible-name translations, the explicit
  `PasteImage`/`MiddleClickPaste` no-op choice, the lifecycle once-per-host-frame tests
  (autosave, title, print poll, discard after issuance) and wheel unit/source/phase coverage are in
  the working tree. Final full CI07 exercised these repairs and the explicit accessibility
  assertions below successfully.

## Remaining acceptance gates

- Required local CI is complete: full CI07 passed i18n-report, fmt, strict workspace all-target
  clippy, workspace tests, doctest, assets (34 attributed files), layers, WASM, parity and keytips.
  The existing timing-sensitive `recalc_with_range_formulas_scales_linearly` test remains ignored
  by the normal CI command; it is not a performance result. No additional CI execution is claimed
  for this documentation-only refresh.
- Strict `egui_kittest` harness qualification completed for the selected end states: `tests/common/mod.rs` now installs
  `theme::pinned_font_definitions()` (bundled faces plus the pinned craft-fonts input, no host
  system scan), `SheetApp::use_prepared_fonts()` keeps that set through the first `logic` pass, and
  `MissingGlyphPolicy::Panic` stays on. Since `run_steps` does not invoke kittest's accessibility
  checker, selected ribbon, locale, pane/menu, dialog, active-editor, backstage, text-box and
  capture/repaint end states now call `check_accessibility()` explicitly; these assertions passed in CI07,
  without claiming coverage of every UI state. `CRAFT_FONTS_DIR` (and
  `CRAFT_FONTS_REQUIRED=1` for acceptance) is the CI contract; without it the deterministic
  bundled set is used and the all-catalog language test records the `Ja`/`Ko`/`Zh` gap.
- End-to-end hidden/occluded `ui.screenshot` with a capturable current size and a minimized
  request that waits for restore (or times out), through a real wgpu/kittest or native window; the
  per-image/aggregate budget stress cases, resize-back/DPI/zoom epoch retries, and native
  clipboard/PRIMARY/IME evidence. No native hidden/minimize/restore run has been performed for
  this source.
- Native Linux, Windows clone, browser and macOS workflows; performance, text-tessellation,
  Windows renderer-construction and print measurements against the recorded baseline.
- Accessibility roles/names beyond the mechanical `Role` port (G3) and native screen-reader smoke
  tests.

## Font provisioning (2026-10-23, font-coverage integration)

The font gap is addressed in the font set itself. Test harnesses keep
`MissingGlyphPolicy::Panic` and the enabled accessibility-check policy; the representative explicit
checks above are needed to execute that policy on `run_steps` fixtures. No test calls `allow_missing_glyphs()`
or disables the accessibility check, and production still renders Tofu for a character no face
has.

- `gridcraft-ui-egui` enables `egui/default_fonts` and `egui/monochrome_emoji_fonts`. egui 0.37
  moved Noto Emoji and emoji-icon-font behind the monochrome feature, and with `default_fonts`
  alone no installed face has `✕` or `📋` (Hack, which the theme adds as a fallback, has the
  arrows and triangles but neither of those).
- `theme::font_definitions_with` appends egui's bundled `Hack` face (arrows, triangles, `↳`) behind
  the UI/cell and emoji faces of every family, and appends the optional craft-fonts faces at the
  very end. The order is: app/bundled faces, Hack/emoji, system script fallbacks, permitted craft
  faces last. A craft face never displaces a system face that already has the glyph.
- `crates/ui-egui/build.rs` embeds the faces named by `CRAFT_FONTS_DIR` through
  `crates/ui-egui/src/craft_fonts_policy.rs`, the single policy shared by the build script and the
  tests. Nothing reads a fixture path from Cargo configuration: the checked `.cargo/config.toml`
  holds only the `xtask` alias, so every local and managed run sets the variable explicitly. With
  it unset the generated table is empty and the app builds, tests and runs; a script that no
  installed system face and no bundled face covers stays the platform gap it was before (Tofu in
  production, and the strict kittest harness still panics on it by design).
- Required faces, matched by exact family and style and by their exact manifest script tags:
  `BIZ UDPGothic` `Regular` and `Bold` (`Jpan,Latn`), `NanumGothic` `Regular` (`Hang,Kore`),
  `Noto Sans Symbols` `Regular` (`Zsym`) and `Noto Sans Symbols 2` `Regular` (`Zsym`).
  `NanumGothic` has no space, and `Noto Sans Symbols` is not `Noto Sans Symbols 2`.
- `CRAFT_FONTS_REQUIRED=1` turns a missing `CRAFT_FONTS_DIR`, a missing `fonts/manifest.txt`, a
  malformed manifest row, a missing font file, an unexpected style of a required family or any
  missing required face into a build error.
- Barred families never enter the font set, although the shared repository keeps them for other
  consumers: `Adobe*`, `Source Han*`, `Source Sans*`, `Source Serif*`, `Noto Sans CJK*` and
  `Noto Serif CJK*`. There is deliberately no broad `Noto Sans` prefix, because it would also
  reject the permitted `Noto Sans Symbols` faces.
- Web (`wasm32`) embeds only the explicit allowlist: `BIZ UDPGothic` `Regular` and `Bold`,
  `NanumGothic` `Regular`, `Noto Sans Symbols` `Regular` and `Noto Sans Symbols 2` `Regular`; no
  CJK collection and no document-only face. Those five files are 12,970,840 bytes raw. Raw font
  bytes, raw wasm and compressed wasm are **not measured yet**: the size gate is a parent
  acceptance item, not a claim made here.
- Native builds opt into the OFL `LXGW WenKai Lite` face from craft-fonts'
  `fonts/simplified-chinese-manifest.txt` (the same face DeckCraft uses). The barred `Noto Sans CJK`
  collection still never enters; system Han faces still lead when installed. `CRAFT_FONTS_REQUIRED=1`
  native builds require the Chinese manifest, ordinary builds tolerate an older checkout without
  it, and `WEB_ALLOWED` excludes it. The Chinese samples join the strict script test only when the
  face is embedded.
- Native also embeds every other non-barred manifest row (for example `BIZ UDMincho`,
  `Shippori Mincho`, `Noto Sans Arabic/Georgian/Hebrew/Thai`). They are OFL and asset-policy safe,
  but the actual set is wider than the five required faces; the size gate must measure it rather
  than citing only the web allowlist.
- No-font/default mode: the bundled faces supply `↘ ◀ ↑ 📋` and the rest of the interface's
  symbols, which `tests/symbol_glyphs.rs` asserts strictly. `✕`, `✓` and `⌃` have no bundled egui
  face, so chrome draws them instead of painting them: the Symbol gallery and the close/check
  controls paint the code-drawn `Icon::Check`/`Icon::Close` (`icons::fallback_for_symbol`,
  `widgets::glyph_button`), and a tooltip whose symbol is missing falls back to `^`
  (`theme::symbol_fallback` through `theme::available_text`). Only workbook text still needs a real
  glyph, because its characters are the user's data. The test documents `✓`/`⌃` as optional-input
  gaps rather than claiming them.
- Extended acceptance mode (`CRAFT_FONTS_DIR` set, and `CRAFT_FONTS_REQUIRED=1` for release):
  `tests/symbol_glyphs.rs` requires the exact union `↘ ✕ ◀ ↑ 📋 ✓ ⌃` in both `Proportional` and
  `CELL` plus the Korean interface labels, and fails instead of skipping when a loaded or required
  set lacks a face. `⌃` comes from `Noto Sans Symbols`, `✕ ◀ ✓` from `Noto Sans Symbols 2`,
  `↘`/`↑` from either Symbols face (NanumGothic also has them), and `📋` from egui's emoji faces.
- The required faces live in the pinned `craft-fonts` checkout at
  `b076417b5880bd4154bd22f00695b1d28c6bea83`. Point `CRAFT_FONTS_DIR` at that checkout: the older `fixtures/craft-fonts` snapshot has no Korean or
  symbol rows, and a loaded set without the required faces fails the extended checks by design.
- `theme::fallback_covers_its_script` is strict whenever a system or craft-fonts face provides the
  script, now including Arabic and Georgian; its skip prints the script name and sample.
- The production positive-rectangle fix in `formula_bar.rs`, the `Role::DisclosureTriangle`
  queries, the linear per-line editor-height assertions and the explicit accessible names for the
  Name Box and both editors are unchanged.


## Explicit G1 accessibility repair — 2026-10-24

Seven existing integration tests now check rendered ribbon/light-dark, each supported locale,
pivot pane and open menu, fifteen dialogs, active cell/formula edits, backstage pages and active
text-box/committed states. The existing capture callback test now uses a full SheetApp Harness,
asserts the Name Box tree is present, and checks accessibility before issuance and after callback
drain/repaint. Its manually supplied pixels still test callback lifecycle rather than GPU readback;
the explicit no-GPU renderer prevents kittest from silently turning it into a platform test.
One-callback, request-token, one-reply and released-reservation assertions remain.

All eight affected tests (seven integration tests and the capture unit test) passed in full CI07.
The four CI06 failures exposed unnamed Options/ribbon ComboBoxes, custom format input and pivot
layout control; visible localized labels or precise semantic names now identify them. The fresh
source review found no blocker; CI07's formatting changed only wrapping/trailing commas in those
four reviewed production files. Original roles, input sequences and behavior assertions remain.
Full CI05 is historical pre-repair evidence and CI06 remains a failed run.

Actual native screenshots/hidden/minimize/restore, screen-reader output, application E21b IME and
platform/performance acceptance remain open. Full CI07 uses the canonical lock, required craft-fonts
input, command-scoped exact-source mirrors and the private Vulkan loader; it is not host-loader or
clean public-fetch qualification. Exact job/log hashes, review union, complete final source inventory
and documentation-only delta are preserved in local task evidence under
`egui037-migration-20261022/final-landing/gridcraft-final-qualification`.

Revision: 2026-10-24 — replaced pending accessibility/CI statements with actual full CI07 results,
corrected the shared outgoing IME inventory, and retained native/performance/publication gates.
