# UI parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-25 · **Change:** minor (AutoSum over a selected row puts its total to the right) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Interaction, controls and feel, against Excel. Part of [target-app-parity.md](target-app-parity.md).
**UI/UX fidelity: ~55% ready, 50–80 h** (estimated). Ribbon command coverage is measured
separately in [parity-checklist.md](parity-checklist.md) (274 / 290).

Sources: `crates/ui-egui/src` (ribbon, grid, editor, formula bar, dialogs, panes),
`crates/engine/src/cmd` (shortcuts in command specs), the owner's black-box notes
(`plan/excel/observed-ui.md`, local only) and user issues.

## Window and chrome

| Element | Excel | GridCraft | Status |
|---|---|---|---|
| 2026-10-14 | minor | Dialogs (Find and Replace, Go To, Format Cells, Sort and the rest) move by their title bar instead of staying pinned to the centre (#1250) |
| Title bar, Quick Access Toolbar, AutoSave toggle | yes | yes | done; macOS window controls misaligned (#170), Dock icon size (#169) |
| Ribbon: tabs, groups, split buttons, galleries, contextual tabs | yes | yes (Table Design, Chart Design, Format, PivotTable, Header & Footer tabs); Fill Color, Font Color and the Format Cells colour pickers end with Recent Colors and More Colors… (#928); large split buttons (Formulas › AutoSum); arrows at the card's edges scroll a tab wider than the window | done; missing contextual tabs for pictures, shapes, sparklines, slicers; groups do not collapse when narrow |
| Ribbon collapse, simplified ribbon, customize ribbon/QAT | yes | partial | customization missing (File ▸ Options is the one missing File command) |
| Ribbon: tabs, groups, split buttons, galleries, contextual tabs | yes | yes (Table Design, Chart Design, Format, PivotTable tabs) | done; missing contextual tabs for pictures, shapes, sparklines, header/footer, slicers |
| Ribbon collapse, simplified ribbon, customize ribbon/QAT | yes | partial | customization missing |
| Key tips (Alt / F10 then letters) | yes, every ribbon control | **initial** (#43, 10-10): Alt or F10 shows key tips on Home for alignment, Paste Special and AutoFit plus legacy Alt sequences; other tabs and controls not yet (#33, #49) | 3–5 h |
| Formula bar with Name Box, expand, function hints | yes | yes; grows with its content from one to five lines (Alt+Enter, wrapped text, or viewing a multi-line cell), then scrolls with the caret in view, the Name Box and buttons staying on the first line; the chevron pins it at five; the in-cell editor shows every line | done; no drag-to-resize handle |
| Sheet tabs: rename, reorder, colour, hide, scroll | yes | yes | Ctrl-drag (Cmd or Option on macOS) drops a copy (#971); active tab revealed on navigation and resize (#681); manual wheel/trackpad browsing preserved; rename focus bug on macOS (#145) |
| Formula bar with Name Box, expand, function hints | yes | yes | done; dynamic-array spill children show the source formula muted and read-only, using current spill ownership |
| Formulas › Error Checking arrow: Circular References | yes | yes: the button opens Error Checking and its arrow opens a menu with Error Checking… and Circular References, which lists a cell on each circular reference as `Sheet!A1`, up to 100, and selects the one picked, on its own sheet (unavailable when there are none) | done; the Error Checking… item and the limit of 100 are GridCraft's own |
| Sheet tabs: rename, reorder, colour, hide, scroll | yes | yes | active tab revealed on navigation and resize (#681); manual wheel/trackpad browsing preserved; rename focus bug on macOS (#145) |
| Sheet tabs: rename, reorder, colour, hide, scroll | yes | yes | active tab revealed on navigation and resize (#681); New sheet (+) right after the last tab, the strip capped at 60% of the bar (#943); manual wheel/trackpad browsing preserved; next/previous skips Hidden and VeryHidden sheets and retains the current sheet and selection when no visible destination exists; rename focus bug on macOS (#145) |
| Status bar: Sum/Count/Average and more, view buttons, zoom | yes | yes; a right-click on the status bar opens a menu that turns the statistics on and off (Average, Count, Numerical Count, Minimum, Maximum, Sum; `view.statusBar`) | done for the statistics; the other status bar options are not customizable |
| Task panes (Format Chart, Comments, Watch, Selection, PivotTable fields) | yes | yes; PivotTable source-field list scrollbar sits at the right edge of its pane | element-level Format pane for shapes and pictures missing |
| Backstage (File) view: Info, New with templates, Open recent, Export, Options | yes | partial | template gallery missing |
| Dark mode, system appearance | yes | yes | done (10-09); follows the system on first run, title-bar switch and File › Display theme (#194, #239); icons, reference colours, primary buttons and the comments pane follow the theme, and a unit test holds every token pair to WCAG 2 contrast in both themes |
| Backstage (File) view: Info, New with templates, Open recent, Export, Options | yes | yes (#174, 10-11): the File tab opens a full-window view (Esc or the back arrow return) with Home, New (blank + the three samples), Open (recent + Browse; the recent list tells when each workbook was last changed, 10-12), Info (location, properties, Protect Workbook), Save, Save As, Print (page preview, active sheet or workbook, fit to page, Page Setup), Export (PDF, CSV, web page), Close and Options (`file.options`: theme, language, formula bar, AutoSave, calculation mode and iterative calculation with Maximum Iterations and Maximum Change, 10-15) | no Account, Share or template gallery beyond the samples; Options applies at once (no OK/Cancel dialog) and holds only the settings GridCraft has |
| Dark mode, system appearance | yes | yes | done (10-09) |
| Page Layout view (paper view with rulers and header editing) | yes | yes (#329, 10-11): per-sheet view saved in XLSX, pages with margins and gaps on the print pagination, "Click to add data" pages, rulers (inches or centimetres), headers and footers drawn and edited in place with a Header & Footer tab, whose Different First Page and Different Odd & Even Pages give the first page and the even pages their own header and footer (#344), and whose Scale with Document and Align with Page Margins set whether header and footer text follows the print scale and the side margins, on paper and in the view | margins drag on the rulers (#792, with header and footer margins); header/footer pictures missing (1–3 h). Unverified: with Align with Page Margins off, header and footer text sits 0.7 in from the paper edge, a value not yet measured in the target app |
| Multiple windows per workbook | yes | yes (10-22, desktop): each workbook window is a native window of its own (New, Open and New Window open one; titles "Book1", or "Book1:1" / "Book1:2" for a workbook's second window, with " — Edited" while unsaved). Each window keeps its own cell draft, dialog, Backstage view, message and grid state, and its ribbon acts on it; on macOS the menu bar acts on the window focused last. Leaving a window enters its draft as Enter would. A window's close button closes that window: one of several on a workbook closes without asking, a workbook's last asks Save / Don't Save / Cancel when unsaved. Quit asks about each unsaved workbook in its window, and Cancel stops it. The last window closed quits on Windows and Linux; macOS keeps running with the menu bar until New or Open. View › Arrange All (dialog: Tiled, Horizontal, Vertical, Cascade, Windows of active workbook; `view.arrangeAll` tiles by default) lays the visible windows out over the main window's monitor, the active one first. Web: one window | Arrange All uses one monitor; a hidden window (View › Hide) has no native window; every window repaints when one does; see [hardware-parity.md](hardware-parity.md) |
| Switch Windows, New Window, unsaved workbooks on quit | yes | yes (10-22): View › New Window opens another window on the same workbook (no copy: saving from either saves it), each with its own selection, active sheet and scroll (showing different sheets copies nothing and leaves the workbook unchanged; Save records the sheet of the window used last); a workbook with several windows titles them "Book1:1", "Book1:2"; File › Close closes one of them without asking and asks only for the last; View › Switch Windows and the macOS Window menu list the open windows (active one checked, unsaved ones marked •) and bring the chosen one forward (`view.switchWindows {window}`, or `{doc}` for a workbook's most recent window); a cell being typed is entered into its own window and workbook before New, Open, Close, Save, Print and window switches (also between two windows on one workbook), and a refused entry stays open and stops the command; quitting (Cmd+Q, the macOS Quit item, the Dock) asks Save / Don't Save / Cancel for each unsaved workbook | |

## Grid interaction

AutoFit column width includes dropdown space only for filter header cells (tables and ordinary
AutoFilters), then takes the maximum against body cells. Windows Excel 16.0 build 20326,
observed using synthetic workbooks via its public automation API on 2026-10-11, reserves
12.5 points at 100-200% zoom and 25 points at 50%, independently of row height. GridCraft
uses that allowance in its grid units; its existing approximate font measurement remains.
Header text aligns and clips before the button. Command and header-double-click paths share
this logic, with one undo step.

| Behaviour | Status |
|---|---|
| Click/drag/Shift/Ctrl selection, multi-area selection | rectangular drag works in one direction only in some cases (#28) |
| Cursor while dragging cells, headers, freeze handles and objects | stable for the active drag (#936); Draw tools keep their crosshair; header arrows and a custom cell cursor remain unimplemented |
| Selection outline glides to a new selection (Excel: "Provide feedback with animation") | done (10-11): 0.12 s, decelerating, with the fill and fill handle; View ▸ Animations (`view.animations`) turns it off. The timing is an estimate, not yet measured against Excel; the system's reduced-motion setting isn't followed yet |
| Fill handle: drag to fill series, double-click to fill down, right-drag menu | fill done (Ctrl+drag swaps copy and series; month patterns use the workbook's 1900 or 1904 date system); right-drag menu partial: Copy Cells, Fill Series, Fill Formatting Only, Fill Without Formatting, and from a date Fill Days, Fill Weekdays, Fill Months and Fill Years, then Series…; a left-drag leaves an Auto Fill Options button with the same fill choices (gone once the workbook changes or the selection moves); Linear/Growth Trend and Flash Fill missing |
| Drag-move and drag-copy cells (border drag, Ctrl/Option, Shift to insert) | done (M3) |
| In-cell editing, Enter/Tab movement, Alt/Option+Enter line breaks | done |
| Point mode (click/drag/arrow references), reference colouring, F4 | done; Command/Ctrl-click adds separate arguments with the edit's locale separator, and Shift-click extends the current reference from a fixed anchor on the same sheet. Pointer-event regressions cover repeated/reversed selections, manual separators, sheet switches and keyboard continuation. |
| Number-format colors (`[Green]`, `[Color1]`–`[Color56]`) | Rendered in grid and PDF from shared RGB mappings (#1452) |
| Point mode (click/drag/arrow references), reference colouring, F4 | done |
| AutoComplete in columns, function autocomplete and argument tips | done; cell helpers clip to the worksheet and hide when the editor scrolls out (#200) |
| Header resize and double-click AutoFit | done (#42, #88 for header dividers) |
| Data › Advanced Filter | dialog offers in-place or copy-to filtering, list and optional criteria ranges, and unique records; Alt+A,Q opens it (#1622). The engine's copy-to path clears only the result footprint and rejects worksheet overflow (#1031). |
| Outline symbols for grouped rows / columns (band beside the headers, brackets, − / +, level buttons 1…n+1) | done (#373): clicks run `data.hideDetail` / `data.showDetail` / `data.showLevels`, frozen panes and right-to-left sheets included; Data › Outline › Settings (`data.outlineSettings`) puts summary rows above and summary columns left of their detail, saved as `outlinePr` (the dialog's Automatic styles check box and Apply Styles button are missing), View › Show Outline Symbols (Ctrl+8) hides the bands and is saved with the sheet, and a block of cells still groups rows without the Rows / Columns dialog |
| Reorder selected whole columns by dragging their headers | done (10-11) |
| Freeze panes, split panes | freeze done; split to verify |
| A Table's header names and filter buttons in the column headings once its header row scrolls away | done (#1423): with the active cell in the Table, as in Excel; the button opens the column's filter menu, the rest of the heading still selects the column. Plain AutoFilter ranges keep their letters (Excel does the same) |
| Copy as HTML table for other apps | done (#36, 10-10) |
| Context menu on cells | 16 items (Cut, Copy, Paste, Paste Special, Insert, Delete, Clear Contents, Filter by value, Sort, New Comment, New Note, Format Cells, Pick From Drop-down List, Define Name, Link). Insert Copied Cells (Insert Cut Cells for whole rows/columns) while the clipboard holds a copy (#21). Show Details on a PivotTable's value, subtotal or grand total cell (`pivot.showDetails`). Missing: Insert Cut Cells for a cell block, paste-option icons, Quick Analysis, Get Data from Table, Show Changes, Smart Lookup |
| Move and resize charts, pictures and shapes | charts can't be resized (#190) |
| Context menus on row/column headers, sheet tabs, charts, shapes | headers and tabs done; objects partial |
| Quick Analysis (Ctrl+Q) | missing |
| Smooth scrolling, scrollbars | done; vertical scrollbar added 10-09 (#73); horizontal scrollbar in the tab bar (60/40 split, drag, track click pages) 10-13 (#176, #840); dragging the handle at the scrollbar's left edge gives the tabs more or less of the bar; each workbook keeps its share while it is open (not saved in the file) |
| Text overflow into empty neighbours, wrap, rotation, shrink to fit | done |

## Keyboard

| Area | Excel | GridCraft |
|---|---|---|
| Shortcuts bound to commands | 200+ documented shortcuts per platform | 59 command shortcuts plus grid and editor keys (55 distinct keys handled in `ui-egui`) |
| Ctrl+arrow, Ctrl+Shift+arrow, Ctrl+Home/End, Page Up/Down, Alt+Page | yes | done |
| Ctrl+1, Ctrl+; and Ctrl+Shift+; , Ctrl+D/R, Ctrl+E, Ctrl+T, Ctrl+K, Ctrl+Shift+L | yes | most done; audit pending |
| F2, F4, F5, F9 family, Shift+F3, F11 | yes | most; F11 chart sheet missing (no chart sheets) |
| Mac-specific (Cmd variants, Fn keys) | yes | Cmd mapping done |
| Delete selected charts, pictures and shapes | yes | done (#213) |
| Shortcuts bound to commands | 200+ documented shortcuts per platform | audited below (#110): shortcuts whose feature exists are bound, with the exceptions listed; the rest wait for their features |
| Ctrl+arrow, Ctrl+Shift+arrow, Ctrl+Home/End, Ctrl+Shift+Home/End, Page Up/Down, Alt+Page | yes | done |
| Ctrl+1, Ctrl+; and Ctrl+Shift+; , Ctrl+D/R, Ctrl+E, Ctrl+T/L, Ctrl+K, Ctrl+Shift+L, Ctrl+Shift+~!@#$%^&_ | yes | done |
| F2, F3, F4, F5, F7, F9 family, Shift+F2/F3/F10/F11, F12 | yes | done; F11 chart sheet, F8 extend mode, F4 Repeat missing |
| Mac-specific (Cmd variants, Control variants, Fn keys) | yes | done where a feature exists (`keymap.rs` picks the Mac variant) |
| Sequential Alt access keys (Alt, H, N, …) on every ribbon tab | yes | 364-sequence keytip table for all tabs: per-control badges, prefix waiting, dropdowns open for editing (type/arrows/Enter), pending bindings for not-yet-built commands; letters also work with left Alt held (Alt+H, O, A), as in Excel; each Borders letter (H, B, …) draws its own preset, H, B, M opens Format Cells on the Border tab, and Erase Border, Line Color and Line Style are reserved; QAT/Backstage excluded |
| Requests | — | #110 (more shortcuts and hot keys), #33 (sequential Alt; initial support landed in #43) |

### Shortcut audit (#110)

Checked against Microsoft's published "Keyboard shortcuts in Excel" (Windows and Mac tables,
2026-10-11). Alt key-tip sequences are tracked separately (#33). Bindings live in
`crates/ui-egui/src/grid.rs` (navigation, editing keys), `ribbon::shortcut` (Ctrl/Cmd letters),
`keymap.rs` (everything added in #110, with a `lookup` table that tests cover) and
`formula_bar.rs` (keys while editing). "Ctrl" means Cmd on a Mac unless the Mac column says
otherwise. Number-format and border keys match the physical key (Ctrl+Shift+4, wherever `$` is).

| Shortcut (Windows) | Mac | Action | Status |
|---|---|---|---|
| Ctrl+C / X / V, Ctrl+Z / Y, Ctrl+A, Ctrl+B / I / U | same with Cmd | clipboard, undo/redo, select all, bold/italic/underline | present |
| Ctrl+S, Ctrl+Shift+S, Ctrl+O, Ctrl+N, Ctrl+W | Cmd | save, save as, open, new, close | present |
| Ctrl+F, Ctrl+H, Ctrl+G, Ctrl+K, Ctrl+T | Cmd (Ctrl+H) | find, replace, go to, hyperlink, table | present |
| Ctrl+D, Ctrl+R, Ctrl+E, Ctrl+1, Ctrl+9 / 0, Ctrl+Shift+9 / 0 | Cmd (Ctrl+E on Mac: see below) | fill down/right, Flash Fill, Format Cells, hide/unhide | present |
| Ctrl+- , Ctrl+Shift+= | Cmd | Delete / Insert cells dialog | present |
| Ctrl+; , Ctrl+` | Control+; | current date, show formulas | present (Mac Control+; added) |
| Arrows, Shift / Ctrl / Ctrl+Shift+arrows, Home, Ctrl+Home / End, Page Up / Down, Ctrl+Page Up / Down | Cmd+arrows | navigation and extension | present |
| Tab, Shift+Tab, Enter, Shift+Enter, Ctrl+Space, Shift+Space, Delete, Esc | same | entry and selection | present |
| F2, F9, F10 (key tips), F4 / Alt+Enter / Ctrl+Enter / Ctrl+Shift+Enter while editing | same | edit, calculate, references, new line, fill, array | present |
| Ctrl+Shift+~ ! @ # $ % ^ | Control+Shift | General, Number, Time, Date, Currency, Percent, Scientific | **added** |
| Ctrl+Shift+& / Ctrl+Shift+_ | Cmd+Option+0 / Cmd+Option+- (and Cmd+Option+arrows for one side) | outline border / remove borders | **added** |
| Ctrl+2 / 3 / 4 / 5 | — | bold / italic / underline / strikethrough | **added** |
| Ctrl+6 | Control+6 | hide / show objects (charts, pictures, shapes, slicers), saved as `workbookPr showObjects` | **added** |
| Ctrl+Shift+: | Cmd+; | current time | **added** |
| Ctrl+' / Ctrl+Shift+" | Control | formula / value from the cell above | **added** |
| Ctrl+L | Cmd+L is Align Left, Cmd+E Center, Control+E Flash Fill | create table | **added** |
| Ctrl+Shift+L | Cmd+Shift+F (present) / Control+Shift+L | filter | **added** |
| Ctrl+Shift+F, Ctrl+Shift+G | — | Format Cells Font tab, Workbook Statistics | **added** |
| — | Cmd+Shift+H, Cmd+Shift+R, Cmd+Shift+K / J, Control+L, Control+U | replace, sort, group / ungroup, Define Name, edit cell | **added** |
| Alt+Shift+Right / Left | Option+Right / Left is next/previous sheet | group / ungroup | **added** |
| Ctrl+Shift+* (Ctrl+Shift+8), Ctrl+Shift+Space | Cmd+Shift+Space | current region, select all | **added** |
| Ctrl+Shift+Home / End, Shift+Home | same | extend to A1 / last used cell / row start | **added** |
| Ctrl+P | Cmd+P | print | **added** |
| Ctrl+Shift+> / < | Cmd+Shift+> / < | font size | **added** |
| Ctrl+Alt+V | Cmd+Control+V, Cmd+Option+V | Paste Special | **added** |
| Ctrl+Alt+= / Ctrl+Alt+- | — | zoom in / out (10% steps) | **added** |
| Alt+=, Alt+Down, Alt+Page Down / Up | Option+Down, Option+Page Down / Up | AutoSum, drop-down list, one screen right / left | **added** |
| Alt+Down on a filter header | Option+Down | Open the existing AutoFilter popup; validation dropdowns keep priority | **added** (AutoFilter portion of #441; Ribbon dropdowns and column value picker are separate work) |
| Alt+F1, Alt+Shift+F1, Shift+F11 | Shift+F11 | embedded chart, new sheet | **added** |
| Shift+F2, Ctrl+Shift+F2 | Shift+F2, Cmd+Shift+F2 | note, threaded comment | **added** |
| Shift+F3, Ctrl+F3, Ctrl+Shift+F3 | Shift+F3, Cmd+F3 (Define Name), Cmd+Shift+F3 | Insert Function, Name Manager, Create from Selection | **added** |
| F5, Shift+F5 (Mac), F7, Shift+F9, Ctrl+Alt+F9, F12, Ctrl+F4 | same | Go To, Find, Spelling, calculate sheet / all, Save As, close | **added** |
| Shift+F10, Alt+Shift+F10, Ctrl+F1 | Shift+F10, Cmd+Option+R | context menu, Error Checking, ribbon | **added** |
| F3 | not checked | Paste Name dialog, only when the workbook has names: OK puts the name at the caret of the formula being edited (or starts `=Name`), Paste List writes each name and what it refers to from the active cell (`formulas.pasteName`) | **added** |
| F1, F4 (repeat), F6 / Shift+F6, F8 / Shift+F8, F11, End mode | same | help, Repeat, pane cycling, extend/add mode, chart sheet | missing (no such feature yet) |
| Ctrl+Q, Ctrl+A after a function name, Ctrl+Shift+U, Ctrl+Alt+P | same | Quick Analysis, Function Arguments, formula bar height, tooltips | missing |
| F1, F3, F4 (repeat), F6 / Shift+F6, F8 / Shift+F8, F11, End mode | same | help, Paste Name, Repeat, pane cycling, extend/add mode, chart sheet | missing (no such feature yet) |
| Ctrl+Q, Ctrl+A after a function name, Ctrl+Alt+P | same | Quick Analysis, Function Arguments, tooltips | missing |
| Ctrl+Shift+A after a function name | Control+Shift+A | argument names: `=SUMIF(` becomes `=SUMIF(range,criteria,sum_range)`, the `(` added when missing; one Undo takes them back | **added** |
| Ctrl+Shift+U | Control+Shift+U | expand or collapse the formula bar (`view.expandFormulaBar`), on the grid and while editing; on Windows and Linux it no longer underlines | **added** |
| Ctrl+Shift+Page Up / Down, Ctrl+Alt+5, Ctrl+Shift+F1, Ctrl+F5–F10, Alt+F8 / F11 / F12 | same | group sheets, shape cycling, full screen, window keys, macros, VBA, Power Query | missing |
| — | Cmd+= (calculate), Cmd+T (references while editing; F4 works), Shift+F7, Cmd+Shift+L | Mac-only extras | missing (Cmd+= is egui's zoom key; no thesaurus or Modify Cell Style) |

On Windows, AltGr arrives as Ctrl+Alt: a Ctrl+Alt key that also types a character is treated as
text, so AltGr symbols (e.g. `~` on a German layout) and user-defined Ctrl+Alt+letter hotkeys that type
a character (e.g. Ctrl+Alt+A for `ä`) never trigger a shortcut, Ctrl+letter ones like Select All included.

## Dialogs

Format Cells, Insert Function/Formula Builder, Find and Replace, Go To and Go To Special, Sort,
Name Manager, Data Validation, Paste Special, Conditional Formatting manager, Page Setup, Goal
Seek, Scenario Manager, Data Analysis, Forecast Sheet and others exist. Missing or partial: Options,
Solver, Power Query editor, Insert Object, Equation editor, Protect/Allow Edit Ranges,
Workbook Links, Accessibility checker. Like Excel's, every dialog opens centred and moves by its
title bar, keeping its place while it is open (#1250).
Go To lists the named ranges of every sheet (a name scoped to another sheet as `Sheet2!Name`) and
the references entered before; a double-click goes to one. Its Reference box takes several names
or references separated by commas (`Price, Type`, `B14:C22,F19:G30`) and selects them all.
New Formatting Rule's formula box points like the formula bar: a click on the grid writes the
cell's absolute reference at the caret (`=$B$2`) and a drag a range, leaving the selection the rule
applies to as it is (#2348). Other dialogs' reference fields don't take clicks yet.

## Accessibility

AccessKit is enabled, but the grid, ribbon and dialogs are not exposed as an accessible tree with
cell addresses and values, so screen readers can't navigate a sheet; there is no high-contrast
theme or accessibility checker. Excel is strong here. 12–18 h. The light and dark themes' colour
pairs (text 4.5:1, glyphs 3:1) are checked by `theme::contrast_tests`.

## Feel

Pixel comparison against Excel screenshots hasn't been run since M1; fonts depend on the system
(no Calibri; Carlito would match metrics, #29, #4, #168 for weights); icons are GridCraft's own
by rule and will never match Office's, by design.

## Work (50–80 h)

Key tips 3–5 · remaining shortcuts with their features 2–4 · context menus and Quick Analysis 5–8 · Page Layout view
8–12 · Options dialog and ribbon/QAT customization 6–10 · accessibility 12–18 · selection and
scrollbar bugs 2–4 · pixel/feel pass against observed Excel 7–12.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-25 | minor | AutoSum (Alt+=, Home › AutoSum) over a selected row puts its total to the right of the row, or in its last cell when that is empty, as in Excel; it wrote a one-cell total under each cell. A selection's empty last column takes each row's total and its empty last row each column's, with the block's total in the corner when both are empty |
| 2026-10-25 | minor | Paste Name lays out only the names in view, so a workbook with thousands of names opens it at once; the arrow keys still scroll the chosen name into view. Go To matches a `Sheet2!Name` reference and hides a workbook name behind the active sheet's own name in any case and script (`Größe` = `GRÖSSE`), as formulas do. The two Page Layout view rows are one; the 0.7 in side margin used when Align with Page Margins is off is marked unverified |
| 2026-10-24 | minor | Go To lists the named ranges on every sheet, as Excel's does ("The Go to popup window shows named ranges on every worksheet in your workbook"): a name scoped to another sheet appears as `Sheet2!Name`, and hidden names, constants and formulas are left out. It keeps the references entered (newest first, ten at most), and a double-click on an entry goes there. The Reference box and `edit.goTo` take several names or references separated by commas (`Price, Type`, `B14:C22,F19:G30,H21:H29`) and select them all; `edit.goTo {list: true}` returns the list |
| 2026-10-24 | minor | Right-clicking the status bar opens a menu of its statistics, as in Excel ("If you want to customize the status bar, right-click it, and then click the options that you want"): Average, Count and Sum are on by default, and Numerical Count, Minimum and Maximum are new and off by default. The choice is a preference (`view.statusBar`), kept in `prefs.json` |
| 2026-10-24 | minor | Ctrl+Shift+U (Control+Shift+U on a Mac) expands or collapses the formula bar, as in Excel ("Expand or collapse the formula bar. Ctrl+Shift+U"), on the grid and in the cell editor or formula bar while editing. On Windows and Linux it used to underline the selection, as Ctrl+U does. The command is `view.expandFormulaBar` (`{on?}`, toggles without it) |
| 2026-10-24 | minor | Right-clicking a PivotTable's value, subtotal or grand total cell offers Show Details in the cell menu, as in Excel ("Right-click a field in the values area of the PivotTable, and then click Show Details"): it lists the cell's source records on a new sheet, as double-clicking the cell does (`pivot.showDetails`). Labels and headers of a current report don't offer it; in a report that is out of date every cell offers it, and choosing it asks for a refresh |
| 2026-10-24 | minor | Sheet tabs: the handle at the horizontal scrollbar's left edge shows the resize cursor, and dragging it moves the boundary, as in Excel ("Hover the mouse pointer at the edge of the scrollbar until you see the double-headed arrow"). Dragging it right gives the tabs more of the bar, until the scrollbar is 100 px wide (the scrollbar stays 100 px wide after the window is narrowed); dragging it left gives the scrollbar more, down to the New sheet button. Room the tabs don't fill stays empty, with New sheet after the last tab. Each open workbook keeps its own share; a workbook starts at the default (tabs up to 60%) |
| 2026-10-24 | minor | Outline Settings (Data ▸ Outline ▸ Settings, `data.outlineSettings`): clearing Summary rows below detail or Summary columns to right of detail puts the − / + buttons on the line above or left of each group, and Hide Detail, Show Detail and the level buttons mark that line collapsed. Changing a direction moves the marks of collapsed groups. Read and written as `outlinePr summaryBelow` / `summaryRight` |
| 2026-10-23 | minor | F3, Formulas › Use in Formula › Paste Names… and Alt+M, S, P open the Paste Name dialog (F3 only when the workbook has names). It lists the workbook's names and the active sheet's own in alphabetical order, hidden names left out; OK or a double-click puts the selected name at the caret of the formula being edited, in place of selected text, or starts `=Name` in the active cell (`formulas.pasteName` `{name}`), and Paste List writes each name and the text of what it refers to in two columns from the active cell (`{list: true}`). While a dialog is open over the edit, the cell editor takes no keys, and Cancel leaves the selected text selected. The Use in Formula menu lists the same names and inserts at the caret too (`formulas.useInFormula`); it used to list hidden names and other sheets' names, the menu added the name at the end of the text, and the command replaced the text being edited |
| 2026-10-23 | minor | Conditional Formatting ▸ New Rule, formula: clicking a cell on the grid writes its absolute reference at the formula box's caret (`=$B$2`, an empty box starts with `=`), a drag writes a range (`$B$2:$C$4`) and a second click replaces the reference just written, as in Excel ("Selecting cells on the worksheet inserts absolute cell references"). The selection the rule applies to doesn't move, and the box keeps the keyboard. After an operand (`=A1>5`) the click does nothing (#2348) |
| 2026-10-23 | minor | Frozen panes: Page Up, Ctrl+Up and Shift variants that move from the scrolled rows into the frozen ones bring the scrolled rows back to the top, so the rows in between are seen again (Page Up from far down used to end on row 1 with the view still a page down); Ctrl+Left and Alt+Page Up do the same for frozen columns. Other ways of selecting a frozen cell (click, Find, Go To, Undo, formula pointing) leave the view where it is |
| 2026-10-23 | minor | After a left-drag of the fill handle an Auto Fill Options button sits below the filled cells; its menu redoes the fill as Copy Cells, Fill Series, Fill Formatting Only or Fill Without Formatting (undoes the drag's fill, then `edit.autoFill` in that mode). It goes away when the workbook changes or the selection moves (#1403) |
| 2026-10-24 | minor | When the fill starts from a date, the right-drag fill menu and the Auto Fill Options button also offer Fill Days, Fill Weekdays (Saturday and Sunday skipped), Fill Months and Fill Years (`edit.autoFill` with `mode` set to `days`, `weekdays`, `months` or `years`). A lone date steps by one unit; two or more dates step by the difference between the first two in that unit, or one unit when they fall in the same one. Fill Months and Fill Years keep the day of the month, clamped to the month's last day. Every fill by months or years (these two, a drag of dates a month apart, and Series by Month or Year) keeps the time of day of a date with a time, which used to be dropped |
| 2026-10-22 | minor | Show Outline Symbols (`view.showOutlineSymbols`, Ctrl+8): a sheet can hide the outline's level buttons and brackets while keeping its outline, as Excel saves with `showOutlineSymbols="0"` (read and written); the grid then gives the bands no room |
| 2026-10-22 | minor | A number in a repeat-fill format (`*`, as in the Accounting format) that is too wide for its column shows #### like any other number, instead of the fill rebuilding the value so its leading digits were cut off (2,509.00 CHF drawn as "509.00 CHF") |
| 2026-10-22 | minor | View › New Window and Arrange All on the View ribbon; Arrange All tiles, stacks or cascades the native windows; two windows on one workbook keep their own sheet without copying the workbook; a dialog opened in one window stays there when a command run from it switches windows |
| 2026-10-22 | minor | One native window per workbook window on the desktop: per-window drafts, dialogs and ribbons; menu bar commands go to the focused window; closing one of several windows on a workbook doesn't ask; Quit asks in each unsaved workbook's window; the last window closed quits (Windows, Linux) or leaves macOS running |
| 2026-10-22 | minor | New Window opens a second window on the same workbook instead of a copy that saved over the original; windows keep their own selection, sheet and scroll; titles Name:1 / Name:2; Switch Windows, Hide and Unhide work by window; File › Close closes one window of several without asking |
| 2026-10-22 | minor | New workbook no longer loses the first one: the cell being typed is entered into its own workbook before any File or window command (an entry the workbook refuses stays open and the command doesn't run; an untouched F2 draft changes nothing), the editor is tied to its workbook, View › Switch Windows and the macOS Window menu list the open workbooks, and the macOS Quit item asks about unsaved workbooks like the close button |
| 2026-10-21 | minor | Background error checking, first part (#1994): a constant whose text would be a number if typed (signs, thousands, currency, percent, exponent, parenthesised negative) gets Excel's green corner triangle; the active cell then shows a warning button whose menu names the problem and offers Convert to Number for the selection (`formulas.convertToNumber`). Dates, times and fractions kept as text and formula results are not flagged, as in Excel |
| 2026-10-21 | minor | Paste Special laid out as Excel's: Paste and Operation options each in two columns inside a frame, Skip blanks and Transpose side by side, Paste Link as a button that pastes references at once; labels translated (the generic form didn't translate them) and the window no longer runs past the screen |
| 2026-10-21 | minor | Form dialogs (conditional-format quick rules, New Name, Remove Duplicates, Text to Columns and every other `Dialog::form`) fit their content: OK and Cancel sit in a row under the last field instead of a right-to-left layout that took the screen's whole height and left the buttons floating halfway down |
| 2026-10-20 | minor | Esc and Enter close a message over the cell editor, such as a data validation alert, and the entry stays open to fix, with the keyboard back in it. Before, the editor took them (the first Esc cancelled the edit behind the alert and only the second closed it), and after the message typing went nowhere until the cell was clicked |
| 2026-10-19 | minor | Ctrl+Enter (`range.fill`) gives each cell what typing the input there gives: a General cell gets the automatic number format (`5%`, `$5`, `3/4/2026`, `=TODAY()`) even when it already held a value, and a cell with its own format, a formatted row or column, or Text keeps it. Cells that held a value used to keep General (`5%` showed 0.05), and empty cells took the active cell's format |
| 2026-10-19 | trivial | While a mouse button is held (a click or a drag selecting cells) the selection outline is drawn on its cells at once instead of gliding after the pointer, which read as lag; keyboard moves still glide |
| 2026-10-18 | minor | Keytip letters typed with left Alt held continue the sequence (Alt+H, O, A / Alt+H, O, I ran nothing before: Alt+letter cancelled keytips); other Alt combinations, AltGr and macOS Option are unchanged (#1846) |
| 2026-10-17 | minor | A Ctrl+Alt+letter key that types a character (a user-defined Windows hotkey such as Ctrl+Alt+A for `ä`) types it instead of running the Ctrl+letter shortcut, e.g. Select All (#1530) |
| 2026-10-16 | minor | Right-drag on the fill handle opens the fill menu on release: Copy Cells, Fill Series, Fill Formatting Only, Fill Without Formatting, Series…; Esc cancels the drag or closes the menu without filling |
| 2026-10-16 | minor | With the active cell in a Table whose header row is scrolled out of view, the column headings show the header names and their filter buttons in place of the letters (#1423) |
| 2026-10-16 | minor | Ctrl+6 hides every object (charts, pictures, shapes, slicers) and shows them again; hidden objects are not drawn, take no clicks and drop their selection, and the setting is saved with the workbook (`workbookPr showObjects`) |
| 2026-10-16 | minor | Ctrl+Shift+A (Control+Shift+A on a Mac) after a function name, or after its `(`, inserts the argument names and the closing parenthesis, as in Excel; one Undo in the editor takes them back |
| 2026-10-16 | minor | Advanced Filter ribbon dialog, optional criteria for unique-only extraction (#1622); safe copy-to output (#1031) |
| 2026-10-16 | minor | Remove Duplicates shows an editable Range field initialized from the selection and passes it to the engine on OK (#1388). Live range-picking outside the dialog remains unsupported |
| 2026-10-16 | minor | Web printing registers one-shot `afterprint` cleanup before opening the dialog; missing content windows and setup, listener or print failures also release the hidden iframe and PDF blob URL (#1054) |
| 2026-10-16 | minor | The shared UI clock delegates to the core Unix-millisecond clock, which reads Date.now() on wasm, instead of returning zero there (#1026). Native clock contract tested; browser timing remains unverified |
| 2026-10-16 | trivial | Web: preferences (theme, interface size, language, editing options…) are kept in the browser's `localStorage` between visits, as the desktop app keeps `ui.json` and `prefs.json`; before, a reload brought back light mode and the defaults (#1562) |
| 2026-10-15 | minor | File › Options › Formulas: Maximum Iterations (1 to 32767) and Maximum Change (0 or more) under Enable iterative calculation, disabled while it is off; invalid entries are refused with a message |
| 2026-10-15 | minor | Number-format named colors use their RGB values; indexed `[Color1]`–`[Color56]` resolve through the shared legacy palette in grid and PDF (#1452) |
| 2026-10-14 | minor | Spill-child formula bar uses the anchor formula with disabled theme text and blocks formula-bar edit entry; selection and recalculation restore ordinary behavior |
| 2026-10-14 | minor | PivotTable source-field list fills the pane width so its scrollbar stays at the list's right edge |
| 2026-10-14 | minor | Alt, H, B border keytips apply the preset each letter names in Excel's Borders menu (they all drew a bottom border, so H,B,N No Border added one): keytips.json entries take optional `params`; H,B,M opens Format Cells on the Border tab; Erase Border, Line Color and Line Style stay reserved |
| 2026-10-13 | minor | Dragging a sheet tab with Ctrl held (Cmd or Option on macOS) drops a copy of the sheet where the tab is released, with the copy cursor (#971) |
| 2026-10-13 | trivial | More Colors… and Recent Colors (#928) |
| 2026-10-13 | minor | Grid drags keep their own cursor over selection borders, fill handles and outside the grid; hover resumes on release (#936) |
| 2026-10-13 | minor | AutoFill month patterns retain the workbook's date system, preventing shifted dates in 1904 workbooks when filling forward or backward |
| 2026-10-13 | minor | Sheet tab bar: the tab strip shrinks to its tabs, up to 60% of the bar, so New sheet (+) sits right after the last tab and the horizontal scrollbar takes the rest (part of #943) |
| 2026-10-13 | minor | Horizontal scrollbar in the sheet tab bar: drag, track click pages, frozen columns excluded from the page (#840) |
| 2026-10-13 | minor | Formula-entry Command/Ctrl-click adds arguments and Shift-click extends references; ordinary replacement and dragging retain their behavior. Readiness estimates unchanged. |
| 2026-10-12 | minor | Next/previous sheet navigation no longer activates hidden worksheets at either workbook boundary; command tests cover Hidden, VeryHidden and selection retention |
| 2026-10-12 | minor | The File view's recent list tells when each workbook was last changed, read off the UI thread |
| 2026-10-12 | minor | Ctrl+wheel zoom steps along a ladder of percentages about 20% apart that never skips 100%, gliding to each step with animations on, so a notch out undoes a notch in (it drifted: 100 → 122 → 101); Excel steps 15 points, see decisions.md |
| 2026-10-12 | minor | Page Layout view: left/right/top/bottom margins are dragged on the rulers (#792) |
| 2026-10-12 | minor | Alt+Down opens the active filter header's existing popup, with validation priority and mirrored anchoring on right-to-left sheets (part of #441) |
| 2026-10-12 | minor | Paste from the ribbon (button and options menu), the cell and header context menus and the macOS menu bar's Edit menu pastes what is on the system clipboard, as Cmd/Ctrl+V does, instead of only GridCraft's own copy (the web build, which can't read the clipboard on request, still pastes GridCraft's copy). Copy and Cut by keytip (H,C / H,X) publish to the system clipboard like the ribbon buttons. After a control-channel or MCP copy, which leaves the system clipboard alone, Paste pastes that copy until other text is copied. Once a cut has been pasted, Paste (ribbon, menus or Cmd/Ctrl+V) doesn't paste the cut's clipboard text again over the moved cells (Excel pastes a cut once), and the Paste keytip (H,V,P) pastes the system clipboard like the button. Regression test for the Paste button click freeze in 0.5.0 (#273, fixed by #272); the Paste button gets an accessible "Paste" name for screen readers |
| 2026-10-11 | minor | Ribbon: scroll arrows on a tab wider than the window, Formulas › AutoSum as a split button with its function menu, View › Show in two columns so Headings is no longer cut off |
| 2026-10-11 | minor | Theme tokens for icon, reference, on-accent and sheet-control colours; WCAG 2 contrast test over both themes (dark danger red and selected header text adjusted) |
| 2026-10-11 | minor | AutoFit accounts for table and ordinary filter dropdowns, including low zoom; header text excludes the button area |
| 2026-10-11 | minor | Outline band for grouped rows and columns, Show / Hide Detail on the Data tab, numpad navigation keys in shortcuts (#373) |
| 2026-10-11 | minor | Shift+Space / Ctrl+Space no longer start an edit; Control+Space selects the column on Mac (#338) |
| 2026-10-11 | minor | Selected whole columns can be reordered by dragging their headers to an insertion point |
| 2026-10-11 | minor | Page Layout view with rulers and in-place headers and footers (#329) |
| 2026-10-11 | minor | Forecast Sheet dialog (#322) |
| 2026-10-11 | minor | Delete and Backspace now remove the selected chart, picture or shape (#213) |
| 2026-10-10 | minor | Keep formula helpers inside the worksheet when scrolling (#200) |
| 2026-10-11 | trivial | Theme: title-bar light/dark switch, File › Display theme, system appearance on first run (#194, #239) |
| 2026-10-11 | minor | Keyboard shortcut audit against Microsoft's Windows and Mac lists; missing shortcuts for existing features added (#110) |
| 2026-10-11 | minor | File tab Backstage view and File › Options (#174) |
| 2026-10-11 | minor | Selection outline glide and the Animations setting |
| 2026-10-11 | major | Sequential Alt keytips: full-tab table (364 sequences), badges on every control, dropdowns open for typing/arrow editing, colour-palette arrows, real-Excel corrections (issues #33, #49) |
| 2026-10-10 | minor | Chart resize bug (#190) |
| 2026-10-10 | major | Created: chrome, grid, keyboard, dialogs, accessibility; key tips, Page Layout view and accessibility tree flagged |
