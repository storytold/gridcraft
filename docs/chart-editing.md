# Chart editing and export

Last updated: 2026-11-03. Reconciled with upstream main `df137b8`. Local CI validation
is recorded below; manual interoperability testing in Microsoft Excel remains outstanding.

## Scope

- Select chart elements with the mouse, Current Selection or arrow keys. Ctrl+1 opens the
  formatting pane. Delete removes supported elements; repeated deletion never falls through
  to deleting the chart. The final series is retained.
- Edit chart/plot fills and borders, primary-axis bounds and units, base-10 logarithmic scales,
  reverse order, number formats, tick marks and axis/gridline strokes.
- Edit series line/fill/marker colors and transparency, widths, smoothing, marker shape/size,
  column gap width and overlap. Use Home font controls for selected chart text without changing cells.
- Place an overlay legend and drag its box or the trendline equation label. Add/remove existing
  trendline types and toggle equation/R-squared labels through the chart menus and pane.
- Use chart context menus for selection, formatting, duplication, copy/cut, PNG/SVG export and
  local named templates. Templates apply to the same chart kind and can be the default for new charts.
  Templates capture visible base series colors as fixed RGBA format overrides, preserving
  explicit line/fill/marker overrides; they do not copy source data or trendline models.
  Per-category palettes in pie/doughnut and other category-colored charts are not captured by
  these per-series overrides. Changing base series colors does not replace explicit color overrides.
- `chart.duplicate` and `chart.paste {snapshot, at?}` return the new chart ID and are undoable.
  Editable clipboard paste requires the same workbook and a matching clipboard image; otherwise
  the image is pasted. The desktop host supplies `Services::paste_image`.
- `chart.export {chart?, format?, scale?, transparent?, path?}` returns image bytes as base64 or
  a saved path. PNG uses cached system fonts natively and bundled fallback fonts on wasm. SVG
  keeps text and vectors; fonts are not embedded. PDF/HTML chart text honors family and italic styling.

Supported settings use native chart XML where implemented, with a GridCraft extension for the
complete formatting model, including ChartEx. Native properties take precedence when read back.
Clipping, horizontal-bar axis/gridline roles and marker tokens are preserved on round-trip.
Classic DrawingML and ChartEx imports both discard out-of-range numeric formatting values after
merging native properties, keeping imported charts editable through the same command validation.
Styling a `ScatterLines` series does not introduce a circle marker when saving; explicit markers remain supported.

The review follow-up measures axis titles and trendline labels at their configured font size,
keeps non-cartesian category labels separate from data-label styling, and aligns minor ticks
with a nonzero manual minimum. Category legends retain their category colors. XLSX round-trips
preserve automatic series colors, explicit bar/area borders, combo axis titles, and independent
ChartEx gridlines. A default template and chart insertion share one undo step; invalid templates
produce a visible error rather than silently losing the selected appearance.

Fractional manual minima also anchor the minor-tick lattice. Combo formatting is recovered by
the native series index before being remapped to loaded series order. On native hosts without
fonts, the engine's PNG test accepts only the specific missing-font error and still checks SVG
and non-mutation; chart unit tests load their bundled test fallback on such hosts.

## Concurrent upstream work

