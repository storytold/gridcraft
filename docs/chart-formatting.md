# Chart formatting and XLSX persistence

Chart appearance is stored in `Chart.format`, separately from the source ranges
and values. `insert.chart` and `chart.set` accept a `format` object; commands
validate it before changing the workbook, so a rejected format also leaves undo
history and other chart properties intact.

The format covers primary-axis bounds and units, logarithmic and reversed axes,
major/minor ticks, axis and gridline strokes, chart and plot surfaces, per-role
text, series strokes and fills, markers, smoothing, bar gap/overlap and annotation
positions. Missing options inherit the normal chart defaults. Text overrides
inherit the whole-chart text style, with axis roles resolved independently.

General-format logarithmic labels preserve fractional decades rather than
rounding them with a linear step. Newly inserted charts use native theme-color
style 2, while explicit style choices remain intact. Stock exports preserve a
native dash marker on the close series through repeated saves.

Rendering applies the same model to drawing primitives consumed by the UI and
print/HTML/PDF paths. Tick sampling is bounded for hostile or dense intervals;
reversed category axes keep data-table columns aligned with the plotted data.
The existing native chart data-table model is retained.

XLSX writes supported properties as DrawingML and stores the remaining format in
a GridCraft extension. Import validates both native fields and extension values.
Native visible styling takes precedence where present, while a hidden gridline
retains its saved extension style. Pie/doughnut slices and stock defaults retain
their native series behavior instead of receiving a series-wide fill/line.
Repeated-save tests cover classic charts and ChartEx.

The extension is intended for GridCraft round trips; other editors may discard
extension-only properties. Automated round trips do not establish Excel visual
parity. Secondary scatter axes, error bars, 3-D, and full per-point/data-label
formatting are outside this change. Element selection and editing controls,
standalone image export and clipboard actions are subsequent parts of the
contribution series; local chart templates are implemented in the templates PR.
