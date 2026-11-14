# Column metrics and fixed-picture probe

Contributor-original fixtures for [#1603](https://github.com/storytold/gridcraft/issues/1603),
complementing the implementations in [#2257](https://github.com/storytold/gridcraft/pull/2257),
[#2258](https://github.com/storytold/gridcraft/pull/2258),
[#1349](https://github.com/storytold/gridcraft/pull/1349) and
[#1272](https://github.com/storytold/gridcraft/pull/1272). This is a reproducible observation
protocol, not another implementation or a claim that those branches were validated.

## Generate

```sh
cargo run -p gridcraft-xlsx --example column_metrics -- plan/excel/column-metrics
```

The example creates four original packages and four `-saved.xlsx` packages read and written
by the current checkout. It uses no templates, vendor assets, fonts, or downloaded issue files.
The single blue pixel is generated in code. Normal style fonts are named, never embedded.

A:H have raw OOXML widths `1, 2.5, 5, 8.43, 10, 13.7, 20, 30`. I has no explicit width;
`baseColWidth=8` tests the default separately. Row 2 contains 2870000 with built-in format 3.
Both pictures have extents 952500 × 476250 EMU (75 × 37.5 pt): a one-cell anchor at B5 and
an absolute anchor at (30, 150) pt from the sheet origin. This separates physical picture
extents from endpoints calculated with the importing application's column metrics.

## Native Mac observation, 2026-11-08

Microsoft Excel for Mac **16.114.4 (26110224)**, macOS desktop, French interface, Normal view,
100% zoom, Retina display. Font and size were confirmed in the ribbon for each original.
All eight generated packages opened without a repair prompt. No file was saved by Excel.
Test workbooks were closed and Excel was quit afterward.

Original column spans below were read from visible gridline positions at 100% zoom: two
screenshot pixels per display point. Allow approximately 0.5 pt for reading a raster edge;
these are screen observations, not an object-model query or an exhaustive rounding rule.

| Normal font | A | B | C | D | E | F | G | H | I (default) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Calibri 11 | 6 | 15 | 30 | 51 | 60 | 82 | 120 | 180 | 53 |
| Calibri 12 | 6 | 15 | 30 | 51 | 60 | 82 | 120 | 180 | 53 |
| Calibri 14 | 7 | 17 | 35 | 59 | 70 | 96 | 140 | 210 | 61 |
| Arial 16 | 9 | 22 | 45 | 76 | 90 | 123 | 180 | 270 | 79 |

Units: display points. Identical raw OOXML widths do not imply identical physical widths
across Normal fonts. The default column is a separate case, not simply raw width 8.

At baseline commit `2c58c769`, each saved workbook visibly changed picture width, with
larger distortion for larger Normal fonts. Calibri 14's selected one-cell picture had
ribbon size **1.04 × 0.52 inches** in the original and **1.22 × 0.63 inches** after the
GridCraft round trip (width × height; ribbon rounded to two decimals). The absolute picture
also moved and widened visibly. Existing #1272 addresses this fixed-anchor class; this
observation adds native Excel Mac evidence, not an independent reason to duplicate its fix.
Explicit column spans remained broadly similar on reopening the saved packages, with small
quantization shifts visible in some cases. This does not validate GridCraft's own rendering.

## Repeat against a candidate

1. Generate originals and saved copies on the baseline, then generate into a different local
   directory on the candidate branch. Keep the originals identical.
2. Open each pair in installed Excel. Confirm font/size and 100% zoom, record version/platform,
   and check for repair prompts. Read gridline spans, keeping default I separate from A:H.
3. Select each picture and inspect Picture Format's width/height; compare physical size and
   position to the original. Compare the same fixture in GridCraft separately to assess its UI.
4. Record observations with measurement precision and branch commit. Do not infer Windows,
   web, other Excel versions, or untested fonts from these Mac results.
5. Close test documents without saving and quit Excel. Keep reference screenshots and any
   reference-app saves only in the ignored local notebook; never commit them.

A follow-up should run the existing candidate PRs against this matrix before proposing more
production changes. Default-width origin, small-width rounding, and fixed physical picture
geometry must be assessed independently.