[PR #2265](https://github.com/storytold/gridcraft/pull/2265) was open at review time, head
`195e3324`. It overlaps model, renderer, axis and XLSX files but is not a dependency of this port;
no source was copied from that PR.

Its `Series.format`/`SeriesFormat`, `AxisScale`, secondary-Y axis, deleted legend entries and
explicit legend box differ from this contribution's `ChartFormat.series` index map,
`ChartAxis`, primary-axis controls and chart-normalized annotation origins clamped to the plot area. It also preserves theme
colors and supports secondary scatter groups. Those features must be reconciled if both PRs land;
the types and serialized fields are not interchangeable. This port additionally covers text fonts,
log/reversed axes, line/marker widths and sizes, UI editing, templates, clipboard and image export.

## Code layout and current upstream integration

Chart rendering follows the shared module split: axes and ticks live in `chart/src/render/frame.rs`,
Cartesian series in `render/cartesian.rs`, markers and series styling in `render/lines.rs`,
legend entries in `render/legend.rs`, and trendline labels in `render/trend.rs`. The root renderer
retains dispatch and whole-group placement, and keeps its existing public re-exports.

The active chart command definitions in the current main live in
`engine/src/cmd/insert.rs`; validation, export and duplication are integrated there.
The older `cmd/insert/chart.rs` still exists upstream but is not declared by that
root module: editing it alone does not change the registered commands. Series changes in
`cmd/extra/chart.rs` retain formatting-index maintenance. Pivot charts and PDF text adaptation
follow the corresponding `cmd/pivot/charts.rs` and `cmd/print/draw.rs` modules.

The UI routes Home font commands through `ui-egui/src/app/chart_font.rs`, clipboard operations
through `ui-egui/src/app/clipboard.rs`, and template saving through
`ui-egui/src/chart_tools/templates.rs`; default template preferences are applied in
`ui-egui/src/app/commands.rs`. Chart picking and label dragging live in `grid/interact.rs`,
keyboard actions in `grid/keyboard.rs`, and menus in `grid/menus.rs`. Object selection and eight-handle resizing remain owned by
upstream `objects.rs`, using `selected_object`/`ObjRef`; chart-element selection
extends it rather than keeping a separate selected-chart field.
Dialogs and ribbon controls use their existing `dialogs/` and `ribbon/`
modules. Host clipboard-image access is declared in `services.rs`.

## Validation and limits

Chart command and font-routing integration tests are registered in the existing
`crates/engine/tests/it/main.rs` and `crates/ui-egui/tests/it/main.rs` binaries.
Relevant cases also live in chart unit tests, XLSX `trendline_roundtrip`/`roundtrip` tests and
UI chart template, font, numeric-control, resize and deletion tests. The new ScatterLines case
checks implicit/explicit no-marker styles, explicit markers and a second save.

The current integration retains upstream data tables in the model, renderer,
Format pane and Add Chart Element menu, and preserves paired-index area decimation.
Regression coverage includes formatting/data-table coexistence across repeated XLSX saves,
table preservation through chart duplication and undo, reversed category rendering, bounded
axis tick thinning, SVG font fallback, ChartEx default fills and stacked-bar writer defaults.
The dependency lock is based on current main plus the export dependency closure, without
carrying the unrelated AccessKit version updates from the old branch.

`cargo xtask ci` passed all 10 local gates on the reconciled source: i18n report, formatting,
Clippy, workspace tests, doctests, asset and layering checks, wasm32, parity and keytips. The
strict UI test harness used the pinned `craft-fonts` checkout; its Unicode glyph regressions
for U+1E9E and U+1EBB also passed. This is automated validation, not a claim of Microsoft
Excel interoperability.

External desktop clipboard exchange and Excel interoperability still need manual verification.
Unsupported XML, theme-based surface overrides, per-point formatting, secondary scatter axes,
individual legend-entry deletion, error bars, `.crtx` files and embedded SVG fonts remain gaps.
Some extension-only properties can become stale after another editor changes a workbook.

## Revision history

| Date | Change |
|---|---|
| 2026-11-03 | Reconcile chart PR with main `df137b8`; address review findings for axis ticks, reversed category/gridline alignment, Waterfall legends, template name limits, XLSX defaults, SVG font fallback and delayed image paste; all 10 local CI gates pass, manual Excel check pending |
| 2026-10-28 | Integrate upstream 0.13 module/lint changes; preserve palette colors in templates and validate native ChartEx strokes |
| 2026-10-27 | Adapt chart rendering, engine commands and UI behavior to the shared module layout; separate chart-font routing into app/chart_font.rs |
| 2026-10-23 | Follow-up review: fractional minor ticks, combo series-index mapping and fontless native test hosts |
| 2026-10-23 | Address chart review: role-aware text layout, ticks/legends, native XLSX defaults, template undo and contextual translations |
| 2026-10-23 | Port chart editing/export and persistence, preserve donor review fixes, document #2265 overlap |
