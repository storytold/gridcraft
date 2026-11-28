Horizontal bar/scatter combination charts now keep their orientation, invisible stack
offsets, numeric overlay coordinates, saved axis bounds, and reversed category order.
These settings also survive regenerated XLSX chart XML. This fixes floating valuation
ranges with a vertical reference-price line in the shared native, headless, and print
renderer.

Regression coverage uses XML and numeric data authored in code; no user workbook or
external asset is included. Verification includes rendering the reported workbook
locally without modifying it.
