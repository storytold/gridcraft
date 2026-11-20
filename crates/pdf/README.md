# gridcraft-pdf

A small PDF 1.7 writer for shaped Arabic text, vector drawing and RGBA images.

## Text outside WinAnsiEncoding

The 14 standard fonts are not embedded. Helvetica, Times and Courier can only carry
`WinAnsiEncoding` (CP1252); Symbol draws its maths glyphs in its own encoding and never takes the
fallback path. Text the WinAnsi faces cannot encode — Vietnamese `ạ ế ơ đ`, Czech `č ř ě`, Greek,
Cyrillic, the rest of Latin Extended — is set in an embedded, subset system TrueType face standing
in for the standard font: a Type0 font with a CIDFontType2 descendant, `Identity-H` encoding and a
ToUnicode CMap, so the printed text stays searchable and copyable. Marked content carries the
source `ActualText`, so copied text is exact even when a face maps two characters (say Greek `Α`
and Cyrillic `А`) to one glyph. The face is the metric-compatible
system family of the standard font (Arial for Helvetica, Times New Roman for Times, Courier New for
Courier; Liberation, DejaVu, Noto or FreeFont where those are not installed), so `advance()` and
`text_width()` measure exactly what is drawn. Nothing is committed to the repository: faces are
read from the operating system on first use. A face whose OS/2 `fsType` forbids embedding or
subsetting is refused, as is one with CFF outlines.

Characters the chosen face lacks (CJK and other scripts) keep the non-embedded Adobe-GB1 CID font
(`STSong-Light`, `UniGB-UTF16-H`). Arabic has its own shaped, embedded Type3 path. On the web there
are no font files to read, so text outside WinAnsiEncoding there keeps the CID fallback. A caller
can supply a face with `PdfDoc::set_unicode_font`; measuring keeps using the system's face.

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
