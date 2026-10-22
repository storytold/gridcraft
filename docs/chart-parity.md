# Chart parity

> **Last reviewed:** 2026-10-18 · **Last updated:** 2026-10-18 · **Change:** trivial (stale chart notes: stock/combo, treemap/sunburst, box and waterfall kinds survive XLSX round-trip; PRs #158 and #161 merged) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Excel's chart types and chart elements against GridCraft. Part of
[target-app-parity.md](target-app-parity.md). **Breadth ~50%, ready ~30%, 40–60 h** (estimated).

Sources: `crates/model/src/features.rs` (`ChartKind`, `Chart`, `Series`), `crates/chart/src`
(layout and rendering), `crates/xlsx/src/chart.rs` (DrawingML chart parts), merged PRs #62, #116,
#158 and #161, and issue #190.

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
| Stock | high-low-close, open-high-low-close, volume-HLC, volume-OHLC | 1 kind | |
| Surface | 3-D surface, wireframe, contour, wireframe contour | 0 of 4 | |
| Radar | radar, with markers, filled | 1 of 3 | |
| Treemap, Sunburst | 1 each | 2 of 2 | |
| Histogram, Pareto | 2 | 1 of 2 | no Pareto |
| Box & Whisker | 1 | 1 of 1 | exclusive (default) and inclusive quartiles per series; XLSX round-trip; `chart.set {quartiles}` changes all series |
| Waterfall, Funnel | 2 | 2 of 2 | lose their kind in XLSX (PR #161) |
| Box & Whisker | 1 | 1 of 1 | |
| Waterfall, Funnel | 2 | 2 of 2 | |
| Filled map | 1 | 0 | needs geographic boundary data (licensing decision) |
| Combo | custom combinations, secondary axis | yes (line on secondary axis) | |
| **Total** | ~60 subtypes | **25 (~42%)** | usage-weighted breadth higher (~65%): column, bar, line, pie and scatter dominate |

## Chart elements and formatting

| Element | Excel | GridCraft |
|---|---|---|
| Move and resize the chart object | yes | yes (corner handle, Format Chart Area Size fields) |
| Chart title, axis titles | text, linked to cells, full formatting | text only |
| Legend | position, overlay, formatting, per-entry delete | position only |
| Data labels | value, category, series, percentage, from cells, callouts, position | one on/off flag |
| Gridlines | major/minor per axis, formatting | one on/off flag |
| Axis options | min/max/units, log scale, reverse order, crossing point, display units, date axis, number format | **missing** (auto only) |
| Multi-level category axis (a category range two or more columns wide beside the values: year beside month) | the outer levels grouped under the labels with separators | read (`multiLvlStrRef`), drawn (rows under the labels; columns beside them in bar charts; up to three outer levels), written back with Excel's level caches |
| Secondary axis | any series | line series in combo charts |
| Trendlines | linear, exponential, logarithmic, polynomial, power, moving average, equation and R² | all six types, several per series, name, forward/backward forecast, set intercept, equation and R² label, line colour/width/dash; read, written and drawn on 2-D column, bar, line, area, scatter and bubble series; `chart.addElement` `trendline` adds/removes them. No Format Trendline pane, label position/number format not kept |
| Error bars | fixed, %, standard deviation/error, custom | **missing** |
| Up/down bars, high-low lines, drop lines, series lines | yes | **missing** |
| Data table under the chart | yes | **missing** |
| Per-series / per-point fill, line, marker, effects | Format pane for every element | series colour only; Format Chart pane covers chart-level options |
| Chart styles and colour palettes | ~16 styles × colour sets | style number stored, own palettes |
| Chart sheets | yes | **kept, not editable**: not shown, and written back unchanged in their place on save (#727); left out when a sheet they plot is deleted or renamed. No F11 / Move Chart > New sheet |
| Chart templates (`.crtx`) | yes | **missing** |
| Recommended Charts | yes | dialog with recommendations from the data's shape (own heuristic) and an All Charts list, live previews (#215) |
| Switch Row/Column, Select Data | yes | yes (Switch on charts read from files landed in #116) |
| Recommended Charts | yes | partial (Insert ▸ Recommended) |
| Switch Row/Column, Select Data | yes | yes (Switch on charts read from files landed in #116); Select Data dialog: data range, remove series, Horizontal (Category) Axis Labels (#287). No series add/edit or hidden/empty cell options |
| Sparklines (line, column, win/loss, markers, axis) | yes | line, column, win/loss |
| PivotCharts | yes | Insert ▸ PivotChart (`insert.pivotChart`): bound to a PivotTable's report, rebuilt on refresh and layout changes, `c:pivotSource` round trip; no field buttons, innermost category level only |
| Animation on data change | yes (Windows) | no |

## XLSX round-trip

Charts are read from and written to DrawingML chart parts, but only the fields the model holds
survive: anything else (axis scaling, label options, error bars, per-point formats, chartex
extensions) is lost when GridCraft saves a workbook it opened. That is the main reason charts are
**~30% ready**: a user who opens an Excel workbook with charts and saves it gets simpler charts
back.

Series data, categories and names may refer to defined names, as dynamic (OFFSET) ranges do:
Excel writes a workbook-scoped name as `[0]!Name` in `<c:f>`. GridCraft reads it as the bare
name and writes it back with `[0]!`; a sheet-scoped name is written `Sheet!Name`. Excel needs the
qualifier: with a bare name in `<c:f>` it draws the whole chart blank, without a repair prompt
(observed in Excel for Windows, Microsoft 365). External books (`[1]Sheet1!…`) are kept as they
are (#1686). Such series get no `numCache`/`strCache` on save (#1690).

## Work (40–60 h)

1. Chart resizing (#190): 1–2 h.
2. Preserve unmodelled chart XML on round-trip (keep the original part, patch only edited fields): 6–10 h.
3. Axis options, label options, gridlines per axis, legend formatting: 10–15 h.
4. Error bars; Format Trendline pane (trendlines landed): 4–6 h.
5. Per-element Format pane parity, smoothed lines, exploded pie, pie/bar of pie, 100% variants: 8–12 h.
6. 3-D column/bar/pie/area, surface and contour, Pareto: 8–12 h.
7. Chart sheets, templates and PivotChart field buttons: 5–8 h. Filled map: owner decision on boundary data.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-18 | trivial | Stale notes: stock/combo, treemap/sunburst, box and waterfall kinds survive XLSX round-trip (PRs #158, #161) |
| 2026-10-16 | trivial | Series that refer to defined names (`[0]!Name`, `Sheet!Name`) resolve instead of showing `#NAME?`, and are saved qualified, as Excel needs (#1686) |
| 2026-10-16 | minor | Box & whisker quartiles default to exclusive, honor each series on draw and XLSX round-trip; `chart.set {quartiles}` changes the method (#1453) |
| 2026-10-13 | minor | Multi-level category axes: read, drawn (outer levels under or beside the labels), written back with level caches |
| 2026-10-12 | trivial | chart sheets are kept on save, still not editable (#727) |
| 2026-10-11 | minor | Trendlines: all six types modelled, round-tripped through `c:trendline`, drawn with forecast and equation/R² labels; `chart.addElement` `trendline` |
| 2026-10-11 | minor | PivotCharts (`insert.pivotChart`) |
| 2026-10-11 | minor | Recommended Charts dialog with previews (#215) |
| 2026-10-11 | minor | Select Data dialog with category axis labels; a blank top-left cell makes the first column the categories (#287) |
| 2026-10-10 | minor | Smooth lines (#62) and Switch Row/Column on file charts (#116) landed; chart resize bug (#190) |
| 2026-10-10 | major | Created from a source audit: 25 kinds (25 of ~60 subtypes), element gaps, round-trip loss |
