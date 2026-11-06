# Document tabs (WPS-style) — design

Status: approved in conversation (2026-10-31). Approach A.

## Intent

Today GridCraft opens every workbook in its own native window (`doc_windows.rs`). Users coming
from WPS/Excel expect one window with a tab per open workbook. Goal: books opened while the app
runs become tabs in the main window; switching tabs switches the document, with the tab showing
the document name and an edited marker.

## Goals

- One window, one tab per visible workbook, in open order.
- Click a tab to switch; the active tab is visually distinct.
- Tab shows the same title the window used to (`Book1`, `Book1:2`) plus the edited marker.
- Close a tab with its `×`, going through the existing unsaved-changes prompt.
- Files dropped/opened, `File › New`, and script/CLI opens all land as tabs.
- Keep `Session`'s multi-window model: the engine still owns windows; only presentation changes.

## Non-goals (this change)

- Dragging a tab to reorder or to detach into a native window (WPS/Excel do this).
- Tab context menu (close others, duplicate, reveal in folder).
- Multi-row/overflow scrolling of the strip beyond what egui gives us.
- Changing the engine, `craft-window`, or the CLI/MCP surfaces.

## Anti-goals (would make it a failure even if it builds)

- Losing the unsaved-changes prompt on tab close.
- Two workbooks drawn in one frame (state bleed between interface states).
- Breaking the web build (`wasm`) or the CLI.

## Constraints

- `craft-window` (`Viewports`, `WindowList`) is an external git dependency we do not fork.
  The tab strip is therefore implemented in `crates/ui-egui`, drawing the root viewport only.
- `SheetApp::show_window(id)` already swaps per-window interface state; reuse it.
- Windows toolchain: `x86_64-pc-windows-gnu` (no Visual Studio Build Tools on this machine).
- Upstream is MIT OR Apache-2.0: keep `NOTICE`/`ATTRIBUTION.md`, ship the fork's changes as a patch.

## Approaches considered

- **A (chosen): tabs replace native windows.** All visible windows draw as tabs in the root
  viewport; `Viewports` is bypassed in tab mode. Least new state, one render path, closest to WPS.
- **B: tabs as an option.** Native windows stay default. Doubles the render path and the testing
  surface for little gain.
- **C: tabs only, multi-window code deleted.** Simplest to maintain but removes a capability
  (second monitor) that works today.

## How it works (as designed)

1. `DocWindows` gains a `tabs: bool` (true on desktop). `new()` sets it.
2. `show()` in tab mode: `sync()` as today → draw the strip → `app.show_window(active)` →
   `app.ui(ui)`.
3. Strip: `visible_windows(app)` order; each tab is a selectable label with the `window_title`
   text and a small `×`; clicking selects (`list.focus(id)` + `app.enter_window(id)`).
4. Closing: `×` runs the existing `window.close` command for that engine window, so the save
   prompt, "last window closes the app" and macOS "keep running" rules stay where they are.
5. The native window title follows the active tab.

## Left to the builder

- Exact strip styling (theme tokens, hover, active underline) — match `tabs.rs` (sheet tabs).
- Whether the strip lives above the ribbon (chosen) or inside the title bar (needs `craft-window`).
- Overflow behaviour when many books are open.
- Tests: pure functions for tab ordering/titles; a UI smoke test if the harness allows.
