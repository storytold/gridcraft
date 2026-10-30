# gridcraft-pdf

A small, dependency-free PDF 1.7 writer for vector drawing and RGBA images.

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

## Thai and other scripts: the fallback font

The 14 standard PDF fonts only cover WinAnsiEncoding. Register one TrueType/OpenType font and
text it covers (Thai, for example) is shaped with harfrust and embedded:

```rust
let bytes = std::fs::read(r"C:\Windows\Fonts\THSarabunNew.ttf")?;
gridcraft_pdf::register_fallback_font(bytes, false)?; // `true` registers the bold face
```

The GridCraft app and `gridcraft-cli` do this at start-up from `GRIDCRAFT_PDF_THAI_FONT` (and
`GRIDCRAFT_PDF_THAI_FONT_BOLD`); the app otherwise uses a craft-fonts face declared for the
`Thai` script, when the build embeds one. Font files are never committed to this repository.

- The whole font file is embedded (no subsetting) as `CIDFontType2` with `Identity-H`, a `/W`
  array of the glyphs used and a `ToUnicode` CMap, so the text can be searched and copied.
- Fonts whose `OS/2.fsType` forbids embedding (restricted, or bitmap-only) are refused.
- Without a bold face, bold text is the regular face filled and stroked.
- `text_width` measures shaped runs, so wrapping and alignment match what is drawn.
