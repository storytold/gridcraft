# GridCraft app icon (PLACEHOLDER)

**This icon is temporary.** Every Crafting App's icon is an engraved creature portrait that the
product owner makes (craftrules `standards/icon-design.md`); GridCraft's hasn't been made yet.
Until it is, this placeholder stands in: a simple ledger page with a title line, a header row, a
row-number column and a selected cell with its fill handle, on a full-bleed GridCraft-green tile. It is original geometry written by hand for this repository, deliberately
not modelled on any vendor's spreadsheet logo (no letters, no "X", no overlapping panels).

When the owner's art arrives, replace `gridcraft.svg`, run `packaging/icons.sh`, update this
README (provenance, creature) and the rows in `ATTRIBUTION.md`, and drop the word placeholder.

## Palette

Four colours: the three Crafting Apps placeholder colours plus GridCraft's darker app colour.

| Colour | Hex | Used for |
|---|---|---|
| Ink | `#0b0b0c` | Line work and contour |
| Paper | `#efe9dc` | The page |
| GridCraft green (app colour, **proposed**) | `#1f9d55` | The full-bleed field |
| GridCraft dark green (darker app colour) | `#147a40` | The header row and row-number column |

The app colour is a proposal pending the owner's approval: GridCraft green `#1f9d55`, with ink
(darker) variant `#147a40` for text and accents on light backgrounds. It differs from
DesignCraft's `#7bb51c`.

The header cells use the darker green, not the field green, because of macOS's Dark and tinted
icon appearances: for an app that ships no dark variant (an `.icns` has no slot for one), macOS 26
and later build it by replacing the field colour with a dark plate. Cells in the exact field green
are replaced too, and the icon loses all its green; in the darker green they stay green.

## Small sizes

The artwork follows Apple's app-icon guidance (HIG › App icons › Design): a minimal number of
shapes and no extremely thin lines, so it survives the system's 16–64 px renders. The page is a
3 × 3 grid with heavy strokes (no ink stroke under 12/512, so at least 1 px at 48 px); there are no
fine details such as a double rule or multiple title lines.

At 32 px and below even that grid blurs, so `gridcraft-small.svg` is a small-size cut of the same
design: the same tilted page, header row, row column and selected cell, but no title line, a
2 × 2 grid, thicker strokes and the selected cell filled solid (an outline can't survive 16 px).
`packaging/icons.sh` uses it for every render of 32 px or less (Linux hicolor 16–32, the `.ico`
16–32 frames, and the `.icns` 16, 16@2x and 32 slots). Everything larger uses `gridcraft.svg`.

## Geometry

A 512 × 512 viewBox, clipped to a rounded square with `rx=112`. Windows and Linux use the full
tile. The macOS icons (`.icns`, `gridcraft-macos-512.png`) sit on Apple's grid: an 824/1024
body with a transparent margin.

## Files

| File | Use |
|---|---|
| `gridcraft.svg` | Canonical artwork; also `hicolor/scalable` |
| `gridcraft-small.svg` | Small-size cut for renders of 32 px and below |
| `gridcraft-1024.png` | 1024 px render of the full tile |
| `gridcraft-macos-512.png` | Runtime Dock icon on macOS (for `apps/gridcraft` to embed) |
| `gridcraft.icns` | macOS bundle icon (`CFBundleIconFile`) |
| `gridcraft.ico` | Windows exe icon, 16–256 px (embedded by `apps/gridcraft/build.rs`) |
| `hicolor/<size>/apps/ai.storyteller.gridcraft.png` | Linux icon theme, 16–512 px; the 256 px one is also the runtime window icon on Windows and Linux |
| `hicolor/scalable/apps/ai.storyteller.gridcraft.svg` | Linux scalable icon |
| `LICENSE.txt` | MIT OR Apache-2.0 |

The app ID is `ai.storyteller.gridcraft`. It's used for the hicolor icon names, the Wayland app
ID and `packaging/linux/ai.storyteller.gridcraft.desktop`.

## Regenerate

Edit `gridcraft.svg` (and `gridcraft-small.svg` to match), then run `packaging/icons.sh`. It needs `resvg`, and `iconutil` for the
`.icns` (macOS only). It packs the `.ico` with `cargo xtask ico`. The macOS DMG background embeds the icon, so also run
`packaging/macos/dmg/generate.py` to refresh `background.tiff`.
