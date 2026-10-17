# Linux equal-zoom reproduction (#1185)

```sh
cargo run --release -p gridcraft-ui-egui --example linux_scale_repro -- linux-scale-synthetic.xlsx
```

The generator uses our XLSX writer plus an original PNG. It emits a standard absolute picture
anchor with explicit 320 × 160-pixel extents, so a two-cell anchor cannot stretch the picture
when a comparison application's substitute fonts change column widths. All cells, pixels and
XML are generated in code; no vendor workbook, real organization/person or font file is used.
The example's ZIP dev dependency is the same locked crate already used by the XLSX library.
It does not add a production dependency or new package version.

Open the same file in GridCraft and LibreOffice Calc, at 50%, 100% and 150% worksheet zoom,
without compensating via interface or desktop scale. On GNOME X11/Linux, with native desktop
scale and GridCraft interface size both 100%, comparison against LibreOffice 24.2.7.2 gave:

| Zoom | GridCraft before PNG | Linux fix PNG | LibreOffice PNG | Fix / LibreOffice row pitch | Fix / LibreOffice Chinese ink height |
|---|---|---|---|---|---|
| 50% | 128 × 64 | 160 × 80 | 153 × 80 | 12 / 12 | 7 / 6 |
| 100% | 256 × 128 | 320 × 160 | 309 × 160 | 24 / 24 | 15 / 15 |
| 150% | 384 × 192 | 480 × 240 | 465 × 240 | 36 / 36 | 24 / 23 |

All sizes are measured screen pixels from each application's own window backing pixmap,
without screenshot resizing. Chinese ink is the dark-pixel extent of the A5 row, excluding
its grid border. The height/pitch agreement and canonical image dimensions verify the removed
20% reduction; small font rasterization differences are not treated as identical pixels.

Limits: LibreOffice's horizontal image import differs by about 3–4% here, and column widths
also depend on its Calibri substitution (Calibri is unavailable on this Linux host). Those
separate font/import conversions are not corrected by changing GridCraft's display scale.
This is not a claim of complete pixel-identical layout parity. Windows, macOS, BSD and web
retain the existing density until measured; #1185's Windows report is not closed by this change.

The native before/after screenshots use local 0.6.1 builds that both include the separate CJK
font-path and bitmap-EMF/import-notice fixes. The source PR's display-scale patch is based
directly on main and does not include those other patches. Tests run in an isolated Flatpak
with private networking/task file grants; the LibreOffice reference uses a separate profile,
read-only loading and local named-pipe UNO control. Both test interfaces are stopped afterward.
