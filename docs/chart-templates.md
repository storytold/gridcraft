# Local chart appearance templates

> Last updated: 2026-11-05. Local presets and command contracts; Excel template interchange remains outside scope.

Use **Save as Chart Template…** in a chart's context menu to save its appearance.
The **Chart Templates** submenu lists templates for the same chart kind, with
Apply, Use for new charts, and Delete Template actions. Saving a template makes
it the default for subsequent UI insertions of that kind; choosing another
default clears the previous one. Explicit insertion options take precedence.

Templates keep the style, legend position, data-label and gridline settings,
format overrides, visible series colors, doughnut hole size and first-slice
angle. They omit source ranges, values, names, chart titles, trendlines, object
identity/placement, and data-table settings. Applying a template changes
appearance through `chart.set`, including its normal validation and undo/redo.
Saving a template does not alter the source workbook.

Templates are stored in this installation/browser profile's UI preferences, independently of
workbook XLSX files. They are not Excel `.crtx` files and are not transferred to
another installation or browser profile. Invalid serialized entries are discarded individually;
an invalid default that fails engine validation reports its error and falls
back to an ordinary chart insertion. Defaults expand into a single engine
command so journal replay and capped undo history agree with the visible result.

The UI command API accepts `chart.saveTemplate {chart, name}`,
`chart.applyTemplate {chart, template}`, `chart.defaultTemplate {template, on}`,
and `chart.deleteTemplate {template}`. Template indices are zero-based. Names
are trimmed and limited to 80 Unicode characters; saving the same name ignoring
case replaces it within that chart kind. The local collection holds at most
256 entries. These UI preference commands are available through `SheetApp::run`
and the UI control channel, while the engine alone remains preference-free.

The feature depends on the chart-format model, not on the separate element
editor, image exporter, or clipboard workflow.

## Revision history

| Date | Change |
|---|---|
| 2026-11-05 | Independent local appearance presets, per-kind defaults, validated application, delete actions and persistence/undo/journal regressions. |
