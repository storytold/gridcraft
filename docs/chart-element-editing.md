# Chart element selection and editing

This change builds on the chart-format model and XLSX persistence described in
[chart-formatting.md](chart-formatting.md). It adds editing controls, not a second
copy of the chart model. Every accepted edit is sent through engine commands.

Clicking a data point selects its series. Group selection marks individual bars
and explicit marker glyphs. Connecting lines and interpolated segments of a
smoothed path do not produce selection boxes. Gridlines are selected as a group. Arrow keys cycle
the elements that actually exist, and
Ctrl/Cmd+1 opens chart formatting when a chart is selected.

The formatting pane offers an element selector, Fill & Line, element options,
and Size & Properties. It includes axis bounds/units, primary logarithmic and
reversed axes, major/minor tick marks and strokes; series lines/markers and
smoothing; text roles; trendline controls; and the upstream Data Table controls.
Colors use the shared palette, retaining HEX entry. Numeric controls keep drafts
and support stepping from inherited defaults instead of rejecting an empty box.

Home-tab font commands target the selected chart text role while the chart has
selection, instead of silently formatting its highlighted source cells. Chart
text styles inherit the chart-wide style; cell formatting is unchanged once
chart selection is released. The chart context menu offers element-relevant
formatting and deletion actions and preserves Select Data and chart-kind controls.
Legend and trendline-label groups can be positioned by dragging the group.

This is not full Excel formatting parity: error bars, 3-D, effects, arbitrary
per-point selection/styles and the complete data-label layout model are not implemented.
Series hit testing uses rendered colors because primitives do not yet carry
stable series/point IDs; coincident colors can make selection ambiguous, and
line-only or filled series have more limited selection feedback.
Standalone image export, clipboard/duplication, and installation-local templates
are separate subsequent parts. Actual UI captures and manual Excel inspection
remain distinct from automated rendering and persistence tests.
