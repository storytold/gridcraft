Horizontal bar/XY charts keep overlay labels on their numeric points and use
the first saved axis settings for overlays sharing an axis pair. Bar rectangles
stay inside the plot. Reversed horizontal axes position category labels,
numeric ticks, bars, and trendlines consistently, including with secondary bars
and with an unfilled series.

XLSX export reports the Office axis limit when regenerating a horizontal bar/XY
chart would need a third axis pair and lose a grouping or scale. Regression inputs
are original chart data authored in the Rust tests.
