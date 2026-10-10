# UI parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** major (created from a source audit and the issue tracker) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Interaction, controls and feel, against Excel. Part of [target-app-parity.md](target-app-parity.md).
**UI/UX fidelity: ~55% ready, 50–80 h** (estimated). Ribbon command coverage is measured
separately in [parity-checklist.md](parity-checklist.md) (258 / 290).

Sources: `crates/ui-egui/src` (ribbon, grid, editor, formula bar, dialogs, panes),
`crates/engine/src/cmd` (shortcuts in command specs), the owner's black-box notes
(`plan/excel/observed-ui.md`, local only) and user issues.

## Window and chrome

| Element | Excel | GridCraft | Status |
|---|---|---|---|
| Title bar, Quick Access Toolbar, AutoSave toggle | yes | yes | done; macOS window controls misaligned (#170), Dock icon size (#169) |
| Ribbon: tabs, groups, split buttons, galleries, contextual tabs | yes | yes (Table Design, Chart Design, Format, PivotTable tabs) | done; missing contextual tabs for pictures, shapes, sparklines, header/footer, slicers |
| Ribbon collapse, simplified ribbon, customize ribbon/QAT | yes | partial | customization missing (File ▸ Options is the one missing File command) |
| Key tips (Alt / Ctrl+F6 then letters) | yes | **no** (#33, #49) | 4–6 h |
| Formula bar with Name Box, expand, function hints | yes | yes | done |
| Sheet tabs: rename, reorder, colour, hide, scroll | yes | yes | rename focus bug on macOS (#145) |
| Status bar: Sum/Count/Average and more, view buttons, zoom | yes | yes | customizable statistics partial |
| Task panes (Format Chart, Comments, Watch, Selection, PivotTable fields) | yes | yes | element-level Format pane for shapes and pictures missing |
| Backstage (File) view: Info, New with templates, Open recent, Export, Options | yes | partial | Options dialog and template gallery missing |
| Dark mode, system appearance | yes | yes | done (10-09) |
| Page Layout view (paper view with rulers and header editing) | yes | **no** (Page Break Preview only) | 8–12 h |
| Multiple windows per workbook | yes | no (#162) | see [hardware-parity.md](hardware-parity.md) |

## Grid interaction

| Behaviour | Status |
|---|---|
| Click/drag/Shift/Ctrl selection, multi-area selection | rectangular drag works in one direction only in some cases (#28) |
| Fill handle: drag to fill series, double-click to fill down, right-drag menu | fill done; right-drag menu missing |
| Drag-move and drag-copy cells (border drag, Ctrl/Option, Shift to insert) | done (M3) |
| In-cell editing, Enter/Tab movement, Alt/Option+Enter line breaks | done |
| Point mode (click/drag/arrow references), reference colouring, F4 | done |
| AutoComplete in columns, function autocomplete and argument tips | done |
| Header resize and double-click AutoFit | done (#42, #88 for header dividers) |
| Freeze panes, split panes | freeze done; split to verify |
| Context menu on cells | 16 items (Cut, Copy, Paste, Paste Special, Insert, Delete, Clear Contents, Filter by value, Sort, New Comment, New Note, Format Cells, Pick From Drop-down List, Define Name, Link). Missing: Insert Copied/Cut Cells (#21), paste-option icons, Quick Analysis, Get Data from Table, Show Changes, Smart Lookup |
| Context menus on row/column headers, sheet tabs, charts, shapes | headers and tabs done; objects partial |
| Quick Analysis (Ctrl+Q) | missing |
| Smooth scrolling, scrollbars | done; vertical scrollbar added 10-09 (#73); horizontal scrollbar reported missing (#176) |
| Text overflow into empty neighbours, wrap, rotation, shrink to fit | done |

## Keyboard

| Area | Excel | GridCraft |
|---|---|---|
| Shortcuts bound to commands | 200+ documented shortcuts per platform | 59 command shortcuts plus grid and editor keys (55 distinct keys handled in `ui-egui`) |
| Ctrl+arrow, Ctrl+Shift+arrow, Ctrl+Home/End, Page Up/Down, Alt+Page | yes | done |
| Ctrl+1, Ctrl+; and Ctrl+Shift+; , Ctrl+D/R, Ctrl+E, Ctrl+T, Ctrl+K, Ctrl+Shift+L | yes | most done; audit pending |
| F2, F4, F5, F9 family, Shift+F3, F11 | yes | most; F11 chart sheet missing (no chart sheets) |
| Mac-specific (Cmd variants, Fn keys) | yes | Cmd mapping done |
| Requests | — | #110 (more shortcuts and hot keys), #33 (sequential Alt) |

Work: a shortcut table generated from the command catalog checked against Microsoft's published
shortcut lists for Windows and Mac, then fill the gaps: 6–10 h.

## Dialogs

Format Cells, Insert Function/Formula Builder, Find and Replace, Go To and Go To Special, Sort,
Name Manager, Data Validation, Paste Special, Conditional Formatting manager, Page Setup, Goal
Seek, Scenario Manager and others exist. Missing or partial: Options, Solver, Data Analysis,
Forecast Sheet, Power Query editor, Insert Object, Equation editor, Protect/Allow Edit Ranges,
Workbook Links, Accessibility checker.

## Accessibility

AccessKit is enabled, but the grid, ribbon and dialogs are not exposed as an accessible tree with
cell addresses and values, so screen readers can't navigate a sheet; there is no high-contrast
theme or accessibility checker. Excel is strong here. 12–18 h.

## Feel

Pixel comparison against Excel screenshots hasn't been run since M1; fonts depend on the system
(no Calibri; Carlito would match metrics, #29, #4, #168 for weights); icons are GridCraft's own
by rule and will never match Office's, by design.

## Work (50–80 h)

Key tips 4–6 · shortcut audit 6–10 · context menus and Quick Analysis 5–8 · Page Layout view
8–12 · Options dialog and ribbon/QAT customization 6–10 · accessibility 12–18 · selection and
scrollbar bugs 2–4 · pixel/feel pass against observed Excel 7–12.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | major | Created: chrome, grid, keyboard, dialogs, accessibility; key tips, Page Layout view and accessibility tree flagged |
