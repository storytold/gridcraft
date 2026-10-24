# Chart editing and export

Last updated: 2026-10-23.

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
- `chart.duplicate` and `chart.paste {snapshot, at?}` return the new chart ID and are undoable.
  Editable clipboard paste requires the same workbook and a matching clipboard image; otherwise
  the image is pasted. The desktop host supplies `Services::paste_image`.
- `chart.export {chart?, format?, scale?, transparent?, path?}` returns image bytes as base64 or
  a saved path. PNG uses cached system fonts natively and bundled fallback fonts on wasm. SVG
  keeps text and vectors; fonts are not embedded. PDF/HTML chart text honors family and italic styling.

Supported settings use native chart XML where implemented, with a GridCraft extension for the
complete formatting model, including ChartEx. Native properties take precedence when read back.
Clipping, horizontal-bar axis/gridline roles and marker tokens are preserved on round-trip.
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
chart-relative legend box differ from this contribution's `ChartFormat.series` index map,
`ChartAxis`, primary-axis controls and plot-relative annotation placement. It also preserves theme
colors and supports secondary scatter groups. Those features must be reconciled if both PRs land;
the types and serialized fields are not interchangeable. This port additionally covers text fonts,
log/reversed axes, line/marker widths and sizes, UI editing, templates, clipboard and image export.

## Validation and limits

Chart command and font-routing integration tests are registered in the existing
`crates/engine/tests/it/main.rs` and `crates/ui-egui/tests/it/main.rs` binaries.
Relevant cases also live in chart unit tests, XLSX `trendline_roundtrip`/`roundtrip` tests and
UI chart template, font, numeric-control, resize and deletion tests. The new ScatterLines case
checks implicit/explicit no-marker styles, explicit markers and a second save.

External desktop clipboard exchange and Excel interoperability still need manual verification.
Unsupported XML, theme-based surface overrides, per-point formatting, secondary scatter axes,
individual legend-entry deletion, error bars, `.crtx` files and embedded SVG fonts remain gaps.
Some extension-only properties can become stale after another editor changes a workbook.

## Revision history

| Date | Change |
|---|---|
| 2026-10-23 | Follow-up review: fractional minor ticks, combo series-index mapping and fontless native test hosts |
| 2026-10-23 | Address chart review: role-aware text layout, ticks/legends, native XLSX defaults, template undo and contextual translations |
| 2026-10-23 | Port chart editing/export and persistence, preserve donor review fixes, document #2265 overlap |
