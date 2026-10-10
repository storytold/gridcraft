# Chart parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created from a source audit of the chart model, renderer and XLSX chart parts) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Excel's chart types and chart elements against GridCraft. Part of
[target-app-parity.md](target-app-parity.md). **Breadth ~50%, ready ~30%, 40–60 h** (estimated).

Sources: `crates/model/src/features.rs` (`ChartKind`, `Chart`, `Series`), `crates/chart/src`
(layout and rendering), `crates/xlsx/src/chart.rs` (DrawingML chart parts), merged PRs #62 and #116,
open PRs #158, #161, and issue #190.

## Chart types

| Excel type | Excel subtypes | GridCraft | Notes |
|---|---|---|---|
| Column | clustered, stacked, 100% stacked, 3-D clustered/stacked/100%, 3-D column | 3 of 7 | no 3-D |
| Bar | clustered, stacked, 100%, 3-D ×3 | 3 of 6 | no 3-D |
| Line | line, stacked, 100% stacked, with markers ×3, 3-D line | 3 of 7 (line, with markers, stacked) | no 100% stacked; smoothed lines landed (#62) |
| Pie | pie, 3-D pie, pie of pie, bar of pie, doughnut | 2 of 5 (pie, doughnut) | no exploded, pie of pie, bar of pie |
| Area | area, stacked, 100%, 3-D ×3 | 2 of 6 | no 100%, no 3-D |
| X Y (scatter) | markers, smooth lines (±markers), straight lines (±markers) | 2 of 5 | no smooth lines |
| Bubble | bubble, 3-D bubble | 1 of 2 | |
| Stock | high-low-close, open-high-low-close, volume-HLC, volume-OHLC | 1 kind | combo/stock lose their kind in XLSX (PR #158) |
| Surface | 3-D surface, wireframe, contour, wireframe contour | 0 of 4 | |
| Radar | radar, with markers, filled | 1 of 3 | |
| Treemap, Sunburst | 1 each | 2 of 2 | lose their kind in XLSX (PR #161) |
| Histogram, Pareto | 2 | 1 of 2 | no Pareto |
| Box & Whisker | 1 | 1 of 1 | loses its kind in XLSX (PR #161) |
| Waterfall, Funnel | 2 | 2 of 2 | lose their kind in XLSX (PR #161) |
| Filled map | 1 | 0 | needs geographic boundary data (licensing decision) |
| Combo | custom combinations, secondary axis | yes (line on secondary axis) | |
| **Total** | ~60 subtypes | **25 (~42%)** | usage-weighted breadth higher (~65%): column, bar, line, pie and scatter dominate |

## Chart elements and formatting

| Element | Excel | GridCraft |
|---|---|---|
| Move and resize the chart object | yes | move yes; **resize broken** (#190) |
| Chart title, axis titles | text, linked to cells, full formatting | text only |
| Legend | position, overlay, formatting, per-entry delete | position only |
| Data labels | value, category, series, percentage, from cells, callouts, position | one on/off flag |
| Gridlines | major/minor per axis, formatting | one on/off flag |
| Axis options | min/max/units, log scale, reverse order, crossing point, display units, date axis, number format | **missing** (auto only) |
| Secondary axis | any series | line series in combo charts |
| Trendlines | linear, exponential, logarithmic, polynomial, power, moving average, equation and R² | **missing** |
| Error bars | fixed, %, standard deviation/error, custom | **missing** |
| Up/down bars, high-low lines, drop lines, series lines | yes | **missing** |
| Data table under the chart | yes | **missing** |
| Per-series / per-point fill, line, marker, effects | Format pane for every element | series colour only; Format Chart pane covers chart-level options |
| Chart styles and colour palettes | ~16 styles × colour sets | style number stored, own palettes |
| Chart sheets | yes | **missing** (skipped with a warning on open) |
| Chart templates (`.crtx`) | yes | **missing** |
| Recommended Charts | yes | partial (Insert ▸ Recommended) |
| Switch Row/Column, Select Data | yes | yes (Switch on charts read from files landed in #116) |
| Sparklines (line, column, win/loss, markers, axis) | yes | line, column, win/loss |
| PivotCharts | yes | **missing** |
| Animation on data change | yes (Windows) | no |

## XLSX round-trip

Charts are read from and written to DrawingML chart parts, but only the fields the model holds
survive: anything else (axis scaling, label options, trendlines, per-point formats, chartex
extensions) is lost when GridCraft saves a workbook it opened. That is the main reason charts are
**~30% ready**: a user who opens an Excel workbook with charts and saves it gets simpler charts
back.

## Work (40–60 h)

1. Kinds survive XLSX round-trip (merge PRs #158, #161): 2–4 h. Chart resizing (#190): 1–2 h.
2. Preserve unmodelled chart XML on round-trip (keep the original part, patch only edited fields): 6–10 h.
3. Axis options, label options, gridlines per axis, legend formatting: 10–15 h.
4. Trendlines and error bars (maths shared with LINEST/LOGEST/GROWTH): 6–10 h.
5. Per-element Format pane parity, smoothed lines, exploded pie, pie/bar of pie, 100% variants: 8–12 h.
6. 3-D column/bar/pie/area, surface and contour, Pareto: 8–12 h.
7. Chart sheets, PivotCharts, templates: 6–10 h. Filled map: owner decision on boundary data.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | Smooth lines (#62) and Switch Row/Column on file charts (#116) landed; chart resize bug (#190) |
| 2026-10-10 | major | Created from a source audit: 25 kinds (25 of ~60 subtypes), element gaps, round-trip loss |
