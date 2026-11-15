# PhotoCraft Brush in GridCraft

This is an experimental idea for maintainer evaluation. Maintainers can decide to continue it, reshape it, use individual findings, or close the proposal. The current prototype implements four presets through PhotoCraft's CPU renderer.

Open **Draw**, then click the PhotoCraft Brush icon. Click the arrow below it to choose Hard round, Soft round, Chalk or Highlighter. The same panel has size, hardness, opacity, flow, smoothing and pressure controls. Expand **Color** to pick a custom color. Use the mouse or an input device that reports force to egui. Choose Select Objects to move or resize a stroke. Choose Eraser to remove it. To change a stroke's style, select it, set the desired options in the brush panel and choose **Apply to selected stroke**.

Each completed stroke is one undo entry. Escape, a cancelled contact or a change of target cancels the current stroke. The renderer lowers bitmap resolution as needed for long strokes while keeping their size and original pointer samples. If an unusually large gesture reaches the remaining safety limits, GridCraft saves the part drawn so far.

GridCraft saves a brush stroke as a transparent PNG drawing with its original samples and settings in a versioned private PNG chunk. GridCraft can restyle it after reopening an XLSX. Excel and other viewers display the drawing as a picture. An application that rewrites the PNG may discard the private chunk, leaving the appearance intact. PDF export and print use the rendered picture.

## Updating the brush renderer

`crates/brush` is the only GridCraft crate with PhotoCraft dependencies. Its manifest pins the CPU renderer and its dependencies to one PhotoCraft revision. The renderer module translates GridCraft's settings and samples into PhotoCraft types. The PNG, workbook, UI and automation interfaces use GridCraft's own data. Installing a different PhotoCraft desktop version cannot change this build.

To upgrade, change the pinned revisions together and refresh the lockfile. Run the brush tests, the engine brush round trip and the UI brush gesture tests. Also check the web build. Compare existing workbook appearance before and after reopening, and restyle an old brush stroke to check that the new renderer can consume version 1 data.

The gesture handler collects normalized force from egui touch events where the platform supplies it. The command interface accepts pressure and tilt samples for scripted drawing. Physical tablet support varies by platform and driver. Legacy Draw tools still use GridCraft's ordinary ink format.

Automation commands are `draw.brush`, `draw.brushStroke`, `draw.brushInspect` and `draw.brushEdit`. The commit command accepts worksheet coordinates, a pressure value from zero to one and optional tilt and time in milliseconds. It returns the drawing's image ID. Use existing image object commands for move and delete.

The [validation report](photocraft-brush-validation.md) records the scale and brush-control checks, CPU measurements, Excel behavior, and remaining limits.
