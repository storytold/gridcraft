# gridcraft-pdf

A small PDF 1.7 writer for vector drawing and RGBA images.

Native exports embed an available TrueType font for characters missing from WinAnsi,
including Romanian letters. The embedded outlines and ToUnicode map preserve both
printing and text extraction. Font metrics come from `ttf-parser`. No font files are
distributed in this repository. The loader checks Windows Arial, Linux Liberation Sans,
and macOS Arial; `GRIDCRAFT_PDF_FONT_DIR` can supply those font files explicitly.
Only fonts permitting outline embedding are used. Fonts are embedded in full.
If no suitable font is available (including on WASM), the existing Adobe-GB1 fallback
remains; that fallback does not cover every Unicode character.

## Installed workbook fonts
Native spreadsheet printing resolves the workbook's family name and bold/italic style against
installed standalone TrueType files. Supported faces are embedded for all characters, and
layout uses their advance widths. The lookup reads internal family names from system/user
font folders and `GRIDCRAFT_PDF_FONT_DIR`; it does not construct paths from workbook names.
The explicit directory override is searched before system/user roots. Missing Unicode glyphs
use a fallback matching the selected face's bold/italic style. Glyph metrics and missing-glyph
results are cached per face, capped at 4096 characters to bound memory usage.
Missing WinAnsi characters draw and measure the same styled Helvetica fallback.
Embedded text uses document-local character codes and an explicit CID-to-glyph map.
ToUnicode maps each original character independently, including supplementary-plane characters
and characters sharing one glyph across pages. Each face can encode 65535 distinct characters
per document; further characters use the existing fallback path.
No installed font bytes are added to this repository. Faces must permit outline embedding.
The native resolver currently supports at most 32 named faces per process, up to 16 MiB per
file. TrueType collections, CFF outlines and variable-font instance selection are not yet
supported. Missing/unsupported faces and WASM retain the standard-family fallback.

Named-font loading does not hold the shared font-cache lock during filesystem access.
Up to 256 missing family/style combinations are cached for the process lifetime,
independently of the embedded-font limit. Unreadable or malformed matching font files
are skipped so a later eligible catalog entry can be used.

## API migration

The geometry-taking methods now use `Point` and `Rect` values to name their coordinates, and
`image_rgba` groups its pixel dimensions into an array. Update calls to the four changed methods as
follows:

```rust
use gridcraft_pdf::{Font, PdfDoc, Point, Rect};

let mut doc = PdfDoc::new();
let page = doc.add_page(612.0, 792.0);

// Before: page.link_page(20.0, 30.0, 80.0, 16.0, 1, 40.0, 50.0);
page.link_page(Rect::new(20.0, 30.0, 80.0, 16.0), 1, Point::new(40.0, 50.0));

// Before: page.stroke_line_dashed(0.0, 0.0, 100.0, 40.0, 1.0, [0, 0, 0], &[4.0, 2.0]);
page.stroke_line_dashed(Point::new(0.0, 0.0), Point::new(100.0, 40.0), 1.0, [0, 0, 0], &[4.0, 2.0]);

// Before: page.text_rotated(20.0, 40.0, 12.0, Font::Helvetica, [0, 0, 0], "Label", 0.2);
page.text_rotated(Point::new(20.0, 40.0), 12.0, Font::Helvetica, [0, 0, 0], "Label", 0.2);

let rgba = [255, 0, 0, 255];
// Before: page.image_rgba(10.0, 10.0, 24.0, 24.0, &rgba, 1, 1);
page.image_rgba(Rect::new(10.0, 10.0, 24.0, 24.0), &rgba, [1, 1]);
```

`Point::new(x, y)` uses the page's top-left origin, where y increases downward. `Rect::new(x, y,
width, height)` describes the destination or link rectangle in the same coordinate system. The
`image_rgba` pixel-size array is `[width, height]`.
