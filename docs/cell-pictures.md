# Pictures inside cells

Choose **Insert → Pictures → Place in Cell**, select a PNG or JPEG file, and insert it into the
active cell. The picture replaces that cell's contents while keeping its formatting. Undo
restores the previous contents.

The picture fits inside the cell without stretching. It follows row and column resizing,
sorting, filtering, insertion/deletion and GridCraft's internal copy/cut/paste. Clearing
contents removes it; clearing or pasting only formats keeps it. Save as XLSX to preserve the
embedded image and its alt text, or export to PDF for a rendered copy.

**Place over Cells** retains the existing floating-picture behavior.

The command API accepts the same placement:

```json
{"command":"insert.picture","params":{"path":"picture.png","placement":"cell","at":"B2","alt":"Product photo"}}
```

This initial feature handles static pictures. Pasting an image from the operating system's
clipboard, converting between floating and cell pictures, and the `IMAGE()` function remain
separate work. Formula references and arithmetic do not carry pictures: the calculation
engine uses a `#VALUE!` fallback, including when a picture is referenced from another cell.
Text-only exports also use that fallback. The picture cell still counts as occupied and
blocks a formula's spill range.

XLSX stores these pictures as rich values. Readers without support for that format may show
`#VALUE!` instead of the image. GridCraft reads embedded PNG/JPEG rich-value images; unsupported
rich content, oversized or damaged picture metadata, and broken image references produce import
warnings while the workbook opens with scalar fallbacks. It does not fetch linked
images from the internet.
