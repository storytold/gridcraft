# GridCraft app icon

The icon is the owner-supplied green "G" mark (a folded ribbon on a transparent background; no
tile). `gridcraft.svg` embeds a 512 px PNG of it inside the 512 × 512 viewBox, so
`packaging/icons.sh` (which widens the viewBox for the macOS grid) keeps working.

## Geometry

A 512 × 512 viewBox. The mark fills 88% of the canvas on Windows and Linux. The macOS icons
(`.icns`, `gridcraft-macos-512.png`) sit on Apple's grid: an 824/1024 body with a transparent
margin.

## Files

| File | Use |
|---|---|
| `gridcraft.svg` | Canonical artwork; also `hicolor/scalable` |
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

Replace the PNG embedded in `gridcraft.svg`, then run `packaging/icons.sh`. It needs `resvg`, and
`iconutil` for the `.icns` (macOS only). It packs the `.ico` with `cargo xtask ico`.
