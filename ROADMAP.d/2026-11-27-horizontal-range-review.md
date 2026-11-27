Horizontal bar/XY chart review fixes keep saved-axis ticks inside their bounds,
share automatic overlay Y scales per axis pair, and exclude XY points from category
slots. Reversed value axes position bars, tick labels, and overlays consistently.

Series fill and explicit outline colours are independent, so unfilled outlined
bars remain visible while fully transparent stack offsets stay invisible. XLSX
normalization rotates axis metadata when bars become columns, and horizontal
bar/XY exports assign incompatible axis families to separate pairs.

Regression fixtures are authored as Rust data and XML in the tests. Rendering and
chart XML round-trip checks cover these failures; no external workbook is included.
