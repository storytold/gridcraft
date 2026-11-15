# Dark sheet, dark charts and dark pictures

> **Last updated:** 2026-11-05 · **Change:** new: three opt-in, screen-only settings that show the workbook light-on-dark · **Target:** settings Excel does not have

Display theme Dark darkens the interface, but the cells stay white (#1264, #1736). Three settings
show the workbook itself light-on-dark. WordCraft has the same feature for its pages
(storytold/wordcraft#194); the colour rule below is WordCraft's, so the two apps look alike side
by side.

## What the user gets

| Setting | Command | What it darkens |
| --- | --- | --- |
| Dark sheet | `view.darkSheet` | the cells, gridlines and everything drawn on the sheet, except charts and pictures |
| Dark charts | `view.darkCharts` | charts on the sheet and in the Recommended Charts preview |
| Dark pictures | `view.darkPictures` | floating and in-cell pictures |

- Each is a checkbox in the View tab (a column next to Display theme and Animations) and in
  **File › Options › General**. The tooltips say what they do and that saving, printing and PDFs
  are unchanged.
- **Opt-in.** All three are off by default, also when Display theme is Dark or System. With all
  three off the sheet is exactly what it was before they existed: `SheetColors::LIGHT` holds
  today's values, and a test pins them.
- **Separate from Display theme, and from each other.** Display theme colours the interface
  (ribbon, row and column headers, formula bar, sheet tabs, scroll bars, status bar, dialogs,
  slicers) and never the cells. `view.theme`, `view.darkMode`, `ui.set {dark}` and System leave
  the three settings alone, and the settings leave the interface alone. Every combination works: a
  light interface with a dark sheet, a dark sheet with charts as authored, dark charts on a light
  sheet.
- **Preferences.** Kept in `UiState` with the other preferences (`ui.json` on the desktop,
  `localStorage` on the web); settings saved before they existed read as off. Turning one on or
  off is not an edit: no dirty flag, no undo step, nothing written to the file.
- **Screen only.** Saving, printing, Print Preview, PDF and the other exports are produced from the
  workbook by the engine, which never sees a display colour, so they keep the authored colours.

Charts and pictures have settings of their own because their colours carry meaning (a brand
palette, red for a loss) and an inverted photo rarely looks right: someone who wants dark cells may
still want either as authored.

## Colour rule

One rule for every colour the workbook supplies, as in WordCraft: **invert the HSL lightness,
keep hue and saturation, then lift the blacks to the sheet grey.**

```text
invert(r,g,b): d = 255 - max(r,g,b) - min(r,g,b); each channel v → clamp(v + d, 0, 255)
lift(v)      : P + v * (255 - P) / 255           with P = 0x33 (WordCraft's DARK_PAPER)
ink(c)       = lift(invert(c))
```

So white becomes `#333333` (the sheet), black becomes white, `#E1E1E1` gridlines become
`#4B4B4B`, dark blue headings (`#1F4E79`) become light blue (`#9EC3E6`), pale fills
(`#DDEBF7`) become deep fills (`#39444E`). Pure hues (`#FF0000`, `#0000FF`, `#FFFF00`) keep
their lightness and so barely change.

**Contrast guard.** Inversion keeps most pairs legible, but not pure hues: blue text
(`#0000FF`, 8.6:1 on white) becomes `#3333FF` on `#333333`, 1.8:1; automatic text on a
yellow fill becomes white on yellow, 1.1:1. So text and border colours are checked after mapping
against the colour actually behind them (the mapped fill, or the sheet):

- target = min(4.5, contrast of the authored pair) for text, min(3, …) for borders and lines,
  using `theme::contrast` (WCAG 2);
- below the target, the ink's HSL lightness moves away from the background (lighter on a dark
  background, darker on a light one), hue kept, in small steps until the target is met or the
  lightness reaches 0 or 1.

The guard never makes a pair less legible than the author made it (white text on a white fill
stays hidden), and never touches fills. Results are cached per (colour, background) pair, so a
large sheet pays for each pair once.

## What follows the dark sheet

| Workbook colour | On the dark sheet |
| --- | --- |
| Sheet background (no fill) | `#333333` = ink(white) |
| Gridlines | `#4B4B4B` = ink(`#E1E1E1`): 1.45:1 on the sheet, as subtle as `#E1E1E1` on white (1.31:1) |
| Automatic font colour | ink(black) = white, then the guard against the cell's fill |
| Font colours: RGB, indexed, theme with tint, Text 1 / Background 1, hyperlinks | ink(), then the guard (`#0563C1` becomes `#64AFFB`, 5.5:1) |
| Number-format colours (`[Red]`, `[Color n]`) | ink(), then the guard |
| Fills, solid and pattern (both colours) | ink(), no guard |
| Borders and diagonal borders | ink() (automatic black becomes white), then the guard at 3:1 against the cell's fill |
| Conditional formats: fonts, fills, colour scales, data bars | ink(); fonts then the guard |
| Table styles and cell styles | resolved to colours first, then the same rules |
| In-cell editor | the cell's mapped fill (or the sheet), with its automatic text guarded on it |
| Page Layout view | the paper is the sheet, so it follows |

Left as they are: charts and pictures (their own settings), icon-set glyphs, sheet tab colours
(interface), note and comment popups (they keep their own paper, black on pale yellow or white),
and explicit colours inside equations (an equation's automatic ink follows the sheet).

Not done yet: sparklines, shapes, text boxes and Draw ink keep their authored colours on the dark
sheet (a text box with automatic black text and no fill is hard to read there), and Page Layout
view draws header and footer text black on the dark paper.

### Colours GridCraft adds to the sheet

These are not the workbook's colours: they get a second set of fixed values, each held to WCAG 2
against `#333333` (4.5:1 for text, 3:1 for glyphs and lines) by `theme::contrast_tests`.

| Element | Light sheet (unchanged) | Dark sheet |
| --- | --- | --- |
| Selection outline, fill handle, active cell | `#107C41` | `#5CC587` (5.9:1); the fill handle's ring is the sheet grey |
| Selection tint | `#107C41` at 0x1C (0x24 with a dark interface) | `#5CC587` at 0x24 |
| Active-row tint | `#107C41` at 0x2C (`#6FD39A` at 0x30 with a dark interface) | `#6FD39A` at 0x30 |
| Formula reference outlines, in-cell editor references | the light reference colours | the dark interface's reference colours (4.8 to 7.0:1) |
| Filter, validation and paste-options buttons, object handles | bg `#F3F3F3`, border `#8A8A8A`, text `#1F1F1F` | bg `#3C3C3C`, border `#9A9A9A`, text `#EEEEEE` |
| Form controls: hover, pressed, slider track, thumb, filled part, focus ring | `#E4E4E4`, `#CFCFCF`, `#D6D6D6`, `#6B6B6B`, `#107C41`, `#107C41` | `#4A4A4A`, `#575757`, `#4A4A4A`, `#A9A9A9`, `#5CC587`, `#5CC587` |
| Picture placeholder | `#D6D6D6` | `#4A4A4A` |
| Trace arrows (precedents, errors) | `#1F5FC9`, `#D13438` | `#6FA3F5`, `#F1707A` |
| Macro overlays (input, footprint, conflict, target) | `#2E6FD8`, `#23803D`, `#C42B1C`, `#6B3FC0` | `#6FA3F5`, `#6CCB85`, `#F1705F`, `#B394EE` |
| PY tag and its text | `#1E6E4E`, white | `#5CBF94`, `#0B1F17` |
| Error-check triangle | `#1E8E3E` (`#4CC26B` with a dark interface) | `#4CC26B` |
| Frozen-pane lines | `#969696` | `#8A8A8A` (3.7:1) |
| Fill-drag preview, move outline | `#5A5A5A`, `#6E6E6E` | `#C8C8C8` (5.8:1) |
| Page Break Preview: outside the print area, break lines, "Page N" | black at 70, `#2E6FD8`, black at 45 | black at 120, `#6FA3F5`, white at 70 |

Row and column headers, the select-all corner and their selection highlight are interface: they
follow Display theme, not the dark sheet.

### Views and previews

- **Normal view** and **Page Layout view** are editing views: the dark sheet applies, including
  the Page Layout paper (like WordCraft's page). The desk around the pages follows Display theme.
- **Page Break Preview**: the dark sheet applies, with the dark values above.
- **Print Preview** (File › Print) shows what will print: always the light sheet.
- **Previews** that show a cell (the border preview of Format Cells, the table style and cell style
  galleries) show it as it will look on the current sheet.

## Commands

`view.darkSheet`, `view.darkCharts` and `view.darkPictures` take `{on?: bool}` (without `on` they
toggle; WordCraft's `value` is accepted as an alias) and return `{on}`. They never open a dialog,
are listed by `engine.commands`, and reach the control channel like every UI command.
`view.darkMode` stays the older interface switch (the title-bar Dark Mode button) and leaves the
cells alone, so existing control-channel and CLI clients see no change. The offscreen `snapshot`
example takes `--dark-sheet`, `--dark-charts` and `--dark-pictures`.

## How it is built

- **Tokens on two axes.** `theme::Tokens` keeps its field names. The sheet fields (`grid_bg`,
  `gridline`, `cell_text`, `sel_*`, `active_row`, `error_check_green` and every `sheet_*`) come
  from a `SheetColors` set chosen by the dark-sheet flag rather than by Display theme:
  `Tokens::palette(dark_ui, dark_sheet)`. The update loop mirrors the three preferences into the
  egui context every frame and `Tokens::get` reads the dark-sheet flag, so every painter that
  already calls `Tokens::get` follows without new parameters. `Tokens::on_sheet` and
  `Tokens::on_sheet_over` map a colour for the current sheet (unchanged on the light sheet).
- **The mapping lives in the UI crate**, in `theme` (`ink`, `ink_guarded`), next to `contrast`. It
  reuses `model::style::rgb_to_hsl` and `hsl_to_rgb` (now public) instead of copying them. Nothing
  below the UI crate changes: the model, the file writers, printing and export never see a mapped
  colour. Cells resolve their colours once in `grid/cell_paint.rs` (`look`), which keeps the
  authored fill for the guard and returns the painted one.
- **Charts**: `chartview::paint_prims_dark` maps every chart primitive's colour with `ink` and text
  with `ink_guarded` against white; the sheet's charts and the Recommended Charts preview call it
  with the dark-charts flag.
- **Pictures**: the floating-picture cache keys a dark texture apart from the light one, and the
  in-cell picture cache decodes a picture again when the flag changes; `theme::ink_image` maps
  every pixel with `ink`, alpha kept.
- **Print Preview** paints with `Tokens::get(ctx).with_dark_sheet(false)`.

## Tests

- `theme::contrast_tests`: every foreground/background pair in all four combinations of Display
  theme and sheet; the light sheet keeps today's values exactly; Display theme alone never changes
  the dark sheet and the dark sheet never changes the interface; the mapping's examples, the
  guard, and a sweep of colour pairs that the guard keeps at least as legible as authored or at the
  minimum.
- `grid::tests`: cell colours are authored on the light sheet and mapped on the dark one.
- `tests/it/dark_sheet_setting.rs`, `dark_sheet_theme.rs` and `dark_charts_setting.rs`: off by
  default, the commands set and toggle, independence from Display theme and from each other, the
  preference round trip with older settings read as off, no dirty flag, and pixels of the rendered
  sheet and charts.
- `cell_pictures` unit test: the in-cell picture cache decodes a picture again for the other setting.

## Earlier proposals

- **#1736** (a `view.darkCells` patch against v0.7.0): black canvas, `#3F3F3F` gridlines,
  automatic ink flips, explicit colours and fills kept as authored. Its separate, saved setting is
  kept here. Not kept: explicit colours as authored leave explicit black text black on the dark
  canvas and white fills glaring, and Excel files often store Text 1 / Background 1 explicitly.
- **#177** darkens the canvas as part of the dark interface theme. Changing the default look for
  everyone is a product decision the maintainers hold (#177), hence opt-in and separate from
  Display theme here.
- **#2614** gives default cells a black background whenever the interface is dark; **#3082** adds a
  worksheet switch that follows the interface by default. Both keep explicit colours as authored.
- **WordCraft #194** is the sibling precedent: the same inversion and lift, screen only, remembered
  between runs. Its paper grey (`#333333`) is used unchanged.

## Revision history

| Date | Change | Notes |
| --- | --- | --- |
| 2026-11-05 | new | Dark sheet, dark charts and dark pictures: three opt-in, screen-only settings with WordCraft's colour rule and a contrast guard |
