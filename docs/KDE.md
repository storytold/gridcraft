# KDE Plasma / Linux desktop integration

Updated 2026-10-29. Linux integration uses the portable menu model and optional desktop backends
from the vendored `craft-desktop` snapshot. Its canonical development source is the parent
workspace's `shared/craft-desktop`; independent application checkouts retain their own snapshot.
The engine does not depend on D-Bus, Wayland, or KDE.

## Global Menu

The desktop host exports text menus through `com.canonical.dbusmenu`. On X11/XWayland it
advertises the export through the KDE window properties and AppMenu registrar; on native
Wayland it associates that export with the existing winit surface using KDE's appmenu
protocol. This requires the bundled winit patch: an upstream raw surface pointer alone is
not used or reconstructed. Unsupported compositors and missing session services leave the
local interface available.

Use Plasma's Global Menu widget to display the active window's menu. The application does not
install or rearrange widgets. Set `GRIDCRAFT_NO_GLOBAL_MENU=1` (or `KDE_NO_GLOBAL_MENU=1`) to
disable export. System appearance and the local ribbon continue to work independently.

Implementation is connected to the host; application-specific live Plasma verification is a
separate check and is not implied by transport or menu-model tests.

The existing launcher policy still prefers XWayland when available; `GRIDCRAFT_WAYLAND=1`
selects native Wayland. The pinned winit fork includes native Wayland file-drop support,
but that path has not been live-verified in this reconciliation.

## Appearance and local interface

System appearance reads the standard XDG Settings portal's numeric
`org.freedesktop.appearance/color-scheme` value. The background watcher requests a repaint when
the setting changes, bounds its D-Bus calls and stops when the application drops it. Explicit
Light/Dark application choices continue to override the system. The application's own palette
is retained; the portal setting is not a complete Breeze colour-role export.

The local ribbon stays available. Its text menu representation is curated from existing
interactive commands: dialogs, formatting, data operations, print and clipboard use the same
application routes as local controls. Commands that need argument entry use a dialog rather
than exporting unusable engine calls. Rich galleries remain in the local ribbon.

File dialogs already use rfd's XDG portal backend. Settings stay in the existing XDG config
directory. No change to Plasma panels or user desktop configuration is required by the app.

## Multiple windows

The exporter currently binds only the original native window. Additional workbook windows,
including another view of the same workbook, retain their local controls. While a child is
active, the root menu is cleared and queued panel actions are rejected. Delayed clipboard
events also retain their originating window identity. Per-window Global Menu binding remains
pending; this migration does not claim full multiwindow Plasma acceptance.

Editor clipboard requests can finish asynchronously. Their document, window, selection and
editor target are rechecked until delivery; returning to a changed target does not revive an
old request. Only one request of each kind may be outstanding. Toolkit clipboard events carry
no request ID, so an overlapping keyboard paste can be conservatively rejected while a stale
menu request is pending; retry after that event resolves. A duplicate menu request displays a
notification. If the native clipboard returns no event, click back in the document before
retrying to cancel the outstanding request. No guessed frame or timeout expiry
allows an old paste into a new target. These paths still require native clipboard acceptance.

Upstream's document-window close and quit workflow, including its unsaved-workbook prompts,
is retained. The earlier single-window close-risk description no longer describes this host.

## Verification

Current reconciliation (2026-10-29): exact `cargo xtask ci` passed all ten stages
(i18n report, formatting, workspace Clippy, tests, doctests, assets, layers, WASM,
parity and keytips), using the pinned required craft-fonts checkout. The focused
clipboard/menu regressions (7) and same-document child-window regression (1) also passed.

A fresh isolated native Wayland smoke test passed: the exported Collapse Ribbon action
changed `ui.ribbonCollapsed` from false to true, updated its checked state, and the resulting
window screenshot was inspected. The real `plasmashell` process requested `GetLayout`.
This checks one root-window action and transport; clipboard races, child-window export,
file drops, mixed DPI and portal interactions still require their own live acceptance.
Evidence: workspace artifact `native-smoke-gridcraft-ja5m4lvq`; CI log
`/tmp/gridcraft-kde-full-ci.log`. The dated results below describe the earlier implementation.

```sh
CARGO_BUILD_JOBS=2 cargo test -p gridcraft -p gridcraft-ui-egui --lib --bins desktop_menu
CARGO_BUILD_JOBS=2 cargo xtask ci
```

Local automated result (2026-10-21): offline workspace/all-targets Clippy with `-D warnings`
and the default-feature desktop build passed. `cargo test --offline -p gridcraft -p gridcraft-ui-egui --lib --bins --test system_theme
--test interface_scale` passed 26 host tests, 101 UI unit tests (including all five native-menu
regressions), five system-theme integration tests, and one interface-scale integration test.
Deferred native Copy/Cut/Paste is rejected after workbook replacement, while ordinary keyboard
clipboard events remain available. Other integration-test executables were not run in this
focused validation pass.

`cargo xtask assets` passed for all 42 attributed assets; `cargo xtask layers`
passed for all 16 workspace crates without layering violations.

Native Plasma smoke test (2026-10-21) also passed with `DISPLAY` unset: the existing Wayland
surface was registered, with no X11 window ID, and the real `org.kde.plasmashell` consumer
issued `GetLayout`, `AboutToShow` and `Event`. All eight text sections appeared. Bold changed
its checked state, About disabled document commands, and File → Close opened the existing
unsaved-workbook dialog; Cancel kept the synthetic workbook open. Selecting System appearance
rendered dark chrome from the real portal. Both light and System-dark screenshots were visually
reviewed. This does not certify dirty **window** close; the limitation above still applies.

Use a synthetic sample and a temporary `XDG_CONFIG_HOME` for live tests. Never test Save over a
working workbook or change the user's panel configuration automatically.

- Verify the actual window backend: a Wayland session can also contain XWayland windows.
- Test menu enabled/checked states after Bold, Undo, selection changes, document switching,
  and opening a modal dialog. A queued command from an old workbook must be rejected.
- Check typing a cell followed by menu Save/formatting: the draft must be committed, not lost.
- Check copy/paste both during cell editing and with a selected range, using synthetic content.
- Change the desktop light/dark setting while **System** is selected, then confirm explicit
  application Light/Dark selections do not change. Missing portals must not prevent launch.
- Check 100%, 125%, 150%, 175%, 200% scale, a narrow window, and moving between mixed-DPI
  monitors. Test native decorations and file-picker focus/cancellation separately.
- Close a dirty workbook from the menu: Save/Don't Save/Cancel behaviour must remain intact.

Unit tests cover the portable menu model and application dispatch. They do not establish
native Plasma rendering, physical mixed-DPI behaviour or file-picker focus correctness.
